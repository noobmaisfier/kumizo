use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{rgba, Animation, AnimationExt, Context, Entity, FocusHandle, KeyDownEvent, px, RenderImage, SharedString, Window};
use kumizo_kit::prelude::*;

use crate::theme::{fullscreen, hard, key_chip, misprint, pal};

pub fn window() -> LayerWindow {
    fullscreen("lock", "wk-lock")
}

const MAX_PASSWORD_LEN: usize = 256;

fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

fn wallpaper_path() -> PathBuf {
    if let Ok(custom) = std::env::var("WIDGETKIT_LOCK_WALLPAPER") {
        return PathBuf::from(custom);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".wallpaper/spark.jpg")
}

fn load_blurred_wallpaper(path: &Path) -> Option<Arc<RenderImage>> {
    let img = image::open(path).ok()?;
    let small = img.thumbnail(480, 270);
    let blurred = image::imageops::blur(&small.to_rgba8(), 8.0);
    let frame = image::Frame::new(blurred);
    Some(Arc::new(RenderImage::new(smallvec::smallvec![frame])))
}

fn zeroize(s: &mut String) {
    unsafe {
        for b in s.as_bytes_mut() {
            *b = 0;
        }
    }
    s.clear();
}

pub struct LockScreen {
    focus: FocusHandle,
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    network: Entity<Network>,
    mpris: Entity<Mpris>,
    wallpaper: Option<Arc<RenderImage>>,
    username: String,
    password: String,
    busy: bool,
    unlocked: bool,
    error: Option<String>,
    fail_count: u32,
}

impl LockScreen {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let clock = cx.service::<Clock>();
        let battery = cx.service::<Battery>();
        let network = cx.service::<Network>();
        let mpris = cx.service::<Mpris>();

        let c = clock.clone();
        cx.observe(&c, |_, _, cx| cx.notify()).detach();
        let b = battery.clone();
        cx.observe(&b, |_, _, cx| cx.notify()).detach();
        let n = network.clone();
        cx.observe(&n, |_, _, cx| cx.notify()).detach();
        let m = mpris.clone();
        cx.observe(&m, |_, _, cx| cx.notify()).detach();

        let path = wallpaper_path();
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move { load_blurred_wallpaper(&path) })
                .await;
            let _ = this.update(cx, |view: &mut LockScreen, cx| {
                view.wallpaper = img;
                cx.notify();
            });
        })
        .detach();

        let username = username();
        LockScreen {
            focus: cx.focus_handle(),
            clock,
            battery,
            network,
            mpris,
            wallpaper: None,
            username,
            password: String::new(),
            busy: false,
            unlocked: false,
            error: None,
            fail_count: 0,
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.unlocked {
            return;
        }

        // Quick power actions on Alt+Key
        if event.keystroke.modifiers.alt {
            match event.keystroke.key.as_str() {
                "s" => {
                    let _ = std::process::Command::new("systemctl").arg("suspend").spawn();
                    return;
                }
                "r" => {
                    let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
                    return;
                }
                "p" => {
                    let _ = std::process::Command::new("systemctl").arg("poweroff").spawn();
                    return;
                }
                _ => {}
            }
        }

        let key = event.keystroke.key.as_str();
        match key {
            "backspace" => {
                self.password.pop();
                self.error = None;
                cx.notify();
            }
            "enter" => self.submit(cx),
            "escape" => {
                zeroize(&mut self.password);
                self.error = None;
                cx.notify();
            }
            _ => {
                if let Some(ref text) = event.keystroke.key_char {
                    for c in text.chars() {
                        if !c.is_control() {
                            self.push_char(c, cx);
                        }
                    }
                } else if key == "space" {
                    self.push_char(' ', cx);
                } else if key.chars().count() == 1 {
                    if let Some(c) = key.chars().next() {
                        if !c.is_control() {
                            self.push_char(c, cx);
                        }
                    }
                }
            }
        }
    }

    fn push_char(&mut self, c: char, cx: &mut Context<Self>) {
        if self.password.len() < MAX_PASSWORD_LEN {
            self.password.push(c);
            self.error = None;
            cx.notify();
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.unlocked || self.password.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();

        let mut secret = std::mem::take(&mut self.password);
        let user = self.username.clone();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let service = if std::path::Path::new("/etc/pam.d/swaylock").exists() {
                        "swaylock"
                    } else {
                        "login"
                    };
                    let r = wk_pam::verify(service, &user, &secret);
                    zeroize(&mut secret);
                    r
                })
                .await;

            let _ = this.update(cx, |view: &mut LockScreen, cx| {
                view.busy = false;
                match &result {
                    Ok(()) => {
                        view.unlocked = true;
                        view.error = None;
                    }
                    Err(e) => {
                        eprintln!("LOCK-AUTH failed for user {}: {e:?}", view.username);
                        view.error = Some(e.user_message().to_string());
                        view.fail_count += 1;
                    }
                }
                cx.notify();
            });

            if result.is_ok() {
                async_io::Timer::after(Duration::from_millis(350)).await;
                let _ = this.update(cx, |view: &mut LockScreen, cx| {
                    view.unlocked = false;
                    zeroize(&mut view.password);
                    cx.hide_window("lock");
                });
            }
        })
        .detach();
    }
}

impl Render for LockScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let clock = self.clock.read(cx);
        let time = format!("{}", clock.time_hm());
        let date = format!("{}", clock.date_formatted());

        let battery = self.battery.read(cx);
        let pct = battery.percent();
        let bat_charging = battery.is_charging();
        let bat_low = !bat_charging && pct <= 20;

        let network = self.network.read(cx);
        let net_connected = network.is_connected();

        let mpris = self.mpris.read(cx);
        let track_title = mpris.title().map(|s| s.to_string());
        let track_artist = mpris.artist().map(|s| s.to_string());

        let count = self.password.len();

        let backdrop = match &self.wallpaper {
            Some(img) => gpui::img(gpui::ImageSource::Render(img.clone()))
                .object_fit(gpui::ObjectFit::Cover)
                .w_full()
                .h_full()
                .into_any_element(),
            None => div().size_full().bg(pal::paper()).into_any_element(),
        };

        let auth_card = div()
            .w(px(440.0))
            .p_6()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 6.0))
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(pal::yellow())
                                    .border_2()
                                    .border_color(pal::ink())
                                    .text_size(px(10.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("AUTH"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(format!("USER: {}", self.username.to_uppercase())),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(pal::gray())
                            .child("\u{f023} LOCKED"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_4()
                    .h(px(54.0))
                    .bg(pal::white())
                    .border_2()
                    .border_color(if self.error.is_some() {
                        pal::pink()
                    } else if self.busy {
                        pal::blue()
                    } else {
                        pal::ink()
                    })
                    .shadow(hard(
                        if self.error.is_some() {
                            pal::pink()
                        } else {
                            pal::ink()
                        },
                        3.0,
                    ))
                    .child(if count == 0 {
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .w_full()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(pal::gray())
                                    .child("Type password to unlock…"),
                            )
                            .child(
                                div()
                                    .w(px(2.0))
                                    .h(px(18.0))
                                    .bg(pal::blue()),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .overflow_hidden()
                            .children((0..count.min(20)).map(|i| {
                                div()
                                    .id(format!("ink-tile-{}", i))
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .bg(pal::ink())
                                    .border_1()
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::blue(), 1.5))
                            }))
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_h(px(20.0))
                    .child(if self.busy {
                        misprint("VERIFYING CREDENTIALS…".to_string(), 12.0, pal::blue(), pal::pink()).into_any_element()
                    } else if self.unlocked {
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::blue())
                            .child("ACCESS GRANTED ✓")
                            .into_any_element()
                    } else if let Some(ref err) = self.error {
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::pink())
                            .child(err.to_uppercase())
                            .into_any_element()
                    } else {
                        div()
                            .text_size(px(11.0))
                            .text_color(pal::gray())
                            .child("Press Enter to verify · Esc to clear")
                            .into_any_element()
                    }),
            );

        let animated_card = if self.fail_count == 0 {
            auth_card.into_any_element()
        } else {
            div()
                .with_animation(
                    SharedString::from(format!("shake-{}", self.fail_count)),
                    Animation::new(Duration::from_millis(450)),
                    |el, delta| {
                        let x = (delta * std::f32::consts::PI * 4.0).sin() * 14.0 * (1.0 - delta);
                        el.relative().left(px(x))
                    },
                )
                .child(auth_card)
                .into_any_element()
        };

        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .items_center()
            .p_10()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(backdrop)
                    .child(div().absolute().inset_0().bg(rgba(0xfffcf0e8))),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1p5()
                            .bg(pal::white())
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 3.0))
                            .child(div().text_size(px(13.0)).text_color(pal::ink()).child("\u{f007}"))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(format!("{}@archlinux", self.username)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1p5()
                                    .bg(if bat_charging { pal::yellow() } else if bat_low { pal::pink() } else { pal::white() })
                                    .border_2()
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                                    .text_color(pal::ink())
                                    .child(div().text_size(px(13.0)).child(if bat_charging { "\u{f0e7}" } else { "\u{f240}" }))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(format!("{}%", pct)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1p5()
                                    .bg(if net_connected { pal::white() } else { pal::pink() })
                                    .border_2()
                                    .border_color(pal::ink())
                                    .shadow(hard(pal::ink(), 3.0))
                                    .text_color(pal::ink())
                                    .child(div().text_size(px(13.0)).child(if net_connected { "\u{f1eb}" } else { "\u{f071}" }))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(network.display_label()),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .child(misprint(time, 88.0, pal::blue(), pal::pink()))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(date),
                            ),
                    )
                    .when(track_title.is_some(), |parent| {
                        let title = track_title.unwrap_or_default();
                        let artist = track_artist.unwrap_or_else(|| "Unknown".into());
                        parent.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_4()
                                .py_2()
                                .bg(pal::white())
                                .border_2()
                                .border_color(pal::ink())
                                .shadow(hard(pal::ink(), 4.0))
                                .child(div().text_size(px(14.0)).text_color(pal::blue()).child("\u{f001}"))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .text_size(px(12.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(pal::ink())
                                        .child(format!("{title} — {artist}")),
                                    ),
                            )
                    })
                    .child(animated_card),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .id("lock-suspend")
                            .cursor_pointer()
                            .child(key_chip("Alt+S", "Suspend"))
                            .on_click(|_, _, _| {
                                let _ = std::process::Command::new("systemctl").arg("suspend").spawn();
                            }),
                    )
                    .child(
                        div()
                            .id("lock-reboot")
                            .cursor_pointer()
                            .child(key_chip("Alt+R", "Reboot"))
                            .on_click(|_, _, _| {
                                let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
                            }),
                    )
                    .child(
                        div()
                            .id("lock-poweroff")
                            .cursor_pointer()
                            .child(key_chip("Alt+P", "Power off"))
                            .on_click(|_, _, _| {
                                let _ = std::process::Command::new("systemctl").arg("poweroff").spawn();
                            }),
                    ),
            )
    }
}
