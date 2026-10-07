use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, Div, Entity, px, Window};
use widgetkit::prelude::*;

use crate::theme::{hard, misprint, out_back, pal, tile, CALENDAR_HEIGHT, CALENDAR_WIDTH, POPUP_TOP};

pub fn window() -> LayerWindow {
    LayerWindow::new("calendar")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(CALENDAR_WIDTH)
        .height(CALENDAR_HEIGHT)
        .margins(Margins::new(POPUP_TOP, 14, 0, 0))
        .namespace("wk-calendar")
        .start_hidden(true)
}

pub struct Calendar {
    clock: Entity<Clock>,
    offset: i32,
}

impl Calendar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            offset: 0,
        }
    }
}

fn ring(left: f32) -> Div {
    div()
        .absolute()
        .top(px(-9.0))
        .left(px(left))
        .w(px(12.0))
        .h(px(24.0))
        .rounded_full()
        .bg(pal::paper())
        .border_2()
        .border_color(pal::ink())
}

impl Render for Calendar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const NAMES: [&str; 12] = [
            "January", "February", "March", "April", "May", "June", "July", "August",
            "September", "October", "November", "December",
        ];
        let dt = self.clock.read(cx).datetime();
        let today = dt.day();
        let mut year = dt.year();
        let mut month = dt.month();
        for _ in 0..self.offset.unsigned_abs() {
            if self.offset > 0 {
                if month == time::Month::December {
                    year += 1;
                }
                month = month.next();
            } else {
                if month == time::Month::January {
                    year -= 1;
                }
                month = month.previous();
            }
        }
        let is_current = self.offset == 0;
        let month_name = NAMES[month as usize - 1];
        let days_in_month = time::util::days_in_month(month, year);
        let first_weekday = time::Date::from_calendar_date(year, month, 1)
            .map(|d| d.weekday().number_from_monday())
            .unwrap_or(1);
        let leading = (first_weekday - 1) as usize;
        let total_slots = leading + days_in_month as usize;
        let rows = (total_slots + 6) / 7;
        let page_key = format!("cal-page-{}", self.offset);

        let grid = div()
            .flex()
            .flex_col()
            .gap_1()
            .children((0..rows).map(|row| {
                div().flex().flex_row().justify_between().children((0..7usize).map(move |col| {
                    let slot = row * 7 + col;
                    if slot < leading || slot >= total_slots {
                        return div().w(px(38.0)).h(px(34.0)).into_any_element();
                    }
                    let day_num = (slot - leading + 1) as u8;
                    let is_today = is_current && day_num == today;
                    div()
                        .w(px(38.0))
                        .h(px(34.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.0))
                        .when(is_today, |s| {
                            s.bg(pal::pink())
                                .border_2()
                                .border_color(pal::ink())
                                .shadow(hard(pal::ink(), 2.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                        })
                        .when(!is_today, |s| {
                            s.border_1()
                                .border_color(pal::faint())
                                .text_color(if col >= 5 { pal::blue() } else { pal::ink() })
                                .hover(|h| h.bg(pal::yellow()))
                        })
                        .child(format!("{day_num}"))
                        .into_any_element()
                }))
            }))
            // Flipping a month drops a fresh page onto the pad.
            .with_animation(page_key, Animation::new(Duration::from_millis(380)), |el, d| {
                el.relative().top(px(-26.0 * (1.0 - out_back(d)))).opacity(d.min(1.0))
            });

        let mini = |id: &'static str, glyph: &'static str| {
            tile(id).w(px(30.0)).h(px(30.0)).shadow(hard(pal::ink(), 2.0)).child(glyph)
        };

        let card = div()
            .id("calendar-card")
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .bg(pal::white())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 5.0))
            .on_click(|_, _, _| {})
            .child(ring(70.0))
            .child(ring(CALENDAR_WIDTH - 70.0 - 12.0 - 14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .bg(pal::blue())
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .child(misprint(month_name.to_string(), 24.0, pal::paper(), pal::pink()))
                            .child(
                                div()
                                    .pb_1()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::yellow())
                                    .child(format!("{year}")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(mini("cal-prev", "‹").on_click(cx.listener(|this, _, _, cx| {
                                this.offset -= 1;
                                cx.notify();
                            })))
                            .when(!is_current, |s| {
                                s.child(
                                    mini("cal-today", "•")
                                        .bg(pal::yellow())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.offset = 0;
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(mini("cal-next", "›").on_click(cx.listener(|this, _, _, cx| {
                                this.offset += 1;
                                cx.notify();
                            }))),
                    ),
            )
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(
                div().flex().flex_col().gap_2().p_3().child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .children(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate().map(
                            |(i, day)| {
                                div()
                                    .w(px(38.0))
                                    .flex()
                                    .justify_center()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(if i >= 5 { pal::blue() } else { pal::ink() })
                                    .child(*day)
                            },
                        )),
                )
                .child(grid),
            );

        div()
            .id("calendar-backdrop")
            .w_full()
            .h_full()
            .pt(px(12.0))
            .pr(px(8.0))
            .pb(px(8.0))
            .on_click(|_, _, cx| {
                cx.hide_window("calendar");
            })
            .child(card)
    }
}
