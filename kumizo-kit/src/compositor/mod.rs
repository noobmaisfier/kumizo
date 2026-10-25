//! # Compositor integrations
//!
//! Native Wayland compositor IPC integrations (Hyprland, Sway, Niri) for Kumizo.
//!
//! Subscribes directly to compositor UNIX domain sockets for:
//! - Workspace tracking and switching
//! - Active window titles and application classes
//! - Multi-monitor assignment
//! - Zero Idle CPU event notifications

/// Native Wayland compositor IPC integration (Sway).
pub mod compositor;
/// Native Wayland compositor IPC integration (Hyprland).
pub mod hyprland;
/// Native Wayland compositor IPC integration (Niri).
pub mod niri;
/// Native Wayland compositor IPC integration (Sway).
pub mod sway;
/// Shared types for compositor IPC (Workspace, AppWindow, …).
pub mod types;

pub use compositor::Compositor;
pub use hyprland::HyprlandIpc;
pub use niri::NiriIpc;
pub use sway::SwayIpc;
pub use types::{ActiveWindow, CompositorEvent, CompositorKind, Workspace};
