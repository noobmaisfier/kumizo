//! OSD volume / luminosité.
//!
//! Le déclenchement vit dans la BARRE (toujours vivante) et non dans la vue
//! OSD : une fenêtre `start_hidden` peut ne pas exister avant sa première
//! ouverture, donc elle ne pourrait pas observer les services à temps.
//! La barre écrit dans `OsdState` (entité globale) ; la vue OSD l'affiche.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{div, px, App, Context, Entity, Global, Window};
use widgetkit::prelude::*;

use crate::{
    icons,
    theme::pal,
    ui::{self, ids},
};

#[derive(Clone, Copy, PartialEq)]
pub enum OsdKind {
    Volume,
    Brightness,
}

pub struct OsdState {
    kind: OsdKind,
    value: u8,
    muted: bool,
    visible: bool,
    generation: u64,
}

impl OsdState {
    fn new() -> Self {
        Self {
            kind: OsdKind::Volume,
            value: 0,
            muted: false,
            visible: false,
            generation: 0,
        }
    }

    pub fn show(&mut self, kind: OsdKind, value: u8, muted: bool, cx: &mut Context<Self>) {
        self.kind = kind;
        self.value = value;
        self.muted = muted;
        self.visible = true;
        // Jeton de génération : seul le DERNIER timer a le droit de masquer.
        self.generation = self.generation.wrapping_add(1);
        let current_gen = self.generation;

        cx.show_window(ids::OSD);
        cx.notify();

        cx.spawn(async move |this, cx| {
            async_io::Timer::after(Duration::from_millis(1800)).await;
            let _ = this.update(cx, |osd, cx| {
                if osd.generation == current_gen {
                    osd.visible = false;
                    cx.notify();
                    cx.hide_window(ids::OSD);
                }
            });
        })
        .detach();
    }
}

struct OsdGlobal(Entity<OsdState>);
impl Global for OsdGlobal {}

pub fn handle(cx: &mut App) -> Entity<OsdState> {
    if !cx.has_global::<OsdGlobal>() {
        let e = cx.new(|_| OsdState::new());
        cx.set_global(OsdGlobal(e));
    }
    cx.global::<OsdGlobal>().0.clone()
}

// ── Vue ─────────────────────────────────────────────────────────────────────
pub struct Osd {
    state: Entity<OsdState>,
}

impl Osd {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = handle(cx);
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self { state }
    }
}

impl Render for Osd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.state.read(cx);
        if !s.visible {
            return div().into_any_element();
        }

        let (g, label) = match s.kind {
            OsdKind::Volume => (
                ui::vol_glyph(s.value, s.muted),
                if s.muted {
                    "Muet".to_string()
                } else {
                    format!("{}%", s.value)
                },
            ),
            OsdKind::Brightness => (icons::SUN, format!("{}%", s.value)),
        };
        let fraction = if s.muted { 0.0 } else { s.value as f32 / 100.0 };

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .h(px(52.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .rounded_full()
                    .bg(pal::surface_a(0.96))
                    .border_1()
                    .border_color(pal::border())
                    .child(ui::glyph(g, 18., pal::accent()))
                    .child(div().flex_1().child(ui::progress(fraction, pal::accent(), 6.)))
                    .child(
                        div()
                            .w(px(48.))
                            .text_sm()
                            .text_color(pal::text())
                            .child(label),
                    ),
            )
            .into_any_element()
    }
}
