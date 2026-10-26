use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::compositor::types::{ActiveWindow, AppWindow, CompositorEvent, Monitor, Workspace};

/// Niri scrollable tiling Wayland compositor IPC client.
///
/// Niri exposes commands through its `niri msg` CLI rather than a raw socket
/// protocol, so this backend shells out to it. Detection still uses
/// `NIRI_SOCKET` so a stale CLI cannot claim a different compositor.
pub struct NiriIpc;

impl NiriIpc {
    /// Detect if running under Niri.
    pub fn try_new() -> Option<Self> {
        let sock = std::env::var("NIRI_SOCKET").ok()?;
        PathBuf::from(sock).exists().then_some(Self)
    }

    /// Build a client from the event listener (the socket was already valid).
    fn current() -> Self {
        Self
    }

    fn command(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new("niri")
            .args(args)
            .output()
            .with_context(|| format!("run `niri {}`", args.join(" ")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "`niri {}` failed: {}",
                args.join(" "),
                stderr.trim()
            );
        }

        Ok(output.stdout)
    }

    fn action(&self, args: &[&str]) -> Result<()> {
        let mut full = vec!["msg", "action"];
        full.extend_from_slice(args);
        self.command(&full).map(|_| ())
    }

    /// Switch workspace using `niri msg action focus-workspace <idx-or-name>`.
    pub fn switch_workspace(&self, idx: i32) -> Result<()> {
        self.switch_workspace_address(&idx.to_string())
    }

    /// Switch to a workspace by index or name.
    pub fn switch_workspace_address(&self, address: &str) -> Result<()> {
        let address = address.trim();
        if address.is_empty() {
            bail!("empty workspace address");
        }
        self.action(&["focus-workspace", address])
    }

    /// Focus a window by its Niri window id.
    pub fn focus_window(&self, id: i64) -> Result<()> {
        self.action(&["focus-window", "--id", &id.to_string()])
    }

    /// Close the focused window.
    pub fn close_window(&self) -> Result<()> {
        self.action(&["close-window"])
    }

    /// Close a specific window by id.
    pub fn close_window_id(&self, id: i64) -> Result<()> {
        self.action(&["close-window", "--id", &id.to_string()])
    }

    /// Toggle floating state for a window (or the focused one).
    pub fn toggle_floating(&self, id: Option<i64>) -> Result<()> {
        match id {
            Some(id) => self.action(&["toggle-window-floating", "--id", &id.to_string()]),
            None => self.action(&["toggle-window-floating"]),
        }
    }

    /// Toggle fullscreen for a window (or the focused one).
    pub fn toggle_fullscreen(&self, id: Option<i64>) -> Result<()> {
        match id {
            Some(id) => self.action(&["fullscreen-window", "--id", &id.to_string()]),
            None => self.action(&["fullscreen-window"]),
        }
    }

    /// Move a window to another workspace.
    pub fn move_window_to_workspace(
        &self,
        id: Option<i64>,
        workspace: &str,
        follow: bool,
    ) -> Result<()> {
        let workspace = workspace.trim();
        if workspace.is_empty() {
            bail!("empty workspace address");
        }

        let id = id.map(|id| id.to_string());
        let mut args = vec!["move-window-to-workspace"];
        if let Some(id) = id.as_deref() {
            args.push("--window-id");
            args.push(id);
        }
        args.push("--focus");
        args.push(if follow { "true" } else { "false" });
        args.push(workspace);
        self.action(&args)
    }

    /// Focus an output by name.
    pub fn focus_monitor(&self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("empty monitor name");
        }
        self.action(&["focus-monitor", name])
    }

    /// Reload the Niri configuration.
    pub fn reload_config(&self) -> Result<()> {
        self.action(&["load-config-file"])
    }

    /// Fetch workspaces via `niri msg -j workspaces`.
    pub fn fetch_workspaces(&self) -> Result<Vec<Workspace>> {
        let output = self.command(&["msg", "-j", "workspaces"])?;

        #[derive(serde::Deserialize)]
        struct NiriWs {
            /// Stable unique id (u64 in niri); narrowed to the SDK's i32 id.
            id: u64,
            idx: i32,
            name: Option<String>,
            #[serde(default)]
            is_focused: bool,
            #[serde(default)]
            is_urgent: bool,
            output: Option<String>,
        }

        let items: Vec<NiriWs> = serde_json::from_slice(&output)?;
        let workspaces = items
            .into_iter()
            .map(|w| {
                // `id` is the stable unique workspace id and the value carried by
                // `WorkspaceActivated` events; `idx` is only its position on the
                // monitor. The SDK exposes an i32 id, so narrow the unique id and
                // keep the real switch reference (name or index) in `address`.
                let name = w.name.filter(|name| !name.is_empty());
                let address = name.clone().unwrap_or_else(|| w.idx.to_string());
                Workspace {
                    id: w.id as i32,
                    address: address.clone(),
                    name: name.unwrap_or(address),
                    // The SDK exposes one globally focused workspace; niri's
                    // `is_active` is per-output, while `is_focused` is global.
                    is_active: w.is_focused,
                    is_urgent: w.is_urgent,
                    window_count: 0,
                    monitor: w.output.unwrap_or_else(|| "default".into()),
                }
            })
            .collect();

        Ok(workspaces)
    }

    /// Fetch open windows via `niri msg -j windows`.
    pub fn fetch_windows(&self) -> Result<Vec<AppWindow>> {
        let output = self.command(&["msg", "-j", "windows"])?;

        #[derive(serde::Deserialize)]
        struct NiriWindow {
            id: i64,
            #[serde(default)]
            title: Option<String>,
            #[serde(default)]
            app_id: Option<String>,
            #[serde(default)]
            workspace_id: Option<i64>,
            #[serde(default)]
            is_focused: bool,
        }

        let items: Vec<NiriWindow> = serde_json::from_slice(&output)?;
        Ok(items
            .into_iter()
            .map(|window| {
                let workspace_id = window.workspace_id.unwrap_or(0) as i32;
                AppWindow {
                    id: window.id,
                    title: window.title.unwrap_or_default(),
                    class: window.app_id.unwrap_or_default(),
                    workspace_id,
                    workspace_name: if workspace_id == 0 {
                        String::new()
                    } else {
                        workspace_id.to_string()
                    },
                    is_focused: window.is_focused,
                }
            })
            .collect())
    }

    /// Fetch the focused window, or `None` when no window has focus.
    pub fn fetch_active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(self.fetch_windows()?.into_iter().find_map(|window| {
            window.is_focused.then_some(ActiveWindow {
                title: window.title,
                class: window.class,
                workspace_id: window.workspace_id,
            })
        }))
    }

    /// Fetch outputs via `niri msg -j outputs`.
    pub fn fetch_monitors(&self) -> Result<Vec<Monitor>> {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Logical {
            #[serde(default)]
            x: i32,
            #[serde(default)]
            y: i32,
            #[serde(default)]
            width: i32,
            #[serde(default)]
            height: i32,
            #[serde(default)]
            scale: f64,
        }

        #[derive(Deserialize)]
        struct Mode {
            #[serde(default)]
            width: i32,
            #[serde(default)]
            height: i32,
            #[serde(default)]
            refresh_rate: u32,
        }

        #[derive(Deserialize)]
        struct Output {
            #[serde(default)]
            name: String,
            #[serde(default)]
            make: String,
            #[serde(default)]
            model: String,
            #[serde(default)]
            serial: Option<String>,
            #[serde(default)]
            modes: Vec<Mode>,
            #[serde(default)]
            current_mode: Option<usize>,
            #[serde(default)]
            logical: Option<Logical>,
        }

        let output = self.command(&["msg", "-j", "outputs"])?;
        let outputs: std::collections::HashMap<String, Output> = serde_json::from_slice(&output)?;
        let focused_output = self
            .command(&["msg", "-j", "focused-output"])
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Option<Output>>(&bytes).ok())
            .flatten()
            .map(|output| output.name);

        Ok(outputs
            .into_values()
            .map(|output| {
                let logical = output.logical.as_ref();
                let mode = output
                    .current_mode
                    .and_then(|index| output.modes.get(index))
                    .or_else(|| output.modes.first());
                let description = format!("{} {}", output.make, output.model)
                    .trim()
                    .to_string();
                let description = match output.serial {
                    Some(serial) if !serial.trim().is_empty() => {
                        format!("{description} ({serial})")
                    }
                    _ => description,
                };

                Monitor {
                    id: 0,
                    name: output.name.clone(),
                    description,
                    width: logical
                        .map(|logical| logical.width)
                        .or_else(|| mode.map(|mode| mode.width))
                        .unwrap_or(0),
                    height: logical
                        .map(|logical| logical.height)
                        .or_else(|| mode.map(|mode| mode.height))
                        .unwrap_or(0),
                    refresh_rate: mode
                        .map(|mode| {
                            if mode.refresh_rate > 1_000 {
                                f64::from(mode.refresh_rate) / 1000.0
                            } else {
                                f64::from(mode.refresh_rate)
                            }
                        })
                        .unwrap_or(0.0),
                    x: logical.map(|logical| logical.x).unwrap_or(0),
                    y: logical.map(|logical| logical.y).unwrap_or(0),
                    scale: logical
                        .map(|logical| logical.scale)
                        .filter(|scale| *scale > 0.0)
                        .unwrap_or(1.0),
                    focused: focused_output.as_deref() == Some(output.name.as_str()),
                    disabled: output.current_mode.is_none(),
                    active_workspace: String::new(),
                    active_workspace_name: String::new(),
                    special_workspace: None,
                    special_workspace_name: None,
                }
            })
            .collect())
    }

    /// Start listening for Niri event stream `niri msg -j event-stream`.
    ///
    /// Niri sends complete snapshots (`WorkspacesChanged`, `WindowsChanged`) for
    /// structural changes and small deltas for focus. State-changing events
    /// trigger a fresh query, for the same reason the other backends do: a
    /// missed event must not leave a stale list behind.
    pub fn start_event_loop(&self, tx: async_channel::Sender<CompositorEvent>) -> Result<()> {
        std::thread::Builder::new()
            .name("kumizo-niri-events".into())
            .spawn(move || {
                let mut backoff = std::time::Duration::from_millis(500);

                loop {
                    if tx.is_closed() {
                        break;
                    }

                    let started = std::time::Instant::now();
                    let mut child = match Command::new("niri")
                        .args(["msg", "-j", "event-stream"])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(child) => child,
                        Err(_) => {
                            std::thread::sleep(backoff);
                            backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
                            continue;
                        }
                    };

                    if let Some(stdout) = child.stdout.take() {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if tx.is_closed() {
                                break;
                            }
                            let Ok(line) = line else { break };
                            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                                continue;
                            };
                            let Some((event, data)) = value.as_object().and_then(|object| {
                                object.iter().next().map(|(key, value)| (key.as_str(), value))
                            }) else {
                                continue;
                            };

                            let ipc = NiriIpc::current();
                            match event {
                                "WorkspaceActivated" => {
                                    let focused = data
                                        .get("focused")
                                        .and_then(|focused| focused.as_bool())
                                        .unwrap_or(true);
                                    if focused {
                                        if let Some(id) =
                                            data.get("id").and_then(|id| id.as_i64())
                                        {
                                            let _ = tx.send_blocking(
                                                CompositorEvent::WorkspaceActivated(id as i32),
                                            );
                                        }
                                    }
                                }
                                "WorkspaceUrgencyChanged" | "WorkspacesChanged" => {
                                    if let Ok(workspaces) = ipc.fetch_workspaces() {
                                        let _ = tx.send_blocking(
                                            CompositorEvent::WorkspacesChanged(workspaces),
                                        );
                                    }
                                    // Niri has no output-list event, but a
                                    // workspace change is the closest signal
                                    // that the output layout may have moved.
                                    if let Ok(monitors) = ipc.fetch_monitors() {
                                        let _ = tx.send_blocking(
                                            CompositorEvent::MonitorsChanged(monitors),
                                        );
                                    }
                                }
                                "WindowOpenedOrChanged"
                                | "WindowClosed"
                                | "WindowFocusChanged"
                                | "WindowUrgencyChanged"
                                | "WindowsChanged" => {
                                    if let Ok(windows) = ipc.fetch_windows() {
                                        if let Some(focused) =
                                            windows.iter().find(|window| window.is_focused)
                                        {
                                            let _ = tx.send_blocking(
                                                CompositorEvent::WindowFocused(ActiveWindow {
                                                    title: focused.title.clone(),
                                                    class: focused.class.clone(),
                                                    workspace_id: focused.workspace_id,
                                                }),
                                            );
                                        } else {
                                            let _ = tx.send_blocking(
                                                CompositorEvent::WindowFocusCleared,
                                            );
                                        }
                                        let _ =
                                            tx.send_blocking(CompositorEvent::WindowsChanged(windows));
                                    }
                                }
                                // A config reload may change output configuration
                                // (niri has no dedicated output event).
                                "ConfigLoaded" => {
                                    if let Ok(monitors) = ipc.fetch_monitors() {
                                        let _ = tx
                                            .send_blocking(CompositorEvent::MonitorsChanged(monitors));
                                    }
                                    if let Ok(workspaces) = ipc.fetch_workspaces() {
                                        let _ = tx.send_blocking(
                                            CompositorEvent::WorkspacesChanged(workspaces),
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    let _ = child.wait();

                    if tx.is_closed() {
                        break;
                    }

                    // Reset the backoff after a healthy connection; otherwise
                    // keep doubling it so a missing `niri` binary cannot spin.
                    if started.elapsed() >= std::time::Duration::from_secs(5) {
                        backoff = std::time::Duration::from_millis(500);
                    }
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
                }
            })
            .context("spawn Niri event listener")?;

        Ok(())
    }
}
