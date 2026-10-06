//! Convenient prelude re-exporting essential types and traits for building WidgetKit apps.

pub use crate::app::{WidgetKit, WindowControlExt, WindowControlGlobal};
pub use crate::registry::{AppContextServiceExt, ServiceContextExt};

// LayerShell configuration
pub use wk_gpui_ext::{Anchor, KeyboardInteractivity, Layer, LayerWindow, Margins, MonitorTarget};

// System Services
pub use wk_services::{
    Audio, Battery, BatteryState, Brightness, Clock, ClockResolution, Mpris, Network, OpenMenu,
    PlaybackStatus, Service, Tray, TrayItem, TrayMenuItem, TrayStatus, WifiBackend,
};

// Compositor Integration
pub use wk_compositor::{ActiveWindow, Compositor, CompositorKind, Workspace};

// Essential GPUI types for fluent UI composition
pub use gpui::{
    div, hsla, px, rgb, rgba, Context, Element, Entity, IntoElement, ParentElement, Render,
    Styled, Window,
};
