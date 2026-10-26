//! Hyprland IPC (socket1 for commands, socket2 for events).
//!
//! This backend targets the protocol implemented by current Hyprland, including
//! the Lua config provider introduced in 0.55/0.56. It also keeps the legacy
//! `hyprlang` dispatcher syntax working on older releases:
//!
//! - commands are written **without** a trailing newline, then the write side
//!   is shut down (this is what `hyprctl` does);
//! - socket paths are looked up under `$XDG_RUNTIME_DIR/hypr`,
//!   `/run/user/<uid>/hypr` and `/tmp/hypr`;
//! - `workspacev2`, `activewindowv2`, `openwindow`, `closewindow`,
//!   `movewindowv2`, `urgent`, `monitor*`, `configreloaded`… are handled;
//! - state-changing events trigger an authoritative re-query instead of
//!   replaying deltas, so named/special workspaces cannot desynchronise;
//! - the event socket reconnects automatically (Hyprland restarts, GPU reset),
//!   and a full sync is emitted after every reconnect.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::warn;

use crate::compositor::types::{ActiveWindow, AppWindow, CompositorEvent, Monitor, Workspace};

/// How the running Hyprland expects dispatchers to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchStyle {
    /// Hyprland >= 0.55 with the Lua config provider:
    /// `dispatch hl.dsp.focus({ workspace = "2" })`.
    Lua,
    /// Older Hyprland / `hyprlang`: `dispatch workspace 2`.
    Legacy,
}

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_COALESCE: Duration = Duration::from_millis(15);
const RECONNECT_MIN: Duration = Duration::from_millis(500);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// Hyprland IPC client and event listener.
///
/// Cloning the handle is cheap: it only copies the instance paths and the
/// shared cache for the detected dispatcher style.
#[derive(Clone)]
pub struct HyprlandIpc {
    instance_sig: Arc<str>,
    instance_dir: PathBuf,
    style: Arc<OnceLock<DispatchStyle>>,
}

/// Reply of `j/status` (Hyprland internal state).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HyprlandStatus {
    /// `"lua"` on Hyprland >= 0.55, `"hyprlang"` on older releases.
    #[serde(rename = "configProvider", default)]
    pub config_provider: String,
    /// Rendering backend (`drm`, `wayland`, …).
    #[serde(default)]
    pub backend: String,
}

impl HyprlandIpc {
    /// Detect and create a new Hyprland IPC instance if running under Hyprland.
    ///
    /// Both `.socket.sock` and `.socket2.sock` must exist: the instance is
    /// considered usable only when commands and events are available.
    pub fn try_new() -> Option<Self> {
        let instance_sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .ok()
            .filter(|sig| !sig.is_empty())?;
        let instance_dir = find_instance_dir(&instance_sig)?;

        Some(Self {
            instance_sig: Arc::from(instance_sig),
            instance_dir,
            style: Arc::new(OnceLock::new()),
        })
    }

    /// Hyprland instance signature (`HYPRLAND_INSTANCE_SIGNATURE`).
    pub fn instance_signature(&self) -> &str {
        &self.instance_sig
    }

    /// Directory containing this Hyprland instance's sockets and lock file.
    pub fn instance_dir(&self) -> &std::path::Path {
        &self.instance_dir
    }

    /// Path to the command socket (`.socket.sock`).
    pub fn command_socket_path(&self) -> PathBuf {
        self.instance_dir.join(".socket.sock")
    }

    /// Path to the event socket (`.socket2.sock`).
    pub fn event_socket_path(&self) -> PathBuf {
        self.instance_dir.join(".socket2.sock")
    }

    /// Dispatch a raw command to Hyprland and return its reply.
    ///
    /// The command is sent exactly as given: no newline is appended. The write
    /// half is shut down so Hyprland sees the request boundary, and the reply is
    /// read until Hyprland closes its side. A 5 s timeout (the same as
    /// `hyprctl`) prevents a stuck compositor from freezing the UI thread.
    pub fn send_command(&self, cmd: &str) -> Result<String> {
        self.send_command_with_timeout(cmd, COMMAND_TIMEOUT)
    }

    fn send_command_with_timeout(&self, cmd: &str, timeout: Duration) -> Result<String> {
        let path = self.command_socket_path();
        let mut stream = UnixStream::connect(&path)
            .with_context(|| format!("connect to Hyprland command socket {}", path.display()))?;
        stream
            .set_read_timeout(Some(timeout))
            .context("set Hyprland command read timeout")?;
        stream
            .set_write_timeout(Some(timeout))
            .context("set Hyprland command write timeout")?;

        stream
            .write_all(cmd.as_bytes())
            .context("write Hyprland IPC command")?;
        stream.flush().context("flush Hyprland IPC command")?;
        let _ = stream.shutdown(Shutdown::Write);

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .context("read Hyprland IPC reply")?;
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    fn request_json<T: DeserializeOwned>(&self, command: &str) -> Result<T> {
        let raw = self.send_command(command)?;
        let reply = raw.trim();
        if reply.is_empty() {
            bail!("Hyprland returned an empty reply to `{command}`");
        }
        if reply.starts_with("unknown request") || reply == "Invalid" {
            bail!("Hyprland rejected `{command}`: {reply}");
        }
        serde_json::from_str(reply)
            .with_context(|| format!("decode Hyprland reply to `{command}`: {reply}"))
    }

    /// Hyprland version string (e.g. `"0.56.2"`).
    pub fn fetch_version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct RawVersion {
            #[serde(default)]
            version: String,
        }

        Ok(self.request_json::<RawVersion>("j/version")?.version)
    }

    /// Hyprland status, including the active config provider.
    pub fn fetch_status(&self) -> Result<HyprlandStatus> {
        self.request_json("j/status")
    }

    /// Whether this Hyprland uses the Lua dispatcher syntax.
    pub fn uses_lua_dispatch(&self) -> bool {
        self.dispatch_style() == DispatchStyle::Lua
    }

    fn dispatch_style(&self) -> DispatchStyle {
        *self.style.get_or_init(|| {
            if let Ok(status) = self.fetch_status() {
                if status.config_provider.eq_ignore_ascii_case("lua") {
                    return DispatchStyle::Lua;
                }
                if !status.config_provider.is_empty() {
                    return DispatchStyle::Legacy;
                }
            }

            // `status` was added after the first releases. Fall back to the
            // version: Lua config landed in 0.55.
            match self.fetch_version() {
                Ok(version) if version_at_least(&version, 0, 55) => DispatchStyle::Lua,
                _ => DispatchStyle::Legacy,
            }
        })
    }

    fn dispatch(&self, body: &str) -> Result<()> {
        let reply = self.send_command(&format!("dispatch {body}"))?;
        let reply = reply.trim();
        if reply.is_empty() || reply == "ok" || reply.starts_with("ok\n") {
            Ok(())
        } else {
            bail!("Hyprland refused dispatch `{body}`: {reply}")
        }
    }

    /// Switch to a numbered workspace.
    pub fn switch_workspace(&self, id: i32) -> Result<()> {
        self.switch_workspace_address(&id.to_string())
    }

    /// Switch to any workspace using its addressable name.
    ///
    /// Hyprland accepts `"3"`, `"web"` and `"special:magic"` here. The
    /// value stored in [`Workspace::address`] is exactly what this expects.
    pub fn switch_workspace_address(&self, address: &str) -> Result<()> {
        let address = sql_safe(address);
        if address.is_empty() {
            bail!("empty workspace address");
        }
        let address = workspace_dispatch_selector(&address);

        match self.dispatch_style() {
            DispatchStyle::Lua => self.dispatch(&format!(
                "hl.dsp.focus({{ workspace = {} }})",
                lua_string(&address)
            )),
            DispatchStyle::Legacy => self.dispatch(&format!("workspace {address}")),
        }
    }

    /// Focus a window by its Hyprland address (`AppWindow::id`).
    pub fn focus_window(&self, address: i64) -> Result<()> {
        let selector = window_selector(address);
        match self.dispatch_style() {
            DispatchStyle::Lua => self.dispatch(&format!(
                "hl.dsp.focus({{ window = {} }})",
                lua_string(&selector)
            )),
            DispatchStyle::Legacy => self.dispatch(&format!("focuswindow {selector}")),
        }
    }

    /// Close the active window.
    pub fn close_window(&self) -> Result<()> {
        match self.dispatch_style() {
            DispatchStyle::Lua => self.dispatch("hl.dsp.window.close()"),
            DispatchStyle::Legacy => self.dispatch("killactive"),
        }
    }

    /// Close a specific window by address.
    pub fn close_window_id(&self, address: i64) -> Result<()> {
        let selector = window_selector(address);
        match self.dispatch_style() {
            DispatchStyle::Lua => self.dispatch(&format!(
                "hl.dsp.window.close({{ window = {} }})",
                lua_string(&selector)
            )),
            DispatchStyle::Legacy => self.dispatch(&format!("closewindow {selector}")),
        }
    }

    /// Move a window (or the active one) to a workspace.
    ///
    /// `follow` is `true` for `movetoworkspace`, `false` for
    /// `movetoworkspacesilent`.
    pub fn move_window_to_workspace(
        &self,
        window: Option<i64>,
        workspace: &str,
        follow: bool,
    ) -> Result<()> {
        let workspace = sql_safe(workspace);
        if workspace.is_empty() {
            bail!("empty workspace address");
        }
        let workspace = workspace_dispatch_selector(&workspace);

        match self.dispatch_style() {
            DispatchStyle::Lua => {
                let mut args = format!("workspace = {}", lua_string(&workspace));
                if let Some(address) = window {
                    args.push_str(&format!(
                        ", window = {}",
                        lua_string(&window_selector(address))
                    ));
                }
                args.push_str(&format!(", follow = {follow}"));
                self.dispatch(&format!("hl.dsp.window.move({{ {args} }})"))
            }
            DispatchStyle::Legacy => {
                let dispatcher = if follow {
                    "movetoworkspace"
                } else {
                    "movetoworkspacesilent"
                };
                match window {
                    Some(address) => self.dispatch(&format!(
                        "{dispatcher} {workspace},{}",
                        window_selector(address)
                    )),
                    None => self.dispatch(&format!("{dispatcher} {workspace}")),
                }
            }
        }
    }

    /// Toggle floating tiling for a window, or for the active window.
    pub fn toggle_floating(&self, window: Option<i64>) -> Result<()> {
        match self.dispatch_style() {
            DispatchStyle::Lua => {
                let mut args = String::from("action = \"toggle\"");
                if let Some(address) = window {
                    args.push_str(&format!(
                        ", window = {}",
                        lua_string(&window_selector(address))
                    ));
                }
                self.dispatch(&format!("hl.dsp.window.float({{ {args} }})"))
            }
            DispatchStyle::Legacy => match window {
                Some(address) => self.dispatch(&format!(
                    "togglefloating {}",
                    window_selector(address)
                )),
                None => self.dispatch("togglefloating"),
            },
        }
    }

    /// Toggle fullscreen for a window, or for the active window.
    ///
    /// The legacy protocol only supports the active window; asking for a
    /// specific window on Hyprland < 0.55 returns an error instead of silently
    /// toggling the wrong one.
    pub fn toggle_fullscreen(&self, window: Option<i64>) -> Result<()> {
        match self.dispatch_style() {
            DispatchStyle::Lua => {
                let mut args = String::from("mode = \"fullscreen\", action = \"toggle\"");
                if let Some(address) = window {
                    args.push_str(&format!(
                        ", window = {}",
                        lua_string(&window_selector(address))
                    ));
                }
                self.dispatch(&format!("hl.dsp.window.fullscreen({{ {args} }})"))
            }
            DispatchStyle::Legacy => {
                if window.is_some() {
                    bail!(
                        "fullscreen for a specific window needs Hyprland >= 0.55 (Lua dispatchers)"
                    );
                }
                self.dispatch("fullscreen 0")
            }
        }
    }

    /// Focus an output by name (`DP-1`, `eDP-1`, …).
    pub fn focus_monitor(&self, name: &str) -> Result<()> {
        let name = sql_safe(name);
        if name.is_empty() {
            bail!("empty monitor name");
        }

        match self.dispatch_style() {
            DispatchStyle::Lua => self.dispatch(&format!(
                "hl.dsp.focus({{ monitor = {} }})",
                lua_string(&name)
            )),
            DispatchStyle::Legacy => self.dispatch(&format!("focusmonitor {name}")),
        }
    }

    /// Toggle a special (scratchpad) workspace by name.
    ///
    /// Both `"magic"` and `"special:magic"` are accepted.
    pub fn toggle_special_workspace(&self, name: &str) -> Result<()> {
        let name = name
            .strip_prefix("special:")
            .unwrap_or(name)
            .trim();
        if name.is_empty() {
            bail!("empty special workspace name");
        }

        match self.dispatch_style() {
            DispatchStyle::Lua => self.dispatch(&format!(
                "hl.dsp.workspace.toggle_special({})",
                lua_string(name)
            )),
            DispatchStyle::Legacy => self.dispatch(&format!("togglespecialworkspace {name}")),
        }
    }

    /// Reload the Hyprland configuration.
    pub fn reload_config(&self) -> Result<()> {
        let reply = self.send_command("reload")?;
        let reply = reply.trim();
        if reply.is_empty() || reply == "ok" {
            Ok(())
        } else {
            bail!("Hyprland refused reload: {reply}")
        }
    }

    /// Fetch the active workspace of the focused monitor.
    pub fn fetch_active_workspace(&self) -> Result<Option<Workspace>> {
        Ok(self
            .fetch_active_workspace_raw()?
            .map(|raw| normalize_workspace(raw, true)))
    }

    fn fetch_active_workspace_raw(&self) -> Result<Option<RawWorkspace>> {
        let raw: RawWorkspace = self.request_json("j/activeworkspace")?;
        Ok((!raw.address.is_empty()).then_some(raw))
    }

    /// Fetch the focused window, or `None` when no window has focus.
    pub fn fetch_active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(self
            .fetch_active_window_raw()?
            .map(active_window_from_raw))
    }

    fn fetch_active_window_raw(&self) -> Result<Option<RawWindow>> {
        let raw: RawWindow = self.request_json("j/activewindow")?;
        Ok((!raw.address.is_empty()).then_some(raw))
    }

    /// Fetch all workspaces.
    ///
    /// Numbered workspaces keep their numeric `id`. Named and special
    /// workspaces have no id in Hyprland's JSON; they get a stable negative id
    /// (see [`Workspace::id`]) and their real selector in
    /// [`Workspace::address`]. A special workspace is marked active while it is
    /// shown on the focused monitor.
    pub fn fetch_workspaces(&self) -> Result<Vec<Workspace>> {
        let raw: Vec<RawWorkspace> = self.request_json("j/workspaces")?;
        let monitors = self.fetch_monitors().unwrap_or_default();
        let active_address = self
            .fetch_active_workspace_raw()
            .ok()
            .flatten()
            .map(|workspace| workspace_selector(&workspace))
            .or_else(|| {
                monitors
                    .iter()
                    .find(|monitor| monitor.focused)
                    .map(|monitor| monitor.active_workspace.clone())
            })
            .unwrap_or_default();
        // A special workspace can be shown on any monitor, not only the focused
        // one; collect them all so the bar does not mark a visible scratchpad as
        // inactive.
        let special_addresses: Vec<String> = monitors
            .iter()
            .filter_map(|monitor| monitor.special_workspace.clone())
            .filter(|address| !address.is_empty())
            .collect();

        let mut used_ids = std::collections::HashSet::new();
        let mut workspaces = Vec::with_capacity(raw.len());

        for mut raw in raw {
            if raw.address.is_empty() {
                raw.address = raw.id.map(|id| id.to_string()).unwrap_or_default();
            }

            let mut workspace = normalize_workspace(raw, false);

            // Keep synthetic ids unique even in the (unlikely) event of a hash
            // collision between two named workspaces.
            if workspace.id < 0 {
                while !used_ids.insert(workspace.id) {
                    workspace.id = workspace.id.saturating_sub(1);
                }
            } else {
                used_ids.insert(workspace.id);
            }

            workspace.is_active = (!active_address.is_empty()
                && workspace.address == active_address)
                || special_addresses
                    .iter()
                    .any(|address| *address == workspace.address);
            workspaces.push(workspace);
        }

        Ok(workspaces)
    }

    /// Fetch the focused window list, including hidden windows.
    ///
    /// `id` is the window address parsed from `0x…`, and `is_focused` comes from
    /// comparing it with `j/activewindow` (not from `focusHistoryID`, which
    /// still contains recently focused windows).
    pub fn fetch_windows(&self) -> Result<Vec<AppWindow>> {
        let raw: Vec<RawWindow> = self.request_json("j/clients")?;
        let focused_address = self
            .fetch_active_window_raw()
            .ok()
            .flatten()
            .map(|window| window.address)
            .unwrap_or_default();

        Ok(raw
            .into_iter()
            .filter(|window| window.mapped || window.hidden)
            .map(|window| {
                let workspace = window.workspace.as_ref();
                AppWindow {
                    id: parse_address(&window.address).unwrap_or(0),
                    title: window.title,
                    class: window.class,
                    workspace_id: workspace.map(workspace_id).unwrap_or(0),
                    workspace_name: workspace
                        .map(workspace_display_name)
                        .unwrap_or_default(),
                    is_focused: !focused_address.is_empty()
                        && window.address.eq_ignore_ascii_case(&focused_address),
                }
            })
            .collect())
    }

    /// Fetch all monitors (outputs) with their active workspaces.
    pub fn fetch_monitors(&self) -> Result<Vec<Monitor>> {
        let raw: Vec<RawMonitor> = self.request_json("j/monitors")?;

        Ok(raw
            .into_iter()
            .map(|monitor| {
                let active = monitor.active_workspace.as_ref();
                let special = monitor
                    .special_workspace
                    .as_ref()
                    .filter(|workspace| !workspace.address.is_empty());

                Monitor {
                    id: monitor.id,
                    name: monitor.name,
                    description: monitor.description,
                    width: monitor.width,
                    height: monitor.height,
                    refresh_rate: monitor.refresh_rate,
                    x: monitor.x,
                    y: monitor.y,
                    scale: if monitor.scale > 0.0 {
                        monitor.scale
                    } else {
                        1.0
                    },
                    focused: monitor.focused,
                    disabled: monitor.disabled,
                    active_workspace: active
                        .map(workspace_selector)
                        .unwrap_or_default(),
                    active_workspace_name: active
                        .map(workspace_display_name)
                        .unwrap_or_default(),
                    special_workspace: special.map(workspace_selector),
                    special_workspace_name: special.map(workspace_display_name),
                }
            })
            .collect())
    }

    /// Connect to `.socket2.sock` and stream events continuously.
    ///
    /// The listener reconnects with exponential backoff when Hyprland restarts
    /// and emits a fresh authoritative burst (workspaces, monitors, windows,
    /// focus) after every connection, so a missed event can never leave stale
    /// state.
    pub fn start_event_loop(
        &self,
        tx: async_channel::Sender<CompositorEvent>,
    ) -> Result<()> {
        let this = self.clone();

        std::thread::Builder::new()
            .name("kumizo-hyprland-events".into())
            .spawn(move || {
                let mut backoff = RECONNECT_MIN;

                while !tx.is_closed() {
                    let started = Instant::now();
                    match this.listen_once(&tx) {
                        Ok(()) => warn!("Kumizo: Hyprland event socket closed, reconnecting"),
                        Err(err) => warn!("Kumizo: Hyprland event socket error: {err}"),
                    }

                    if tx.is_closed() {
                        break;
                    }

                    // A connection that stayed up for a while is a healthy one;
                    // reset the backoff. Otherwise keep backing off so a dead
                    // compositor does not wake the process in a tight loop.
                    if started.elapsed() >= Duration::from_secs(5) {
                        backoff = RECONNECT_MIN;
                    }
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(RECONNECT_MAX);
                }
            })
            .context("spawn Hyprland event listener")?;

        Ok(())
    }

    fn listen_once(&self, tx: &async_channel::Sender<CompositorEvent>) -> Result<()> {
        let stream = UnixStream::connect(self.event_socket_path()).with_context(|| {
            format!(
                "connect to Hyprland event socket {}",
                self.event_socket_path().display()
            )
        })?;
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::with_capacity(1080);
        let mut pending = Pending::default();

        // Catch up on anything that changed between the initial fetch in
        // `Compositor::init` and this connection (or while we were disconnected).
        self.emit_full_sync(tx);

        loop {
            match read_event_line(&mut reader, &mut buffer) {
                Ok(Some(line)) => {
                    classify_event(&mut pending, &line);

                    // Hyprland emits v1 + v2 variants back-to-back. A short
                    // coalescing window turns a burst into one re-query.
                    let _ = reader
                        .get_ref()
                        .set_read_timeout(Some(EVENT_COALESCE));
                    loop {
                        match read_event_line(&mut reader, &mut buffer) {
                            Ok(Some(line)) => classify_event(&mut pending, &line),
                            Ok(None) => {
                                let _ = reader.get_ref().set_read_timeout(None);
                                self.flush_pending(tx, &mut pending);
                                return Ok(());
                            }
                            Err(err)
                                if err.kind() == io::ErrorKind::WouldBlock
                                    || err.kind() == io::ErrorKind::TimedOut =>
                            {
                                break;
                            }
                            Err(err) => {
                                let _ = reader.get_ref().set_read_timeout(None);
                                self.flush_pending(tx, &mut pending);
                                return Err(err.into());
                            }
                        }
                    }
                    let _ = reader.get_ref().set_read_timeout(None);
                    self.flush_pending(tx, &mut pending);

                    if tx.is_closed() {
                        return Ok(());
                    }
                }
                Ok(None) => {
                    self.flush_pending(tx, &mut pending);
                    return Ok(());
                }
                Err(err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut =>
                {
                    // No timeout is set on the blocking path, but be tolerant
                    // of platforms that report it anyway.
                    continue;
                }
                Err(err) => {
                    self.flush_pending(tx, &mut pending);
                    return Err(err.into());
                }
            }
        }
    }

    fn emit_full_sync(&self, tx: &async_channel::Sender<CompositorEvent>) {
        if let Ok(workspaces) = self.fetch_workspaces() {
            if !workspaces.is_empty() {
                let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(workspaces));
            }
        }
        if let Ok(monitors) = self.fetch_monitors() {
            let _ = tx.send_blocking(CompositorEvent::MonitorsChanged(monitors));
        }
        if let Ok(windows) = self.fetch_windows() {
            let _ = tx.send_blocking(CompositorEvent::WindowsChanged(windows));
        }

        match self.fetch_active_window() {
            Ok(Some(window)) => {
                let _ = tx.send_blocking(CompositorEvent::WindowFocused(window));
            }
            Ok(None) => {
                let _ = tx.send_blocking(CompositorEvent::WindowFocusCleared);
            }
            Err(_) => {}
        }
    }

    fn flush_pending(
        &self,
        tx: &async_channel::Sender<CompositorEvent>,
        pending: &mut Pending,
    ) {
        let pending = std::mem::take(pending);

        if pending.workspaces {
            match self.fetch_workspaces() {
                Ok(workspaces) if !workspaces.is_empty() => {
                    let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(workspaces));
                }
                Ok(_) => {}
                Err(err) => warn!("Kumizo: refresh Hyprland workspaces: {err}"),
            }
        }

        if pending.monitors {
            match self.fetch_monitors() {
                Ok(monitors) => {
                    let _ = tx.send_blocking(CompositorEvent::MonitorsChanged(monitors));
                }
                Err(err) => warn!("Kumizo: refresh Hyprland monitors: {err}"),
            }
        }

        if pending.windows {
            match self.fetch_windows() {
                Ok(windows) => {
                    let _ = tx.send_blocking(CompositorEvent::WindowsChanged(windows));
                }
                Err(err) => warn!("Kumizo: refresh Hyprland windows: {err}"),
            }
        }

        if pending.focus {
            match self.fetch_active_window() {
                Ok(Some(window)) => {
                    let _ = tx.send_blocking(CompositorEvent::WindowFocused(window));
                }
                Ok(None) => {
                    let _ = tx.send_blocking(CompositorEvent::WindowFocusCleared);
                }
                Err(err) => warn!("Kumizo: refresh Hyprland focus: {err}"),
            }
        }

        for id in pending.urgent {
            let _ = tx.send_blocking(CompositorEvent::WindowUrgent(id));
        }
    }
}

/// Events parsed from `.socket2.sock` that require a state refresh.
#[derive(Debug, Default)]
struct Pending {
    workspaces: bool,
    monitors: bool,
    windows: bool,
    focus: bool,
    urgent: Vec<i64>,
}

/// Read one complete `\n`-terminated socket2 line.
///
/// `read_until` may leave a partial line in `buffer` when the short coalescing
/// timeout fires. Keeping the buffer across calls is what prevents a split
/// event from being dropped.
fn read_event_line(
    reader: &mut BufReader<UnixStream>,
    buffer: &mut Vec<u8>,
) -> io::Result<Option<String>> {
    loop {
        match reader.read_until(b'\n', buffer) {
            Ok(0) => {
                if buffer.is_empty() {
                    return Ok(None);
                }
                let line = String::from_utf8_lossy(buffer).trim().to_string();
                buffer.clear();
                return Ok(Some(line));
            }
            Ok(_) => {
                if buffer.last() == Some(&b'\n') {
                    let line = String::from_utf8_lossy(buffer)
                        .trim_end_matches(|c| c == '\n' || c == '\r')
                        .to_string();
                    buffer.clear();
                    return Ok(Some(line));
                }

                // `read_until` stopped without finding a newline: EOF. Treat
                // the remaining bytes as the final event.
                let line = String::from_utf8_lossy(buffer).trim().to_string();
                buffer.clear();
                return Ok(Some(line));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
}

fn classify_event(pending: &mut Pending, line: &str) {
    let Some((event, data)) = line.split_once(">>") else {
        return;
    };

    match event {
        "workspace" | "workspacev2" | "createworkspace" | "createworkspacev2"
        | "destroyworkspace" | "destroyworkspacev2" | "renameworkspace"
        | "changeworkspaceid" | "moveworkspace" | "moveworkspacev2" => {
            pending.workspaces = true;
            // Each monitor reports its active workspace in `j/monitors`.
            pending.monitors = true;
        }

        "focusedmon" | "focusedmonv2" | "activespecial" | "activespecialv2" => {
            pending.workspaces = true;
            pending.monitors = true;
        }

        "monitoradded" | "monitoraddedv2" | "monitorremoved" | "monitorremovedv2" => {
            pending.workspaces = true;
            pending.monitors = true;
            // Removing an output can move its windows to another workspace.
            pending.windows = true;
        }

        "configreloaded" => {
            pending.workspaces = true;
            pending.monitors = true;
            pending.windows = true;
            pending.focus = true;
        }

        "movewindow" | "movewindowv2" | "openwindow" | "closewindow" => {
            pending.workspaces = true;
            pending.windows = true;
        }

        "activewindow" | "activewindowv2" => {
            pending.windows = true;
            pending.focus = true;
        }

        "windowtitle" | "windowtitlev2" | "changefloatingmode" | "fullscreen"
        | "togglegroup" | "moveintogroup" | "moveoutofgroup" | "pin" | "minimized"
        | "kill" => pending.windows = true,

        "urgent" => {
            if let Some(id) = parse_address(data.trim()) {
                pending.urgent.push(id);
            }
        }

        _ => {}
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    address: String,
    #[serde(default)]
    id: Option<i32>,
    #[serde(default)]
    name: String,
    /// `"normal"` or `"special"`. A *named* workspace may also be called
    /// `special:x`; the type is what tells the two apart.
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    monitor: String,
    #[serde(default)]
    windows: usize,
    #[serde(default)]
    #[allow(dead_code)]
    hasfullscreen: bool,
    #[serde(default)]
    #[allow(dead_code)]
    ispersistent: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawWindow {
    #[serde(default)]
    address: String,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    workspace: Option<RawWorkspace>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawMonitor {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    width: i32,
    #[serde(default)]
    height: i32,
    #[serde(rename = "refreshRate", default)]
    refresh_rate: f64,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    scale: f64,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(rename = "activeWorkspace", default)]
    active_workspace: Option<RawWorkspace>,
    #[serde(rename = "specialWorkspace", default)]
    special_workspace: Option<RawWorkspace>,
}

fn normalize_workspace(raw: RawWorkspace, is_active: bool) -> Workspace {
    let id = workspace_id(&raw);
    let address = workspace_selector(&raw);
    let name = if raw.name.is_empty() {
        address.clone()
    } else if workspace_type_is_special(&raw.kind, &raw.address) {
        // Hyprland's display name for a special workspace is its address
        // (`special:magic`); the bar only needs `magic`.
        raw.name
            .strip_prefix("special:")
            .unwrap_or(&raw.name)
            .to_string()
    } else {
        raw.name
    };

    Workspace {
        id,
        name,
        address,
        is_active,
        is_urgent: false,
        window_count: raw.windows,
        monitor: if raw.monitor.is_empty() {
            "default".into()
        } else {
            raw.monitor
        },
    }
}

/// Whether this workspace JSON object is a real special workspace.
///
/// `j/workspaces` carries a `type` field. When it is missing (older releases),
/// fall back to the address prefix, which is how special workspaces were always
/// spelled.
fn workspace_type_is_special(kind: &str, address: &str) -> bool {
    if kind.eq_ignore_ascii_case("special") {
        true
    } else if kind.is_empty() {
        address.starts_with("special:")
    } else {
        false
    }
}

/// Backend selector for [`Workspace::address`].
///
/// Numbered and special workspaces use their addressable name as-is. A *named*
/// workspace that happens to be called `special:x` must be prefixed with
/// `name:` — otherwise Hyprland resolves it to the actual special workspace with
/// the same address.
fn workspace_selector(raw: &RawWorkspace) -> String {
    let address = if raw.address.is_empty() {
        raw.id
            .map(|id| id.to_string())
            .unwrap_or_else(|| raw.name.clone())
    } else {
        raw.address.clone()
    };

    if raw.id.is_some() || workspace_type_is_special(&raw.kind, &address) {
        address
    } else if address.starts_with("special:") {
        format!("name:{address}")
    } else {
        address
    }
}

/// Selector to hand to a Hyprland dispatcher.
///
/// Bare names are accepted by the resolver, but several spellings are reserved
/// (`prev`, `next`, `empty`, `special`, `r+1`…). Prefixing named workspaces with
/// `name:` makes the resolution unambiguous and is harmless for the names
/// Hyprland reports. Numbered and special workspaces are passed through.
fn workspace_dispatch_selector(address: &str) -> String {
    if address.starts_with("special:") || address.starts_with("name:") {
        address.to_string()
    } else if address.parse::<u32>().is_ok() {
        address.to_string()
    } else {
        format!("name:{address}")
    }
}

fn workspace_id(raw: &RawWorkspace) -> i32 {
    raw.id.unwrap_or_else(|| {
        if raw.address.is_empty() {
            0
        } else {
            synthetic_workspace_id(&raw.kind, &raw.address)
        }
    })
}

fn workspace_display_name(raw: &RawWorkspace) -> String {
    if !raw.name.is_empty() {
        if workspace_type_is_special(&raw.kind, &raw.address) {
            raw.name
                .strip_prefix("special:")
                .unwrap_or(&raw.name)
                .to_string()
        } else {
            raw.name.clone()
        }
    } else if !raw.address.is_empty() {
        raw.address.clone()
    } else {
        raw.id.map(|id| id.to_string()).unwrap_or_default()
    }
}

fn active_window_from_raw(raw: RawWindow) -> ActiveWindow {
    ActiveWindow {
        title: raw.title,
        class: raw.class,
        workspace_id: raw.workspace.as_ref().map(workspace_id).unwrap_or(0),
    }
}

/// Stable negative id for a named workspace that Hyprland does not number. It
/// is only a UI handle: commands always use [`Workspace::address`].
///
/// The workspace *type* is part of the hash so a real special workspace
/// (`special:x`, type `special`) and a named workspace that happens to be
/// called `special:x` (type `normal`) get distinct ids even though their JSON
/// address is identical.
fn synthetic_workspace_id(kind: &str, address: &str) -> i32 {
    if address.is_empty() {
        return 0;
    }

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in kind.as_bytes().iter().copied().chain([0xff]).chain(address.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    -((hash & 0x3fff_ffff) as i32 + 1)
}

fn parse_address(address: &str) -> Option<i64> {
    let address = address.trim();
    let hex = address
        .strip_prefix("0x")
        .or_else(|| address.strip_prefix("0X"))
        .unwrap_or(address);
    u64::from_str_radix(hex, 16).ok().map(|value| value as i64)
}

fn window_selector(address: i64) -> String {
    format!("address:0x{:x}", address as u64)
}

fn lua_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\x{:02x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Keep user-provided names from breaking the one-line socket request. A
/// newline cannot be represented in a workspace/monitor name anyway.
fn sql_safe(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| {
            if character == '\n' || character == '\r' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn version_at_least(version: &str, major: u64, minor: u64) -> bool {
    let version = version.trim().trim_start_matches('v');
    let mut parts = version.split('.');

    let Some(found_major) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    let found_minor = parts
        .next()
        .and_then(|part| {
            part.chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .ok()
        })
        .unwrap_or(0);

    (found_major, found_minor) >= (major, minor)
}

fn runtime_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime_dir.is_empty() {
            roots.push(PathBuf::from(runtime_dir).join("hypr"));
        }
    }

    // Hyprland itself falls back to `/run/user/<uid>` only when the variable is
    // missing; `hyprctl` does the same. Try it before the legacy `/tmp` path.
    let uid = unsafe { libc::getuid() };
    let run_user = PathBuf::from(format!("/run/user/{uid}/hypr"));
    if !roots.contains(&run_user) {
        roots.push(run_user);
    }

    let legacy = PathBuf::from("/tmp/hypr");
    if !roots.contains(&legacy) {
        roots.push(legacy);
    }

    roots
}

fn find_instance_dir(instance_sig: &str) -> Option<PathBuf> {
    runtime_roots().into_iter().find_map(|root| {
        let dir = root.join(instance_sig);
        (dir.join(".socket.sock").exists() && dir.join(".socket2.sock").exists())
            .then_some(dir)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    struct FakeHyprland {
        dir: PathBuf,
        stop: Arc<AtomicBool>,
        requests: Arc<Mutex<Vec<String>>>,
        thread: Option<JoinHandle<()>>,
    }

    impl FakeHyprland {
        fn start(responses: &[(&str, &str)]) -> Self {
            let dir = unique_temp_dir();
            let listener = UnixListener::bind(dir.join(".socket.sock")).unwrap();
            listener.set_nonblocking(true).unwrap();
            std::fs::write(dir.join(".socket2.sock"), b"").unwrap();

            let stop = Arc::new(AtomicBool::new(false));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let responses: Vec<(String, String)> = responses
                .iter()
                .map(|(command, reply)| (command.to_string(), reply.to_string()))
                .collect();

            let thread = {
                let stop = stop.clone();
                let requests = requests.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((mut stream, _)) => {
                                let mut raw = Vec::new();
                                stream.read_to_end(&mut raw).unwrap();
                                let request = String::from_utf8_lossy(&raw).into_owned();
                                requests.lock().unwrap().push(request.clone());
                                let reply = responses
                                    .iter()
                                    .find(|(command, _)| command == &request)
                                    .map(|(_, reply)| reply.as_str())
                                    .unwrap_or("unknown request");
                                let _ = stream.write_all(reply.as_bytes());
                            }
                            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            Err(_) => break,
                        }
                    }
                })
            };

            Self {
                dir,
                stop,
                requests,
                thread: Some(thread),
            }
        }

        fn ipc(&self) -> HyprlandIpc {
            HyprlandIpc {
                instance_sig: Arc::from("test"),
                instance_dir: self.dir.clone(),
                style: Arc::new(OnceLock::new()),
            }
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for FakeHyprland {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn unique_temp_dir() -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "kumizo-hyprland-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn commands_are_not_newline_terminated() {
        let server = FakeHyprland::start(&[(
            "j/version",
            r#"{"version":"0.56.2","branch":"main"}"#,
        )]);

        let version = server.ipc().fetch_version().unwrap();
        assert_eq!(version, "0.56.2");
        assert_eq!(server.requests(), vec!["j/version".to_string()]);
    }

    #[test]
    fn lua_dispatch_is_used_on_modern_hyprland() {
        let server = FakeHyprland::start(&[
            (
                "j/status",
                r#"{"configProvider":"lua","backend":"drm"}"#,
            ),
            ("dispatch hl.dsp.focus({ workspace = \"3\" })", "ok"),
            ("dispatch hl.dsp.focus({ workspace = \"name:web\" })", "ok"),
            (
                "dispatch hl.dsp.focus({ workspace = \"special:magic\" })",
                "ok",
            ),
            (
                "dispatch hl.dsp.focus({ window = \"address:0x1234\" })",
                "ok",
            ),
            (
                "dispatch hl.dsp.window.close({ window = \"address:0x1234\" })",
                "ok",
            ),
            ("dispatch hl.dsp.window.close()", "ok"),
        ]);

        let ipc = server.ipc();
        assert!(ipc.uses_lua_dispatch());
        ipc.switch_workspace(3).unwrap();
        ipc.switch_workspace_address("web").unwrap();
        ipc.switch_workspace_address("special:magic").unwrap();
        ipc.focus_window(0x1234).unwrap();
        ipc.close_window_id(0x1234).unwrap();
        ipc.close_window().unwrap();

        assert_eq!(
            server.requests(),
            vec![
                "j/status".to_string(),
                "dispatch hl.dsp.focus({ workspace = \"3\" })".to_string(),
                "dispatch hl.dsp.focus({ workspace = \"name:web\" })".to_string(),
                "dispatch hl.dsp.focus({ workspace = \"special:magic\" })".to_string(),
                "dispatch hl.dsp.focus({ window = \"address:0x1234\" })".to_string(),
                "dispatch hl.dsp.window.close({ window = \"address:0x1234\" })".to_string(),
                "dispatch hl.dsp.window.close()".to_string(),
            ]
        );
    }

    #[test]
    fn legacy_dispatch_is_used_on_older_hyprland() {
        let server = FakeHyprland::start(&[
            (
                "j/status",
                r#"{"configProvider":"hyprlang","backend":"drm"}"#,
            ),
            ("dispatch workspace 3", "ok"),
            ("dispatch focuswindow address:0x10", "ok"),
            ("dispatch killactive", "ok"),
        ]);

        let ipc = server.ipc();
        assert!(!ipc.uses_lua_dispatch());
        ipc.switch_workspace(3).unwrap();
        ipc.focus_window(0x10).unwrap();
        ipc.close_window().unwrap();

        assert_eq!(
            server.requests(),
            vec![
                "j/status".to_string(),
                "dispatch workspace 3".to_string(),
                "dispatch focuswindow address:0x10".to_string(),
                "dispatch killactive".to_string(),
            ]
        );
    }

    #[test]
    fn workspaces_support_named_and_special_addresses() {
        let server = FakeHyprland::start(&[
            (
                "j/workspaces",
                r#"[
                    {"address":"1","id":1,"type":"normal","name":"1","monitor":"DP-1","windows":2},
                    {"address":"web","type":"normal","name":"web","monitor":"DP-1","windows":1},
                    {"address":"special:magic","type":"special","name":"special:magic","monitor":"DP-1","windows":3}
                ]"#,
            ),
            (
                "j/activeworkspace",
                r#"{"address":"web","type":"normal","name":"web","monitor":"DP-1","windows":1}"#,
            ),
            (
                "j/monitors",
                r#"[{"id":0,"name":"DP-1","description":"Dell","width":1920,"height":1080,"refreshRate":60.0,"x":0,"y":0,"scale":1.0,"focused":true,"disabled":false,"activeWorkspace":{"address":"web","type":"normal","name":"web"},"specialWorkspace":{"address":"special:magic","type":"special","name":"special:magic"}}]"#,
            ),
        ]);

        let workspaces = server.ipc().fetch_workspaces().unwrap();
        assert_eq!(workspaces.len(), 3);

        let numbered = &workspaces[0];
        assert_eq!(numbered.id, 1);
        assert_eq!(numbered.address, "1");
        assert!(!numbered.is_active, "the focused workspace is `web`, not `1`");

        let named = &workspaces[1];
        assert_eq!(named.name, "web");
        assert_eq!(
            named.address, "web",
            "Hyprland reports the bare address for named workspaces"
        );
        assert!(named.id < 0, "named workspaces get a synthetic id");
        assert!(named.is_active);

        let special = &workspaces[2];
        assert_eq!(special.address, "special:magic");
        assert_eq!(special.name, "magic");
        assert!(special.is_special());
        assert!(special.is_active);
    }

    #[test]
    fn named_workspace_colliding_with_a_special_address_is_disambiguated() {
        let server = FakeHyprland::start(&[
            (
                "j/workspaces",
                r#"[
                    {"address":"special:term","type":"normal","name":"special:term","monitor":"DP-1","windows":1},
                    {"address":"special:term","type":"special","name":"special:term","monitor":"DP-1","windows":1}
                ]"#,
            ),
            ("j/activeworkspace", r#"{"address":"1","id":1,"type":"normal","name":"1"}"#),
            ("j/monitors", "[]"),
        ]);

        let workspaces = server.ipc().fetch_workspaces().unwrap();
        let named = workspaces
            .iter()
            .find(|workspace| !workspace.is_special())
            .expect("named workspace");
        let special = workspaces
            .iter()
            .find(|workspace| workspace.is_special())
            .expect("special workspace");

        assert_eq!(named.address, "name:special:term");
        assert_ne!(named.id, special.id, "ids must not collide");
        assert_eq!(special.address, "special:term");
    }

    #[test]
    fn windows_parse_hyprland_addresses_and_focus() {
        let server = FakeHyprland::start(&[
            (
                "j/clients",
                r#"[
                    {"address":"0x1234","mapped":true,"class":"firefox","title":"Mozilla","workspace":{"address":"1","id":1,"type":"normal","name":"1"}},
                    {"address":"0x5678","mapped":true,"hidden":false,"class":"kitty","title":"shell","workspace":{"address":"web","type":"normal","name":"web"}}
                ]"#,
            ),
            (
                "j/activewindow",
                r#"{"address":"0x1234","class":"firefox","title":"Mozilla","workspace":{"address":"1","id":1,"type":"normal","name":"1"}}"#,
            ),
        ]);

        let windows = server.ipc().fetch_windows().unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, 0x1234);
        assert_eq!(windows[0].workspace_id, 1);
        assert!(windows[0].is_focused);
        assert_eq!(windows[1].workspace_name, "web");
        assert!(windows[1].workspace_id < 0);
        assert!(!windows[1].is_focused);
    }

    #[test]
    fn monitors_expose_focus_and_special_workspaces() {
        let server = FakeHyprland::start(&[(
            "j/monitors",
            r#"[
                {"id":0,"name":"DP-1","description":"Dell Inc.","width":2560,"height":1440,"refreshRate":143.999,"x":0,"y":0,"scale":1.25,"focused":true,"disabled":false,"activeWorkspace":{"address":"web","type":"normal","name":"web"},"specialWorkspace":{"address":"special:magic","type":"special","name":"special:magic"}},
                {"id":1,"name":"HDMI-A-1","description":"","width":1920,"height":1080,"refreshRate":60.0,"x":2560,"y":0,"scale":1.0,"focused":false,"disabled":false,"activeWorkspace":{"address":"2","id":2,"type":"normal","name":"2"},"specialWorkspace":{"address":"","type":"special","name":""}}
            ]"#,
        )]);

        let monitors = server.ipc().fetch_monitors().unwrap();
        assert_eq!(monitors.len(), 2);

        let focused = &monitors[0];
        assert!(focused.focused);
        assert_eq!(focused.active_workspace, "web");
        assert_eq!(focused.special_workspace.as_deref(), Some("special:magic"));
        assert_eq!(focused.special_workspace_name.as_deref(), Some("magic"));
        assert!((focused.refresh_rate - 143.999).abs() < f64::EPSILON);
        assert!((focused.scale - 1.25).abs() < f64::EPSILON);

        let other = &monitors[1];
        assert!(!other.focused);
        assert_eq!(other.special_workspace, None);
        assert_eq!(other.active_workspace, "2");
    }

    #[test]
    fn event_classification_requests_the_right_refresh() {
        let mut pending = Pending::default();
        classify_event(&mut pending, "workspacev2>>2,2");
        assert!(pending.workspaces);
        assert!(!pending.windows);

        classify_event(&mut pending, "openwindow>>0x10,1,kitty,shell");
        assert!(pending.workspaces);
        assert!(pending.windows);

        classify_event(&mut pending, "activewindow>>kitty,shell");
        assert!(pending.focus);

        classify_event(&mut pending, "urgent>>0x10");
        assert_eq!(pending.urgent, vec![0x10]);

        // `bell` does not change any tracked state.
        let mut pending = Pending::default();
        classify_event(&mut pending, "bell>>");
        assert!(!pending.workspaces);
        assert!(!pending.monitors);
        assert!(!pending.windows);
        assert!(!pending.focus);
        assert!(pending.urgent.is_empty());

        classify_event(&mut pending, "moveworkspacev2>>name:web,web,DP-1");
        assert!(pending.workspaces);
        assert!(pending.monitors);
    }

    #[test]
    fn version_and_lua_helpers() {
        assert!(version_at_least("0.56.2", 0, 55));
        assert!(version_at_least("v0.55.0", 0, 55));
        assert!(!version_at_least("0.54.3", 0, 55));
        assert_eq!(
            lua_string("a\"b\\c"),
            r#""a\"b\\c""#
        );
        assert_eq!(parse_address("0x1234"), Some(0x1234));
        assert_eq!(parse_address("1234"), Some(0x1234));
        assert_eq!(parse_address("nope"), None);
    }
}
