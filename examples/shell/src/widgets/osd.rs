use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, Div, Entity, px, Rgba, Window};
use kumizo_kit::prelude::*;

use crate::theme::{hard, misprint, out_back, pal, OSD_HEIGHT, OSD_WIDTH};

pub fn window() -> LayerWindow {
    LayerWindow::new("osd")
        .layer(Layer::Overlay)
        .anchor(Anchor::BOTTOM)
        .exclusive_zone(0)
        .width(OSD_WIDTH)
        .height(OSD_HEIGHT)
        .margins(Margins::new(0, 0, 56, 0))
        .namespace("wk-osd")
}

pub struct VolumeOsd {
    audio: Entity<Audio>,
    brightness: Entity<Brightness>,
    visible: bool,
    shown: OsdKind,
    last_volume: u8,
    last_muted: bool,
    last_brightness: u8,
    generation: u64,
    session: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OsdKind {
    Volume,
    Brightness,
}

fn reveal_osd(osd: &mut VolumeOsd, cx: &mut Context<VolumeOsd>, kind: OsdKind) {
    if !osd.visible {
        osd.session = osd.session.wrapping_add(1);
    }
    osd.shown = kind;
    osd.visible = true;
    osd.generation = osd.generation.wrapping_add(1);
    let current_gen = osd.generation;
    cx.notify();

    cx.spawn(async move |this, cx| {
        async_io::Timer::after(Duration::from_millis(2000)).await;
        let _ = this.update(cx, |osd, cx| {
            if osd.generation == current_gen {
                osd.visible = false;
                cx.notify();
            }
        });
    })
    .detach();
}

impl VolumeOsd {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let brightness = cx.service::<Brightness>();
        let last_volume = audio.read(cx).volume();
        let last_muted = audio.read(cx).is_muted();
        let last_brightness = brightness.read(cx).percent();

        let osd = Self {
            audio,
            brightness,
            visible: false,
            shown: OsdKind::Volume,
            last_volume,
            last_muted,
            last_brightness,
            generation: 0,
            session: 0,
        };

        let watched = osd.audio.clone();
        cx.observe(&watched, |this, audio, cx| {
            let volume = audio.read(cx).volume();
            let muted = audio.read(cx).is_muted();
            if volume == this.last_volume && muted == this.last_muted {
                return;
            }
            this.last_volume = volume;
            this.last_muted = muted;
            reveal_osd(this, cx, OsdKind::Volume);
        })
        .detach();

        let watched_brightness = osd.brightness.clone();
        cx.observe(&watched_brightness, |this, brightness, cx| {
            let level = brightness.read(cx).percent();
            if level == this.last_brightness {
                return;
            }
            this.last_brightness = level;
            reveal_osd(this, cx, OsdKind::Brightness);
        })
        .detach();

        osd
    }
}

const OSD_BLOCKS: usize = 16;

/// Segmented level bar. Each newly filled block pops up like a type slug.
fn segments(pct: usize, fill: Rgba) -> Div {
    let filled = (pct * OSD_BLOCKS + 99) / 100;
    div()
        .flex()
        .flex_row()
        .items_end()
        .gap(px(2.0))
        .h(px(18.0))
        .children((0..OSD_BLOCKS).map(move |i| {
            let block = div().w(px(10.0)).border_1().border_color(pal::ink());
            if i < filled {
                block
                    .bg(fill)
                    .with_animation(
                        format!("seg-{i}"),
                        Animation::new(Duration::from_millis(300)),
                        |el, d| el.h(px((18.0 * out_back(d)).max(0.0))),
                    )
                    .into_any_element()
            } else {
                block.h(px(18.0)).bg(pal::white()).into_any_element()
            }
        }))
}

impl Render for VolumeOsd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().w_full().h_full();
        }

        let (icon, label, pct, fill) = match self.shown {
            OsdKind::Volume => {
                let audio = self.audio.read(cx);
                let muted = audio.is_muted();
                (
                    audio.icon(),
                    if muted { "Muted" } else { "Volume" },
                    audio.volume(),
                    if muted { pal::pink() } else { pal::blue() },
                )
            }
            OsdKind::Brightness => {
                let b = self.brightness.read(cx);
                (b.icon(), "Brightness", b.percent(), pal::yellow())
            }
        };

        div().flex().items_start().justify_center().w_full().h_full().pt(px(4.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .w(px(OSD_WIDTH - 28.0))
                .p_4()
                .bg(pal::white())
                .border_2()
                .border_color(pal::ink())
                .shadow(hard(pal::ink(), 6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(46.0))
                        .h(px(46.0))
                        .bg(fill)
                        .border_2()
                        .border_color(pal::ink())
                        .text_size(px(22.0))
                        .text_color(if fill == pal::blue() { pal::paper() } else { pal::ink() })
                        .child(icon),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap_1p5()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(label),
                        )
                        .child(segments(pct as usize, fill)),
                )
                .child(
                    div()
                        .w(px(64.0))
                        .flex()
                        .justify_end()
                        .child(misprint(format!("{pct}"), 34.0, pal::blue(), pal::pink())),
                )
                .with_animation(
                    format!("osd-in-{}", self.session),
                    Animation::new(Duration::from_millis(340)),
                    |el, d| {
                        let t = out_back(d);
                        el.relative().top(px(26.0 * (1.0 - t))).opacity(d.min(1.0))
                    },
                ),
        )
    }
}
