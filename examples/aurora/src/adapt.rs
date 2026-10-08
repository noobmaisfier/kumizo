//! ═══════════════════════════════════════════════════════════════════════════
//!  Couche d'adaptation WidgetKit ⇄ Aurora
//! ═══════════════════════════════════════════════════════════════════════════
//!
//!  Le guide WidgetKit documente les MÉTHODES des services mais pas les champs
//!  de certaines structures (périphérique Bluetooth, `AppInfo`, `TrayItem`,
//!  variantes de `PlaybackStatus`/`Urgency`).
//!
//!  Toute hypothèse sur ces types est isolée dans CE fichier. Si
//!  `cargo check` se plaint d'un champ ou d'un type, c'est ici — et seulement
//!  ici — qu'il faut ajuster (ouvre `wk-services` pour voir les vrais noms).

use std::sync::Arc;

use gpui::RenderImage;
use widgetkit::prelude::*;

// ── Bluetooth ───────────────────────────────────────────────────────────────
pub struct BtDevice {
    pub name: String,
    pub path: String,
    pub connected: bool,
}

/// HYPOTHÈSE : chaque périphérique expose `name`, `path`, `connected`.
pub fn bt_devices(bt: &Bluetooth) -> Vec<BtDevice> {
    bt.devices()
        .iter()
        .map(|d| BtDevice {
            name: d.name.clone(),
            path: d.path.clone(),
            connected: d.connected,
        })
        .collect()
}

/// HYPOTHÈSE : le 2ᵉ argument est l'état de connexion ACTUEL.
pub fn bt_toggle(bt: &Bluetooth, d: &BtDevice) {
    bt.toggle_device_connection(d.path.clone(), d.connected);
}

// ── Media ───────────────────────────────────────────────────────────────────
/// HYPOTHÈSE : `PlaybackStatus::Playing` existe.
pub fn is_playing(m: &Mpris) -> bool {
    matches!(m.status(), PlaybackStatus::Playing)
}

// ── Applications ────────────────────────────────────────────────────────────
/// HYPOTHÈSE : `AppInfo` a `name: String` et `icon: Option<Arc<RenderImage>>` et est `Clone`.
pub fn app_name(a: &AppInfo) -> String {
    a.name.clone()
}

pub fn app_icon(a: &AppInfo) -> Option<Arc<RenderImage>> {
    a.icon.clone()
}

/// Résultats du lanceur : 8 premières apps (requête vide) ou recherche floue.
pub fn app_results(apps: &Apps, query: &str) -> Vec<AppInfo> {
    if query.is_empty() {
        apps.apps().iter().take(8).cloned().collect()
    } else {
        apps.search(query).into_iter().take(8).collect()
    }
}

/// Icône d'une fenêtre ouverte à partir de son app_id / class.
pub fn window_icon(apps: &Apps, class: &str) -> Option<Arc<RenderImage>> {
    apps.find_icon(class)
}

// ── Tray ────────────────────────────────────────────────────────────────────
pub struct TrayEntry {
    pub id: String,
    pub icon: Option<Arc<RenderImage>>,
}

/// HYPOTHÈSE : `TrayItem` a `id: String` et `icon: Option<Arc<RenderImage>>`.
pub fn tray_entries(t: &Tray) -> Vec<TrayEntry> {
    t.items()
        .iter()
        .map(|i| TrayEntry {
            id: i.id.clone(),
            icon: i.icon.clone(),
        })
        .collect()
}

pub fn tray_activate(t: &mut Tray, id: &str) {
    t.activate(id);
}

// ── Notifications ───────────────────────────────────────────────────────────
pub struct NotifData {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: Option<Arc<RenderImage>>,
    pub critical: bool,
    /// (clé, libellé) — l'action "default" est exclue (clic sur la carte).
    pub actions: Vec<(String, String)>,
    pub time: String,
}

/// HYPOTHÈSE : `Urgency` est exporté par le prelude ; `created_at` est local.
pub fn notif_data(n: &Notification) -> NotifData {
    NotifData {
        id: n.id,
        app: n.app_name.clone(),
        summary: n.summary.clone(),
        body: n.body.clone(),
        icon: n.app_icon.clone(),
        critical: matches!(n.urgency, Urgency::Critical),
        actions: n
            .actions
            .iter()
            .filter(|a| a.key != "default")
            .map(|a| (a.key.clone(), a.label.clone()))
            .collect(),
        time: format!("{:02}:{:02}", n.created_at.hour(), n.created_at.minute()),
    }
}
