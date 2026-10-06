//! FreeDesktop Desktop Notifications service (`org.freedesktop.Notifications`).
//!
//! Replaces external notification daemons (dunst, mako, swaync) by hosting the
//! standard D-Bus notification server in the same process as the shell.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use gpui::{Context, WeakEntity, RenderImage};
use image::{Frame, ImageBuffer};
use smallvec::smallvec;
use tracing::{info, warn};
use zbus::object_server::SignalContext;
use zbus::zvariant::{OwnedValue, Value};
use zbus::Connection;

use crate::service::Service;
use crate::window::WindowControlExt;

const BUS_NAME: &str = "org.freedesktop.Notifications";
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";

/// Urgency level of a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Default for Urgency {
    fn default() -> Self {
        Urgency::Normal
    }
}

/// An interactive action attached to a notification.
#[derive(Debug, Clone)]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

/// A desktop notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub app_icon: Option<Arc<RenderImage>>,
    pub urgency: Urgency,
    pub actions: Vec<NotificationAction>,
    pub created_at: time::OffsetDateTime,
    /// Generation count for auto-dismiss timers.
    pub generation: u64,
}

/// Internal events forwarded from the D-Bus object to the GPUI service task.
enum NotificationEvent {
    Notify {
        id: u32,
        app_name: String,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    },
    Close {
        id: u32,
    },
}

/// FreeDesktop Notifications service.
pub struct Notifications {
    /// Active popups currently shown on screen (ephemeral toasts).
    toasts: Vec<Notification>,
    /// Full history of received notifications.
    history: Vec<Notification>,
    /// Do Not Disturb mode: suppresses toasts, but records in history.
    dnd: bool,
    /// Connection for emitting D-Bus signals (NotificationClosed, ActionInvoked).
    connection: Option<Connection>,
    /// Monotonic generation for toast lifecycle.
    generation: u64,
}

impl Notifications {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded();

        let service = Self {
            toasts: Vec::new(),
            history: Vec::new(),
            dnd: false,
            connection: None,
            generation: 0,
        };

        // Start D-Bus server in background
        cx.spawn(async move |_this, _cx| {
            if let Err(e) = run_dbus_server(tx).await {
                warn!("Notifications: D-Bus server error: {e}");
            }
        })
        .detach();

        // Listen for incoming notifications from D-Bus
        cx.spawn(async move |this, cx| {
            handle_incoming(this, cx, rx).await;
        })
        .detach();

        service
    }

    /// Currently active on-screen popups (toasts).
    pub fn toasts(&self) -> &[Notification] {
        &self.toasts
    }

    /// Complete notification history (newest first).
    pub fn history(&self) -> &[Notification] {
        &self.history
    }

    /// Total number of unread notifications in history.
    pub fn unread_count(&self) -> usize {
        self.history.len()
    }

    /// Whether "Do Not Disturb" is enabled.
    pub fn is_dnd(&self) -> bool {
        self.dnd
    }

    /// Toggle "Do Not Disturb" mode.
    pub fn toggle_dnd(&mut self) {
        self.dnd = !self.dnd;
        info!("Notifications: Do Not Disturb set to {}", self.dnd);
    }

    /// Dismiss a toast from the screen.
    pub fn dismiss_toast(&mut self, id: u32) {
        self.toasts.retain(|n| n.id != id);
    }

    /// Dismiss and remove a notification from history.
    pub fn remove_history(&mut self, id: u32) {
        self.history.retain(|n| n.id != id);
        self.dismiss_toast(id);
    }

    /// Clear all notification history.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.toasts.clear();
    }

    /// Invoke an action on a notification (e.g. user clicked "Reply" or "Open").
    pub fn invoke_action(&self, id: u32, action_key: &str) {
        if let Some(conn) = &self.connection {
            let conn = conn.clone();
            let key = action_key.to_string();
            async_io::block_on(async move {
                if let Ok(ctxt) = SignalContext::new(&conn, OBJECT_PATH) {
                    let _ = Server::action_invoked(&ctxt, id, &key).await;
                }
            });
        }
    }
}

impl Service for Notifications {
    fn init(cx: &mut Context<Self>) -> Self {
        Self::new(cx)
    }

    fn ipc_action(&mut self, action: &str, _payload: Option<&str>) -> bool {
        match action {
            "toggle_dnd" => {
                self.toggle_dnd();
                true
            }
            "clear" => {
                self.clear_history();
                true
            }
            _ => false,
        }
    }
}

async fn handle_incoming(
    this: WeakEntity<Notifications>,
    cx: &mut gpui::AsyncApp,
    rx: Receiver<NotificationEvent>,
) {
    while let Ok(event) = rx.recv().await {
        match event {
            NotificationEvent::Notify {
                id,
                app_name,
                app_icon,
                summary,
                body,
                actions,
                hints,
                expire_timeout,
            } => {
                let urgency = parse_urgency(&hints);
                let parsed_actions = parse_actions(&actions);
                let icon = resolve_notification_icon(&app_icon, &hints);
                let now = time::OffsetDateTime::now_utc();

                let _ = this.update(cx, |notifs, cx| {
                    notifs.generation = notifs.generation.wrapping_add(1);
                    let gen = notifs.generation;

                    let notif = Notification {
                        id,
                        app_name,
                        summary,
                        body,
                        app_icon: icon,
                        urgency,
                        actions: parsed_actions,
                        created_at: now,
                        generation: gen,
                    };

                    // Add to history (newest first, keep up to 100)
                    notifs.history.insert(0, notif.clone());
                    if notifs.history.len() > 100 {
                        notifs.history.truncate(100);
                    }

                    // Unless DND is active, push as toast popup
                    if !notifs.dnd {
                        cx.show_window("notifications");

                        // Replace existing toast if id already visible
                        notifs.toasts.retain(|t| t.id != id);
                        notifs.toasts.insert(0, notif);

                        // Schedule auto-dismiss (unless critical or -1)
                        let timeout_ms = if expire_timeout > 0 {
                            expire_timeout as u64
                        } else if urgency == Urgency::Critical {
                            0 // Critical notifications stay until user dismisses
                        } else {
                            5000 // Default 5 seconds
                        };

                        if timeout_ms > 0 {
                            cx.spawn(async move |this, cx| {
                                async_io::Timer::after(Duration::from_millis(timeout_ms)).await;
                                let _ = this.update(cx, |notifs, cx| {
                                    notifs.toasts.retain(|t| t.id != id || t.generation != gen);
                                    if notifs.toasts.is_empty() {
                                        cx.hide_window("notifications");
                                    }
                                    cx.notify();
                                });
                            })
                            .detach();
                        }
                    }

                    cx.notify();
                });
            }
            NotificationEvent::Close { id } => {
                let _ = this.update(cx, |notifs, cx| {
                    notifs.toasts.retain(|t| t.id != id);
                    if notifs.toasts.is_empty() {
                        cx.hide_window("notifications");
                    }
                    cx.notify();
                });
            }
        }
    }
}

fn parse_urgency(hints: &HashMap<String, OwnedValue>) -> Urgency {
    if let Some(val) = hints.get("urgency") {
        if let Ok(u) = u8::try_from(val) {
            return match u {
                0 => Urgency::Low,
                2 => Urgency::Critical,
                _ => Urgency::Normal,
            };
        }
    }
    Urgency::Normal
}

fn parse_actions(actions: &[String]) -> Vec<NotificationAction> {
    let mut out = Vec::new();
    for chunk in actions.chunks(2) {
        if chunk.len() == 2 {
            out.push(NotificationAction {
                key: chunk[0].clone(),
                label: chunk[1].clone(),
            });
        }
    }
    out
}

fn resolve_notification_icon(
    app_icon: &str,
    hints: &HashMap<String, OwnedValue>,
) -> Option<Arc<RenderImage>> {
    // 1. Try image-data / icon_data hints
    for key in ["image-data", "image_data", "icon_data"] {
        if let Some(val) = hints.get(key) {
            if let Some(img) = decode_image_data_hint(val) {
                return Some(img);
            }
        }
    }

    // 2. Try app_icon as file or theme icon
    if !app_icon.is_empty() {
        if let Some(img) = crate::apps::load_icon_pub(app_icon) {
            return Some(img);
        }
    }

    None
}

/// Decode raw image structure: `(iiibiiay)`
fn decode_image_data_hint(val: &OwnedValue) -> Option<Arc<RenderImage>> {
    let value = Value::try_from(val).ok()?;
    let Value::Structure(struct_val) = value else {
        return None;
    };
    let mut fields = struct_val.into_fields();
    if fields.len() < 7 {
        return None;
    }

    let width = i32::try_from(fields.remove(0)).ok()?;
    let height = i32::try_from(fields.remove(0)).ok()?;
    let _rowstride = i32::try_from(fields.remove(0)).ok()?;
    let has_alpha = bool::try_from(fields.remove(0)).ok()?;
    let _bits_per_sample = i32::try_from(fields.remove(0)).ok()?;
    let _channels = i32::try_from(fields.remove(0)).ok()?;
    let data_val = fields.remove(0);

    let Value::Array(byte_array) = data_val else {
        return None;
    };
    let bytes: Vec<u8> = byte_array
        .into_iter()
        .filter_map(|v| u8::try_from(v).ok())
        .collect();

    if width <= 0 || height <= 0 {
        return None;
    }
    let (w, h) = (width as u32, height as u32);

    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    let chunk_size = if has_alpha { 4 } else { 3 };

    for chunk in bytes.chunks_exact(chunk_size) {
        if has_alpha {
            bgra.extend_from_slice(&[chunk[2], chunk[1], chunk[0], chunk[3]]);
        } else {
            bgra.extend_from_slice(&[chunk[2], chunk[1], chunk[0], 255]);
        }
    }

    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}

/// D-Bus interface implementation for `org.freedesktop.Notifications`.
struct Server {
    tx: Sender<NotificationEvent>,
    next_id: AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "actions".into(),
            "body".into(),
            "body-markup".into(),
            "icon-static".into(),
            "persistence".into(),
        ]
    }

    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "WidgetKit Notifications".into(),
            "WidgetKit Contributors".into(),
            "0.1.0".into(),
            "1.2".into(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id > 0 {
            replaces_id
        } else {
            self.next_id.fetch_add(1, Ordering::SeqCst)
        };

        let _ = self
            .tx
            .send(NotificationEvent::Notify {
                id,
                app_name,
                app_icon,
                summary,
                body,
                actions,
                hints,
                expire_timeout,
            })
            .await;

        id
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.tx.send(NotificationEvent::Close { id }).await;
    }

    #[zbus(signal)]
    async fn notification_closed(
        ctxt: &SignalContext<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        ctxt: &SignalContext<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

async fn run_dbus_server(tx: Sender<NotificationEvent>) -> zbus::Result<()> {
    let server = Server {
        tx,
        next_id: AtomicU32::new(1),
    };

    let connection = Connection::session().await?;
    connection.object_server().at(OBJECT_PATH, server).await?;

    match connection.request_name(BUS_NAME).await {
        Ok(_) => info!("Notifications: hosted '{BUS_NAME}' successfully"),
        Err(e) => {
            warn!(
                "Notifications: could not claim '{BUS_NAME}' ({e}). An existing daemon (e.g. mako/dunst) may be running."
            );
        }
    }

    // Keep connection alive
    futures_util::future::pending::<()>().await;
    Ok(())
}
