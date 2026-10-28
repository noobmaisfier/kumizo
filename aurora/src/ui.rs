//! Shared UI building blocks. RULE: no helper calls `.hover()` (GPUI
//! panics if `.hover()` is applied twice) — it is always the call site
//! that applies it, only once.

use gpui::prelude::*;
use gpui::{
    div, point, px, App, Canvas, DefiniteLength, Div, ElementId, FontFeatures, KeyDownEvent,
    PathBuilder, Rgba, Stateful,
};
use kumizo_kit::prelude::*;

use crate::{icons, theme::pal};

// ── Tokens ──────────────────────────────────────────────────────────────────
/// A single radius scale for the whole shell: chips, controls, rows,
/// cards, panels. Capsules (`pill`, `btn`) stay `full`.
pub const RADIUS_XS: f32 = 4.;
pub const RADIUS_SM: f32 = 6.;
pub const RADIUS_CTRL: f32 = 8.;
pub const RADIUS_ROW: f32 = 10.;
pub const RADIUS_CARD: f32 = 12.;
pub const RADIUS_PANEL: f32 = 14.;

/// Hover background: `raised` at 65% instead of a solid fill. On a chip or
/// card already tinted, hover is visible without "jumping" to full strength.
///
/// Avoid on a card placed directly on a transparent surface layer
/// (toasts): the alpha would let the desktop show through. Use
/// [`hover_bg_solid`] in that case.
pub fn hover_bg() -> Rgba {
    pal::with_alpha(pal::raised(), 0.65)
}

/// Opaque hover background, for cards floating on a transparent window.
pub fn hover_bg_solid() -> Rgba {
    pal::raised()
}

/// Tabular numerals: clock, durations and percentages keep the same width
/// when a digit changes (no more micro-shifting).
pub fn tabular() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![("tnum".to_string(), 1)]))
}

/// Window ids — prefixed `aurora-` (ids are global on the socket).
pub mod ids {
    pub const BAR: &str = "aurora-bar";
    pub const LAUNCHER: &str = "aurora-launcher";
    pub const TASKS: &str = "aurora-tasks";
    pub const POWER: &str = "aurora-power";
    pub const CLIPBOARD: &str = "aurora-clipboard";
    pub const CONTROL: &str = "aurora-control";
    pub const NOTIFS: &str = "aurora-notifs";
    pub const TOASTS: &str = "aurora-toasts";
    pub const THEMES: &str = "aurora-themes";
    pub const OSD: &str = "aurora-osd";
}

/// Actually installed Nerd Font family (same as waybar): requesting an
/// absent family (`Symbols Nerd Font Mono` does not exist here, only the
/// proportional variant `Symbols Nerd Font` is installed) makes every
/// glyph fall back to a fallback lottery (e.g. `f002` rendered by DejaVu
/// Sans instead of the magnifying glass).
pub const ICON_FONT: &str = "JetBrainsMono Nerd Font";

// ── Glyphs ──────────────────────────────────────────────────────────────────
pub fn glyph(g: &'static str, size: f32, color: Rgba) -> Div {
    div()
        .font_family(ICON_FONT)
        .text_size(px(size))
        .text_color(color)
        .child(g)
}

pub fn vol_glyph(volume: u8, muted: bool) -> &'static str {
    if muted || volume == 0 {
        icons::VOL_OFF
    } else if volume < 50 {
        icons::VOL_LOW
    } else {
        icons::VOL_HIGH
    }
}

pub fn bat_glyph(pct: u8) -> &'static str {
    match pct {
        88..=255 => icons::BAT_FULL,
        63..=87 => icons::BAT_3Q,
        38..=62 => icons::BAT_HALF,
        13..=37 => icons::BAT_1Q,
        _ => icons::BAT_EMPTY,
    }
}

// ── Containers ──────────────────────────────────────────────────────────────
/// Floating bar capsule (38 px).
pub fn pill() -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(38.))
        .px_3()
        .rounded_full()
        .bg(pal::surface_a(0.94))
        .border_1()
        .border_color(pal::border())
}

/// Panel card.
pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(px(RADIUS_CARD))
        .bg(pal::surface())
        .border_1()
        .border_color(pal::border())
}

/// Small rounded button (no hover: to be applied at the call site).
pub fn btn(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .h(px(28.))
        .px_2()
        .rounded_full()
        .cursor_pointer()
}

/// Full-screen backdrop: a click in the empty area closes the `window`.
/// Panels placed on top must call `.occlude()`.
pub fn backdrop(id: &'static str, window: &'static str, dim: f32) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .bg(pal::with_alpha(pal::bg(), dim))
        .on_click(move |_, _, cx| cx.hide_window(window))
}

// ── Window control ──────────────────────────────────────────────────────────
/// Opens/closes `target`, closing all other overlay surfaces.
pub fn toggle_exclusive(target: &'static str, cx: &mut App) {
    for id in [
        ids::CONTROL,
        ids::NOTIFS,
        ids::LAUNCHER,
        ids::TASKS,
        ids::POWER,
        ids::CLIPBOARD,
    ] {
        if id != target {
            cx.hide_window(id);
        }
    }
    cx.toggle_window(target);
}

/// Runs a command without blocking the UI (dedicated thread → no zombie).
///
/// Adds `~/.local/bin` to the child's `PATH`: aurora is launched with a
/// system `PATH` that does not include it, and the `change-theme/` scripts
/// call `kumizo` (installed in `~/.local/bin`) to re-theme the shell.
pub fn run(cmd: &str, args: &[&str]) {
    let cmd = cmd.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        let mut c = std::process::Command::new(&cmd);
        c.args(&args);
        if let Some(home) = std::env::var_os("HOME") {
            let local_bin = std::path::PathBuf::from(&home).join(".local/bin");
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut paths: Vec<std::path::PathBuf> =
                std::env::split_paths(&path).collect();
            if !paths.iter().any(|p| p == &local_bin) {
                paths.insert(0, local_bin);
            }
            if let Ok(joined) = std::env::join_paths(paths) {
                c.env("PATH", joined);
            }
        }
        let _ = c.status();
    });
}

pub fn key(ev: &KeyDownEvent) -> &str {
    ev.keystroke.key.as_str()
}

// ── Bars ────────────────────────────────────────────────────────────────────
pub fn progress(fraction: f32, fill: Rgba, height: f32) -> Div {
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(pal::raised())
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(fill)
                .w(DefiniteLength::Fraction(fraction.clamp(0.0, 1.0))),
        )
}

/// Clickable 20-segment slider (no bounds calculation: each segment
/// knows its value). `on_set` receives a percentage 5..=100.
pub fn segments(
    prefix: &'static str,
    value: u8,
    on_set: impl Fn(u8, &mut App) + Clone + 'static,
) -> Div {
    let mut row = div().flex().flex_row().items_center().gap(px(3.)).w_full();
    for i in 0..20usize {
        let target = ((i + 1) * 5) as u8;
        let filled = value >= target.saturating_sub(2);
        let f = on_set.clone();
        row = row.child(
            div()
                .id((prefix, i))
                .flex_1()
                .h(px(20.))
                .rounded(px(RADIUS_XS))
                .cursor_pointer()
                .bg(if filled { pal::accent() } else { pal::raised() })
                .hover(|s| s.opacity(0.75))
                .on_click(move |_, _, cx| f(target, cx)),
        );
    }
    row
}

/// Progress ring drawn on canvas (no transform/scale in this
/// GPUI snapshot). Clockwise from noon; `progress` is clamped to 0..=1.
pub fn ring(
    progress: f32,
    size: f32,
    thickness: f32,
    color: Rgba,
    track: Rgba,
) -> Canvas<()> {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.center();
            let cx = f32::from(center.x);
            let cy = f32::from(center.y);
            let r = (size - thickness) / 2.;

            let mut base = PathBuilder::stroke(px(thickness));
            circle(&mut base, cx, cy, r);
            if let Ok(path) = base.build() {
                window.paint_path(path, track);
            }

            let p = progress.clamp(0.0, 1.0);
            if p > 0.002 {
                let mut arc = PathBuilder::stroke(px(thickness));
                if p >= 0.999 {
                    circle(&mut arc, cx, cy, r);
                } else {
                    let start = -std::f32::consts::FRAC_PI_2;
                    let end = start + std::f32::consts::TAU * p;
                    arc.move_to(point(px(cx), px(cy - r)));
                    arc.arc_to(
                        point(px(r), px(r)),
                        px(0.),
                        p > 0.5,
                        true,
                        point(px(cx + r * end.cos()), px(cy + r * end.sin())),
                    );
                }
                if let Ok(path) = arc.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .w(px(size))
    .h(px(size))
}

fn circle(b: &mut PathBuilder, cx: f32, cy: f32, r: f32) {
    b.move_to(point(px(cx + r), px(cy)));
    b.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        true,
        point(px(cx - r), px(cy)),
    );
    b.arc_to(
        point(px(r), px(r)),
        px(0.),
        false,
        true,
        point(px(cx + r), px(cy)),
    );
}
