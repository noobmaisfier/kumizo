//! Lecteur de musique locale — aucun service en ligne.
//!
//! engine  : rockbox-playback en processus (codecs Rockbox + DSP, cpal)
//! library : parcours du dossier, tags (lofty), pochettes
//! model   : file d'attente + état de lecture (entité globale)
//! lyrics  : paroles locales (.lrc / .txt / tags)
//! disc    : pochette, karaoké, barre de lecture, boutons
//! view    : fenêtre `aurora-music`
//!
//! NOTE LICENCE : rockbox-playback est GPL-2.0-or-later, voir `Cargo.toml`.

mod disc;
mod engine;
mod library;
mod lyrics;
mod model;
mod view;

pub use library::fmt_time;
pub use model::{handle, Music, Player};
pub use view::MusicPanel;
