//! Menu d'alimentation plein écran.
//!
//! Clavier : ←/→ ou Tab pour choisir, Entrée pour valider, L/O/S/R/P en
//! raccourci direct, Échap pour annuler. Les actions destructrices
//! (déconnexion, redémarrage, extinction) demandent une 2ᵉ validation.

use gpui::prelude::*;
use gpui::{div, px, ClickEvent, Context, FocusHandle, FontWeight, KeyDownEvent, Window};
use widgetkit::prelude::*;

use crate::{
    icons,
    theme::pal,
    ui::{self, ids},
};

#[derive(Clone, Copy)]
enum Action {
    Lock,
    Logout,
    Suspend,
    Reboot,
    Shutdown,
}

struct Entry {
    action: Action,
    glyph: &'static str,
    label: &'static str,
    key: &'static str,
    confirm: bool,
}

const ENTRIES: [Entry; 5] = [
    Entry { action: Action::Lock, glyph: icons::LOCK, label: "Verrouiller", key: "l", confirm: false },
    Entry { action: Action::Logout, glyph: icons::LOGOUT, label: "Déconnexion", key: "o", confirm: true },
    Entry { action: Action::Suspend, glyph: icons::MOON, label: "Veille", key: "s", confirm: false },
    Entry { action: Action::Reboot, glyph: icons::REBOOT, label: "Redémarrer", key: "r", confirm: true },
    Entry { action: Action::Shutdown, glyph: icons::POWER, label: "Éteindre", key: "p", confirm: true },
];

fn logout() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        ui::run("hyprctl", &["dispatch", "exit"]);
    } else if std::env::var_os("SWAYSOCK").is_some() {
        ui::run("swaymsg", &["exit"]);
    } else if let Ok(session) = std::env::var("XDG_SESSION_ID") {
        ui::run("loginctl", &["terminate-session", &session]);
    }
}

fn perform(action: Action) {
    match action {
        // Binaire séparé, fourni par ce projet (cf. src/bin/lockscreen.rs)
        Action::Lock => ui::run("aurora-lockscreen", &[]),
        Action::Logout => logout(),
        Action::Suspend => ui::run("systemctl", &["suspend"]),
        Action::Reboot => ui::run("systemctl", &["reboot"]),
        Action::Shutdown => ui::run("systemctl", &["poweroff"]),
    }
}

pub struct PowerMenu {
    selected: usize,
    armed: Option<usize>,
    focus: FocusHandle,
}

impl PowerMenu {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            selected: 0,
            armed: None,
            focus: cx.focus_handle(),
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.selected = 0;
        self.armed = None;
        cx.hide_window(ids::POWER);
        cx.notify();
    }

    fn activate(&mut self, i: usize, cx: &mut Context<Self>) {
        let entry = &ENTRIES[i];
        self.selected = i;
        if entry.confirm && self.armed != Some(i) {
            self.armed = Some(i);
            cx.notify();
            return;
        }
        let action = entry.action;
        self.close(cx);
        perform(action);
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let n = ENTRIES.len();
        match ui::key(ev) {
            "escape" => {
                if self.armed.is_some() {
                    self.armed = None;
                    cx.notify();
                } else {
                    self.close(cx);
                }
            }
            "left" => {
                self.selected = (self.selected + n - 1) % n;
                self.armed = None;
                cx.notify();
            }
            "right" | "tab" => {
                self.selected = (self.selected + 1) % n;
                self.armed = None;
                cx.notify();
            }
            "enter" => self.activate(self.selected, cx),
            other => {
                if let Some(i) = ENTRIES.iter().position(|e| e.key == other) {
                    self.activate(i, cx);
                }
            }
        }
    }
}

impl Render for PowerMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let mut row = div().flex().flex_row().gap_4();
        for (i, e) in ENTRIES.iter().enumerate() {
            let selected = i == self.selected;
            let armed = self.armed == Some(i);
            let fg = if armed { pal::on_accent() } else { pal::text() };
            row = row.child(
                div()
                    .id(("pw", i))
                    .w(px(136.))
                    .h(px(136.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .rounded_2xl()
                    .cursor_pointer()
                    .bg(if armed {
                        pal::danger()
                    } else if selected {
                        pal::raised()
                    } else {
                        pal::surface()
                    })
                    .border_2()
                    .border_color(if armed {
                        pal::danger()
                    } else if selected {
                        pal::accent()
                    } else {
                        pal::border()
                    })
                    .hover(|s| s.opacity(0.9))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.activate(i, cx);
                    }))
                    .child(ui::glyph(
                        e.glyph,
                        34.,
                        if armed {
                            pal::on_accent()
                        } else if matches!(e.action, Action::Shutdown) {
                            pal::danger()
                        } else {
                            pal::accent()
                        },
                    ))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(if armed { "Confirmer ?" } else { e.label }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if armed { pal::on_accent() } else { pal::muted() })
                            .child(format!("[{}]", e.key.to_uppercase())),
                    ),
            );
        }

        ui::backdrop("pw-bg", ids::POWER, 0.72)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .occlude()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_6()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(pal::text())
                            .child("Session"),
                    )
                    .child(row)
                    .child(
                        div()
                            .text_xs()
                            .text_color(pal::muted())
                            .child("←/→ choisir · Entrée valider · Échap annuler"),
                    ),
            )
    }
}
