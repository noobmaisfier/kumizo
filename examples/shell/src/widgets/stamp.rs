use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, Entity, px, Window};
use kumizo_kit::prelude::*;

use crate::theme::{hard, misprint, out_back, pal, STAMP_HEIGHT, STAMP_WIDTH};

pub fn window() -> LayerWindow {
    LayerWindow::new("stamp")
        .layer(Layer::Top)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(STAMP_WIDTH)
        .height(STAMP_HEIGHT)
        .margins(Margins::new(10, 14, 0, 0))
        .namespace("wk-stamp")
}

pub struct Stamp {
    clock: Entity<Clock>,
    mpris: Entity<Mpris>,
}

impl Stamp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            mpris: cx.service::<Mpris>(),
        }
    }
}

impl Render for Stamp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let mpris = self.mpris.read(cx);
        let time = format!("{}", clock.time_hms());
        let line2 = if mpris.title().is_some() {
            format!("\u{f00e}  {}", mpris.display_label())
        } else {
            format!("{}", clock.date_formatted())
        };

        let panel = div()
            .id("wk-clock")
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(pal::yellow())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 5.0))
            .cursor_pointer()
            .hover(|s| s.bg(pal::white()))
            .active(|s| s.shadow(Vec::new()).top(px(5.0)).left(px(5.0)))
            .on_click(|_, _w, cx| cx.toggle_window("calendar"))
            // Registration bar: the three plates of the print.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .h(px(7.0))
                    .child(div().flex_1().bg(pal::pink()))
                    .child(div().flex_1().bg(pal::blue()))
                    .child(div().flex_1().bg(pal::ink())),
            )
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px_3()
                    .pt_1()
                    .child(misprint(time, 28.0, pal::blue(), pal::pink()))
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child(line2),
                    ),
            );

        div().w_full().h_full().pr(px(6.0)).pb(px(6.0)).child(panel.with_animation(
            "stamp-in",
            Animation::new(Duration::from_millis(560)),
            |el, d| {
                let t = out_back(d);
                el.relative().top(px(-46.0 * (1.0 - t))).opacity(d.min(1.0))
            },
        ))
    }
}
