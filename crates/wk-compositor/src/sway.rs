use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use crate::types::{CompositorEvent, Workspace};

const I3_IPC_MAGIC: &[u8] = b"i3-ipc";
const I3_IPC_MESSAGE_RUN_COMMAND: u32 = 0;
const I3_IPC_MESSAGE_GET_WORKSPACES: u32 = 1;
const I3_IPC_MESSAGE_SUBSCRIBE: u32 = 2;

/// Sway / i3 compositor IPC client.
pub struct SwayIpc {
    socket_path: PathBuf,
}

impl SwayIpc {
    /// Detect whether running under Sway or i3 and return IPC instance.
    pub fn try_new() -> Option<Self> {
        let sock = std::env::var("SWAYSOCK")
            .or_else(|_| std::env::var("I3SOCK"))
            .ok()?;
        let path = PathBuf::from(sock);
        if path.exists() {
            Some(Self { socket_path: path })
        } else {
            None
        }
    }

    /// Build a client for an already resolved socket path.
    ///
    /// The event listener needs this to re-query the compositor for the current
    /// workspace list, since a long-lived subscription cannot report a list it
    /// never received in full.
    fn new(socket_path: PathBuf) -> Option<Self> {
        socket_path.exists().then_some(Self { socket_path })
    }

    /// Send a framed IPC message to Sway.
    pub fn send_message(&self, msg_type: u32, payload: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.write_all(I3_IPC_MAGIC)?;
        stream.write_all(&(payload.len() as u32).to_ne_bytes())?;
        stream.write_all(&msg_type.to_ne_bytes())?;
        stream.write_all(payload)?;
        stream.flush()?;

        let mut magic_buf = [0u8; 6];
        stream.read_exact(&mut magic_buf)?;
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_ne_bytes(len_buf) as usize;
        let mut type_buf = [0u8; 4];
        stream.read_exact(&mut type_buf)?;

        let mut response = vec![0u8; len];
        stream.read_exact(&mut response)?;
        Ok(response)
    }

    /// Switch workspace by number.
    pub fn switch_workspace(&self, num: i32) -> anyhow::Result<()> {
        let cmd = format!("workspace number {}", num);
        let _ = self.send_message(I3_IPC_MESSAGE_RUN_COMMAND, cmd.as_bytes())?;
        Ok(())
    }

    /// Retrieve list of workspaces from Sway.
    pub fn fetch_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        let bytes = self.send_message(I3_IPC_MESSAGE_GET_WORKSPACES, b"")?;
        #[derive(serde::Deserialize)]
        struct SwayWs {
            num: i32,
            name: String,
            focused: bool,
            urgent: bool,
            output: String,
        }

        let items: Vec<SwayWs> = serde_json::from_slice(&bytes)?;
        let workspaces = items
            .into_iter()
            .map(|w| Workspace {
                id: w.num,
                name: w.name,
                is_active: w.focused,
                is_urgent: w.urgent,
                window_count: 1,
                monitor: w.output,
            })
            .collect();

        Ok(workspaces)
    }

    /// Start listening for Sway workspace and window events.
    ///
    /// Sway multiplexes `workspace` and `window` events on one subscription, and
    /// both carry a `change` field. A `window` focus event also has a `current`,
    /// so filtering on the object type is mandatory: decoding a window event as a
    /// workspace event yields a bogus activation with `num = 0`.
    ///
    /// The `init` / `empty` changes are what create and remove workspaces. The
    /// listener cannot patch its own copy of the list incrementally without
    /// replaying ordering, so it re-asks the compositor for the authoritative
    /// list whenever the set of workspaces may have changed.
    pub fn start_event_loop(&self, tx: async_channel::Sender<CompositorEvent>) -> anyhow::Result<()> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        let sub_payload = serde_json::to_vec(&vec!["workspace", "window"])?;

        stream.write_all(I3_IPC_MAGIC)?;
        stream.write_all(&(sub_payload.len() as u32).to_ne_bytes())?;
        stream.write_all(&I3_IPC_MESSAGE_SUBSCRIBE.to_ne_bytes())?;
        stream.write_all(&sub_payload)?;
        stream.flush()?;

        // Read subscribe confirmation
        let mut header = [0u8; 14];
        stream.read_exact(&mut header)?;
        let len = u32::from_ne_bytes([header[6], header[7], header[8], header[9]]) as usize;
        let mut initial_body = vec![0u8; len];
        stream.read_exact(&mut initial_body)?;

        let socket_path = self.socket_path.clone();

        std::thread::spawn(move || {
            let mut hdr = [0u8; 14];
            loop {
                // `read_exact` on a shared buffer would leave partial data behind
                // when a read is interrupted; start from a clean header each time.
                if stream.read_exact(&mut hdr).is_err() {
                    break;
                }
                let msg_len = u32::from_ne_bytes([hdr[6], hdr[7], hdr[8], hdr[9]]) as usize;
                let mut body = vec![0u8; msg_len];
                if stream.read_exact(&mut body).is_err() {
                    break;
                }

                #[derive(serde::Deserialize)]
                struct SwayEvent {
                    change: Option<String>,
                    /// Present for both `workspace` and `window` focus events.
                    current: Option<SwayNode>,
                    /// Present for window focus events only.
                    container: Option<SwayNode>,
                }

                #[derive(serde::Deserialize)]
                struct SwayNode {
                    #[serde(rename = "type")]
                    kind: String,
                    num: Option<i32>,
                }

                let Ok(ev) = serde_json::from_slice::<SwayEvent>(&body) else {
                    continue;
                };

                // A window event carries `container`, and its `current` is null.
                // Never treat it as a workspace event.
                if ev.container.is_some() {
                    continue;
                }

                let change = ev.change.as_deref().unwrap_or("");
                let Some(current) = ev.current.as_ref() else {
                    continue;
                };
                if current.kind != "workspace" {
                    continue;
                }

                // `init` and `empty` mean the workspace appeared or disappeared.
                // `focus` only moves the highlight. `destroy` also removes it.
                let structure_changed = matches!(change, "init" | "empty" | "destroy");

                if structure_changed {
                    if let Some(client) = SwayIpc::new(socket_path.clone()) {
                        if let Ok(list) = client.fetch_workspaces() {
                            let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(list));
                        }
                    }
                }

                // The `change` field is the authority on what happened, not the
                // `focused` flag on the node snapshot: Sway observably emits
                // `focus` with `focused: false` when the switch also destroys a
                // workspace (captured live: `focus` 1 with `focused=false`
                // followed by `empty` 8). Requiring the flag drops the
                // activation and leaves no workspace highlighted.
                if change == "focus" {
                    if let Some(num) = current.num {
                        let _ = tx.send_blocking(CompositorEvent::WorkspaceActivated(num));
                    }
                }
            }
        });

        Ok(())
    }
}
