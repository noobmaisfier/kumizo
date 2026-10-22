//! Aurora — shell Wayland complet sur Kumizo (un seul process, un seul GPU).
//!
//! Fenêtres (ids préfixés `aurora-`, pilotables via `kumizo toggle <id>`) :
//!   aurora-bar       barre flottante (toujours visible)
//!   aurora-launcher  lanceur d'apps            (plein écran, clavier exclusif)
//!   aurora-music     lecteur de musique locale (plein écran, clavier exclusif)
//!   aurora-tasks     fenêtres ouvertes         (plein écran, clavier exclusif)
//!   aurora-power     menu d'alimentation       (plein écran, clavier exclusif)
//!   aurora-control   centre de contrôle        (plein écran transparent, panneau à droite)
//!   aurora-notifs    historique notifications  (plein écran transparent, panneau à droite)
//!   aurora-toasts    popups de notification    (coin haut-droit, ouvert par la barre)
//!   aurora-osd       volume / luminosité       (bas-centre, ouvert par la barre)

mod bar;
mod clipboard;
mod control;
mod icons;
mod launcher;
/// Widget musique : code conservé et compilé, mais débranché du shell.
/// Réactiver = passer `MUSIC_ENABLED` à `true` (fenêtre + pastille barre).
mod music;
mod notifs;
mod osd;
mod pomodoro;
mod power;
mod tasks;
mod theme;
mod theme_ctl;
mod toasts;
mod ui;

/// Interrupteur du widget musique. À `false` : ni fenêtre `aurora-music`,
/// ni pastille ♪, ni thread audio — le code reste compilé pour une
/// réactivation en un basculement.
pub const MUSIC_ENABLED: bool = true;

use gpui::prelude::*;
use kumizo_kit::prelude::*;

use ui::ids;

/// Surface plein écran transparente/semi-opaque au-dessus de tout.
fn fullscreen(id: &'static str, namespace: &'static str, keyboard: KeyboardInteractivity) -> LayerWindow {
    LayerWindow::new(id)
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(keyboard)
        .transparent(true)
        .start_hidden(true)
        .namespace(namespace)
}

fn main() {
    theme::init();

    let app = Kumizo::new()
        // ── Services (uniquement ceux que les vues consomment) ──────────────
        .service::<Clock>()
        .service::<Battery>()
        .service::<Memory>()
        .service::<Audio>()
        .service::<Brightness>()
        .service::<Bluetooth>()
        .service::<Network>()
        .service::<Compositor>()
        .service::<Apps>()
        .service::<Clipboard>()
        .service::<Mpris>()
        .service::<Tray>()
        .service::<Notifications>()
        .service::<theme_ctl::Theme>()
        .service::<music::Music>()
        // ── Barre ───────────────────────────────────────────────────────────
        .window(
            LayerWindow::new(ids::BAR)
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(50)
                .size(None, Some(50.0))
                .keyboard_interactivity(KeyboardInteractivity::None)
                .transparent(true)
                .namespace("aurora-bar"),
            |cx: &mut gpui::App| cx.new(bar::TopBar::new),
        )
        // ── Panneaux latéraux (fond transparent cliquable pour fermer) ──────
        .window(
            fullscreen(ids::CONTROL, "aurora-control", KeyboardInteractivity::OnDemand),
            |cx: &mut gpui::App| cx.new(control::ControlCenter::new),
        )
        .window(
            fullscreen(ids::NOTIFS, "aurora-notifs", KeyboardInteractivity::OnDemand),
            |cx: &mut gpui::App| cx.new(notifs::NotificationCenter::new),
        )
        // ── Modales plein écran ─────────────────────────────────────────────
        .window(
            fullscreen(ids::LAUNCHER, "aurora-launcher", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(launcher::AppLauncher::new),
        )
        .window(
            fullscreen(ids::TASKS, "aurora-tasks", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(tasks::TaskSwitcher::new),
        )
        .window(
            fullscreen(ids::POWER, "aurora-power", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(power::PowerMenu::new),
        )
        .window(
            fullscreen(ids::CLIPBOARD, "aurora-clipboard", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(clipboard::ClipboardPanel::new),
        );
    // ── Musique : débranchée (`MUSIC_ENABLED = false`), code conservé ──────
    let app = if MUSIC_ENABLED {
        app.window(
            fullscreen(ids::MUSIC, "aurora-music", KeyboardInteractivity::Exclusive),
            |cx: &mut gpui::App| cx.new(music::MusicPanel::new),
        )
    } else {
        app
    };
    app
        // ── Toasts (ouverts/fermés par la barre selon `toasts()`) ───────────
        .window(
            LayerWindow::new(ids::TOASTS)
                .layer(Layer::Overlay)
                .anchor(Anchor::TOP | Anchor::RIGHT)
                .exclusive_zone(0)
                .size(Some(380.0), Some(440.0))
                .margins(Margins::new(58, 12, 0, 0))
                .keyboard_interactivity(KeyboardInteractivity::None)
                .transparent(true)
                .start_hidden(true)
                .namespace("aurora-toasts"),
            |cx: &mut gpui::App| cx.new(toasts::NotificationToasts::new),
        )
        // ── OSD volume / luminosité (ouvert par la barre) ───────────────────
        .window(
            LayerWindow::new(ids::OSD)
                .layer(Layer::Overlay)
                .anchor(Anchor::BOTTOM)
                .exclusive_zone(0)
                .size(Some(300.0), Some(64.0))
                .margins(Margins::new(0, 0, 90, 0))
                .keyboard_interactivity(KeyboardInteractivity::None)
                .transparent(true)
                .start_hidden(true)
                .namespace("aurora-osd"),
            |cx: &mut gpui::App| cx.new(osd::Osd::new),
        )
        .run();
}
