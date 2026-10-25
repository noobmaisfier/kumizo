//! Thèmes prédéfinis + palette « à chaud ».
//!
//! Le thème courant est un simple index atomique : tous les windows du shell
//! vivent dans le même process, donc `theme::set(i, cx)` + `refresh_windows()`
//! restyle toute l'interface instantanément. Le choix est mémorisé dans
//! `~/.config/aurora/theme`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{rgb, App, Rgba};

pub struct Theme {
    pub name: &'static str,
    pub bg: u32,
    pub surface: u32,
    pub raised: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent2: u32,
    /// Couleur du texte posé SUR un fond `accent`.
    pub on_accent: u32,
    pub ok: u32,
    pub warn: u32,
    pub danger: u32,
}

#[allow(clippy::too_many_arguments)]
const fn t(
    name: &'static str,
    bg: u32,
    surface: u32,
    raised: u32,
    border: u32,
    text: u32,
    muted: u32,
    accent: u32,
    accent2: u32,
    on_accent: u32,
    ok: u32,
    warn: u32,
    danger: u32,
) -> Theme {
    Theme {
        name,
        bg,
        surface,
        raised,
        border,
        text,
        muted,
        accent,
        accent2,
        on_accent,
        ok,
        warn,
        danger,
    }
}

//                    bg        surface   raised    border    text      muted     accent    accent2   on_accent ok        warn      danger
pub static THEMES: [Theme; 11] = [
    t("Aurora",           0x0b0e14, 0x131824, 0x1c2333, 0x232b40, 0xe6edf7, 0x6b7794, 0x00e8c6, 0x8b7cff, 0x04120f, 0x4ade80, 0xfbbf24, 0xff5c7a),
    t("Flexoki Light",    0xfffcf0, 0xf2f0e5, 0xe6e4d9, 0xdad8ce, 0x100f0f, 0x6f6e69, 0x205ea6, 0xda702c, 0xfffcf0, 0x66800b, 0xad8301, 0xaf3029),
    t("Catppuccin Mocha", 0x1e1e2e, 0x313244, 0x45475a, 0x45475a, 0xcdd6f4, 0x7f849c, 0xf5c2e7, 0x89b4fa, 0x11111b, 0xa6e3a1, 0xf9e2af, 0xf38ba8),
    t("Catppuccin Latte", 0xeff1f5, 0xe6e9ef, 0xccd0da, 0xbcc0cc, 0x4c4f69, 0x7c7f93, 0x8839ef, 0x1e66f5, 0xeff1f5, 0x40a02b, 0xdf8e1d, 0xd20f39),
    t("Tokyo Night",      0x1a1b26, 0x24283b, 0x2f334d, 0x3b4261, 0xc0caf5, 0x737aa2, 0x7aa2f7, 0xbb9af7, 0x16161e, 0x9ece6a, 0xe0af68, 0xf7768e),
    t("Gruvbox",          0x282828, 0x32302f, 0x3c3836, 0x504945, 0xebdbb2, 0xa89984, 0xfabd2f, 0x83a598, 0x1d2021, 0xb8bb26, 0xfe8019, 0xfb4934),
    t("Nord",             0x2e3440, 0x3b4252, 0x434c5e, 0x4c566a, 0xeceff4, 0x9aa5be, 0x88c0d0, 0xb48ead, 0x2e3440, 0xa3be8c, 0xebcb8b, 0xbf616a),
    t("Rose Pine",        0x191724, 0x1f1d2e, 0x26233a, 0x403d52, 0xe0def4, 0x908caa, 0xc4a7e7, 0xebbcba, 0x191724, 0x9ccfd8, 0xf6c177, 0xeb6f92),
    t("Dracula",          0x282a36, 0x303344, 0x44475a, 0x44475a, 0xf8f8f2, 0x8a93bd, 0xbd93f9, 0xff79c6, 0x282a36, 0x50fa7b, 0xf1fa8c, 0xff5555),
    t("Everforest",       0x2d353b, 0x343f44, 0x3d484d, 0x475258, 0xd3c6aa, 0x9da9a0, 0xa7c080, 0x7fbbb3, 0x2d353b, 0xa7c080, 0xdbbc7f, 0xe67e80),
    t("GitHub Light",     0xffffff, 0xf6f8fa, 0xeff1f4, 0xd0d7de, 0x1f2328, 0x59636e, 0x0969da, 0x8250df, 0xffffff, 0x1a7f37, 0x9a6700, 0xd1242c),
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/aurora/theme"))
}

/// À appeler une fois au démarrage : relit le thème mémorisé.
pub fn init() {
    let saved = config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| THEMES.iter().position(|th| th.name == s.trim()));
    if let Some(i) = saved {
        CURRENT.store(i, Ordering::Relaxed);
    }
}

pub fn current_index() -> usize {
    CURRENT.load(Ordering::Relaxed).min(THEMES.len() - 1)
}

pub fn current() -> &'static Theme {
    &THEMES[current_index()]
}

/// Vrai si le fond du thème est clair. Sert aux rares endroits où il faut
/// adapter le contraste (icônes de tray monochromes, par exemple).
pub fn is_light() -> bool {
    let c = current().bg;
    let r = ((c >> 16) & 0xff) as f32;
    let g = ((c >> 8) & 0xff) as f32;
    let b = (c & 0xff) as f32;
    0.2126 * r + 0.7152 * g + 0.0722 * b > 127.5
}

/// Clés `change-theme` des thèmes appliqués à tout le système (sway, foot,
/// yazi, vim, gtk, herdr, puis aurora par IPC). Les thèmes absents de cette
/// table ne changent qu'aurora : le script ne sait pas les décliner.
const SYSTEM_KEYS: [(&str, &str); 7] = [
    ("flexoki-light", "Flexoki Light"),
    ("mocha", "Catppuccin Mocha"),
    ("aurora", "Aurora"),
    ("github-light", "GitHub Light"),
    ("nord", "Nord"),
    ("tokyo-night", "Tokyo Night"),
    ("gruvbox-dark", "Gruvbox"),
];

/// Clé `change-theme` d'un thème, si le script sait l'appliquer au système.
pub fn system_key(index: usize) -> Option<&'static str> {
    let name = THEMES.get(index)?.name;
    SYSTEM_KEYS
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(k, _)| *k)
}

/// Thèmes système dans l'ordre du script, avec leur index dans `THEMES`.
pub fn system_themes() -> impl Iterator<Item = (usize, &'static str)> {
    SYSTEM_KEYS.iter().filter_map(|(key, name)| {
        THEMES
            .iter()
            .position(|t| t.name == *name)
            .map(|i| (i, *key))
    })
}

/// Applique un thème à tout le système via `~/.local/bin/change-theme`.
/// Le script rebascule ensuite aurora par IPC (`kumizo service theme set`).
pub fn apply_system(key: &str) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let script = PathBuf::from(home).join(".local/bin/change-theme");
    if !script.is_file() {
        return;
    }
    if let Some(path) = script.to_str() {
        crate::ui::run(path, &[key]);
    }
}

/// Résout une cible CLI (« Flexoki Light » ou un index « 3 ») en index de
/// thème. Les noms sont comparés sans tenir compte de la casse ; un index
/// numérique est accepté tel quel (`set` borne ensuite).
pub fn resolve(target: &str) -> Option<usize> {
    let t = target.trim();
    if let Ok(i) = t.parse::<usize>() {
        return Some(i);
    }
    THEMES.iter().position(|th| th.name.eq_ignore_ascii_case(t))
}

/// Change de thème, le mémorise, et redessine toutes les fenêtres.
pub fn set(index: usize, cx: &mut App) {
    let index = index.min(THEMES.len() - 1);
    CURRENT.store(index, Ordering::Relaxed);
    if let Some(path) = config_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, THEMES[index].name);
    }
    cx.refresh_windows();
}

/// Tokens sémantiques : `pal::accent()`, `pal::surface()`, …
pub mod pal {
    use gpui::{rgb, Rgba};

    macro_rules! tokens {
        ($($n:ident),* $(,)?) => {
            $(pub fn $n() -> Rgba { rgb(super::current().$n) })*
        };
    }

    tokens!(bg, surface, raised, border, text, muted, accent, accent2, on_accent, ok, warn, danger);

    pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
        Rgba { a, ..c }
    }

    pub fn bg_a(a: f32) -> Rgba {
        with_alpha(bg(), a)
    }

    pub fn surface_a(a: f32) -> Rgba {
        with_alpha(surface(), a)
    }
}

/// Couleur brute d'un thème (pour les pastilles du sélecteur).
pub fn swatch(hex: u32) -> Rgba {
    rgb(hex)
}
