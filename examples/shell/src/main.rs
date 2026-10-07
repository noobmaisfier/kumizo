//! "Kumizo" — a Wayland shell dressed in risograph print style.
//!
//! Structure:
//! - `theme`: Flexoki palette, typography, Risograph aesthetics, animations, and shared helpers.
//! - `widgets/`: Independent modular components (rail, stamp, calendar, launcher, kanban, etc.).

pub mod theme;
pub mod widgets;

use gpui::prelude::*;
use widgetkit::prelude::*;
use widgets::{
    bluetooth, calendar, clipboard, kanban, launcher, lock, notifications, osd, powermenu, rail,
    stamp, tasks, tray,
};

fn main() {
    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .service::<Network>()
        .service::<Bluetooth>()
        .service::<Tray>()
        .service::<Audio>()
        .service::<Brightness>()
        .service::<Mpris>()
        .service::<Compositor>()
        .service::<Apps>()
        .service::<Notifications>()
        .service::<Clipboard>()
        .window(rail::window(), |cx: &mut gpui::App| cx.new(rail::Rail::new))
        .window(stamp::window(), |cx: &mut gpui::App| cx.new(stamp::Stamp::new))
        .window(osd::window(), |cx: &mut gpui::App| cx.new(osd::VolumeOsd::new))
        .window(tray::window(), |cx: &mut gpui::App| cx.new(tray::TrayMenu::new))
        .window(calendar::window(), |cx: &mut gpui::App| cx.new(calendar::Calendar::new))
        .window(bluetooth::window(), |cx: &mut gpui::App| cx.new(bluetooth::BluetoothMenu::new))
        .window(launcher::window(), |cx: &mut gpui::App| cx.new(launcher::AppLauncher::new))
        .window(powermenu::window(), |cx: &mut gpui::App| cx.new(powermenu::PowerMenu::new))
        .window(notifications::toasts_window(), |cx: &mut gpui::App| {
            cx.new(notifications::NotificationToasts::new)
        })
        .window(notifications::center_window(), |cx: &mut gpui::App| {
            cx.new(notifications::NotificationCenter::new)
        })
        .window(clipboard::window(), |cx: &mut gpui::App| {
            cx.new(clipboard::ClipboardManager::new)
        })
        .window(tasks::window(), |cx: &mut gpui::App| {
            cx.new(tasks::TaskSwitcher::new)
        })
        .window(kanban::window(), |cx: &mut gpui::App| {
            cx.new(kanban::KanbanScreen::new)
        })
        .window(lock::window(), |cx: &mut gpui::App| {
            cx.new(lock::LockScreen::new)
        })
        .run();
}
