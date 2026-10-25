//! Programmatic control of Kumizo windows from inside the daemon.
//!
//! The same `WindowCmd` channels that serve `kumizo show/hide` are shared
//! with in-process users (e.g. the tray service opening its context menu):
//! [`WindowControl`] is cloned wherever needed and its `show`/`hide` methods
//! never block the caller — a short-lived thread does the send and waits for
//! the answer with a timeout.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use async_channel::Sender;
use tracing::warn;
use crate::ipc::protocol::{Request, Response};

/// A request forwarded to a window's main-thread control task, plus the
/// channel to answer on. Shared with the IPC server path.
pub type WindowCmd = (Request, mpsc::Sender<Response>);
/// Control channel of one named window.
pub type WindowSender = Sender<WindowCmd>;

/// How long an internal show/hide waits for the main thread.
const CONTROL_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Window name -> control channel, shared between the IPC thread, the main
/// thread and in-process users such as services.
#[derive(Clone, Default, Debug)]
pub struct WindowControl {
    inner: Arc<Mutex<Vec<(String, WindowSender)>>>,
}

impl WindowControl {
    /// Empty registry; entries are added by the application builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a window's control channel (called once per window).
    pub fn register(&self, name: String, tx: WindowSender) {
        self.inner
            .lock()
            .expect("window registry poisoned")
            .push((name, tx));
    }

    /// Look up a window's control channel.
    pub fn lookup(&self, name: &str) -> Option<WindowSender> {
        self.inner
            .lock()
            .expect("window registry poisoned")
            .iter()
            .find(|(registered, _)| registered == name)
            .map(|(_, tx)| tx.clone())
    }

    /// All registered window names (for `Status` and error messages).
    pub fn names(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("window registry poisoned")
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// All registered control channels (for broadcast `Reload`).
    pub fn senders(&self) -> Vec<WindowSender> {
        self.inner
            .lock()
            .expect("window registry poisoned")
            .iter()
            .map(|(_, tx)| tx.clone())
            .collect()
    }

    /// Ask the main thread to (re-)create and show a window.
    ///
    /// Never blocks the caller: a short-lived thread sends the request and
    /// waits for the answer, warning on failure or timeout.
    pub fn show(&self, name: &str) {
        self.send(Request::ShowWindow {
            name: name.to_string(),
        });
    }

    /// Ask the main thread to destroy a window's surface (state is kept for
    /// a later `show`).
    pub fn hide(&self, name: &str) {
        self.send(Request::HideWindow {
            name: name.to_string(),
        });
    }

    /// Show the window if hidden, hide it if shown.
    pub fn toggle(&self, name: &str) {
        self.send(Request::ToggleWindow {
            name: name.to_string(),
        });
    }

    fn send(&self, req: Request) {
        let Some(tx) = self.lookup(request_window_name(&req)) else {
            warn!("WindowControl: unknown window for internal request");
            return;
        };
        std::thread::spawn(move || {
            let (reply_tx, reply_rx) = mpsc::channel();
            if tx.send_blocking((req, reply_tx)).is_err() {
                warn!("WindowControl: control task is gone");
                return;
            }
            match reply_rx.recv_timeout(CONTROL_REPLY_TIMEOUT) {
                Ok(Response::Error { message }) => {
                    warn!("WindowControl: main thread reported: {message}")
                }
                Err(_) => warn!("WindowControl: main thread did not answer in time"),
                _ => {}
            }
        });
    }
}

/// Which window a request is addressed to, if any.
fn request_window_name(req: &Request) -> &str {
    match req {
        Request::ShowWindow { name }
        | Request::HideWindow { name }
        | Request::ToggleWindow { name } => name,
        _ => "",
    }
}
