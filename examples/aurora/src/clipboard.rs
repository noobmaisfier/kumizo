//! Presse-papier : historique, recherche et recopie via le service `Clipboard`.
//!
//! Kumizo n'a pas de champ texte natif : la saisie est gérée à la main
//! depuis `KeyDownEvent` (touche + `key_char`), comme le launcher.

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, ScrollHandle,
    Window,
};
use kumizo_kit::prelude::*;

use crate::{
    icons,
    theme::pal,
    ui::{self, ids},
};

pub struct ClipboardPanel {
    clipboard: Entity<Clipboard>,
    query: String,
    selected: usize,
    focus: FocusHandle,
    /// Idem launcher : offset persistant + suivi clavier one-shot.
    scroll: ScrollHandle,
}

impl ClipboardPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clipboard: cx.service::<Clipboard>(),
            query: String::new(),
            selected: 0,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        }
    }

    fn results(&self, cx: &Context<Self>) -> Vec<ClipboardItem> {
        self.clipboard.read(cx).search(&self.query)
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.query.clear();
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.hide_window(ids::CLIPBOARD);
        cx.notify();
    }

    fn activate(&mut self, i: usize, cx: &mut Context<Self>) {
        let id = self.results(cx).get(i).map(|it| it.id);
        if let Some(id) = id {
            self.clipboard.update(cx, |c, _| c.copy_item(id));
        }
        self.close(cx);
    }

    fn remove(&mut self, i: usize, cx: &mut Context<Self>) {
        let id = self.results(cx).get(i).map(|it| it.id);
        if let Some(id) = id {
            self.clipboard.update(cx, |c, _| c.remove_item(id));
            let n = self.results(cx).len();
            self.selected = if n == 0 { 0 } else { self.selected.min(n - 1) };
            self.scroll.scroll_to_item(self.selected);
            cx.notify();
        }
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.results(cx).len();
        if n > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
            self.scroll.scroll_to_item(self.selected);
            cx.notify();
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        match ui::key(ev) {
            "escape" => self.close(cx),
            "enter" => self.activate(self.selected, cx),
            "delete" => self.remove(self.selected, cx),
            "up" => self.step(-1, cx),
            "down" | "tab" => self.step(1, cx),
            "backspace" => {
                self.query.pop();
                self.selected = 0;
                self.scroll.scroll_to_item(0);
                cx.notify();
            }
            "space" => {
                self.query.push(' ');
                self.selected = 0;
                self.scroll.scroll_to_item(0);
                cx.notify();
            }
            _ => {
                let m = &ev.keystroke.modifiers;
                if !m.control && !m.alt && !m.platform {
                    if let Some(ch) = ev.keystroke.key_char.as_ref() {
                        if self.query.chars().count() < 64 {
                            self.query.push_str(ch);
                            self.selected = 0;
                            self.scroll.scroll_to_item(0);
                            cx.notify();
                        }
                    }
                }
            }
        }
    }
}

impl Render for ClipboardPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let results = self.results(cx);
        let total = results.len();
        if self.selected >= total {
            self.selected = 0;
        }
        let scroll = self.scroll.clone();

        // Champ de saisie : texte + faux curseur (▏)
        let clear = self.clipboard.clone();
        let input = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .h(px(56.))
            .border_b_1()
            .border_color(pal::border())
            .child(ui::glyph(icons::CLIPBOARD, 16., pal::accent()))
            .child(if self.query.is_empty() {
                div()
                    .flex_1()
                    .text_color(pal::muted())
                    .child("Rechercher dans le presse-papier…")
            } else {
                div()
                    .flex_1()
                    .text_color(pal::text())
                    .child(format!("{}▏", self.query))
            })
            .child(
                div()
                    .text_xs()
                    .text_color(pal::muted())
                    .child(format!("{total}")),
            )
            .when(total > 0, |this| {
                this.child(
                    ui::btn("clip-clear")
                        .px_3()
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            clear.update(cx, |c, cx| {
                                c.clear();
                                cx.notify();
                            });
                        })
                        .child(div().text_xs().text_color(pal::muted()).child("Vider")),
                )
            });

        // Lignes en enfants directs : `scroll_to_item(i)` tombe juste.
        let mut list = div()
            .id("clip-scroll")
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .max_h(px(400.));
        if results.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_3()
                    .text_color(pal::muted())
                    .child("Rien dans le presse-papier pour l'instant."),
            );
        }
        for (i, item) in results.iter().enumerate() {
            let selected = i == self.selected;
            let preview = item.preview();
            let lines = item.line_count();
            let chars = item.char_count();
            list = list.child(
                div()
                    .id(("clip", i))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .h(px(48.))
                    .rounded_xl()
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
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.activate(i, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .font_weight(if selected {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(pal::text())
                                    .child(preview),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .child(format!(
                                        "{lines} ligne{}, {chars} car.",
                                        if lines > 1 { "s" } else { "" }
                                    )),
                            ),
                    )
                    .child(
                        ui::btn(("clip-del", i))
                            .w(px(28.))
                            .hover(|s| s.bg(pal::danger()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.remove(i, cx);
                            }))
                            .child(ui::glyph(icons::CLOSE, 12., pal::muted())),
                    ),
            );
        }

        ui::backdrop("clip-bg", ids::CLIPBOARD, 0.6)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .flex()
            .justify_center()
            .items_start()
            .pt(px(150.))
            .child(
                div()
                    .occlude()
                    .w(px(640.))
                    .flex()
                    .flex_col()
                    .rounded_2xl()
                    .bg(pal::bg_a(0.97))
                    .border_1()
                    .border_color(pal::border())
                    .shadow_lg()
                    .overflow_hidden()
                    .child(input)
                    .child(list),
            )
    }
}
