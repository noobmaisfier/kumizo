use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, AnyElement, Context, Entity, FocusHandle, KeyDownEvent, px, Window};
use widgetkit::prelude::*;

use crate::theme::{find_field, fullscreen, hard, key_chip, out_cubic, pal, printed};

pub fn window() -> LayerWindow {
    fullscreen("tasks", "wk-tasks")
}

pub struct TaskSwitcher {
    focus: FocusHandle,
    compositor: Entity<Compositor>,
    apps: Entity<Apps>,
    query: String,
    selected_index: usize,
}

impl TaskSwitcher {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            compositor: cx.service::<Compositor>(),
            apps: cx.service::<Apps>(),
            query: String::new(),
            selected_index: 0,
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let comp = self.compositor.read(cx);
        let all_windows = comp.windows();
        let q = self.query.trim().to_lowercase();
        let filtered: Vec<_> = all_windows
            .iter()
            .filter(|w| {
                if q.is_empty() {
                    true
                } else {
                    w.title.to_lowercase().contains(&q) || w.class.to_lowercase().contains(&q)
                }
            })
            .collect();
        let count = filtered.len();

        match key {
            "escape" => {
                self.query.clear();
                self.selected_index = 0;
                cx.hide_window("tasks");
            }
            "enter" => {
                if !filtered.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let target = filtered[idx];
                    let con_id = target.id;
                    self.compositor.read(cx).focus_window(con_id);
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("tasks");
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

impl Render for TaskSwitcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let comp = self.compositor.read(cx);
        let apps = self.apps.read(cx);
        let all_windows = comp.windows();
        let q = self.query.trim().to_lowercase();
        let filtered: Vec<_> = all_windows
            .iter()
            .filter(|w| {
                if q.is_empty() {
                    true
                } else {
                    w.title.to_lowercase().contains(&q) || w.class.to_lowercase().contains(&q)
                }
            })
            .collect();

        let total = filtered.len();
        let selected = self.selected_index.min(total.saturating_sub(1));
        let max_visible = 6;
        let start_idx = if selected >= max_visible { selected - max_visible + 1 } else { 0 };
        let visible_items: Vec<_> =
            filtered.iter().enumerate().skip(start_idx).take(max_visible).collect();

        let card = div()
            .id("tasks-modal")
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
                find_field("Windows", pal::yellow(), &self.query, "Search open windows").child(
                    div()
                        .px_2()
                        .py_0p5()
                        .bg(pal::white())
                        .border_1()
                        .border_color(pal::ink())
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(pal::ink())
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
                            .child("No open windows matching.")
                    })
                    .children(visible_items.into_iter().map(|(idx, &win)| {
                        let is_selected = idx == selected;
                        let con_id = win.id;
                        let comp_ref = self.compositor.clone();
                        let app_icon = apps.find_icon(&win.class);
                        let item_key = format!("task-sel-{}", win.id);

                        let icon_el: AnyElement = if let Some(img) = app_icon {
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::white())
                                .border_2()
                                .border_color(pal::ink())
                                .child(gpui::img(img).w(px(26.0)).h(px(26.0)))
                                .into_any_element()
                        } else {
                            let letter = win
                                .class
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
                                .bg(pal::tint())
                                .border_2()
                                .border_color(pal::ink())
                                .text_size(px(18.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(letter)
                                .into_any_element()
                        };

                        let row = div()
                            .id(format!("task-item-{}", win.id))
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
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_size(px(14.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(pal::ink())
                                                    .truncate()
                                                    .child(win.title.clone()),
                                            )
                                            .when(win.is_focused, |this| {
                                                this.child(
                                                    div()
                                                        .px_1p5()
                                                        .bg(pal::pink())
                                                        .text_size(px(9.0))
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(pal::ink())
                                                        .child("active"),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_size(px(11.0))
                                            .text_color(pal::ink())
                                            .opacity(0.65)
                                            .child(format!("app: {}", win.class))
                                            .child("•")
                                            .child(format!("workspace {}", win.workspace_name)),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                comp_ref.read(cx).focus_window(con_id);
                                cx.hide_window("tasks");
                            });

                        if is_selected {
                            row.with_animation(
                                item_key,
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
                    .child(key_chip("↑↓", "select"))
                    .child(key_chip("Enter", "switch"))
                    .child(key_chip("Esc", "close")),
            );

        div()
            .id("tasks-backdrop")
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
                cx.hide_window("tasks");
            })
            .child(printed(card, "tasks", pal::blue()))
    }
}
