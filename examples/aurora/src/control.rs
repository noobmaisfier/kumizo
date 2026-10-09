//! Centre de contrôle : tuiles rapides, volume, luminosité, Bluetooth,
//! pomodoro, média et sélecteur de thème (changement à chaud).

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, KeyDownEvent, Stateful,
    Window,
};
use widgetkit::prelude::*;

use crate::{
    adapt, icons,
    pomodoro::{self, Pomodoro},
    theme::{self, pal},
    ui::{self, btn, card, ids},
};

pub struct ControlCenter {
    audio: Entity<Audio>,
    brightness: Entity<Brightness>,
    bluetooth: Entity<Bluetooth>,
    network: Entity<Network>,
    battery: Entity<Battery>,
    mpris: Entity<Mpris>,
    notifs: Entity<Notifications>,
    pomodoro: Entity<Pomodoro>,
    /// SSID en cours de saisie mot de passe (`None` = pas de saisie).
    wifi_pw_for: Option<String>,
    /// Champ mot de passe (masqué, headless `wk-ui`).
    wifi_pw: Entity<TextBuffer>,
    focus: FocusHandle,
}

/// Tuile rapide. Le hover est posé ICI, une seule fois : les appelants n'y touchent pas.
fn tile(id: &'static str, g: &'static str, title: &str, sub: String, on: bool) -> Stateful<Div> {
    let (bg, fg) = if on {
        (pal::accent(), pal::on_accent())
    } else {
        (pal::surface(), pal::text())
    };
    let sub_color = if on { pal::on_accent() } else { pal::muted() };
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(58.))
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .rounded_xl()
        .bg(bg)
        .border_1()
        .border_color(pal::border())
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .child(ui::glyph(g, 18., fg))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(sub_color)
                        .truncate()
                        .child(sub),
                ),
        )
}

fn section_title(text: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(pal::muted())
        .child(text.to_uppercase())
}

impl ControlCenter {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let pomodoro = pomodoro::handle(cx);
        cx.observe(&pomodoro, |_, _, cx| cx.notify()).detach();
        Self {
            audio: cx.service::<Audio>(),
            brightness: cx.service::<Brightness>(),
            bluetooth: cx.service::<Bluetooth>(),
            network: cx.service::<Network>(),
            battery: cx.service::<Battery>(),
            mpris: cx.service::<Mpris>(),
            notifs: cx.service::<Notifications>(),
            pomodoro,
            wifi_pw_for: None,
            wifi_pw: cx.new(|_| {
                let mut b = TextBuffer::masked();
                b.set_max_chars(63); // PSK WPA : 8..63 caractères
                b
            }),
            focus: cx.focus_handle(),
        }
    }

    // ── Tuiles rapides ─────────────────────────────────────────────────────
    fn toggles(&self, cx: &mut Context<Self>) -> Div {
        let net = self.network.read(cx);
        let (wifi_on, ssid) = (net.is_connected(), net.ssid().map(str::to_string));
        let radio = net.wifi_enabled();
        let avail = net.has_radio();
        let signal = net.signal_percent();
        let bt = self.bluetooth.read(cx);
        let (bt_avail, bt_on, bt_count) = (bt.is_available(), bt.is_powered(), bt.connected_count());
        let a = self.audio.read(cx);
        let muted = a.is_muted();
        let dnd = self.notifs.read(cx).is_dnd();

        let wifi = tile(
            "cc-wifi",
            icons::WIFI,
            "Wi-Fi",
            if !avail {
                "Indisponible".into()
            } else if !radio {
                "Désactivé".into()
            } else if wifi_on {
                let sig = signal.map(|s| format!(" · {s}%")).unwrap_or_default();
                format!("{}{sig}", ssid.unwrap_or_else(|| "connecté".into()))
            } else {
                "Hors ligne".into()
            },
            radio,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.network.update(cx, |n, _| {
                let _ = n.set_enabled(!n.wifi_enabled());
            });
        }));

        let bluetooth = tile(
            "cc-bt",
            icons::BLUETOOTH,
            "Bluetooth",
            if !bt_avail {
                "Indisponible".into()
            } else if !bt_on {
                "Désactivé".into()
            } else if bt_count > 0 {
                format!("{bt_count} connecté(s)")
            } else {
                "Activé".into()
            },
            bt_on,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.bluetooth.update(cx, |b, _| b.toggle_power());
        }));

        let mute = tile(
            "cc-mute",
            ui::vol_glyph(a.volume(), muted),
            "Son",
            if muted { "Muet".into() } else { "Actif".into() },
            !muted,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.audio.update(cx, |a, _| a.toggle_mute());
        }));

        let dnd_tile = tile(
            "cc-dnd",
            if dnd { icons::BELL_OFF } else { icons::BELL },
            "Silence",
            if dnd { "Activé".into() } else { "Désactivé".into() },
            dnd,
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.notifs.update(cx, |n, _| n.toggle_dnd());
        }));

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().flex_row().gap_2().child(wifi).child(bluetooth))
            .child(div().flex().flex_row().gap_2().child(mute).child(dnd_tile))
    }

    // ── Sliders ────────────────────────────────────────────────────────────
    fn sliders(&self, cx: &mut Context<Self>) -> Div {
        let a = self.audio.read(cx);
        let (vol, muted) = (a.volume(), a.is_muted());
        let bri = self.brightness.read(cx).percent();

        let (audio_set, audio_mute) = (self.audio.clone(), self.audio.clone());
        let bright_set = self.brightness.clone();

        let vol_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                btn("cc-vol-icon")
                    .w(px(30.))
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(move |_, _, cx| {
                        audio_mute.update(cx, |a, _| a.toggle_mute());
                    })
                    .child(ui::glyph(
                        ui::vol_glyph(vol, muted),
                        16.,
                        if muted { pal::muted() } else { pal::accent() },
                    )),
            )
            .child(div().flex_1().child(ui::segments(
                "cc-vol",
                if muted { 0 } else { vol },
                move |p, cx| {
                    audio_set.update(cx, |a, _| a.set_volume(p));
                },
            )))
            .child(
                div()
                    .w(px(40.))
                    .text_sm()
                    .text_color(pal::muted())
                    .child(format!("{vol}%")),
            );

        let bri_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                div()
                    .w(px(30.))
                    .flex()
                    .justify_center()
                    .child(ui::glyph(icons::SUN, 16., pal::accent2())),
            )
            .child(div().flex_1().child(ui::segments(
                "cc-bri",
                bri,
                move |p, cx| {
                    bright_set.update(cx, |b, _| b.set_percent(p));
                },
            )))
            .child(
                div()
                    .w(px(40.))
                    .text_sm()
                    .text_color(pal::muted())
                    .child(format!("{bri}%")),
            );

        card().child(vol_row).child(bri_row)
    }

    // ── Bluetooth : appareils ──────────────────────────────────────────────
    fn bluetooth_card(&self, cx: &mut Context<Self>) -> Option<Div> {
        let bt = self.bluetooth.read(cx);
        if !bt.is_available() || !bt.is_powered() {
            return None;
        }
        let scanning = bt.is_discovering();
        let devices = adapt::bt_devices(bt);

        let scan = btn("cc-bt-scan")
            .px_3()
            .bg(if scanning { pal::accent() } else { pal::raised() })
            .text_color(if scanning { pal::on_accent() } else { pal::text() })
            .text_xs()
            .hover(|s| s.opacity(0.85))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.bluetooth.update(cx, |b, _| b.toggle_scan());
            }))
            .child(if scanning { "Recherche…" } else { "Rechercher" });

        let mut list = div().flex().flex_col().gap_1();
        if devices.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(pal::muted())
                    .child("Aucun appareil — lance une recherche."),
            );
        }
        for (i, d) in devices.into_iter().enumerate() {
            let connected = d.connected;
            let name = d.name.clone();
            list = list.child(
                div()
                    .id(("cc-btdev", i))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .h(px(34.))
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.bluetooth.update(cx, |b, _| adapt::bt_toggle(b, &d));
                    }))
                    .child(ui::glyph(
                        icons::BLUETOOTH,
                        12.,
                        if connected { pal::accent() } else { pal::muted() },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(pal::text())
                            .child(name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if connected { pal::accent() } else { pal::muted() })
                            .child(if connected { "Connecté" } else { "Appairé" }),
                    ),
            );
        }

        Some(
            card()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(div().flex_1().child(section_title("Appareils Bluetooth")))
                        .child(scan),
                )
                .child(list),
        )
    }

    // ── Wi-Fi : réseaux ──────────────────────────────────────────────────
    fn wifi_submit_pw(&mut self, cx: &mut Context<Self>) {
        if let Some(ssid) = self.wifi_pw_for.clone() {
            let pw = self.wifi_pw.read(cx).value().to_string();
            if !pw.is_empty() {
                self.network.update(cx, |n, _| {
                    n.connect_with_password(&ssid, &pw);
                });
            }
            self.wifi_pw.update(cx, |b, _| b.clear());
            self.wifi_pw_for = None;
            cx.notify();
        }
    }

    fn wifi_cancel_pw(&mut self, cx: &mut Context<Self>) {
        self.wifi_pw.update(cx, |b, _| b.clear());
        self.wifi_pw_for = None;
        cx.notify();
    }

    fn wifi_card(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (enabled, networks) = {
            let net = self.network.read(cx);
            (net.wifi_enabled(), net.networks().to_vec())
        };
        // Comme le Bluetooth : éteint = pas de carte du tout. L'interrupteur
        // est la tuile, pas un bouton dans la carte.
        if !enabled {
            return None;
        }

        let scan = btn("cc-wifi-scan")
            .px_3()
            .bg(pal::raised())
            .text_color(pal::text())
            .text_xs()
            .hover(|s| s.opacity(0.85))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.network.update(cx, |n, _| n.request_scan());
            }))
            .child("Analyser");

        let mut list = div().flex().flex_col().gap_1();
        if networks.is_empty() {
            list = list.child(
                div()
                    .text_sm()
                    .text_color(pal::muted())
                    .child("Aucun réseau — lance une analyse."),
            );
        }
        for (i, n) in networks.iter().enumerate() {
            let (ssid, strength, secured, active) =
                (n.ssid.clone(), n.strength, n.secured, n.active);
            let pw_open = self.wifi_pw_for.as_deref() == Some(ssid.as_str());
            let click_ssid = ssid.clone();
            let mut row = div()
                .id(("cc-wifinet", i))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(34.))
                .rounded_lg()
                .cursor_pointer()
                .hover(|s| s.bg(pal::raised()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if active {
                        this.network.update(cx, |n, _| {
                            n.disconnect();
                        });
                    } else if secured {
                        this.wifi_pw.update(cx, |b, _| b.clear());
                        this.wifi_pw_for = Some(click_ssid.clone());
                        cx.notify();
                    } else {
                        this.network.update(cx, |n, _| {
                            n.connect(&click_ssid);
                        });
                    }
                }))
                .child(ui::glyph(
                    icons::WIFI,
                    12.,
                    if active { pal::accent() } else { pal::muted() },
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(pal::text())
                        .child(ssid.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(pal::muted())
                        .child(format!("{strength}%")),
                );
            if secured {
                row = row.child(ui::glyph(icons::LOCK, 10., pal::muted()));
            }
            if active {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(pal::accent())
                        .child("Connecté"),
                );
            }
            list = list.child(row);

            if pw_open {
                let style = TextStyle {
                    text: pal::text(),
                    placeholder: pal::muted(),
                    caret: pal::accent(),
                    size_px: 14.,
                };
                let pw_row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .h(px(36.))
                    .rounded_lg()
                    .bg(pal::raised())
                    .child(div().flex_1().min_w_0().child(text_input(
                        "cc-wifi-pw",
                        &self.wifi_pw,
                        style,
                        "Mot de passe…",
                        cx,
                    )))
                    .child(
                        btn("cc-wifi-ok")
                            .px_3()
                            .h(px(28.))
                            .bg(pal::accent())
                            .text_color(pal::on_accent())
                            .text_xs()
                            .hover(|s| s.opacity(0.85))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.wifi_submit_pw(cx);
                            }))
                            .child("OK"),
                    );
                list = list.child(pw_row);
            }
        }

        Some(
            card()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().child(section_title("Réseaux Wi-Fi")))
                        .child(scan),
                )
                .child(list),
        )
    }

    // ── Pomodoro ───────────────────────────────────────────────────────────
    fn pomodoro_card(&self, cx: &mut Context<Self>) -> Div {
        let p = self.pomodoro.read(cx);
        let (phase, running, time, frac, done, sessions) = (
            p.phase,
            p.running,
            p.mmss(),
            p.progress(),
            p.cycle_done(),
            p.sessions,
        );
        let color = phase.color();

        let (pm_toggle, pm_reset, pm_skip) = (
            self.pomodoro.clone(),
            self.pomodoro.clone(),
            self.pomodoro.clone(),
        );

        let mut dots = div().flex().flex_row().gap(px(5.)).items_center();
        for i in 0..4u32 {
            dots = dots.child(
                div()
                    .size(px(8.))
                    .rounded_full()
                    .bg(if i < done { color } else { pal::raised() }),
            );
        }

        let toggle = btn("cc-pomo-toggle")
            .flex_1()
            .h(px(36.))
            .bg(color)
            .text_color(pal::on_accent())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.opacity(0.85))
            .on_click(move |_, _, cx| {
                pm_toggle.update(cx, |p, cx| p.toggle(cx));
            })
            .child(ui::glyph(
                if running { icons::PAUSE } else { icons::PLAY },
                12.,
                pal::on_accent(),
            ))
            .child(if running { "Pause" } else { "Démarrer" });

        let reset = btn("cc-pomo-reset")
            .h(px(36.))
            .px_3()
            .bg(pal::raised())
            .text_color(pal::text())
            .hover(|s| s.opacity(0.85))
            .on_click(move |_, _, cx| {
                pm_reset.update(cx, |p, cx| p.reset(cx));
            })
            .child(ui::glyph(icons::REBOOT, 12., pal::muted()))
            .child("Reset");

        let skip = btn("cc-pomo-skip")
            .h(px(36.))
            .px_3()
            .bg(pal::raised())
            .text_color(pal::text())
            .hover(|s| s.opacity(0.85))
            .on_click(move |_, _, cx| {
                pm_skip.update(cx, |p, cx| p.skip(cx));
            })
            .child(ui::glyph(icons::NEXT, 12., pal::muted()))
            .child("Passer");

        card()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(ui::glyph(icons::HOURGLASS, 12., color))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color)
                                    .child(phase.label()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(dots)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(pal::muted())
                                    .child(format!("{sessions} sessions")),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_center()
                    .text_size(px(44.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(pal::text())
                    .child(time),
            )
            .child(ui::progress(frac, color, 6.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(toggle)
                    .child(reset)
                    .child(skip),
            )
    }

    // ── Média ──────────────────────────────────────────────────────────────
    fn media_card(&self, cx: &mut Context<Self>) -> Option<Div> {
        let m = self.mpris.read(cx);
        let title = m.title().map(str::to_string)?;
        let artist = m.artist().map(str::to_string).unwrap_or_default();
        let playing = adapt::is_playing(m);

        let (p, pp, n) = (self.mpris.clone(), self.mpris.clone(), self.mpris.clone());
        Some(
            card()
                .flex_row()
                .items_center()
                .gap_3()
                .child(ui::glyph(icons::MUSIC, 18., pal::accent2()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(pal::text())
                                .child(title),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(pal::muted())
                                .child(artist),
                        ),
                )
                .child(
                    btn("cc-mp-prev")
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            p.update(cx, |m, _| m.previous());
                        })
                        .child(ui::glyph(icons::PREV, 12., pal::muted())),
                )
                .child(
                    btn("cc-mp-play")
                        .w(px(34.))
                        .h(px(34.))
                        .bg(pal::accent())
                        .hover(|s| s.opacity(0.85))
                        .on_click(move |_, _, cx| {
                            pp.update(cx, |m, _| m.play_pause());
                        })
                        .child(ui::glyph(
                            if playing { icons::PAUSE } else { icons::PLAY },
                            12.,
                            pal::on_accent(),
                        )),
                )
                .child(
                    btn("cc-mp-next")
                        .hover(|s| s.bg(pal::raised()))
                        .on_click(move |_, _, cx| {
                            n.update(cx, |m, _| m.next());
                        })
                        .child(ui::glyph(icons::NEXT, 12., pal::muted())),
                ),
        )
    }

    // ── Thèmes ─────────────────────────────────────────────────────────────
    fn themes_card(&self) -> Div {
        let current = theme::current_index();
        let mut grid = div().flex().flex_row().flex_wrap().gap_2();
        for (i, th) in theme::THEMES.iter().enumerate() {
            let selected = i == current;
            grid = grid.child(
                div()
                    .id(("cc-theme", i))
                    .size(px(40.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(theme::swatch(th.bg))
                    .border_2()
                    .border_color(if selected {
                        pal::text()
                    } else {
                        theme::swatch(th.border)
                    })
                    .hover(|s| s.opacity(0.8))
                    .on_click(move |_, _, cx| theme::set(i, cx))
                    .child(
                        div()
                            .size(px(12.))
                            .rounded_full()
                            .bg(theme::swatch(th.accent)),
                    )
                    .child(
                        div()
                            .size(px(12.))
                            .rounded_full()
                            .bg(theme::swatch(th.accent2)),
                    ),
            );
        }

        card()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(icons::BRUSH, 12., pal::muted()))
                    .child(section_title("Thème"))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_end()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(pal::accent())
                            .child(theme::current().name),
                    ),
            )
            .child(grid)
    }

    // ── En-tête : batterie ─────────────────────────────────────────────────
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let b = self.battery.read(cx);
        let (pct, charging) = (b.percent(), b.is_charging());
        let color = if charging {
            pal::ok()
        } else if pct <= 15 {
            pal::danger()
        } else {
            pal::text()
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_color(pal::text())
                    .font_weight(FontWeight::BOLD)
                    .child("Centre de contrôle"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(ui::glyph(ui::bat_glyph(pct), 14., color))
                    .child(div().text_sm().text_color(color).child(format!(
                        "{pct}%{}",
                        if charging { " · en charge" } else { "" }
                    ))),
            )
    }
}

impl Render for ControlCenter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        // Responsive: le panneau ne dépasse jamais le viewport, le contenu
        // en trop scrolle dans `#cc-scroll` (58px de décalage haut + marges).
        let panel_max_h = window.viewport_size().height - px(82.);
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.header(cx))
            .child(self.toggles(cx))
            .child(self.sliders(cx))
            .children(self.wifi_card(cx))
            .children(self.bluetooth_card(cx))
            .child(self.pomodoro_card(cx))
            .children(self.media_card(cx))
            .child(self.themes_card());

        ui::backdrop("cc-bg", ids::CONTROL, 0.0)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                // Saisie mot de passe Wi-Fi : le champ capte tout, Entrée
                // valide, Échap annule la saisie (pas la fenêtre).
                if this.wifi_pw_for.is_some() {
                    match this.wifi_pw.update(cx, |b, _| b.handle_key(ev)) {
                        TextAction::Submit => this.wifi_submit_pw(cx),
                        TextAction::Cancel => this.wifi_cancel_pw(cx),
                        TextAction::Changed => cx.notify(),
                        TextAction::Ignored => {}
                    }
                    return;
                }
                if ui::key(ev) == "escape" {
                    cx.hide_window(ids::CONTROL);
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
                    .p_4()
                    .rounded_2xl()
                    .bg(pal::bg_a(0.97))
                    .border_1()
                    .border_color(pal::border())
                    .shadow_lg()
                    .child(
                        div()
                            .id("cc-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(content),
                    ),
            )
    }
}
