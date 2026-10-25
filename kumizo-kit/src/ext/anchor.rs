use bitflags::bitflags;

bitflags! {
    /// Wayland Layer Shell surface anchor flags.
    ///
    /// Determines which edges of the output screen the window surface attaches to.
    /// Combining opposite anchors (e.g. `TOP | LEFT | RIGHT`) stretches the surface
    /// across the screen along that axis.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Anchor: u32 {
        /// Anchor to the top edge of the output.
        const TOP = 1 << 0;
        /// Anchor to the bottom edge of the output.
        const BOTTOM = 1 << 1;
        /// Anchor to the left edge of the output.
        const LEFT = 1 << 2;
        /// Anchor to the right edge of the output.
        const RIGHT = 1 << 3;

        /// Convenience constant: spans full width at the top (`TOP | LEFT | RIGHT`).
        const TOP_BAR = Self::TOP.bits() | Self::LEFT.bits() | Self::RIGHT.bits();
        /// Convenience constant: spans full width at the bottom (`BOTTOM | LEFT | RIGHT`).
        const BOTTOM_BAR = Self::BOTTOM.bits() | Self::LEFT.bits() | Self::RIGHT.bits();
        /// Convenience constant: spans full height on the left (`LEFT | TOP | BOTTOM`).
        const LEFT_BAR = Self::LEFT.bits() | Self::TOP.bits() | Self::BOTTOM.bits();
        /// Convenience constant: spans full height on the right (`RIGHT | TOP | BOTTOM`).
        const RIGHT_BAR = Self::RIGHT.bits() | Self::TOP.bits() | Self::BOTTOM.bits();
        /// Convenience constant: fullscreen surface covering all edges.
        const FULLSCREEN = Self::TOP.bits() | Self::BOTTOM.bits() | Self::LEFT.bits() | Self::RIGHT.bits();
    }
}

impl Default for Anchor {
    fn default() -> Self {
        Anchor::TOP | Anchor::LEFT | Anchor::RIGHT
    }
}
