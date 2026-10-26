use serde::{Deserialize, Serialize};

/// Wayland workspace descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Numerical or logical identifier of the workspace.
    ///
    /// Numbered workspaces keep their compositor number. Named and special
    /// workspaces (`web`, `special:magic`) have no numeric id, so Kumizo assigns
    /// a stable negative id and keeps the real selector in
    /// [`Workspace::address`]; [`Compositor::switch_workspace`] resolves it.
    pub id: i32,
    /// Display name or label (e.g. "1", "web", "magic").
    pub name: String,
    /// Backend-native workspace selector.
    ///
    /// Hyprland: `"3"`, `"web"` (named) or `"special:magic"`. A named workspace
    /// whose own name starts with `special:` is stored as `"name:special:x"` so
    /// it is not mistaken for the real special workspace. Sway/Niri: the
    /// workspace name. Pass this to the backend when the numeric id is not
    /// enough (named/special workspaces).
    #[serde(default)]
    pub address: String,
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
        let name = name.into();
        Self {
            id,
            name: name.clone(),
            address: name,
            is_active,
            is_urgent: false,
            window_count: 0,
            monitor: "default".into(),
        }
    }

    /// Whether this is a Hyprland special (scratchpad) workspace.
    ///
    /// Only the selector counts: a *named* workspace may also be called
    /// `special:x`, and Hyprland stores it as `name:special:x` to tell it apart.
    pub fn is_special(&self) -> bool {
        self.address.starts_with("special:")
    }
}

/// Description of an output/monitor and its active workspaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    /// Backend monitor id.
    pub id: i64,
    /// Output name (e.g. `DP-1`, `eDP-1`).
    pub name: String,
    /// Human-readable description (Hyprland `description`, Sway/Niri empty).
    pub description: String,
    /// Width in logical pixels.
    pub width: i32,
    /// Height in logical pixels.
    pub height: i32,
    /// Refresh rate in Hz.
    pub refresh_rate: f64,
    /// Position on the global layout.
    pub x: i32,
    /// Position on the global layout.
    pub y: i32,
    /// Output scale factor.
    pub scale: f64,
    /// Whether this is the focused output.
    pub focused: bool,
    /// Whether the output is disabled (Hyprland), always `false` elsewhere.
    pub disabled: bool,
    /// Address of the monitor's active regular workspace (Hyprland).
    pub active_workspace: String,
    /// Display name of the monitor's active regular workspace.
    pub active_workspace_name: String,
    /// Address of the monitor's active special workspace, if any (Hyprland).
    pub special_workspace: Option<String>,
    /// Display name of the monitor's active special workspace, if any.
    pub special_workspace_name: Option<String>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            description: String::new(),
            width: 0,
            height: 0,
            refresh_rate: 0.0,
            x: 0,
            y: 0,
            scale: 1.0,
            focused: false,
            disabled: false,
            active_workspace: String::new(),
            active_workspace_name: String::new(),
            special_workspace: None,
            special_workspace_name: None,
        }
    }
}

/// Description of an open application window on the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AppWindow {
    /// Compositor internal container/window ID.
    ///
    /// Sway: node id. Hyprland: the 64-bit window address (`0x…`), kept as an
    /// `i64` so the same field works for both. Niri: window id.
    pub id: i64,
    /// Window title (e.g. "Cargo.toml - Neovim", "GitHub - Mozilla Firefox").
    pub title: String,
    /// Application class or app_id (e.g. "firefox", "kitty", "helium").
    pub class: String,
    /// Workspace ID/num where this window resides.
    pub workspace_id: i32,
    /// Workspace name (e.g. "1", "2").
    pub workspace_name: String,
    /// Whether this window currently has keyboard/cursor focus.
    pub is_focused: bool,
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
    /// The active workspace changed to the given workspace number.
    ///
    /// This only moves the highlight: it says nothing about workspaces having
    /// been created or removed, since a compositor is free to keep a workspace
    /// that no longer holds windows.
    WorkspaceActivated(i32),
    /// The workspace list changed (creation, removal, rename, move to another
    /// output). The payload is the new list, already normalised.
    ///
    /// The compositor is the only authority on this, so the event carries the
    /// fresh list rather than a delta that would have to be replayed in the
    /// right order.
    WorkspacesChanged(Vec<Workspace>),
    /// Active focused window changed.
    WindowFocused(ActiveWindow),
    /// Focus moved to a layer surface or to no window at all.
    WindowFocusCleared,
    /// A window raised its urgency hint (`urgent` Hyprland event). The payload
    /// is the backend window id; the service marks its workspace as urgent.
    WindowUrgent(i64),
    /// Open windows list changed (opened, closed, renamed, focused).
    WindowsChanged(Vec<AppWindow>),
    /// Monitor list changed (output added, removed or reconfigured).
    MonitorsChanged(Vec<Monitor>),
}
