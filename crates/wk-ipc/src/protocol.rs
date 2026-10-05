use serde::{Deserialize, Serialize};

/// Default socket path resolution, used by daemons that do not declare a name.
pub fn default_socket_path() -> std::path::PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime).join("widgetkit.sock")
    } else {
        std::path::PathBuf::from("/tmp").join(format!("widgetkit-{}.sock", libc_user_id()))
    }
}

/// Resolve the socket path of a *named* daemon instance.
///
/// Every widget binary must own a distinct socket: `IpcServer::bind` unlinks
/// any pre-existing file before binding, so two unnamed daemons on
/// `default_socket_path()` would silently steal the socket from one another and
/// only the last one started would answer.
pub fn socket_path(name: &str) -> std::path::PathBuf {
    let file = format!("widgetkit-{}.sock", sanitize(name));
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        std::path::PathBuf::from(runtime).join(file)
    } else {
        std::path::PathBuf::from("/tmp").join(format!(
            "widgetkit-{}-{}.sock",
            sanitize(name),
            libc_user_id()
        ))
    }
}

/// Discover every WidgetKit daemon socket reachable in the current session.
///
/// Returns `(daemon name, socket path)` pairs, sorted by name. Stale sockets
/// left behind by a crashed daemon are filtered out by attempting a ping.
pub fn discover_daemons() -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();

    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if let Ok(entries) = std::fs::read_dir(&runtime) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(stripped) = name.strip_prefix("widgetkit-") else {
                    continue;
                };
                let Some(daemon) = stripped.strip_suffix(".sock") else {
                    continue;
                };
                out.push((daemon.to_string(), entry.path()));
            }
        }
    }

    out.sort();
    out
}

/// Reduce an arbitrary label to characters that are safe in a file name.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn libc_user_id() -> u32 {
    #[cfg(unix)]
    unsafe {
        libc::getuid()
    }
    #[cfg(not(unix))]
    1000
}

/// Request commands sent to the WidgetKit daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Ping request to verify daemon responsiveness.
    Ping,
    /// Query daemon status and active window metadata.
    Status,
    /// Toggle visibility of a window by its identifier.
    ToggleWindow { name: String },
    /// Show a specific window.
    ShowWindow { name: String },
    /// Hide a specific window.
    HideWindow { name: String },
    /// Request the daemon to reload configuration or re-render.
    Reload,
    /// Trigger an action on a registered service (e.g. mpris next, audio toggle).
    ServiceAction {
        service: String,
        action: String,
        payload: Option<String>,
    },
}

/// Responses returned by the WidgetKit daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Pong acknowledgment.
    Pong,
    /// Status response containing active windows and services.
    Status {
        uptime_secs: u64,
        windows: Vec<String>,
        services: Vec<String>,
    },
    /// Successful action completion message.
    Ok { message: String },
    /// Error description.
    Error { message: String },
}
