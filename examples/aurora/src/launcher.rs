//! Lanceur d'applications : saisie au clavier, recherche floue via `Apps::search`.
//!
//! WidgetKit n'a pas de champ texte natif : la saisie est gérée à la main
//! depuis `KeyDownEvent` (touche + `key_char`).

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, Window,
};
use widgetkit::prelude::*;

use crate::{
    adapt, icons,
    theme::pal,
    ui::{self, ids},
};

pub struct AppLauncher {
    apps: Entity<Apps>,
    query: String,
    selected: usize,
    focus: FocusHandle,
}

impl AppLauncher {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            apps: cx.service::<Apps>(),
            query: String::new(),
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.query.clear();
        self.selected = 0;
        cx.hide_window(ids::LAUNCHER);
        cx.notify();
    }

    fn activate(&mut self, i: usize, cx: &mut Context<Self>) {
        let results = adapt::app_results(self.apps.read(cx), &self.query);
        if let Some(app) = results.get(i) {
            self.apps.update(cx, |a, _| a.launch(app));
        }
        self.close(cx);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = adapt::app_results(self.apps.read(cx), &self.query).len();
        match ui::key(ev) {
            "escape" => self.close(cx),
            "enter" => self.activate(self.selected, cx),
            "backspace" => {
                self.query.pop();
                self.selected = 0;
                cx.notify();
            }
            "down" | "tab" => {
                if count > 0 {
                    self.selected = (self.selected + 1) % count;
                    cx.notify();
                }
            }
            "up" => {
                if count > 0 {
                    self.selected = (self.selected + count - 1) % count;
                    cx.notify();
                }
            }
            _ => {
                let m = &ev.keystroke.modifiers;
                if !m.control && !m.alt && !m.platform {
                    if let Some(ch) = ev.keystroke.key_char.as_ref() {
                        if self.query.chars().count() < 64 {
                            self.query.push_str(ch);
                            self.selected = 0;
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

        let results = adapt::app_results(self.apps.read(cx), &self.query);
        if self.selected >= results.len() {
            self.selected = 0;
        }

        // Champ de saisie : texte + faux curseur (▏)
        let input = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .h(px(56.))
            .border_b_1()
            .border_color(pal::border())
            .child(ui::glyph(icons::SEARCH, 16., pal::accent()))
            .child(if self.query.is_empty() {
                div()
                    .flex_1()
                    .text_color(pal::muted())
                    .child("Rechercher une application…")
            } else {
                div()
                    .flex_1()
                    .text_color(pal::text())
                    .child(format!("{}▏", self.query))
            });

        let mut list = div().flex().flex_col().p_2().gap_1();
        if results.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_3()
                    .text_color(pal::muted())
                    .child("Aucun résultat"),
            );
        }
        for (i, app) in results.iter().enumerate() {
            let selected = i == self.selected;
            let icon = match adapt::app_icon(app) {
                Some(img) => gpui::img(img).w(px(28.)).h(px(28.)).into_any_element(),
                None => ui::glyph(icons::WINDOWS, 18., pal::muted()).into_any_element(),
            };
            list = list.child(
                div()
                    .id(("app", i))
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
                            .w(px(28.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon),
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
                            .child(adapt::app_name(app)),
                    ),
            );
        }

        ui::backdrop("lc-bg", ids::LAUNCHER, 0.6)
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
