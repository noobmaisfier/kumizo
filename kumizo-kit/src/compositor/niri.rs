//! Niri compositor backend — direct IPC over `$NIRI_SOCKET`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use niri_ipc::{
    socket::Socket, Action, Event, Reply, Request, Response, WorkspaceReferenceArg,
};

use crate::compositor::types::{
    ActiveWindow, AppWindow, CompositorEvent, Monitor, Workspace,
};

pub struct NiriIpc {
    socket_path: PathBuf,
    cmd_socket: Arc<Mutex<Option<Socket>>>,
}

impl NiriIpc {
    pub fn try_new() -> Option<Self> {
        let raw = std::env::var_os("NIRI_SOCKET")?;
        let path = PathBuf::from(raw);
        if !path.exists() {
            return None;
        }
        Some(Self {
            socket_path: path,
            cmd_socket: Arc::new(Mutex::new(None)),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn send_request(&self, request: Request) -> Result<Response> {
        let mut guard = self.cmd_socket.lock().unwrap();

        if let Some(sock) = guard.as_mut() {
            match sock.send(request.clone()) {
                Ok(reply) => {
                    let reply: Reply = reply;
                    return reply.map_err(|e| anyhow::anyhow!("Niri error: {e}"));
                }
                Err(_) => *guard = None,
            }
        }

        let mut sock = Socket::connect_to(&self.socket_path)
            .context("connect to Niri command socket")?;
        let reply: Reply = sock.send(request).context("Niri socket send")?;
        *guard = Some(sock);
        reply.map_err(|e| anyhow::anyhow!("Niri error: {e}"))
    }

    fn send_action(&self, action: Action) -> Result<()> {
        match self.send_request(Request::Action(action))? {
            Response::Handled => Ok(()),
            other => anyhow::bail!("unexpected action reply: {other:?}"),
        }
    }

    pub fn switch_workspace(&self, idx: i32) -> Result<()> {
        if idx < 0 {
            anyhow::bail!("invalid workspace index: {idx}");
        }
        self.send_action(Action::FocusWorkspace {
            reference: WorkspaceReferenceArg::Index(idx as u8),
        })
    }

    pub fn switch_workspace_address(&self, address: &str) -> Result<()> {
        self.send_action(Action::FocusWorkspace {
            reference: parse_workspace_ref(address),
        })
    }

    pub fn focus_window(&self, id: i64) -> Result<()> {
        self.send_action(Action::FocusWindow { id: id as u64 })
    }

    pub fn close_window(&self) -> Result<()> {
        self.send_action(Action::CloseWindow { id: None })
    }

    pub fn close_window_id(&self, id: i64) -> Result<()> {
        self.send_action(Action::CloseWindow { id: Some(id as u64) })
    }

    pub fn move_window_to_workspace(
        &self,
        window_id: Option<i64>,
        workspace: &str,
        follow: bool,
    ) -> Result<()> {
        self.send_action(Action::MoveWindowToWorkspace {
            window_id: window_id.map(|i| i as u64),
            reference: parse_workspace_ref(workspace),
            focus: follow,
        })
    }

    pub fn focus_monitor(&self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("empty monitor name");
        }
        self.send_action(Action::FocusMonitor {
            output: name.to_string(),
        })
    }

    pub fn toggle_floating(&self, window_id: Option<i64>) -> Result<()> {
        self.send_action(Action::ToggleWindowFloating {
            id: window_id.map(|i| i as u64),
        })
    }

    pub fn toggle_fullscreen(&self, window_id: Option<i64>) -> Result<()> {
        self.send_action(Action::FullscreenWindow {
            id: window_id.map(|i| i as u64),
        })
    }

    pub fn reload_config(&self) -> Result<()> {
        self.send_action(Action::LoadConfigFile {})
    }

    pub fn fetch_workspaces(&self) -> Result<Vec<Workspace>> {
        let reply = self.send_request(Request::Workspaces)?;
        let raw = match reply {
            Response::Workspaces(ws) => ws,
            other => anyhow::bail!("unexpected Workspaces reply: {other:?}"),
        };
        Ok(raw.into_iter().map(workspace_from_niri).collect())
    }

    pub fn fetch_windows(&self, workspaces: &[Workspace]) -> Result<Vec<AppWindow>> {
        let reply = self.send_request(Request::Windows)?;
        let raw = match reply {
            Response::Windows(ws) => ws,
            other => anyhow::bail!("unexpected Windows reply: {other:?}"),
        };
        let names: HashMap<i32, &str> =
            workspaces.iter().map(|w| (w.id, w.name.as_str())).collect();
        Ok(raw
            .into_iter()
            .map(|w| window_from_niri(w, &names))
            .collect())
    }

    pub fn fetch_windows_default(&self) -> Result<Vec<AppWindow>> {
        let workspaces = self.fetch_workspaces().unwrap_or_default();
        self.fetch_windows(&workspaces)
    }

    pub fn fetch_monitors(&self, workspaces: &[Workspace]) -> Result<Vec<Monitor>> {
        let active_by_output: HashMap<String, (u64, String)> = workspaces
            .iter()
            .filter(|w| w.is_active && !w.monitor.is_empty())
            .map(|w| (w.monitor.clone(), (w.id as u64, w.name.clone())))
            .collect();

        let reply = self.send_request(Request::Outputs)?;
        let outputs = match reply {
            Response::Outputs(map) => map,
            other => anyhow::bail!("unexpected Outputs reply: {other:?}"),
        };

        Ok(outputs
            .into_values()
            .map(|o| monitor_from_niri(o, &active_by_output))
            .collect())
    }

    pub fn fetch_monitors_default(&self) -> Result<Vec<Monitor>> {
        let workspaces = self.fetch_workspaces().unwrap_or_default();
        self.fetch_monitors(&workspaces)
    }

    pub fn fetch_active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(self.fetch_windows_default()?.into_iter().find_map(|window| {
            window.is_focused.then_some(ActiveWindow {
                title: window.title,
                class: window.class,
                workspace_id: window.workspace_id,
            })
        }))
    }

    pub fn fetch_state(&self) -> Result<(Vec<Workspace>, Vec<AppWindow>, Vec<Monitor>)> {
        let mut workspaces = self.fetch_workspaces()?;
        let windows = self.fetch_windows(&workspaces)?;
        merge_window_counts(&mut workspaces, &windows);
        let monitors = self.fetch_monitors(&workspaces)?;
        Ok((workspaces, windows, monitors))
    }

    pub fn start_event_loop(&self, tx: async_channel::Sender<CompositorEvent>) -> Result<()> {
        let socket_path = self.socket_path.clone();
        let state_fetcher = NiriIpc {
            socket_path: socket_path.clone(),
            cmd_socket: self.cmd_socket.clone(),
        };

        thread::Builder::new()
            .name("kumizo-niri-events".into())
            .spawn(move || {
                let mut backoff = Duration::from_millis(500);
                let mut current_workspaces: Vec<Workspace> = Vec::new();
                let mut current_windows: Vec<AppWindow> = Vec::new();

                loop {
                    if tx.is_closed() {
                        return;
                    }

                    if let Ok((ws, wins, monitors)) = state_fetcher.fetch_state() {
                        current_workspaces = ws.clone();
                        current_windows = wins.clone();
                        let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(ws));
                        let _ = tx.send_blocking(CompositorEvent::WindowsChanged(wins));
                        let _ = tx.send_blocking(CompositorEvent::MonitorsChanged(monitors));
                    }

                    let started = Instant::now();

                    let mut sock = match Socket::connect_to(&socket_path) {
                        Ok(s) => s,
                        Err(_) => {
                            thread::sleep(backoff);
                            backoff = (backoff * 2).min(Duration::from_secs(30));
                            continue;
                        }
                    };
                    if sock.send(Request::EventStream).is_err() {
                        thread::sleep(backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                        continue;
                    }

                    let mut read_event = sock.read_events();
                    loop {
                        if tx.is_closed() {
                            return;
                        }
                        match read_event() {
                            Ok(event) => {
                                dispatch_event(
                                    event,
                                    &state_fetcher,
                                    &tx,
                                    &mut current_workspaces,
                                    &mut current_windows,
                                );
                            }
                            Err(_) => break,
                        }
                    }

                    if started.elapsed() >= Duration::from_secs(10) {
                        backoff = Duration::from_millis(500);
                    }
                    thread::sleep(backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            })
            .context("spawn Niri event listener")?;

        Ok(())
    }
}

fn dispatch_event(
    event: Event,
    state_fetcher: &NiriIpc,
    tx: &async_channel::Sender<CompositorEvent>,
    current_workspaces: &mut Vec<Workspace>,
    current_windows: &mut Vec<AppWindow>,
) {
    match event {
        Event::WorkspacesChanged { workspaces } => {
            let mut ws: Vec<Workspace> = workspaces.into_iter().map(workspace_from_niri).collect();
            merge_window_counts(&mut ws, current_windows);
            *current_workspaces = ws.clone();
            let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(ws));
        }
        Event::WorkspaceActivated { id, .. } => {
            let _ = tx.send_blocking(CompositorEvent::WorkspaceActivated(id as i32));
        }
        Event::WindowsChanged { windows } => {
            let win = rebuild_windows(windows, current_workspaces);
            *current_windows = win.clone();
            let _ = tx.send_blocking(CompositorEvent::WindowsChanged(win));
            push_workspace_counts(tx, current_workspaces, current_windows);
        }
        Event::WindowOpenedOrChanged { window } => {
            let w = {
                let names: HashMap<i32, &str> = current_workspaces
                    .iter()
                    .map(|ws| (ws.id, ws.name.as_str()))
                    .collect();
                window_from_niri(window, &names)
            };
            if let Some(existing) = current_windows.iter_mut().find(|x| x.id == w.id) {
                *existing = w.clone();
            } else {
                current_windows.push(w.clone());
            }
            let _ = tx.send_blocking(CompositorEvent::WindowOpenedOrChanged(w.clone()));
            let _ = tx.send_blocking(CompositorEvent::WindowsChanged(current_windows.clone()));
            push_workspace_counts(tx, current_workspaces, current_windows);
        }
        Event::WindowClosed { id } => {
            current_windows.retain(|w| w.id != id as i64);
            let _ = tx.send_blocking(CompositorEvent::WindowClosed(id as i64));
            let _ = tx.send_blocking(CompositorEvent::WindowsChanged(current_windows.clone()));
            push_workspace_counts(tx, current_workspaces, current_windows);
        }
        Event::WindowFocusChanged { id } => match id {
            Some(id) => {
                for w in current_windows.iter_mut() {
                    w.is_focused = w.id == id as i64;
                }
                if let Some(w) = current_windows.iter().find(|w| w.id == id as i64) {
                    let active = ActiveWindow {
                        title: w.title.clone(),
                        class: w.class.clone(),
                        workspace_id: w.workspace_id,
                    };
                    let _ = tx.send_blocking(CompositorEvent::WindowFocused(active));
                }
                let _ = tx.send_blocking(CompositorEvent::WindowsChanged(current_windows.clone()));
            }
            None => {
                for w in current_windows.iter_mut() {
                    w.is_focused = false;
                }
                let _ = tx.send_blocking(CompositorEvent::WindowFocusCleared);
                let _ = tx.send_blocking(CompositorEvent::WindowsChanged(current_windows.clone()));
            }
        },
        Event::ConfigLoaded { .. } => {
            if let Ok(monitors) = state_fetcher.fetch_monitors(current_workspaces) {
                let _ = tx.send_blocking(CompositorEvent::MonitorsChanged(monitors));
            }
            if let Ok((ws, wins, _)) = state_fetcher.fetch_state() {
                *current_workspaces = ws.clone();
                *current_windows = wins.clone();
                let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(ws));
                let _ = tx.send_blocking(CompositorEvent::WindowsChanged(wins));
            }
        }
        _ => {}
    }
}

fn push_workspace_counts(
    tx: &async_channel::Sender<CompositorEvent>,
    current_workspaces: &[Workspace],
    current_windows: &[AppWindow],
) {
    let mut ws = current_workspaces.to_vec();
    merge_window_counts(&mut ws, current_windows);
    let _ = tx.send_blocking(CompositorEvent::WorkspacesChanged(ws));
}

fn parse_workspace_ref(s: &str) -> WorkspaceReferenceArg {
    let s = s.trim();
    if s.is_empty() {
        return WorkspaceReferenceArg::Index(1);
    }
    if let Ok(idx) = s.parse::<u8>() {
        WorkspaceReferenceArg::Index(idx)
    } else {
        WorkspaceReferenceArg::Name(s.to_string())
    }
}

fn workspace_from_niri(w: niri_ipc::Workspace) -> Workspace {
    let name = w.name.clone().unwrap_or_else(|| w.idx.to_string());
    Workspace {
        id: w.id as i32,
        address: w.idx.to_string(),
        name,
        is_active: w.is_active,
        is_urgent: w.is_urgent,
        window_count: 0,
        monitor: w.output.unwrap_or_else(|| "default".into()),
    }
}

fn window_from_niri(w: niri_ipc::Window, ws_names: &HashMap<i32, &str>) -> AppWindow {
    let workspace_id = w.workspace_id.map(|id| id as i32).unwrap_or(0);
    let workspace_name = ws_names
        .get(&workspace_id)
        .copied()
        .unwrap_or("")
        .to_string();
    AppWindow {
        id: w.id as i64,
        title: w.title.unwrap_or_default(),
        class: w.app_id.unwrap_or_default(),
        workspace_id,
        workspace_name,
        is_focused: w.is_focused,
    }
}

fn rebuild_windows(
    windows: Vec<niri_ipc::Window>,
    workspaces: &[Workspace],
) -> Vec<AppWindow> {
    let names: HashMap<i32, &str> =
        workspaces.iter().map(|w| (w.id, w.name.as_str())).collect();
    windows
        .into_iter()
        .map(|w| window_from_niri(w, &names))
        .collect()
}

fn monitor_from_niri(
    o: niri_ipc::Output,
    active_by_output: &HashMap<String, (u64, String)>,
) -> Monitor {
    let (active_workspace, active_workspace_name) = active_by_output
        .get(&o.name)
        .cloned()
        .unwrap_or((0, String::new()));

    let (width, height, x, y, scale) = o
        .logical
        .as_ref()
        .map(|l| (l.width as i32, l.height as i32, l.x, l.y, l.scale))
        .unwrap_or((0, 0, 0, 0, 1.0));

    let refresh_rate = o
        .current_mode
        .as_ref()
        .and_then(|idx| o.modes.get(*idx))
        .map(|m| m.refresh_rate as f64 / 1000.0)
        .unwrap_or(0.0);

    let description = format!("{} {} {}", o.make, o.model, o.serial.as_deref().unwrap_or(""))
        .trim()
        .to_string();

    Monitor {
        id: 0,
        name: o.name.clone(),
        description,
        width,
        height,
        refresh_rate,
        x,
        y,
        scale,
        focused: false,
        disabled: o.current_mode.is_none(),
        active_workspace: active_workspace.to_string(),
        active_workspace_name,
        special_workspace: None,
        special_workspace_name: None,
    }
}

pub fn merge_window_counts(workspaces: &mut [Workspace], windows: &[AppWindow]) {
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for w in windows {
        *counts.entry(w.workspace_id).or_default() += 1;
    }
    for ws in workspaces {
        ws.window_count = *counts.get(&ws.id).unwrap_or(&0);
    }
}
