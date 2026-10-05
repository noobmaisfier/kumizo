//! # wk-services
//!
//! Asynchronous background system services for WidgetKit.
//!
//! Provides reactive, zero-idle-CPU implementations for:
//! - [`Clock`]: Nanosecond-precise, zero-drift system clock
//! - [`Battery`]: UPower D-Bus and Linux sysfs battery monitor
//! - [`Audio`]: PipeWire/WirePlumber and PulseAudio volume/mute control
//! - [`Mpris`]: Media player state and control (Spotify, Firefox, mpv, etc.)

pub mod audio;
pub mod battery;
pub mod clock;
pub mod mpris;
pub mod service;

pub use audio::Audio;
pub use battery::{Battery, BatteryState};
pub use clock::{Clock, ClockResolution};
pub use mpris::{Mpris, PlaybackStatus};
pub use service::Service;
