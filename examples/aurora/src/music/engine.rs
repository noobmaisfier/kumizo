//! Moteur audio : rockbox-playback (codecs Rockbox + DSP, sortie cpal).
//!
//! * Tout se passe sur un thread dédié : l'UI n'attend jamais le décodage.
//! * La vue lit un `Snapshot` (position, pause, compteur de fins de piste).
//! * Même interface publique que les anciens moteurs : `model.rs` inchangé.
//! * Un seul morceau à la fois (`set_queue([path])`) : la file, le mélange
//!   et la répétition restent dans `model.rs`, qui avance sur `ended`.
//! * Crossfade / ReplayGain / EQ : réglages d'usine (neutres), non utilisés.
//!   Nos fichiers n'ont pas de tags ReplayGain : le mode natif serait sans
//!   effet. Voir le niveau trop chaud de certains cantiques.
//!
//! Spécificités rockbox à connaître :
//! * `set_volume(0.0)` fige l'anneau (position gelée) : le plancher est donc
//!   0.001, inaudible mais vivant (voir `floor_volume`).
//! * Fin NATURELLE = transition `Playing → Stopped` ; un `Load`/`Stop`
//!   manuel l'inhibe (`suppress_end`).
//! * Un fichier illisible reste `Stopped` sans jamais jouer : erreur affichée
//!   après un délai de grâce, sans enchaîner (comme avant).

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rockbox_playback::{OutputConfig, PlaybackState, PlayerConfig};

#[derive(Clone, Default)]
pub struct Snapshot {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    /// Incrémenté à chaque fin NATURELLE de piste (pas lors d'un changement manuel).
    pub ended: u64,
    /// Message d'erreur affichable (périphérique absent, fichier illisible…).
    pub error: Option<String>,
}

enum Cmd {
    Load(PathBuf),
    Pause(bool),
    Seek(f64),
    Volume(u8),
    Stop,
    Quit,
}

pub struct Engine {
    tx: mpsc::Sender<Cmd>,
    shared: Arc<Mutex<Snapshot>>,
}

/// Volume 0 demandé (muet) : 0.0 exact figerait l'anneau rockbox (position
/// gelée, `get_pos` immobile). 0.001 est inaudible et garde le flux vivant.
fn floor_volume(pct: u8) -> f32 {
    if pct == 0 { 0.001 } else { pct as f32 / 100.0 }
}

impl Engine {
    /// Démarre le worker ; retourne immédiatement (le périphérique audio
    /// s'ouvre en arrière-plan).
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let worker_shared = shared.clone();
        thread::Builder::new()
            .name("aurora-rockbox".into())
            .spawn(move || worker(rx, worker_shared))
            .ok();
        Self { tx, shared }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn load(&self, path: &Path) {
        let _ = self.tx.send(Cmd::Load(path.to_path_buf()));
    }
    pub fn set_paused(&self, paused: bool) {
        let _ = self.tx.send(Cmd::Pause(paused));
    }
    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs.max(0.0)));
    }
    pub fn set_volume(&self, pct: u8) {
        let _ = self.tx.send(Cmd::Volume(pct.min(100)));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

fn set_error(shared: &Arc<Mutex<Snapshot>>, msg: String) {
    if let Ok(mut s) = shared.lock() {
        s.error = Some(msg);
    }
}

/// Délai après `Load` sans aucun son avant de déclarer le fichier illisible.
const LOAD_GRACE: Duration = Duration::from_secs(2);

fn worker(rx: mpsc::Receiver<Cmd>, shared: Arc<Mutex<Snapshot>>) {
    let player = match PlayerConfig::builder()
        .output(OutputConfig::Cpal)
        .open()
    {
        Ok(p) => p,
        Err(e) => {
            set_error(&shared, format!("audio indisponible ({e:?})"));
            return;
        }
    };

    let mut volume = 0.8f32;
    player.set_volume(volume);
    // Détection de fin naturelle : front Playing → Stopped.
    let mut was_playing = false;
    let mut suppress_end = false;
    // Fichier en cours de chargement : jamais entendu depuis `Load`.
    let mut loading_since: Option<Instant> = None;
    let mut load_failed = false;

    // Convenance : `Player` n'est ni `Send` ni `Sync`, il vit et meurt ici.
    let player = player;

    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Cmd::Load(path)) => {
                player.set_queue(vec![path]);
                player.play();
                was_playing = false;
                suppress_end = false;
                loading_since = Some(Instant::now());
                load_failed = false;
                if let Ok(mut s) = shared.lock() {
                    s.position = 0.0;
                    s.paused = false;
                    s.error = None;
                }
            }
            Ok(Cmd::Pause(b)) => {
                if b {
                    player.pause();
                } else {
                    player.play();
                }
            }
            Ok(Cmd::Seek(t)) => {
                player.seek(Duration::from_secs_f64(t));
            }
            Ok(Cmd::Volume(v)) => {
                volume = floor_volume(v);
                player.set_volume(volume);
            }
            Ok(Cmd::Stop) => {
                player.stop();
                was_playing = false;
                suppress_end = true;
                loading_since = None;
            }
            Ok(Cmd::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let st = player.status();
        let state = st.state;
        let playing_now = state == PlaybackState::Playing;
        if playing_now {
            loading_since = None;
        }

        let mut finished = false;
        if was_playing && state == PlaybackState::Stopped && !suppress_end {
            finished = true;
        }
        // Un `Load`/`Stop` consomme l'inhibition dès qu'on rejoue.
        if playing_now {
            suppress_end = false;
        }
        was_playing = playing_now;

        // Fichier illisible : resté `Stopped` sans son après le délai.
        if !load_failed {
            if let Some(since) = loading_since {
                if since.elapsed() > LOAD_GRACE
                    && state == PlaybackState::Stopped
                    && st.position.is_zero()
                {
                    load_failed = true;
                    loading_since = None;
                    set_error(&shared, "fichier illisible".to_string());
                }
            }
        }

        if let Ok(mut s) = shared.lock() {
            s.position = st.position.as_secs_f64();
            // Durée : le tag fait foi tant que le moteur n'a rien annoncé.
            if !st.duration.is_zero() {
                s.duration = st.duration.as_secs_f64();
            }
            s.paused = state == PlaybackState::Paused;
            if finished {
                s.ended += 1;
            }
        }
    }
}
