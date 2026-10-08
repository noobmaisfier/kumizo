//! Pomodoro : modèle partagé (barre + centre de contrôle).
//!
//! Zéro CPU au repos : la tâche de tick (1 Hz) n'existe QUE quand le minuteur
//! tourne. Un jeton de génération annule proprement les anciennes tâches.

use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, Global, Rgba};

use crate::{theme::pal, ui};

const FOCUS_SECS: u32 = 25 * 60;
const SHORT_SECS: u32 = 5 * 60;
const LONG_SECS: u32 = 15 * 60;
/// Une pause longue toutes les N sessions de focus.
const LONG_EVERY: u32 = 4;

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn secs(self) -> u32 {
        match self {
            Phase::Focus => FOCUS_SECS,
            Phase::ShortBreak => SHORT_SECS,
            Phase::LongBreak => LONG_SECS,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Focus",
            Phase::ShortBreak => "Pause courte",
            Phase::LongBreak => "Pause longue",
        }
    }

    pub fn color(self) -> Rgba {
        match self {
            Phase::Focus => pal::accent(),
            Phase::ShortBreak => pal::ok(),
            Phase::LongBreak => pal::accent2(),
        }
    }
}

pub struct Pomodoro {
    pub phase: Phase,
    pub remaining: u32,
    pub running: bool,
    /// Sessions de focus terminées.
    pub sessions: u32,
    generation: u64,
}

impl Pomodoro {
    fn new() -> Self {
        Self {
            phase: Phase::Focus,
            remaining: FOCUS_SECS,
            running: false,
            sessions: 0,
            generation: 0,
        }
    }

    pub fn mmss(&self) -> String {
        format!("{:02}:{:02}", self.remaining / 60, self.remaining % 60)
    }

    /// 0.0 (début) → 1.0 (fin de phase).
    pub fn progress(&self) -> f32 {
        1.0 - self.remaining as f32 / self.phase.secs() as f32
    }

    /// Nombre de pastilles pleines dans le cycle courant (0..=4).
    pub fn cycle_done(&self) -> u32 {
        if self.sessions > 0 && self.sessions % LONG_EVERY == 0 {
            LONG_EVERY
        } else {
            self.sessions % LONG_EVERY
        }
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.running = !self.running;
        self.generation = self.generation.wrapping_add(1);
        if self.running {
            self.spawn_ticker(cx);
        }
        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.running = false;
        self.generation = self.generation.wrapping_add(1);
        self.remaining = self.phase.secs();
        cx.notify();
    }

    /// Termine la phase en cours et passe à la suivante (sans notification).
    pub fn skip(&mut self, cx: &mut Context<Self>) {
        self.advance(false);
        cx.notify();
    }

    fn spawn_ticker(&mut self, cx: &mut Context<Self>) {
        let g = self.generation;
        cx.spawn(async move |this, cx| loop {
            async_io::Timer::after(Duration::from_secs(1)).await;
            let alive = this
                .update(cx, |p, cx| {
                    if p.generation != g || !p.running {
                        return false;
                    }
                    p.tick(cx);
                    p.running && p.generation == g
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        })
        .detach();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            self.advance(true);
        }
        cx.notify();
    }

    fn advance(&mut self, announce: bool) {
        let next = match self.phase {
            Phase::Focus => {
                self.sessions += 1;
                if self.sessions % LONG_EVERY == 0 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            _ => Phase::Focus,
        };
        self.phase = next;
        self.remaining = next.secs();
        self.running = false;
        self.generation = self.generation.wrapping_add(1);

        if announce {
            let (title, body) = match next {
                Phase::Focus => ("Retour au focus", "La pause est terminée."),
                Phase::ShortBreak => ("Pause courte", "Bien joué — 5 minutes pour respirer."),
                Phase::LongBreak => ("Pause longue", "Cycle terminé — 15 minutes de repos."),
            };
            // Passe par le serveur de notifications du shell lui-même.
            ui::run("notify-send", &["-a", "Aurora Pomodoro", title, body]);
            ui::run(
                "paplay",
                &["/usr/share/sounds/freedesktop/stereo/complete.oga"],
            );
        }
    }
}

// ── Handle global (une seule instance pour toutes les fenêtres) ─────────────
struct PomodoroGlobal(Entity<Pomodoro>);
impl Global for PomodoroGlobal {}

pub fn handle(cx: &mut App) -> Entity<Pomodoro> {
    if !cx.has_global::<PomodoroGlobal>() {
        let e = cx.new(|_| Pomodoro::new());
        cx.set_global(PomodoroGlobal(e));
    }
    cx.global::<PomodoroGlobal>().0.clone()
}
