use std::path::PathBuf;
use std::time::Duration;
use gpui::Context;
use tracing::warn;
use crate::service::Service;

/// Step used by `increase` / `decrease` and the IPC actions.
const STEP: u8 = 5;
/// How often sysfs is re-read: backlight changes emit no events
/// (no `pactl subscribe` equivalent), so a cheap file read on a timer is
/// the only signal. Notify only on change (see `init`).
const POLL_INTERVAL: Duration = Duration::from_secs(2);

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

    /// First backlight device and its level, if any.
    pub fn query() -> Option<(u8, String)> {
        let base = PathBuf::from("/sys/class/backlight");
        let mut devices: Vec<String> = std::fs::read_dir(&base)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        devices.sort();
        let name = devices.into_iter().next()?;
        let max: u32 = std::fs::read_to_string(base.join(&name).join("max_brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        if max == 0 {
            return None;
        }
        let cur: u32 = std::fs::read_to_string(base.join(&name).join("brightness"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        Some((((cur * 100) / max).min(100) as u8, name))
    }

    /// Set the level, updating optimistically: sysfs only catches up on the
    /// next poll, and waiting for it would make keys feel dead.
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

        cx.spawn(async move |this, cx| loop {
            async_io::Timer::after(POLL_INTERVAL).await;
            if let Some((pct, _)) = Brightness::query() {
                let res = this.update(cx, |b, cx| {
                    if b.percent != pct {
                        b.percent = pct;
                        cx.notify();
                    }
                });
                if res.is_err() {
                    break;
                }
            } else if this.upgrade().is_none() {
                break;
            }
        })
        .detach();

        initial
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_math_matches_sysfs() {
        // 17040/65535, as measured on this machine: must be 26.
        let pct = ((17040u32 * 100) / 65535).min(100) as u8;
        assert_eq!(pct, 26);
    }
}
