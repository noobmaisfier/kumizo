use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, AnyElement, Context, Entity, FocusHandle, KeyDownEvent, px, Window};
use kumizo_kit::prelude::*;

use crate::theme::{find_field, fullscreen, hard, key_chip, out_cubic, pal, printed};

pub fn window() -> LayerWindow {
    fullscreen("launcher", "wk-launcher")
}

pub struct AppLauncher {
    apps: Entity<Apps>,
    query: String,
    selected_index: usize,
    focus: FocusHandle,
}

impl AppLauncher {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            apps: cx.service::<Apps>(),
            query: String::new(),
            selected_index: 0,
            focus: cx.focus_handle(),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let matches = self.apps.read(cx).search(&self.query);
        let count = matches.len();

        match key {
            "escape" => {
                self.query.clear();
                self.selected_index = 0;
                cx.hide_window("launcher");
            }
            "enter" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let app = &matches[idx];
                    self.apps.read(cx).launch(app);
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("launcher");
                }
            }
            "up" => {
                if count > 0 {
                    self.selected_index =
                        if self.selected_index == 0 { count - 1 } else { self.selected_index - 1 };
                    cx.notify();
                }
            }
            "down" | "tab" => {
                if count > 0 {
                    self.selected_index = (self.selected_index + 1) % count;
                    cx.notify();
                }
            }
            "backspace" => {
                self.query.pop();
                self.selected_index = 0;
                cx.notify();
            }
            "space" => {
                self.query.push(' ');
                self.selected_index = 0;
                cx.notify();
            }
            _ => {
                if let Some(ref text) = event.keystroke.key_char {
                    for c in text.chars() {
                        if !c.is_control() {
                            self.query.push(c);
                            self.selected_index = 0;
                            cx.notify();
                        }
                    }
                } else if key.chars().count() == 1 {
                    if let Some(c) = key.chars().next() {
                        if !c.is_control() {
                            self.query.push(c);
                            self.selected_index = 0;
                            cx.notify();
                        }
                    }
                }
            }
        }
    }
}

impl Render for AppLauncher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let matches = self.apps.read(cx).search(&self.query);
        let total = matches.len();
        let selected = self.selected_index.min(total.saturating_sub(1));
        let apps_service = self.apps.clone();

        let max_visible = 6;
        let start_idx = if selected >= max_visible { selected - max_visible + 1 } else { 0 };
        let visible_items: Vec<_> =
            matches.iter().enumerate().skip(start_idx).take(max_visible).collect();

        let card = div()
            .id("launcher-modal")
            .w(px(640.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .on_click(|_, _, _| {})
            .child(
                find_field("Find", pal::yellow(), &self.query, "Type an app name").child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(pal::blue())
                        .child(format!("{total}")),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .when(total == 0, |s| {
                        s.py_8()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child("No app by that name. Try fewer letters.")
                    })
                    .children(visible_items.into_iter().map(|(idx, app)| {
                        let is_selected = idx == selected;
                        let app_item = app.clone();
                        let apps_ref = apps_service.clone();
                        let app_key = format!("launcher-sel-{}", app.id);

                        let icon_el: AnyElement = if let Some(img) = &app.icon {
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::white())
                                .border_2()
                                .border_color(pal::ink())
                                .child(gpui::img(img.clone()).w(px(28.0)).h(px(28.0)))
                                .into_any_element()
                        } else {
                            let letter = app
                                .name
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string();
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::pink())
                                .border_2()
                                .border_color(pal::ink())
                                .text_size(px(18.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(letter)
                                .into_any_element()
                        };

                        let comment = if app.comment.is_empty() {
                            app.exec.clone()
                        } else {
                            app.comment.clone()
                        };

                        let row = div()
                            .id(format!("launcher-item-{}", app.id))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .border_2()
                            .cursor_pointer()
                            .when(is_selected, |s| {
                                s.bg(pal::yellow())
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                            })
                            .when(!is_selected, |s| {
                                s.border_color(pal::clear()).hover(|h| h.bg(pal::tint()))
                            })
                            .child(
                                div()
                                    .w(px(12.0))
                                    .text_size(px(14.0))
                                    .text_color(pal::blue())
                                    .child(if is_selected { "\u{25B6}" } else { "" }),
                            )
                            .child(icon_el)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(15.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .child(app.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(pal::ink())
                                            .opacity(0.65)
                                            .truncate()
                                            .child(comment),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                apps_ref.read(cx).launch(&app_item);
                                cx.hide_window("launcher");
                            });

                        // The selected row snaps in from the left.
                        if is_selected {
                            row.with_animation(
                                app_key,
                                Animation::new(Duration::from_millis(200)),
                                |el, d| el.relative().left(px(-12.0 * (1.0 - out_cubic(d)))),
                            )
                            .into_any_element()
                        } else {
                            row.into_any_element()
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(key_chip("↑↓", "move"))
                    .child(key_chip("Enter", "open"))
                    .child(key_chip("Esc", "close")),
            );

        div()
            .id("launcher-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(pal::veil())
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .pt(px(120.0))
            .on_click(|_, _, cx| {
                cx.hide_window("launcher");
            })
            .child(printed(card, "launcher", pal::pink()))
    }
}
