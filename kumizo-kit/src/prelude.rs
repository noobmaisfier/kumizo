//! Convenient prelude re-exporting essential types and traits for building Kumizo apps.

pub use crate::app::{Kumizo, WindowControlExt, WindowControlGlobal};
pub use crate::registry::{AppContextServiceExt, ServiceContextExt};

// LayerShell configuration
pub use crate::ext::{Anchor, KeyboardInteractivity, Layer, LayerWindow, Margins, MonitorTarget};

// System Services
pub use crate::services::{
    AppInfo, Apps, Audio, Battery, BatteryState, Bluetooth, BluetoothDevice, Brightness, Clipboard,
    ClipboardItem, Clock, ClockResolution, Emitter, FileSource, Memory, Mpris, Network, Notification,
    NotificationAction, Notifications, OpenMenu, PlaybackStatus, ProcessSource, Service, Source,
    SourceState, StopToken, TrackExt, Tray, TrayItem, TrayMenuItem, TrayStatus, Urgency, WifiBackend,
    WifiNetwork,
};

// Shared headless UI primitives (text inputs, ...)
pub use crate::ui::{TextAction, TextBuffer, TextStyle, text_area, text_input};

// Compositor Integration
pub use crate::compositor::{
    ActiveWindow, AppWindow, Compositor, CompositorKind, Monitor, Workspace,
};

// Essential GPUI types for fluent UI composition
pub use gpui::{
    div, hsla, px, rgb, rgba, Context, Element, Entity, IntoElement, ParentElement, Render,
    Styled, Window,
};
