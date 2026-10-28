//! aurora-lockscreen — SEPARATE binary from the shell (cf. §11 of the Kumizo guide).
//!
//! * `.ipc(false)` : no socket, no remote control, no conflict with the shell.
//! * Fullscreen `Overlay` + `Exclusive` keyboard.
//! * Password verified by PAM (service `login`) on the background executor.
//! * No code or service shared with the shell: the mini-palette and the clock
//!   are deliberately duplicated.
//!
//! ⚠ Design limit: this is a layer-shell surface, NOT the `ext-session-lock`
//! protocol. A compositor may still react to system shortcuts (TTY switch,
//! etc.). For strict "security" locking, prefer a locker based on
//! ext-session-lock.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    div, linear_color_stop, linear_gradient, px, Context, Entity, FocusHandle, FontFeatures,
    FontWeight, KeyDownEvent, ObjectFit, RenderImage, Rgba, Window,
};
use image::{Frame, ImageBuffer};
use kumizo_kit::prelude::*;
use smallvec::smallvec;

// ── Mini-palette (duplicated, reads the theme chosen in the shell) ─────────────
//                 name                bg        text      muted     accent    danger
const PALETTES: [(&str, u32, u32, u32, u32, u32); 11] = [
    ("Aurora",           0x0b0e14, 0xe6edf7, 0x6b7794, 0x00e8c6, 0xff5c7a),
    ("Flexoki Light",    0xfffcf0, 0x100f0f, 0x6f6e69, 0x205ea6, 0xaf3029),
    ("Catppuccin Mocha", 0x1e1e2e, 0xcdd6f4, 0x7f849c, 0xf5c2e7, 0xf38ba8),
    ("Catppuccin Latte", 0xeff1f5, 0x4c4f69, 0x7c7f93, 0x8839ef, 0xd20f39),
    ("Tokyo Night",      0x1a1b26, 0xc0caf5, 0x737aa2, 0x7aa2f7, 0xf7768e),
    ("Gruvbox",          0x282828, 0xebdbb2, 0xa89984, 0xfabd2f, 0xfb4934),
    ("Nord",             0x2e3440, 0xeceff4, 0x9aa5be, 0x88c0d0, 0xbf616a),
    ("Rose Pine",        0x191724, 0xe0def4, 0x908caa, 0xc4a7e7, 0xeb6f92),
    ("Dracula",          0x282a36, 0xf8f8f2, 0x8a93bd, 0xbd93f9, 0xff5555),
    ("Everforest",       0x2d353b, 0xd3c6aa, 0x9da9a0, 0xa7c080, 0xe67e80),
    ("GitHub Light",     0xffffff, 0x1f2328, 0x59636e, 0x0969da, 0xd1242c),
];

struct Colors {
    bg: Rgba,
    text: Rgba,
    muted: Rgba,
    accent: Rgba,
    danger: Rgba,
}

fn colors() -> Colors {
    let saved = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/aurora/theme"))
        .and_then(|p| std::fs::read_to_string(p).ok());
    let p = saved
        .as_deref()
        .and_then(|s| PALETTES.iter().find(|p| p.0 == s.trim()))
        .unwrap_or(&PALETTES[0]);
    Colors {
        bg: gpui::rgb(p.1),
        text: gpui::rgb(p.2),
        muted: gpui::rgb(p.3),
        accent: gpui::rgb(p.4),
        danger: gpui::rgb(p.5),
    }
}

/// Tabular figures, like the bar clock: minutes no longer make the block width
/// dance.
fn clock_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))
}

/// Current wallpaper, fetched from awww (same pool as the applied theme).
/// We extend PATH by hand: the lockscreen is launched by sway before
/// `~/.local/bin` is necessarily present in the environment.
fn wallpaper_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut dirs = vec![
        PathBuf::from(&home).join(".local/bin"),
        PathBuf::from(&home).join(".cargo/bin"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let path_env = std::env::join_paths(dirs).ok()?;

    let out = Command::new("awww")
        .arg("query")
        .env("PATH", path_env)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let path = text
        .split("currently displaying: image: ")
        .nth(1)?
        .lines()
        .next()?
        .trim();
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

/// True if the theme background is dark (same criterion as the shell).
fn is_dark(c: Rgba) -> bool {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b < 0.5
}

/// Decode the wallpaper, shrink it strongly then upscale it again: a "cheap"
/// but stable blur, without dragging a gaussian filter over 4K.
/// On a dark theme, the image is also darkened and slightly desaturated
/// ("Soft"): we keep the shapes without having to raise the veil.
fn wallpaper(dark: bool) -> Option<Arc<RenderImage>> {
    let path = wallpaper_path()?;
    let img = image::open(&path).ok()?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return None;
    }

    let scale = (1920.0 / w.max(h) as f32).min(1.0);
    let tw = ((w as f32 * scale) as u32).max(1);
    let th = ((h as f32 * scale) as u32).max(1);

    let small = image::imageops::resize(
        &img,
        (tw / 8).max(8),
        (th / 8).max(8),
        image::imageops::FilterType::Triangle,
    );
    let small = image::imageops::blur(&small, 2.0);
    let small = if dark {
        grade(small, 0.62, 0.8)
    } else {
        small
    };
    let blurred = image::imageops::resize(&small, tw, th, image::imageops::FilterType::Triangle);

    // GPUI expects BGRA (same conversion as `load_png` in `kumizo_kit::services`).
    let mut raw = blurred.into_raw();
    for px in raw.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(tw, th, raw)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}

/// Multiply brightness and bring colors closer to gray: darken
/// without crushing the black point, unlike a flat veil.
fn grade(mut img: image::RgbaImage, brightness: f32, saturation: f32) -> image::RgbaImage {
    for p in img.pixels_mut() {
        let [r, g, b, a] = p.0;
        let (r, g, b) = (r as f32, g as f32, b as f32);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        p.0 = [
            ((luma + (r - luma) * saturation) * brightness).clamp(0.0, 255.0) as u8,
            ((luma + (g - luma) * saturation) * brightness).clamp(0.0, 255.0) as u8,
            ((luma + (b - luma) * saturation) * brightness).clamp(0.0, 255.0) as u8,
            a,
        ];
    }
    img
}

// ── PAM (raw FFI, libpam) ──────────────────────────────────────────────────
#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        handle: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
}

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;

extern "C" fn conversation(
    n: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    // SAFETY: PAM provides `n` valid messages; `appdata` points to the CString
    // of the password, which lives until `pam_end` in `pam_check`.
    unsafe {
        let password = appdata as *const c_char;
        let replies =
            libc::calloc(n as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if replies.is_null() {
            return PAM_BUF_ERR;
        }
        for i in 0..n as isize {
            let m = &*(*msg.offset(i));
            if m.msg_style == PAM_PROMPT_ECHO_OFF || m.msg_style == PAM_PROMPT_ECHO_ON {
                (*replies.offset(i)).resp = libc::strdup(password);
            }
        }
        *resp = replies;
        PAM_SUCCESS
    }
}

fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: volatile write on a valid byte (prevents optimization).
        unsafe { std::ptr::write_volatile(b, 0) };
    }
}

fn pam_check(user: &str, password: Vec<u8>) -> bool {
    let (Ok(user_c), Ok(pw_c), Ok(service)) = (
        CString::new(user),
        CString::new(password),
        CString::new("login"),
    ) else {
        return false;
    };
    let conv = PamConv {
        conv: conversation,
        appdata_ptr: pw_c.as_ptr() as *mut c_void,
    };
    let mut handle: *mut c_void = std::ptr::null_mut();

    // SAFETY: all pointers remain valid until `pam_end`.
    let ok = unsafe {
        if pam_start(service.as_ptr(), user_c.as_ptr(), &conv, &mut handle) != PAM_SUCCESS {
            false
        } else {
            let rc = pam_authenticate(handle, 0);
            pam_end(handle, rc);
            rc == PAM_SUCCESS
        }
    };
    let mut bytes = pw_c.into_bytes();
    wipe(&mut bytes);
    ok
}

// ── View ─────────────────────────────────────────────────────────────────────
struct Lock {
    clock: Entity<Clock>,
    user: String,
    password: String,
    busy: bool,
    error: bool,
    colors: Colors,
    wallpaper: Option<Arc<RenderImage>>,
    focus: FocusHandle,
}

impl Lock {
    fn new(cx: &mut Context<Self>) -> Self {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "user".into());
        let colors = colors();
        let wallpaper = wallpaper(is_dark(colors.bg));
        Self {
            clock: cx.service::<Clock>(),
            user,
            password: String::new(),
            busy: false,
            error: false,
            colors,
            wallpaper,
            focus: cx.focus_handle(),
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.password.is_empty() {
            return;
        }
        self.busy = true;
        self.error = false;
        let user = self.user.clone();
        let pw = std::mem::take(&mut self.password).into_bytes();
        cx.notify();

        cx.spawn(async move |this, cx| {
            // Never on the UI thread: PAM may block for several seconds.
            let ok = cx
                .background_executor()
                .spawn(async move { pam_check(&user, pw) })
                .await;
            if ok {
                std::process::exit(0);
            }
            // Short anti-burst pause before re-enabling input.
            async_io::Timer::after(Duration::from_millis(400)).await;
            let _ = this.update(cx, |lock, cx| {
                lock.busy = false;
                lock.error = true;
                cx.notify();
            });
        })
        .detach();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match ev.keystroke.key.as_str() {
            "enter" => self.submit(cx),
            "backspace" => {
                self.password.pop();
                self.error = false;
                cx.notify();
            }
            "escape" => {
                self.password.clear();
                self.error = false;
                cx.notify();
            }
            _ => {
                let m = &ev.keystroke.modifiers;
                if !m.control && !m.alt && !m.platform {
                    if let Some(ch) = ev.keystroke.key_char.as_ref() {
                        if self.password.len() < 256 {
                            self.password.push_str(ch);
                            self.error = false;
                            cx.notify();
                        }
                    }
                }
            }
        }
    }
}

impl Render for Lock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let c = &self.colors;
        let clock = self.clock.read(cx);
        let (time, date) = (clock.time_hm().to_string(), clock.date_formatted().to_string());

        let dots = "•".repeat(self.password.chars().count());
        let (status, status_color) = if self.busy {
            ("Verifying…", c.muted)
        } else if self.error {
            ("Incorrect password", c.danger)
        } else {
            ("Type your password then Enter", c.muted)
        };

        let mut root = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .size_full()
            .relative()
            .bg(c.bg)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4();

        // Background: the current wallpaper (theme pool) blurred, veiled by a
        // gradient of the `bg` color to keep the clock readable. On a dark
        // theme, the image is already darkened ("Soft"): the veil can therefore
        // stay light and let the wallpaper breathe.
        if let Some(wall) = self.wallpaper.clone() {
            let (top, bottom) = if is_dark(c.bg) {
                (0.15, 0.40)
            } else {
                (0.35, 0.70)
            };
            root = root
                .child(
                    gpui::img(wall)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(c.bg.opacity(top), 0.),
                            linear_color_stop(c.bg.opacity(bottom), 1.),
                        )),
                );
        }

        root.child(
            div()
                .text_size(px(120.))
                .font_weight(FontWeight::BOLD)
                .font_features(clock_features())
                .text_color(c.text)
                .child(time),
        )
        .child(div().text_size(px(20.)).text_color(c.muted).child(date))
        .child(
            div()
                .mt(px(40.))
                .w(px(360.))
                .h(px(52.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_2()
                .border_color(if self.error { c.danger } else { c.accent })
                .text_color(c.text)
                .child(if dots.is_empty() {
                    div().text_color(c.muted).child(self.user.clone())
                } else {
                    div().child(dots)
                }),
        )
        .child(div().text_sm().text_color(status_color).child(status))
    }
}

fn main() {
    Kumizo::new()
        .ipc(false)
        .service::<Clock>()
        .window(
            LayerWindow::new("aurora-lockscreen")
                .layer(Layer::Overlay)
                .anchor(Anchor::FULLSCREEN)
                .exclusive_zone(-1)
                .keyboard_interactivity(KeyboardInteractivity::Exclusive)
                .namespace("aurora-lockscreen"),
            |cx: &mut gpui::App| cx.new(Lock::new),
        )
        .run();
}
