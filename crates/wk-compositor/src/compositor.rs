use gpui::Context;
use tracing::{info, warn};
use wk_services::Service;
use crate::hyprland::HyprlandIpc;
use crate::niri::NiriIpc;
use crate::sway::SwayIpc;
use crate::types::{ActiveWindow, CompositorEvent, CompositorKind, Workspace};

/// Asynchronous compositor state service.
///
/// Automatically detects Hyprland, Sway, or Niri. Manages active workspaces,
/// focused windows, and window management commands with Zero Idle CPU.
pub struct Compositor {
    kind: CompositorKind,
    workspaces: Vec<Workspace>,
    active_workspace_id: i32,
    active_window: Option<ActiveWindow>,
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
    pub fn switch_workspace(&mut self, id: i32) {
        match self.kind {
            CompositorKind::Hyprland => {
                if let Some(hypr) = HyprlandIpc::try_new() {
                    let _ = hypr.switch_workspace(id);
                }
            }
            CompositorKind::Sway => {
                if let Some(sway) = SwayIpc::try_new() {
                    let _ = sway.switch_workspace(id);
                }
            }
            CompositorKind::Niri => {
                if let Some(niri) = NiriIpc::try_new() {
                    let _ = niri.switch_workspace(id);
                }
            }
            CompositorKind::Fallback => {
                // Update local state for testing
                self.active_workspace_id = id;
                for w in &mut self.workspaces {
                    w.is_active = w.id == id;
                }
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
            info!("WidgetKit: Connected to Hyprland IPC");
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
            info!("WidgetKit: Connected to Sway IPC");
            comp.kind = CompositorKind::Sway;
            if let Ok(ws) = sway.fetch_workspaces() {
                if !ws.is_empty() {
                    comp.workspaces = ws;
                }
            }
            let _ = sway.start_event_loop(tx);
        } else if let Some(niri) = NiriIpc::try_new() {
            info!("WidgetKit: Connected to Niri IPC");
            comp.kind = CompositorKind::Niri;
            if let Ok(ws) = niri.fetch_workspaces() {
                if !ws.is_empty() {
                    comp.workspaces = ws;
                }
            }
            let _ = niri.start_event_loop(tx);
        } else {
            warn!("WidgetKit: No supported Wayland compositor IPC socket found. Using fallback mock workspaces.");
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
                        CompositorEvent::WindowFocused(win) => {
                            comp.active_window = Some(win);
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
