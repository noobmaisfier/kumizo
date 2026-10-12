use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, FocusHandle, KeyDownEvent, px, Rgba, Window};
use kumizo_kit::prelude::*;

use crate::theme::{fullscreen, hard, key_chip, misprint, out_back, pal, staggered};

pub fn window() -> LayerWindow {
    fullscreen("powermenu", "wk-powermenu")
}

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

pub struct PowerMenu {
    focus: FocusHandle,
}

impl PowerMenu {
    pub fn new(cx: &mut Context<Self>) -> Self {
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
        if matches!(action, PowerAction::Lock) {
            cx.show_window("lock");
            cx.hide_window("powermenu");
            return;
        }
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
            if matches!(action, PowerAction::Lock) {
                cx.show_window("lock");
            } else {
                action.run();
            }
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
