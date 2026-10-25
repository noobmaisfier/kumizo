//! In-process window control, shared by views and services.
//!
//! The builder publishes a [`WindowControlGlobal`] before any service starts,
//! so a service (the tray opening its menu) or a view (a click handler) can
//! show, hide or toggle a window directly in memory, with no IPC round trip.

use gpui::{App, Global};
use tracing::warn;
use crate::ipc::WindowControl;

/// Shared window control, published as a GPUI global by `Kumizo::run`.
#[derive(Clone)]
pub struct WindowControlGlobal(pub WindowControl);

impl Global for WindowControlGlobal {}

/// Direct, in-process control of the shell's windows, no IPC round trip.
///
/// Implemented for [`App`]; `Context<V>` derefs to it, so a click handler can
/// write `cx.toggle_window("calendar")`. Calls never block the UI: the request
/// is handed to the window's control task and answered asynchronously. An
/// unknown window name is logged as a warning.
pub trait WindowControlExt {
    /// Create/show the window named `name`.
    fn show_window(&self, name: &str);
    /// Destroy the window's surface (its view state is kept).
    fn hide_window(&self, name: &str);
    /// Show the window if hidden, hide it if shown.
    fn toggle_window(&self, name: &str);
}

impl WindowControlExt for App {
    fn show_window(&self, name: &str) {
        match self.try_global::<WindowControlGlobal>() {
            Some(g) => g.0.show(name),
            None => warn!("show_window('{name}'): Kumizo is not running"),
        }
    }

    fn hide_window(&self, name: &str) {
        match self.try_global::<WindowControlGlobal>() {
            Some(g) => g.0.hide(name),
            None => warn!("hide_window('{name}'): Kumizo is not running"),
        }
    }

    fn toggle_window(&self, name: &str) {
        match self.try_global::<WindowControlGlobal>() {
            Some(g) => g.0.toggle(name),
            None => warn!("toggle_window('{name}'): Kumizo is not running"),
        }
    }
}
