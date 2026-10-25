//! # System services
//!
//! Asynchronous background system services for Kumizo.
//!
//! Provides reactive, zero-idle-CPU implementations for:
//! - [`Clock`]: Nanosecond-precise, zero-drift system clock
//! - [`Battery`]: UPower D-Bus and Linux sysfs battery monitor
//! - [`Audio`]: PipeWire/WirePlumber and PulseAudio volume/mute control
//! - [`Mpris`]: Media player state and control (Spotify, Firefox, mpv, etc.)
//! - [`Network`]: Wi-Fi status via NetworkManager, iwd or `/proc/net/wireless`
//! - [`Tray`]: System tray host (StatusNotifierItem watcher, icons, menus)
//! - [`Brightness`]: Screen backlight via sysfs + `brightnessctl`
//! - [`Memory`]: RAM usage from `/proc/meminfo` (polled, notify on change)
//! - [`Source`]: build your own event-driven data source (process stdout, file, socket) without a full `Service`

/// FreeDesktop .desktop application catalog and launching.
pub mod apps;
/// Audio volume/mute control (PipeWire, PulseAudio).
pub mod audio;
/// Battery state via UPower D-Bus and Linux sysfs.
pub mod battery;
/// Bluetooth devices and connection state (BlueZ).
pub mod bluetooth;
/// Screen backlight control via sysfs.
pub mod brightness;
/// Clipboard history and copy operations.
pub mod clipboard;
/// System clock with nanosecond precision.
pub mod clock;
/// RAM usage from /proc/meminfo.
pub mod memory;
/// MPRIS media player state and control.
pub mod mpris;
/// Wi-Fi and network status (NetworkManager, iwd).
pub mod network;
/// In-process notifications server (org.freedesktop.Notifications).
pub mod notifications;
/// Reactive system service trait.
pub mod service;
/// Build custom event-driven data sources without a full Service.
pub mod source;
/// System tray host (StatusNotifierItem watcher, icons, menus).
pub mod tray;
/// Window control primitives for IPC-driven show/hide/toggle.
pub mod window;

pub use apps::{AppInfo, Apps};
pub use audio::Audio;
pub use battery::{Battery, BatteryState};
pub use bluetooth::{Bluetooth, BluetoothDevice};
pub use brightness::Brightness;
pub use clipboard::{Clipboard, ClipboardItem};
pub use clock::{Clock, ClockResolution};
pub use memory::Memory;
pub use mpris::{Mpris, PlaybackStatus};
pub use network::{Network, WifiBackend, WifiNetwork};
pub use notifications::{Notification, NotificationAction, Notifications, Urgency};
pub use service::Service;
pub use source::{Emitter, FileSource, ProcessSource, Source, SourceState, StopToken, TrackExt};
pub use tray::{OpenMenu, Tray, TrayItem, TrayMenuItem, TrayStatus};
