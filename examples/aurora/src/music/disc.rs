//! Briques visuelles du lecteur : pochette, barre de lecture, boutons.
//!
//! L'ancre visuelle est la pochette grand format (300 px). Les paroles vivent
//! dans l'onglet Paroles à droite : aucun bandeau karaoké ici (doublon).
//! Zéro minuteur dédié : le ticker 500 ms du `Player` suffit.

use gpui::prelude::*;
use gpui::{div, px, App, Div, Entity, Rgba, Stateful};

use super::model::{Player, Repeat};
use crate::{
    icons,
    theme::pal,
    ui::{self},
};

/// Pochette grand format (300 px, coins arrondis) : l'image de l'album
/// (`cover.jpg` du dossier ou art intégré aux tags, voir `library::cover_for`),
/// ou un glyphe note en repli. `gpui::img` sait lire un `PathBuf`.
pub fn cover_large(cover: Option<std::path::PathBuf>) -> Div {
    const S: f32 = 300.0;
    let picture: gpui::AnyElement = match cover {
        Some(path) => gpui::img(path)
            .w(px(S))
            .h(px(S))
            .into_any_element(),
        None => div()
            .size(px(S))
            .flex()
            .items_center()
            .justify_center()
            .bg(pal::raised())
            .child(ui::glyph(icons::MUSIC, 64., pal::muted()))
            .into_any_element(),
    };
    div()
        .w(px(S))
        .h(px(S))
        .flex_none()
        .rounded_xl()
        .overflow_hidden()
        .child(picture)
}

/// Barre de lecture cliquable : 48 segments, chacun connaît sa fraction.
pub fn seek_bar(frac: f32, on_seek: impl Fn(f32, &mut App) + Clone + 'static) -> Div {
    const N: usize = 48;
    let filled = (frac.clamp(0.0, 1.0) * N as f32).round() as usize;
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.))
        .w_full()
        .h(px(16.));
    for i in 0..N {
        let f = on_seek.clone();
        let target = (i as f32 + 0.5) / N as f32;
        row = row.child(
            div()
                .id(("mu-seek", i))
                .flex_1()
                .h(px(8.))
                .rounded_sm()
                .cursor_pointer()
                .bg(if i < filled { pal::accent() } else { pal::raised() })
                .hover(|s| s.opacity(0.75))
                .on_click(move |_, _, cx| f(target, cx)),
        );
    }
    row
}

/// Bouton-icône. Pose son propre `.hover()` : les appelants n'y touchent pas.
pub fn icon_btn(id: &'static str, glyph: &'static str, size: f32, color: Rgba) -> Stateful<Div> {
    ui::btn(id)
        .w(px(34.))
        .h(px(34.))
        .hover(|s| s.bg(pal::raised()))
        .child(ui::glyph(glyph, size, color))
}

/// Rangée de commandes : aléatoire · précédent · lecture/pause · suivant · répétition.
pub fn controls(player: &Entity<Player>, playing: bool, shuffle: bool, repeat: Repeat) -> Div {
    let (p_shuffle, p_prev, p_play, p_next, p_repeat) = (
        player.clone(),
        player.clone(),
        player.clone(),
        player.clone(),
        player.clone(),
    );

    let on = |active: bool| if active { pal::accent() } else { pal::muted() };

    let shuffle_btn = icon_btn("mu-shuffle", icons::SHUFFLE, 14., on(shuffle))
        .on_click(move |_, _, cx| {
            p_shuffle.update(cx, |p, cx| p.toggle_shuffle(cx));
        });

    let prev_btn = icon_btn("mu-prev", icons::PREV, 15., pal::text()).on_click(move |_, _, cx| {
        p_prev.update(cx, |p, cx| p.prev(cx));
    });

    let play_btn = ui::btn("mu-play")
        .w(px(56.))
        .h(px(56.))
        .rounded_full()
        .bg(pal::accent())
        .hover(|s| s.opacity(0.85))
        .on_click(move |_, _, cx| {
            p_play.update(cx, |p, cx| p.toggle(cx));
        })
        .child(ui::glyph(
            if playing { icons::PAUSE } else { icons::PLAY },
            20.,
            pal::on_accent(),
        ));

    let next_btn = icon_btn("mu-next", icons::NEXT, 15., pal::text()).on_click(move |_, _, cx| {
        p_next.update(cx, |p, cx| p.next(cx));
    });

    // L'état de répétition est dit en texte (« 1 » / « ∞ »), pas seulement en couleur.
    let repeat_btn = icon_btn("mu-repeat", icons::REPEAT, 14., on(repeat != Repeat::Off))
        .on_click(move |_, _, cx| {
            p_repeat.update(cx, |p, cx| p.cycle_repeat(cx));
        })
        .child(div().text_xs().text_color(pal::accent()).child(match repeat {
            Repeat::Off => "",
            Repeat::All => "∞",
            Repeat::One => "1",
        }));

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .child(shuffle_btn)
        .child(prev_btn)
        .child(play_btn)
        .child(next_btn)
        .child(repeat_btn)
}
