use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use gpui::Context;
use crate::services::service::Service;

/// Asynchronous audio management service.
///
/// Tracks volume and mute status via PipeWire/WirePlumber or PulseAudio.
/// Operates with Zero Idle CPU by listening to audio server event streams
/// (`pactl subscribe` / PipeWire signals).
#[derive(Debug, Clone)]
pub struct Audio {
    volume: u8,
    muted: bool,
    sink_name: String,
}

impl Audio {
    /// Current volume level as a percentage (0 to 100).
    pub fn volume(&self) -> u8 {
        self.volume
    }

    /// Whether output is currently muted.
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Default sink name or description.
    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }

    /// Volume icon representation helper.
    pub fn icon(&self) -> &'static str {
        if self.muted || self.volume == 0 {
            "󰝟"
        } else if self.volume < 33 {
            "󰕿"
        } else if self.volume < 66 {
            "󰖀"
        } else {
            "󰕾"
        }
    }

    /// Set volume percentage directly (0..=100).
    pub fn set_volume(&mut self, pct: u8) {
        let clamped = pct.min(100);
        let _ = Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", clamped as f32 / 100.0)])
            .status()
            .or_else(|_| {
                Command::new("pactl")
                    .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", clamped)])
                    .status()
            });
        self.volume = clamped;
    }

    /// Increase volume by step percentage.
    pub fn increase_volume(&mut self, step: u8) {
        self.set_volume(self.volume.saturating_add(step));
    }

    /// Decrease volume by step percentage.
    pub fn decrease_volume(&mut self, step: u8) {
        self.set_volume(self.volume.saturating_sub(step));
    }

    /// Toggle audio mute state.
    pub fn toggle_mute(&mut self) {
        let _ = Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .status()
            .or_else(|_| {
                Command::new("pactl")
                    .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
                    .status()
            });
        self.muted = !self.muted;
    }

    /// Query current volume and mute state synchronously using wpctl / pactl.
    pub fn query_state() -> (u8, bool, String) {
        // Try wpctl first (PipeWire standard)
        if let Ok(output) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                // format: "Volume: 0.65 [MUTED]" or "Volume: 0.65"
                let is_muted = text.contains("[MUTED]");
                if let Some(vol_part) = text.split_whitespace().nth(1) {
                    if let Ok(vol_f) = vol_part.parse::<f32>() {
                        let vol = (vol_f * 100.0).round() as u8;
                        return (vol, is_muted, "PipeWire Sink".into());
                    }
                }
            }
        }

        // Fallback to pactl
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                // Extract percentage e.g. "50%"
                if let Some(idx) = text.find('%') {
                    let before = &text[..idx];
                    if let Some(num_str) = before.split_whitespace().last() {
                        if let Ok(vol) = num_str.parse::<u8>() {
                            let mute_out = Command::new("pactl")
                                .args(["get-sink-mute", "@DEFAULT_SINK@"])
                                .output();
                            let is_muted = mute_out
                                .map(|o| String::from_utf8_lossy(&o.stdout).contains("yes"))
                                .unwrap_or(false);
                            return (vol, is_muted, "PulseAudio Sink".into());
                        }
                    }
                }
            }
        }

        (50, false, "Default Audio".into())
    }
}

impl Default for Audio {
    fn default() -> Self {
        let (vol, muted, sink) = Self::query_state();
        Self {
            volume: vol,
            muted,
            sink_name: sink,
        }
    }
}

impl Service for Audio {
    fn ipc_action(&mut self, _cx: &mut Context<Self>, action: &str, payload: Option<&str>) -> bool {
        match action {
            "toggle_mute" | "toggle-mute" => {
                self.toggle_mute();
                true
            }
            "mute" => {
                if !self.muted {
                    self.toggle_mute();
                }
                true
            }
            "unmute" => {
                if self.muted {
                    self.toggle_mute();
                }
                true
            }
            "volume_up" | "volume-up" => {
                self.increase_volume(5);
                true
            }
            "volume_down" | "volume-down" => {
                self.decrease_volume(5);
                true
            }
            "volume_set" | "volume-set" => {
                let Some(value) = payload.and_then(|p| p.trim().parse::<u8>().ok()) else {
                    return false;
                };
                self.set_volume(value);
                true
            }
            _ => false,
        }
    }

    fn init(cx: &mut Context<Self>) -> Self {
        let initial = Audio::default();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        cx.spawn(async move |this, cx| {
            // Spawn a background thread running `pactl subscribe` which blocks until an event occurs
            let (tx, rx) = async_channel::bounded::<()>(10);

            std::thread::spawn(move || {
                let mut child = Command::new("pactl")
                    .arg("subscribe")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn();

                if let Ok(ref mut proc) = child {
                    use std::io::{BufRead, BufReader};
                    if let Some(stdout) = proc.stdout.take() {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if !running_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            if let Ok(l) = line {
                                if l.contains("sink") || l.contains("server") {
                                    let _ = tx.send_blocking(());
                                }
                            }
                        }
                    }
                }
            });

            // Zero Idle CPU: thread sleeps awaiting event notification
            while let Ok(()) = rx.recv().await {
                let (vol, muted, sink) = Audio::query_state();
                let res = this.update(cx, |audio, cx| {
                    if audio.volume != vol || audio.muted != muted {
                        audio.volume = vol;
                        audio.muted = muted;
                        audio.sink_name = sink;
                        cx.notify();
                    }
                });

                if res.is_err() {
                    running.store(false, Ordering::Relaxed);
                    break;
                }
            }
        })
        .detach();

        initial
    }
}
