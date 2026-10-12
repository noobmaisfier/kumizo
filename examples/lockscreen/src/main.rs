//! Fullscreen PAM lockscreen, running as a **separate locker process**
//! (same role as `swaylock`, but rendered with WidgetKit/GPUI).
//!
//! Trigger it, don't host it:
//!
//! ```sh
//! lockscreen                                  # manual (sway keybinding)
//! swayidle -w timeout 300 'lockscreen' \      # automatic (idle + lid)
//!   before-sleep 'lockscreen'
//! ```
//!
//! The password is really verified through PAM (`login` service). There is
//! deliberately **no IPC server** on this process: nothing may query or
//! unlock it remotely — `widgetctl` cannot reach it.
//!
//! Honest limitation (GPUI exposes no `ext-session-lock-v1`): Sway's own
//! keybindings stay live underneath, so this is a password gate, not an
//! eviction-proof seal. See `docs/REFERENTIEL.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, FocusHandle, KeyDownEvent, RenderImage, SharedString,
};
use kumizo_kit::prelude::*;

fn pam_service() -> &'static str {
    if std::path::Path::new("/etc/pam.d/swaylock").exists() {
        "swaylock"
    } else {
        "login"
    }
}

/// Default backdrop. Overridable with `WIDGETKIT_LOCK_WALLPAPER`.
const DEFAULT_WALLPAPER: &str = ".wallpaper/spark.jpg";
/// Max password length accepted into the buffer.
const MAX_PASSWORD_LEN: usize = 256;
/// Lock icon (Feather `lock`, stroke baked in — usvg has no `currentColor`).
const LOCK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#89b4fa" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>"##;

fn main() {
    WidgetKit::new()
        .ipc(false)
        .service::<Clock>()
        .service::<Battery>()
        .service::<Network>()
        .window(
            LayerWindow::new("lock")
                .layer(Layer::Overlay)
                .anchor(Anchor::FULLSCREEN)
                .exclusive_zone(-1)
                .keyboard_interactivity(KeyboardInteractivity::Exclusive)
                .transparent(false)
                .namespace("wk-lock"),
            |cx: &mut gpui::App| cx.new(LockScreen::new),
        )
        .run();
}

/// Login of the user who must authenticate (never root-forced: PAM checks
/// *this* user's password, which needs no privilege).
fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_string())
}

/// Backdrop path: `$WIDGETKIT_LOCK_WALLPAPER`, else `~/.wallpaper/wall-08.webp`.
fn wallpaper_path() -> PathBuf {
    if let Ok(custom) = std::env::var("WIDGETKIT_LOCK_WALLPAPER") {
        return PathBuf::from(custom);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(DEFAULT_WALLPAPER)
}

/// Blocking PAM check. Runs on the background executor, never on the UI
/// thread — `pam_authenticate` may converse with `unix_chkpwd` and block.
/// Implemented in the `wk-pam` crate (raw FFI, no bindgen/libclang).
fn verify_password(user: &str, password: &str) -> Result<(), wk_pam::AuthError> {
    wk_pam::verify(pam_service(), user, password)
}


/// Load the backdrop, shrink it hard, then blur: a 480px-wide gaussian is
/// milliseconds instead of seconds, and upscaled blur hides every pixel.
fn load_blurred_wallpaper(path: &Path) -> Option<Arc<RenderImage>> {
    let img = image::open(path).ok()?;
    let small = img.thumbnail(480, 270);
    let blurred = image::imageops::blur(&small.to_rgba8(), 8.0);
    let frame = image::Frame::new(blurred);
    Some(Arc::new(RenderImage::new(smallvec::smallvec![frame])))
}

/// Overwrite a password buffer before dropping it: `String::clear` would
/// leave the secret in freed memory.
fn zeroize(s: &mut String) {
    // Sound: the buffer is owned, exclusively borrowed, and no slice of it
    // is live while we scribble over it.
    unsafe {
        for b in s.as_bytes_mut() {
            *b = 0;
        }
    }
    s.clear();
}

struct LockScreen {
    focus: FocusHandle,
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    network: Entity<Network>,
    wallpaper: Option<Arc<RenderImage>>,
    username: String,
    password: String,
    busy: bool,
    unlocked: bool,
    error: Option<String>,
    fail_count: u32,
}

impl LockScreen {
    fn new(cx: &mut Context<Self>) -> Self {
        let clock = cx.service::<Clock>();
        let battery = cx.service::<Battery>();
        let network = cx.service::<Network>();

        // Repaint on every service tick (clock seconds, battery, wifi).
        let c = clock.clone();
        cx.observe(&c, |_, _, cx| cx.notify()).detach();
        let b = battery.clone();
        cx.observe(&b, |_, _, cx| cx.notify()).detach();
        let n = network.clone();
        cx.observe(&n, |_, _, cx| cx.notify()).detach();

        // Backdrop decodes off-thread; the solid base shows meanwhile.
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
            wallpaper: None,
            username,
            password: String::new(),
            busy: false,
            unlocked: false,
            error: None,
            fail_count: 0,
        }
    }

    /// Initial letter used as avatar: no `~/.face` on this machine, so a
    /// gradient disc with the letter beats a broken image promise.
    fn initial(&self) -> String {
        self.username
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string())
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.unlocked {
            return;
        }
        // TEMP-DIAG (retirer après diagnostic): nom des touches non
        // imprimables seulement — jamais le contenu des caractères.
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

        // Move the secret out now: the field is empty from this frame on,
        // and the worker owns (then wipes) the only copy.
        let mut secret = std::mem::take(&mut self.password);
        let user = self.username.clone();
        eprintln!("LOCK-SUBMIT len={}", secret.len()); // TEMP-DIAG
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let r = verify_password(&user, &secret);
                    zeroize(&mut secret);
                    r
                })
                .await;
            eprintln!("LOCK-AUTH {result:?}"); // TEMP-DIAG
            let updated = this
                .update(cx, |view: &mut LockScreen, cx| {
                    view.busy = false;
                    match &result {
                        Ok(()) => {
                            view.unlocked = true;
                            view.error = None;
                        }
                        Err(e) => {
                            // Screen stays vague (no user enumeration);
                            // the typed error goes to the log line above.
                            view.error = Some(e.user_message().to_string());
                            view.fail_count += 1;
                        }
                    }
                    cx.notify();
                })
                .is_ok();
            eprintln!("LOCK-UPDATE updated={updated}"); // TEMP-DIAG
            if result.is_ok() {
                // Brief "welcome" frame, then the locker is gone.
                async_io::Timer::after(Duration::from_millis(600)).await;
                eprintln!("LOCK-EXIT"); // TEMP-DIAG
                std::process::exit(0);
            }
        })
        .detach();
    }

    fn password_field(&self) -> AnyElement {
        let dots = "•".repeat(self.password.len());
        let text = if dots.is_empty() {
            "Password".to_string()
        } else {
            dots
        };
        let dimmed = self.password.is_empty();
        let failed = self.error.is_some();

        let field = div()
            .flex()
            .items_center()
            .justify_center()
            .w(px(320.0))
            .h(px(52.0))
            .rounded_full()
            .bg(rgba(0x1e1e2eff))
            .border_1()
            .border_color(if failed {
                rgba(0xf38ba8ff)
            } else if self.busy {
                rgba(0x585b7088)
            } else {
                rgba(0x45475aff)
            })
            .text_size(px(17.0))
            .text_color(if dimmed { rgb(0x585b70) } else { rgb(0xcdd6f4) })
            .child(text)
            .child(div().text_color(rgb(0x89b4fa)).child(if self.busy {
                "checking…"
            } else if self.unlocked {
                "welcome ✓"
            } else {
                ""
            }));

        // A fresh animation id per failure restarts the shake; no shake on
        // first paint (fail_count == 0 renders a plain field).
        if self.fail_count == 0 {
            field.into_any_element()
        } else {
            div()
                .with_animation(
                    SharedString::from(format!("shake-{}", self.fail_count)),
                    Animation::new(Duration::from_millis(450)),
                    |el, delta| {
                        let x = (delta * std::f32::consts::PI * 4.0).sin() * 12.0 * (1.0 - delta);
                        el.relative().left(px(x))
                    },
                )
                .child(field)
                .into_any_element()
        }
    }
}

impl Render for LockScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The locker owns the keyboard: grab focus back if anything stole it.
        // (This cannot defeat compositor bindings — see module docs.)
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let clock = self.clock.read(cx);
        let time = clock.time_hm();
        let date = clock.date_formatted();
        let battery = self.battery.read(cx);
        let pct = battery.percent();
        let bat_label = format!("{} {}%", battery.state().icon(pct), pct);
        let network = self.network.read(cx);
        let net_label = format!("{} {}", network.icon(), network.display_label());

        let backdrop = match &self.wallpaper {
            Some(img) => gpui::img(gpui::ImageSource::Render(img.clone()))
                .object_fit(gpui::ObjectFit::Cover)
                .w_full()
                .h_full()
                .into_any_element(),
            None => div().w_full().h_full().bg(rgb(0x11111b)).into_any_element(),
        };

        div()
            .w_full()
            .h_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(div().absolute().w_full().h_full().child(backdrop))
            // Dark veil: blurred wallpaper stays visible, text stays readable.
            .child(div().absolute().w_full().h_full().bg(rgba(0x11111b99)))
            .child(
                div()
                    .absolute()
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_5()
                    // Fade the whole card in on appear.
                    .with_animation(
                        "lock-appear",
                        Animation::new(Duration::from_millis(500)),
                        |el, delta| el.opacity(delta),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(84.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(0xcdd6f4))
                                    .child(time),
                            )
                            .child(
                                div()
                                    .text_size(px(20.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(date),
                            ),
                    )
                    // Avatar: breathing glow ring (opacity pulse) around the
                    // initial disc. The SVG lock badge sits on its edge.
                    .child(
                        div()
                            .relative()
                            .w(px(112.0))
                            .h(px(112.0))
                            .child(
                                div()
                                    .absolute()
                                    .w_full()
                                    .h_full()
                                    .rounded_full()
                                    .border_2()
                                    .border_color(rgba(0x89b4faff))
                                    .with_animation(
                                        "avatar-pulse",
                                        Animation::new(Duration::from_secs(2)).repeat(),
                                        |el, delta| {
                                            let t = (delta * std::f32::consts::PI * 2.0).sin()
                                                * 0.5
                                                + 0.5;
                                            el.opacity(0.35 + 0.35 * t)
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .w_full()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w(px(88.0))
                                            .h(px(88.0))
                                            .rounded_full()
                                            .bg(rgb(0x45475a))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(40.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xcdd6f4))
                                            .child(self.initial()),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom(px(2.0))
                                    .right(px(2.0))
                                    .w(px(30.0))
                                    .h(px(30.0))
                                    .rounded_full()
                                    .bg(rgb(0x1e1e2e))
                                    .border_1()
                                    .border_color(rgba(0x45475aff))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        gpui::svg()
                                            .data(LOCK_SVG.as_bytes())
                                            .w(px(15.0))
                                            .h(px(15.0)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(16.0))
                            .text_color(rgb(0xa6adc8))
                            .child(self.username.clone()),
                    )
                    .child(self.password_field())
                    .child(
                        div()
                            .h(px(22.0))
                            .text_size(px(14.0))
                            .text_color(rgb(0xf38ba8))
                            .child(self.error.clone().unwrap_or_else(|| {
                                if self.busy {
                                    String::new()
                                } else {
                                    "Enter ↵ to unlock · Esc clears".to_string()
                                }
                            })),
                    ),
            )
            // Status pills, dimmed: proof the machine is alive.
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .bottom(px(28.0))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .text_size(px(13.0))
                    .text_color(rgb(0x6c7086))
                    .child(bat_label)
                    .child(net_label),
            )
    }
}
