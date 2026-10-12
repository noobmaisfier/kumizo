use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, Entity, FocusHandle, KeyDownEvent, px, Window};
use kumizo_kit::prelude::*;

use crate::theme::{find_field, fullscreen, hard, key_chip, out_cubic, pal, printed, tile};

pub fn window() -> LayerWindow {
    fullscreen("clipboard", "wk-clipboard")
}

pub struct ClipboardManager {
    clipboard: Entity<Clipboard>,
    query: String,
    selected_index: usize,
    focus: FocusHandle,
}

impl ClipboardManager {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clipboard: cx.service::<Clipboard>(),
            query: String::new(),
            selected_index: 0,
            focus: cx.focus_handle(),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let matches = self.clipboard.read(cx).search(&self.query);
        let count = matches.len();

        match key {
            "escape" => {
                self.query.clear();
                self.selected_index = 0;
                cx.hide_window("clipboard");
            }
            "enter" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let item_id = matches[idx].id;
                    self.clipboard.update(cx, |c, _| c.copy_item(item_id));
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("clipboard");
                }
            }
            "delete" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let item_id = matches[idx].id;
                    self.clipboard.update(cx, |c, _| c.remove_item(item_id));
                    if self.selected_index >= count.saturating_sub(1) {
                        self.selected_index = self.selected_index.saturating_sub(1);
                    }
                    cx.notify();
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

impl Render for ClipboardManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let matches = self.clipboard.read(cx).search(&self.query);
        let total = matches.len();
        let selected = self.selected_index.min(total.saturating_sub(1));
        let clipboard_service = self.clipboard.clone();
        let clipboard_clear = self.clipboard.clone();

        let max_visible = 6;
        let start_idx = if selected >= max_visible { selected - max_visible + 1 } else { 0 };
        let visible_items: Vec<_> =
            matches.iter().enumerate().skip(start_idx).take(max_visible).collect();

        let card = div()
            .id("clipboard-modal")
            .w(px(640.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                find_field("Paste", pal::pink(), &self.query, "Search what you copied")
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::blue())
                            .child(format!("{total}")),
                    )
                    .when(total > 0, |this| {
                        this.child(
                            tile("clip-clear-btn")
                                .w_auto()
                                .h(px(28.0))
                                .px_3()
                                .shadow(hard(pal::ink(), 2.0))
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child("Clear all")
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    clipboard_clear.update(cx, |c, _| c.clear());
                                }),
                        )
                    }),
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
                            .child("Nothing copied yet. Copy a line and it appears here.")
                    })
                    .children(visible_items.into_iter().map(|(idx, item)| {
                        let is_selected = idx == selected;
                        let item_id = item.id;
                        let clip_ref = clipboard_service.clone();
                        let clip_del = clipboard_service.clone();
                        let preview = item.preview();
                        let lines = item.line_count();
                        let chars = item.char_count();
                        let key = format!("clip-sel-{item_id}");

                        let row = div()
                            .id(format!("clip-item-{item_id}"))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .border_2()
                            .cursor_pointer()
                            .when(is_selected, |s| {
                                s.bg(pal::pink())
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                            })
                            .when(!is_selected, |s| {
                                s.border_color(pal::clear()).hover(|h| h.bg(pal::tint()))
                            })
                            // Ticket stub with the entry number.
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .w(px(30.0))
                                    .h(px(30.0))
                                    .bg(pal::white())
                                    .border_2()
                                    .border_color(pal::ink())
                                    .text_size(px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(format!("{}", idx + 1)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .w_full()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .truncate()
                                            .child(preview),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(pal::ink())
                                            .opacity(0.65)
                                            .child(format!(
                                                "{lines} line{}, {chars} characters",
                                                if lines > 1 { "s" } else { "" }
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id(format!("clip-del-{item_id}"))
                                    .flex_shrink_0()
                                    .px_1p5()
                                    .text_color(pal::ink())
                                    .hover(|h| h.bg(pal::yellow()))
                                    .child("✕")
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        clip_del.update(cx, |c, _| c.remove_item(item_id));
                                    }),
                            )
                            .on_click(move |_, _, cx| {
                                clip_ref.update(cx, |c, _| c.copy_item(item_id));
                                cx.hide_window("clipboard");
                            });

                        if is_selected {
                            row.with_animation(
                                key,
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
                    .child(key_chip("Enter", "copy"))
                    .child(key_chip("Del", "remove"))
                    .child(key_chip("Esc", "close")),
            );

        div()
            .id("clipboard-backdrop")
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
                cx.hide_window("clipboard");
            })
            .child(printed(card, "clipboard", pal::blue()))
    }
}
