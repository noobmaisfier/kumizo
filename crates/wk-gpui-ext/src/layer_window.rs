use crate::{anchor::Anchor, keyboard::KeyboardInteractivity, layer::Layer, margins::Margins};
use gpui::{
    px, size, Bounds, Pixels, Point, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions,
};

/// Fallback surface width when neither the caller nor the output says otherwise.
const DEFAULT_WIDTH: f32 = 800.0;
/// Fallback surface height when neither the caller nor the output says otherwise.
const DEFAULT_HEIGHT: f32 = 32.0;

/// Monitor/output target selection for multi-head Wayland setups.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MonitorTarget {
    /// Render on primary/active display.
    #[default]
    Primary,
    /// Duplicate surface across all connected displays.
    All,
    /// Target a specific Wayland output name (e.g. "eDP-1", "DP-2").
    Named(String),
}

/// Ergonomic builder for creating and configuring Wayland Layer Shell windows in GPUI.
///
/// Wraps native compositor layer surface configuration (anchors, exclusive zones,
/// layers, margins, namespaces) into a clean, fluent builder.
#[derive(Debug, Clone)]
pub struct LayerWindow {
    /// Unique identifier for this window instance.
    pub id: String,
    /// Wayland layer-shell namespace string (used for compositor blur/animations, e.g. `layerrule = blur, wk-topbar`).
    pub namespace: String,
    /// Z-order layer (Background, Bottom, Top, Overlay).
    pub layer: Layer,
    /// Surface anchors attaching to output edges.
    pub anchor: Anchor,
    /// Reserved screen space in pixels (exclusive zone).
    /// Positive integer reserves space, -1 ignores exclusive zones, 0 specifies no reserved space.
    pub exclusive_zone: i32,
    /// Outer margins relative to the anchors.
    pub margins: Margins,
    /// Keyboard interactivity mode.
    pub keyboard_interactivity: KeyboardInteractivity,
    /// Target monitor / display.
    pub monitor: MonitorTarget,
    /// Explicit width in physical pixels (if unanchored horizontally).
    pub width: Option<f32>,
    /// Explicit height in physical pixels (if unanchored vertically).
    pub height: Option<f32>,
    /// Whether window background supports transparency (for blur and custom styling).
    pub transparent: bool,
    /// Open with no surface: the window is created on demand by `show` and
    /// destroyed by `hide`, so a popup never reserves input area while closed.
    pub start_hidden: bool,
}

impl LayerWindow {
    /// Create a new LayerWindow configuration with a given ID.
    ///
    /// The window is created with neutral defaults:
    /// - `anchor`: `Anchor::empty()` (caller specifies attached edges)
    /// - `exclusive_zone`: `0` (no space reserved by default)
    /// - `width`: `None` (auto-expanded if anchored `LEFT | RIGHT`, or fallback)
    /// - `height`: `None` (auto-expanded if anchored `TOP | BOTTOM`, or fallback)
    pub fn new(id: impl Into<String>) -> Self {
        let name = id.into();
        Self {
            namespace: format!("wk-{}", name),
            id: name,
            layer: Layer::Top,
            anchor: Anchor::empty(),
            exclusive_zone: 0,
            margins: Margins::zero(),
            keyboard_interactivity: KeyboardInteractivity::None,
            monitor: MonitorTarget::Primary,
            width: None,
            height: None,
            transparent: true,
            start_hidden: false,
        }
    }

    /// Set the desktop layer.
    pub fn layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// Set the anchor edges.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set the exclusive zone (reserved pixels for panel/dock).
    pub fn exclusive_zone(mut self, zone: i32) -> Self {
        self.exclusive_zone = zone;
        self
    }

    /// Set the Wayland layer-shell namespace (used for compositor blur and shadow rules).
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Set the surface margins.
    pub fn margins(mut self, margins: Margins) -> Self {
        self.margins = margins;
        self
    }

    /// Set keyboard interactivity.
    pub fn keyboard_interactivity(mut self, interactivity: KeyboardInteractivity) -> Self {
        self.keyboard_interactivity = interactivity;
        self
    }

    /// Set the target monitor or output.
    pub fn monitor(mut self, monitor: MonitorTarget) -> Self {
        self.monitor = monitor;
        self
    }

    /// Explicitly set the dimensions.
    pub fn size(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set height shorthand.
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Start with no surface; the window appears on `show` and is destroyed
    /// by `hide`. For popups (tray menu) that must not reserve input area
    /// while closed.
    pub fn start_hidden(mut self, hidden: bool) -> Self {
        self.start_hidden = hidden;
        self
    }
    /// Set width shorthand.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Enable or disable transparency.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Whether this window is anchored to both the left and right edges, which
    /// means it should stretch across the whole output width.
    pub fn stretches_horizontally(&self) -> bool {
        self.anchor.contains(Anchor::LEFT) && self.anchor.contains(Anchor::RIGHT)
    }

    /// Whether this window is anchored to both the top and bottom edges.
    pub fn stretches_vertically(&self) -> bool {
        self.anchor.contains(Anchor::TOP) && self.anchor.contains(Anchor::BOTTOM)
    }

    /// Converts this `LayerWindow` configuration into native GPUI `WindowOptions`.
    ///
    /// Without display information a window anchored to opposite edges falls
    /// back to its explicit `width`/`height`. Prefer
    /// [`to_gpui_options_with_display`](Self::to_gpui_options_with_display) when
    /// a `gpui::App` is available, otherwise a top bar gets a fixed 800px
    /// surface instead of spanning the screen.
    pub fn to_gpui_options(&self) -> WindowOptions {
        self.to_gpui_options_with_display(None)
    }

    /// Same as [`to_gpui_options`](Self::to_gpui_options), but resolves the
    /// surface size against the bounds of the target output.
    ///
    /// A layer-shell surface is *not* stretched by the compositor to fill the
    /// space between opposite anchors: the client has to request that size. So
    /// for a `TOP | LEFT | RIGHT` bar we must subtract the margins from the
    /// output width, otherwise the surface overflows and the right-hand side of
    /// the bar gets clipped.
    pub fn to_gpui_options_with_display(
        &self,
        display_bounds: Option<Bounds<Pixels>>,
    ) -> WindowOptions {
        let left = px(self.margins.left as f32);
        let right = px(self.margins.right as f32);
        let top = px(self.margins.top as f32);
        let bottom = px(self.margins.bottom as f32);

        // An explicit `width`/`height` always wins. Only when the caller left it
        // unset do we stretch across the output, and only then do the anchors
        // actually make sense for a bar: a surface anchored LEFT|RIGHT has to
        // request the span itself, because the compositor does not grow it.
        let w = match self.width {
            Some(w) => px(w),
            None => match display_bounds {
                Some(bounds) if self.stretches_horizontally() => {
                    (bounds.size.width - left - right).max(px(1.0))
                }
                _ => px(DEFAULT_WIDTH),
            },
        };
        let h = match self.height {
            Some(h) => px(h),
            None => match display_bounds {
                Some(bounds) if self.stretches_vertically() => {
                    (bounds.size.height - top - bottom).max(px(1.0))
                }
                _ => px(DEFAULT_HEIGHT),
            },
        };

        // Position is owned by the compositor: anchors plus margins. Using the
        // margins as the origin as well would apply them twice.
        let initial_bounds = Bounds {
            origin: Point::new(px(0.0), px(0.0)),
            size: size(w, h),
        };

        #[cfg(all(target_os = "linux", feature = "layer-shell-variant"))]
        let kind = WindowKind::LayerShell(self.to_layer_shell_options());

        #[cfg(not(all(target_os = "linux", feature = "layer-shell-variant")))]
        let kind = WindowKind::PopUp;

        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(initial_bounds)),
            titlebar: None,
            window_background: if self.transparent {
                WindowBackgroundAppearance::Transparent
            } else {
                WindowBackgroundAppearance::Opaque
            },
            focus: self.keyboard_interactivity != KeyboardInteractivity::None,
            show: true,
            kind,
            is_movable: false,
            display_id: None,
            window_min_size: None,
            app_id: Some(self.namespace.clone()),
            ..Default::default()
        }
    }

    /// Converts this configuration into native `wlr-layer-shell-unstable-v1` options.
    ///
    /// This is what actually forwards the anchors, exclusive zone, margins,
    /// layer and keyboard interactivity to the compositor. Available on Linux
    /// when the `layer-shell-variant` feature is enabled.
    #[cfg(all(target_os = "linux", feature = "layer-shell-variant"))]
    pub fn to_layer_shell_options(&self) -> gpui::layer_shell::LayerShellOptions {
        use gpui::layer_shell;

        // wlr-layer-shell semantics: >0 reserves that many pixels, 0 reserves
        // nothing, <0 ignores exclusive zones entirely (span every output).
        let exclusive_zone = match self.exclusive_zone {
            zone if zone > 0 => Some(px(zone as f32)),
            0 => None,
            _ => Some(px(-1.0)),
        };

        let layer = match self.layer {
            Layer::Background => layer_shell::Layer::Background,
            Layer::Bottom => layer_shell::Layer::Bottom,
            Layer::Top => layer_shell::Layer::Top,
            Layer::Overlay => layer_shell::Layer::Overlay,
        };

        let mut anchor = layer_shell::Anchor::empty();
        if self.anchor.contains(Anchor::TOP) {
            anchor |= layer_shell::Anchor::TOP;
        }
        if self.anchor.contains(Anchor::BOTTOM) {
            anchor |= layer_shell::Anchor::BOTTOM;
        }
        if self.anchor.contains(Anchor::LEFT) {
            anchor |= layer_shell::Anchor::LEFT;
        }
        if self.anchor.contains(Anchor::RIGHT) {
            anchor |= layer_shell::Anchor::RIGHT;
        }

        let keyboard_interactivity = match self.keyboard_interactivity {
            KeyboardInteractivity::None => layer_shell::KeyboardInteractivity::None,
            KeyboardInteractivity::Exclusive => layer_shell::KeyboardInteractivity::Exclusive,
            KeyboardInteractivity::OnDemand => layer_shell::KeyboardInteractivity::OnDemand,
        };

        layer_shell::LayerShellOptions {
            namespace: self.namespace.clone(),
            layer,
            anchor,
            exclusive_zone,
            exclusive_edge: None,
            margin: Some((
                px(self.margins.top as f32),
                px(self.margins.right as f32),
                px(self.margins.bottom as f32),
                px(self.margins.left as f32),
            )),
            keyboard_interactivity,
        }
    }
}
