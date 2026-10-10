//! Lecteur de musique locale — aucun service en ligne.
//!
//! engine  : mpv piloté par IPC (threads dédiés)
//! library : parcours du dossier, tags (lofty), pochettes
//! model   : file d'attente + état de lecture (entité globale)
//! lyrics  : paroles locales (.lrc / .txt / tags)
//! disc    : disque-pochette, barre de lecture, boutons
//! view    : fenêtre `aurora-music`

mod disc;
mod engine;
mod library;
mod lyrics;
mod model;
mod view;

pub use library::fmt_time;
pub use model::{handle, Player};
pub use view::MusicPanel;
