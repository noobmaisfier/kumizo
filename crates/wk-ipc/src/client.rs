use crate::protocol::{socket_path, Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// Client for communicating with the running WidgetKit instance.
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    /// Client for the session's single WidgetKit socket.
    pub fn connect() -> Self {
        Self {
            socket_path: socket_path(),
        }
    }

    /// Client for an explicit socket path (tests, nested sessions).
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: path.as_ref().to_path_buf(),
        }
    }

    /// Path this client talks to.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Whether a live instance answers on the socket.
    pub fn is_alive(&self) -> bool {
        matches!(self.send(Request::Ping), Ok(Response::Pong))
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
}
