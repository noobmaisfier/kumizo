use crate::protocol::{default_socket_path, discover_daemons, socket_path, Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// Client for communicating with a running WidgetKit daemon.
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    /// Connect to default socket.
    pub fn default_socket() -> Self {
        Self {
            socket_path: default_socket_path(),
        }
    }

    /// Connect to the socket of a named daemon, e.g. `IpcClient::named("topbar")`.
    pub fn named(name: &str) -> Self {
        Self {
            socket_path: socket_path(name),
        }
    }

    /// Connect with explicit socket path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: path.as_ref().to_path_buf(),
        }
    }

    /// Resolve a `--socket` style argument: either a bare daemon name or a path.
    ///
    /// Precedence: explicit `WIDGETKIT_SOCKET` env var, then the argument, then
    /// the default socket.
    pub fn resolve(target: Option<&str>) -> Self {
        if let Ok(from_env) = std::env::var("WIDGETKIT_SOCKET") {
            if !from_env.is_empty() {
                return Self::new(from_env);
            }
        }
        match target {
            Some(t) if !t.is_empty() => {
                // A value containing a separator is treated as a path, anything
                // else as a daemon name.
                if t.contains('/') {
                    Self::new(t)
                } else {
                    Self::named(t)
                }
            }
            _ => Self::default_socket(),
        }
    }

    /// Path this client talks to.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send a request and wait for the response.
    pub fn send(&self, req: Request) -> anyhow::Result<Response> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        let mut payload = serde_json::to_string(&req)?;
        payload.push('\n');

        stream.write_all(payload.as_bytes())?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        let resp: Response = serde_json::from_str(&line)?;
        Ok(resp)
    }

    /// List the daemons currently reachable in this session, with their status.
    ///
    /// Falls back to the legacy unnamed socket if no `widgetkit-<name>.sock`
    /// file is present.
    pub fn list_daemons() -> Vec<(String, PathBuf, Option<Response>)> {
        let mut out: Vec<(String, PathBuf, Option<Response>)> = discover_daemons()
            .into_iter()
            .map(|(name, path)| {
                let reply = Self::new(&path).send(Request::Ping).ok();
                (name, path, reply)
            })
            .collect();

        if out.is_empty() {
            let path = default_socket_path();
            if path.exists() {
                let reply = Self::new(&path).send(Request::Ping).ok();
                out.push(("(default)".to_string(), path, reply));
            }
        }
        out
    }
}
