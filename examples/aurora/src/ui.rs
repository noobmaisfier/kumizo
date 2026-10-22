//! Briques UI partagées. RÈGLE : aucun helper n'appelle `.hover()` (GPUI
//! panique si `.hover()` est posé deux fois) — c'est toujours le site d'appel
//! qui le pose, une seule fois.

use gpui::prelude::*;
use gpui::{div, px, App, DefiniteLength, Div, ElementId, KeyDownEvent, Rgba, Stateful};
use kumizo_kit::prelude::*;

use crate::{icons, theme::pal};

/// Ids de fenêtres — préfixés `aurora-` (les ids sont globaux sur le socket).
pub mod ids {
    pub const BAR: &str = "aurora-bar";
    pub const LAUNCHER: &str = "aurora-launcher";
    pub const TASKS: &str = "aurora-tasks";
    pub const POWER: &str = "aurora-power";
    pub const CLIPBOARD: &str = "aurora-clipboard";
    pub const CONTROL: &str = "aurora-control";
    pub const NOTIFS: &str = "aurora-notifs";
    pub const TOASTS: &str = "aurora-toasts";
    pub const OSD: &str = "aurora-osd";
}

/// Famille Nerd Font réellement installée (même que waybar) : demander une
/// famille absente (`Symbols Nerd Font Mono` n'existe pas ici, seule la
/// variante proportionnelle `Symbols Nerd Font` est installée) fait tomber
/// chaque glyphe dans une loterie de fallback (ex. `f002` rendu par DejaVu
/// Sans au lieu de la loupe).
pub const ICON_FONT: &str = "JetBrainsMono Nerd Font";

// ── Glyphes ─────────────────────────────────────────────────────────────────
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

// ── Conteneurs ──────────────────────────────────────────────────────────────
/// Capsule flottante de la barre (38 px).
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

/// Carte de panneau.
pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_xl()
        .bg(pal::surface())
        .border_1()
        .border_color(pal::border())
}

/// Petit bouton arrondi (pas de hover : à poser au site d'appel).
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

/// Fond plein écran : un clic dans le vide ferme la fenêtre `window`.
/// Les panneaux posés dessus doivent appeler `.occlude()`.
pub fn backdrop(id: &'static str, window: &'static str, dim: f32) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .bg(pal::with_alpha(pal::bg(), dim))
        .on_click(move |_, _, cx| cx.hide_window(window))
}

// ── Contrôle des fenêtres ───────────────────────────────────────────────────
/// Ouvre/ferme `target` en fermant toutes les autres surfaces overlay.
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

/// Lance une commande sans bloquer l'UI (thread dédié → pas de zombie).
///
/// Ajoute `~/.local/bin` au `PATH` du fils : aurora est lancé avec un `PATH`
/// système qui ne le contient pas, et les scripts `change-theme/` appellent
/// `kumizo` (installé dans `~/.local/bin`) pour re-thèmer le shell.
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

// ── Barres ──────────────────────────────────────────────────────────────────
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

/// Slider cliquable à 20 segments (pas de calcul de bounds : chaque segment
/// connaît sa valeur). `on_set` reçoit un pourcentage 5..=100.
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
                .rounded_sm()
                .cursor_pointer()
                .bg(if filled { pal::accent() } else { pal::raised() })
                .hover(|s| s.opacity(0.75))
                .on_click(move |_, _, cx| f(target, cx)),
        );
    }
    row
}
