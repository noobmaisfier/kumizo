//! Top bar: three floating islands (left / center / right).
//!
//! It is also the always-alive "conductor": it watches audio,
//! brightness and notifications to trigger the OSD and toasts.

use gpui::prelude::*;
use gpui::{div, px, App, Context, Div, Entity, FontWeight, Window};
use kumizo_kit::prelude::*;

use crate::{
    icons,
    osd::{self, OsdKind, OsdState},
    pomodoro::{self, Pomodoro},
    theme::{self, pal},
    ui::{self, btn, ids, pill},
};

/// Nerd Font icons for workspaces 1-10 (taken from the user's waybar):
/// the bar shows glyphs, not numbers. Outside 1-10, the workspace name is
/// shown as text (fallback).
const WS_ICONS: [&str; 10] = [
    "\u{f018d}", // 1
    "\u{f0239}", // 2
    "\u{f024b}", // 3
    "\u{f0a1e}", // 4
    "\u{f0219}", // 5
    "\u{f066f}", // 6
    "\u{f0386}", // 7
    "\u{f048b}", // 8
    "\u{f059f}", // 9
    "\u{f075a}", // 10
];

pub struct TopBar {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    memory: Entity<Memory>,
    audio: Entity<Audio>,
    brightness: Entity<Brightness>,
    network: Entity<Network>,
    bluetooth: Entity<Bluetooth>,
    compositor: Entity<Compositor>,
    mpris: Entity<Mpris>,
    tray: Entity<Tray>,
    notifs: Entity<Notifications>,
    pomodoro: Entity<Pomodoro>,
    osd: Entity<OsdState>,
    last_vol: (u8, bool),
    last_bri: u8,
    toasts_open: bool,
}

impl TopBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let brightness = cx.service::<Brightness>();
        let memory = cx.service::<Memory>();
        let notifs = cx.service::<Notifications>();
        let pomodoro = pomodoro::handle(cx);
        let osd = osd::handle(cx);
        let mpris = cx.service::<Mpris>();

        cx.observe(&pomodoro, |_, _, cx| cx.notify()).detach();
        // The media pill wakes up when the player changes title/state
        // (MPRIS events via `playerctl --follow`, zero CPU at rest).
        cx.observe(&mpris, |_, _, cx| cx.notify()).detach();

        let last_vol = {
            let a = audio.read(cx);
            (a.volume(), a.is_muted())
        };
        let last_bri = brightness.read(cx).percent();

        // Volume -> OSD
        cx.observe(&audio, |this, audio, cx| {
            let now = {
                let a = audio.read(cx);
                (a.volume(), a.is_muted())
            };
            if now != this.last_vol {
                this.last_vol = now;
                let osd = this.osd.clone();
                osd.update(cx, |o, cx| o.show(OsdKind::Volume, now.0, now.1, cx));
            }
        })
        .detach();

        // Brightness -> OSD
        cx.observe(&brightness, |this, b, cx| {
            let v = b.read(cx).percent();
            if v != this.last_bri {
                this.last_bri = v;
                let osd = this.osd.clone();
                osd.update(cx, |o, cx| o.show(OsdKind::Brightness, v, false, cx));
            }
        })
        .detach();

        // Notifications -> toasts (window is opened only if there are any)
        cx.observe(&notifs, |this, n, cx| {
            let any = !n.read(cx).toasts().is_empty();
            if any != this.toasts_open {
                this.toasts_open = any;
                if any {
                    cx.show_window(ids::TOASTS);
                } else {
                    cx.hide_window(ids::TOASTS);
                }
            }
        })
        .detach();

        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            memory,
            audio,
            brightness,
            network: cx.service::<Network>(),
            bluetooth: cx.service::<Bluetooth>(),
            compositor: cx.service::<Compositor>(),
            mpris,
            tray: cx.service::<Tray>(),
            notifs,
            pomodoro,
            osd,
            last_vol,
            last_bri,
            toasts_open: false,
        }
    }

    // ── Left : launcher · workspaces ──────────────────────────────────────
    fn left(&self, cx: &App) -> Div {
        let comp = self.compositor.read(cx);
        let workspaces: Vec<(i32, String, bool, bool, usize, bool)> = comp
            .workspaces()
            .iter()
            .map(|w| {
                (
                    w.id,
                    w.name.clone(),
                    w.is_active,
                    w.is_urgent,
                    w.window_count,
                    w.is_special(),
                )
            })
            .collect();
        let mut ws_row = div().flex().flex_row().items_center().gap_1();
        for (id, name, active, urgent, count, special) in workspaces {
            let compositor = self.compositor.clone();
            let toggle_name = name.clone();
            // No full background: the active one stands out by its icon color
            // (accent), urgency by red, the rest by text.
            let fg = if active {
                pal::accent()
            } else if urgent {
                pal::danger()
            } else if count > 0 {
                pal::text()
            } else {
                pal::muted()
            };
            ws_row = ws_row.child(
                btn(("ws", id as usize))
                    .min_w(px(26.))
                    .bg(pal::with_alpha(pal::raised(), 0.0))
                    .text_color(fg)
                    .text_xs()
                    .font_weight(FontWeight::NORMAL)
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(move |_, _, cx| {
                        compositor.update(cx, |c, _| {
                            // A special (scratchpad) workspace is shown *on top*
                            // of the regular one: clicking it toggles visibility,
                            // while a normal workspace is simply focused.
                            if special {
                                let _ = c.toggle_special_workspace(&toggle_name);
                            } else {
                                c.switch_workspace(id);
                            }
                        });
                    })
                    .child(if (1..=10).contains(&id) {
                        ui::glyph(WS_ICONS[(id - 1) as usize], 13., fg)
                    } else {
                        div().text_color(fg).child(name)
                    }),
            );
        }

        // Pomodoro: **separate** pill, to the right of the workspaces island.
        // The ring replaces the glyph: phase progress, cycle color,
        // remaining time on the right.
        let pomodoro_pill = {
            let p = self.pomodoro.read(cx);
            let color = p.phase.color();
            let frac = p.progress();
            let fg = if p.running { color } else { pal::muted() };
            let ring_color = if p.running {
                color
            } else {
                pal::with_alpha(color, 0.5)
            };
            let time = p.mmss();
            let pomodoro = self.pomodoro.clone();
            pill()
                .id("bar-pomodoro")
                .cursor_pointer()
                .hover(|s| s.bg(pal::raised()))
                .on_click(move |_, _, cx| {
                    pomodoro.update(cx, |p, cx| p.toggle(cx));
                })
                .child(ui::ring(
                    frac,
                    18.,
                    2.5,
                    ring_color,
                    pal::with_alpha(color, 0.18),
                ))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_features(ui::tabular())
                        .text_color(fg)
                        .child(time),
                )
        };

        let main = pill().child(ws_row);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(main)
            .child(pomodoro_pill)
    }

    // ── Center : clock · media ───────────────────────────────────────────
    /// Media pill: **MPRIS only** (browser, external player...).
    /// The shell no longer talks to an integrated player: a single indicator,
    /// a single set of buttons, those of the player MPRIS designates.
    fn media_pill(&self, cx: &App) -> Div {
        let m = self.mpris.read(cx);
        let label = m.title().map(|_| m.display_label());
        let playing = matches!(m.status(), PlaybackStatus::Playing);

        let glyph_color = if playing { pal::accent() } else { pal::muted() };
        let play_glyph = || {
            ui::glyph(
                if playing { icons::PAUSE } else { icons::PLAY },
                11.,
                pal::accent(),
            )
        };
        let mut row = pill().child(ui::glyph(icons::MUSIC, 12., glyph_color));

        if let Some(text) = label {
            row = row.child(
                div()
                    .min_w_0()
                    .max_w(px(160.))
                    .truncate()
                    .text_sm()
                    .text_color(pal::text())
                    .child(text),
            );
            let (pr, pp, nx) = (self.mpris.clone(), self.mpris.clone(), self.mpris.clone());
            row = row
                .child(
                    btn("mp-prev")
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            pr.update(cx, |m, _| m.previous());
                        })
                        .child(ui::glyph(icons::PREV, 11., pal::muted())),
                )
                .child(
                    btn("mp-play")
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            pp.update(cx, |m, _| m.play_pause());
                        })
                        .child(play_glyph()),
                )
                .child(
                    btn("mp-next")
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            nx.update(cx, |m, _| m.next());
                        })
                        .child(ui::glyph(icons::NEXT, 11., pal::muted())),
                );
        }
        div().flex().items_center().child(row)
    }

    fn center(&self, cx: &App) -> Div {
        // Time only: the date clutters the bar (the clock pill always opens
        // the notifications center).
        let clock = self.clock.read(cx);
        let time = clock.time_hm().to_string();

        let clock_pill = pill()
            .id("bar-clock")
            .cursor_pointer()
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, cx| ui::toggle_exclusive(ids::NOTIFS, cx))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .font_features(ui::tabular())
                    .text_color(pal::text())
                    .child(time),
            );

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(clock_pill)
            .child(self.media_pill(cx))
    }

    // ── Right : tray · system · notifications ────────────────────────────
    fn right(&self, cx: &App) -> Div {
        let mut row = div().flex().flex_row().items_center().gap_2();

        // Tray. Some apps (Free Download Manager) provide a monochrome white
        // logo, invisible on a light theme. We then put a `muted` pill behind
        // the icon; on a dark theme the white logo already stands out, the pill
        // would be redundant.
        let tray_svc = self.tray.read(cx);
        let entries = tray_svc.items();
        if !entries.is_empty() {
            let chip = theme::is_light();
            let mut tray_row = pill().gap_1();
            for (i, e) in entries.iter().enumerate() {
                let tray = self.tray.clone();
                let id = e.id.clone();
                let icon = match e.icon.clone() {
                    Some(img) => gpui::img(img).w(px(16.)).h(px(16.)).into_any_element(),
                    None => {
                        // Without a pill, the fallback glyph follows `muted`;
                        // with one, it must stand out on the `muted` background.
                        let color = if chip { pal::surface() } else { pal::muted() };
                        ui::glyph(icons::WINDOWS, 13., color).into_any_element()
                    }
                };
                let icon = if chip {
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(20.))
                        .rounded(px(ui::RADIUS_SM))
                        .bg(pal::muted())
                        .child(icon)
                        .into_any_element()
                } else {
                    icon
                };
                tray_row = tray_row.child(
                    btn(("tray", i))
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            tray.update(cx, |t, _| t.activate(&id));
                        })
                        .child(icon),
                );
            }
            row = row.child(tray_row);
        }

        // System cluster
        let net = self.network.read(cx);
        let (wifi_on, radio, wifi_avail) =
            (net.is_connected(), net.wifi_enabled(), net.has_radio());
        let bt = self.bluetooth.read(cx);
        let (bt_avail, bt_on, bt_count) = (bt.is_available(), bt.is_powered(), bt.connected_count());
        let a = self.audio.read(cx);
        let (vol, muted) = (a.volume(), a.is_muted());
        let bri = self.brightness.read(cx);
        let bri_pct = bri.percent();
        let bri_icon = bri.icon();
        let b = self.battery.read(cx);
        let (pct, charging) = (b.percent(), b.is_charging());
        let mem = self.memory.read(cx);
        let mem_pct = mem.percent();

        let bat_color = if charging {
            pal::ok()
        } else if pct <= 15 {
            pal::danger()
        } else if pct <= 30 {
            pal::warn()
        } else {
            pal::text()
        };
        // RAM is only red under heavy pressure: cache counts as "used", so we
        // don't want to shout too early. The icon stays green as long as
        // everything is fine; the value follows the threshold.
        let ram_color = if mem_pct >= 90 {
            pal::danger()
        } else if mem_pct >= 75 {
            pal::warn()
        } else {
            pal::text()
        };
        let ram_icon = if mem_pct >= 90 {
            pal::danger()
        } else if mem_pct >= 75 {
            pal::warn()
        } else {
            pal::ok()
        };

        let mut sys = div().flex().flex_row().items_center().gap_3();
        // Like Bluetooth: hidden if no radio, accent if active.
        // Wi-Fi has one more state (connected vs on-but-offline).
        if wifi_avail {
            sys = sys.child(ui::glyph(
                icons::WIFI,
                14.,
                if wifi_on {
                    pal::accent()
                } else if radio {
                    pal::text()
                } else {
                    pal::muted()
                },
            ));
        }
        if bt_avail {
            let mut bt_cell = div().flex().items_center().gap_1().child(ui::glyph(
                icons::BLUETOOTH,
                13.,
                if bt_on { pal::accent() } else { pal::muted() },
            ));
            if bt_count > 0 {
                bt_cell = bt_cell.child(
                    div()
                        .text_xs()
                        .text_color(pal::accent())
                        .child(bt_count.to_string()),
                );
            }
            sys = sys.child(bt_cell);
        }
        sys = sys
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(
                        ui::vol_glyph(vol, muted),
                        14.,
                        if muted { pal::muted() } else { pal::accent2() },
                    ))
                    .child(div().text_xs().child(if muted {
                        "muted".to_string()
                    } else {
                        format!("{vol}%")
                    })),
            )
            // Brightness: right after volume (same cell shape).
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(bri_icon, 14., pal::warn()))
                    .child(div().text_xs().child(format!("{bri_pct}%"))),
            )
            // RAM: right before battery (usage/total in GiB, colored under
            // memory pressure).
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(icons::RAM, 14., ram_icon))
                    .child(
                        div()
                            .text_xs()
                            .text_color(ram_color)
                            .child(format!(
                                "{:.1}/{:.1}G",
                                mem.used_gib(),
                                mem.total_gib()
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(ui::bat_glyph(pct), 14., bat_color))
                    .child(
                        div()
                            .text_xs()
                            .text_color(bat_color)
                            .child(format!("{pct}%")),
                    )
                    .children(charging.then(|| ui::glyph(icons::BOLT, 10., pal::ok()))),
            );

        row = row.child(
            pill()
                .id("bar-system")
                .cursor_pointer()
                .hover(|s| s.bg(pal::raised()))
                .on_click(|_, _, cx| ui::toggle_exclusive(ids::CONTROL, cx))
                .child(sys),
        );

        // Notifications
        let n = self.notifs.read(cx);
        let (unread, dnd) = (n.unread_count(), n.is_dnd());
        let mut bell = pill()
            .id("bar-bell")
            .cursor_pointer()
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, cx| ui::toggle_exclusive(ids::NOTIFS, cx))
            .child(ui::glyph(
                if dnd { icons::BELL_OFF } else { icons::BELL },
                14.,
                if dnd { pal::muted() } else { pal::text() },
            ));
        if unread > 0 {
            bell = bell.child(
                div()
                    .px_2()
                    .rounded_full()
                    .bg(pal::accent())
                    .text_color(pal::on_accent())
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .child(if unread > 99 {
                        "99+".to_string()
                    } else {
                        unread.to_string()
                    }),
            );
        }
        row.child(bell)
    }
}

impl Render for TopBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .items_start()
            .px(px(8.))
            .pt(px(6.))
            .text_sm()
            .text_color(pal::text())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_start()
                    .child(self.left(cx)),
            )
            .child(self.center(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_end()
                    .child(self.right(cx)),
            )
    }
}
