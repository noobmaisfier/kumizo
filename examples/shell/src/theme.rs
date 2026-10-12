use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    point, pulsating_between, Animation, AnimationExt, BoxShadow, Div, ElementId, IntoElement,
    px, Rgba, Stateful, Styled,
};
use kumizo_kit::prelude::*;

// ---------------------------------------------------------------------------
// Layout Dimensions
// ---------------------------------------------------------------------------

pub const RAIL_WIDTH: f32 = 72.0;
pub const STAMP_WIDTH: f32 = 236.0;
pub const STAMP_HEIGHT: f32 = 80.0;
pub const OSD_WIDTH: f32 = 400.0;
pub const OSD_HEIGHT: f32 = 104.0;
pub const CALENDAR_WIDTH: f32 = 330.0;
pub const CALENDAR_HEIGHT: f32 = 392.0;
/// Where popups start, just under the stamp.
pub const POPUP_TOP: i32 = 98;

// ---------------------------------------------------------------------------
// Flexoki Color Palette
// ---------------------------------------------------------------------------

pub mod pal {
    use gpui::{rgb, rgba, Rgba};

    // Official Flexoki Light Palette by Steph Ango (https://stephango.com/flexoki)
    // Warm analog paper, deep inky contrast, and calibrated 600/400-series accents.

    /// Paper (#FFFCF0) — warm analog base paper.
    pub fn paper() -> Rgba { rgb(0xfffcf0) }

    /// Base-50 (#F2F0E5) — secondary surface / cards / input boxes.
    pub fn white() -> Rgba { rgb(0xf2f0e5) }

    /// Black / Ink (#100F0F) — deep, inky rich black for text, 2px borders, hard shadows.
    pub fn ink() -> Rgba { rgb(0x100f0f) }

    /// Blue 600 (#205EA6) — primary editorial blue accent (active workspaces, buttons).
    pub fn blue() -> Rgba { rgb(0x205ea6) }

    /// Red 400 (#D14D41) — warm terracotta red for misprint plates, battery low, mute.
    pub fn pink() -> Rgba { rgb(0xd14d41) }

    /// Yellow 400 (#D0A215) — warm golden amber for launcher tile and charging gauges.
    pub fn yellow() -> Rgba { rgb(0xd0a215) }

    /// Orange / Peach 400 (#DA702C) — warm editorial orange for in-progress tasks.
    pub fn peach() -> Rgba { rgb(0xda702c) }

    /// Green 600 (#879A39) — sage botanical green for completed tasks.
    pub fn green() -> Rgba { rgb(0x879a39) }

    /// Purple 400 (#8B7EC8) — warm iris purple for notes and memos.
    pub fn purple() -> Rgba { rgb(0x8b7ec8) }

    /// Base-600 / Text Muted (#6F6E69) — stone graphite secondary text.
    pub fn gray() -> Rgba { rgb(0x6f6e69) }

    /// UI-2 (#DAD8CE) — subtle selection tint.
    pub fn tint() -> Rgba { rgb(0xdad8ce) }

    /// Translucent Paper Veil (#FFFCF0 at 91% opacity).
    pub fn veil() -> Rgba { rgba(0xfffcf0e8) }

    /// Faint Ink (#100F0F at 12% opacity).
    pub fn faint() -> Rgba { rgba(0x100f0f1f) }

    /// Clear transparent.
    pub fn clear() -> Rgba { rgba(0x00000000) }
}

// ---------------------------------------------------------------------------
// Risograph Aesthetic Helpers
// ---------------------------------------------------------------------------

pub fn hard(color: Rgba, d: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: color.into(),
        offset: point(px(d), px(d)),
        blur_radius: px(0.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

pub fn out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

pub fn staggered(d: f32, i: usize, n: usize, step: f32) -> f32 {
    let span = (1.0 - step * n.saturating_sub(1) as f32).max(0.25);
    ((d - step * i as f32) / span).clamp(0.0, 1.0)
}

/// Blue text over a pink plate that is 1.5px off register.
pub fn misprint(text: String, size: f32, top: Rgba, under: Rgba) -> Div {
    div()
        .relative()
        .text_size(px(size))
        .font_weight(gpui::FontWeight::BOLD)
        .child(
            div()
                .absolute()
                .top(px(1.5))
                .left(px(1.5))
                .text_color(under)
                .child(text.clone()),
        )
        .child(div().text_color(top).child(text))
}

/// A physical square button: lifts on hover, sinks on press.
pub fn tile(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .w(px(40.0))
        .h(px(40.0))
        .bg(pal::paper())
        .border_2()
        .border_color(pal::ink())
        .shadow(hard(pal::ink(), 3.0))
        .text_size(px(16.0))
        .text_color(pal::ink())
        .cursor_pointer()
        .active(|s| s.shadow(Vec::new()).top(px(3.0)).left(px(3.0)))
}

pub fn key_chip(key: &'static str, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .px_1p5()
                .bg(pal::white())
                .border_2()
                .border_color(pal::ink())
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(pal::ink())
                .child(key),
        )
        .child(div().text_size(px(11.0)).text_color(pal::ink()).child(label))
}

/// Printed-card entrance: a colored plate lands off-register behind the card
/// and settles into place while the card fades in.
pub fn printed<E: Styled + IntoElement + 'static>(card: E, key: &str, echo: Rgba) -> impl IntoElement {
    let k_plate = format!("{key}-plate");
    let k_card = format!("{key}-card");
    div()
        .relative()
        .child(
            div()
                .absolute()
                .size_full()
                .bg(echo)
                .border_2()
                .border_color(pal::ink())
                .with_animation(k_plate, Animation::new(Duration::from_millis(460)), |el, d| {
                    let off = 7.0 + 18.0 * (1.0 - out_back(d));
                    el.left(px(off)).top(px(off))
                }),
        )
        .child(
            card.relative()
                .with_animation(k_card, Animation::new(Duration::from_millis(300)), |el, d| {
                    el.opacity(d.min(1.0))
                }),
        )
}

pub fn caret() -> impl IntoElement {
    div()
        .w(px(10.0))
        .h(px(22.0))
        .bg(pal::blue())
        .with_animation(
            "caret",
            Animation::new(Duration::from_millis(900))
                .repeat()
                .with_easing(pulsating_between(0.0, 1.0)),
            |el, d| el.opacity(d),
        )
}

/// Tagged input field shared by launcher and clipboard.
pub fn find_field(tag: &'static str, tag_bg: Rgba, query: &str, placeholder: &'static str) -> Div {
    let empty = query.is_empty();
    div()
        .flex()
        .items_center()
        .gap_3()
        .h(px(52.0))
        .pr_3()
        .bg(pal::white())
        .border_2()
        .border_color(pal::ink())
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .px_3()
                .bg(tag_bg)
                .text_size(px(14.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(pal::ink())
                .child(tag),
        )
        .child(div().w(px(2.0)).h_full().bg(pal::ink()))
        .child(
            div()
                .flex()
                .items_center()
                .flex_1()
                .gap_0p5()
                .text_size(px(18.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .when(empty, |s| s.child(caret()))
                .child(
                    div()
                        .text_color(if empty { pal::gray() } else { pal::ink() })
                        .child(if empty { placeholder.to_string() } else { query.to_string() }),
                )
                .when(!empty, |s| s.child(caret())),
        )
}

/// Helper for standard fullscreen overlay windows.
pub fn fullscreen(name: &'static str, ns: &'static str) -> LayerWindow {
    LayerWindow::new(name)
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .namespace(ns)
        .start_hidden(true)
}
