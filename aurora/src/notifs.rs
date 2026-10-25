//! Centre de notifications (historique consultable) + carte partagée avec les toasts.

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, KeyDownEvent, Stateful,
    Window,
};
use kumizo_kit::prelude::*;

use crate::{
    icons,
    theme::pal,
    ui::{self, btn, ids},
};

/// Carte de notification. `history = true` : la croix supprime de l'historique ;
/// sinon elle ferme seulement le toast (l'historique est conservé).
pub fn card(
    prefix: &'static str,
    n: &Notification,
    notifs: Entity<Notifications>,
    history: bool,
) -> Stateful<Div> {
    let id = n.id;
    let critical = matches!(n.urgency, Urgency::Critical);
    let border = if critical { pal::danger() } else { pal::border() };

    let n_card = notifs.clone();
    let n_close = notifs.clone();

    let icon = match n.app_icon.clone() {
        Some(img) => gpui::img(img).w(px(20.)).h(px(20.)).into_any_element(),
        None => ui::glyph(icons::BELL, 14., pal::accent()).into_any_element(),
    };

    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(pal::muted())
                .child(n.app_name.clone()),
        );
    if history {
        let time = format!(
            "{:02}:{:02}",
            n.created_at.hour(),
            n.created_at.minute()
        );
        header = header.child(div().text_xs().text_color(pal::muted()).child(time));
    }
    header = header.child(
        btn(("notif-x", id as usize))
            .w(px(22.))
            .h(px(22.))
            .hover(|s| s.bg(ui::hover_bg()))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                n_close.update(cx, |n, _| {
                    if history {
                        n.remove_history(id);
                    } else {
                        n.dismiss_toast(id);
                    }
                });
            })
            .child(ui::glyph(icons::CLOSE, 10., pal::muted())),
    );

    let mut col = div()
        .id((prefix, id as usize))
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded(px(ui::RADIUS_CARD))
        .bg(pal::surface())
        .border_1()
        .border_color(border)
        .cursor_pointer()
        .hover(|s| s.bg(ui::hover_bg()))
        .on_click(move |_, _, cx| {
            n_card.update(cx, |n, _| {
                n.invoke_action(id, "default");
                n.dismiss_toast(id);
            });
        })
        .child(header)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(pal::text())
                .line_clamp(2)
                .child(n.summary.clone()),
        );

    if !n.body.is_empty() {
        col = col.child(
            div()
                .text_sm()
                .text_color(pal::muted())
                .line_clamp(3)
                .child(n.body.clone()),
        );
    }

    // L'action "default" est exclue : c'est le clic sur la carte.
    let actions: Vec<(String, String)> = n
        .actions
        .iter()
        .filter(|a| a.key != "default")
        .map(|a| (a.key.clone(), a.label.clone()))
        .collect();
    if !actions.is_empty() {
        let mut row = div().flex().flex_row().gap_2().pt_1();
        for (i, (key, label)) in actions.into_iter().enumerate() {
            let n = notifs.clone();
            row = row.child(
                btn(("notif-act", (id as usize) * 16 + i))
                    .bg(pal::raised())
                    .text_xs()
                    .text_color(pal::accent())
                    .hover(|s| s.opacity(0.8))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        n.update(cx, |n, _| n.invoke_action(id, &key));
                    })
                    .child(label),
            );
        }
        col = col.child(row);
    }
    col
}

// ── Panneau ─────────────────────────────────────────────────────────────────
pub struct NotificationCenter {
    notifs: Entity<Notifications>,
    focus: FocusHandle,
}

impl NotificationCenter {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            notifs: cx.service::<Notifications>(),
            focus: cx.focus_handle(),
        }
    }
}

impl Render for NotificationCenter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let n = self.notifs.read(cx);
        let dnd = n.is_dnd();
        let items: Vec<&Notification> = n.history().iter().collect();
        let count = items.len();

        let dnd_btn = btn("nc-dnd")
            .px_3()
            .bg(if dnd { pal::accent() } else { pal::raised() })
            .text_color(if dnd { pal::on_accent() } else { pal::text() })
            .text_xs()
            .hover(|s| s.opacity(0.85))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.notifs.update(cx, |n, _| n.toggle_dnd());
            }))
            .child(ui::glyph(
                if dnd { icons::BELL_OFF } else { icons::BELL },
                11.,
                if dnd { pal::on_accent() } else { pal::text() },
            ))
            .child(if dnd { "Silence activé" } else { "Ne pas déranger" });

        let clear_btn = btn("nc-clear")
            .px_3()
            .bg(pal::raised())
            .text_xs()
            .text_color(pal::muted())
            .hover(|s| s.opacity(0.85))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.notifs.update(cx, |n, _| n.clear_history());
            }))
            .child(ui::glyph(icons::TRASH, 11., pal::muted()))
            .child("Tout effacer");

        let mut list = div().flex().flex_col().gap_2();
        // Responsive: voir control.rs — même calcul, même raison.
        let panel_max_h = window.viewport_size().height - px(82.);
        if items.is_empty() {
            list = list.child(
                div()
                    .py(px(48.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(icons::BELL, 28., pal::raised()))
                    .child(div().text_color(pal::muted()).child("Rien à signaler")),
            );
        } else {
            for item in items {
                list = list.child(card("hist", item, self.notifs.clone(), true));
            }
        }

        ui::backdrop("nc-bg", ids::NOTIFS, 0.0)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, _, cx| {
                if ui::key(ev) == "escape" {
                    cx.hide_window(ids::NOTIFS);
                }
            }))
            .flex()
            .justify_end()
            .items_start()
            .pt(px(58.))
            .pr(px(12.))
            .child(
                div()
                    .occlude()
                    .w(px(400.))
                    .max_h(panel_max_h)
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .rounded(px(ui::RADIUS_PANEL))
                    .bg(pal::bg_a(0.97))
                    .border_1()
                    .border_color(pal::border())
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(pal::text())
                                    .font_weight(FontWeight::BOLD)
                                    .child(format!("Notifications · {count}")),
                            )
                            .child(dnd_btn)
                            .child(clear_btn),
                    )
                    .child(
                        div()
                            .id("nc-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(list),
                    ),
            )
    }
}
