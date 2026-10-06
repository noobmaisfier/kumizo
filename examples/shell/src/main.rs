//! A single-process Wayland shell hosting the topbar, the dock and the OSD.
//!
//! Running the three widgets as three separate binaries works, but each one
//! pays the full cost of initialising the GPU stack, the font stack and the
//! compositor connection. In a single process that cost is paid once, and the
//! services (`Clock`, `Audio`, `Compositor`, ...) are instantiated once and
//! shared by every window, which also means one wakeup per second instead of one
//! per binary.
//!
//! The trade-off is the loss of isolation: a panic in any window takes down all
//! of them. That is the same trade-off Waybar and AGS make, and it is the right
//! one for a desktop shell.
//!
//! Run it with:
//!
//! ```sh
//! cargo run --package shell-example
//! ```

use std::time::Duration;

use gpui::prelude::*;
use gpui::{AnyElement, FocusHandle, KeyDownEvent};
use widgetkit::prelude::*;

/// Height of the top bar, in logical pixels. Also used as its exclusive zone so
/// regular windows are pushed below it.
const TOPBAR_HEIGHT: f32 = 36.0;
/// Height of the dock.
const DOCK_HEIGHT: f32 = 58.0;
/// Size of the volume/brightness OSD.
const OSD_WIDTH: f32 = 280.0;
const OSD_HEIGHT: f32 = 80.0;
/// Size of the calendar popup.
const CALENDAR_WIDTH: f32 = 300.0;
const CALENDAR_HEIGHT: f32 = 290.0;

fn main() {
    WidgetKit::new()
        // Services are declared once and shared by all windows: the clock
        // ticks once per second for the whole shell, not once per widget.
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
        .window(topbar_window(), |cx: &mut gpui::App| {
            cx.new(TopBar::new)
        })
        .window(dock_window(), |cx: &mut gpui::App| cx.new(Dock::new))
        .window(osd_window(), |cx: &mut gpui::App| cx.new(VolumeOsd::new))
        .window(traymenu_window(), |cx: &mut gpui::App| cx.new(TrayMenu::new))
        .window(calendar_window(), |cx: &mut gpui::App| cx.new(Calendar::new))
        .window(launcher_window(), |cx: &mut gpui::App| cx.new(AppLauncher::new))
        .window(powermenu_window(), |cx: &mut gpui::App| cx.new(PowerMenu::new))
        .window(notifications_window(), |cx: &mut gpui::App| cx.new(NotificationToasts::new))
        .window(notification_center_window(), |cx: &mut gpui::App| cx.new(NotificationCenter::new))
        .window(clipboard_window(), |cx: &mut gpui::App| cx.new(ClipboardManager::new))
        .run();
}

fn topbar_window() -> LayerWindow {
    LayerWindow::new("topbar")
        .layer(Layer::Top)
        .anchor(Anchor::TOP_BAR)
        // Reserving the bar's height keeps maximised windows from overlapping it.
        .exclusive_zone(TOPBAR_HEIGHT as i32)
        .height(TOPBAR_HEIGHT)
        .margins(Margins::new(6, 12, 0, 12))
        .namespace("wk-topbar")
}

fn dock_window() -> LayerWindow {
    LayerWindow::new("dock")
        .layer(Layer::Top)
        .anchor(Anchor::BOTTOM)
        .exclusive_zone(DOCK_HEIGHT as i32)
        .width(480.0)
        .height(DOCK_HEIGHT)
        .margins(Margins::new(0, 0, 10, 0))
        .namespace("wk-dock")
}

fn osd_window() -> LayerWindow {
    LayerWindow::new("osd")
        .layer(Layer::Overlay)
        .anchor(Anchor::BOTTOM)
        // No exclusive zone: the OSD must never reserve screen space.
        .exclusive_zone(0)
        .width(OSD_WIDTH)
        .height(OSD_HEIGHT)
        .margins(Margins::new(0, 0, 100, 0))
        .namespace("wk-osd")
}

/// Context menu of a tray icon. Starts with no surface (`start_hidden`) so
/// it never reserves input area while closed; the `Tray` service shows and
/// hides it through `WindowControl` as menus open and close.
fn traymenu_window() -> LayerWindow {
    LayerWindow::new("traymenu")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(300.0)
        .height(420.0)
        .margins(Margins::new(44, 12, 0, 0))
        .namespace("wk-traymenu")
        .start_hidden(true)
}

/// Month calendar popup, toggled in-memory by a click on the clock
/// (`cx.toggle_window("calendar")`). Like the tray menu it has no surface
/// while closed.
fn calendar_window() -> LayerWindow {
    LayerWindow::new("calendar")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP)
        .exclusive_zone(0)
        .width(CALENDAR_WIDTH)
        .height(CALENDAR_HEIGHT)
        .margins(Margins::new(44, 0, 0, 0))
        .namespace("wk-calendar")
        .start_hidden(true)
}

/// Fullscreen overlay application launcher (Rofi/Spotlight style).
fn launcher_window() -> LayerWindow {
    LayerWindow::new("launcher")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(0)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .size(None, None)
        .namespace("wk-launcher")
        .start_hidden(true)
}

/// Fullscreen overlay power menu (Lock, Suspend, Logout, Reboot, Shutdown).
fn powermenu_window() -> LayerWindow {
    LayerWindow::new("powermenu")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(0)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .size(None, None)
        .namespace("wk-powermenu")
        .start_hidden(true)
}

/// Ephemeral notification toasts displayed in the top-right corner.
fn notifications_window() -> LayerWindow {
    LayerWindow::new("notifications")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(360.0)
        .height(600.0)
        .margins(Margins::new(44, 12, 0, 0))
        .namespace("wk-notifications")
        .start_hidden(true)
}

/// Notification history center / drawer toggled by the bell icon.
fn notification_center_window() -> LayerWindow {
    LayerWindow::new("notification_center")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::RIGHT)
        .exclusive_zone(0)
        .width(380.0)
        .height(520.0)
        .margins(Margins::new(44, 12, 0, 0))
        .namespace("wk-notification-center")
        .start_hidden(true)
}

/// Fullscreen overlay clipboard manager (history, fuzzy search, pasteback).
fn clipboard_window() -> LayerWindow {
    LayerWindow::new("clipboard")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(0)
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .size(None, None)
        .namespace("wk-clipboard")
        .start_hidden(true)
}

// ---------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------

struct TopBar {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    network: Entity<Network>,
    tray: Entity<Tray>,
    audio: Entity<Audio>,
    mpris: Entity<Mpris>,
    compositor: Entity<Compositor>,
    notifications: Entity<Notifications>,
}

impl TopBar {
    fn new(cx: &mut Context<Self>) -> Self {
        let tray = cx.service::<Tray>();
        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            network: cx.service::<Network>(),
            tray,
            audio: cx.service::<Audio>(),
            mpris: cx.service::<Mpris>(),
            compositor: cx.service::<Compositor>(),
            notifications: cx.service::<Notifications>(),
        }
    }
}

impl Render for TopBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let battery = self.battery.read(cx);
        let network = self.network.read(cx);
        let tray = self.tray.read(cx);
        let audio = self.audio.read(cx);
        let mpris = self.mpris.read(cx);
        let comp = self.compositor.read(cx);
        let notifs = self.notifications.read(cx);
        let unread = notifs.unread_count();
        let dnd = notifs.is_dnd();

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(TOPBAR_HEIGHT))
            .bg(rgba(0x181825ee))
            .border_1()
            .border_color(rgba(0x31324488))
            .rounded_lg()
            .px_3()
            .text_size(px(13.0))
            .text_color(rgb(0xcdd6f4))
            // App Launcher and Workspaces
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("wk-launcher-btn")
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .cursor_pointer()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .text_color(rgb(0x89b4fa))
                            .child("\u{f00a}")
                            .on_click(|_, _window, cx| cx.toggle_window("launcher")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .bg(rgb(0x1e1e2e))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .children(comp.workspaces().iter().map(|ws| {
                                let is_active = ws.is_active;
                                let ws_id = ws.id;
                                let name = ws.name.clone();
                                let compositor = self.compositor.clone();

                                div()
                                    .id(format!("workspace-{}", ws_id))
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .when(is_active, |s| {
                                        s.bg(rgb(0x89b4fa))
                                            .text_color(rgb(0x11111b))
                                            .font_weight(gpui::FontWeight::BOLD)
                                    })
                                    .when(!is_active, |s| s.hover(|h| h.bg(rgb(0x313244))))
                                    .child(name)
                                    .on_click(move |_, _window, cx| {
                                        compositor.update(cx, |c, _| c.switch_workspace(ws_id));
                                    })
                            })),
                    ),
            )
            // Date and time. Click toggles the calendar popup, in memory.
            .child(
                div()
                    .id("wk-clock")
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(0x1e1e2e))
                    .hover(|s| s.bg(rgb(0x313244)))
                    .cursor_pointer()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .on_click(|_, _window, cx| cx.toggle_window("calendar"))
                    .child(div().text_color(rgb(0xb4befe)).child(clock.date_formatted()))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0xcdd6f4))
                            .child(clock.time_hms()),
                    ),
            )
            // MPRIS, tray, volume and battery.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(mpris.title().is_some(), |this| {
                        let mpris_entity = self.mpris.clone();
                        let status_icon = mpris.status().icon();
                        let label = mpris.display_label();

                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .bg(rgb(0x1e1e2e))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .child(div().text_color(rgb(0xf5c2e7)).child("\u{f00e}"))
                                .child(
                                    div()
                                        .text_color(rgb(0xcdd6f4))
                                        .max_w(px(MPRIS_LABEL_MAX_WIDTH))
                                        .truncate()
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .id("wk-mpris-toggle")
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(0x89b4fa)))
                                        .child(status_icon)
                                        .on_click(move |_, _window, cx| {
                                            mpris_entity.read(cx).play_pause();
                                        }),
                                ),
                        )
                    })
                    // Tray icons: left-click activates, right-click opens the menu.
                    .children(tray.items().into_iter().map(|item| {
                        let activate_id = item.id.clone();
                        let menu_id = item.id.clone();
                        let tray_entity = self.tray.clone();
                        let tray_entity2 = self.tray.clone();
                        let icon = item.icon.clone();
                        let title = item.title.clone();

                        div()
                            .id(format!("tray-{}", item.id))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|h| h.bg(rgb(0x313244)))
                            .when(icon.is_some(), |this| {
                                this.child(
                                    gpui::img(icon.clone().unwrap())
                                        .w(px(20.0))
                                        .h(px(20.0)),
                                )
                            })
                            .when(icon.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_color(rgb(0xf38ba8))
                                        .child(title.chars().next().map(|c| c.to_string()).unwrap_or_else(|| "?".to_string())),
                                )
                            })
                            .on_click(move |_, _, cx| {
                                tray_entity.update(cx, |t, _| t.activate(&activate_id));
                            })
                            .on_mouse_down(gpui::MouseButton::Right, move |_, _, cx| {
                                tray_entity2.update(cx, |t, _| t.open_menu(&menu_id));
                            })
                    }))
                    .child({
                        let audio_entity = self.audio.clone();
                        let icon = audio.icon();
                        let volume = audio.volume();
                        let muted = audio.is_muted();

                        div()
                            .id("wk-audio-toggle")
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .cursor_pointer()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .child(
                                div()
                                    .text_color(if muted {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0x89b4fa)
                                    })
                                    .child(icon),
                            )
                            .child(format!("{}%", volume))
                            .on_click(move |_, _window, cx| {
                                audio_entity.update(cx, |a, _| a.toggle_mute());
                            })
                    })
                    .child({
                        let connected = network.is_connected();

                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(0x1e1e2e))
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .child(
                                div()
                                    .text_color(if connected {
                                        rgb(0x89b4fa)
                                    } else {
                                        rgb(0xf38ba8)
                                    })
                                    .child(network.icon()),
                            )
                            .child(network.display_label())
                    })
                    .child({
                        let percent = battery.percent();
                        let charging = battery.is_charging();

                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(0x1e1e2e))
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .child(
                                div()
                                    .text_color(if charging {
                                        rgb(0xa6e3a1)
                                    } else if percent <= 20 {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0xf9e2af)
                                    })
                                    .child(battery.state().icon(percent)),
                            )
                            .child(format!("{}%", percent))
                    })
                    .child(
                        div()
                            .id("wk-notif-bell")
                            .flex()
                            .items_center()
                            .gap_1()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .cursor_pointer()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .text_color(if dnd {
                                rgb(0x6c7086)
                            } else if unread > 0 {
                                rgb(0xf9e2af)
                            } else {
                                rgb(0x89b4fa)
                            })
                            .child(if dnd { "\u{f1f6}" } else { "\u{f0f3}" })
                            .when(unread > 0, |this| {
                                this.child(
                                    div()
                                        .bg(rgb(0xf38ba8))
                                        .text_color(rgb(0x11111b))
                                        .text_size(px(10.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_full()
                                        .child(format!("{unread}")),
                                )
                            })
                            .on_click(|_, _window, cx| cx.toggle_window("notification_center")),
                    )
                    .child(
                        div()
                            .id("wk-power-btn")
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .cursor_pointer()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .text_color(rgb(0xf38ba8))
                            .child("\u{f011}")
                            .on_click(|_, _window, cx| cx.toggle_window("powermenu")),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// Tray context menu
// ---------------------------------------------------------------------------

/// Max width of the media title in the topbar, in logical pixels.
///
/// The bar's right block is laid out after the left one, so an unbounded
/// title pushes the battery and the wifi pill off the screen. Truncation is
/// done by the layout (`truncate()`), not by slicing the string: slicing by
/// byte index splits UTF-8 (accents, emoji) and would panic or corrupt it.
const MPRIS_LABEL_MAX_WIDTH: f32 = 260.0;

/// Context menu of a tray icon, rendered in the `traymenu` window.
///
/// The window has no surface while closed (`start_hidden`); the whole window
/// is a click-catcher that closes the menu, so no surface lingers.
struct TrayMenu {
    tray: Entity<Tray>,
}

impl TrayMenu {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            tray: cx.service::<Tray>(),
        }
    }
}

/// Render menu entries recursively. Submenus appear expanded and indented;
/// separators become hairlines; disabled rows are dimmed and inert.
fn render_menu_items(
    items: &[TrayMenuItem],
    tray: &Entity<Tray>,
) -> Vec<gpui::AnyElement> {
    let mut out = Vec::new();
    for item in items {
        if item.separator {
            out.push(
                div()
                    .h(px(1.0))
                    .my_1()
                    .bg(rgb(0x313244))
                    .w_full()
                    .into_any_element(),
            );
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
            .rounded_md()
            .text_color(if item.enabled {
                rgb(0xcdd6f4)
            } else {
                rgb(0x6c7086)
            });
        if item.enabled {
            let tray_entity = tray.clone();
            row = row
                .hover(|h| h.bg(rgb(0x313244)))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    tray_entity.update(cx, |t, _| t.click_menu_item(id));
                });
        }
        let check = match item.toggle {
            Some(true) => "✓ ",
            Some(false) => "○ ",
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
            .on_click(move |_, _, cx| {
                tray_entity.update(cx, |t, _| t.close_menu());
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .bg(rgb(0x1e1e2e))
                    .border_1()
                    .border_color(rgba(0x31324488))
                    .rounded_lg()
                    .p_2()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .text_color(rgb(0x89b4fa))
                            .child(menu.title.clone()),
                    )
                    .child(div().h(px(1.0)).my_1().bg(rgb(0x313244)).w_full())
                    .children(render_menu_items(&menu.items, &self.tray)),
            )
    }
}

// ---------------------------------------------------------------------------
// Calendar popup
// ---------------------------------------------------------------------------

struct Calendar {
    clock: Entity<Clock>,
}

impl Calendar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
        }
    }
}

impl Render for Calendar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let dt = clock.datetime();
        let year = dt.year();
        let month = dt.month();
        let today = dt.day();

        let month_name = match month {
            time::Month::January => "January",
            time::Month::February => "February",
            time::Month::March => "March",
            time::Month::April => "April",
            time::Month::May => "May",
            time::Month::June => "June",
            time::Month::July => "July",
            time::Month::August => "August",
            time::Month::September => "September",
            time::Month::October => "October",
            time::Month::November => "November",
            time::Month::December => "December",
        };

        let days_in_month = time::util::days_in_month(month, year);
        let first_day_weekday = time::Date::from_calendar_date(year, month, 1)
            .map(|d| d.weekday().number_from_monday())
            .unwrap_or(1);

        let weekdays = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

        div()
            .id("calendar-backdrop")
            .w_full()
            .h_full()
            .on_click(|_, _, cx| {
                cx.hide_window("calendar");
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .bg(rgb(0x1e1e2e))
                    .border_1()
                    .border_color(rgba(0x31324488))
                    .rounded_xl()
                    .p_3()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_2()
                            .py_1()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(0x89b4fa))
                                    .text_size(px(15.0))
                                    .child(format!("{month_name} {year}")),
                            )
                            .child(
                                div()
                                    .id("calendar-close-btn")
                                    .cursor_pointer()
                                    .text_color(rgb(0x6c7086))
                                    .hover(|s| s.text_color(rgb(0xf38ba8)))
                                    .child("✕")
                                    .on_click(|_, _, cx| {
                                        cx.hide_window("calendar");
                                    }),
                            ),
                    )
                    .child(div().h(px(1.0)).bg(rgb(0x313244)).w_full())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .px_1()
                            .children(weekdays.iter().map(|day| {
                                div()
                                    .w(px(34.0))
                                    .flex()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xa6adc8))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(*day)
                            })),
                    )
                    .child({
                        let leading_blanks = (first_day_weekday - 1) as usize;
                        let total_slots = leading_blanks + days_in_month as usize;
                        let rows = (total_slots + 6) / 7;

                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children((0..rows).map(|row| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .px_1()
                                    .children((0..7).map(|col| {
                                        let slot_idx = row * 7 + col;
                                        if slot_idx < leading_blanks || slot_idx >= total_slots {
                                            div().w(px(34.0)).h(px(28.0))
                                        } else {
                                            let day_num = (slot_idx - leading_blanks + 1) as u8;
                                            let is_today = day_num == today;
                                            div()
                                                .w(px(34.0))
                                                .h(px(28.0))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded_md()
                                                .text_size(px(13.0))
                                                .when(is_today, |s| {
                                                    s.bg(rgb(0x89b4fa))
                                                        .text_color(rgb(0x11111b))
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                })
                                                .when(!is_today, |s| {
                                                    s.text_color(rgb(0xcdd6f4))
                                                        .hover(|h| h.bg(rgb(0x313244)))
                                                })
                                                .child(format!("{day_num}"))
                                        }
                                    }))
                            }))
                    }),
            )
    }
}

// ---------------------------------------------------------------------------
// Dock
// ---------------------------------------------------------------------------

struct DockItem {
    icon: &'static str,
    command: &'static str,
    /// Lowercase app_id/class fragment used to detect a running indicator.
    app_class: &'static str,
}

struct Dock {
    items: Vec<DockItem>,
    compositor: Entity<Compositor>,
}

impl Dock {
    fn new(cx: &mut Context<Self>) -> Self {
        let items = vec![
            DockItem { icon: "\u{f1209}", command: "ghostty", app_class: "ghostty" },
            DockItem { icon: "\u{f0219}", command: "firefox", app_class: "firefox" },
            DockItem { icon: "\u{f129e}", command: "zed", app_class: "dev.zed.zed" },
            DockItem { icon: "\u{f024b}", command: "nautilus", app_class: "org.gnome.nautilus" },
            DockItem { icon: "\u{f0243}", command: "spotify", app_class: "spotify" },
            DockItem { icon: "\u{f0266}", command: "discord", app_class: "discord" },
        ];

        Self {
            items,
            compositor: cx.service::<Compositor>(),
        }
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_class = self
            .compositor
            .read(cx)
            .active_window()
            .map(|w| w.class.to_lowercase())
            .unwrap_or_default();

        div()
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(rgba(0x181825ee))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .child(
                        div()
                            .id("dock-launcher")
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .w(px(44.0))
                            .h(px(44.0))
                            .rounded_xl()
                            .cursor_pointer()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .text_size(px(20.0))
                            .text_color(rgb(0x89b4fa))
                            .child("\u{f00a}")
                            .on_click(|_, _window, cx| cx.toggle_window("launcher")),
                    )
                    .child(
                        div()
                            .w(px(1.0))
                            .h(px(24.0))
                            .bg(rgb(0x313244))
                            .mx_1(),
                    )
                    .children(self.items.iter().map(|item| {
                        let running = !active_class.is_empty()
                            && active_class.contains(&item.app_class);
                        let command = item.command;

                        div()
                            .id(format!("dock-{}", command))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .w(px(44.0))
                            .h(px(44.0))
                            .rounded_xl()
                            .cursor_pointer()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .child(
                                div()
                                    .text_size(px(22.0))
                                    .text_color(rgb(0xcdd6f4))
                                    .child(item.icon),
                            )
                            // Running indicator dot.
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(if running {
                                        rgb(0x89b4fa)
                                    } else {
                                        rgba(0x00000000)
                                    }),
                            )
                            .on_click(move |_, _window, _cx| {
                                let _ = std::process::Command::new(command).spawn();
                            })
                    })),
            )
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
}

/// What the OSD currently displays. Volume and brightness share one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OsdKind {
    Volume,
    Brightness,
}

/// Reveal the OSD for `kind` and reset its 2-second auto-hide timer.
///
/// Each adjustment increments `generation`. Any previous timer that wakes
/// up sees an outdated generation and does nothing; only the latest timer
/// (2s after the *last* user adjustment) will dismiss the OSD.
fn reveal_osd(osd: &mut VolumeOsd, cx: &mut Context<VolumeOsd>, kind: OsdKind) {
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
        };

        // React to volume AND mute changes: mute alone used to stay silent
        // because only the level was compared.
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

        // Brightness shares the same surface, shown with its own label.
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

impl Render for VolumeOsd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            // Nothing to show: stay transparent instead of burning frames.
            return div().w_full().h_full();
        }

        let (icon, label, pct, accent) = match self.shown {
            OsdKind::Volume => {
                let audio = self.audio.read(cx);
                let muted = audio.is_muted();
                (
                    audio.icon(),
                    if muted { "Muted" } else { "Volume" },
                    audio.volume(),
                    if muted { rgb(0xf38ba8) } else { rgb(0x89b4fa) },
                )
            }
            OsdKind::Brightness => {
                let brightness = self.brightness.read(cx);
                (
                    brightness.icon(),
                    "Brightness",
                    brightness.percent(),
                    rgb(0x89b4fa),
                )
            }
        };

        div()
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .w(px(OSD_WIDTH - 20.0))
                    .p_3()
                    .bg(rgba(0x181825f2))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .shadow_xl()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .w_full()
                            .px_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_color(accent)
                                    .text_size(px(18.0))
                                    .child(icon)
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xcdd6f4))
                                            .child(label),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(format!("{}%", pct)),
                            ),
                    )
                    // Progress bar.
                    .child(
                        div()
                            .w_full()
                            .h(px(8.0))
                            .bg(rgb(0x313244))
                            .rounded_full()
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::DefiniteLength::Fraction(pct as f32 / 100.0))
                                    .bg(accent)
                                    .rounded_full(),
                            ),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// Application Launcher (Rofi / Spotlight style)
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
                    if self.selected_index == 0 {
                        self.selected_index = count - 1;
                    } else {
                        self.selected_index -= 1;
                    }
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
        let start_idx = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let visible_items: Vec<_> = matches
            .iter()
            .enumerate()
            .skip(start_idx)
            .take(max_visible)
            .collect();

        div()
            .id("launcher-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(rgba(0x11111b88))
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .pt(px(140.0))
            .on_click(|_, _, cx| {
                cx.hide_window("launcher");
            })
            .child(
                div()
                    .id("launcher-modal")
                    .w(px(580.0))
                    .bg(rgb(0x1e1e2e))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .shadow_2xl()
                    .p_4()
                    .gap_3()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, _| {})
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .bg(rgb(0x181825))
                            .border_1()
                            .border_color(rgba(0x31324488))
                            .rounded_xl()
                            .px_3()
                            .py_2p5()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .text_color(rgb(0x89b4fa))
                                    .child("\u{f002}"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_1()
                                    .text_size(px(15.0))
                                    .when(self.query.is_empty(), |s| {
                                        s.text_color(rgb(0x6c7086))
                                            .child("Search applications...")
                                    })
                                    .when(!self.query.is_empty(), |s| {
                                        s.text_color(rgb(0xcdd6f4))
                                            .child(self.query.clone())
                                            .child(
                                                div()
                                                    .text_color(rgb(0x89b4fa))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .child("|"),
                                            )
                                    }),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgb(0x313244))
                                    .text_size(px(11.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(format!("{total} apps")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .when(total == 0, |s| {
                                s.py_6()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .text_color(rgb(0x6c7086))
                                    .child("No matching applications")
                            })
                            .children(visible_items.into_iter().map(|(idx, app)| {
                                let is_selected = idx == selected;
                                let app_item = app.clone();
                                let apps_ref = apps_service.clone();

                                let icon_el: AnyElement = if let Some(img) = &app.icon {
                                    gpui::img(img.clone())
                                        .w(px(32.0))
                                        .h(px(32.0))
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
                                        .w(px(32.0))
                                        .h(px(32.0))
                                        .rounded_lg()
                                        .bg(rgb(0x313244))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(16.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(0x89b4fa))
                                        .child(letter)
                                        .into_any_element()
                                };

                                let comment = if app.comment.is_empty() {
                                    app.exec.clone()
                                } else {
                                    app.comment.clone()
                                };

                                div()
                                    .id(format!("launcher-item-{}", app.id))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .p_2()
                                    .rounded_xl()
                                    .cursor_pointer()
                                    .when(is_selected, |s| {
                                        s.bg(rgb(0x313244))
                                            .border_1()
                                            .border_color(rgba(0x89b4fa66))
                                    })
                                    .when(!is_selected, |s| {
                                        s.hover(|h| h.bg(rgb(0x181825)))
                                    })
                                    .child(icon_el)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .child(
                                                div()
                                                    .text_size(px(14.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(if is_selected {
                                                        rgb(0x89b4fa)
                                                    } else {
                                                        rgb(0xcdd6f4)
                                                    })
                                                    .child(app.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.0))
                                                    .text_color(rgb(0x6c7086))
                                                    .truncate()
                                                    .child(comment),
                                            ),
                                    )
                                    .when(is_selected, |s| {
                                        s.child(
                                            div()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(rgb(0x1e1e2e))
                                                .text_size(px(10.0))
                                                .text_color(rgb(0x89b4fa))
                                                .child("↵ Enter"),
                                        )
                                    })
                                    .on_click(move |_, _, cx| {
                                        apps_ref.read(cx).launch(&app_item);
                                        cx.hide_window("launcher");
                                    })
                            })),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// Power Menu (Session actions: Lock, Suspend, Logout, Reboot, Shutdown)
// ---------------------------------------------------------------------------

struct PowerMenu {
    focus: FocusHandle,
}

impl PowerMenu {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }

    fn action_lock(&self, cx: &mut Context<Self>) {
        let _ = std::process::Command::new("loginctl").arg("lock-session").spawn();
        cx.hide_window("powermenu");
    }

    fn action_suspend(&self, cx: &mut Context<Self>) {
        let _ = std::process::Command::new("systemctl").arg("suspend").spawn();
        cx.hide_window("powermenu");
    }

    fn action_logout(&self, cx: &mut Context<Self>) {
        let _ = std::process::Command::new("swaymsg").arg("exit").spawn();
        let _ = std::process::Command::new("loginctl").args(["terminate-session", ""]).spawn();
        cx.hide_window("powermenu");
    }

    fn action_reboot(&self, cx: &mut Context<Self>) {
        let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
        cx.hide_window("powermenu");
    }

    fn action_poweroff(&self, cx: &mut Context<Self>) {
        let _ = std::process::Command::new("systemctl").arg("poweroff").spawn();
        cx.hide_window("powermenu");
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" | "q" => cx.hide_window("powermenu"),
            "l" => self.action_lock(cx),
            "s" => self.action_suspend(cx),
            "e" => self.action_logout(cx),
            "r" => self.action_reboot(cx),
            "p" => self.action_poweroff(cx),
            _ => {}
        }
    }
}

impl Render for PowerMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let user = std::env::var("USER").unwrap_or_else(|_| "User".to_string());

        div()
            .id("powermenu-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(rgba(0x11111bd0))
            .flex()
            .items_center()
            .justify_center()
            .on_click(|_, _, cx| {
                cx.hide_window("powermenu");
            })
            .child(
                div()
                    .id("powermenu-card")
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_6()
                    .p_8()
                    .bg(rgb(0x1e1e2e))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .shadow_2xl()
                    .on_click(|_, _, _| {})
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(22.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(0xcdd6f4))
                                    .child(format!("Goodbye, {user}")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(rgb(0x6c7086))
                                    .child("Choose an action or press shortcut key"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .id("pw-lock")
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .w(px(88.0))
                                    .h(px(96.0))
                                    .rounded_xl()
                                    .bg(rgb(0x181825))
                                    .border_1()
                                    .border_color(rgba(0x31324488))
                                    .hover(|h| h.bg(rgb(0x313244)).border_color(rgb(0x89b4fa)))
                                    .cursor_pointer()
                                    .text_color(rgb(0x89b4fa))
                                    .child(div().text_size(px(28.0)).child("\u{f023}"))
                                    .child(div().text_size(px(12.0)).font_weight(gpui::FontWeight::BOLD).child("Lock"))
                                    .child(div().text_size(px(10.0)).text_color(rgb(0x6c7086)).child("[L]"))
                                    .on_click(|_, _, cx| {
                                        let _ = std::process::Command::new("loginctl").arg("lock-session").spawn();
                                        cx.hide_window("powermenu");
                                    }),
                            )
                            .child(
                                div()
                                    .id("pw-suspend")
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .w(px(88.0))
                                    .h(px(96.0))
                                    .rounded_xl()
                                    .bg(rgb(0x181825))
                                    .border_1()
                                    .border_color(rgba(0x31324488))
                                    .hover(|h| h.bg(rgb(0x313244)).border_color(rgb(0xa6e3a1)))
                                    .cursor_pointer()
                                    .text_color(rgb(0xa6e3a1))
                                    .child(div().text_size(px(28.0)).child("\u{f186}"))
                                    .child(div().text_size(px(12.0)).font_weight(gpui::FontWeight::BOLD).child("Suspend"))
                                    .child(div().text_size(px(10.0)).text_color(rgb(0x6c7086)).child("[S]"))
                                    .on_click(|_, _, cx| {
                                        let _ = std::process::Command::new("systemctl").arg("suspend").spawn();
                                        cx.hide_window("powermenu");
                                    }),
                            )
                            .child(
                                div()
                                    .id("pw-logout")
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .w(px(88.0))
                                    .h(px(96.0))
                                    .rounded_xl()
                                    .bg(rgb(0x181825))
                                    .border_1()
                                    .border_color(rgba(0x31324488))
                                    .hover(|h| h.bg(rgb(0x313244)).border_color(rgb(0xf9e2af)))
                                    .cursor_pointer()
                                    .text_color(rgb(0xf9e2af))
                                    .child(div().text_size(px(28.0)).child("\u{f2f5}"))
                                    .child(div().text_size(px(12.0)).font_weight(gpui::FontWeight::BOLD).child("Logout"))
                                    .child(div().text_size(px(10.0)).text_color(rgb(0x6c7086)).child("[E]"))
                                    .on_click(|_, _, cx| {
                                        let _ = std::process::Command::new("swaymsg").arg("exit").spawn();
                                        let _ = std::process::Command::new("loginctl").args(["terminate-session", ""]).spawn();
                                        cx.hide_window("powermenu");
                                    }),
                            )
                            .child(
                                div()
                                    .id("pw-reboot")
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .w(px(88.0))
                                    .h(px(96.0))
                                    .rounded_xl()
                                    .bg(rgb(0x181825))
                                    .border_1()
                                    .border_color(rgba(0x31324488))
                                    .hover(|h| h.bg(rgb(0x313244)).border_color(rgb(0xfab387)))
                                    .cursor_pointer()
                                    .text_color(rgb(0xfab387))
                                    .child(div().text_size(px(28.0)).child("\u{f021}"))
                                    .child(div().text_size(px(12.0)).font_weight(gpui::FontWeight::BOLD).child("Reboot"))
                                    .child(div().text_size(px(10.0)).text_color(rgb(0x6c7086)).child("[R]"))
                                    .on_click(|_, _, cx| {
                                        let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
                                        cx.hide_window("powermenu");
                                    }),
                            )
                            .child(
                                div()
                                    .id("pw-shutdown")
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .w(px(88.0))
                                    .h(px(96.0))
                                    .rounded_xl()
                                    .bg(rgb(0x181825))
                                    .border_1()
                                    .border_color(rgba(0x31324488))
                                    .hover(|h| h.bg(rgb(0x313244)).border_color(rgb(0xf38ba8)))
                                    .cursor_pointer()
                                    .text_color(rgb(0xf38ba8))
                                    .child(div().text_size(px(28.0)).child("\u{f011}"))
                                    .child(div().text_size(px(12.0)).font_weight(gpui::FontWeight::BOLD).child("Shutdown"))
                                    .child(div().text_size(px(10.0)).text_color(rgb(0x6c7086)).child("[P]"))
                                    .on_click(|_, _, cx| {
                                        let _ = std::process::Command::new("systemctl").arg("poweroff").spawn();
                                        cx.hide_window("powermenu");
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(0x585b70))
                            .child("Press Esc or click backdrop to cancel"),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// Notification Toasts (Ephemeral popups in top-right)
// ---------------------------------------------------------------------------

struct NotificationToasts {
    notifications: Entity<Notifications>,
}

impl NotificationToasts {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            notifications: cx.service::<Notifications>(),
        }
    }
}

impl Render for NotificationToasts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notifs = self.notifications.read(cx);
        let toasts = notifs.toasts().to_vec();

        div()
            .flex()
            .flex_col()
            .gap_2p5()
            .w_full()
            .children(toasts.into_iter().map(|notif| {
                let notif_id = notif.id;
                let notifs_entity = self.notifications.clone();
                let notifs_entity_close = self.notifications.clone();
                let urgency_color = match notif.urgency {
                    Urgency::Critical => rgb(0xf38ba8),
                    Urgency::Low => rgb(0x6c7086),
                    Urgency::Normal => rgb(0x89b4fa),
                };

                div()
                    .id(format!("toast-{}", notif.id))
                    .flex()
                    .flex_row()
                    .w_full()
                    .rounded_xl()
                    .bg(rgba(0x181825f2))
                    .border_1()
                    .border_color(rgba(0x313244cc))
                    .shadow_lg()
                    // Urgency accent left bar
                    .child(
                        div()
                            .w(px(4.0))
                            .rounded_l_xl()
                            .bg(urgency_color),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap_1p5()
                            .p_3()
                            // Header: App icon / name + summary + close button
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
                                    .when(notif.app_icon.is_some(), |this| {
                                        this.child(
                                            gpui::img(notif.app_icon.clone().unwrap())
                                                .w(px(18.0))
                                                .h(px(18.0))
                                                .rounded_sm(),
                                        )
                                    })
                                    .when(notif.app_icon.is_none(), |this| {
                                        this.child(
                                            div()
                                                .text_color(rgb(0x89b4fa))
                                                .text_size(px(14.0))
                                                .child("\u{f0f3}"),
                                        )
                                    })
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xa6adc8))
                                            .truncate()
                                            .child(if notif.app_name.is_empty() {
                                                "Notification".to_string()
                                            } else {
                                                notif.app_name.clone()
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .id(format!("toast-close-{}", notif_id))
                                    .cursor_pointer()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_md()
                                    .text_color(rgb(0x6c7086))
                                    .hover(|h| h.bg(rgb(0x313244)).text_color(rgb(0xf38ba8)))
                                    .child("✕")
                                    .on_click(move |_, _, cx| {
                                        notifs_entity_close.update(cx, |n, _| n.dismiss_toast(notif_id));
                                    }),
                            ),
                    )
                    // Summary (title)
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0xcdd6f4))
                            .child(notif.summary.clone()),
                    )
                    // Body (optional)
                    .when(!notif.body.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(0xa6adc8))
                                .line_height(px(16.0))
                                .child(notif.body.clone()),
                        )
                    })
                    // Action buttons (if any)
                    .when(!notif.actions.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .pt_1()
                                .children(notif.actions.into_iter().map(|action| {
                                    let notifs_act = notifs_entity.clone();
                                    let act_key = action.key.clone();
                                    div()
                                        .id(format!("toast-act-{}-{}", notif_id, action.key))
                                        .cursor_pointer()
                                        .px_2p5()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x313244))
                                        .hover(|h| h.bg(rgb(0x45475a)))
                                        .text_size(px(11.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(0xcdd6f4))
                                        .child(action.label)
                                        .on_click(move |_, _, cx| {
                                            notifs_act.update(cx, |n, _| {
                                                n.invoke_action(notif_id, &act_key);
                                                n.dismiss_toast(notif_id);
                                            });
                                        })
                                })),
                        )
                    }),
                    )
            }))
    }
}

// ---------------------------------------------------------------------------
// Notification Center (Drawer toggled by topbar bell icon)
// ---------------------------------------------------------------------------

struct NotificationCenter {
    notifications: Entity<Notifications>,
}

impl NotificationCenter {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            notifications: cx.service::<Notifications>(),
        }
    }
}

impl Render for NotificationCenter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notifs = self.notifications.read(cx);
        let history = notifs.history().to_vec();
        let dnd = notifs.is_dnd();
        let notifs_entity = self.notifications.clone();
        let notifs_entity2 = self.notifications.clone();

        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(rgba(0x181825f8))
            .border_1()
            .border_color(rgba(0x313244aa))
            .rounded_2xl()
            .p_4()
            .gap_3()
            .shadow_xl()
            // Header: Title + DND button + Clear All button + Close button
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .border_color(rgba(0x31324488))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(0xcdd6f4))
                                    .child("Notifications"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(0x6c7086))
                                    .child(format!("({})", history.len())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            // DND Toggle
                            .child(
                                div()
                                    .id("nc-toggle-dnd")
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_2p5()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .when(dnd, |s| s.bg(rgb(0xf38ba8)).text_color(rgb(0x11111b)))
                                    .when(!dnd, |s| {
                                        s.bg(rgb(0x313244))
                                            .text_color(rgb(0xa6adc8))
                                            .hover(|h| h.bg(rgb(0x45475a)))
                                    })
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(if dnd { "\u{f1f6} DND On" } else { "\u{f0f3} DND Off" })
                                    .on_click(move |_, _, cx| {
                                        notifs_entity.update(cx, |n, _| n.toggle_dnd());
                                    }),
                            )
                            // Clear All button
                            .when(!history.is_empty(), |this| {
                                this.child(
                                    div()
                                        .id("nc-clear-all")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(rgb(0x313244))
                                        .hover(|h| h.bg(rgb(0x45475a)).text_color(rgb(0xf38ba8)))
                                        .cursor_pointer()
                                        .text_size(px(11.0))
                                        .text_color(rgb(0xa6adc8))
                                        .child("Clear")
                                        .on_click(move |_, _, cx| {
                                            notifs_entity2.update(cx, |n, _| n.clear_history());
                                        }),
                                )
                            })
                            // Close drawer button
                            .child(
                                div()
                                    .id("nc-close")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|h| h.bg(rgb(0x313244)))
                                    .cursor_pointer()
                                    .text_size(px(13.0))
                                    .text_color(rgb(0x6c7086))
                                    .child("✕")
                                    .on_click(|_, _, cx| cx.hide_window("notification_center")),
                            ),
                    ),
            )
            // Body: List or Empty placeholder
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_2()
                    .overflow_hidden()
                    .when(history.is_empty(), |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .h_full()
                                .gap_2()
                                .py_12()
                                .child(
                                    div()
                                        .text_size(px(32.0))
                                        .text_color(rgb(0x45475a))
                                        .child("\u{f0f3}"),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .text_color(rgb(0x6c7086))
                                        .child("No notifications"),
                                ),
                        )
                    })
                    .children(history.into_iter().take(10).map(|notif| {
                        let notif_id = notif.id;
                        let notifs_remove = self.notifications.clone();
                        let urgency_color = match notif.urgency {
                            Urgency::Critical => rgb(0xf38ba8),
                            Urgency::Low => rgb(0x6c7086),
                            Urgency::Normal => rgb(0x89b4fa),
                        };

                        div()
                            .id(format!("hist-{}", notif.id))
                            .flex()
                            .flex_row()
                            .rounded_xl()
                            .bg(rgb(0x1e1e2e))
                            .border_1()
                            .border_color(rgba(0x31324466))
                            .hover(|h| h.bg(rgb(0x242438)))
                            .child(
                                div()
                                    .w(px(3.0))
                                    .rounded_l_xl()
                                    .bg(urgency_color),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .gap_1()
                                    .p_3()
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
                                            .when(notif.app_icon.is_some(), |this| {
                                                this.child(
                                                    gpui::img(notif.app_icon.clone().unwrap())
                                                        .w(px(16.0))
                                                        .h(px(16.0))
                                                        .rounded_sm(),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .text_size(px(11.0))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(rgb(0xa6adc8))
                                                    .truncate()
                                                    .child(if notif.app_name.is_empty() {
                                                        "System".to_string()
                                                    } else {
                                                        notif.app_name.clone()
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id(format!("hist-del-{}", notif_id))
                                            .cursor_pointer()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_md()
                                            .text_color(rgb(0x585b70))
                                            .hover(|h| h.bg(rgb(0x313244)).text_color(rgb(0xf38ba8)))
                                            .child("✕")
                                            .on_click(move |_, _, cx| {
                                                notifs_remove.update(cx, |n, _| n.remove_history(notif_id));
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xcdd6f4))
                                    .child(notif.summary.clone()),
                            )
                            .when(!notif.body.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(rgb(0xa6adc8))
                                        .line_height(px(16.0))
                                        .child(notif.body.clone()),
                                )
                            }),
                            )
                    })),
            )
    }
}

// ---------------------------------------------------------------------------
// Clipboard Manager (Spotlight style popup modal)
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
                    let item = &matches[idx];
                    let item_id = item.id;
                    self.clipboard.update(cx, |c, _| c.copy_item(item_id));
                    self.query.clear();
                    self.selected_index = 0;
                    cx.hide_window("clipboard");
                }
            }
            "delete" => {
                if !matches.is_empty() {
                    let idx = self.selected_index.min(count.saturating_sub(1));
                    let item = &matches[idx];
                    let item_id = item.id;
                    self.clipboard.update(cx, |c, _| c.remove_item(item_id));
                    if self.selected_index >= count.saturating_sub(1) {
                        self.selected_index = self.selected_index.saturating_sub(1);
                    }
                    cx.notify();
                }
            }
            "up" => {
                if count > 0 {
                    if self.selected_index == 0 {
                        self.selected_index = count - 1;
                    } else {
                        self.selected_index -= 1;
                    }
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
        let start_idx = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        let visible_items: Vec<_> = matches
            .iter()
            .enumerate()
            .skip(start_idx)
            .take(max_visible)
            .collect();

        div()
            .id("clipboard-backdrop")
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .bg(rgba(0x11111b88))
            .flex()
            .flex_col()
            .items_center()
            .justify_start()
            .pt(px(140.0))
            .on_click(|_, _, cx| {
                cx.hide_window("clipboard");
            })
            .child(
                div()
                    .id("clipboard-modal")
                    .w(px(580.0))
                    .bg(rgb(0x1e1e2e))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .shadow_2xl()
                    .p_4()
                    .gap_3()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, _| {})
                    // Header: Search bar + items count + clear button
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .bg(rgb(0x181825))
                            .border_1()
                            .border_color(rgba(0x31324488))
                            .rounded_xl()
                            .px_3()
                            .py_2p5()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .text_color(rgb(0x89b4fa))
                                    .child("\u{f0ea}"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_1()
                                    .text_size(px(15.0))
                                    .when(self.query.is_empty(), |s| {
                                        s.text_color(rgb(0x6c7086))
                                            .child("Search clipboard history...")
                                    })
                                    .when(!self.query.is_empty(), |s| {
                                        s.text_color(rgb(0xcdd6f4))
                                            .child(self.query.clone())
                                            .child(
                                                div()
                                                    .text_color(rgb(0x89b4fa))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .child("|"),
                                            )
                                    }),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgb(0x313244))
                                    .text_size(px(11.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(format!("{total} items")),
                            )
                            .when(total > 0, |this| {
                                this.child(
                                    div()
                                        .id("clip-clear-btn")
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(rgb(0x313244))
                                        .hover(|h| h.bg(rgb(0x45475a)).text_color(rgb(0xf38ba8)))
                                        .cursor_pointer()
                                        .text_size(px(11.0))
                                        .text_color(rgb(0xa6adc8))
                                        .child("Clear")
                                        .on_click(move |_, _, cx| {
                                            clipboard_clear.update(cx, |c, _| c.clear());
                                        }),
                                )
                            }),
                    )
                    // Results list
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .when(total == 0, |s| {
                                s.py_6()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .text_color(rgb(0x6c7086))
                                    .child("No clipboard history")
                            })
                            .children(visible_items.into_iter().map(|(idx, item)| {
                                let is_selected = idx == selected;
                                let item_id = item.id;
                                let clip_ref = clipboard_service.clone();
                                let clip_del = clipboard_service.clone();
                                let preview = item.preview();
                                let lines = item.line_count();
                                let chars = item.char_count();

                                div()
                                    .id(format!("clip-item-{}", item.id))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .p_2p5()
                                    .rounded_xl()
                                    .cursor_pointer()
                                    .when(is_selected, |s| {
                                        s.bg(rgb(0x313244))
                                            .border_1()
                                            .border_color(rgba(0x89b4fa66))
                                    })
                                    .when(!is_selected, |s| {
                                        s.hover(|h| h.bg(rgb(0x181825)))
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .text_size(px(13.0))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(rgb(0xcdd6f4))
                                                    .truncate()
                                                    .child(preview),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .text_size(px(10.0))
                                                    .text_color(rgb(0x6c7086))
                                                    .child(format!("{lines} line{}", if lines > 1 { "s" } else { "" }))
                                                    .child("•")
                                                    .child(format!("{chars} chars")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id(format!("clip-del-{}", item_id))
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_md()
                                            .text_color(rgb(0x585b70))
                                            .hover(|h| h.bg(rgb(0x45475a)).text_color(rgb(0xf38ba8)))
                                            .child("✕")
                                            .on_click(move |_, _, cx| {
                                                clip_del.update(cx, |c, _| c.remove_item(item_id));
                                            }),
                                    )
                                    .on_click(move |_, _, cx| {
                                        clip_ref.update(cx, |c, _| c.copy_item(item_id));
                                        cx.hide_window("clipboard");
                                    })
                            })),
                    )
                    // Footer hint
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt_2()
                            .border_color(rgba(0x31324488))
                            .text_size(px(11.0))
                            .text_color(rgb(0x6c7086))
                            .child("Enter to copy • Del to delete • Esc to close")
                            .child(format!("{total} total entries")),
                    ),
            )
    }
}