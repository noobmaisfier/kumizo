//! Modèle du lecteur : bibliothèque, file d'attente, état de lecture.
//!
//! Une seule instance (entité globale, comme le pomodoro) partagée par la barre
//! et la fenêtre `aurora-music`.
//!
//! Zéro coût au repos : mpv n'est lancé qu'à la première lecture, le dossier
//! n'est parcouru qu'à la première ouverture, et le minuteur de 500 ms
//! n'existe que pendant la lecture.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{App, AppContext, Context, Entity, Global};

use super::disc::{ARM_DOWN, ARM_UP, SPIN_FRAMES};
use super::engine::{Engine, Snapshot};
use super::library::{self, Track};
use super::lyrics::{self, Lyrics};

#[derive(Clone, Copy, PartialEq)]
pub enum Repeat {
    Off,
    All,
    One,
}

pub struct Player {
    pub tracks: Vec<Track>,
    pub scanned: bool,
    pub scanning: bool,
    /// File d'attente : indices dans `tracks`.
    pub queue: Vec<usize>,
    /// Position courante DANS `queue`.
    pub current: Option<usize>,
    pub playing: bool,
    pub snap: Snapshot,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub cover: Option<PathBuf>,
    pub lyrics: Option<Arc<Lyrics>>,
    pub error: Option<String>,

    engine: Option<Engine>,
    /// Vrai quand la file est terminée : « lecture » doit alors relancer le morceau.
    finished: bool,
    seen_ended: u64,
    last_sec: u64,
    tick_gen: u64,
    cover_gen: u64,
    lyrics_gen: u64,
    rng: u64,
    /// Animation du vinyle : position du label (0..18) et du bras (0..5).
    /// Les boucles (`spin_gen`, `arm_gen`) n'existent que pendant leurs
    /// transitions — zéro CPU au repos comme le reste du shell.
    pub spin_frame: usize,
    spin_gen: u64,
    pub arm_idx: usize,
    arm_gen: u64,
}

impl Player {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self {
            tracks: Vec::new(),
            scanned: false,
            scanning: false,
            queue: Vec::new(),
            current: None,
            playing: false,
            snap: Snapshot::default(),
            volume: 80,
            shuffle: false,
            repeat: Repeat::Off,
            cover: None,
            lyrics: None,
            error: None,
            engine: None,
            finished: false,
            seen_ended: 0,
            last_sec: 0,
            tick_gen: 0,
            cover_gen: 0,
            lyrics_gen: 0,
            rng: seed | 1,
            spin_frame: 0,
            spin_gen: 0,
            arm_idx: ARM_UP,
            arm_gen: 0,
        }
    }

    // ── Lecture seule pour les vues ────────────────────────────────────────
    pub fn now(&self) -> Option<&Track> {
        let idx = *self.queue.get(self.current?)?;
        self.tracks.get(idx)
    }

    // ── Bibliothèque ───────────────────────────────────────────────────────
    /// Lance le parcours du dossier musique si ce n'est pas déjà fait.
    pub fn ensure_scanned(&mut self, cx: &mut Context<Self>) {
        if !self.scanned && !self.scanning {
            self.rescan(cx);
        }
    }

    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let root = library::music_dir();
            let tracks = cx
                .background_executor()
                .spawn(async move { library::scan(&root) })
                .await;
            let _ = this.update(cx, |p, cx| {
                p.tracks = tracks;
                p.scanned = true;
                p.scanning = false;
                // La file référençait l'ancienne liste : on la vide.
                p.queue.clear();
                p.current = None;
                p.playing = false;
                p.finished = false;
                p.cover = None;
                p.lyrics = None;
                // Sans ça, mpv continuerait à jouer un titre que l'interface ne connaît plus.
                if let Some(e) = &p.engine {
                    e.stop();
                }
                p.swing_to(ARM_UP, cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ── Moteur (paresseux) ─────────────────────────────────────────────────
    fn engine(&mut self) -> &Engine {
        if self.engine.is_none() {
            let e = Engine::new();
            e.set_volume(self.volume);
            self.engine = Some(e);
        }
        self.engine.as_ref().expect("engine initialisé ci-dessus")
    }

    // ── Commandes de lecture ───────────────────────────────────────────────
    /// Remplace la file par `list` (indices dans `tracks`) et joue `list[start]`.
    pub fn play_list(&mut self, list: Vec<usize>, start: usize, cx: &mut Context<Self>) {
        if list.is_empty() {
            return;
        }
        let start = start.min(list.len() - 1);
        self.queue = list;
        if self.shuffle {
            // Le morceau choisi passe en tête, le reste est mélangé.
            self.queue.swap(0, start);
            self.shuffle_tail(1);
            self.start(0, cx);
        } else {
            self.start(start, cx);
        }
    }

    fn start(&mut self, pos: usize, cx: &mut Context<Self>) {
        let Some(&idx) = self.queue.get(pos) else {
            return;
        };
        let Some(track) = self.tracks.get(idx).cloned() else {
            return;
        };
        self.current = Some(pos);
        self.playing = true;
        self.finished = false;
        self.error = None;
        self.snap.position = 0.0;
        self.snap.duration = track.secs as f64;
        self.last_sec = 0;
        self.engine().load(&track.path);
        self.spawn_ticker(cx);
        self.spawn_spin(cx);
        self.swing_to(ARM_DOWN, cx);
        self.load_cover(track.clone(), cx);
        self.load_lyrics(track, cx);
        cx.notify();
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.current.is_none() {
            // Rien en cours : la file d'attente si elle existe, sinon toute la bibliothèque.
            let list: Vec<usize> = if self.queue.is_empty() {
                (0..self.tracks.len()).collect()
            } else {
                self.queue.clone()
            };
            self.play_list(list, 0, cx);
            return;
        }
        if self.finished {
            // La file s'est terminée : mpv est inactif, il faut recharger le titre.
            if let Some(c) = self.current {
                self.start(c, cx);
            }
            return;
        }
        self.playing = !self.playing;
        let paused = !self.playing;
        self.engine().set_paused(paused);
        self.snap.paused = paused;
        if self.playing {
            self.spawn_ticker(cx);
            self.spawn_spin(cx);
            self.swing_to(ARM_DOWN, cx);
        } else {
            self.swing_to(ARM_UP, cx);
        }
        cx.notify();
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        self.advance(false, cx);
    }

    pub fn prev(&mut self, cx: &mut Context<Self>) {
        let Some(cur) = self.current else { return };
        if self.snap.position > 3.0 || cur == 0 {
            self.seek_to(0.0, cx);
            if !self.playing {
                self.toggle(cx);
            }
        } else {
            self.start(cur - 1, cx);
        }
    }

    pub fn jump(&mut self, pos: usize, cx: &mut Context<Self>) {
        self.start(pos, cx);
    }

    pub fn seek_to(&mut self, secs: f64, cx: &mut Context<Self>) {
        let max = if self.snap.duration > 0.0 {
            self.snap.duration
        } else {
            f64::MAX
        };
        let t = secs.clamp(0.0, max);
        self.engine().seek(t);
        self.snap.position = t;
        cx.notify();
    }

    pub fn seek_rel(&mut self, delta: f64, cx: &mut Context<Self>) {
        let t = self.snap.position + delta;
        self.seek_to(t, cx);
    }

    pub fn set_volume(&mut self, pct: u8, cx: &mut Context<Self>) {
        self.volume = pct.min(100);
        let v = self.volume;
        self.engine().set_volume(v);
        cx.notify();
    }

    pub fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            let from = self.current.map(|c| c + 1).unwrap_or(0);
            self.shuffle_tail(from);
        }
        cx.notify();
    }

    pub fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        cx.notify();
    }

    // ── File d'attente ─────────────────────────────────────────────────────
    pub fn enqueue(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.tracks.len() {
            self.queue.push(idx);
            cx.notify();
        }
    }

    /// Insère juste après le morceau courant.
    pub fn play_next(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.tracks.len() {
            return;
        }
        let at = self.current.map(|c| c + 1).unwrap_or(0).min(self.queue.len());
        self.queue.insert(at, idx);
        cx.notify();
    }

    pub fn remove_from_queue(&mut self, pos: usize, cx: &mut Context<Self>) {
        if pos >= self.queue.len() || Some(pos) == self.current {
            return; // on ne retire pas le morceau en cours
        }
        self.queue.remove(pos);
        if let Some(c) = self.current {
            if pos < c {
                self.current = Some(c - 1);
            }
        }
        cx.notify();
    }

    pub fn clear_queue(&mut self, cx: &mut Context<Self>) {
        // Garde uniquement le morceau en cours.
        if let Some(c) = self.current {
            let keep = self.queue[c];
            self.queue = vec![keep];
            self.current = Some(0);
        } else {
            self.queue.clear();
        }
        cx.notify();
    }

    // ── Interne ────────────────────────────────────────────────────────────
    fn advance(&mut self, auto: bool, cx: &mut Context<Self>) {
        let Some(cur) = self.current else { return };
        if cur + 1 < self.queue.len() {
            self.start(cur + 1, cx);
        } else if self.repeat == Repeat::All && !self.queue.is_empty() {
            if self.shuffle {
                self.shuffle_tail(0);
            }
            self.start(0, cx);
        } else if auto {
            // Fin de file : on s'arrête proprement sur le dernier morceau.
            self.playing = false;
            self.finished = true;
            self.snap.position = 0.0;
            self.snap.paused = true;
            self.engine().stop();
            cx.notify();
        }
    }

    fn spawn_ticker(&mut self, cx: &mut Context<Self>) {
        self.tick_gen = self.tick_gen.wrapping_add(1);
        let g = self.tick_gen;
        cx.spawn(async move |this, cx| loop {
            async_io::Timer::after(Duration::from_millis(500)).await;
            let keep = this
                .update(cx, |p, cx| {
                    if p.tick_gen != g || !p.playing {
                        return false;
                    }
                    p.tick(cx);
                    p.playing && p.tick_gen == g
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    /// Rotation du label : 18 positions × 100 ms = 1,8 s/tour (≈ 33 tours/min).
    /// Ne tourne que pendant la lecture ; la boucle meurt dès la pause.
    fn spawn_spin(&mut self, cx: &mut Context<Self>) {
        self.spin_gen = self.spin_gen.wrapping_add(1);
        let g = self.spin_gen;
        cx.spawn(async move |this, cx| loop {
            async_io::Timer::after(Duration::from_millis(100)).await;
            let keep = this
                .update(cx, |p, cx| {
                    if p.spin_gen != g || !p.playing {
                        return false;
                    }
                    p.spin_frame = (p.spin_frame + 1) % SPIN_FRAMES;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    /// Bascule du bras vers `target` (~100 ms par cran, ≈ 700 ms au total).
    /// Un nouveau basculement invalide l'ancien via `arm_gen`.
    fn swing_to(&mut self, target: usize, cx: &mut Context<Self>) {
        let target = target.min(ARM_UP);
        self.arm_gen = self.arm_gen.wrapping_add(1);
        let g = self.arm_gen;
        if self.arm_idx == target {
            return;
        }
        cx.spawn(async move |this, cx| loop {
            async_io::Timer::after(Duration::from_millis(100)).await;
            let keep = this
                .update(cx, |p, cx| {
                    if p.arm_gen != g {
                        return false;
                    }
                    if p.arm_idx < target {
                        p.arm_idx += 1;
                    } else if p.arm_idx > target {
                        p.arm_idx -= 1;
                    }
                    cx.notify();
                    p.arm_idx != target
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let Some(snap) = self.engine.as_ref().map(Engine::snapshot) else {
            return;
        };
        if let Some(err) = snap.error.clone() {
            self.error = Some(err);
            self.playing = false;
            cx.notify();
            return;
        }
        if snap.ended != self.seen_ended {
            self.seen_ended = snap.ended;
            if self.repeat == Repeat::One {
                if let Some(c) = self.current {
                    self.start(c, cx);
                }
            } else {
                self.advance(true, cx);
            }
            return;
        }
        // On ne redessine qu'au changement de seconde (ou de pause).
        let sec = snap.position as u64;
        let changed = sec != self.last_sec || snap.paused != self.snap.paused;
        self.last_sec = sec;
        // Durée : le tag fait foi tant que mpv n'a rien annoncé.
        let duration = if snap.duration > 0.0 {
            snap.duration
        } else {
            self.snap.duration
        };
        self.snap = Snapshot { duration, ..snap };
        if changed {
            cx.notify();
        }
    }

    fn load_cover(&mut self, track: Track, cx: &mut Context<Self>) {
        self.cover_gen = self.cover_gen.wrapping_add(1);
        let g = self.cover_gen;
        self.cover = None;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { library::cover_for(&track) })
                .await;
            let _ = this.update(cx, |p, cx| {
                if p.cover_gen == g {
                    p.cover = found;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn load_lyrics(&mut self, track: Track, cx: &mut Context<Self>) {
        self.lyrics_gen = self.lyrics_gen.wrapping_add(1);
        let g = self.lyrics_gen;
        self.lyrics = None;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { lyrics::load(&track.path) })
                .await;
            let _ = this.update(cx, |p, cx| {
                if p.lyrics_gen == g {
                    p.lyrics = found.map(Arc::new);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Fisher-Yates sur `queue[from..]` (xorshift64, sans dépendance).
    fn shuffle_tail(&mut self, from: usize) {
        let n = self.queue.len();
        if from + 1 >= n {
            return;
        }
        for i in (from + 1..n).rev() {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            let j = from + (self.rng as usize) % (i - from + 1);
            self.queue.swap(i, j);
        }
    }
}

// ── Handle global ───────────────────────────────────────────────────────────
struct PlayerGlobal(Entity<Player>);
impl Global for PlayerGlobal {}

pub fn handle(cx: &mut App) -> Entity<Player> {
    if !cx.has_global::<PlayerGlobal>() {
        let e = cx.new(|_| Player::new());
        cx.set_global(PlayerGlobal(e));
    }
    cx.global::<PlayerGlobal>().0.clone()
}
