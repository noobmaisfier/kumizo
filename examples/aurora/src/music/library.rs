//! Bibliothèque locale : parcours du dossier musique, lecture des tags, pochettes.
//!
//! Aucun accès réseau, aucun service en ligne : uniquement des fichiers sur disque.
//! Les fonctions ici sont BLOQUANTES — le modèle les appelle sur l'executor
//! d'arrière-plan, jamais sur le thread UI.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use lofty::prelude::*;

const EXTENSIONS: [&str; 9] = [
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wma",
];
const COVER_NAMES: [&str; 8] = [
    "cover.jpg",
    "cover.png",
    "folder.jpg",
    "folder.png",
    "front.jpg",
    "front.png",
    "albumart.jpg",
    "album.jpg",
];
const MAX_TRACKS: usize = 20_000;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub number: u32,
    pub secs: u32,
    /// « titre artiste album » en minuscules, calculé une fois pour la recherche.
    pub hay: String,
}

/// Dossier musique : `$AURORA_MUSIC_DIR`, sinon `~/.config/aurora/music_dir`, sinon `~/Music`.
pub fn music_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("AURORA_MUSIC_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if let Ok(s) = std::fs::read_to_string(home.join(".config/aurora/music_dir")) {
        let t = s.trim();
        if !t.is_empty() {
            return match t.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => PathBuf::from(t),
            };
        }
    }
    home.join("Music")
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Parcourt `root` (récursif, sans suivre les liens symboliques) et lit les tags.
pub fn scan(root: &Path) -> Vec<Track> {
    let mut files = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if files.len() >= MAX_TRACKS {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            // `file_type()` ne suit pas les liens : pas de boucle possible.
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if !hidden && depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if ft.is_file() && is_audio(&path) {
                files.push(path);
                if files.len() >= MAX_TRACKS {
                    break;
                }
            }
        }
    }

    let mut tracks: Vec<Track> = files.iter().map(|p| read_track(p)).collect();
    tracks.sort_by_cached_key(|t| {
        (
            t.artist.to_lowercase(),
            t.album.to_lowercase(),
            t.number,
            t.title.to_lowercase(),
        )
    });
    tracks
}

fn read_track(path: &Path) -> Track {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut track = Track {
        path: path.to_path_buf(),
        title: stem,
        artist: "Artiste inconnu".into(),
        album: "Album inconnu".into(),
        number: 0,
        secs: 0,
        hay: String::new(),
    };
    // Fichier illisible ou sans tags : on garde le nom du fichier, jamais d'erreur.
    if let Ok(tagged) = lofty::read_from_path(path) {
        track.secs = tagged.properties().duration().as_secs() as u32;
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(t) = tag.title().filter(|t| !t.trim().is_empty()) {
                track.title = t.trim().to_string();
            }
            if let Some(a) = tag.artist().filter(|a| !a.trim().is_empty()) {
                track.artist = a.trim().to_string();
            }
            if let Some(a) = tag.album().filter(|a| !a.trim().is_empty()) {
                track.album = a.trim().to_string();
            }
            track.number = tag.track().unwrap_or(0);
        }
    }
    track.hay = format!("{} {} {}", track.title, track.artist, track.album).to_lowercase();
    track
}

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let dir = base.join("aurora/covers");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Chemin d'une pochette pour `track` : image du dossier, sinon pochette
/// embarquée extraite dans le cache. `None` si rien n'est trouvé.
pub fn cover_for(track: &Track) -> Option<PathBuf> {
    // 1. Image à côté du fichier.
    if let Some(dir) = track.path.parent() {
        for name in COVER_NAMES {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // 2. Pochette embarquée, mise en cache par album.
    let cache = cache_dir()?;
    let mut h = DefaultHasher::new();
    (&track.album, &track.artist, track.path.parent()).hash(&mut h);
    let key = format!("{:016x}", h.finish());
    for ext in ["jpg", "png"] {
        let cached = cache.join(format!("{key}.{ext}"));
        if cached.is_file() {
            return Some(cached);
        }
    }

    let tagged = lofty::read_from_path(&track.path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pic = tag.pictures().first()?;
    let ext = match pic.mime_type() {
        Some(lofty::picture::MimeType::Png) => "png",
        _ => "jpg",
    };
    let out = cache.join(format!("{key}.{ext}"));
    std::fs::write(&out, pic.data()).ok()?;
    Some(out)
}

/// « 3:07 » / « 1:02:09 ».
pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
