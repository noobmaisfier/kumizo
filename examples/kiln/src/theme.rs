//! Systeme de themes : tokens `pal`, presets TOML, synchronisation du fond d'ecran.

use gpui::{px, rgb, App, Rgba, Styled};
use serde::Deserialize;
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        LazyLock, RwLock,
    },
};

#[derive(Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Meta {
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub variant: String,
}

#[derive(Clone, Deserialize, Default)]
pub struct Wallpaper {
    pub path: String,
    #[serde(default)]
    pub mode: String,
}

#[derive(Clone, Deserialize, Default)]
pub struct Colors {
    pub bg: String,
    pub surface: String,
    pub text: String,
    pub text_muted: String,
    pub accent: String,
    pub secondary: String,
    pub border: String,
    pub urgent: String,
}

#[derive(Clone, Deserialize)]
#[allow(dead_code)]
pub struct Style {
    pub border_width: f32,
    pub corner_radius: f32,
    #[serde(default)]
    pub shadow_size: f32,
    #[serde(default = "default_font")]
    pub font: String,
}

impl Default for Style {
    fn default() -> Self {
        Self { border_width: 1.0, corner_radius: 8.0, shadow_size: 0.0, font: default_font() }
    }
}

fn default_font() -> String {
    "monospace".into()
}

#[derive(Clone, Deserialize, Default)]
pub struct Theme {
    pub meta: Meta,
    pub wallpaper: Wallpaper,
    pub colors: Colors,
    #[serde(default)]
    pub style: Style,
}

impl Theme {
    pub fn swatches(&self) -> [Rgba; 5] {
        let c = &self.colors;
        [hex(&c.bg), hex(&c.surface), hex(&c.accent), hex(&c.secondary), hex(&c.text)]
    }
    pub fn wallpaper_file(&self) -> String {
        self.wallpaper.path.rsplit('/').next().unwrap_or("").to_string()
    }
}

fn hex(s: &str) -> Rgba {
    rgb(u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0xff00ff))
}

const BUILTIN: [&str; 3] = [
    include_str!("../themes/obsidian-bloom.toml"),
    include_str!("../themes/papier-atelier.toml"),
    include_str!("../themes/amber-crt.toml"),
];

static THEMES: LazyLock<RwLock<Vec<Theme>>> = LazyLock::new(|| RwLock::new(load_all()));
static INDEX: AtomicUsize = AtomicUsize::new(0);

fn load_all() -> Vec<Theme> {
    let mut v: Vec<Theme> = BUILTIN.iter().filter_map(|s| toml::from_str(s).ok()).collect();
    // Themes utilisateur : ~/.config/kiln/themes/*.toml
    if let Some(home) = std::env::var_os("HOME") {
        let dir = PathBuf::from(home).join(".config/kiln/themes");
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            files.sort();
            for p in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "toml")) {
                if let Ok(txt) = std::fs::read_to_string(&p) {
                    if let Ok(t) = toml::from_str::<Theme>(&txt) {
                        v.push(t);
                    }
                }
            }
        }
    }
    v
}

pub fn all() -> Vec<Theme> {
    THEMES.read().unwrap().clone()
}

pub fn current_index() -> usize {
    INDEX.load(Ordering::Relaxed)
}

fn current() -> Theme {
    let g = THEMES.read().unwrap();
    g.get(current_index()).or_else(|| g.first()).cloned().unwrap_or_default()
}

/// Applique un theme en memoire (zero redemarrage), redessine toutes les fenetres
/// et synchronise le fond d'ecran.
pub fn apply(idx: usize, cx: &mut App) {
    let len = THEMES.read().unwrap().len();
    if len == 0 {
        return;
    }
    INDEX.store(idx % len, Ordering::Relaxed);
    apply_wallpaper(&current());
    cx.refresh_windows();
}

/// A appeler au demarrage pour poser le fond d'ecran du theme initial.
pub fn init_wallpaper() {
    apply_wallpaper(&current());
}

fn expand(p: &str) -> PathBuf {
    if let (Some(rest), Some(home)) = (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(p)
    }
}

fn apply_wallpaper(t: &Theme) {
    let path = expand(&t.wallpaper.path);
    let exists = path.exists();
    let mode = if t.wallpaper.mode.is_empty() { "fill" } else { t.wallpaper.mode.as_str() };
    let bg = t.colors.bg.clone();
    let p = path.to_string_lossy().to_string();

    let cmd = if std::env::var_os("SWAYSOCK").is_some() {
        let mut c = Command::new("swaymsg");
        if exists {
            c.args(["output", "*", "bg", &p, mode]);
        } else {
            c.args(["output", "*", "bg", &bg, "solid_color"]); // repli : aplat de couleur
        }
        Some(c)
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        let mut c = Command::new("swww");
        if exists {
            c.args(["img", &p]);
        } else {
            c.args(["clear", bg.trim_start_matches('#')]);
        }
        Some(c)
    } else if exists {
        let mut c = Command::new("wbg");
        c.arg(&p);
        Some(c)
    } else {
        None
    };

    if let Some(mut c) = cmd {
        std::thread::spawn(move || {
            let _ = c.status();
        });
    }
}

/// Bordure + arrondi pilotes par le theme.
pub fn framed<E: Styled>(e: E) -> E {
    let e = if pal::border_w() >= 2.0 { e.border_2() } else { e.border_1() };
    e.border_color(pal::border()).rounded(px(pal::radius()))
}

pub mod pal {
    use super::*;
    pub fn bg() -> Rgba { hex(&current().colors.bg) }
    pub fn surface() -> Rgba { hex(&current().colors.surface) }
    pub fn text() -> Rgba { hex(&current().colors.text) }
    pub fn text_muted() -> Rgba { hex(&current().colors.text_muted) }
    pub fn accent() -> Rgba { hex(&current().colors.accent) }
    pub fn secondary() -> Rgba { hex(&current().colors.secondary) }
    pub fn border() -> Rgba { hex(&current().colors.border) }
    pub fn urgent() -> Rgba { hex(&current().colors.urgent) }
    pub fn radius() -> f32 { current().style.corner_radius }
    pub fn border_w() -> f32 { current().style.border_width }
    pub fn font() -> String { current().style.font }
}
