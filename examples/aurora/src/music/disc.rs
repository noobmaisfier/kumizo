//! Briques visuelles du lecteur : vinyle SVG en 3 calques, barre de lecture, boutons.
//!
//! GPUI ne sait ni pivoter une image (`Img`), ni rendre un SVG riche
//! (`svg()` ne sort qu'un masque alpha monochrome, `window.rs:4849`) : le
//! vinyle est donc découpé en 3 calques rasterisés via `resvg` — socle fixe,
//! disque complet (18 angles), bras (8 angles) — superposés en absolu, comme
//! les groupes `#vinyl-disc` / `#tonearm` de la démo d'origine. Les couleurs
//! suivent le thème courant. Zéro CPU au repos : les images sont figées,
//! seuls le basculement du bras (~700 ms au play/pause) et la rotation du
//! disque (pendant la lecture) avancent, pilotés par le modèle.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::prelude::*;
use gpui::{div, px, App, Div, Entity, RenderImage, Rgba, Stateful};
use image::{Frame, ImageBuffer};
use smallvec::smallvec;

use super::model::{Player, Repeat};
use crate::{
    icons,
    theme::{self, pal},
    ui::{self},
};

const BASE_TEMPLATE: &str = include_str!("vinyl-base.svg");
const RECORD_TEMPLATE: &str = include_str!("vinyl-record.svg");
const ARM_TEMPLATE: &str = include_str!("vinyl-arm.svg");

/// Affichage : 300×240 (ratio 5:4 du SVG).
const DISP_W: f32 = 300.0;
const DISP_H: f32 = 240.0;
/// Disque : boîte 336×336 à (12, 32) unités → position/taille d'affichage.
const RECORD_X: f32 = 7.0;
const RECORD_Y: f32 = 19.0;
const RECORD_S: f32 = 202.0;

/// Angles du bras : indice 0 = posé sur le disque … 7 = relevé au repos.
/// Espacement ease-in-out (petits pas aux bouts, grands au milieu) : approxime
/// le `cubic-bezier(0.65, 0, 0.35, 1)` de la démo d'origine sur ~700 ms.
pub const ARM_DOWN: usize = 0;
pub const ARM_UP: usize = 7;
const ARM_DEGS: [f32; 8] = [0.0, -1.6, -6.2, -14.0, -24.0, -31.8, -36.4, -38.0];
/// Positions du disque : 18 × 20° = un tour (1,8 s à 100 ms, 33 tours/min).
pub const SPIN_FRAMES: usize = 18;

/// Un rendu par clé `calque:thème:indice`. Un seul thème en mémoire à la
/// fois : tout changement de thème purge (`CACHE_THEME`), et `evict()` vide
/// à la fermeture de la fenêtre. Rendu en 1× (300×240 max) : ~5 Mo au lieu
/// de ~17 Mo en 2×, indiscernable à la taille d'affichage.
static VINYL_CACHE: OnceLock<Mutex<HashMap<String, Arc<RenderImage>>>> = OnceLock::new();
/// Thème du contenu actuel du cache (`""` = vide).
static CACHE_THEME: OnceLock<Mutex<String>> = OnceLock::new();

/// Base `fontdb` partagée : `load_system_fonts` coûte ~100 ms, une seule fois.
static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();

fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

fn rasterize(svg: &[u8], w: u32, h: u32, vw: f32, vh: f32) -> Option<Arc<RenderImage>> {
    let mut opt = resvg::usvg::Options::default();
    // Sans ça, les textes du label (« VINYL »…) sont ignorés (base vide).
    opt.fontdb = fonts();
    let tree = resvg::usvg::Tree::from_data(svg, &opt).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(w as f32 / vw, h as f32 / vh),
        &mut pixmap.as_mut(),
    );
    let mut bgra = pixmap.take();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}

fn cached(key: &str, render: impl FnOnce() -> Option<Arc<RenderImage>>) -> Option<Arc<RenderImage>> {
    let cache = VINYL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let tag_lock = CACHE_THEME.get_or_init(|| Mutex::new(String::new()));
    let theme_tag = theme::current().name.to_string();
    {
        let mut c = cache.lock().ok()?;
        let mut tag = tag_lock.lock().ok()?;
        // Un seul thème à la fois : le changement de thème jette l'ancien.
        if *tag != theme_tag {
            c.clear();
            *tag = theme_tag;
        }
        if let Some(img) = c.get(key).cloned() {
            return Some(img);
        }
    }
    // Hors verrou : le rendu resvg prend des millisecondes.
    let img = render()?;
    if let Ok(mut c) = cache.lock() {
        c.insert(key.to_string(), img.clone());
    }
    Some(img)
}

/// Vide tout le cache (~5 Mo libérés, re-rasterisé en fond à la prochaine
/// ouverture via `prewarm`). Appelé à la fermeture de la fenêtre musique ;
/// sans appel, le cache dort jusqu'au prochain changement de thème.
pub fn evict() {
    if let Some(cache) = VINYL_CACHE.get() {
        if let Ok(mut c) = cache.lock() {
            c.clear();
        }
    }
    if let Some(tag) = CACHE_THEME.get() {
        if let Ok(mut t) = tag.lock() {
            t.clear();
        }
    }
}

fn base_image() -> Option<Arc<RenderImage>> {
    let t = theme::current();
    let key = format!("base:{}", t.name);
    cached(&key, || {
        let svg = BASE_TEMPLATE
            .replace("{SURFACE}", &hex(t.surface))
            .replace("{BORDER}", &hex(t.border))
            .into_bytes();
        rasterize(&svg, 300, 240, 500.0, 400.0)
    })
}

fn arm_image(idx: usize) -> Option<Arc<RenderImage>> {
    let t = theme::current();
    let idx = idx.min(ARM_UP);
    let key = format!("arm:{}:{idx}", t.name);
    cached(&key, || {
        let svg = ARM_TEMPLATE
            .replace("{ACCENT}", &hex(t.accent))
            .replace("{DEG}", &format!("{}", ARM_DEGS[idx]))
            .into_bytes();
        rasterize(&svg, 300, 240, 500.0, 400.0)
    })
}

fn record_image(idx: usize) -> Option<Arc<RenderImage>> {
    let t = theme::current();
    let idx = idx % SPIN_FRAMES;
    let key = format!("record:{}:{idx}", t.name);
    cached(&key, || {
        let svg = RECORD_TEMPLATE
            .replace("{ACCENT}", &hex(t.accent))
            .replace("{ACCENT2}", &hex(t.accent2))
            .replace("{DEG}", &format!("{}", idx as f32 * 20.0))
            .into_bytes();
        rasterize(&svg, 202, 202, 336.0, 336.0)
    })
}

/// Pré-rend tous les calques du thème courant en arrière-plan (zéro jank à
/// la première ouverture) : à appeler une fois, le reste est paresseux.
pub fn prewarm(cx: &mut App) {
    cx.background_executor()
        .spawn(async {
            base_image();
            for i in 0..=ARM_UP {
                arm_image(i);
            }
            for i in 0..SPIN_FRAMES {
                record_image(i);
            }
        })
        .detach();
}

/// Vinyle + bras + disque, centré dans sa colonne. `arm_idx` 0 = bras posé
/// … 7 = relevé ; `spin_idx` = position de rotation du disque.
/// Pas de cadre ajouté : le style vient du SVG (thème actif injecté).
pub fn vinyl(arm_idx: usize, spin_idx: usize) -> Div {
    let base: gpui::AnyElement = match base_image() {
        Some(img) => gpui::img(img)
            .w(px(DISP_W))
            .h(px(DISP_H))
            .into_any_element(),
        None => div()
            .w(px(DISP_W))
            .h(px(DISP_H))
            .flex()
            .items_center()
            .justify_center()
            .rounded_xl()
            .bg(pal::raised())
            .child(ui::glyph(icons::MUSIC, 48., pal::muted()))
            .into_any_element(),
    };
    let mut stack = div()
        .relative()
        .w(px(DISP_W))
        .h(px(DISP_H))
        .child(base);
    // Ordre de peinture = ordre des enfants : le disque d'abord, le bras
    // PAR-DESSUS (comme dans le SVG d'origine : #tonearm après #vinyl-disc).
    // Inversé, la cellule disparaît sous le disque.
    if let Some(img) = record_image(spin_idx) {
        stack = stack.child(
            div()
                .absolute()
                .top(px(RECORD_Y))
                .left(px(RECORD_X))
                .child(gpui::img(img).w(px(RECORD_S)).h(px(RECORD_S))),
        );
    }
    if let Some(img) = arm_image(arm_idx) {
        stack = stack.child(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .child(gpui::img(img).w(px(DISP_W)).h(px(DISP_H))),
        );
    }
    div().w_full().flex().items_center().justify_center().child(stack)
}
/// Vignette de pochette (48 px, coins arrondis) : l'image de l'album
/// (`cover.jpg` du dossier ou art intégré aux tags, voir `library::cover_for`),
/// ou un glyphe note en repli. `gpui::img` sait lire un `PathBuf`.
pub fn cover_thumb(cover: Option<std::path::PathBuf>) -> Div {
    const S: f32 = 48.0;
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
            .child(ui::glyph(icons::MUSIC, 20., pal::muted()))
            .into_any_element(),
    };
    div()
        .size(px(S))
        .flex_none()
        .rounded_lg()
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
