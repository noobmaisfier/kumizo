//! Theme picker (Super+T).
//!
//! Only shows themes that `change-theme` knows how to apply to **everything** in
//! the system (sway, foot, yazi, vim, gtk, herdr, aurora). Selecting launches the
//! script, which switches aurora back via IPC — the window closes immediately.

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, FocusHandle, FontWeight, KeyDownEvent, ScrollHandle, Window,
};
use kumizo_kit::prelude::*;

use crate::{
    icons,
    theme::{self, pal},
    ui::{self, ids},
};

pub struct ThemePicker {
    selected: usize,
    focus: FocusHandle,
    /// Offset kept between two renders; `scroll_to_item` follows the keyboard.
    scroll: ScrollHandle,
}

impl ThemePicker {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            selected: 0,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        }
    }

    fn themes() -> Vec<(usize, &'static str)> {
        theme::system_themes().collect()
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.hide_window(ids::THEMES);
        cx.notify();
    }

    fn apply(&mut self, pos: usize, cx: &mut Context<Self>) {
        if let Some((_, key)) = Self::themes().get(pos) {
            theme::apply_system(key);
        }
        self.close(cx);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let n = Self::themes().len();
        match ui::key(ev) {
            "escape" => self.close(cx),
            "enter" => self.apply(self.selected, cx),
            "down" | "tab" => {
                if n > 0 {
                    self.selected = (self.selected + 1) % n;
                    self.scroll.scroll_to_item(self.selected);
                    cx.notify();
                }
            }
            "up" => {
                if n > 0 {
                    self.selected = (self.selected + n - 1) % n;
                    self.scroll.scroll_to_item(self.selected);
                    cx.notify();
                }
            }
            _ => {}
        }
    }
}

impl Render for ThemePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            // (Re)opening: selection starts from the current theme.
            let themes = Self::themes();
            let current = theme::current_index();
            if let Some(pos) = themes.iter().position(|(i, _)| *i == current) {
                self.selected = pos;
                self.scroll.scroll_to_item(pos);
            }
            window.focus(&self.focus, cx);
        }

        let themes = Self::themes();
        let current = theme::current_index();
        let scroll = self.scroll.clone();

        let mut list = div()
            .id("th-scroll")
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .max_h(px(420.));

        for (pos, (idx, _)) in themes.iter().enumerate() {
            let th = &theme::THEMES[*idx];
            let selected = pos == self.selected;
            let is_current = *idx == current;
            list = list.child(
                div()
                    .id(("theme", pos))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .h(px(46.))
                    .rounded(px(ui::RADIUS_ROW))
                    .cursor_pointer()
                    .bg(if selected {
                        pal::raised()
                    } else {
                        pal::with_alpha(pal::raised(), 0.0)
                    })
                    .border_l_2()
                    .border_color(if selected {
                        pal::accent()
                    } else {
                        pal::with_alpha(pal::accent(), 0.0)
                    })
                    .hover(move |s| {
                        s.bg(if selected {
                            pal::raised()
                        } else {
                            ui::hover_bg()
                        })
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.apply(pos, cx);
                    }))
                    .child(
                        div()
                            .w(px(34.))
                            .h(px(24.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(3.))
                            .rounded(px(ui::RADIUS_SM))
                            .bg(theme::swatch(th.bg))
                            .border_1()
                            .border_color(theme::swatch(th.border))
                            .child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(theme::swatch(th.accent)),
                            )
                            .child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(theme::swatch(th.accent2)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(pal::text())
                            .child(th.name),
                    )
                    // Check mark of the applied theme (the script updates aurora
                    // via IPC, so `current` actually moves).
                    .children(is_current.then(|| ui::glyph(icons::CHECK, 13., pal::accent()))),
            );
        }

        ui::backdrop("th-bg", ids::THEMES, 0.55)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .flex()
            .justify_center()
            .items_start()
            .pt(px(120.))
            .child(
                div()
                    .occlude()
                    .w(px(400.))
                    .flex()
                    .flex_col()
                    .rounded(px(ui::RADIUS_PANEL))
                    .bg(pal::bg_a(0.97))
                    .border_1()
                    .border_color(pal::border())
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_4()
                            .h(px(52.))
                            .border_b_1()
                            .border_color(pal::border())
                            .child(ui::glyph(icons::BRUSH, 14., pal::accent()))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(pal::text())
                                    .child("Theme"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .justify_end()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .child("Enter applies · Esc closes"),
                            ),
                    )
                    .child(list),
            )
    }
}
