use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, Entity, px, Rgba, Window};
use widgetkit::prelude::*;

use crate::theme::{hard, misprint, out_back, pal, printed, staggered, tile, POPUP_TOP};

pub fn toasts_window() -> LayerWindow {
    LayerWindow::new("notifications")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(380.0)
        .height(600.0)
        .margins(Margins::new(POPUP_TOP, 14, 0, 0))
        .namespace("wk-notifications")
        .start_hidden(true)
}

pub fn center_window() -> LayerWindow {
    LayerWindow::new("notification_center")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(400.0)
        .height(540.0)
        .margins(Margins::new(POPUP_TOP, 14, 0, 0))
        .namespace("wk-notification-center")
        .start_hidden(true)
}

pub struct NotificationToasts {
    notifications: Entity<Notifications>,
    toasts_open: bool,
}

impl NotificationToasts {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let notifications = cx.service::<Notifications>();
        // The service is pure data: this view owns the toast window and only
        // opens it while at least one toast is on screen.
        cx.observe(&notifications, |this, notifs, cx| {
            let any = !notifs.read(cx).toasts().is_empty();
            if any != this.toasts_open {
                this.toasts_open = any;
                if any {
                    cx.show_window("notifications");
                } else {
                    cx.hide_window("notifications");
                }
            }
        })
        .detach();
        Self { notifications, toasts_open: false }
    }
}

/// Plate color for a notification: pink is normal, blue is urgent, gray is quiet.
fn urgency_plate(u: &Urgency) -> Rgba {
    match u {
        Urgency::Critical => pal::blue(),
        Urgency::Low => pal::gray(),
        Urgency::Normal => pal::pink(),
    }
}

impl Render for NotificationToasts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toasts = self.notifications.read(cx).toasts().to_vec();

        div().flex().flex_col().gap_4().w_full().pr(px(10.0)).children(toasts.into_iter().map(
            |notif| {
                let notif_id = notif.id;
                let notifs_entity = self.notifications.clone();
                let notifs_entity_close = self.notifications.clone();
                let plate = urgency_plate(&notif.urgency);
                let app_icon = notif.app_icon.clone();
                let summary = notif.summary.clone();
                let body = notif.body.clone();
                let actions = notif.actions.clone();
                let app_name = if notif.app_name.is_empty() {
                    "Notification".to_string()
                } else {
                    notif.app_name.clone()
                };

                let default_action = actions
                    .iter()
                    .find(|a| a.key == "default")
                    .map(|a| a.key.clone())
                    .or_else(|| actions.first().map(|a| a.key.clone()));
                let notifs_card = notifs_entity.clone();

                let card = div()
                    .id(format!("toast-{notif_id}"))
                    .flex()
                    .flex_col()
                    .w_full()
                    .gap_1p5()
                    .p_3()
                    .bg(pal::white())
                    .border_2()
                    .border_color(pal::ink())
                    .cursor_pointer()
                    .hover(|h| h.bg(pal::tint()))
                    .on_click(move |_, _, cx| {
                        let act = default_action.clone().unwrap_or_else(|| "default".to_string());
                        notifs_card.update(cx, |n, _| {
                            n.invoke_action(notif_id, &act);
                            n.dismiss_toast(notif_id);
                        });
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .when(app_icon.is_some(), |this| {
                                        this.child(
                                            gpui::img(app_icon.clone().unwrap())
                                                .w(px(18.0))
                                                .h(px(18.0)),
                                        )
                                    })
                                    .child(
                                        div()
                                            .px_2()
                                            .bg(pal::yellow())
                                            .border_2()
                                            .border_color(pal::ink())
                                            .text_size(px(11.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .truncate()
                                            .child(app_name),
                                    ),
                            )
                            .child(
                                tile(format!("toast-close-{notif_id}"))
                                    .w(px(24.0))
                                    .h(px(24.0))
                                    .shadow(hard(pal::ink(), 2.0))
                                    .text_size(px(12.0))
                                    .child("✕")
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        notifs_entity_close
                                            .update(cx, |n, _| n.dismiss_toast(notif_id));
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .child(summary),
                    )
                    .when(!body.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(17.0))
                                .text_color(pal::ink())
                                .opacity(0.8)
                                .child(body),
                        )
                    })
                    .when(!actions.is_empty(), |this| {
                        this.child(div().flex().flex_row().gap_2().pt_1().children(
                            actions.into_iter().map(|action| {
                                let notifs_act = notifs_entity.clone();
                                let act_key = action.key.clone();
                                tile(format!("toast-act-{}-{}", notif_id, action.key))
                                    .w_auto()
                                    .h(px(28.0))
                                    .px_3()
                                    .shadow(hard(pal::ink(), 2.0))
                                    .text_size(px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(action.label)
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        notifs_act.update(cx, |n, _| {
                                            n.invoke_action(notif_id, &act_key);
                                            n.dismiss_toast(notif_id);
                                        });
                                    })
                            }),
                        ))
                    });

                printed(card, &format!("toast-{notif_id}"), plate)
            },
        ))
    }
}

pub struct NotificationCenter {
    notifications: Entity<Notifications>,
}

impl NotificationCenter {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { notifications: cx.service::<Notifications>() }
    }
}

impl Render for NotificationCenter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notifs = self.notifications.read(cx);
        let history = notifs.history().to_vec();
        let dnd = notifs.is_dnd();
        let notifs_dnd = self.notifications.clone();
        let notifs_clear = self.notifications.clone();
        let shown = history.len().min(10);
        let is_empty = history.is_empty();
        let count = history.len();

        let card = div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 6.0))
            // Ledger header.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .bg(pal::ink())
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(17.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::paper())
                                    .child("Notifications"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::yellow())
                                    .child(format!("{count}")),
                            ),
                    )
                    .child(
                        tile("nc-close")
                            .w(px(26.0))
                            .h(px(26.0))
                            .shadow(hard(pal::pink(), 2.0))
                            .text_size(px(12.0))
                            .child("✕")
                            .on_click(|_, _, cx| cx.hide_window("notification_center")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .child(
                        tile("nc-toggle-dnd")
                            .w_auto()
                            .h(px(28.0))
                            .px_3()
                            .shadow(hard(pal::ink(), 2.0))
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .when(dnd, |s| s.bg(pal::pink()))
                            .child(if dnd { "\u{f1f6}  Quiet is on" } else { "\u{f0f3}  Quiet is off" })
                            .on_click(move |_, _, cx| {
                                notifs_dnd.update(cx, |n, _| n.toggle_dnd());
                            }),
                    )
                    .when(!is_empty, |this| {
                        this.child(
                            tile("nc-clear-all")
                                .w_auto()
                                .h(px(28.0))
                                .px_3()
                                .shadow(hard(pal::ink(), 2.0))
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child("Clear all")
                                .on_click(move |_, _, cx| {
                                    notifs_clear.update(cx, |n, _| n.clear_history());
                                }),
                        )
                    }),
            )
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_2()
                    .p_3()
                    .overflow_hidden()
                    .when(is_empty, |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .h_full()
                                .gap_2()
                                .child(misprint("Nothing new".to_string(), 24.0, pal::blue(), pal::pink()))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(pal::ink())
                                        .child("New notifications will land here."),
                                ),
                        )
                    })
                    .children(history.into_iter().take(10).enumerate().map(|(i, notif)| {
                        let notif_id = notif.id;
                        let notifs_del = self.notifications.clone();
                        let notifs_open = self.notifications.clone();
                        let notifs_acts = self.notifications.clone();
                        let tab = urgency_plate(&notif.urgency);
                        let app_icon = notif.app_icon.clone();
                        let summary = notif.summary.clone();
                        let body = notif.body.clone();
                        let actions = notif.actions.clone();
                        let default_action = actions
                            .iter()
                            .find(|a| a.key == "default")
                            .map(|a| a.key.clone())
                            .or_else(|| actions.first().map(|a| a.key.clone()));
                        let app_name = if notif.app_name.is_empty() {
                            "System".to_string()
                        } else {
                            notif.app_name.clone()
                        };

                        div()
                            .id(format!("hist-{notif_id}"))
                            .flex()
                            .flex_row()
                            .bg(pal::white())
                            .border_2()
                            .border_color(pal::ink())
                            .cursor_pointer()
                            .hover(|h| h.bg(pal::tint()))
                            .on_click(move |_, _, cx| {
                                let act = default_action.clone().unwrap_or_else(|| "default".to_string());
                                notifs_open.update(cx, |n, _| {
                                    n.invoke_action(notif_id, &act);
                                });
                            })
                            .child(div().w(px(8.0)).bg(tab))
                            .child(div().w(px(2.0)).bg(pal::ink()))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .gap_1()
                                    .p_2p5()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .justify_between()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .when(app_icon.is_some(), |this| {
                                                        this.child(
                                                            gpui::img(app_icon.clone().unwrap())
                                                                .w(px(16.0))
                                                                .h(px(16.0)),
                                                        )
                                                    })
                                                    .child(
                                                        div()
                                                            .text_size(px(11.0))
                                                            .font_weight(gpui::FontWeight::BOLD)
                                                            .text_color(pal::blue())
                                                            .truncate()
                                                            .child(app_name),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .id(format!("hist-del-{notif_id}"))
                                                    .cursor_pointer()
                                                    .px_1p5()
                                                    .text_color(pal::ink())
                                                    .hover(|h| h.bg(pal::pink()))
                                                    .child("✕")
                                                    .on_click(move |_, _, cx| {
                                                        cx.stop_propagation();
                                                        notifs_del.update(cx, |n, _| {
                                                            n.remove_history(notif_id);
                                                        });
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .child(summary),
                                    )
                                    .when(!body.is_empty(), |this| {
                                        this.child(
                                            div()
                                                .text_size(px(12.0))
                                                .line_height(px(16.0))
                                                .text_color(pal::ink())
                                                .opacity(0.75)
                                                .child(body),
                                        )
                                    })
                                    .when(!actions.is_empty(), |this| {
                                        this.child(div().flex().flex_row().gap_2().pt_1().children(
                                            actions.into_iter().map(move |action| {
                                                let notifs_act = notifs_acts.clone();
                                                let act_key = action.key.clone();
                                                tile(format!("hist-act-{}-{}", notif_id, action.key))
                                                    .w_auto()
                                                    .h(px(24.0))
                                                    .px_2()
                                                    .shadow(hard(pal::ink(), 1.5))
                                                    .text_size(px(11.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .child(action.label)
                                                    .on_click(move |_, _, cx| {
                                                        cx.stop_propagation();
                                                        notifs_act.update(cx, |n, _| {
                                                            n.invoke_action(notif_id, &act_key);
                                                        });
                                                    })
                                            }),
                                        ))
                                    }),
                            )
                            // Entries slam in from the right, one after another.
                            .with_animation(
                                format!("hist-anim-{notif_id}"),
                                Animation::new(Duration::from_millis(620)),
                                move |el, d| {
                                    let t = staggered(d, i, shown, 0.08);
                                    el.relative().left(px(48.0 * (1.0 - out_back(t)))).opacity((t * 3.0).min(1.0))
                                },
                            )
                    })),
            );

        div().w_full().h_full().pr(px(10.0)).pb(px(10.0)).child(card)
    }
}
