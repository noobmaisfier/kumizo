//! Headless UI primitives shared by all Kumizo shells.
//!
//! "Headless" means: behaviour and state live here, looks do not. Every
//! render helper takes its colours/sizes from the caller, so each shell
//! keeps 100% of its aesthetic freedom (same philosophy as the SDK).
//!
//! Reference design: `gpui-component` 0.7.x (`InputState` + `Input`,
//! `TextareaState` + `Textarea`) — same `gpui-pre =0.3.8` snapshot as this
//! workspace, so no second GPUI copy. Unlike that kit, these components
//! impose no theme: just the editing state machine and a caret.

pub mod text;

pub use text::{TextAction, TextBuffer, TextStyle, text_area, text_input};
