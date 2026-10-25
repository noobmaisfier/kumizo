use crate::ipc::protocol::{Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

/// Callback handler type for processing incoming IPC requests.
pub type RequestHandler = Arc<dyn Fn(Request) -> Response + Send + Sync + 'static>;

/// Kumizo IPC server managing the UNIX domain socket.
///
/// The listener is held here rather than only inside the accept thread: dropping
/// it closes the socket, so the owning thread cannot be the only owner.
pub struct IpcServer {
    /// Kept alive purely to own the socket; see [`IpcServer::socket_path`].
    _listener: UnixListener,
    socket_path: PathBuf,
    is_running: Arc<AtomicBool>,
}

impl IpcServer {
    /// Bind to the specified socket path with a custom request handler.
    pub fn bind(
        path: impl AsRef<Path>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let socket_path = path.as_ref().to_path_buf();

        if socket_path.exists() {
            // Single-instance guard. `UnixListener::bind` needs the path to be
            // free, so a leftover file has to go, but only a *stale* one: if
            // something answers, another instance owns it and we must not
            // steal its socket.
            if crate::ipc::client::IpcClient::new(&socket_path).is_alive() {
                anyhow::bail!(
                    "another Kumizo instance is already running on {}",
                    socket_path.display()
                );
            }
            let _ = std::fs::remove_file(&socket_path);
        }

        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        info!(
            "Kumizo IPC server listening on {}",
            socket_path.display()
        );

        let is_running = Arc::new(AtomicBool::new(true));
        let running_clone = is_running.clone();
        let handler_arc: RequestHandler = Arc::new(handler);

        {
            // The accept loop runs on its own thread, but `listener` stays owned
            // by `Self` below so the socket cannot be closed from under it.
            let listener = listener.try_clone()?;
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if !running_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    match stream {
                        Ok(sock) => {
                            let handler = handler_arc.clone();
                            std::thread::spawn(move || {
                                // `BufReader` would hold a shared borrow of `sock` for the
                                // whole loop, which conflicts with writing the response on
                                // the same connection. `try_clone` dups the descriptor so we
                                // get an independent handle for the write half.
                                let mut writer = match sock.try_clone() {
                                    Ok(writer) => writer,
                                    Err(err) => {
                                        warn!("IPC socket could not be duplicated: {}", err);
                                        return;
                                    }
                                };

                                let reader = BufReader::new(sock);
                                for line in reader.lines() {
                                    if let Ok(l) = line {
                                        if l.trim().is_empty() {
                                            continue;
                                        }

                                        let resp = match serde_json::from_str::<Request>(&l) {
                                            Ok(req) => handler(req),
                                            Err(err) => Response::Error {
                                                message: format!("Malformed JSON request: {}", err),
                                            },
                                        };

                                        if let Ok(mut json) = serde_json::to_string(&resp) {
                                            json.push('\n');
                                            let _ = writer.write_all(json.as_bytes());
                                            let _ = writer.flush();
                                        }
                                    }
                                }
                            });
                        }
                        Err(err) => {
                            warn!("IPC socket incoming error: {}", err);
                        }
                    }
                }
            });
        }

        Ok(Self {
            _listener: listener,
            socket_path,
            is_running,
        })
    }

    /// Path of the bound socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Stop the server and clean up socket file.
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}
