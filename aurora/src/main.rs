//! Aurora — full Wayland shell on Kumizo (single process, single GPU).
//!
//! Windows (ids prefixed `aurora-`, controllable via `kumizo toggle <id>`):
//!   aurora-bar       floating bar (always visible)
//!   aurora-launcher  app launcher            (fullscreen, exclusive keyboard)
//!   aurora-themes    theme selector          (fullscreen, exclusive keyboard)
//!   aurora-tasks     open windows            (fullscreen, exclusive keyboard)
//!   aurora-power     power menu              (fullscreen, exclusive keyboard)
//!   aurora-control   control center          (transparent fullscreen, panel on the right)
//!   aurora-notifs    notification history   (transparent fullscreen, panel on the right)
//!   aurora-toasts    notification popups     (top-right corner, opened by the bar)
//!   aurora-osd       volume / brightness     (bottom-center, opened by the bar)
//!
//! No music player: the shell displays and controls *remote* media
//! (browser, etc.) via MPRIS. A local player lives in its own
//! process (`kumizo-music`) and publishes itself via MPRIS as well.

mod bar;
mod clipboard;
mod control;
mod icons;
mod launcher;
mod notifs;
mod osd;
mod pomodoro;
mod power;
mod tasks;
mod theme;
mod theme_ctl;
mod theme_picker;
mod toasts;
mod ui;

use gpui::prelude::*;
use kumizo_kit::prelude::*;

use ui::ids;

/// Transparent/semi-opaque fullscreen surface above everything.
fn fullscreen(id: &'static str, namespace: &'static str, keyboard: KeyboardInteractivity) -> LayerWindow {
    LayerWindow::new(id)
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(keyboard)
        .transparent(true)
        .start_hidden(true)
        .namespace(namespace)
}

fn main() {
    theme::init();

    Kumizo::new()
        // ── Services (only those consumed by the views) ──────────────────────
        .service::<Clock>()
        .service::<Battery>()
        .service::<Memory>()
        .service::<Audio>()
        .service::<Brightness>()
        .service::<Bluetooth>()
        .service::<Network>()
        .service::<Compositor>()
        .service::<Apps>()
        .service::<Clipboard>()
        .service::<Mpris>()
        .service::<Tray>()
        .service::<Notifications>()
        .service::<theme_ctl::Theme>()
        // ── Bar ──────────────────────────────────────────────────────────────
        .window(
            LayerWindow::new(ids::BAR)
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(50)
                .size(None, Some(50.0))
                .keyboard_interactivity(KeyboardInteractivity::None)
                .transparent(true)
                .namespace("aurora-bar"),
            |cx: &mut gpui::App| cx.new(bar::TopBar::new),
        )
        // ── Side panels (transparent clickable background to close) ──────────
        .window(
            fullscreen(ids::CONTROL, "aurora-control", KeyboardInteractivity::OnDemand),
            |cx: &mut gpui::App| cx.new(control::ControlCenter::new),
        )
        .window(
            fullscreen(ids::NOTIFS, "aurora-notifs", KeyboardInteractivity::OnDemand),
            |cx: &mut gpui::App| cx.new(notifs::NotificationCenter::new),
        )
        // ── Fullscreen modals ────────────────────────────────────────────────
        .window(
            fullscreen(ids::LAUNCHER, "aurora-launcher", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(launcher::AppLauncher::new),
        )
        .window(
            fullscreen(ids::THEMES, "aurora-themes", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(theme_picker::ThemePicker::new),
        )
        .window(
            fullscreen(ids::TASKS, "aurora-tasks", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(tasks::TaskSwitcher::new),
        )
        .window(
            fullscreen(ids::POWER, "aurora-power", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(power::PowerMenu::new),
        )
        .window(
            fullscreen(ids::CLIPBOARD, "aurora-clipboard", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(clipboard::ClipboardPanel::new),
        )
        // ── Toasts (opened/closed by the bar according to `toasts()`) ───────
        .window(
            LayerWindow::new(ids::TOASTS)
                .layer(Layer::Overlay)
                .anchor(Anchor::TOP | Anchor::RIGHT)
                .exclusive_zone(0)
                .size(Some(380.0), Some(440.0))
                .margins(Margins::new(58, 12, 0, 0))
                .keyboard_interactivity(KeyboardInteractivity::None)
                .transparent(true)
                .start_hidden(true)
                .namespace("aurora-toasts"),
            |cx: &mut gpui::App| cx.new(toasts::NotificationToasts::new),
        )
        // ── Volume / brightness OSD (opened by the bar) ─────────────────────
        .window(
            LayerWindow::new(ids::OSD)
                .layer(Layer::Overlay)
                .anchor(Anchor::BOTTOM)
                .exclusive_zone(0)
                .size(Some(300.0), Some(64.0))
                .margins(Margins::new(0, 0, 90, 0))
                .keyboard_interactivity(KeyboardInteractivity::None)
                .transparent(true)
                .start_hidden(true)
                .namespace("aurora-osd"),
            |cx: &mut gpui::App| cx.new(osd::Osd::new),
        )
        .run();
}
