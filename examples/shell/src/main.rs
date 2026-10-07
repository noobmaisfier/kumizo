//! "Riso" — a Wayland shell dressed like a risograph print.
//!
//! Structure
//! ---------
//! * No topbar. A vertical **rail** on the left holds everything that used to
//!   be spread over the bar and the dock: launcher, workspaces, apps, media,
//!   tray, status gauges, notifications, power.
//! * A small **stamp** in the top-right corner carries the clock (and the
//!   current track when something is playing).
//! * Popups hang under the stamp; the launcher, clipboard and power menu are
//!   sheets printed on a pale paper veil.
//!
//! Look
//! ----
//! * Paper grey, electric blue, fluo pink, yellow, ink. 2px ink outlines,
//!   square corners, hard offset shadows (blur 0).
//! * Active text is "misprinted": a pink layer sits 1.5px off a blue one.
//!
//! Motion
//! ------
//! * Things are *printed*: a pink plate lands slightly off-register behind a
//!   card and settles into its hard shadow while the card fades in.
//! * Buttons are physical: hover lifts, press sinks and loses its shadow.
//! * Power tiles slam down one after another; calendar pages drop in when you
//!   flip months; OSD blocks pop as the level rises; the rail slides in once.
//!
//! Run it with: `cargo run --package shell-example`

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    point, pulsating_between, Animation, AnimationExt, AnyElement, BoxShadow, Div, ElementId,
    FocusHandle, KeyDownEvent, Rgba, Stateful,
};
use widgetkit::prelude::*;

const RAIL_WIDTH: f32 = 72.0;
const STAMP_WIDTH: f32 = 236.0;
const STAMP_HEIGHT: f32 = 80.0;
const OSD_WIDTH: f32 = 400.0;
const OSD_HEIGHT: f32 = 104.0;
const CALENDAR_WIDTH: f32 = 330.0;
const CALENDAR_HEIGHT: f32 = 392.0;
/// Where popups start, just under the stamp.
const POPUP_TOP: i32 = 98;

// ---------------------------------------------------------------------------
// Tokens and helpers
// ---------------------------------------------------------------------------

mod pal {
    use gpui::{rgb, rgba, Rgba};

    // Official Flexoki Light Palette by Steph Ango (https://stephango.com/flexoki)
    // Warm analog paper, deep inky contrast, and calibrated 600/400-series accents.

    /// Paper (#FFFCF0) — warm analog base paper.
    pub fn paper() -> Rgba { rgb(0xfffcf0) }

    /// Base-50 (#F2F0E5) — secondary surface / cards / input boxes.
    pub fn white() -> Rgba { rgb(0xf2f0e5) }

    /// Black / Ink (#100F0F) — deep, inky rich black for text, 2px borders, hard shadows.
    pub fn ink() -> Rgba { rgb(0x100f0f) }

    /// Blue 600 (#205EA6) — primary editorial blue accent (active workspaces, buttons).
    pub fn blue() -> Rgba { rgb(0x205ea6) }

    /// Red 400 (#D14D41) — warm terracotta red for misprint plates, battery low, mute.
    pub fn pink() -> Rgba { rgb(0xd14d41) }

    /// Yellow 400 (#D0A215) — warm golden amber for launcher tile and charging gauges.
    pub fn yellow() -> Rgba { rgb(0xd0a215) }

    /// Base-600 / Text Muted (#6F6E69) — stone graphite secondary text.
    pub fn gray() -> Rgba { rgb(0x6f6e69) }

    /// UI-2 (#DAD8CE) — subtle selection tint.
    pub fn tint() -> Rgba { rgb(0xdad8ce) }

    /// Translucent Paper Veil (#FFFCF0 at 91% opacity).
    pub fn veil() -> Rgba { rgba(0xfffcf0e8) }

    /// Faint Ink (#100F0F at 12% opacity).
    pub fn faint() -> Rgba { rgba(0x100f0f1f) }

    /// Clear transparent.
    pub fn clear() -> Rgba { rgba(0x00000000) }
}

fn hard(color: Rgba, d: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: color.into(),
        offset: point(px(d), px(d)),
        blur_radius: px(0.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

fn out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

fn staggered(d: f32, i: usize, n: usize, step: f32) -> f32 {
    let span = (1.0 - step * n.saturating_sub(1) as f32).max(0.25);
    ((d - step * i as f32) / span).clamp(0.0, 1.0)
}

/// Blue text over a pink plate that is 1.5px off register.
fn misprint(text: String, size: f32, top: Rgba, under: Rgba) -> Div {
    div()
        .relative()
        .text_size(px(size))
        .font_weight(gpui::FontWeight::BOLD)
        .child(
            div()
                .absolute()
                .top(px(1.5))
                .left(px(1.5))
                .text_color(under)
                .child(text.clone()),
        )
        .child(div().text_color(top).child(text))
}

/// A physical square button: lifts on hover, sinks on press.
fn tile(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .w(px(40.0))
        .h(px(40.0))
        .bg(pal::paper())
        .border_2()
        .border_color(pal::ink())
        .shadow(hard(pal::ink(), 3.0))
        .text_size(px(16.0))
        .text_color(pal::ink())
        .cursor_pointer()
        .active(|s| s.shadow(Vec::new()).top(px(3.0)).left(px(3.0)))
}

fn key_chip(key: &'static str, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .px_1p5()
                .bg(pal::white())
                .border_2()
                .border_color(pal::ink())
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(pal::ink())
                .child(key),
        )
        .child(div().text_size(px(11.0)).text_color(pal::ink()).child(label))
}

/// Printed-card entrance: a colored plate lands off-register behind the card
/// and settles into place while the card fades in.
fn printed<E: Styled + IntoElement + 'static>(card: E, key: &str, echo: Rgba) -> impl IntoElement {
    let k_plate = format!("{key}-plate");
    let k_card = format!("{key}-card");
    div()
        .relative()
        .child(
            div()
                .absolute()
                .size_full()
                .bg(echo)
                .border_2()
                .border_color(pal::ink())
                .with_animation(k_plate, Animation::new(Duration::from_millis(460)), |el, d| {
                    let off = 7.0 + 18.0 * (1.0 - out_back(d));
                    el.left(px(off)).top(px(off))
                }),
        )
        .child(
            card.relative()
                .with_animation(k_card, Animation::new(Duration::from_millis(300)), |el, d| {
                    el.opacity(d.min(1.0))
                }),
        )
}

fn caret() -> impl IntoElement {
    div()
        .w(px(10.0))
        .h(px(22.0))
        .bg(pal::blue())
        .with_animation(
            "caret",
            Animation::new(Duration::from_millis(900))
                .repeat()
                .with_easing(pulsating_between(0.0, 1.0)),
            |el, d| el.opacity(d),
        )
}

/// Tagged input field shared by launcher and clipboard.
fn find_field(tag: &'static str, tag_bg: Rgba, query: &str, placeholder: &'static str) -> Div {
    let empty = query.is_empty();
    div()
        .flex()
        .items_center()
        .gap_3()
        .h(px(52.0))
        .pr_3()
        .bg(pal::white())
        .border_2()
        .border_color(pal::ink())
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .px_3()
                .bg(tag_bg)
                .text_size(px(14.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(pal::ink())
                .child(tag),
        )
        .child(div().w(px(2.0)).h_full().bg(pal::ink()))
        .child(
            div()
                .flex()
                .items_center()
                .flex_1()
                .gap_0p5()
                .text_size(px(18.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .when(empty, |s| s.child(caret()))
                .child(
                    div()
                        .text_color(if empty { pal::gray() } else { pal::ink() })
                        .child(if empty { placeholder.to_string() } else { query.to_string() }),
                )
                .when(!empty, |s| s.child(caret())),
        )
}

// ---------------------------------------------------------------------------
// Entry point and windows
// ---------------------------------------------------------------------------

fn main() {
    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .service::<Network>()
        .service::<Tray>()
        .service::<Audio>()
        .service::<Brightness>()
        .service::<Mpris>()
        .service::<Compositor>()
        .service::<Apps>()
        .service::<Notifications>()
        .service::<Clipboard>()
        .window(rail_window(), |cx: &mut gpui::App| cx.new(Rail::new))
        .window(stamp_window(), |cx: &mut gpui::App| cx.new(Stamp::new))
        .window(osd_window(), |cx: &mut gpui::App| cx.new(VolumeOsd::new))
        .window(traymenu_window(), |cx: &mut gpui::App| cx.new(TrayMenu::new))
        .window(calendar_window(), |cx: &mut gpui::App| cx.new(Calendar::new))
        .window(launcher_window(), |cx: &mut gpui::App| cx.new(AppLauncher::new))
        .window(powermenu_window(), |cx: &mut gpui::App| cx.new(PowerMenu::new))
        .window(notifications_window(), |cx: &mut gpui::App| cx.new(NotificationToasts::new))
        .window(notification_center_window(), |cx: &mut gpui::App| cx.new(NotificationCenter::new))
        .window(clipboard_window(), |cx: &mut gpui::App| cx.new(ClipboardManager::new))
        .window(tasks_window(), |cx: &mut gpui::App| cx.new(TaskSwitcher::new))
        .run();
}

fn tasks_window() -> LayerWindow { fullscreen("tasks", "wk-tasks") }

fn rail_window() -> LayerWindow {
    LayerWindow::new("rail")
        .layer(Layer::Top)
        .anchor(Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM)
        .exclusive_zone(RAIL_WIDTH as i32 + 10)
        .size(Some(RAIL_WIDTH), None)
        .margins(Margins::new(8, 0, 8, 8))
        .namespace("wk-rail")
}

fn stamp_window() -> LayerWindow {
    LayerWindow::new("stamp")
        .layer(Layer::Top)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(STAMP_WIDTH)
        .height(STAMP_HEIGHT)
        .margins(Margins::new(10, 14, 0, 0))
        .namespace("wk-stamp")
}

fn osd_window() -> LayerWindow {
    LayerWindow::new("osd")
        .layer(Layer::Overlay)
        .anchor(Anchor::BOTTOM)
        .exclusive_zone(0)
        .width(OSD_WIDTH)
        .height(OSD_HEIGHT)
        .margins(Margins::new(0, 0, 56, 0))
        .namespace("wk-osd")
}

fn traymenu_window() -> LayerWindow {
    LayerWindow::new("traymenu")
        .layer(Layer::Overlay)
        .anchor(Anchor::LEFT | Anchor::BOTTOM)
        .exclusive_zone(0)
        .width(310.0)
        .height(430.0)
        .margins(Margins::new(0, 0, 110, 88))
        .namespace("wk-traymenu")
        .start_hidden(true)
}

fn calendar_window() -> LayerWindow {
    LayerWindow::new("calendar")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(CALENDAR_WIDTH)
        .height(CALENDAR_HEIGHT)
        .margins(Margins::new(POPUP_TOP, 14, 0, 0))
        .namespace("wk-calendar")
        .start_hidden(true)
}

fn fullscreen(name: &'static str, ns: &'static str) -> LayerWindow {
    LayerWindow::new(name)
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .namespace(ns)
        .start_hidden(true)
}

fn launcher_window() -> LayerWindow { fullscreen("launcher", "wk-launcher") }
fn powermenu_window() -> LayerWindow { fullscreen("powermenu", "wk-powermenu") }
fn clipboard_window() -> LayerWindow { fullscreen("clipboard", "wk-clipboard") }

fn notifications_window() -> LayerWindow {
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

fn notification_center_window() -> LayerWindow {
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

// ---------------------------------------------------------------------------
// Rail: launcher, tasks, workspaces, media, tray, status, bell, power
// ---------------------------------------------------------------------------

struct Rail {
    battery: Entity<Battery>,
    network: Entity<Network>,
    tray: Entity<Tray>,
    audio: Entity<Audio>,
    mpris: Entity<Mpris>,
    compositor: Entity<Compositor>,
    notifications: Entity<Notifications>,
}

impl Rail {
    fn new(cx: &mut Context<Self>) -> Self {
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

// ---------------------------------------------------------------------------
// Stamp: clock (and the current track)
// ---------------------------------------------------------------------------

struct Stamp {
    clock: Entity<Clock>,
    mpris: Entity<Mpris>,
}

impl Stamp {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { clock: cx.service::<Clock>(), mpris: cx.service::<Mpris>() }
    }
}

impl Render for Stamp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let mpris = self.mpris.read(cx);
        let time = format!("{}", clock.time_hms());
        let line2 = if mpris.title().is_some() {
            format!("\u{f00e}  {}", mpris.display_label())
        } else {
            format!("{}", clock.date_formatted())
        };

        let panel = div()
            .id("wk-clock")
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(pal::yellow())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 5.0))
            .cursor_pointer()
            .hover(|s| s.bg(pal::white()))
            .active(|s| s.shadow(Vec::new()).top(px(5.0)).left(px(5.0)))
            .on_click(|_, _w, cx| cx.toggle_window("calendar"))
            // Registration bar: the three plates of the print.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .h(px(7.0))
                    .child(div().flex_1().bg(pal::pink()))
                    .child(div().flex_1().bg(pal::blue()))
                    .child(div().flex_1().bg(pal::ink())),
            )
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px_3()
                    .pt_1()
                    .child(misprint(time, 28.0, pal::blue(), pal::pink()))
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child(line2),
                    ),
            );

        div().w_full().h_full().pr(px(6.0)).pb(px(6.0)).child(panel.with_animation(
            "stamp-in",
            Animation::new(Duration::from_millis(560)),
            |el, d| {
                let t = out_back(d);
                el.relative().top(px(-46.0 * (1.0 - t))).opacity(d.min(1.0))
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Tray context menu
// ---------------------------------------------------------------------------

struct TrayMenu {
    tray: Entity<Tray>,
}

impl TrayMenu {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { tray: cx.service::<Tray>() }
    }
}

fn render_menu_items(items: &[TrayMenuItem], tray: &Entity<Tray>) -> Vec<gpui::AnyElement> {
    let mut out = Vec::new();
    for item in items {
        if item.separator {
            out.push(div().h(px(2.0)).my_1().bg(pal::ink()).w_full().into_any_element());
            continue;
        }
        let id = item.id;
        let mut row = div()
            .id(format!("traymenu-item-{id}"))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .text_size(px(13.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(if item.enabled { pal::ink() } else { pal::gray() });
        if item.enabled {
            let tray_entity = tray.clone();
            row = row
                .hover(|h| h.bg(pal::yellow()))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    tray_entity.update(cx, |t, _| t.click_menu_item(id));
                });
        }
        let check = match item.toggle {
            Some(true) => "■ ",
            Some(false) => "□ ",
            None => "",
        };
        row = row.child(format!("{}{}", check, item.label));
        out.push(row.into_any_element());
        if !item.children.is_empty() {
            out.push(
                div()
                    .pl_4()
                    .flex()
                    .flex_col()
                    .children(render_menu_items(&item.children, tray))
                    .into_any_element(),
            );
        }
    }
    out
}

impl Render for TrayMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tray_entity = self.tray.clone();
        let Some(menu) = self.tray.read(cx).menu() else {
            return div().id("traymenu-empty").w_full().h_full();
        };
        div()
            .id("traymenu-backdrop")
            .w_full()
            .h_full()
            .pr(px(8.0))
            .pb(px(8.0))
            .on_click(move |_, _, cx| {
                tray_entity.update(cx, |t, _| t.close_menu());
            })
            .child(printed(
                div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .bg(pal::white())
                    .border_2()
                    .border_color(pal::ink())
                    .child(
                        div()
                            .px_3()
                            .py_1p5()
                            .bg(pal::blue())
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::paper())
                            .child(menu.title.clone()),
                    )
                    .child(div().h(px(2.0)).my_1().bg(pal::ink()).w_full())
                    .children(render_menu_items(&menu.items, &self.tray)),
                "traymenu",
                pal::pink(),
            ))
    }
}

// ---------------------------------------------------------------------------
// Calendar: a tear-off pad
// ---------------------------------------------------------------------------

struct Calendar {
    clock: Entity<Clock>,
    offset: i32,
}

impl Calendar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { clock: cx.service::<Clock>(), offset: 0 }
    }
}

fn ring(left: f32) -> Div {
    div()
        .absolute()
        .top(px(-9.0))
        .left(px(left))
        .w(px(12.0))
        .h(px(24.0))
        .rounded_full()
        .bg(pal::paper())
        .border_2()
        .border_color(pal::ink())
}

impl Render for Calendar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const NAMES: [&str; 12] = [
            "January", "February", "March", "April", "May", "June", "July", "August",
            "September", "October", "November", "December",
        ];
        let dt = self.clock.read(cx).datetime();
        let today = dt.day();
        let mut year = dt.year();
        let mut month = dt.month();
        for _ in 0..self.offset.unsigned_abs() {
            if self.offset > 0 {
                if month == time::Month::December {
                    year += 1;
                }
                month = month.next();
            } else {
                if month == time::Month::January {
                    year -= 1;
                }
                month = month.previous();
            }
        }
        let is_current = self.offset == 0;
        let month_name = NAMES[month as usize - 1];
        let days_in_month = time::util::days_in_month(month, year);
        let first_weekday = time::Date::from_calendar_date(year, month, 1)
            .map(|d| d.weekday().number_from_monday())
            .unwrap_or(1);
        let leading = (first_weekday - 1) as usize;
        let total_slots = leading + days_in_month as usize;
        let rows = (total_slots + 6) / 7;
        let page_key = format!("cal-page-{}", self.offset);

        let grid = div()
            .flex()
            .flex_col()
            .gap_1()
            .children((0..rows).map(|row| {
                div().flex().flex_row().justify_between().children((0..7usize).map(move |col| {
                    let slot = row * 7 + col;
                    if slot < leading || slot >= total_slots {
                        return div().w(px(38.0)).h(px(34.0)).into_any_element();
                    }
                    let day_num = (slot - leading + 1) as u8;
                    let is_today = is_current && day_num == today;
                    div()
                        .w(px(38.0))
                        .h(px(34.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.0))
                        .when(is_today, |s| {
                            s.bg(pal::pink())
                                .border_2()
                                .border_color(pal::ink())
                                .shadow(hard(pal::ink(), 2.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                        })
                        .when(!is_today, |s| {
                            s.border_1()
                                .border_color(pal::faint())
                                .text_color(if col >= 5 { pal::blue() } else { pal::ink() })
                                .hover(|h| h.bg(pal::yellow()))
                        })
                        .child(format!("{day_num}"))
                        .into_any_element()
                }))
            }))
            // Flipping a month drops a fresh page onto the pad.
            .with_animation(page_key, Animation::new(Duration::from_millis(380)), |el, d| {
                el.relative().top(px(-26.0 * (1.0 - out_back(d)))).opacity(d.min(1.0))
            });

        let mini = |id: &'static str, glyph: &'static str| {
            tile(id).w(px(30.0)).h(px(30.0)).shadow(hard(pal::ink(), 2.0)).child(glyph)
        };

        let card = div()
            .id("calendar-card")
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .bg(pal::white())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 5.0))
            .on_click(|_, _, _| {})
            .child(ring(70.0))
            .child(ring(CALENDAR_WIDTH - 70.0 - 12.0 - 14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .bg(pal::blue())
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .child(misprint(month_name.to_string(), 24.0, pal::paper(), pal::pink()))
                            .child(
                                div()
                                    .pb_1()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::yellow())
                                    .child(format!("{year}")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(mini("cal-prev", "‹").on_click(cx.listener(|this, _, _, cx| {
                                this.offset -= 1;
                                cx.notify();
                            })))
                            .when(!is_current, |s| {
                                s.child(
                                    mini("cal-today", "•")
                                        .bg(pal::yellow())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.offset = 0;
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(mini("cal-next", "›").on_click(cx.listener(|this, _, _, cx| {
                                this.offset += 1;
                                cx.notify();
                            }))),
                    ),
            )
            .child(div().h(px(2.0)).w_full().bg(pal::ink()))
            .child(
                div().flex().flex_col().gap_2().p_3().child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .children(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate().map(
                            |(i, day)| {
                                div()
                                    .w(px(38.0))
                                    .flex()
                                    .justify_center()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(if i >= 5 { pal::blue() } else { pal::ink() })
                                    .child(*day)
                            },
                        )),
                )
                .child(grid),
            );

        div()
            .id("calendar-backdrop")
            .w_full()
            .h_full()
            .pt(px(12.0))
            .pr(px(8.0))
            .pb(px(8.0))
            .on_click(|_, _, cx| {
                cx.hide_window("calendar");
            })
            .child(card)
    }
}

// ---------------------------------------------------------------------------
// Volume / brightness OSD
// ---------------------------------------------------------------------------

struct VolumeOsd {
    audio: Entity<Audio>,
    brightness: Entity<Brightness>,
    visible: bool,
    shown: OsdKind,
    last_volume: u8,
    last_muted: bool,
    last_brightness: u8,
    generation: u64,
    session: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OsdKind {
    Volume,
    Brightness,
}

fn reveal_osd(osd: &mut VolumeOsd, cx: &mut Context<VolumeOsd>, kind: OsdKind) {
    if !osd.visible {
        osd.session = osd.session.wrapping_add(1);
    }
    osd.shown = kind;
    osd.visible = true;
    osd.generation = osd.generation.wrapping_add(1);
    let current_gen = osd.generation;
    cx.notify();

    cx.spawn(async move |this, cx| {
        async_io::Timer::after(Duration::from_millis(2000)).await;
        let _ = this.update(cx, |osd, cx| {
            if osd.generation == current_gen {
                osd.visible = false;
                cx.notify();
            }
        });
    })
    .detach();
}

impl VolumeOsd {
    fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let brightness = cx.service::<Brightness>();
        let last_volume = audio.read(cx).volume();
        let last_muted = audio.read(cx).is_muted();
        let last_brightness = brightness.read(cx).percent();

        let osd = Self {
            audio,
            brightness,
            visible: false,
            shown: OsdKind::Volume,
            last_volume,
            last_muted,
            last_brightness,
            generation: 0,
            session: 0,
        };

        let watched = osd.audio.clone();
        cx.observe(&watched, |this, audio, cx| {
            let volume = audio.read(cx).volume();
            let muted = audio.read(cx).is_muted();
            if volume == this.last_volume && muted == this.last_muted {
                return;
            }
            this.last_volume = volume;
            this.last_muted = muted;
            reveal_osd(this, cx, OsdKind::Volume);
        })
        .detach();

        let watched_brightness = osd.brightness.clone();
        cx.observe(&watched_brightness, |this, brightness, cx| {
            let level = brightness.read(cx).percent();
            if level == this.last_brightness {
                return;
            }
            this.last_brightness = level;
            reveal_osd(this, cx, OsdKind::Brightness);
        })
        .detach();

        osd
    }
}

const OSD_BLOCKS: usize = 16;

/// Segmented level bar. Each newly filled block pops up like a type slug.
fn segments(pct: usize, fill: Rgba) -> Div {
    let filled = (pct * OSD_BLOCKS + 99) / 100;
    div()
        .flex()
        .flex_row()
        .items_end()
        .gap(px(2.0))
        .h(px(18.0))
        .children((0..OSD_BLOCKS).map(move |i| {
            let block = div().w(px(10.0)).border_1().border_color(pal::ink());
            if i < filled {
                block
                    .bg(fill)
                    .with_animation(
                        format!("seg-{i}"),
                        Animation::new(Duration::from_millis(300)),
                        |el, d| el.h(px((18.0 * out_back(d)).max(0.0))),
                    )
                    .into_any_element()
            } else {
                block.h(px(18.0)).bg(pal::white()).into_any_element()
            }
        }))
}

impl Render for VolumeOsd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().w_full().h_full();
        }

        let (icon, label, pct, fill) = match self.shown {
            OsdKind::Volume => {
                let audio = self.audio.read(cx);
                let muted = audio.is_muted();
                (
                    audio.icon(),
                    if muted { "Muted" } else { "Volume" },
                    audio.volume(),
                    if muted { pal::pink() } else { pal::blue() },
                )
            }
            OsdKind::Brightness => {
                let b = self.brightness.read(cx);
                (b.icon(), "Brightness", b.percent(), pal::yellow())
            }
        };

        div().flex().items_start().justify_center().w_full().h_full().pt(px(4.0)).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .w(px(OSD_WIDTH - 28.0))
                .p_4()
                .bg(pal::white())
                .border_2()
                .border_color(pal::ink())
                .shadow(hard(pal::ink(), 6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(46.0))
                        .h(px(46.0))
                        .bg(fill)
                        .border_2()
                        .border_color(pal::ink())
                        .text_size(px(22.0))
                        .text_color(if fill == pal::blue() { pal::paper() } else { pal::ink() })
                        .child(icon),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap_1p5()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(label),
                        )
                        .child(segments(pct as usize, fill)),
                )
                .child(
                    div()
                        .w(px(64.0))
                        .flex()
                        .justify_end()
                        .child(misprint(format!("{pct}"), 34.0, pal::blue(), pal::pink())),
                )
                .with_animation(
                    format!("osd-in-{}", self.session),
                    Animation::new(Duration::from_millis(340)),
                    |el, d| {
                        let t = out_back(d);
                        el.relative().top(px(26.0 * (1.0 - t))).opacity(d.min(1.0))
                    },
                ),
        )
    }
}

// ---------------------------------------------------------------------------
// Launcher: a printed sheet on a paper veil
// ---------------------------------------------------------------------------

struct AppLauncher {
    apps: Entity<Apps>,
    query: String,
    selected_index: usize,
    focus: FocusHandle,
}

impl AppLauncher {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            apps: cx.service::<Apps>(),
            query: String::new(),
            selected_index: 0,
            focus: cx.focus_handle(),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let matches = self.apps.read(cx).search(&self.query);
        let count = matches.len();

        match key {
            "escape" => {
                self.query.clear();
                self.selected_index = 0;
                cx.hide_window("launcher");
            }
            "enter" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let app = &matches[idx];
                    self.apps.read(cx).launch(app);
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("launcher");
                }
            }
            "up" => {
                if count > 0 {
                    self.selected_index =
                        if self.selected_index == 0 { count - 1 } else { self.selected_index - 1 };
                    cx.notify();
                }
            }
            "down" | "tab" => {
                if count > 0 {
                    self.selected_index = (self.selected_index + 1) % count;
                    cx.notify();
                }
            }
            "backspace" => {
                self.query.pop();
                self.selected_index = 0;
                cx.notify();
            }
            "space" => {
                self.query.push(' ');
                self.selected_index = 0;
                cx.notify();
            }
            s if s.chars().count() == 1 => {
                if let Some(c) = s.chars().next() {
                    if !c.is_control() {
                        self.query.push(c);
                        self.selected_index = 0;
                        cx.notify();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Render for AppLauncher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let matches = self.apps.read(cx).search(&self.query);
        let total = matches.len();
        let selected = self.selected_index.min(total.saturating_sub(1));
        let apps_service = self.apps.clone();

        let max_visible = 6;
        let start_idx = if selected >= max_visible { selected - max_visible + 1 } else { 0 };
        let visible_items: Vec<_> =
            matches.iter().enumerate().skip(start_idx).take(max_visible).collect();

        let card = div()
            .id("launcher-modal")
            .w(px(640.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .on_click(|_, _, _| {})
            .child(
                find_field("Find", pal::yellow(), &self.query, "Type an app name").child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(pal::blue())
                        .child(format!("{total}")),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .when(total == 0, |s| {
                        s.py_8()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child("No app by that name. Try fewer letters.")
                    })
                    .children(visible_items.into_iter().map(|(idx, app)| {
                        let is_selected = idx == selected;
                        let app_item = app.clone();
                        let apps_ref = apps_service.clone();
                        let app_key = format!("launcher-sel-{}", app.id);

                        let icon_el: AnyElement = if let Some(img) = &app.icon {
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::white())
                                .border_2()
                                .border_color(pal::ink())
                                .child(gpui::img(img.clone()).w(px(28.0)).h(px(28.0)))
                                .into_any_element()
                        } else {
                            let letter = app
                                .name
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string();
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::pink())
                                .border_2()
                                .border_color(pal::ink())
                                .text_size(px(18.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(letter)
                                .into_any_element()
                        };

                        let comment = if app.comment.is_empty() {
                            app.exec.clone()
                        } else {
                            app.comment.clone()
                        };

                        let row = div()
                            .id(format!("launcher-item-{}", app.id))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .border_2()
                            .cursor_pointer()
                            .when(is_selected, |s| {
                                s.bg(pal::yellow())
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                            })
                            .when(!is_selected, |s| {
                                s.border_color(pal::clear()).hover(|h| h.bg(pal::tint()))
                            })
                            .child(
                                div()
                                    .w(px(12.0))
                                    .text_size(px(14.0))
                                    .text_color(pal::blue())
                                    .child(if is_selected { "\u{25B6}" } else { "" }),
                            )
                            .child(icon_el)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(15.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .child(app.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(pal::ink())
                                            .opacity(0.65)
                                            .truncate()
                                            .child(comment),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                apps_ref.read(cx).launch(&app_item);
                                cx.hide_window("launcher");
                            });

                        // The selected row snaps in from the left.
                        if is_selected {
                            row.with_animation(
                                app_key,
                                Animation::new(Duration::from_millis(200)),
                                |el, d| el.relative().left(px(-12.0 * (1.0 - out_cubic(d)))),
                            )
                            .into_any_element()
                        } else {
                            row.into_any_element()
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(key_chip("↑↓", "move"))
                    .child(key_chip("Enter", "open"))
                    .child(key_chip("Esc", "close")),
            );

        div()
            .id("launcher-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(pal::veil())
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .pt(px(120.0))
            .on_click(|_, _, cx| {
                cx.hide_window("launcher");
            })
            .child(printed(card, "launcher", pal::pink()))
    }
}

// ---------------------------------------------------------------------------
// Power menu: five slabs that slam down
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum PowerAction {
    Lock,
    Suspend,
    Logout,
    Reboot,
    Poweroff,
}

impl PowerAction {
    fn run(self) {
        use std::process::Command;
        match self {
            PowerAction::Lock => {
                let _ = Command::new("loginctl").arg("lock-session").spawn();
            }
            PowerAction::Suspend => {
                let _ = Command::new("systemctl").arg("suspend").spawn();
            }
            PowerAction::Logout => {
                let _ = Command::new("swaymsg").arg("exit").spawn();
                let _ = Command::new("loginctl").args(["terminate-session", ""]).spawn();
            }
            PowerAction::Reboot => {
                let _ = Command::new("systemctl").arg("reboot").spawn();
            }
            PowerAction::Poweroff => {
                let _ = Command::new("systemctl").arg("poweroff").spawn();
            }
        }
    }
}

struct PowerMenu {
    focus: FocusHandle,
}

impl PowerMenu {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { focus: cx.focus_handle() }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let action = match event.keystroke.key.as_str() {
            "escape" | "q" => {
                cx.hide_window("powermenu");
                return;
            }
            "l" => PowerAction::Lock,
            "s" => PowerAction::Suspend,
            "e" => PowerAction::Logout,
            "r" => PowerAction::Reboot,
            "p" => PowerAction::Poweroff,
            _ => return,
        };
        action.run();
        cx.hide_window("powermenu");
    }
}

#[allow(clippy::too_many_arguments)]
fn power_tile(
    i: usize,
    n: usize,
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    key: &'static str,
    bg: Rgba,
    fg: Rgba,
    action: PowerAction,
) -> impl IntoElement {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .w(px(128.0))
        .h(px(128.0))
        .bg(bg)
        .border_2()
        .border_color(pal::ink())
        .shadow(hard(pal::ink(), 6.0))
        .text_color(fg)
        .cursor_pointer()
        .hover(|s| s.top(px(-3.0)).left(px(-3.0)).shadow(hard(pal::ink(), 9.0)))
        .active(|s| s.top(px(5.0)).left(px(5.0)).shadow(Vec::new()))
        .child(div().text_size(px(36.0)).child(glyph))
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(gpui::FontWeight::BOLD)
                .child(label),
        )
        // Shortcut sticker on the corner.
        .child(
            div()
                .absolute()
                .top(px(-10.0))
                .right(px(-10.0))
                .flex()
                .items_center()
                .justify_center()
                .w(px(24.0))
                .h(px(24.0))
                .bg(pal::white())
                .border_2()
                .border_color(pal::ink())
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(pal::ink())
                .child(key),
        )
        .on_click(move |_, _, cx| {
            action.run();
            cx.hide_window("powermenu");
        })
        // Each slab slams down a beat after the previous one.
        .with_animation(
            format!("pw-tile-{id}"),
            Animation::new(Duration::from_millis(760)),
            move |el, d| {
                let t = staggered(d, i, n, 0.12);
                el.relative().top(px(-80.0 * (1.0 - out_back(t)))).opacity((t * 3.0).min(1.0))
            },
        )
}

impl Render for PowerMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let user = std::env::var("USER").unwrap_or_else(|_| "User".to_string());
        let n = 5;

        div()
            .id("powermenu-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(pal::veil())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_10()
            .on_click(|_, _, cx| {
                cx.hide_window("powermenu");
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(misprint(
                        format!("Done for today, {user}?"),
                        44.0,
                        pal::blue(),
                        pal::pink(),
                    ))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child("Pick a slab, or press the letter on its corner."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_6()
                    .child(power_tile(0, n, "pw-lock", "\u{f023}", "Lock", "L", pal::yellow(), pal::ink(), PowerAction::Lock))
                    .child(power_tile(1, n, "pw-suspend", "\u{f186}", "Suspend", "S", pal::blue(), pal::paper(), PowerAction::Suspend))
                    .child(power_tile(2, n, "pw-logout", "\u{f2f5}", "Log out", "E", pal::paper(), pal::ink(), PowerAction::Logout))
                    .child(power_tile(3, n, "pw-reboot", "\u{f021}", "Restart", "R", pal::pink(), pal::ink(), PowerAction::Reboot))
                    .child(power_tile(4, n, "pw-shutdown", "\u{f011}", "Shut down", "P", pal::ink(), pal::yellow(), PowerAction::Poweroff)),
            )
            .child(key_chip("Esc", "stay logged in"))
    }
}

// ---------------------------------------------------------------------------
// Notification toasts
// ---------------------------------------------------------------------------

struct NotificationToasts {
    notifications: Entity<Notifications>,
}

impl NotificationToasts {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { notifications: cx.service::<Notifications>() }
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

// ---------------------------------------------------------------------------
// Notification center
// ---------------------------------------------------------------------------

struct NotificationCenter {
    notifications: Entity<Notifications>,
}

impl NotificationCenter {
    fn new(cx: &mut Context<Self>) -> Self {
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
                        let notifs_remove = self.notifications.clone();
                        let tab = urgency_plate(&notif.urgency);
                        let app_icon = notif.app_icon.clone();
                        let summary = notif.summary.clone();
                        let body = notif.body.clone();
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
                            .hover(|h| h.bg(pal::tint()))
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
                                                        notifs_remove.update(cx, |n, _| {
                                                            n.remove_history(notif_id)
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

// ---------------------------------------------------------------------------
// Clipboard manager
// ---------------------------------------------------------------------------

struct ClipboardManager {
    clipboard: Entity<Clipboard>,
    query: String,
    selected_index: usize,
    focus: FocusHandle,
}

impl ClipboardManager {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clipboard: cx.service::<Clipboard>(),
            query: String::new(),
            selected_index: 0,
            focus: cx.focus_handle(),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let matches = self.clipboard.read(cx).search(&self.query);
        let count = matches.len();

        match key {
            "escape" => {
                self.query.clear();
                self.selected_index = 0;
                cx.hide_window("clipboard");
            }
            "enter" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let item_id = matches[idx].id;
                    self.clipboard.update(cx, |c, _| c.copy_item(item_id));
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("clipboard");
                }
            }
            "delete" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let item_id = matches[idx].id;
                    self.clipboard.update(cx, |c, _| c.remove_item(item_id));
                    if self.selected_index >= count.saturating_sub(1) {
                        self.selected_index = self.selected_index.saturating_sub(1);
                    }
                    cx.notify();
                }
            }
            "up" => {
                if count > 0 {
                    self.selected_index =
                        if self.selected_index == 0 { count - 1 } else { self.selected_index - 1 };
                    cx.notify();
                }
            }
            "down" | "tab" => {
                if count > 0 {
                    self.selected_index = (self.selected_index + 1) % count;
                    cx.notify();
                }
            }
            "backspace" => {
                self.query.pop();
                self.selected_index = 0;
                cx.notify();
            }
            "space" => {
                self.query.push(' ');
                self.selected_index = 0;
                cx.notify();
            }
            s if s.chars().count() == 1 => {
                if let Some(c) = s.chars().next() {
                    if !c.is_control() {
                        self.query.push(c);
                        self.selected_index = 0;
                        cx.notify();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Render for ClipboardManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let matches = self.clipboard.read(cx).search(&self.query);
        let total = matches.len();
        let selected = self.selected_index.min(total.saturating_sub(1));
        let clipboard_service = self.clipboard.clone();
        let clipboard_clear = self.clipboard.clone();

        let max_visible = 6;
        let start_idx = if selected >= max_visible { selected - max_visible + 1 } else { 0 };
        let visible_items: Vec<_> =
            matches.iter().enumerate().skip(start_idx).take(max_visible).collect();

        let card = div()
            .id("clipboard-modal")
            .w(px(640.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .on_click(|_, _, _| {})
            .child(
                find_field("Paste", pal::pink(), &self.query, "Search what you copied")
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::blue())
                            .child(format!("{total}")),
                    )
                    .when(total > 0, |this| {
                        this.child(
                            tile("clip-clear-btn")
                                .w_auto()
                                .h(px(28.0))
                                .px_3()
                                .shadow(hard(pal::ink(), 2.0))
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child("Clear all")
                                .on_click(move |_, _, cx| {
                                    clipboard_clear.update(cx, |c, _| c.clear());
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .when(total == 0, |s| {
                        s.py_8()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child("Nothing copied yet. Copy a line and it appears here.")
                    })
                    .children(visible_items.into_iter().map(|(idx, item)| {
                        let is_selected = idx == selected;
                        let item_id = item.id;
                        let clip_ref = clipboard_service.clone();
                        let clip_del = clipboard_service.clone();
                        let preview = item.preview();
                        let lines = item.line_count();
                        let chars = item.char_count();
                        let key = format!("clip-sel-{item_id}");

                        let row = div()
                            .id(format!("clip-item-{item_id}"))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .border_2()
                            .cursor_pointer()
                            .when(is_selected, |s| {
                                s.bg(pal::pink())
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                            })
                            .when(!is_selected, |s| {
                                s.border_color(pal::clear()).hover(|h| h.bg(pal::tint()))
                            })
                            // Ticket stub with the entry number.
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .w(px(30.0))
                                    .h(px(30.0))
                                    .bg(pal::white())
                                    .border_2()
                                    .border_color(pal::ink())
                                    .text_size(px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(format!("{}", idx + 1)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .w_full()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .truncate()
                                            .child(preview),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(pal::ink())
                                            .opacity(0.65)
                                            .child(format!(
                                                "{lines} line{}, {chars} characters",
                                                if lines > 1 { "s" } else { "" }
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id(format!("clip-del-{item_id}"))
                                    .flex_shrink_0()
                                    .px_1p5()
                                    .text_color(pal::ink())
                                    .hover(|h| h.bg(pal::yellow()))
                                    .child("✕")
                                    .on_click(move |_, _, cx| {
                                        clip_del.update(cx, |c, _| c.remove_item(item_id));
                                    }),
                            )
                            .on_click(move |_, _, cx| {
                                clip_ref.update(cx, |c, _| c.copy_item(item_id));
                                cx.hide_window("clipboard");
                            });

                        if is_selected {
                            row.with_animation(
                                key,
                                Animation::new(Duration::from_millis(200)),
                                |el, d| el.relative().left(px(-12.0 * (1.0 - out_cubic(d)))),
                            )
                            .into_any_element()
                        } else {
                            row.into_any_element()
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(key_chip("Enter", "copy"))
                    .child(key_chip("Del", "remove"))
                    .child(key_chip("Esc", "close")),
            );

        div()
            .id("clipboard-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(pal::veil())
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .pt(px(120.0))
            .on_click(|_, _, cx| {
                cx.hide_window("clipboard");
            })
            .child(printed(card, "clipboard", pal::blue()))
    }
}

// ---------------------------------------------------------------------------
// Task Switcher (open windows view with decoded icons)
// ---------------------------------------------------------------------------

struct TaskSwitcher {
    focus: FocusHandle,
    compositor: Entity<Compositor>,
    apps: Entity<Apps>,
    query: String,
    selected_index: usize,
}

impl TaskSwitcher {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            compositor: cx.service::<Compositor>(),
            apps: cx.service::<Apps>(),
            query: String::new(),
            selected_index: 0,
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let comp = self.compositor.read(cx);
        let all_windows = comp.windows();
        let q = self.query.trim().to_lowercase();
        let filtered: Vec<_> = all_windows
            .iter()
            .filter(|w| {
                if q.is_empty() {
                    true
                } else {
                    w.title.to_lowercase().contains(&q) || w.class.to_lowercase().contains(&q)
                }
            })
            .collect();
        let count = filtered.len();

        match key {
            "escape" => {
                self.query.clear();
                self.selected_index = 0;
                cx.hide_window("tasks");
            }
            "enter" => {
                if !filtered.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let target = filtered[idx];
                    let con_id = target.id;
                    self.compositor.read(cx).focus_window(con_id);
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("tasks");
                }
            }
            "up" => {
                if count > 0 {
                    self.selected_index =
                        if self.selected_index == 0 { count - 1 } else { self.selected_index - 1 };
                    cx.notify();
                }
            }
            "down" | "tab" => {
                if count > 0 {
                    self.selected_index = (self.selected_index + 1) % count;
                    cx.notify();
                }
            }
            "backspace" => {
                self.query.pop();
                self.selected_index = 0;
                cx.notify();
            }
            "space" => {
                self.query.push(' ');
                self.selected_index = 0;
                cx.notify();
            }
            s if s.chars().count() == 1 => {
                if let Some(c) = s.chars().next() {
                    if !c.is_control() {
                        self.query.push(c);
                        self.selected_index = 0;
                        cx.notify();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Render for TaskSwitcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let comp = self.compositor.read(cx);
        let apps = self.apps.read(cx);
        let all_windows = comp.windows();
        let q = self.query.trim().to_lowercase();
        let filtered: Vec<_> = all_windows
            .iter()
            .filter(|w| {
                if q.is_empty() {
                    true
                } else {
                    w.title.to_lowercase().contains(&q) || w.class.to_lowercase().contains(&q)
                }
            })
            .collect();

        let total = filtered.len();
        let selected = self.selected_index.min(total.saturating_sub(1));
        let max_visible = 6;
        let start_idx = if selected >= max_visible { selected - max_visible + 1 } else { 0 };
        let visible_items: Vec<_> =
            filtered.iter().enumerate().skip(start_idx).take(max_visible).collect();

        let card = div()
            .id("tasks-modal")
            .w(px(640.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .on_click(|_, _, _| {})
            .child(
                find_field("Windows", pal::yellow(), &self.query, "Search open windows").child(
                    div()
                        .px_2()
                        .py_0p5()
                        .bg(pal::white())
                        .border_1()
                        .border_color(pal::ink())
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(pal::ink())
                        .child(format!("{total}")),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .when(total == 0, |s| {
                        s.py_8()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(pal::ink())
                            .child("No open windows matching.")
                    })
                    .children(visible_items.into_iter().map(|(idx, &win)| {
                        let is_selected = idx == selected;
                        let con_id = win.id;
                        let comp_ref = self.compositor.clone();
                        let app_icon = apps.find_icon(&win.class);
                        let item_key = format!("task-sel-{}", win.id);

                        let icon_el: AnyElement = if let Some(img) = app_icon {
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::white())
                                .border_2()
                                .border_color(pal::ink())
                                .child(gpui::img(img).w(px(26.0)).h(px(26.0)))
                                .into_any_element()
                        } else {
                            let letter = win
                                .class
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string();
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(40.0))
                                .h(px(40.0))
                                .bg(pal::tint())
                                .border_2()
                                .border_color(pal::ink())
                                .text_size(px(18.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(letter)
                                .into_any_element()
                        };

                        let row = div()
                            .id(format!("task-item-{}", win.id))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .border_2()
                            .cursor_pointer()
                            .when(is_selected, |s| {
                                s.bg(pal::yellow())
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                            })
                            .when(!is_selected, |s| {
                                s.border_color(pal::clear()).hover(|h| h.bg(pal::tint()))
                            })
                            .child(
                                div()
                                    .w(px(12.0))
                                    .text_size(px(14.0))
                                    .text_color(pal::blue())
                                    .child(if is_selected { "\u{25B6}" } else { "" }),
                            )
                            .child(icon_el)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_size(px(14.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(pal::ink())
                                                    .truncate()
                                                    .child(win.title.clone()),
                                            )
                                            .when(win.is_focused, |this| {
                                                this.child(
                                                    div()
                                                        .px_1p5()
                                                        .bg(pal::pink())
                                                        .text_size(px(9.0))
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .text_color(pal::ink())
                                                        .child("active"),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_size(px(11.0))
                                            .text_color(pal::ink())
                                            .opacity(0.65)
                                            .child(format!("app: {}", win.class))
                                            .child("•")
                                            .child(format!("workspace {}", win.workspace_name)),
                                    ),
                            )
                            .on_click(move |_, _, cx| {
                                comp_ref.read(cx).focus_window(con_id);
                                cx.hide_window("tasks");
                            });

                        if is_selected {
                            row.with_animation(
                                item_key,
                                Animation::new(Duration::from_millis(200)),
                                |el, d| el.relative().left(px(-12.0 * (1.0 - out_cubic(d)))),
                            )
                            .into_any_element()
                        } else {
                            row.into_any_element()
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(key_chip("↑↓", "select"))
                    .child(key_chip("Enter", "switch"))
                    .child(key_chip("Esc", "close")),
            );

        div()
            .id("tasks-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(pal::veil())
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .pt(px(120.0))
            .on_click(|_, _, cx| {
                cx.hide_window("tasks");
            })
            .child(printed(card, "tasks", pal::blue()))
    }
}
