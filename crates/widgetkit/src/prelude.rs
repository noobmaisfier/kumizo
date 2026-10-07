//! Convenient prelude re-exporting essential types and traits for building WidgetKit apps.

pub use crate::app::{WidgetKit, WindowControlExt, WindowControlGlobal};
pub use crate::registry::{AppContextServiceExt, ServiceContextExt};

// LayerShell configuration
pub use wk_gpui_ext::{Anchor, KeyboardInteractivity, Layer, LayerWindow, Margins, MonitorTarget};

// System Services
pub use wk_services::{
    AppInfo, Apps, Audio, Battery, BatteryState, Brightness, Clipboard, ClipboardItem, Clock,
    ClockResolution, Emitter, FileSource, Mpris, Network, Notification, NotificationAction,
    Notifications, OpenMenu, PlaybackStatus, ProcessSource, Service, Source, SourceState,
    StopToken, TrackExt, Tray, TrayItem, TrayMenuItem, TrayStatus, Urgency, WifiBackend,
};

// Compositor Integration
pub use wk_compositor::{ActiveWindow, Compositor, CompositorKind, Workspace};

// Essential GPUI types for fluent UI composition
pub use gpui::{
    div, hsla, px, rgb, rgba, Context, Element, Entity, IntoElement, ParentElement, Render,
    Styled, Window,
};
