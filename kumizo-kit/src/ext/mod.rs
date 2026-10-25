//! # Layer shell extensions
//!
//! Wayland Layer Shell extension traits and ergonomic wrappers for GPUI.
//!
//! Provides first-class support for `wlr-layer-shell-unstable-v1` configuration
//! in pure Rust desktop environments built with GPUI.

/// Anchor bitflags for Wayland layer-shell surfaces.
pub mod anchor;
/// Keyboard interactivity modes for layer-shell windows.
pub mod keyboard;
/// Wayland layer-shell layers (Background, Bottom, Top, Overlay).
pub mod layer;
/// Ergonomic builder for Wayland Layer Shell windows.
pub mod layer_window;
/// Outer margins for window surfaces.
pub mod margins;

pub use anchor::Anchor;
pub use keyboard::KeyboardInteractivity;
pub use layer::Layer;
pub use layer_window::{LayerWindow, MonitorTarget};
pub use margins::Margins;

use gpui::{App, Entity, Render, Window, WindowHandle};

/// Extension trait for `gpui::App` to easily open LayerShell windows.
pub trait AppLayerShellExt {
    /// Opens a layer shell window configured with the given `LayerWindow` builder.
    fn open_layer_window<V: 'static + Render>(
        &mut self,
        config: &LayerWindow,
        build_view: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    ) -> anyhow::Result<WindowHandle<V>>;
}

impl AppLayerShellExt for App {
    fn open_layer_window<V: 'static + Render>(
        &mut self,
        config: &LayerWindow,
        build_view: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    ) -> anyhow::Result<WindowHandle<V>> {
        let options = config.to_gpui_options();
        self.open_window(options, build_view)
    }
}
