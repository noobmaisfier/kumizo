//! KILN : un shell WidgetKit complet.
//!
//! Topologie :
//!   - Rail vertical flottant a gauche (workspaces, volume, batterie, horloge empilee)
//!   - Ilot dynamique en haut (media en cours, sinon date)
//!   - Lanceur plein ecran (recherche clavier)
//!   - Selecteur de themes (fleches / Entree / Echap)
//!   - Toasts de notifications + OSD volume (apparaissent a la demande)

mod theme;

use gpui::prelude::*;
use gpui::{div, px, KeyDownEvent, FocusHandle, Window, Context, Render, Entity, IntoElement};
use std::time::Duration;
use theme::{framed, pal};
use kumizo_kit::prelude::*;

fn main() {
    theme::init_wallpaper();

    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .service::<Audio>()
        .service::<Compositor>()
        .service::<Apps>()
        .service::<Mpris>()
        .service::<Notifications>()
        .window(rail_window(), |cx: &mut gpui::App| cx.new(Rail::new))
        .window(island_window(), |cx: &mut gpui::App| cx.new(Island::new))
        .window(launcher_window(), |cx: &mut gpui::App| cx.new(Launcher::new))
        .window(theme_window(), |cx: &mut gpui::App| cx.new(ThemeSwitcher::new))
        .window(toasts_window(), |cx: &mut gpui::App| cx.new(Toasts::new))
        .window(osd_window(), |cx: &mut gpui::App| cx.new(Osd::new))
        .run();
}

// ---------------------------------------------------------------------------
// Fenetres layer-shell
// ---------------------------------------------------------------------------

fn rail_window() -> LayerWindow {
    LayerWindow::new("rail")
        .layer(Layer::Top)
        .anchor(Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM)
        .width(56.0)
        .margins(Margins::new(8, 0, 8, 8))
        .exclusive_zone(64) // 56 + marge gauche 8
        .namespace("kiln-rail")
        .transparent(true)
}

fn island_window() -> LayerWindow {
    LayerWindow::new("island")
        .layer(Layer::Top)
        .anchor(Anchor::TOP)
        .size(Some(440.0), Some(40.0))
        .margins(Margins::new(8, 0, 0, 0))
        .exclusive_zone(0)
        .namespace("kiln-island")
        .transparent(true)
}

fn launcher_window() -> LayerWindow {
    LayerWindow::new("launcher")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .start_hidden(true)
        .namespace("kiln-launcher")
        .transparent(true)
}

fn theme_window() -> LayerWindow {
    LayerWindow::new("theme")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .start_hidden(true)
        .namespace("kiln-theme")
        .transparent(true)
}

fn toasts_window() -> LayerWindow {
    LayerWindow::new("toasts")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .size(Some(360.0), Some(320.0))
        .margins(Margins::new(12, 12, 0, 0))
        .exclusive_zone(0)
        .start_hidden(true)
        .namespace("kiln-toasts")
        .transparent(true)
}

fn osd_window() -> LayerWindow {
    LayerWindow::new("osd")
        .layer(Layer::Overlay)
        .anchor(Anchor::BOTTOM)
        .size(Some(260.0), Some(48.0))
        .margins(Margins::new(0, 0, 48, 0))
        .exclusive_zone(0)
        .start_hidden(true)
        .namespace("kiln-osd")
        .transparent(true)
}

// ---------------------------------------------------------------------------
// Petits composants
// ---------------------------------------------------------------------------

/// Bouton glyphe du rail (le hover est defini ICI uniquement).
fn rail_button(id: &'static str, glyph: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(36.0))
        .h(px(36.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px((pal::radius() * 0.6).min(12.0)))
        .text_color(pal::text_muted())
        .cursor_pointer()
        .hover(|s| s.bg(pal::bg()).text_color(pal::accent()))
        .child(glyph)
}

// ---------------------------------------------------------------------------
// Rail vertical
// ---------------------------------------------------------------------------

struct Rail {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    audio: Entity<Audio>,
    compositor: Entity<Compositor>,
    notifs: Entity<Notifications>,
}

impl Rail {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            audio: cx.service::<Audio>(),
            compositor: cx.service::<Compositor>(),
            notifs: cx.service::<Notifications>(),
        }
    }
}

impl Render for Rail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let hh = format!("{:02}", clock.hour());
        let mm = format!("{:02}", clock.minute());
        let bat = self.battery.read(cx);
        let pct = bat.percent();
        let charging = bat.is_charging();
        let bat_color = if charging {
            pal::secondary()
        } else if pct < 20 {
            pal::urgent()
        } else {
            pal::accent()
        };
        let audio = self.audio.read(cx);
        let vol_label = if audio.is_muted() { "×".to_string() } else { format!("{}", audio.volume()) };
        let comp = self.compositor.read(cx);
        let notifs = self.notifs.read(cx);
        let unread = notifs.unread_count();
        let dnd = notifs.is_dnd();

        let workspaces = comp.workspaces().iter().map(|ws| {
            let id = ws.id;
            let active = ws.is_active;
            div()
                .id(format!("ws-{}", id))
                .w(px(28.0))
                .h(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px((pal::radius() * 0.5).min(10.0)))
                .text_size(px(12.0))
                .cursor_pointer()
                .when(active, |s| s.bg(pal::accent()).text_color(pal::bg()))
                .when(!active, |s| s.text_color(pal::text_muted()))
                .child(ws.name.clone())
                .on_click(move |_, _, cx| {
                    cx.service::<Compositor>().update(cx, |c, _| c.switch_workspace(id));
                })
        });

        framed(
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .py_3()
                .gap_3()
                .bg(pal::surface())
                .text_color(pal::text())
                .font_family(pal::font()),
        )
        // Lanceur
        .child(
            rail_button("btn-launcher", "◈")
                .text_color(pal::accent())
                .on_click(|_, _, cx| cx.toggle_window("launcher")),
        )
        // Separateur
        .child(div().w(px(20.0)).h(px(1.0)).bg(pal::border()))
        // Workspaces
        .child(div().flex().flex_col().items_center().gap_1().children(workspaces))
        // Pousse le reste en bas
        .child(div().flex_1())
        // Notifications : badge non-lus / DND
        .child(
            div()
                .relative()
                .child(
                    rail_button("btn-notif", if dnd { "☾" } else { "◔" }).on_click(|_, _, cx| {
                        cx.service::<Notifications>().update(cx, |n, _| n.toggle_dnd());
                    }),
                )
                .when(unread > 0, |s| {
                    s.child(
                        div()
                            .absolute()
                            .top(px(2.0))
                            .right(px(2.0))
                            .w(px(8.0))
                            .h(px(8.0))
                            .rounded_full()
                            .bg(pal::urgent()),
                    )
                }),
        )
        // Volume
        .child(
            div()
                .id("btn-vol")
                .flex()
                .flex_col()
                .items_center()
                .text_size(px(11.0))
                .text_color(pal::text_muted())
                .cursor_pointer()
                .child("♪")
                .child(vol_label)
                .on_click(|_, _, cx| {
                    cx.service::<Audio>().update(cx, |a, _| a.toggle_mute());
                }),
        )
        // Batterie : jauge verticale
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .w(px(6.0))
                        .h(px(40.0))
                        .rounded_full()
                        .bg(pal::bg())
                        .flex()
                        .flex_col()
                        .justify_end()
                        .child(
                            div()
                                .w_full()
                                .h(px(40.0 * (pct as f32).min(100.0) / 100.0))
                                .rounded_full()
                                .bg(bat_color),
                        ),
                )
                .child(div().text_size(px(10.0)).text_color(pal::text_muted()).child(format!("{}", pct))),
        )
        // Horloge empilee
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(div().text_size(px(20.0)).text_color(pal::text()).child(hh))
                .child(div().text_size(px(20.0)).text_color(pal::accent()).child(mm)),
        )
        // Themes
        .child(rail_button("btn-theme", "◐").on_click(|_, _, cx| cx.toggle_window("theme")))
    }
}

// ---------------------------------------------------------------------------
// Ilot dynamique
// ---------------------------------------------------------------------------

struct Island {
    clock: Entity<Clock>,
    mpris: Entity<Mpris>,
}

impl Island {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { clock: cx.service::<Clock>(), mpris: cx.service::<Mpris>() }
    }
}

impl Render for Island {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let mpris = self.mpris.read(cx);
        let playing = matches!(mpris.status(), PlaybackStatus::Playing);
        let title = mpris.title().map(|s| s.to_string());
        let artist = mpris.artist().map(|s| s.to_string());

        let base = framed(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_between()
                .px_4()
                .bg(pal::surface())
                .text_color(pal::text())
                .text_size(px(13.0))
                .font_family(pal::font()),
        )
        .rounded_full();

        match (playing, title) {
            (true, Some(t)) => {
                let label = match artist {
                    Some(a) => format!("{} — {}", t, a),
                    None => t,
                };
                base.child(div().text_color(pal::accent()).child("♪"))
                    .child(div().flex_1().min_w_0().truncate().mx_3().child(label))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .text_color(pal::text_muted())
                            .child(
                                div().id("prev").cursor_pointer().child("⏮").on_click(|_, _, cx| {
                                    cx.service::<Mpris>().update(cx, |m, _| m.previous());
                                }),
                            )
                            .child(
                                div()
                                    .id("pp")
                                    .cursor_pointer()
                                    .text_color(pal::accent())
                                    .child("⏯")
                                    .on_click(|_, _, cx| {
                                        cx.service::<Mpris>().update(cx, |m, _| m.play_pause());
                                    }),
                            )
                            .child(
                                div().id("next").cursor_pointer().child("⏭").on_click(|_, _, cx| {
                                    cx.service::<Mpris>().update(cx, |m, _| m.next());
                                }),
                            ),
                    )
            }
            _ => base
                .justify_center()
                .gap_3()
                .child(div().text_color(pal::text_muted()).child(clock.date_formatted()))
                .child(div().text_color(pal::accent()).child(clock.time_hm())),
        }
    }
}

// ---------------------------------------------------------------------------
// Lanceur
// ---------------------------------------------------------------------------

struct Launcher {
    apps: Entity<Apps>,
    query: String,
    selected: usize,
    focus: FocusHandle,
}

impl Launcher {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { apps: cx.service::<Apps>(), query: String::new(), selected: 0, focus: cx.focus_handle() }
    }

    fn results(&self, cx: &Context<Self>) -> Vec<AppInfo> {
        let apps = self.apps.read(cx);
        let mut v = if self.query.is_empty() { apps.apps().to_vec() } else { apps.search(&self.query) };
        v.truncate(8);
        v
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.query.clear();
        self.selected = 0;
        cx.hide_window("launcher");
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let results = self.results(cx);
        let sel = self.selected.min(results.len().saturating_sub(1));
        let query = self.query.clone();

        let rows = results.iter().enumerate().map(|(i, app)| {
            let app_c = app.clone();
            let active = i == sel;
            div()
                .id(format!("app-{}", i))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded(px((pal::radius() * 0.5).min(10.0)))
                .cursor_pointer()
                .when(active, |s| s.bg(pal::accent()).text_color(pal::bg()))
                .child(match app.icon.clone() {
                    Some(icon) => div().child(gpui::img(icon).w(px(28.0)).h(px(28.0))),
                    None => div().w(px(28.0)).child("◇"),
                })
                .child(div().min_w_0().truncate().child(app.name.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.service::<Apps>().update(cx, |a, _| a.launch(&app_c));
                    this.close(cx);
                }))
        });

        div()
            .id("launcher-backdrop")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000b0))
            .track_focus(&self.focus)
            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                let n = this.results(cx).len();
                match ev.keystroke.key.as_str() {
                    "escape" => this.close(cx),
                    "down" => this.selected = (this.selected + 1).min(n.saturating_sub(1)),
                    "up" => this.selected = this.selected.saturating_sub(1),
                    "backspace" => {
                        this.query.pop();
                        this.selected = 0;
                    }
                    "enter" => {
                        if let Some(app) = this.results(cx).get(this.selected).cloned() {
                            cx.service::<Apps>().update(cx, |a, _| a.launch(&app));
                            this.close(cx);
                        }
                    }
                    _ => {
                        let m = &ev.keystroke.modifiers;
                        if !m.control && !m.alt && !m.platform {
                            if let Some(ch) = &ev.keystroke.key_char {
                                this.query.push_str(ch);
                                this.selected = 0;
                            }
                        }
                    }
                }
                cx.notify();
            }))
            .child(
                framed(
                    div()
                        .w(px(520.0))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_4()
                        .bg(pal::surface())
                        .text_color(pal::text())
                        .font_family(pal::font()),
                )
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(px(22.0))
                        .child(if query.is_empty() {
                            div().text_color(pal::text_muted()).child("Rechercher une application…")
                        } else {
                            div().child(format!("{}▏", query))
                        }),
                )
                .child(div().h(px(1.0)).bg(pal::border()))
                .children(rows),
            )
    }
}

// ---------------------------------------------------------------------------
// Selecteur de themes
// ---------------------------------------------------------------------------

struct ThemeSwitcher {
    selected: usize,
    focus: FocusHandle,
}

impl ThemeSwitcher {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { selected: theme::current_index(), focus: cx.focus_handle() }
    }
}

impl Render for ThemeSwitcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let themes = theme::all();
        let current = theme::current_index();

        let rows = themes.iter().enumerate().map(|(i, t)| {
            let active = i == self.selected;
            let sw = t.swatches();
            div()
                .id(format!("theme-{}", i))
                .flex()
                .items_center()
                .gap_4()
                .px_3()
                .py_2()
                .rounded(px((pal::radius() * 0.5).min(10.0)))
                .cursor_pointer()
                .when(active, |s| s.bg(pal::bg()).border_1().border_color(pal::accent()))
                .child(div().flex().gap_1().children(sw.into_iter().map(|c| {
                    div().w(px(18.0)).h(px(18.0)).rounded_sm().bg(c).border_1().border_color(pal::border())
                })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(t.meta.name.clone()))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(pal::text_muted())
                                .truncate()
                                .child(format!("{} · {}", t.meta.variant, t.wallpaper_file())),
                        ),
                )
                .when(i == current, |s| s.child(div().text_color(pal::accent()).child("●")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = i;
                    theme::apply(i, cx);
                    cx.hide_window("theme");
                }))
        });

        div()
            .id("theme-backdrop")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000a0))
            .track_focus(&self.focus)
            .on_click(|_, _, cx| cx.hide_window("theme"))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                let n = theme::all().len();
                match ev.keystroke.key.as_str() {
                    "escape" => cx.hide_window("theme"),
                    "down" | "right" => this.selected = (this.selected + 1) % n.max(1),
                    "up" | "left" => this.selected = (this.selected + n.max(1) - 1) % n.max(1),
                    "enter" => {
                        theme::apply(this.selected, cx);
                        cx.hide_window("theme");
                    }
                    _ => {}
                }
                cx.notify();
            }))
            .child(
                framed(
                    div()
                        .w(px(460.0))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_4()
                        .bg(pal::surface())
                        .text_color(pal::text())
                        .font_family(pal::font()),
                )
                .child(
                    div()
                        .pb_2()
                        .text_size(px(12.0))
                        .text_color(pal::text_muted())
                        .child("THEMES  ·  ↑↓ naviguer  ·  ⏎ appliquer  ·  esc fermer"),
                )
                .children(rows),
            )
    }
}

// ---------------------------------------------------------------------------
// Toasts de notifications
// ---------------------------------------------------------------------------

struct Toasts {
    notifs: Entity<Notifications>,
}

impl Toasts {
    fn new(cx: &mut Context<Self>) -> Self {
        let notifs = cx.service::<Notifications>();
        // Affiche / masque la fenetre selon la presence de toasts.
        cx.observe(&notifs, |this, _, cx| {
            if this.notifs.read(cx).toasts().is_empty() {
                cx.hide_window("toasts");
            } else {
                cx.show_window("toasts");
            }
        })
        .detach();
        Self { notifs }
    }
}

impl Render for Toasts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards = self.notifs.read(cx).toasts().iter().take(3).map(|n| {
            let id = n.id;
            framed(
                div()
                    .id(format!("toast-{}", id))
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .bg(pal::surface())
                    .text_color(pal::text())
                    .font_family(pal::font())
                    .cursor_pointer(),
            )
            .child(div().text_size(px(11.0)).text_color(pal::accent()).truncate().child(n.app_name.clone()))
            .child(div().min_w_0().truncate().child(n.summary.clone()))
            .child(div().text_size(px(12.0)).text_color(pal::text_muted()).min_w_0().truncate().child(n.body.clone()))
            .on_click(move |_, _, cx| {
                cx.service::<Notifications>().update(cx, |s, _| s.dismiss_toast(id));
            })
        });

        div().size_full().flex().flex_col().gap_2().children(cards)
    }
}

// ---------------------------------------------------------------------------
// OSD volume
// ---------------------------------------------------------------------------

struct Osd {
    audio: Entity<Audio>,
    last: Option<(u8, bool)>,
    gen: u64,
}

impl Osd {
    fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        cx.observe(&audio, |this, audio, cx| {
            let a = audio.read(cx);
            let now = (a.volume(), a.is_muted());
            let first = this.last.is_none();
            let changed = this.last != Some(now);
            this.last = Some(now);
            if first || !changed {
                return; // ignore l'etat initial
            }
            this.gen += 1;
            let g = this.gen;
            cx.show_window("osd");
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(1400)).await;
                let _ = this.update(cx, |t, cx| {
                    if t.gen == g {
                        cx.hide_window("osd");
                    }
                });
            })
            .detach();
        })
        .detach();
        Self { audio, last: None, gen: 0 }
    }
}

impl Render for Osd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = self.audio.read(cx);
        let vol = a.volume().min(100) as f32;
        let muted = a.is_muted();
        let fill = if muted { pal::text_muted() } else { pal::accent() };

        framed(
            div()
                .size_full()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .bg(pal::surface())
                .text_color(pal::text())
                .font_family(pal::font()),
        )
        .child(div().text_color(fill).child(if muted { "×" } else { "♪" }))
        .child(
            div().flex_1().h(px(6.0)).rounded_full().bg(pal::bg()).child(
                div()
                    .h_full()
                    .w(gpui::DefiniteLength::Fraction((vol / 100.0).clamp(0.0, 1.0)))
                    .rounded_full()
                    .bg(fill),
            ),
        )
        .child(div().w(px(32.0)).text_size(px(12.0)).child(format!("{}", vol as u32)))
    }
}
