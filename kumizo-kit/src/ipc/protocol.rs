use serde::{Deserialize, Serialize};

/// Path of the one and only Kumizo socket of the current session.
///
/// Kumizo is a **single-instance** shell: every window and service lives in
/// one process, so there is exactly one socket. `$KUMIZO_SOCKET` overrides
/// it (tests, nested sessions); otherwise it is `$XDG_RUNTIME_DIR/kumizo.sock`
/// with a per-user `/tmp` fallback.
pub fn socket_path() -> std::path::PathBuf {
    if let Ok(custom) = std::env::var("KUMIZO_SOCKET") {
        if !custom.is_empty() {
            return std::path::PathBuf::from(custom);
        }
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime).join("kumizo.sock")
    } else {
        std::path::PathBuf::from("/tmp").join(format!("kumizo-{}.sock", libc_user_id()))
    }
}

fn libc_user_id() -> u32 {
    #[cfg(unix)]
    unsafe {
        libc::getuid()
    }
    #[cfg(not(unix))]
    1000
}

/// Request commands sent to the Kumizo daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Ping request to verify daemon responsiveness.
    Ping,
    /// Query daemon status and active window metadata.
    Status,
    /// Toggle visibility of a window by its identifier.
    ToggleWindow {
        /// Unique window identifier.
        name: String,
    },
    /// Show a specific window.
    ShowWindow {
        /// Unique window identifier.
        name: String,
    },
    /// Hide a specific window.
    HideWindow {
        /// Unique window identifier.
        name: String,
    },
    /// Request the daemon to reload configuration or re-render.
    Reload,
    /// Trigger an action on a registered service (e.g. mpris next, audio toggle).
    ServiceAction {
        /// Service short name (e.g. "audio", "mpris").
        service: String,
        /// Action name (e.g. "toggle_mute", "next").
        action: String,
        /// Optional action payload.
        payload: Option<String>,
    },
}

/// Responses returned by the Kumizo daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Pong acknowledgment.
    Pong,
    /// Status response containing active windows and services.
    Status {
        /// Seconds since the shell started.
        uptime_secs: u64,
        /// List of visible window IDs.
        windows: Vec<String>,
        /// List of registered service short names.
        services: Vec<String>,
    },
    /// Successful action completion message.
    Ok {
        /// Human-readable success message.
        message: String,
    },
    /// Error description.
    Error {
        /// Human-readable error message.
        message: String,
    },
}
