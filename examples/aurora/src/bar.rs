//! Barre du haut : trois îlots flottants (gauche / centre / droite).
//!
//! C'est aussi le « chef d'orchestre » toujours vivant : elle observe audio,
//! luminosité et notifications pour déclencher l'OSD et les toasts.

use gpui::prelude::*;
use gpui::{div, px, App, Context, Div, Entity, FontWeight, Window};
use kumizo_kit::prelude::*;

use crate::{
    icons,
    osd::{self, OsdKind, OsdState},
    pomodoro::{self, Pomodoro},
    theme::pal,
    ui::{self, btn, ids, pill},
};

/// Icônes Nerd Font des workspaces 1-10 (reprises du waybar de
/// l'utilisateur) : la barre montre des glyphes, pas des chiffres.
/// Hors 1-10, le nom du workspace s'affiche en texte (repli).
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

/// Exécute un script au hasard de `~/.local/bin/change-theme/` (bascule
/// de thème sway/foot/yazi/nvim/tmux + fond d'écran). Sans script
/// exécutable, ne fait rien (pas d'erreur, pas de bruit).
fn shuffle_theme() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(std::path::PathBuf::from(home).join(".local/bin/change-theme")) else {
        return;
    };
    let mut scripts: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.metadata()
                    .map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0)
                    .unwrap_or(false)
        })
        .collect();
    scripts.sort();
    if scripts.is_empty() {
        return;
    }
    // Pas de dépendance de tirage : nanos xor pid, biais négligeable ici.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let pick = &scripts[((nanos ^ std::process::id() as u64 * 0x9e3779b1) % scripts.len() as u64) as usize];
    if let Some(s) = pick.to_str() {
        ui::run(s, &[]);
    }
}

pub struct TopBar {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    audio: Entity<Audio>,
    /// Gardée vivante : l'observer ci-dessus pilote l'OSD.
    #[allow(dead_code)]
    brightness: Entity<Brightness>,
    network: Entity<Network>,
    bluetooth: Entity<Bluetooth>,
    compositor: Entity<Compositor>,
    mpris: Entity<Mpris>,
    tray: Entity<Tray>,
    notifs: Entity<Notifications>,
    pomodoro: Entity<Pomodoro>,
    player: Entity<crate::music::Player>,
    osd: Entity<OsdState>,
    last_vol: (u8, bool),
    last_bri: u8,
    toasts_open: bool,
}

impl TopBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let brightness = cx.service::<Brightness>();
        let notifs = cx.service::<Notifications>();
        let pomodoro = pomodoro::handle(cx);
        let osd = osd::handle(cx);
        let player = crate::music::handle(cx);
        let mpris = cx.service::<Mpris>();

        cx.observe(&pomodoro, |_, _, cx| cx.notify()).detach();
        cx.observe(&player, |_, _, cx| cx.notify()).detach();
        // Le pill média unifié bascule selon la source active : la barre se
        // réveille aussi quand le navigateur change de titre/état (événements
        // seuls via `playerctl --follow`, zéro CPU au repos).
        cx.observe(&mpris, |_, _, cx| cx.notify()).detach();

        let last_vol = {
            let a = audio.read(cx);
            (a.volume(), a.is_muted())
        };
        let last_bri = brightness.read(cx).percent();

        // Volume → OSD
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

        // Luminosité → OSD
        cx.observe(&brightness, |this, b, cx| {
            let v = b.read(cx).percent();
            if v != this.last_bri {
                this.last_bri = v;
                let osd = this.osd.clone();
                osd.update(cx, |o, cx| o.show(OsdKind::Brightness, v, false, cx));
            }
        })
        .detach();

        // Notifications → toasts (la fenêtre n'est ouverte que s'il y en a)
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
            audio,
            brightness,
            network: cx.service::<Network>(),
            bluetooth: cx.service::<Bluetooth>(),
            compositor: cx.service::<Compositor>(),
            mpris,
            tray: cx.service::<Tray>(),
            notifs,
            pomodoro,
            player,
            osd,
            last_vol,
            last_bri,
            toasts_open: false,
        }
    }

    // ── Gauche : lanceur · workspaces · fenêtres · titre actif ─────────────
    fn left(&self, cx: &App) -> Div {
        let comp = self.compositor.read(cx);
        let workspaces: Vec<(i32, String, bool, bool, usize)> = comp
            .workspaces()
            .iter()
            .map(|w| (w.id, w.name.clone(), w.is_active, w.is_urgent, w.window_count))
            .collect();
        let title = comp.active_window().map(|a| {
            if a.title.is_empty() {
                a.class.clone()
            } else {
                a.title.clone()
            }
        });
        let n_windows = comp.windows().len();

        let mut ws_row = div().flex().flex_row().items_center().gap_1();
        for (id, name, active, urgent, count) in workspaces {
            let compositor = self.compositor.clone();
            let (bg, fg) = if active {
                (pal::accent(), pal::on_accent())
            } else if urgent {
                (pal::danger(), pal::on_accent())
            } else {
                (
                    pal::with_alpha(pal::raised(), 0.0),
                    if count > 0 { pal::text() } else { pal::muted() },
                )
            };
            ws_row = ws_row.child(
                btn(("ws", id as usize))
                    .min_w(px(if active { 36. } else { 26. }))
                    .bg(bg)
                    .text_color(fg)
                    .text_xs()
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .hover(|s| s.opacity(0.8))
                    .on_click(move |_, _, cx| {
                        compositor.update(cx, |c, _| c.switch_workspace(id));
                    })
                    .child(if (1..=10).contains(&id) {
                        ui::glyph(WS_ICONS[(id - 1) as usize], 13., fg)
                    } else {
                        div().text_color(fg).child(name)
                    }),
            );
        }

        let launcher = btn("bar-launcher")
            .w(px(28.))
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, cx| ui::toggle_exclusive(ids::LAUNCHER, cx))
            .child(ui::glyph(icons::SEARCH, 14., pal::accent()));

        let tasks = btn("bar-tasks")
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, cx| ui::toggle_exclusive(ids::TASKS, cx))
            .child(ui::glyph(icons::WINDOWS, 14., pal::text()))
            .child(
                div()
                    .text_xs()
                    .text_color(pal::muted())
                    .child(n_windows.to_string()),
            );

        let clipboard = btn("bar-clipboard")
            .w(px(28.))
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, cx| ui::toggle_exclusive(ids::CLIPBOARD, cx))
            .child(ui::glyph(icons::CLIPBOARD, 14., pal::text()));

        // Pinceau : bascule de thème (un script au hasard de
        // `~/.local/bin/change-theme/`).
        let theme = btn("bar-theme")
            .w(px(28.))
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, _| shuffle_theme())
            .child(ui::glyph(icons::BRUSH, 14., pal::text()));

        let mut row = pill()
            .child(launcher)
            .child(ws_row)
            .child(tasks)
            .child(clipboard)
            .child(theme);
        if let Some(t) = title {
            row = row.child(
                div()
                    .min_w_0()
                    .max_w(px(300.))
                    .truncate()
                    .text_sm()
                    .text_color(pal::muted())
                    .child(t),
            );
        }
        div().flex().min_w_0().child(row)
    }

    // ── Centre : horloge · média ───────────────────────────────────────────
    /// Mini-lecteur local : toujours un point d'entrée vers le panneau
    /// musique ; titre et commandes dès qu'un morceau est chargé.
    /// Indicateur média UNIQUE : le lecteur local et MPRIS (navigateur…)
    /// ne s'affichent plus côte à côte. Priorité : le local s'il joue,
    /// sinon MPRIS si elle joue, sinon la dernière source chargée en pause,
    /// sinon icône seule (accès souris à la fenêtre musique). Les boutons
    /// pilotent la source affichée.
    fn media_pill(&self, cx: &App) -> Div {
        #[derive(Clone, Copy, PartialEq)]
        enum Src {
            Local,
            Mpris,
        }

        let p = self.player.read(cx);
        let local_label = p.now().map(|t| format!("{} — {}", t.artist, t.title));
        let local_playing = p.playing;
        let m = self.mpris.read(cx);
        let mpris_label = m.title().map(|_| m.display_label());
        let mpris_playing = matches!(m.status(), PlaybackStatus::Playing);

        let src = if local_playing || (local_label.is_some() && !mpris_playing) {
            Src::Local
        } else if mpris_label.is_some() {
            Src::Mpris
        } else {
            Src::Local
        };
        let playing = match src {
            Src::Local => local_playing,
            Src::Mpris => mpris_playing,
        };
        let label = match src {
            Src::Local => local_label,
            Src::Mpris => mpris_label,
        };

        let glyph_color = if playing { pal::accent() } else { pal::muted() };
        let play_glyph = || {
            ui::glyph(
                if playing { icons::PAUSE } else { icons::PLAY },
                11.,
                pal::accent(),
            )
        };
        let mut row = pill();
        // Local : l'icône ouvre la fenêtre ; MPRIS : simple voyant (le
        // navigateur a déjà ses contrôles dans sa fenêtre).
        if src == Src::Local {
            row = row.child(
                btn("bar-music")
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(|_, _, cx| ui::toggle_exclusive(ids::MUSIC, cx))
                    .child(ui::glyph(icons::MUSIC, 13., glyph_color)),
            );
        } else {
            row = row.child(ui::glyph(icons::MUSIC, 12., glyph_color));
        }

        if let Some(text) = label {
            row = row.child(
                div()
                    .min_w_0()
                    .max_w(px(220.))
                    .truncate()
                    .text_sm()
                    .text_color(pal::text())
                    .child(text),
            );
            match src {
                Src::Local => {
                    let (prev, toggle, next) =
                        (self.player.clone(), self.player.clone(), self.player.clone());
                    row = row
                        .child(
                            btn("bar-mu-prev")
                                .hover(|s| s.bg(pal::raised()))
                                .on_click(move |_, _, cx| {
                                    prev.update(cx, |p, cx| p.prev(cx));
                                })
                                .child(ui::glyph(icons::PREV, 11., pal::muted())),
                        )
                        .child(
                            btn("bar-mu-play")
                                .hover(|s| s.bg(pal::raised()))
                                .on_click(move |_, _, cx| {
                                    toggle.update(cx, |p, cx| p.toggle(cx));
                                })
                                .child(play_glyph()),
                        )
                        .child(
                            btn("bar-mu-next")
                                .hover(|s| s.bg(pal::raised()))
                                .on_click(move |_, _, cx| {
                                    next.update(cx, |p, cx| p.next(cx));
                                })
                                .child(ui::glyph(icons::NEXT, 11., pal::muted())),
                        );
                }
                Src::Mpris => {
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
            }
        }
        div().flex().items_center().child(row)
    }

    fn center(&self, cx: &App) -> Div {
        let clock = self.clock.read(cx);
        let time = clock.time_hm().to_string();
        let date = clock.date_formatted().to_string();

        let clock_pill = pill()
            .id("bar-clock")
            .cursor_pointer()
            .hover(|s| s.bg(pal::raised()))
            .on_click(|_, _, cx| ui::toggle_exclusive(ids::NOTIFS, cx))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_color(pal::text())
                    .child(time),
            )
            .child(div().text_xs().text_color(pal::muted()).child(date));

        let mut row = div().flex().flex_row().items_center().gap_2().child(clock_pill);
        if crate::MUSIC_ENABLED {
            row = row.child(self.media_pill(cx));
        }

        row
    }

    // ── Droite : pomodoro · tray · système · notifications · power ─────────
    fn right(&self, cx: &App) -> Div {
        let mut row = div().flex().flex_row().items_center().gap_2();

        // Pomodoro
        {
            let p = self.pomodoro.read(cx);
            let color = if p.running { p.phase.color() } else { pal::muted() };
            let time = p.mmss();
            let pomodoro = self.pomodoro.clone();
            row = row.child(
                pill()
                    .id("bar-pomodoro")
                    .cursor_pointer()
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(move |_, _, cx| {
                        pomodoro.update(cx, |p, cx| p.toggle(cx));
                    })
                    .child(ui::glyph(icons::HOURGLASS, 13., color))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(time),
                    ),
            );
        }

        // Tray
        let tray_svc = self.tray.read(cx);
        let entries = tray_svc.items();
        if !entries.is_empty() {
            let mut tray_row = pill().gap_1();
            for (i, e) in entries.iter().enumerate() {
                let tray = self.tray.clone();
                let id = e.id.clone();
                let icon = match e.icon.clone() {
                    Some(img) => gpui::img(img).w(px(16.)).h(px(16.)).into_any_element(),
                    None => ui::glyph(icons::WINDOWS, 13., pal::muted()).into_any_element(),
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

        // Cluster système
        let net = self.network.read(cx);
        let (wifi_on, radio, wifi_avail) =
            (net.is_connected(), net.wifi_enabled(), net.has_radio());
        let bt = self.bluetooth.read(cx);
        let (bt_avail, bt_on, bt_count) = (bt.is_available(), bt.is_powered(), bt.connected_count());
        let a = self.audio.read(cx);
        let (vol, muted) = (a.volume(), a.is_muted());
        let b = self.battery.read(cx);
        let (pct, charging) = (b.percent(), b.is_charging());

        let bat_color = if charging {
            pal::ok()
        } else if pct <= 15 {
            pal::danger()
        } else if pct <= 30 {
            pal::warn()
        } else {
            pal::text()
        };

        let mut sys = div().flex().flex_row().items_center().gap_3();
        // Comme le Bluetooth : caché si pas de radio, accent si actif.
        // Le Wi-Fi garde un état de plus (connecté vs allumé-hors-ligne).
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
                        if muted { pal::muted() } else { pal::text() },
                    ))
                    .child(div().text_xs().child(if muted {
                        "muet".to_string()
                    } else {
                        format!("{vol}%")
                    })),
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
        row = row.child(bell);

        // Power
        row.child(
            pill()
                .id("bar-power")
                .px_3()
                .cursor_pointer()
                .hover(|s| s.bg(pal::raised()))
                .on_click(|_, _, cx| ui::toggle_exclusive(ids::POWER, cx))
                .child(ui::glyph(icons::POWER, 14., pal::danger())),
        )
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
