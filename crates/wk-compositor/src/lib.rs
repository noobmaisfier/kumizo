//! # wk-compositor
//!
//! Native Wayland compositor IPC integrations (Hyprland, Sway, Niri) for Kumizo.
//!
//! Subscribes directly to compositor UNIX domain sockets for:
//! - Workspace tracking and switching
//! - Active window titles and application classes
//! - Multi-monitor assignment
//! - Zero Idle CPU event notifications

pub mod compositor;
pub mod hyprland;
pub mod niri;
pub mod sway;
pub mod types;

pub use compositor::Compositor;
pub use hyprland::HyprlandIpc;
pub use niri::NiriIpc;
pub use sway::SwayIpc;
pub use types::{ActiveWindow, CompositorEvent, CompositorKind, Workspace};
