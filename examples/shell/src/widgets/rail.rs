use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    pulsating_between, Animation, AnimationExt, AnyElement, Context, Div, Entity, px, Window,
};
use widgetkit::prelude::*;

use crate::theme::{hard, out_back, out_cubic, pal, tile, RAIL_WIDTH};

pub fn window() -> LayerWindow {
    LayerWindow::new("rail")
        .layer(Layer::Top)
        .anchor(Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM)
        .exclusive_zone(RAIL_WIDTH as i32 + 10)
        .size(Some(RAIL_WIDTH), None)
        .margins(Margins::new(8, 0, 8, 8))
        .namespace("wk-rail")
}

pub struct Rail {
    battery: Entity<Battery>,
    network: Entity<Network>,
    tray: Entity<Tray>,
    audio: Entity<Audio>,
    mpris: Entity<Mpris>,
    compositor: Entity<Compositor>,
    notifications: Entity<Notifications>,
}

impl Rail {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            battery: cx.service::<Battery>(),
            network: cx.service::<Network>(),
            tray: cx.service::<Tray>(),
            audio: cx.service::<Audio>(),
            mpris: cx.service::<Mpris>(),
            compositor: cx.service::<Compositor>(),
            notifications: cx.service::<Notifications>(),
        }
    }
}

/// Three colored bars that jump while media is playing, or stay static when paused.
fn equalizer(playing: bool) -> impl IntoElement {
    let colors = [pal::yellow(), pal::pink(), pal::white()];
    div()
        .flex()
        .flex_row()
        .items_end()
        .gap(px(3.0))
        .h(px(22.0))
        .children(colors.into_iter().enumerate().map(move |(i, c)| {
            let base = div()
                .w(px(6.0))
                .border_1()
                .border_color(pal::ink())
                .bg(c);

            if playing {
                base.with_animation(
                    format!("eq-{i}"),
                    Animation::new(Duration::from_millis(480 + i as u64 * 170))
                        .repeat()
                        .with_easing(pulsating_between(0.0, 1.0)),
                    |el, d| el.h(px(5.0 + 17.0 * d)),
                )
                .into_any_element()
            } else {
                let static_height = match i {
                    0 => 8.0,
                    1 => 16.0,
                    _ => 11.0,
                };
                base.h(px(static_height)).into_any_element()
            }
        }))
}

fn gauge(fraction: f32) -> Div {
    div()
        .w(px(28.0))
        .h(px(5.0))
        .border_1()
        .border_color(pal::ink())
        .bg(pal::white())
        .child(
            div()
                .h_full()
                .w(gpui::DefiniteLength::Fraction(fraction.clamp(0.0, 1.0)))
                .bg(pal::ink()),
        )
}

fn status_row(glyph: AnyElement, label: String) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(1.0))
        .py_1()
        .text_size(px(10.0))
        .font_weight(gpui::FontWeight::BOLD)
        .child(div().text_size(px(15.0)).child(glyph))
        .child(label)
}

impl Render for Rail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let battery = self.battery.read(cx);
        let network = self.network.read(cx);
        let tray = self.tray.read(cx);
        let audio = self.audio.read(cx);
        let mpris = self.mpris.read(cx);
        let comp = self.compositor.read(cx);
        let notifs = self.notifications.read(cx);
        let unread = notifs.unread_count();
        let dnd = notifs.is_dnd();

        let volume = audio.volume();
        let muted = audio.is_muted();
        let connected = network.is_connected();
        let percent = battery.percent();
        let charging = battery.is_charging();
        let low = !charging && percent <= 20;

        let has_media = mpris.title().is_some();
        let mpris_entity = self.mpris.clone();
        let audio_entity = self.audio.clone();

        // --- Rows of the status block ---------------------------------------
        let audio_row = div()
            .id("wk-audio-toggle")
            .cursor_pointer()
            .when(muted, |s| s.bg(pal::pink()))
            .when(!muted, |s| s.hover(|h| h.bg(pal::yellow())))
            .child(
                status_row(
                    div().child(audio.icon()).into_any_element(),
                    if muted { "mute".to_string() } else { format!("{volume}") },
                )
                .child(gauge(if muted { 0.0 } else { volume as f32 / 100.0 })),
            )
            .on_click(move |_, _w, cx| {
                audio_entity.update(cx, |a, _| a.toggle_mute());
            });

        let net_row = div()
            .when(!connected, |s| s.bg(pal::pink()))
            .child(status_row(
                div().child(network.icon()).into_any_element(),
                if connected { "on".to_string() } else { "off".to_string() },
            ));

        let batt_inner = div()
            .when(charging, |s| s.bg(pal::yellow()))
            .when(low, |s| s.bg(pal::pink()))
            .child(
                status_row(
                    div().child(battery.state().icon(percent)).into_any_element(),
                    format!("{percent}"),
                )
                .child(gauge(percent as f32 / 100.0)),
            );
        // A low battery flashes; anything else stays still.
        let batt_row: AnyElement = if low {
            batt_inner
                .with_animation(
                    "batt-low",
                    Animation::new(Duration::from_millis(900))
                        .repeat()
                        .with_easing(pulsating_between(0.4, 1.0)),
                    |el, d| el.opacity(d),
                )
                .into_any_element()
            } else {
                batt_inner.into_any_element()
            };

        let status_block = div()
            .flex()
            .flex_col()
            .w(px(44.0))
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .child(audio_row)
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(net_row)
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(batt_row);

        let tray_items: Vec<_> = tray.items().into_iter().collect();

        let panel = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .w(px(64.0))
            .h_full()
            .py_2()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 5.0))
            // Launcher.
            .child(
                tile("wk-launcher-btn")
                    .bg(pal::yellow())
                    .hover(|s| s.bg(pal::pink()))
                    .child("\u{f00a}")
                    .on_click(|_, _w, cx| cx.toggle_window("launcher")),
            )
            // Task Switcher (open windows).
            .child(
                tile("wk-tasks-btn")
                    .bg(pal::white())
                    .hover(|s| s.bg(pal::tint()))
                    .child("\u{f2d0}")
                    .on_click(|_, _w, cx| cx.toggle_window("tasks")),
            )
            // Kanban Board & Sticky Notes.
            .child(
                tile("wk-kanban-btn")
                    .bg(pal::peach())
                    .hover(|s| s.bg(pal::yellow()))
                    .child("\u{f249}")
                    .on_click(|_, _w, cx| cx.toggle_window("kanban")),
            )
            // Workspaces: the active one is blue with a pink plate, and stretches.
            .child(div().flex().flex_col().items_center().gap_1p5().children(
                comp.workspaces().iter().map(|ws| {
                    let is_active = ws.is_active;
                    let ws_id = ws.id;
                    let name = ws.name.clone();
                    let compositor = self.compositor.clone();

                    div()
                        .id(format!("workspace-{ws_id}"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(40.0))
                        .h(px(30.0))
                        .border_2()
                        .border_color(pal::ink())
                        .cursor_pointer()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .when(is_active, |s| {
                            s.bg(pal::blue())
                                .text_color(pal::paper())
                                .shadow(hard(pal::pink(), 3.0))
                        })
                        .when(!is_active, |s| {
                            s.bg(pal::paper())
                                .text_color(pal::ink())
                                .hover(|h| h.bg(pal::yellow()))
                        })
                        .child(name)
                        .on_click(move |_, _w, cx| {
                            compositor.update(cx, |c, _| c.switch_workspace(ws_id));
                        })
                        .with_animation(
                            format!("ws-anim-{ws_id}-{is_active}"),
                            Animation::new(Duration::from_millis(380)),
                            move |el, d| {
                                if is_active {
                                    el.h(px(30.0 + 12.0 * out_back(d)))
                                } else {
                                    el
                                }
                            },
                        )
                }),
            ))
            .child(div().w(px(32.0)).h(px(2.0)).bg(pal::ink()))
            // Flexible spacer pushing status, media and power to the bottom.
            .child(div().flex_1())
            // Media.
            .when(has_media, |s| {
                let playing = mpris.status().is_playing();
                s.child(
                    tile("wk-mpris-toggle")
                        .bg(pal::blue())
                        .hover(|h| h.bg(pal::pink()))
                        .child(equalizer(playing))
                        .on_click(move |_, _w, cx| {
                            mpris_entity.read(cx).play_pause();
                        }),
                )
            })
            // Tray: left-click activates, right-click opens the menu.
            .children(tray_items.into_iter().map(|item| {
                let activate_id = item.id.clone();
                let menu_id = item.id.clone();
                let tray_entity = self.tray.clone();
                let tray_entity2 = self.tray.clone();
                let icon = item.icon.clone();
                let title = item.title.clone();

                div()
                    .id(format!("tray-{}", item.id))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(32.0))
                    .h(px(32.0))
                    .border_2()
                    .border_color(pal::ink())
                    .bg(pal::white())
                    .cursor_pointer()
                    .hover(|h| h.bg(pal::yellow()))
                    .when(icon.is_some(), |this| {
                        this.child(gpui::img(icon.clone().unwrap()).w(px(18.0)).h(px(18.0)))
                    })
                    .when(icon.is_none(), |this| {
                        this.child(div().text_color(pal::blue()).child(
                            title
                                .chars()
                                .next()
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| "?".to_string()),
                        ))
                    })
                    .on_click(move |_, _, cx| {
                        tray_entity.update(cx, |t, _| t.activate(&activate_id));
                    })
                    .on_mouse_down(gpui::MouseButton::Right, move |_, _, cx| {
                        tray_entity2.update(cx, |t, _| t.open_menu(&menu_id));
                    })
            }))
            .child(status_block)
            // Notifications.
            .child(
                tile("wk-notif-bell")
                    .when(!dnd, |s| s.hover(|h| h.bg(pal::yellow())))
                    .when(dnd, |s| s.bg(pal::ink()).text_color(pal::paper()).hover(|h| h.bg(pal::pink()).text_color(pal::ink())))
                    .child(if dnd { "\u{f1f6}" } else { "\u{f0f3}" })
                    .when(unread > 0, |s| {
                        s.child(
                            div()
                                .absolute()
                                .right(px(-8.0))
                                .px_1()
                                .min_w(px(18.0))
                                .flex()
                                .justify_center()
                                .bg(pal::pink())
                                .border_2()
                                .border_color(pal::ink())
                                .text_size(px(10.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(format!("{unread}"))
                                // The badge bobs: something is waiting.
                                .with_animation(
                                    "bell-badge",
                                    Animation::new(Duration::from_millis(900))
                                        .repeat()
                                        .with_easing(pulsating_between(0.0, 1.0)),
                                    |el, d| el.top(px(-9.0 - 4.0 * d)),
                                ),
                        )
                    })
                    .on_click(|_, _w, cx| cx.toggle_window("notification_center")),
            )
            // Power.
            .child(
                tile("wk-power-btn")
                    .bg(pal::ink())
                    .text_color(pal::pink())
                    .hover(|s| s.bg(pal::pink()).text_color(pal::ink()))
                    .child("\u{f011}")
                    .on_click(|_, _w, cx| cx.toggle_window("powermenu")),
            );

        // Room on the right and bottom for the hard shadow.
        div().w_full().h_full().pb(px(6.0)).child(
            panel.with_animation(
                "rail-intro",
                Animation::new(Duration::from_millis(650)),
                |el, d| {
                    let t = out_cubic(d);
                    el.relative().left(px(-(RAIL_WIDTH + 12.0) * (1.0 - t)))
                },
            ),
        )
    }
}
