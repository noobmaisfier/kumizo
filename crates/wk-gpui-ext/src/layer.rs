/// Wayland Layer Shell z-order layers.
///
/// Determines where in the desktop hierarchy the layer surface resides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Layer {
    /// Rendered below all regular windows, typically for desktop wallpapers.
    Background,
    /// Rendered below regular windows but above the background (e.g. desktop widgets).
    Bottom,
    /// Rendered above regular windows (e.g. status bars, docks, panels).
    #[default]
    Top,
    /// Rendered above all surfaces including fullscreen windows (e.g. OSD, lockscreen, notification banners).
    Overlay,
}

impl Layer {
    /// Convert layer to protocol string identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            Layer::Background => "background",
            Layer::Bottom => "bottom",
            Layer::Top => "top",
            Layer::Overlay => "overlay",
        }
    }
}
