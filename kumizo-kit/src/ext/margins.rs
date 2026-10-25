/// Outer margins for layer surfaces relative to output screen anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Margins {
    /// Top margin in physical pixels.
    pub top: i32,
    /// Right margin in physical pixels.
    pub right: i32,
    /// Bottom margin in physical pixels.
    pub bottom: i32,
    /// Left margin in physical pixels.
    pub left: i32,
}

impl Margins {
    /// Create zeroed margins.
    pub const fn zero() -> Self {
        Self {
            top: 0,
            right: 0,
            bottom: 0,
            left: 0,
        }
    }

    /// Uniform margins on all sides.
    pub const fn all(val: i32) -> Self {
        Self {
            top: val,
            right: val,
            bottom: val,
            left: val,
        }
    }

    /// Symmetric margins: (horizontal, vertical).
    pub const fn symmetric(h: i32, v: i32) -> Self {
        Self {
            top: v,
            right: h,
            bottom: v,
            left: h,
        }
    }

    /// Explicit four-side margins: (top, right, bottom, left).
    pub const fn new(top: i32, right: i32, bottom: i32, left: i32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}
