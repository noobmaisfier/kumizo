use serde::{Deserialize, Serialize};

/// Wayland workspace descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Numerical or logical identifier of the workspace.
    pub id: i32,
    /// Display name or label (e.g. "1", "2:web", "term").
    pub name: String,
    /// Whether this workspace currently has focus on its monitor.
    pub is_active: bool,
    /// Whether this workspace contains an urgent window.
    pub is_urgent: bool,
    /// Number of managed windows in this workspace.
    pub window_count: usize,
    /// Output/Monitor name where this workspace resides.
    pub monitor: String,
}

impl Workspace {
    /// Create a new workspace.
    pub fn new(id: i32, name: impl Into<String>, is_active: bool) -> Self {
        Self {
            id,
            name: name.into(),
            is_active,
            is_urgent: false,
            window_count: 0,
            monitor: "default".into(),
        }
    }
}

/// Description of the currently focused window on the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActiveWindow {
    /// Window title (e.g. "Cargo.toml - Neovim", "GitHub - Mozilla Firefox").
    pub title: String,
    /// Application class or app_id (e.g. "firefox", "kitty", "org.wezfurlong.wezterm").
    pub class: String,
    /// Workspace ID where this window is located.
    pub workspace_id: i32,
}

/// Identified Wayland compositor backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompositorKind {
    /// Hyprland dynamic tiling Wayland compositor.
    Hyprland,
    /// Sway i3-compatible Wayland compositor.
    Sway,
    /// Niri scrollable tiling Wayland compositor.
    Niri,
    /// Generic or fallback simulation for development without live Wayland compositor.
    #[default]
    Fallback,
}

/// Unified event stream from compositor IPC sockets.
#[derive(Debug, Clone)]
pub enum CompositorEvent {
    /// Workspaces list was updated or workspace focus changed.
    WorkspaceActivated(i32),
    /// Active focused window changed.
    WindowFocused(ActiveWindow),
    /// Full workspace layout refresh needed.
    FullSync(Vec<Workspace>, Option<ActiveWindow>),
}
