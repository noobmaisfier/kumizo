use gpui::Context;
use tracing::{info, warn};
use wk_services::Service;
use crate::hyprland::HyprlandIpc;
use crate::niri::NiriIpc;
use crate::sway::SwayIpc;
use crate::types::{ActiveWindow, AppWindow, CompositorEvent, CompositorKind, Workspace};

/// Asynchronous compositor state service.
///
/// Automatically detects Hyprland, Sway, or Niri. Manages active workspaces,
/// focused windows, and window management commands with Zero Idle CPU.
pub struct Compositor {
    kind: CompositorKind,
    workspaces: Vec<Workspace>,
    active_workspace_id: i32,
    active_window: Option<ActiveWindow>,
    windows: Vec<AppWindow>,
}

impl Compositor {
    /// Create a new compositor service instance with fallback defaults.
    pub fn new() -> Self {
        let default_workspaces = (1..=5)
            .map(|i| Workspace::new(i, i.to_string(), i == 1))
            .collect();

        Self {
            kind: CompositorKind::Fallback,
            workspaces: default_workspaces,
            active_workspace_id: 1,
            active_window: None,
            windows: Vec::new(),
        }
    }

    /// Detected compositor kind.
    pub fn kind(&self) -> CompositorKind {
        self.kind
    }

    /// List of current workspaces.
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Currently active workspace ID.
    pub fn active_workspace_id(&self) -> i32 {
        self.active_workspace_id
    }

    /// Currently active window title and class (if any).
    pub fn active_window(&self) -> Option<&ActiveWindow> {
        self.active_window.as_ref()
    }

    /// Switch workspace by ID.
    ///
    /// The highlight is updated optimistically on every backend. Waiting for the
    /// compositor to echo a `focus` event would leave the click feeling dead,
    /// since the round trip includes a compositor round trip; if the command
    /// fails or the workspace does not exist, the next event corrects it.
    pub fn switch_workspace(&mut self, id: i32) {
        let dispatched = match self.kind {
            CompositorKind::Hyprland => HyprlandIpc::try_new()
                .map(|hypr| hypr.switch_workspace(id).is_ok())
                .unwrap_or(false),
            CompositorKind::Sway => SwayIpc::try_new()
                .map(|sway| sway.switch_workspace(id).is_ok())
                .unwrap_or(false),
            CompositorKind::Niri => NiriIpc::try_new()
                .map(|niri| niri.switch_workspace(id).is_ok())
                .unwrap_or(false),
            // Nothing to dispatch to; the local update below is all there is.
            CompositorKind::Fallback => true,
        };

        if dispatched {
            self.active_workspace_id = id;
            for w in &mut self.workspaces {
                w.is_active = w.id == id;
            }
        }
    }

    /// List of all currently open application windows.
    pub fn windows(&self) -> &[AppWindow] {
        &self.windows
    }

    /// Focus an application window by its container/node ID.
    pub fn focus_window(&self, con_id: i64) {
        if self.kind == CompositorKind::Sway {
            if let Some(sway) = SwayIpc::try_new() {
                let _ = sway.focus_window(con_id);
            }
        }
    }

    /// Close the active window.
    pub fn close_window(&self) {
        if self.kind == CompositorKind::Hyprland {
            if let Some(hypr) = HyprlandIpc::try_new() {
                let _ = hypr.close_window();
            }
        }
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Service for Compositor {
    fn init(cx: &mut Context<Self>) -> Self {
        let mut comp = Compositor::new();
        let (tx, rx) = async_channel::bounded::<CompositorEvent>(20);

        // Detect backend
        if let Some(hypr) = HyprlandIpc::try_new() {
            info!("Kumizo: Connected to Hyprland IPC");
            comp.kind = CompositorKind::Hyprland;
            if let Ok(ws) = hypr.fetch_workspaces() {
                if !ws.is_empty() {
                    comp.workspaces = ws;
                }
            }
            if let Ok(act) = hypr.fetch_active_workspace_id() {
                comp.active_workspace_id = act;
            }
            if let Ok(win) = hypr.fetch_active_window() {
                comp.active_window = win;
            }
            let _ = hypr.start_event_loop(tx);
        } else if let Some(sway) = SwayIpc::try_new() {
            info!("Kumizo: Connected to Sway IPC");
            comp.kind = CompositorKind::Sway;
            if let Ok(ws) = sway.fetch_workspaces() {
                if !ws.is_empty() {
                    comp.workspaces = ws;
                }
            }
            if let Ok(wins) = sway.fetch_windows() {
                comp.windows = wins;
            }
            let _ = sway.start_event_loop(tx);
        } else if let Some(niri) = NiriIpc::try_new() {
            info!("Kumizo: Connected to Niri IPC");
            comp.kind = CompositorKind::Niri;
            if let Ok(ws) = niri.fetch_workspaces() {
                if !ws.is_empty() {
                    comp.workspaces = ws;
                }
            }
            let _ = niri.start_event_loop(tx);
        } else {
            warn!("Kumizo: No supported Wayland compositor IPC socket found. Using fallback mock workspaces.");
        }

        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                let res = this.update(cx, |comp, cx| {
                    match event {
                        CompositorEvent::WorkspaceActivated(id) => {
                            comp.active_workspace_id = id;
                            for w in &mut comp.workspaces {
                                w.is_active = w.id == id;
                            }
                            cx.notify();
                        }
                        // A workspace was created, removed or moved: replace the
                        // list outright rather than patching it, so a stale entry
                        // can never survive. The fetched flags come from a
                        // synchronous GET_WORKSPACES, so they are the
                        // compositor's truth at fetch time; adopt them and sync
                        // the active id. Only when nothing is marked (transient
                        // fetch mid-switch) keep the previous active id, so the
                        // highlight flickers rather than vanishes.
                        CompositorEvent::WorkspacesChanged(mut workspaces) => {
                            if !workspaces.is_empty() {
                                if let Some(focused) = workspaces.iter().find(|w| w.is_active) {
                                    comp.active_workspace_id = focused.id;
                                } else {
                                    for w in &mut workspaces {
                                        w.is_active = w.id == comp.active_workspace_id;
                                    }
                                }
                                comp.workspaces = workspaces;
                                cx.notify();
                            }
                        }
                        CompositorEvent::WindowFocused(win) => {
                            comp.active_window = Some(win);
                            cx.notify();
                        }
                        CompositorEvent::WindowsChanged(windows) => {
                            comp.windows = windows;
                            cx.notify();
                        }
                        CompositorEvent::FullSync(ws, win) => {
                            comp.workspaces = ws;
                            comp.active_window = win;
                            cx.notify();
                        }
                    }
                });

                if res.is_err() {
                    break;
                }
            }
        })
        .detach();

        comp
    }
}
