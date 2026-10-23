//! Desktop applications service.
//!
//! Scans FreeDesktop `.desktop` files, parses names, categories, and commands,
//! and resolves and decodes icons (PNG and SVG via `resvg`) directly into GPU textures.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, RenderImage};
use image::{Frame, ImageBuffer};
use smallvec::smallvec;
use tracing::{error, info};

use crate::service::Service;

/// One discovered desktop application.
#[derive(Debug, Clone)]
pub struct AppInfo {
    /// Desktop file ID (e.g. `firefox` or `org.gnome.Nautilus`).
    pub id: String,
    /// Display name (e.g. "Firefox Web Browser").
    pub name: String,
    /// Optional short description or subtitle.
    pub comment: String,
    /// Sanitized launch command.
    pub exec: String,
    /// Original icon name or path.
    pub icon_name: String,
    /// Decoded GPU-ready image texture (PNG or SVG).
    pub icon: Option<Arc<RenderImage>>,
    /// Categories (e.g. "Network", "WebBrowser").
    pub categories: Vec<String>,
    /// Search keywords.
    pub keywords: Vec<String>,
}

/// FreeDesktop application catalogue.
pub struct Apps {
    apps: Vec<AppInfo>,
    /// Cache of decoded icon names to avoid re-decoding duplicates.
    icon_cache: HashMap<String, Option<Arc<RenderImage>>>,
}

impl Apps {
    /// Create the apps service and scan system applications.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        let mut service = Self {
            apps: Vec::new(),
            icon_cache: HashMap::new(),
        };
        service.reload();
        service
    }

    /// List all discovered applications.
    pub fn apps(&self) -> &[AppInfo] {
        &self.apps
    }

    /// Find an application's decoded GPU icon by its application class or ID (case-insensitive).
    pub fn find_icon(&self, class_or_app_id: &str) -> Option<Arc<RenderImage>> {
        let needle = class_or_app_id.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }

        // 1. Direct match on app id (e.g. "helium", "foot", "code")
        if let Some(app) = self.apps.iter().find(|a| a.id.to_lowercase() == needle) {
            if let Some(icon) = &app.icon {
                return Some(icon.clone());
            }
        }

        // 2. Substring or executable match (e.g. "helium-browser", "footclient")
        if let Some(app) = self.apps.iter().find(|a| {
            a.id.to_lowercase().contains(&needle)
                || needle.contains(&a.id.to_lowercase())
                || a.exec.to_lowercase().contains(&needle)
                || needle.contains(&a.exec.to_lowercase())
        }) {
            if let Some(icon) = &app.icon {
                return Some(icon.clone());
            }
        }

        None
    }

    /// Search applications by substring match on name, executable, keywords, and description.
    pub fn search(&self, query: &str) -> Vec<AppInfo> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.apps.clone();
        }

        let mut scored: Vec<(i32, AppInfo)> = self
            .apps
            .iter()
            .filter_map(|app| {
                let name_lower = app.name.to_lowercase();
                let exec_lower = app.exec.to_lowercase();
                let comment_lower = app.comment.to_lowercase();

                let mut score = 0;
                if name_lower == q {
                    score += 100;
                } else if name_lower.starts_with(&q) {
                    score += 50;
                } else if name_lower.contains(&q) {
                    score += 30;
                } else if exec_lower.starts_with(&q) {
                    score += 25;
                } else if exec_lower.contains(&q) {
                    score += 15;
                } else if app.keywords.iter().any(|k| k.to_lowercase().contains(&q)) {
                    score += 10;
                } else if comment_lower.contains(&q) {
                    score += 5;
                } else {
                    return None;
                }

                Some((score, app.clone()))
            })
            .collect();

        // Sort descending by score, then alphabetically
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
        });

        scored.into_iter().map(|(_, app)| app).collect()
    }

    /// Launch an application in the background.
    pub fn launch(&self, app: &AppInfo) {
        let cmd = app.exec.clone();
        info!("Launching application '{}': {}", app.name, cmd);
        if let Err(e) = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{} &", cmd))
            .spawn()
        {
            error!("Failed to launch '{}': {}", app.name, e);
        }
    }

    /// Rescan `.desktop` files.
    pub fn reload(&mut self) {
        let mut dirs = vec![
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/usr/local/share/applications"),
        ];

        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(&home).join(".local/share/applications"));
        }

        if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
            for dir in data_dirs.split(':') {
                let p = PathBuf::from(dir).join("applications");
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }

        let mut discovered = HashMap::new();

        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "desktop") {
                    if let Some(app) = self.parse_desktop_file(&path) {
                        // Prevent duplicates: local overrides system if encountered
                        discovered.entry(app.id.clone()).or_insert(app);
                    }
                }
            }
        }

        let mut apps: Vec<AppInfo> = discovered.into_values().collect();
        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        info!("Apps service loaded {} applications", apps.len());
        self.apps = apps;
    }

    fn parse_desktop_file(&mut self, path: &Path) -> Option<AppInfo> {
        let content = fs::read_to_string(path).ok()?;
        let file_stem = path.file_stem()?.to_string_lossy().to_string();

        let mut in_desktop_entry = false;
        let mut name: Option<String> = None;
        let mut generic_name: Option<String> = None;
        let mut comment: Option<String> = None;
        let mut exec: Option<String> = None;
        let mut icon_name: Option<String> = None;
        let mut no_display = false;
        let mut hidden = false;
        let mut is_app = false;
        let mut categories = Vec::new();
        let mut keywords = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_desktop_entry = line == "[Desktop Entry]";
                continue;
            }

            if !in_desktop_entry || line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();

            match key {
                "Type" => is_app = val == "Application",
                "NoDisplay" => no_display = val.eq_ignore_ascii_case("true"),
                "Hidden" => hidden = val.eq_ignore_ascii_case("true"),
                "Name" if name.is_none() => name = Some(val.to_string()),
                "GenericName" if generic_name.is_none() => generic_name = Some(val.to_string()),
                "Comment" if comment.is_none() => comment = Some(val.to_string()),
                "Exec" if exec.is_none() => exec = Some(val.to_string()),
                "Icon" if icon_name.is_none() => icon_name = Some(val.to_string()),
                "Categories" => {
                    categories = val
                        .split(';')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                }
                "Keywords" => {
                    keywords = val
                        .split(';')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                }
                _ => {}
            }
        }

        if !is_app || no_display || hidden {
            return None;
        }

        let name = name?;
        let raw_exec = exec?;
        let clean_exec = sanitize_exec(&raw_exec);
        let icon_str = icon_name.unwrap_or_default();

        let icon = self.resolve_icon(&icon_str);

        Some(AppInfo {
            id: file_stem,
            name,
            comment: comment.or(generic_name).unwrap_or_default(),
            exec: clean_exec,
            icon_name: icon_str,
            icon,
            categories,
            keywords,
        })
    }

    fn resolve_icon(&mut self, name: &str) -> Option<Arc<RenderImage>> {
        if name.is_empty() {
            return None;
        }

        if let Some(cached) = self.icon_cache.get(name) {
            return cached.clone();
        }

        let loaded = load_icon(name);
        self.icon_cache.insert(name.to_string(), loaded.clone());
        loaded
    }
}

impl Service for Apps {
    fn init(cx: &mut Context<Self>) -> Self {
        Self::new(cx)
    }
}

/// Strip FreeDesktop field codes (`%f`, `%u`, `%F`, etc.) from Exec command.
fn sanitize_exec(exec: &str) -> String {
    let mut parts = Vec::new();
    for token in exec.split_whitespace() {
        if token.starts_with('%') {
            continue;
        }
        parts.push(token);
    }
    parts.join(" ")
}

/// Public helper to load an icon by name or absolute path.
pub(crate) fn load_icon_pub(name: &str) -> Option<Arc<RenderImage>> {
    load_icon(name)
}

/// Resolve and load an icon by name or absolute path (supports PNG and SVG).
fn load_icon(name: &str) -> Option<Arc<RenderImage>> {
    let path = find_icon_path(name)?;
    if path.extension().is_some_and(|e| e == "png") {
        load_png(&path)
    } else if path.extension().is_some_and(|e| e == "svg") {
        load_svg(&path, 48, 48)
    } else {
        None
    }
}

fn find_icon_path(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(name);
    if p.is_absolute() && p.is_file() {
        return Some(p);
    }

    let mut base_dirs = vec![
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/share/pixmaps"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        base_dirs.push(PathBuf::from(&home).join(".local/share/icons"));
        base_dirs.push(PathBuf::from(&home).join(".icons"));
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    // Direct file in a base directory (e.g. /usr/share/pixmaps/foo.png).
    for base in &base_dirs {
        for ext in ["png", "svg", "xpm"] {
            let candidate = base.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
    }

    // Themed layout: <base>/<theme>/<size>/apps/<name>.<ext>
    // (hicolor, Luv, Adwaita, the user's icon theme, …). Collect every
    // matching size so we can pick the sharpest one afterwards — FreeDesktop
    // apps routinely ship only a 256x256 (or 512x512) asset, which the old
    // fixed size list missed.
    for base in &base_dirs {
        let Ok(themes) = fs::read_dir(base) else {
            continue;
        };
        for theme in themes.flatten() {
            let theme_dir = theme.path();
            if !theme_dir.is_dir() {
                continue;
            }
            let Ok(sizes) = fs::read_dir(&theme_dir) else {
                continue;
            };
            for size in sizes.flatten() {
                let size_dir = size.path();
                if !size_dir.is_dir() {
                    continue;
                }
                let apps_dir = size_dir.join("apps");
                if !apps_dir.is_dir() {
                    continue;
                }
                for ext in ["png", "svg", "xpm"] {
                    let candidate = apps_dir.join(format!("{name}.{ext}"));
                    if candidate.is_file() {
                        candidates.push(candidate);
                    }
                }
            }
        }
    }

    // Prefer the largest available size (scalable counts as 10000); on ties,
    // prefer SVG for crispness at small render sizes.
    candidates.into_iter().max_by_key(|p| {
        let size = p
            .ancestors()
            .find_map(|a| {
                let n = a.file_name()?.to_str()?;
                if n == "scalable" {
                    return Some(10000u32);
                }
                n.split('x').next()?.parse::<u32>().ok()
            })
            .unwrap_or(0);
        let svg = if p.extension().and_then(|e| e.to_str()) == Some("svg") {
            1u32
        } else {
            0
        };
        (size, svg)
    })
}

fn load_png(path: &Path) -> Option<Arc<RenderImage>> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    let mut bgra = img.into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}

fn load_svg(path: &Path, width: u32, height: u32) -> Option<Arc<RenderImage>> {
    let bytes = fs::read(path).ok()?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&bytes, &opt).ok()?;

    let (tw, th) = (tree.size().width(), tree.size().height());
    if tw <= 0.0 || th <= 0.0 {
        return None;
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let sx = width as f32 / tw;
    let sy = height as f32 / th;
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let mut bgra = pixmap.take();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }

    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}
