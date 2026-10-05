use gpui::Context;
use serde::{Deserialize, Serialize};
use crate::service::Service;

/// MPRIS playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlaybackStatus {
    /// Track is actively playing.
    Playing,
    /// Playback is paused.
    Paused,
    /// Playback is stopped.
    #[default]
    Stopped,
}

impl PlaybackStatus {
    /// Icon representation for media widget.
    pub fn icon(&self) -> &'static str {
        match self {
            PlaybackStatus::Playing => "󰏤", // pause icon for toggle
            PlaybackStatus::Paused => "󰐊",  // play icon for toggle
            PlaybackStatus::Stopped => "󰓛",
        }
    }

    /// Whether a track is currently playing.
    pub fn is_playing(&self) -> bool {
        matches!(self, PlaybackStatus::Playing)
    }
}

/// Metadata and state for an active MPRIS media player (Spotify, Firefox, mpv, etc.).
#[derive(Debug, Clone, Default)]
pub struct Mpris {
    player_name: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    status: PlaybackStatus,
}

impl Mpris {
    /// Active track title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Active artist name.
    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// Active album name.
    pub fn album(&self) -> Option<&str> {
        self.album.as_deref()
    }

    /// Human-friendly display label: "Artist - Title" or "Title".
    pub fn display_label(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{} - {}", artist, title),
            (None, Some(title)) => title.clone(),
            _ => "No media playing".to_string(),
        }
    }

    /// Current playback status.
    pub fn status(&self) -> PlaybackStatus {
        self.status
    }

    /// Active player name (e.g. "spotify", "firefox").
    pub fn player_name(&self) -> Option<&str> {
        self.player_name.as_deref()
    }

    /// Toggle Play/Pause on the active player via `playerctl` or D-Bus.
    pub fn play_pause(&self) {
        let _ = std::process::Command::new("playerctl")
            .arg("play-pause")
            .status();
    }

    /// Skip to next track.
    pub fn next(&self) {
        let _ = std::process::Command::new("playerctl")
            .arg("next")
            .status();
    }

    /// Skip to previous track.
    pub fn previous(&self) {
        let _ = std::process::Command::new("playerctl")
            .arg("previous")
            .status();
    }

    /// Stop playback.
    pub fn stop(&self) {
        let _ = std::process::Command::new("playerctl")
            .arg("stop")
            .status();
    }
}

impl Service for Mpris {
    fn ipc_action(&mut self, action: &str, _payload: Option<&str>) -> bool {
        match action {
            "play_pause" | "play-pause" => {
                self.play_pause();
                true
            }
            "next" => {
                self.next();
                true
            }
            "previous" | "prev" => {
                self.previous();
                true
            }
            "stop" => {
                self.stop();
                true
            }
            _ => false,
        }
    }

    fn init(cx: &mut Context<Self>) -> Self {
        let mut initial = Mpris::default();

        // Initial snapshot via playerctl if installed
        if let Ok(output) = std::process::Command::new("playerctl")
            .args(["metadata", "--format", "{{status}}///{{artist}}///{{title}}///{{album}}///{{playerName}}"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = s.trim().split("///").collect();
                if parts.len() >= 5 {
                    initial.status = match parts[0] {
                        "Playing" => PlaybackStatus::Playing,
                        "Paused" => PlaybackStatus::Paused,
                        _ => PlaybackStatus::Stopped,
                    };
                    if !parts[1].is_empty() { initial.artist = Some(parts[1].to_string()); }
                    if !parts[2].is_empty() { initial.title = Some(parts[2].to_string()); }
                    if !parts[3].is_empty() { initial.album = Some(parts[3].to_string()); }
                    if !parts[4].is_empty() { initial.player_name = Some(parts[4].to_string()); }
                }
            }
        }

        cx.spawn(async move |this, cx| {
            // Zero Idle CPU: listen to `playerctl metadata --follow`
            let (tx, rx) = async_channel::bounded::<String>(10);

            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                use std::process::{Command, Stdio};

                if let Ok(mut child) = Command::new("playerctl")
                    .args(["metadata", "--follow", "--format", "{{status}}///{{artist}}///{{title}}///{{album}}///{{playerName}}"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    if let Some(stdout) = child.stdout.take() {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(l) = line {
                                if tx.send_blocking(l).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            while let Ok(line) = rx.recv().await {
                let parts: Vec<&str> = line.trim().split("///").collect();
                if parts.len() >= 5 {
                    let status = match parts[0] {
                        "Playing" => PlaybackStatus::Playing,
                        "Paused" => PlaybackStatus::Paused,
                        _ => PlaybackStatus::Stopped,
                    };
                    let artist = if parts[1].is_empty() { None } else { Some(parts[1].to_string()) };
                    let title = if parts[2].is_empty() { None } else { Some(parts[2].to_string()) };
                    let album = if parts[3].is_empty() { None } else { Some(parts[3].to_string()) };
                    let player = if parts[4].is_empty() { None } else { Some(parts[4].to_string()) };

                    let res = this.update(cx, |mpris, cx| {
                        mpris.status = status;
                        mpris.artist = artist;
                        mpris.title = title;
                        mpris.album = album;
                        mpris.player_name = player;
                        cx.notify();
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
