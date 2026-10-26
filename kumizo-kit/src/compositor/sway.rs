use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tracing::warn;

use crate::compositor::types::{ActiveWindow, AppWindow, CompositorEvent, Monitor, Workspace};

const I3_IPC_MAGIC: &[u8] = b"i3-ipc";
const I3_IPC_MESSAGE_RUN_COMMAND: u32 = 0;
const I3_IPC_MESSAGE_GET_WORKSPACES: u32 = 1;
const I3_IPC_MESSAGE_SUBSCRIBE: u32 = 2;
const I3_IPC_MESSAGE_GET_OUTPUTS: u32 = 3;
const I3_IPC_MESSAGE_GET_TREE: u32 = 4;

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
        path.exists().then_some(Self { socket_path: path })
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
    pub fn send_message(&self, msg_type: u32, payload: &[u8]) -> Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("connect to Sway socket {}", self.socket_path.display()))?;
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

    /// Run a Sway command and surface a failed `[{ "success": false }]` reply.
    pub fn run_command(&self, command: &str) -> Result<()> {
        let response = self.send_message(I3_IPC_MESSAGE_RUN_COMMAND, command.as_bytes())?;
        let Ok(replies) = serde_json::from_slice::<serde_json::Value>(&response) else {
            return Ok(());
        };

        if let Some(replies) = replies.as_array() {
            for reply in replies {
                if reply.get("success").and_then(|value| value.as_bool()) == Some(false) {
                    let error = reply
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown error");
                    bail!("Sway refused `{command}`: {error}");
                }
            }
        }

        Ok(())
    }

    /// Switch workspace by number.
    pub fn switch_workspace(&self, num: i32) -> Result<()> {
        self.switch_workspace_address(&num.to_string())
    }

    /// Switch to a workspace by its backend address.
    ///
    /// Sway stores the workspace name in [`Workspace::address`]. Numeric names
    /// use `workspace number N` so they target the numbered workspace rather
    /// than a same-named one; everything else is quoted.
    pub fn switch_workspace_address(&self, address: &str) -> Result<()> {
        let address = address.trim();
        if address.is_empty() {
            bail!("empty workspace address");
        }

        if let Ok(number) = address.parse::<i32>() {
            self.run_command(&format!("workspace number {number}"))
        } else {
            self.run_command(&format!("workspace {}", sway_quote(address)))
        }
    }

    /// Focus a window container by its Sway node ID.
    pub fn focus_window(&self, con_id: i64) -> Result<()> {
        self.run_command(&format!("[con_id={con_id}] focus"))
    }

    /// Close the focused window.
    pub fn close_window(&self) -> Result<()> {
        self.run_command("kill")
    }

    /// Close a specific window container.
    pub fn close_window_id(&self, con_id: i64) -> Result<()> {
        self.run_command(&format!("[con_id={con_id}] kill"))
    }

    /// Toggle floating state for a window (or the focused one).
    pub fn toggle_floating(&self, con_id: Option<i64>) -> Result<()> {
        match con_id {
            Some(con_id) => self.run_command(&format!("[con_id={con_id}] floating toggle")),
            None => self.run_command("floating toggle"),
        }
    }

    /// Toggle fullscreen for a window (or the focused one).
    pub fn toggle_fullscreen(&self, con_id: Option<i64>) -> Result<()> {
        match con_id {
            Some(con_id) => self.run_command(&format!("[con_id={con_id}] fullscreen toggle")),
            None => self.run_command("fullscreen toggle"),
        }
    }

    /// Move a window to another workspace.
    pub fn move_window_to_workspace(
        &self,
        con_id: Option<i64>,
        workspace: &str,
        follow: bool,
    ) -> Result<()> {
        let workspace = workspace.trim();
        if workspace.is_empty() {
            bail!("empty workspace address");
        }

        let target = workspace
            .parse::<i32>()
            .map(|number| format!("number {number}"))
            .unwrap_or_else(|_| sway_quote(workspace));
        let selector = con_id
            .map(|con_id| format!("[con_id={con_id}] "))
            .unwrap_or_default();
        self.run_command(&format!("{selector}move container to workspace {target}"))?;

        if follow {
            self.switch_workspace_address(workspace)?;
        }

        Ok(())
    }

    /// Focus an output by name.
    pub fn focus_monitor(&self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("empty monitor name");
        }
        self.run_command(&format!("focus output {}", sway_quote(name)))
    }

    /// Reload the Sway configuration.
    pub fn reload_config(&self) -> Result<()> {
        self.run_command("reload")
    }

    /// Retrieve all open application windows across all workspaces from Sway tree.
    pub fn fetch_windows(&self) -> Result<Vec<AppWindow>> {
        let bytes = self.send_message(I3_IPC_MESSAGE_GET_TREE, b"")?;
        let root: serde_json::Value = serde_json::from_slice(&bytes)?;
        let mut windows = Vec::new();

        fn traverse(
            node: &serde_json::Value,
            current_ws_id: i32,
            current_ws_name: &str,
            acc: &mut Vec<AppWindow>,
        ) {
            let mut ws_id = current_ws_id;
            let mut ws_name = current_ws_name;

            if node.get("type").and_then(|t| t.as_str()) == Some("workspace") {
                if let Some(name) = node.get("name").and_then(|n| n.as_str()) {
                    ws_name = name;
                }
                let number = node.get("num").and_then(|n| n.as_i64()).unwrap_or(-1);
                // Named workspaces report `num = -1`; derive the same stable
                // synthetic id the workspace list uses so windows still map to
                // their workspace.
                ws_id = if number >= 0 {
                    number as i32
                } else {
                    synthetic_workspace_id(ws_name)
                };
            }

            let app_id = node.get("app_id").and_then(|a| a.as_str());
            let win_class = node
                .get("window_properties")
                .and_then(|wp| wp.get("class"))
                .and_then(|c| c.as_str());
            let title = node.get("name").and_then(|n| n.as_str());

            if let (Some(class), Some(title)) = (app_id.or(win_class), title) {
                if !class.is_empty() && !title.is_empty() {
                    let id = node.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
                    let focused = node
                        .get("focused")
                        .and_then(|f| f.as_bool())
                        .unwrap_or(false);

                    acc.push(AppWindow {
                        id,
                        title: title.to_string(),
                        class: class.to_string(),
                        workspace_id: ws_id,
                        workspace_name: ws_name.to_string(),
                        is_focused: focused,
                    });
                }
            }

            for key in ["nodes", "floating_nodes"] {
                if let Some(children) = node.get(key).and_then(|n| n.as_array()) {
                    for child in children {
                        traverse(child, ws_id, ws_name, acc);
                    }
                }
            }
        }

        traverse(&root, 1, "1", &mut windows);
        Ok(windows)
    }

    /// Retrieve list of workspaces from Sway.
    pub fn fetch_workspaces(&self) -> Result<Vec<Workspace>> {
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
            .map(|w| {
                // i3/Sway report `num = -1` for named workspaces. A negative id
                // would collide with every other named workspace, so derive a
                // stable synthetic id (as the Hyprland backend does) and keep the
                // real selector in `address`.
                let id = if w.num >= 0 {
                    w.num
                } else {
                    synthetic_workspace_id(&w.name)
                };
                Workspace {
                    id,
                    address: w.name.clone(),
                    name: w.name,
                    is_active: w.focused,
                    is_urgent: w.urgent,
                    window_count: 0,
                    monitor: w.output,
                }
            })
            .collect();

        Ok(workspaces)
    }

    /// Retrieve outputs (monitors) from Sway.
    pub fn fetch_monitors(&self) -> Result<Vec<Monitor>> {
        let bytes = self.send_message(I3_IPC_MESSAGE_GET_OUTPUTS, b"")?;
        #[derive(serde::Deserialize)]
        struct SwayRect {
            #[serde(default)]
            x: i32,
            #[serde(default)]
            y: i32,
            #[serde(default)]
            width: i32,
            #[serde(default)]
            height: i32,
        }

        #[derive(serde::Deserialize)]
        struct SwayOutput {
            name: String,
            #[serde(default)]
            make: String,
            #[serde(default)]
            model: String,
            #[serde(default)]
            active: bool,
            #[serde(default)]
            primary: bool,
            #[serde(default)]
            current_workspace: Option<String>,
            #[serde(default)]
            scale: f64,
            #[serde(default)]
            rect: Option<SwayRect>,
            #[serde(default)]
            focused: bool,
        }

        let outputs: Vec<SwayOutput> = serde_json::from_slice(&bytes)?;
        // Sway does not flag the focused output in `get_outputs`; the focused
        // workspace knows which output it lives on.
        let focused_output = self
            .fetch_workspaces()
            .ok()
            .and_then(|workspaces| {
                workspaces
                    .into_iter()
                    .find(|workspace| workspace.is_active)
                    .map(|workspace| workspace.monitor)
            });

        Ok(outputs
            .into_iter()
            .map(|output| {
                let description = format!("{} {}", output.make, output.model)
                    .trim()
                    .to_string();
                Monitor {
                    id: 0,
                    name: output.name.clone(),
                    description,
                    width: output.rect.as_ref().map(|r| r.width).unwrap_or(0),
                    height: output.rect.as_ref().map(|r| r.height).unwrap_or(0),
                    refresh_rate: 0.0,
                    x: output.rect.as_ref().map(|r| r.x).unwrap_or(0),
                    y: output.rect.as_ref().map(|r| r.y).unwrap_or(0),
                    scale: if output.scale > 0.0 { output.scale } else { 1.0 },
                    focused: focused_output
                        .as_deref()
                        .map(|name| name == output.name)
                        .unwrap_or(output.focused || output.primary),
                    disabled: !output.active,
                    active_workspace: output.current_workspace.clone().unwrap_or_default(),
                    active_workspace_name: output.current_workspace.unwrap_or_default(),
                    special_workspace: None,
                    special_workspace_name: None,
                }
            })
            .collect())
    }

    /// Start listening for Sway workspace, window and output events.
    ///
    /// Sway multiplexes every subscription on one socket, and workspace and
    /// window events both carry a `change` field. A `window` focus event also
    /// has a `current`, so filtering on the object type is mandatory: decoding a
    /// window event as a workspace event yields a bogus activation with
    /// `num = 0`.
    ///
    /// The `init` / `empty` changes are what create and remove workspaces. The
    /// listener cannot patch its own copy of the list incrementally without
    /// replaying ordering, so it re-asks the compositor for the authoritative
    /// list whenever the set of workspaces may have changed.
    pub fn start_event_loop(&self, tx: async_channel::Sender<CompositorEvent>) -> Result<()> {
        let socket_path = self.socket_path.clone();

        std::thread::Builder::new()
            .name("kumizo-sway-events".into())
            .spawn(move || {
                let mut backoff = Duration::from_millis(500);

                while !tx.is_closed() {
                    let started = Instant::now();
                    let result = SwayIpc::new(socket_path.clone())
                        .ok_or_else(|| anyhow::anyhow!("Sway socket disappeared"))
                        .and_then(|client| client.listen_once(&tx));
                    if let Err(err) = result {
                        warn!("Kumizo: Sway event socket: {err}");
                    }

                    if tx.is_closed() {
                        break;
                    }
                    // Reset the backoff after a healthy session; otherwise keep
                    // doubling it so a dead compositor cannot spin.
                    if started.elapsed() >= Duration::from_secs(5) {
                        backoff = Duration::from_millis(500);
                    }
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            })
            .context("spawn Sway event listener")?;

        Ok(())
    }

    /// Connect, subscribe and stream events until the socket closes.
    fn listen_once(&self, tx: &async_channel::Sender<CompositorEvent>) -> Result<()> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        let sub_payload = serde_json::to_vec(&vec!["workspace", "window", "output"])?;

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

        // Catch up on anything that changed while disconnected.
        self.emit_full_sync(tx);

        let mut hdr = [0u8; 14];
        loop {
            // `read_exact` on a shared buffer would leave partial data
            // behind when a read is interrupted; start from a clean
            // header each time.
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
                kind: Option<String>,
                num: Option<i32>,
                id: Option<i64>,
                urgent: Option<bool>,
                focused: Option<bool>,
            }

            let Ok(ev) = serde_json::from_slice::<SwayEvent>(&body) else {
                continue;
            };

            // Window events carry container; handle creation/close/focus/title
            if let Some(container) = ev.container.as_ref() {
                let win_change = ev.change.as_deref().unwrap_or("");
                if matches!(
                    win_change,
                    "new" | "close" | "focus" | "title" | "mark" | "move" | "urgent" | "floating"
                ) {
                    if let Some(client) = SwayIpc::new(socket_path.clone()) {
                        if let Ok(windows) = client.fetch_windows() {
                            if let Some(focused) =
                                windows.iter().find(|window| window.is_focused)
                            {
                                let _ = tx.send_blocking(CompositorEvent::WindowFocused(
                                    ActiveWindow {
                                        title: focused.title.clone(),
                                        class: focused.class.clone(),
                                        workspace_id: focused.workspace_id,
                                    },
                                ));
                            } else if win_change == "close" || win_change == "focus" {
                                let _ =
                                    tx.send_blocking(CompositorEvent::WindowFocusCleared);
                            }
                            let _ = tx
                                .send_blocking(CompositorEvent::WindowsChanged(windows));
                        }
                    }
                }
                if win_change == "urgent" {
                    if let (Some(id), Some(true)) =
                        (container.id, container.urgent.or(container.focused))
                    {
                        let _ =
                            tx.send_blocking(CompositorEvent::WindowUrgent(id));
                    }
                }
                continue;
            }

            // Output hotplug / mode change.
            if ev.current.is_none()
                && ev.change.as_deref().is_some()
                && matches!(
                    ev.change.as_deref(),
                    Some("unspecified")
                        | Some("output")
                        | Some("added")
                        | Some("removed")
                )
            {
                if let Some(client) = SwayIpc::new(socket_path.clone()) {
                    if let Ok(monitors) = client.fetch_monitors() {
                        let _ = tx.send_blocking(CompositorEvent::MonitorsChanged(monitors));
                    }
                }
            }

            let change = ev.change.as_deref().unwrap_or("");
            let Some(current) = ev.current.as_ref() else {
                continue;
            };
            if current.kind.as_deref() != Some("workspace") {
                continue;
            }

            // `init` and `empty` mean the workspace appeared or
            // disappeared. `move` changes its output, `rename` its name,
            // `urgent` its hint and `reload` may renumber everything.
            // All of them are easiest to handle with an authoritative
            // re-query.
            let structure_changed = matches!(
                change,
                "init" | "empty" | "destroy" | "move" | "rename" | "urgent" | "reload"
            );

            if structure_changed {
                if let Some(client) = SwayIpc::new(socket_path.clone()) {
                    if let Ok(list) = client.fetch_workspaces() {
                        let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(list));
                    }
                    if let Ok(windows) = client.fetch_windows() {
                        let _ =
                            tx.send_blocking(CompositorEvent::WindowsChanged(windows));
                    }
                }
            }

            // The `change` field is the authority on what happened, not
            // the `focused` flag on the node snapshot: Sway observably
            // emits `focus` with `focused: false` when the switch also
            // destroys a workspace (captured live: `focus` 1 with
            // `focused=false` followed by `empty` 8). Requiring the flag
            // drops the activation and leaves no workspace highlighted.
            if change == "focus" {
                if let Some(num) = current.num.filter(|num| *num >= 0) {
                    let _ = tx.send_blocking(CompositorEvent::WorkspaceActivated(num));
                } else if let Some(client) = SwayIpc::new(socket_path.clone()) {
                    // Named workspaces have no number; the authoritative
                    // list carries the `focused` flag we need.
                    if let Ok(list) = client.fetch_workspaces() {
                        let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(list));
                    }
                }
            }
        }

        Ok(())
    }

    /// Push an authoritative snapshot of workspaces, windows and monitors.
    ///
    /// Called after every (re)connection so state that changed while the event
    /// stream was down is picked up instead of being silently missed.
    fn emit_full_sync(&self, tx: &async_channel::Sender<CompositorEvent>) {
        if let Ok(workspaces) = self.fetch_workspaces() {
            let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(workspaces));
        }
        if let Ok(windows) = self.fetch_windows() {
            if let Some(focused) = windows.iter().find(|window| window.is_focused) {
                let _ = tx.send_blocking(CompositorEvent::WindowFocused(ActiveWindow {
                    title: focused.title.clone(),
                    class: focused.class.clone(),
                    workspace_id: focused.workspace_id,
                }));
            } else {
                let _ = tx.send_blocking(CompositorEvent::WindowFocusCleared);
            }
            let _ = tx.send_blocking(CompositorEvent::WindowsChanged(windows));
        }
        if let Ok(monitors) = self.fetch_monitors() {
            let _ = tx.send_blocking(CompositorEvent::MonitorsChanged(monitors));
        }
    }
}

/// Quote a Sway command argument. Sway parses quoted strings with backslash
/// escapes, so embedded quotes must not break the command framing.
fn sway_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Stable negative id for named workspaces (`num = -1` in Sway).
fn synthetic_workspace_id(name: &str) -> i32 {
    if name.is_empty() {
        return -1;
    }

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    -((hash & 0x3fff_ffff) as i32 + 1)
}
