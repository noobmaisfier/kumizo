//! Ergonomic event-driven data sources.
//!
//! A [`Service`](crate::Service) is the right tool for a long-lived, shared,
//! type-keyed subsystem (battery, audio, ...). Most personal widgets only need
//! *one value from one place*: the temperature in a sysfs file, the last line of
//! a script, a message pushed over a socket. Writing a full `Service` for that
//! is a lot of ceremony, so this module provides the small building blocks:
//!
//! | Constructor | Source of events |
//! |---|---|
//! | [`ProcessSource`] / [`Source::command_lines`] | each line a child process prints on stdout |
//! | [`FileSource`] / [`Source::watch_file`] | the contents of a file, via `inotify` (optionally also polled) |
//! | [`Source::listen_unix`] | each line written to a UNIX socket we listen on |
//! | [`Source::from_thread`] | anything else: you own the thread, we own the plumbing |
//!
//! Every constructor returns an `Entity<Source<T>>`: a plain GPUI entity that
//! holds the latest `T`. There are **no UI components** here, the layout stays
//! entirely yours:
//!
//! ```ignore
//! struct Temp { src: Entity<Source<i32>> }
//!
//! impl Temp {
//!     fn new(cx: &mut Context<Self>) -> Self {
//!         let src = Source::watch_file(cx, "/tmp/temp", |s| s.trim().parse().ok());
//!         Self { src: src.track(cx) } // re-render this view on every update
//!     }
//! }
//!
//! impl Render for Temp {
//!     fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
//!         let t = self.src.read(cx).value().copied().unwrap_or(0);
//!         div().child(format!("{t} C"))
//!     }
//! }
//! ```
//!
//! ## Guarantees
//!
//! * **Zero idle CPU.** Every thread sleeps in `poll(2)`; nothing wakes up
//!   unless the kernel has an event (the only exception is an explicit
//!   [`FileSource::poll_every`], for files such as `/proc` and `/sys` that never
//!   raise `inotify` events).
//! * **Parsing happens off the UI thread.** Your `parse` closure runs on the
//!   source's own thread and returns `Option<T>`; `None` skips the line.
//! * **Cleanup is tied to the entity.** Dropping the entity wakes the thread,
//!   kills a child process and removes a socket file. Nothing leaks.
//! * **Back-pressure.** The hand-off to the UI is a bounded queue: a producer
//!   that outruns the UI (`yes`) blocks instead of eating memory.

use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use gpui::{App, AppContext, Context, Entity};
use tracing::{debug, warn};

/// A line parser shared by every connection thread of a socket source.
type SharedParse<T> = Arc<dyn Fn(&str) -> Option<T> + Send + Sync>;

/// Messages queued between a source thread and the UI.
const CHANNEL_CAPACITY: usize = 256;
/// A line longer than this without a newline is flushed as if it had one.
const MAX_LINE_BYTES: usize = 1 << 20;
/// Lower bound on a restart delay, so a crashing command cannot spin.
const MIN_RESTART: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------------
// Public state type
// ---------------------------------------------------------------------------

/// Lifecycle of a [`Source`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceState {
    /// The thread is alive (or about to deliver its next value).
    Running,
    /// The producer finished normally (process exited, no restart configured).
    Ended,
    /// The producer reported an error (command not found, socket in use...).
    /// A later successful value flips the state back to [`SourceState::Running`].
    Failed(String),
}

enum Msg<T> {
    Value(T),
    Failed(String),
    Ended,
}

/// The latest value pushed by a background producer.
///
/// Create one with a constructor ([`Source::watch_file`], [`ProcessSource`],
/// [`Source::listen_unix`], [`Source::from_thread`]), read it from `render`
/// with `entity.read(cx)`, and call [`TrackExt::track`] so the view re-renders
/// when a new value arrives.
pub struct Source<T: 'static> {
    value: Option<T>,
    state: SourceState,
    updates: u64,
    /// Dropped with the entity: wakes the thread and kills any child process.
    _stop: StopGuard,
}

impl<T: 'static> Source<T> {
    /// The most recent value, or `None` until the first one arrives.
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Current lifecycle state.
    pub fn state(&self) -> &SourceState {
        &self.state
    }

    /// `true` unless the producer ended or failed.
    pub fn is_running(&self) -> bool {
        self.state == SourceState::Running
    }

    /// The error message when the state is [`SourceState::Failed`].
    pub fn error(&self) -> Option<&str> {
        match &self.state {
            SourceState::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// Number of values received so far. Handy to detect "something happened"
    /// even when the new value equals the previous one.
    pub fn updates(&self) -> u64 {
        self.updates
    }

    fn apply(&mut self, msg: Msg<T>) {
        match msg {
            Msg::Value(value) => {
                self.value = Some(value);
                self.updates += 1;
                self.state = SourceState::Running;
            }
            Msg::Failed(message) => self.state = SourceState::Failed(message),
            // A failure is more informative than "ended", keep it.
            Msg::Ended => {
                if !matches!(self.state, SourceState::Failed(_)) {
                    self.state = SourceState::Ended;
                }
            }
        }
    }
}

impl<T: Send + 'static> Source<T> {
    /// The primitive every other constructor is built on.
    ///
    /// `run` is executed on a dedicated thread. Call [`Emitter::emit`] for each
    /// value; return when you are done. Block on file descriptors with
    /// [`StopToken::wait`]-aware `poll(2)` (or any blocking call that returns
    /// once the [`StopToken`] fires) so the thread exits promptly when the
    /// entity is dropped. `emit` returns `false` once nobody listens anymore.
    pub fn from_thread<F>(cx: &mut App, run: F) -> Entity<Source<T>>
    where
        F: FnOnce(Emitter<T>, StopToken) + Send + 'static,
    {
        let (emitter, rx) = emitter_pair::<T>();
        let (guard, token) = stop_pair();
        let end_emitter = emitter.clone();

        std::thread::Builder::new()
            .name("wk-source".into())
            .spawn(move || {
                run(emitter, token);
                let _ = end_emitter.tx.send_blocking(Msg::Ended);
            })
            .expect("failed to spawn data source thread");

        cx.new(|cx| {
            cx.spawn(async move |this: gpui::WeakEntity<Source<T>>, cx: &mut gpui::AsyncApp| {
                while let Ok(msg) = rx.recv().await {
                    let alive = this
                        .update(cx, |source, cx| {
                            source.apply(msg);
                            cx.notify();
                        })
                        .is_ok();
                    if !alive {
                        // Entity gone: dropping `rx` unblocks a producer stuck
                        // on a full queue.
                        break;
                    }
                }
            })
            .detach();

            Source {
                value: None,
                state: SourceState::Running,
                updates: 0,
                _stop: guard,
            }
        })
    }

    /// Run `program args...`, turning each stdout line into a value.
    ///
    /// Shorthand for `ProcessSource::new(program).args(args).lines(cx, parse)`.
    /// Use [`ProcessSource`] directly for restarts or environment variables.
    pub fn command_lines<I, S>(
        cx: &mut App,
        program: impl Into<OsString>,
        args: I,
        parse: impl Fn(&str) -> Option<T> + Send + 'static,
    ) -> Entity<Source<T>>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        ProcessSource::new(program).args(args).lines(cx, parse)
    }

    /// Watch a file; every change is re-read and passed to `parse`.
    ///
    /// Shorthand for `FileSource::new(path).watch(cx, parse)`. Use
    /// [`FileSource`] directly to also poll (`/proc`, `/sys`).
    pub fn watch_file(
        cx: &mut App,
        path: impl Into<PathBuf>,
        parse: impl Fn(&str) -> Option<T> + Send + 'static,
    ) -> Entity<Source<T>> {
        FileSource::new(path).watch(cx, parse)
    }

    /// Listen on a UNIX socket; every line a client writes becomes a value.
    ///
    /// Any number of clients may connect (`echo hi | socat - UNIX-CONNECT:path`).
    /// A stale socket file is replaced; a socket another process is serving is
    /// reported as [`SourceState::Failed`]. The file is created with mode
    /// `0600` and removed when the entity is dropped.
    pub fn listen_unix(
        cx: &mut App,
        path: impl Into<PathBuf>,
        parse: impl Fn(&str) -> Option<T> + Send + Sync + 'static,
    ) -> Entity<Source<T>> {
        let path = path.into();
        let parse: SharedParse<T> = Arc::new(parse);
        Source::from_thread(cx, move |emit, stop| run_unix(&path, &stop, &emit, parse))
    }
}

// ---------------------------------------------------------------------------
// View integration
// ---------------------------------------------------------------------------

/// Re-render a view whenever an entity (typically a [`Source`]) notifies.
///
/// Services get this for free through `cx.service::<S>()`; sources are plain
/// entities, so opt in once:
///
/// ```ignore
/// let src = Source::watch_file(cx, path, parse).track(cx);
/// ```
pub trait TrackExt<S: 'static> {
    /// Observe the entity from `cx` and `cx.notify()` on every change.
    /// Returns the entity so the call chains.
    fn track<V: 'static>(self, cx: &mut Context<V>) -> Entity<S>;
}

impl<S: 'static> TrackExt<S> for Entity<S> {
    fn track<V: 'static>(self, cx: &mut Context<V>) -> Entity<S> {
        cx.observe(&self, |_this, _entity, cx| cx.notify()).detach();
        self
    }
}

// ---------------------------------------------------------------------------
// Emitter and stop plumbing
// ---------------------------------------------------------------------------

/// Handle a source thread uses to deliver values to the UI.
pub struct Emitter<T> {
    tx: Sender<Msg<T>>,
}

impl<T> Clone for Emitter<T> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

impl<T> Emitter<T> {
    /// Deliver a value. Blocks while the UI queue is full. Returns `false`
    /// when the entity is gone and the thread should stop.
    pub fn emit(&self, value: T) -> bool {
        self.tx.send_blocking(Msg::Value(value)).is_ok()
    }

    /// Report an error without ending the source (the state becomes
    /// [`SourceState::Failed`] until the next value).
    pub fn fail(&self, message: impl Into<String>) -> bool {
        self.tx.send_blocking(Msg::Failed(message.into())).is_ok()
    }

    /// `true` once the entity was dropped.
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

fn emitter_pair<T>() -> (Emitter<T>, Receiver<Msg<T>>) {
    let (tx, rx) = async_channel::bounded(CHANNEL_CAPACITY);
    (Emitter { tx }, rx)
}

/// Held by the [`Source`] entity. Dropping it fires the paired [`StopToken`].
struct StopGuard {
    /// Closing the write end makes the read end report `POLLHUP`.
    _write: OwnedFd,
    child: Arc<AtomicI32>,
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        let pid = self.child.swap(0, Ordering::SeqCst);
        if pid > 0 {
            // SAFETY: plain signal delivery to a pid we spawned and have not
            // yet reaped (the runner zeroes the slot right after `wait`).
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
    }
}

/// Given to a source thread: fires when the owning entity is dropped.
///
/// Include [`StopToken::as_raw_fd`] in your own `poll(2)` set (it becomes
/// readable / `POLLHUP` on stop), or use [`StopToken::wait`] as an
/// interruptible sleep.
pub struct StopToken {
    fd: OwnedFd,
    child: Arc<AtomicI32>,
}

impl StopToken {
    /// The raw descriptor to add to a `poll(2)` set.
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// `true` once the entity was dropped.
    pub fn is_stopped(&self) -> bool {
        self.wait(Some(Duration::ZERO))
    }

    /// Sleep up to `timeout` (forever for `None`), returning early with `true`
    /// when the stop fires. `false` means the timeout elapsed.
    pub fn wait(&self, timeout: Option<Duration>) -> bool {
        let mut fds = [libc::pollfd {
            fd: self.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        match poll(&mut fds, timeout_ms(timeout)) {
            Ok(_) => fds[0].revents != 0,
            // An unusable stop fd must not leave the thread running forever.
            Err(_) => true,
        }
    }

    /// A second handle for another thread of the same source.
    pub fn try_clone(&self) -> io::Result<StopToken> {
        Ok(StopToken {
            fd: self.fd.try_clone()?,
            child: self.child.clone(),
        })
    }

    /// Ask for `pid` to be sent `SIGTERM` when the source is dropped. Pass `0`
    /// once the child has been reaped, to avoid signalling a recycled pid.
    pub fn kill_on_stop(&self, pid: i32) {
        self.child.store(pid, Ordering::SeqCst);
    }
}

fn stop_pair() -> (StopGuard, StopToken) {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a valid 2-element array for pipe2 to fill.
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    assert!(rc == 0, "pipe2 failed: {}", io::Error::last_os_error());
    // SAFETY: both descriptors were just created and are owned exclusively here.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    let child = Arc::new(AtomicI32::new(0));
    (
        StopGuard { _write: write, child: child.clone() },
        StopToken { fd: read, child },
    )
}

// ---------------------------------------------------------------------------
// poll(2) helpers
// ---------------------------------------------------------------------------

fn timeout_ms(timeout: Option<Duration>) -> libc::c_int {
    match timeout {
        None => -1,
        Some(d) if d.is_zero() => 0,
        // Round up: a sub-millisecond timeout must not become a busy loop.
        Some(d) => d.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as libc::c_int,
    }
}

fn poll(fds: &mut [libc::pollfd], timeout_ms: libc::c_int) -> io::Result<usize> {
    loop {
        // SAFETY: `fds` is a valid mutable slice of pollfd for the whole call.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if rc >= 0 {
            return Ok(rc as usize);
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

enum Wake {
    Ready,
    Timeout,
    Stopped,
}

/// Block until `fd` is readable, the timeout elapses, or the source is stopped.
fn wait_ready(fd: RawFd, stop: &StopToken, timeout: Option<Duration>) -> io::Result<Wake> {
    let mut fds = [
        libc::pollfd { fd, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    let ready = poll(&mut fds, timeout_ms(timeout))?;
    Ok(if fds[1].revents != 0 {
        Wake::Stopped
    } else if ready == 0 {
        Wake::Timeout
    } else {
        Wake::Ready
    })
}

/// Read `fd` until EOF or stop, handing each line (without `\n` / `\r\n`) to
/// `on_line`. A final unterminated line is delivered at EOF. `on_line` returning
/// `false` ends the loop.
fn pump_lines(fd: RawFd, stop: &StopToken, mut on_line: impl FnMut(&str) -> bool) -> io::Result<()> {
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];

    let mut deliver = |bytes: &[u8]| -> bool {
        let decoded = String::from_utf8_lossy(bytes);
        let line = decoded.strip_suffix('\r').unwrap_or(&decoded);
        on_line(line)
    };

    loop {
        if let Wake::Stopped = wait_ready(fd, stop, None)? {
            return Ok(());
        }
        // SAFETY: `buf` is valid for `buf.len()` bytes; `fd` is open for the call.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            match err.kind() {
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => continue,
                _ => return Err(err),
            }
        }
        if n == 0 {
            if !pending.is_empty() {
                deliver(&pending);
            }
            return Ok(());
        }

        pending.extend_from_slice(&buf[..n as usize]);
        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=pos).collect();
            if !deliver(&line[..line.len() - 1]) {
                return Ok(());
            }
        }
        if pending.len() > MAX_LINE_BYTES {
            let line = std::mem::take(&mut pending);
            if !deliver(&line) {
                return Ok(());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Process source
// ---------------------------------------------------------------------------

/// Builder for a source fed by the stdout of a child process.
///
/// ```ignore
/// let vol = ProcessSource::new("pactl")
///     .arg("subscribe")
///     .restart_after(Duration::from_secs(2)) // pipewire restarted? reconnect
///     .lines(cx, |line| line.contains("sink").then(|| line.to_string()));
/// ```
///
/// stdin and stderr are `/dev/null`. The child is killed when the entity is
/// dropped, and also if the shell itself dies (`PR_SET_PDEATHSIG`).
#[derive(Debug, Clone)]
pub struct ProcessSource {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
    restart: Option<Duration>,
}

impl ProcessSource {
    /// Start describing `program` (looked up in `PATH`).
    pub fn new(program: impl Into<OsString>) -> Self {
        Self { program: program.into(), args: Vec::new(), envs: Vec::new(), restart: None }
    }

    /// Append one argument.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set an environment variable for the child.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }

    /// Relaunch the command `delay` after it exits or fails to start (at least
    /// 50 ms). Without this, the source ends when the process does.
    pub fn restart_after(mut self, delay: Duration) -> Self {
        self.restart = Some(delay.max(MIN_RESTART));
        self
    }

    /// Spawn the command and turn each stdout line into a value with `parse`
    /// (`None` skips the line). `parse` runs on the source thread.
    pub fn lines<T: Send + 'static>(
        self,
        cx: &mut App,
        parse: impl Fn(&str) -> Option<T> + Send + 'static,
    ) -> Entity<Source<T>> {
        Source::from_thread(cx, move |emit, stop| run_process(&self, &stop, &emit, &parse))
    }
}

fn run_process<T>(
    spec: &ProcessSource,
    stop: &StopToken,
    emit: &Emitter<T>,
    parse: &dyn Fn(&str) -> Option<T>,
) {
    loop {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .envs(spec.envs.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: only async-signal-safe `prctl` runs between fork and exec.
        // The child gets SIGTERM if the shell dies, so no orphan keeps running.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM as libc::c_ulong);
                Ok(())
            });
        }

        match cmd.spawn() {
            Err(err) => {
                let message = format!("failed to start `{}`: {err}", spec.program.to_string_lossy());
                warn!("data source: {message}");
                if !emit.fail(message) {
                    return;
                }
            }
            Ok(mut child) => {
                stop.kill_on_stop(child.id() as i32);
                if let Some(stdout) = child.stdout.take() {
                    let result = pump_lines(stdout.as_raw_fd(), stop, |line| match parse(line) {
                        Some(value) => emit.emit(value),
                        None => true,
                    });
                    if let Err(err) = result {
                        debug!("data source: read error: {err}");
                    }
                }
                // Reap before clearing the pid slot so a recycled pid is never signalled.
                let _ = child.kill();
                let _ = child.wait();
                stop.kill_on_stop(0);
            }
        }

        if emit.is_closed() || stop.is_stopped() {
            return;
        }
        match spec.restart {
            None => return,
            Some(delay) => {
                if stop.wait(Some(delay)) {
                    return;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File source
// ---------------------------------------------------------------------------

/// Builder for a source fed by the contents of a file.
///
/// The file is read once at start, then again whenever `inotify` reports that
/// a writer *finished* (closed the file, or renamed one over it). Reading only
/// at that point means a half-written or just-truncated file is never seen.
/// The *parent directory* is watched, so scripts that replace the file
/// atomically (write a temp file, `rename` over it) are seen too, and the file
/// may not exist yet at start. Identical consecutive contents are not
/// re-parsed.
///
/// Two cases raise no such event; add [`poll_every`](FileSource::poll_every):
/// * `/proc` and `/sys` files (the kernel never notifies);
/// * a process that keeps the file open and writes without ever closing it.
#[derive(Debug, Clone)]
pub struct FileSource {
    path: PathBuf,
    poll: Option<Duration>,
}

impl FileSource {
    /// Watch `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), poll: None }
    }

    /// Also re-read every `interval` (at least 50 ms). This is the one place a
    /// source wakes without a kernel event, so reserve it for `/proc` / `/sys`.
    pub fn poll_every(mut self, interval: Duration) -> Self {
        self.poll = Some(interval.max(MIN_RESTART));
        self
    }

    /// Start watching; `parse` receives the whole file as text (`None` skips).
    /// It runs on the source thread.
    pub fn watch<T: Send + 'static>(
        self,
        cx: &mut App,
        parse: impl Fn(&str) -> Option<T> + Send + 'static,
    ) -> Entity<Source<T>> {
        Source::from_thread(cx, move |emit, stop| run_file(&self, &stop, &emit, &parse))
    }
}

/// A non-blocking inotify instance watching one directory.
struct Inotify {
    fd: OwnedFd,
}

impl Inotify {
    fn new(dir: &Path) -> io::Result<Self> {
        // SAFETY: plain syscall, no pointers.
        let raw = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` was just returned by the kernel and is owned here.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let cpath = CString::new(dir.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))?;
        // Only "the writer is done" events. IN_MODIFY / IN_CREATE fire on the
        // truncation that precedes every `echo > file`, when the file is
        // momentarily empty, which would flash an empty value in the UI.
        let mask = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO;
        // SAFETY: `cpath` is a valid NUL-terminated string for the call.
        let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), cpath.as_ptr(), mask) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    /// Drain pending events; `true` when one concerns `name` (or the queue overflowed).
    fn drain_relevant(&self, name: &[u8]) -> bool {
        let header = std::mem::size_of::<libc::inotify_event>();
        let mut buf = [0u8; 4096];
        let mut relevant = false;
        loop {
            // SAFETY: `buf` is valid for `buf.len()` bytes; the fd is open.
            let n = unsafe {
                libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len())
            };
            if n <= 0 {
                // EAGAIN (drained), EOF or an error: nothing more to read now.
                return relevant;
            }
            let n = n as usize;
            let mut off = 0;
            while off + header <= n {
                // SAFETY: the bounds check above guarantees `header` readable
                // bytes; `read_unaligned` copes with the byte buffer's alignment.
                let event =
                    unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off) as *const libc::inotify_event) };
                let name_start = off + header;
                let name_end = name_start + event.len as usize;
                if name_end > n {
                    break;
                }
                let raw = &buf[name_start..name_end];
                let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
                if event.mask & libc::IN_Q_OVERFLOW != 0 || &raw[..end] == name {
                    relevant = true;
                }
                off = name_end;
            }
        }
    }
}

fn run_file<T>(
    spec: &FileSource,
    stop: &StopToken,
    emit: &Emitter<T>,
    parse: &dyn Fn(&str) -> Option<T>,
) {
    let path = spec.path.as_path();
    let Some(name) = path.file_name() else {
        emit.fail(format!("`{}` has no file name", path.display()));
        return;
    };
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let inotify = match Inotify::new(dir) {
        Ok(i) => Some(i),
        Err(err) if spec.poll.is_some() => {
            debug!("data source: inotify unavailable for {} ({err}), polling only", dir.display());
            None
        }
        Err(err) => {
            emit.fail(format!("cannot watch `{}`: {err}", dir.display()));
            return;
        }
    };

    let mut last: Option<Vec<u8>> = None;
    // Returns `false` when the entity is gone.
    let refresh = |last: &mut Option<Vec<u8>>| -> bool {
        match std::fs::read(path) {
            Ok(bytes) => {
                if last.as_deref() != Some(bytes.as_slice()) {
                    let value = parse(&String::from_utf8_lossy(&bytes));
                    *last = Some(bytes);
                    if let Some(value) = value {
                        return emit.emit(value);
                    }
                }
                true
            }
            Err(_) => {
                // Missing or unreadable: keep the last value, and re-emit if it
                // comes back with identical contents.
                *last = None;
                true
            }
        }
    };

    if !refresh(&mut last) {
        return;
    }

    loop {
        let wake = match &inotify {
            Some(i) => match wait_ready(i.fd.as_raw_fd(), stop, spec.poll) {
                Ok(wake) => wake,
                Err(err) => {
                    emit.fail(format!("poll failed: {err}"));
                    return;
                }
            },
            None => {
                if stop.wait(spec.poll) {
                    Wake::Stopped
                } else {
                    Wake::Timeout
                }
            }
        };
        match wake {
            Wake::Stopped => return,
            Wake::Timeout => {
                if !refresh(&mut last) {
                    return;
                }
            }
            Wake::Ready => {
                let touched = inotify.as_ref().is_some_and(|i| i.drain_relevant(name.as_bytes()));
                if touched && !refresh(&mut last) {
                    return;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// UNIX socket source
// ---------------------------------------------------------------------------

fn run_unix<T: Send + 'static>(
    path: &Path,
    stop: &StopToken,
    emit: &Emitter<T>,
    parse: SharedParse<T>,
) {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            emit.fail(format!("socket `{}` is already served by another process", path.display()));
            return;
        }
        // Nobody answers: a leftover from a crash.
        let _ = std::fs::remove_file(path);
    }

    let listener = match UnixListener::bind(path) {
        Ok(l) => l,
        Err(err) => {
            emit.fail(format!("cannot listen on `{}`: {err}", path.display()));
            return;
        }
    };
    // Only the owner may push data into the shell.
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    if let Err(err) = listener.set_nonblocking(true) {
        emit.fail(format!("cannot configure `{}`: {err}", path.display()));
        let _ = std::fs::remove_file(path);
        return;
    }

    'serve: loop {
        match wait_ready(listener.as_raw_fd(), stop, None) {
            Ok(Wake::Ready) => {}
            Ok(Wake::Timeout) => continue,
            Ok(Wake::Stopped) | Err(_) => break,
        }
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_err() {
                        continue;
                    }
                    let (Ok(stop), emit, parse) = (stop.try_clone(), emit.clone(), parse.clone()) else {
                        continue;
                    };
                    let spawned = std::thread::Builder::new().name("wk-source-conn".into()).spawn(move || {
                        let _ = pump_lines(stream.as_raw_fd(), &stop, |line| match parse(line) {
                            Some(value) => emit.emit(value),
                            None => true,
                        });
                    });
                    if let Err(err) = spawned {
                        warn!("data source: cannot spawn connection thread: {err}");
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    emit.fail(format!("accept failed: {err}"));
                    break 'serve;
                }
            }
        }
        if emit.is_closed() {
            break;
        }
    }

    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// Tests: the runners are plain threads + channels, so no GPUI is needed.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const TIMEOUT: Duration = Duration::from_secs(5);

    /// Collect `n` values (ignoring failures/ends) or panic on timeout.
    fn collect<T>(rx: &Receiver<Msg<T>>, n: usize) -> Vec<T> {
        let deadline = Instant::now() + TIMEOUT;
        let mut out = Vec::new();
        while out.len() < n {
            match rx.try_recv() {
                Ok(Msg::Value(v)) => out.push(v),
                Ok(_) => {}
                Err(_) => {
                    assert!(Instant::now() < deadline, "timed out waiting for {n} values, got {}", out.len());
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        out
    }

    fn join_within(handle: std::thread::JoinHandle<()>, what: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while !handle.is_finished() {
            assert!(Instant::now() < deadline, "{what}: thread did not stop");
            std::thread::sleep(Duration::from_millis(5));
        }
        handle.join().unwrap();
    }

    fn unique(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wk-source-test-{}-{tag}", std::process::id()))
    }

    #[test]
    fn process_lines_handle_crlf_and_unterminated_tail() {
        let (emit, rx) = emitter_pair::<String>();
        let (_guard, stop) = stop_pair();
        let spec = ProcessSource::new("sh").args(["-c", r#"printf 'a\nb\r\nc'"#]);
        let t = std::thread::spawn(move || run_process(&spec, &stop, &emit, &|l| Some(l.to_string())));
        assert_eq!(collect(&rx, 3), ["a", "b", "c"]);
        join_within(t, "process");
    }

    #[test]
    fn process_parse_none_skips_lines() {
        let (emit, rx) = emitter_pair::<i32>();
        let (_guard, stop) = stop_pair();
        let spec = ProcessSource::new("sh").args(["-c", "echo 1; echo nope; echo 3"]);
        let t = std::thread::spawn(move || run_process(&spec, &stop, &emit, &|l| l.parse().ok()));
        assert_eq!(collect(&rx, 2), [1, 3]);
        join_within(t, "process");
    }

    #[test]
    fn dropping_the_guard_kills_a_long_running_child() {
        let (emit, rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let spec = ProcessSource::new("sh").args(["-c", "while true; do echo x; sleep 0.05; done"]);
        let t = std::thread::spawn(move || run_process(&spec, &stop, &emit, &|l| Some(l.to_string())));
        collect(&rx, 1);
        drop(guard);
        join_within(t, "process");
    }

    #[test]
    fn dropping_the_guard_stops_a_silent_child() {
        // No output at all: only the stop pipe can wake the reader.
        let (emit, _rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let spec = ProcessSource::new("sleep").arg("60");
        let t = std::thread::spawn(move || run_process(&spec, &stop, &emit, &|l| Some(l.to_string())));
        std::thread::sleep(Duration::from_millis(100));
        drop(guard);
        join_within(t, "silent process");
    }

    #[test]
    fn restart_relaunches_and_missing_binary_reports_failure() {
        let (emit, rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let spec = ProcessSource::new("sh")
            .args(["-c", "echo hi"])
            .restart_after(Duration::from_millis(60));
        let t = std::thread::spawn(move || run_process(&spec, &stop, &emit, &|l| Some(l.to_string())));
        assert_eq!(collect(&rx, 2), ["hi", "hi"]);
        drop(guard);
        join_within(t, "restart");

        let (emit, rx) = emitter_pair::<String>();
        let (_guard, stop) = stop_pair();
        let spec = ProcessSource::new("wk-definitely-not-a-command-xyz");
        run_process(&spec, &stop, &emit, &|l| Some(l.to_string()));
        match rx.try_recv() {
            Ok(Msg::Failed(m)) => assert!(m.contains("failed to start"), "{m}"),
            _ => panic!("expected a failure message"),
        }
    }

    #[test]
    fn file_changes_in_place_and_by_atomic_rename() {
        let path = unique("file");
        std::fs::write(&path, "1\n").unwrap();

        let (emit, rx) = emitter_pair::<i32>();
        let (guard, stop) = stop_pair();
        let spec = FileSource::new(&path);
        let t = std::thread::spawn(move || run_file(&spec, &stop, &emit, &|s| s.trim().parse().ok()));

        assert_eq!(collect(&rx, 1), [1]);

        std::fs::write(&path, "2\n").unwrap();
        assert_eq!(collect(&rx, 1), [2]);

        let tmp = unique("file.tmp");
        std::fs::write(&tmp, "3\n").unwrap();
        std::fs::rename(&tmp, &path).unwrap();
        assert_eq!(collect(&rx, 1), [3]);

        drop(guard);
        join_within(t, "file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn file_created_after_start_is_picked_up() {
        let path = unique("late");
        let _ = std::fs::remove_file(&path);

        let (emit, rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let spec = FileSource::new(&path);
        let t = std::thread::spawn(move || run_file(&spec, &stop, &emit, &|s| Some(s.to_string())));

        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(&path, "late").unwrap();
        assert_eq!(collect(&rx, 1), ["late"]);

        drop(guard);
        join_within(t, "late file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn identical_rewrites_are_deduplicated() {
        let path = unique("dedup");
        std::fs::write(&path, "same").unwrap();

        let (emit, rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let spec = FileSource::new(&path);
        let t = std::thread::spawn(move || run_file(&spec, &stop, &emit, &|s| Some(s.to_string())));

        assert_eq!(collect(&rx, 1), ["same"]);
        std::fs::write(&path, "same").unwrap();
        std::fs::write(&path, "different").unwrap();
        // The identical rewrite must not produce an extra value in between.
        assert_eq!(collect(&rx, 1), ["different"]);

        drop(guard);
        join_within(t, "dedup");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn polling_covers_files_without_inotify_events() {
        // A file in a directory that cannot be watched is the failure mode we
        // poll for; here we simply check poll-only operation works when asked.
        let path = unique("poll");
        std::fs::write(&path, "a").unwrap();

        let (emit, rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let spec = FileSource::new(&path).poll_every(Duration::from_millis(50));
        let t = std::thread::spawn(move || run_file(&spec, &stop, &emit, &|s| Some(s.to_string())));

        assert_eq!(collect(&rx, 1), ["a"]);
        std::fs::write(&path, "b").unwrap();
        assert_eq!(collect(&rx, 1), ["b"]);

        drop(guard);
        join_within(t, "poll");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unix_socket_receives_lines_from_several_clients_and_cleans_up() {
        use std::io::Write;
        let path = unique("sock");
        let _ = std::fs::remove_file(&path);

        let (emit, rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let p = path.clone();
        let parse: SharedParse<String> = Arc::new(|l| Some(l.to_string()));
        let t = std::thread::spawn(move || run_unix(&p, &stop, &emit, parse));

        let deadline = Instant::now() + TIMEOUT;
        let mut a = loop {
            match UnixStream::connect(&path) {
                Ok(s) => break s,
                Err(_) => {
                    assert!(Instant::now() < deadline, "socket never came up");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        let mut b = UnixStream::connect(&path).unwrap();
        a.write_all(b"one\ntwo\n").unwrap();
        b.write_all(b"three\n").unwrap();

        let mut got = collect(&rx, 3);
        got.sort();
        assert_eq!(got, ["one", "three", "two"]);

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        drop(guard);
        join_within(t, "socket");
        assert!(!path.exists(), "socket file must be removed on stop");
    }

    #[test]
    fn unix_socket_replaces_a_stale_file_but_refuses_a_live_one() {
        let path = unique("stale");
        let _ = std::fs::remove_file(&path);

        // A crash leaves the socket file behind with nobody listening.
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());

        let (emit, _rx) = emitter_pair::<String>();
        let (guard, stop) = stop_pair();
        let p = path.clone();
        let parse: SharedParse<String> = Arc::new(|l| Some(l.to_string()));
        let t = std::thread::spawn(move || run_unix(&p, &stop, &emit, parse));

        let deadline = Instant::now() + TIMEOUT;
        while UnixStream::connect(&path).is_err() {
            assert!(Instant::now() < deadline, "stale socket was not replaced");
            std::thread::sleep(Duration::from_millis(5));
        }

        // A second source on the same path must not steal it.
        let (emit2, rx2) = emitter_pair::<String>();
        let (_guard2, stop2) = stop_pair();
        let parse2: SharedParse<String> = Arc::new(|l| Some(l.to_string()));
        run_unix(&path, &stop2, &emit2, parse2);
        match rx2.try_recv() {
            Ok(Msg::Failed(m)) => assert!(m.contains("already served"), "{m}"),
            _ => panic!("expected an 'already served' failure"),
        }

        drop(guard);
        join_within(t, "stale socket");
    }

    #[test]
    fn timeout_rounds_up_so_short_waits_do_not_spin() {
        assert_eq!(timeout_ms(None), -1);
        assert_eq!(timeout_ms(Some(Duration::ZERO)), 0);
        assert_eq!(timeout_ms(Some(Duration::from_micros(1))), 1);
        assert_eq!(timeout_ms(Some(Duration::from_millis(250))), 250);
    }

    #[test]
    fn source_state_transitions() {
        let (guard, _stop) = stop_pair();
        let mut s = Source::<i32> { value: None, state: SourceState::Running, updates: 0, _stop: guard };
        assert!(s.is_running() && s.value().is_none());

        s.apply(Msg::Value(7));
        assert_eq!((s.value().copied(), s.updates()), (Some(7), 1));

        s.apply(Msg::Failed("boom".into()));
        assert_eq!(s.error(), Some("boom"));
        // Ending after a failure keeps the more useful message.
        s.apply(Msg::Ended);
        assert_eq!(s.error(), Some("boom"));

        // A later value recovers the source.
        s.apply(Msg::Value(8));
        assert!(s.is_running());
        s.apply(Msg::Ended);
        assert_eq!(*s.state(), SourceState::Ended);
    }
}
