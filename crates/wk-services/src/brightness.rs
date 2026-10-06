use std::path::PathBuf;
use gpui::Context;
use tracing::warn;
use crate::service::Service;

/// Step used by `increase` / `decrease` and the IPC actions.
const STEP: u8 = 5;

/// Screen backlight level.
///
/// Reads `/sys/class/backlight`, writes via `brightnessctl` (handles the
/// sysfs permission dance through udev). Mirrors the `Audio` service shape
/// so the OSD and `widgetctl` treat both the same way.
#[derive(Debug, Clone)]
pub struct Brightness {
    percent: u8,
    device: String,
}

impl Brightness {
    /// Current level as 0-100.
    pub fn percent(&self) -> u8 {
        self.percent
    }

    /// Backlight device in use (e.g. `amdgpu_bl1`), empty if none found.
    pub fn device(&self) -> &str {
        &self.device
    }

    /// Brightness icon: a single sun (`weather-sunny`, `U+F0599`).
    ///
    /// Deliberately NOT the `brightness-1..7` range (`U+F00DA..F00E0`):
    /// those codepoints are correct in MDI 7.4 CSS but render as moon
    /// phases in the locally installed Nerd Font (older MDI). Rendered
    /// proof beats the spec sheet — this is exactly why glyphs are
    /// render-tested before use. The level is carried by the % and the bar.
    pub fn icon(&self) -> &'static str {
        "󰖙"
    }

    /// First backlight device (sorted by name): its sysfs directory and name.
    pub(crate) fn find_device() -> Option<(PathBuf, String)> {
        let base = PathBuf::from("/sys/class/backlight");
        let mut devices: Vec<String> = std::fs::read_dir(&base)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        devices.sort();
        let name = devices.into_iter().next()?;
        Some((base.join(&name), name))
    }

    /// First backlight device and its level, if any.
    pub fn query() -> Option<(u8, String)> {
        let (dir, name) = Self::find_device()?;
        let max: u32 = std::fs::read_to_string(dir.join("max_brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        if max == 0 {
            return None;
        }
        let cur: u32 = std::fs::read_to_string(dir.join("brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        Some((((cur * 100) / max).min(100) as u8, name))
    }

    /// Set the level, updating optimistically so keys feel instant; the
    /// kernel's change notification then confirms (or corrects) the value.
    pub fn set_percent(&mut self, pct: u8) {
        let clamped = pct.min(100);
        let out = std::process::Command::new("brightnessctl")
            .args(["set", &format!("{clamped}%")])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                self.percent = clamped;
            }
            _ => warn!("Brightness: brightnessctl set failed (not in video group?)"),
        }
    }

    /// Raise by one step.
    pub fn increase(&mut self) {
        self.set_percent(self.percent.saturating_add(STEP));
    }

    /// Lower by one step.
    pub fn decrease(&mut self) {
        self.set_percent(self.percent.saturating_sub(STEP));
    }
}

impl Service for Brightness {
    /// Actions for `widgetctl service brightness …`:
    /// `up` / `down` (±5) and `set <0-100>`.
    fn ipc_action(&mut self, action: &str, payload: Option<&str>) -> bool {
        match action {
            "up" | "brightness_up" => {
                self.increase();
                true
            }
            "down" | "brightness_down" => {
                self.decrease();
                true
            }
            "set" | "brightness_set" => {
                if let Some(pct) = payload.and_then(|p| p.parse::<u8>().ok()) {
                    self.set_percent(pct);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn init(cx: &mut Context<Self>) -> Self {
        let mut initial = Brightness {
            percent: 50,
            device: String::new(),
        };
        if let Some((pct, dev)) = Brightness::query() {
            initial.percent = pct;
            initial.device = dev;
        }

        // Event-driven: a dedicated thread sleeps in poll(2) on the kernel's
        // notifications and wakes this task only when something happened.
        let (tx, rx) = async_channel::unbounded::<()>();
        watch::spawn(tx);

        cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                // Coalesce bursts (a key held down, a fade) into one read.
                while rx.try_recv().is_ok() {}
                // Re-resolving the device also handles hot-plugged panels.
                if let Some((pct, dev)) = Brightness::query() {
                    let res = this.update(cx, |b, cx| {
                        if b.percent != pct || b.device != dev {
                            b.percent = pct;
                            b.device = dev;
                            cx.notify();
                        }
                    });
                    if res.is_err() {
                        break;
                    }
                }
            }
        })
        .detach();

        initial
    }
}

/// Kernel notification sources for `/sys/class/backlight`.
///
/// The backlight core already tells userspace when the level changes; no
/// polling is needed:
///
/// * `sysfs_notify(.., "actual_brightness")` on every change (sysfs write or
///   `backlight_force_update()`): wakes `poll(2)` with `POLLPRI | POLLERR` on
///   an open, already-read `actual_brightness` file.
/// * a `KOBJ_CHANGE` uevent (`SUBSYSTEM=backlight`) on the kernel's netlink
///   `NETLINK_KOBJECT_UEVENT` group 1. Readable by unprivileged users, needs
///   no udev, and also reports devices appearing or disappearing.
///
/// One thread waits on both with a single `poll(2)`: zero CPU while idle.
///
/// Honest limit: a driver that changes the level in hardware (hotkey) without
/// calling `backlight_force_update()` signals nothing, and no userspace trick
/// can detect that. For such a machine set `WIDGETKIT_BRIGHTNESS_POLL_MS`
/// (e.g. `2000`) to add a safety re-read on top of the events; it is off by
/// default. Without any event source (no backlight device, no netlink) the
/// watcher falls back to `FALLBACK_POLL_MS`.
mod watch {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    use async_channel::Sender;
    use tracing::{debug, warn};

    /// `NETLINK_KOBJECT_UEVENT` (linux/netlink.h).
    const NETLINK_KOBJECT_UEVENT: libc::c_int = 15;
    /// Kernel-originated uevents (group 2 is udev's re-broadcast).
    const UEVENT_GROUP_KERNEL: u32 = 1;
    /// Re-read period when no event source could be set up at all.
    const FALLBACK_POLL_MS: libc::c_int = 2000;

    pub(super) fn spawn(tx: Sender<()>) {
        let spawned = std::thread::Builder::new()
            .name("wk-backlight".into())
            .spawn(move || run(tx));
        if let Err(err) = spawned {
            warn!("Brightness: cannot start the backlight watcher: {err}");
        }
    }

    /// Optional safety poll for drivers that never notify.
    fn safety_poll_ms() -> Option<libc::c_int> {
        std::env::var("WIDGETKIT_BRIGHTNESS_POLL_MS")
            .ok()?
            .parse::<libc::c_int>()
            .ok()
            .filter(|ms| *ms > 0)
    }

    /// Open the kernel uevent socket, subscribed to kernel events only.
    fn open_uevent_socket() -> Option<OwnedFd> {
        // SAFETY: plain syscalls; the fd is wrapped in `OwnedFd` right away so
        // it is closed on every path, and `sockaddr_nl` is zero-initialised.
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                NETLINK_KOBJECT_UEVENT,
            );
            if fd < 0 {
                warn!("Brightness: no uevent socket: {}", std::io::Error::last_os_error());
                return None;
            }
            let owned = OwnedFd::from_raw_fd(fd);
            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            addr.nl_groups = UEVENT_GROUP_KERNEL;
            let rc = libc::bind(
                owned.as_raw_fd(),
                &addr as *const libc::sockaddr_nl as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            );
            if rc < 0 {
                warn!("Brightness: uevent bind failed: {}", std::io::Error::last_os_error());
                return None;
            }
            Some(owned)
        }
    }

    /// Open `actual_brightness` of the first backlight device and arm it.
    ///
    /// sysfs only reports `POLLPRI` for a file that has been read since it
    /// was opened, hence the immediate read.
    fn open_actual_brightness() -> Option<File> {
        let (dir, _) = super::Brightness::find_device()?;
        let mut file = File::open(dir.join("actual_brightness")).ok()?;
        let mut sink = String::new();
        file.read_to_string(&mut sink).ok()?;
        Some(file)
    }

    /// Re-arm after a notification: seek back and read again.
    fn rearm(file: &mut File) -> bool {
        let mut sink = String::new();
        file.seek(SeekFrom::Start(0)).is_ok() && file.read_to_string(&mut sink).is_ok()
    }

    /// Drain pending uevents; true if one concerns the backlight subsystem.
    fn drain_uevents(fd: &OwnedFd) -> bool {
        let mut buf = [0u8; 8192];
        let mut relevant = false;
        loop {
            // SAFETY: `buf` is valid for `buf.len()` bytes for the whole call.
            let n = unsafe {
                libc::recv(
                    fd.as_raw_fd(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if n <= 0 {
                return relevant; // EAGAIN (drained) or error
            }
            relevant |= is_backlight_uevent(&buf[..n as usize]);
        }
    }

    /// A kernel uevent is `action@devpath\0KEY=value\0...`.
    pub(super) fn is_backlight_uevent(msg: &[u8]) -> bool {
        msg.split(|b| *b == 0)
            .any(|field| field == b"SUBSYSTEM=backlight")
    }

    fn run(tx: Sender<()>) {
        let uevents = open_uevent_socket();
        let mut sysfs = open_actual_brightness();
        let safety = safety_poll_ms();

        if uevents.is_none() && sysfs.is_none() {
            warn!(
                "Brightness: no kernel notification source, re-reading every {FALLBACK_POLL_MS} ms"
            );
        } else {
            debug!(
                "Brightness: watching (uevent: {}, sysfs notify: {})",
                uevents.is_some(),
                sysfs.is_some()
            );
        }

        loop {
            let mut pfds: Vec<libc::pollfd> = Vec::with_capacity(2);
            if let Some(fd) = &uevents {
                pfds.push(libc::pollfd {
                    fd: fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
            }
            let sysfs_index = sysfs.as_ref().map(|file| {
                pfds.push(libc::pollfd {
                    fd: file.as_raw_fd(),
                    events: libc::POLLPRI | libc::POLLERR,
                    revents: 0,
                });
                pfds.len() - 1
            });

            let timeout = if pfds.is_empty() {
                FALLBACK_POLL_MS
            } else {
                safety.unwrap_or(-1) // -1: sleep until the kernel speaks
            };

            // SAFETY: `pfds` is a valid slice of `pollfd` for the call.
            let rc = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout) };
            if rc < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                warn!("Brightness: poll failed: {}", std::io::Error::last_os_error());
                return;
            }

            // `rc == 0` is a timeout: the fallback/safety tick asks for a re-read.
            let mut changed = rc == 0;
            let mut reopen = false;

            // The uevent socket, when present, is always pollfd 0.
            if let Some(fd) = &uevents {
                if pfds[0].revents & libc::POLLIN != 0 && drain_uevents(fd) {
                    changed = true;
                    reopen = true; // device may have appeared, gone or changed
                }
            }
            if let (Some(i), Some(file)) = (sysfs_index, sysfs.as_mut()) {
                if pfds[i].revents & (libc::POLLPRI | libc::POLLERR) != 0 {
                    changed = true;
                    if !rearm(file) {
                        reopen = true;
                    }
                }
            }
            if reopen {
                sysfs = open_actual_brightness();
            }

            if changed && tx.send_blocking(()).is_err() {
                return; // the service is gone
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::watch::is_backlight_uevent;

    #[test]
    fn recognises_backlight_change_uevent() {
        let msg = b"change@/devices/pci0000:00/backlight/amdgpu_bl1\0ACTION=change\0SUBSYSTEM=backlight\0SEQNUM=42\0";
        assert!(is_backlight_uevent(msg));
    }

    #[test]
    fn ignores_other_subsystems() {
        let msg = b"change@/devices/foo\0ACTION=change\0SUBSYSTEM=power_supply\0";
        assert!(!is_backlight_uevent(msg));
        // A path merely containing the word must not match.
        let tricky = b"change@/devices/backlight\0SUBSYSTEM=drm\0";
        assert!(!is_backlight_uevent(tricky));
    }
    #[test]
    fn percent_math_matches_sysfs() {
        // 17040/65535, as measured on this machine: must be 26.
        let pct = ((17040u32 * 100) / 65535).min(100) as u8;
        assert_eq!(pct, 26);
    }
}
