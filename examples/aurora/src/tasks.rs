//! Sélecteur de fenêtres ouvertes (Sway / Hyprland via le service Compositor).
//! Clavier : flèches / Tab, Entrée pour focaliser, Échap pour fermer.

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

struct Row {
    id: i64,
    title: String,
    class: String,
    workspace: String,
    focused: bool,
}

pub struct TaskSwitcher {
    compositor: Entity<Compositor>,
    apps: Entity<Apps>,
    selected: usize,
    focus: FocusHandle,
}

impl TaskSwitcher {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            compositor: cx.service::<Compositor>(),
            apps: cx.service::<Apps>(),
            selected: 0,
            focus: cx.focus_handle(),
        }
    }

    fn rows(&self, cx: &Context<Self>) -> Vec<Row> {
        self.compositor
            .read(cx)
            .windows()
            .iter()
            .map(|w| Row {
                id: w.id,
                title: w.title.clone(),
                class: w.class.clone(),
                workspace: w.workspace_name.clone(),
                focused: w.is_focused,
            })
            .collect()
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.selected = 0;
        cx.hide_window(ids::TASKS);
        cx.notify();
    }

    fn activate(&mut self, i: usize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        if let Some(r) = rows.get(i) {
            let id = r.id;
            self.compositor.update(cx, |c, _| c.focus_window(id));
        }
        self.close(cx);
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.rows(cx).len();
        if n > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
            cx.notify();
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        match ui::key(ev) {
            "escape" => self.close(cx),
            "enter" => self.activate(self.selected, cx),
            "left" | "up" => self.step(-1, cx),
            "right" | "down" | "tab" => self.step(1, cx),
            _ => {}
        }
    }
}

impl Render for TaskSwitcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let rows = self.rows(cx);
        if self.selected >= rows.len() {
            self.selected = 0;
        }

        let mut grid = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_center()
            .gap_3()
            .max_w(px(1000.));

        if rows.is_empty() {
            grid = grid.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(icons::WINDOWS, 34., pal::raised()))
                    .child(
                        div()
                            .text_color(pal::muted())
                            .child("Aucune fenêtre ouverte"),
                    ),
            );
        }

        for (i, r) in rows.into_iter().enumerate() {
            let selected = i == self.selected;
            let icon = match adapt::window_icon(self.apps.read(cx), &r.class) {
                Some(img) => gpui::img(img).w(px(36.)).h(px(36.)).into_any_element(),
                None => ui::glyph(icons::WINDOWS, 26., pal::muted()).into_any_element(),
            };
            let title = if r.title.is_empty() {
                r.class.clone()
            } else {
                r.title.clone()
            };
            grid = grid.child(
                div()
                    .id(("task", i))
                    .w(px(300.))
                    .h(px(112.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .p_3()
                    .rounded_2xl()
                    .cursor_pointer()
                    .bg(if selected { pal::raised() } else { pal::surface() })
                    .border_2()
                    .border_color(if selected { pal::accent() } else { pal::border() })
                    .hover(|s| s.opacity(0.9))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.activate(i, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(icon)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(pal::text())
                                    .line_clamp(2)
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(pal::muted())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("{} · espace {}", r.class, r.workspace)),
                            )
                            // L'état « actif » est dit en texte, pas seulement en couleur.
                            .children(r.focused.then(|| {
                                div()
                                    .px_2()
                                    .rounded_full()
                                    .bg(pal::accent())
                                    .text_color(pal::on_accent())
                                    .child("actif")
                            })),
                    ),
            );
        }

        ui::backdrop("tk-bg", ids::TASKS, 0.72)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .occlude()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_6()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(pal::text())
                            .child("Fenêtres"),
                    )
                    .child(grid)
                    .child(
                        div()
                            .text_xs()
                            .text_color(pal::muted())
                            .child("←/→ choisir · Entrée focaliser · Échap fermer"),
                    ),
            )
    }
}
