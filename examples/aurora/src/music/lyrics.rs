//! Paroles locales : `.lrc` synchronisé, `.txt`, ou paroles intégrées aux tags.
//!
//! Aucun accès réseau. Les fonctions sont BLOQUANTES : le modèle les appelle sur
//! l'executor d'arrière-plan. Aucun découpage par indices d'octets sur du texte
//! utilisateur : seuls les caractères ASCII `[` `]` `:` servent de repères.

use std::path::Path;

use lofty::prelude::*;
use lofty::tag::ItemKey;

const MAX_BYTES: u64 = 1_000_000;
const MAX_LINES: usize = 2_000;

#[derive(Clone, Default)]
pub struct Lyrics {
    /// (horodatage en secondes si synchronisé, texte). Une ligne vide = respiration.
    pub lines: Vec<(Option<f64>, String)>,
    pub synced: bool,
}

impl Lyrics {
    /// Index de la ligne en cours (paroles synchronisées uniquement).
    pub fn current(&self, pos: f64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        self.lines
            .iter()
            .rposition(|(t, _)| t.map_or(false, |t| t <= pos))
    }
}

/// Cherche d'abord `titre.lrc` puis `titre.txt` à côté du fichier, puis les
/// paroles embarquées dans les tags.
pub fn load(track: &Path) -> Option<Lyrics> {
    for ext in ["lrc", "txt"] {
        let p = track.with_extension(ext);
        let small = std::fs::metadata(&p)
            .map(|m| m.is_file() && m.len() <= MAX_BYTES)
            .unwrap_or(false);
        if !small {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Some(l) = parse(&text) {
                return Some(l);
            }
        }
    }

    let tagged = lofty::read_from_path(track).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let text = tag
        .items()
        .find(|i| i.key() == ItemKey::Lyrics)
        .and_then(|i| i.value().text())?;
    parse(text)
}

/// « 01:23.45 » → 83.45 ; `None` pour les balises de métadonnées (`ar:…`).
fn parse_stamp(s: &str) -> Option<f64> {
    let (m, sec) = s.split_once(':')?;
    let m: f64 = m.trim().parse().ok()?;
    let sec: f64 = sec.trim().parse().ok()?;
    let t = m * 60.0 + sec;
    (t.is_finite() && t >= 0.0).then_some(t)
}

/// `[ar:Artiste]`, `[ti:Titre]`, `[by:…]` : balises à ignorer.
fn is_meta(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|r| r.split_once(']'))
        .map_or(false, |(inner, tail)| {
            tail.trim().is_empty()
                && inner
                    .split_once(':')
                    .map_or(false, |(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic()))
        })
}

pub fn parse(text: &str) -> Option<Lyrics> {
    let mut lines: Vec<(Option<f64>, String)> = Vec::new();
    let mut synced = false;

    for raw in text.lines() {
        let mut rest = raw.trim();
        if is_meta(rest) {
            continue;
        }
        // Plusieurs horodatages possibles : [00:12.00][00:45.10]texte
        let mut stamps = Vec::new();
        while let Some(inner_and_tail) = rest.strip_prefix('[') {
            let Some((inner, tail)) = inner_and_tail.split_once(']') else {
                break;
            };
            match parse_stamp(inner) {
                Some(t) => {
                    stamps.push(t);
                    rest = tail.trim_start();
                }
                None => break,
            }
        }
        if stamps.is_empty() {
            lines.push((None, rest.to_string()));
        } else {
            synced = true;
            for t in stamps {
                lines.push((Some(t), rest.to_string()));
            }
        }
        if lines.len() >= MAX_LINES {
            break;
        }
    }

    if synced {
        // Fichier synchronisé : les lignes sans horodatage n'ont pas de place.
        lines.retain(|(t, _)| t.is_some());
        lines.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        // Texte brut : on retire les blancs de début et de fin.
        while lines.first().map_or(false, |(_, s)| s.is_empty()) {
            lines.remove(0);
        }
        while lines.last().map_or(false, |(_, s)| s.is_empty()) {
            lines.pop();
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(Lyrics { lines, synced })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_synchronise() {
        let l = parse("[ar:Moi]\n[00:01.50]un\n[00:03.00][00:09.00]deux\n").unwrap();
        assert!(l.synced);
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.current(0.5), None);
        assert_eq!(l.current(2.0), Some(0));
        assert_eq!(l.current(9.5), Some(2));
    }

    #[test]
    fn texte_brut_et_utf8() {
        let l = parse("\n\nÀ la lune éclatée 🌙\n\nrefrain\n\n").unwrap();
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.lines[0].1, "À la lune éclatée 🌙");
    }

    #[test]
    fn vide() {
        assert!(parse("[ti:rien]\n").is_none());
    }
}
