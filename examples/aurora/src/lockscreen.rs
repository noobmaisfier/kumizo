//! aurora-lockscreen — binaire SÉPARÉ du shell (cf. §11 du guide Kumizo).
//!
//! * `.ipc(false)` : aucun socket, aucun contrôle distant, aucun conflit avec le shell.
//! * Plein écran `Overlay` + clavier `Exclusive`.
//! * Mot de passe vérifié par PAM (service `login`) sur l'executor d'arrière-plan.
//! * Aucun code ni service partagé avec le shell : la mini-palette et l'horloge
//!   sont volontairement dupliquées.
//!
//! ⚠ Limite de conception : c'est une surface layer-shell, PAS le protocole
//! `ext-session-lock`. Un compositeur peut encore réagir à des raccourcis
//! système (changement de TTY, etc.). Pour un verrouillage « de sécurité »
//! strict, préfère un locker basé sur ext-session-lock.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{div, px, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, Rgba, Window};
use kumizo_kit::prelude::*;

// ── Mini-palette (dupliquée, lit le thème choisi dans le shell) ─────────────
//                 nom                bg        text      muted     accent    danger
const PALETTES: [(&str, u32, u32, u32, u32, u32); 11] = [
    ("Aurora",           0x0b0e14, 0xe6edf7, 0x6b7794, 0x00e8c6, 0xff5c7a),
    ("Flexoki Light",    0xfffcf0, 0x100f0f, 0x6f6e69, 0x205ea6, 0xaf3029),
    ("Catppuccin Mocha", 0x1e1e2e, 0xcdd6f4, 0x7f849c, 0xcba6f7, 0xf38ba8),
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

// ── PAM (FFI brut, libpam) ──────────────────────────────────────────────────
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
    // SAFETY: PAM fournit `n` messages valides ; `appdata` pointe vers le CString
    // du mot de passe, qui vit jusqu'à `pam_end` dans `pam_check`.
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
        // SAFETY: écriture volatile sur un octet valide (empêche l'optimisation).
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

    // SAFETY: tous les pointeurs restent valides jusqu'à `pam_end`.
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

// ── Vue ─────────────────────────────────────────────────────────────────────
struct Lock {
    clock: Entity<Clock>,
    user: String,
    password: String,
    busy: bool,
    error: bool,
    colors: Colors,
    focus: FocusHandle,
}

impl Lock {
    fn new(cx: &mut Context<Self>) -> Self {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "utilisateur".into());
        Self {
            clock: cx.service::<Clock>(),
            user,
            password: String::new(),
            busy: false,
            error: false,
            colors: colors(),
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
            // Jamais sur le thread UI : PAM peut bloquer plusieurs secondes.
            let ok = cx
                .background_executor()
                .spawn(async move { pam_check(&user, pw) })
                .await;
            if ok {
                std::process::exit(0);
            }
            // Petite pause anti-rafale avant de ré-autoriser la saisie.
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
            ("Vérification…", c.muted)
        } else if self.error {
            ("Mot de passe incorrect", c.danger)
        } else {
            ("Tape ton mot de passe puis Entrée", c.muted)
        };

        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .size_full()
            .bg(c.bg)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .text_size(px(120.))
                    .font_weight(FontWeight::BOLD)
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
