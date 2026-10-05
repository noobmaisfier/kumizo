//! # wk-gpui-ext
//!
//! Wayland Layer Shell extension traits and ergonomic wrappers for GPUI.
//!
//! Provides first-class support for `wlr-layer-shell-unstable-v1` configuration
//! in pure Rust desktop environments built with GPUI.

pub mod anchor;
pub mod keyboard;
pub mod layer;
pub mod layer_window;
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
