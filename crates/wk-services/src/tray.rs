use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use gpui::{Context, RenderImage};
use image::{Frame, ImageBuffer};
use smallvec::smallvec;
use tracing::warn;
use zbus::message::Header;
use zbus::object_server::SignalContext;
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use wk_ipc::WindowControl;
use crate::service::Service;

/// Well-known name of the tray host. Owning it makes this daemon the
/// destination of every `RegisterStatusNotifierItem` call.
const WATCHER_BUS: &str = "org.kde.StatusNotifierWatcher";
/// Object path of the watcher, per the StatusNotifier specification.
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// Name of the context-menu window (declared by the application builder).
const MENU_WINDOW: &str = "traymenu";

/// Item visibility as reported by the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Active,
    Passive,
    NeedsAttention,
}

impl TrayStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "Passive" => TrayStatus::Passive,
            "NeedsAttention" => TrayStatus::NeedsAttention,
            _ => TrayStatus::Active,
        }
    }
}

/// One visible tray icon: decoded pixels plus addressing info.
#[derive(Debug, Clone)]
pub struct TrayItem {
    /// Registry key (`bus + path`), used for clicks.
    pub id: String,
    /// Human-readable title (tooltip).
    pub title: String,
    /// Decoded icon, ready for `gpui::img`. `None` while loading.
    pub icon: Option<Arc<RenderImage>>,
    /// Item wants attention (alert icon state).
    pub attention: bool,
}

/// One context-menu entry, recursively.
#[derive(Debug, Clone)]
pub struct TrayMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    /// `Some(checked)` for checkmark/radio items.
    pub toggle: Option<bool>,
    pub children: Vec<TrayMenuItem>,
}

/// The currently open context menu, if any.
#[derive(Debug, Clone)]
pub struct OpenMenu {
    /// Registry key of the owning item.
    pub owner: String,
    pub title: String,
    pub items: Vec<TrayMenuItem>,
}

#[derive(Debug, Clone)]
struct TrackedItem {
    bus: String,
    path: String,
    title: String,
    status: TrayStatus,
    icon: Option<Arc<RenderImage>>,
    menu_path: Option<String>,
}

/// System tray host (StatusNotifierItem watcher + item tracker).
///
/// Owns `org.kde.StatusNotifierWatcher`, decodes item pixmaps into GPU
/// textures, and fetches dbusmenu layouts on demand. The topbar renders
/// [`Tray::items`]; right-click opens the [`Tray::menu`] in the `traymenu`
/// window via [`WindowControl`], so no surface exists while closed.
#[derive(Debug, Default)]
pub struct Tray {
    items: HashMap<String, TrackedItem>,
    /// Insertion order, for a stable icon row.
    order: Vec<String>,
    open_menu: Option<OpenMenu>,
    control: Option<WindowControl>,
}

impl Tray {
    /// Visible icons in registration order (`Passive` items are hidden,
    /// per the specification).
    pub fn items(&self) -> Vec<TrayItem> {
        self.order
            .iter()
            .filter_map(|key| self.items.get(key))
            .filter(|it| it.status != TrayStatus::Passive)
            .map(|it| TrayItem {
                id: key_of(&it.bus, &it.path),
                title: it.title.clone(),
                icon: it.icon.clone(),
                attention: it.status == TrayStatus::NeedsAttention,
            })
            .collect()
    }

    /// The open context menu, if any.
    pub fn menu(&self) -> Option<OpenMenu> {
        self.open_menu.clone()
    }

    /// Short human-readable inventory, for `widgetctl service tray list`.
    fn describe(&self) -> String {
        if self.items.is_empty() {
            return String::from("no tray items");
        }
        self.order
            .iter()
            .filter_map(|k| self.items.get(k))
            .map(|it| {
                format!(
                    "{} [{}]{}",
                    it.title,
                    match it.status {
                        TrayStatus::Active => "active",
                        TrayStatus::Passive => "passive",
                        TrayStatus::NeedsAttention => "attention",
                    },
                    if it.menu_path.is_some() { " +menu" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Left-click: ask the application to activate (open its window).
    ///
    /// `id` is the registry key, but a bus name or title fragment also
    /// resolves (the key embeds a NUL byte, impractical on a CLI).
    pub fn activate(&self, id: &str) {
        let Some(it) = self.resolve(id) else {
            return;
        };
        let (bus, path) = (it.bus.clone(), it.path.clone());
        async_io::block_on(async move {
            let Ok(conn) = session_conn().await else {
                return;
            };
            let Ok(proxy) = zbus::Proxy::new(
                &conn,
                bus.as_str(),
                path.as_str(),
                "org.kde.StatusNotifierItem",
            )
            .await
            else {
                return;
            };
            // Coordinates are informational; 0,0 is accepted.
            let _ = proxy.call_method("Activate", &(0i32, 0i32)).await;
        });
    }

    /// Right-click: fetch the dbusmenu layout and open the menu window.
    ///
    /// Spec flow: `ContextMenu(x, y)` first (the item may populate or reveal
    /// its menu on demand), then re-read `Menu`, then `AboutToShow` +
    /// `GetLayout`. Skipping the call works for Qt/Telegram but an item that
    /// only honors `ContextMenu` would show nothing.
    pub fn open_menu(&mut self, id: &str) {
        let key = self.resolve_key(id);
        let Some(it) = key.as_ref().and_then(|k| self.items.get(k)) else {
            return;
        };
        let (bus, item_path, title) = (it.bus.clone(), it.path.clone(), it.title.clone());
        let bus_warn = bus.clone();
        let items = async_io::block_on(async move {
            let Ok(conn) = session_conn().await else {
                return None;
            };
            let Ok(item) = zbus::Proxy::new(
                &conn,
                bus.as_str(),
                item_path.as_str(),
                "org.kde.StatusNotifierItem",
            )
            .await
            else {
                return None;
            };
            // Hint, fire-and-forget: unknown method on items that ignore it.
            let _ = item.call_method("ContextMenu", &(0i32, 0i32)).await;
            let menu_path: Option<String> = item
                .get_property::<OwnedObjectPath>("Menu")
                .await
                .ok()
                .map(|p| p.to_string())
                .filter(|p| p != "/");
            let Some(menu_path) = menu_path else {
                return None;
            };
            fetch_menu(&bus, &menu_path).await
        });
        let Some(items) = items else {
            warn!("Tray: no menu layout from {bus_warn}");
            return;
        };
        self.open_menu = Some(OpenMenu {
            owner: key.unwrap(),
            title,
            items,
        });
        if let Some(control) = &self.control {
            control.show(MENU_WINDOW);
        }
    }

    /// Close the menu window and forget its items.
    pub fn close_menu(&mut self) {
        if self.open_menu.is_none() {
            return;
        }
        self.open_menu = None;
        if let Some(control) = &self.control {
            control.hide(MENU_WINDOW);
        }
    }

    /// Click a menu entry: fire `clicked` then close.
    pub fn click_menu_item(&mut self, item_id: i32) {
        let Some(menu) = self.open_menu.clone() else {
            return;
        };
        let Some(it) = self.items.get(&menu.owner) else {
            self.close_menu();
            return;
        };
        if it.menu_path.is_none() {
            self.close_menu();
            return;
        }
        let (bus, path) = (it.bus.clone(), it.menu_path.clone().unwrap());
        async_io::block_on(async move {
            let Ok(conn) = session_conn().await else {
                return;
            };
            let Ok(proxy) = zbus::Proxy::new(
                &conn,
                bus.as_str(),
                path.as_str(),
                "com.canonical.dbusmenu",
            )
            .await
            else {
                return;
            };
            let result = proxy
                .call_method("Event", &(item_id, "clicked", Value::I32(0), 0u32))
                .await;
            if let Err(err) = result {
                warn!("Tray: Event({item_id}) to {bus} {path} failed: {err}");
            }
        });
        self.close_menu();
    }
}

fn key_of(bus: &str, path: &str) -> String {
    format!("{bus}\u{0}{path}")
}

impl Tray {
    /// Resolve a registry key, a bus name or a title fragment to a key.
    fn resolve_key(&self, id: &str) -> Option<String> {
        if self.items.contains_key(id) {
            return Some(id.to_string());
        }
        self.order
            .iter()
            .find(|k| {
                self.items.get(*k).is_some_and(|it| {
                    it.bus == id || it.title.contains(id)
                })
            })
            .cloned()
    }

    /// Resolve to the tracked item itself.
    fn resolve(&self, id: &str) -> Option<&TrackedItem> {
        self.resolve_key(id).and_then(|k| self.items.get(&k))
    }
}

async fn session_conn() -> zbus::Result<Connection> {
    Connection::session().await
}

/// Fetch and parse a full dbusmenu layout: `AboutToShow(0)`, then
/// `GetLayout(0, -1, [])`.
async fn fetch_menu(bus: &str, path: &str) -> Option<Vec<TrayMenuItem>> {
    let conn = session_conn().await.ok()?;
    let proxy = zbus::Proxy::new(&conn, bus, path, "com.canonical.dbusmenu")
        .await
        .ok()?;
    // Refresh dynamic menus before reading, per the specification workflow.
    let _ = proxy.call_method("AboutToShow", &0i32).await;
    let reply = proxy
        .call_method("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .await
        .ok()?;
    let (_revision, layout): (u32, LayoutNode) = match reply.body().deserialize() {
        Ok(v) => v,
        Err(err) => {
            warn!("Tray: GetLayout from {bus} {path} did not parse: {err}");
            return None;
        }
    };
    let items: Vec<TrayMenuItem> = layout
        .children
        .into_iter()
        .filter_map(|c| match LayoutNode::try_from(c) {
            Ok(node) => Some(node),
            Err(err) => {
                warn!("Tray: child of {bus} {path} unusable: {err}");
                None
            }
        })
        .filter_map(TrayMenuItem::from_node)
        .collect();
    tracing::info!(
        "Tray: menu {bus} {path}: {}",
        items
            .iter()
            .map(|it| format!("{}:{}{}", it.id, it.label, if it.enabled { "" } else { " (off)" }))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Some(items)
}

/// Raw dbusmenu layout node: `(ia{sv}av)`. Children arrive boxed in variants
/// (D-Bus forbids direct recursion), hence `Vec<OwnedValue>` plus a manual
/// unboxing impl below (no blanket `TryFrom<OwnedValue>` exists for custom
/// structs in zvariant 4).
#[derive(Debug, serde::Deserialize, zbus::zvariant::Type)]
struct LayoutNode {
    id: i32,
    properties: HashMap<String, OwnedValue>,
    children: Vec<OwnedValue>,
}

impl<'a> TryFrom<Value<'a>> for LayoutNode {
    type Error = zbus::zvariant::Error;

    fn try_from(value: Value<'a>) -> Result<Self, Self::Error> {
        use zbus::zvariant::Error;
        // Two shapes reach this function, depending on whether the node came
        // from a `v` box (children of `av`) or was already unboxed (the root
        // returned by `deserialize`). Accept both rather than guessing which
        // caller produced the value.
        let value = match value {
            Value::Value(boxed) => *boxed,
            other => other,
        };
        let Value::Structure(node) = value else {
            return Err(Error::IncorrectType);
        };
        let mut fields = node.into_fields();
        if fields.len() != 3 {
            return Err(Error::IncorrectType);
        }
        let children = fields.pop().unwrap();
        let properties = fields.pop().unwrap();
        let id = fields.pop().unwrap();
        Ok(LayoutNode {
            id: i32::try_from(id)?,
            properties: HashMap::<String, OwnedValue>::try_from(properties)?,
            children: Vec::<OwnedValue>::try_from(children)?,
        })
    }
}

impl TryFrom<OwnedValue> for LayoutNode {
    type Error = zbus::zvariant::Error;

    fn try_from(value: OwnedValue) -> Result<Self, Self::Error> {
        LayoutNode::try_from(Value::try_from(&value)?)
    }
}

impl TrayMenuItem {
    fn from_node(node: LayoutNode) -> Option<Self> {
        let mut props = node.properties;
        let get_str = |props: &mut HashMap<String, OwnedValue>, key: &str| -> String {
            props
                .remove(key)
                .and_then(|v| String::try_from(v).ok())
                .unwrap_or_default()
        };
        let get_bool =
            |props: &mut HashMap<String, OwnedValue>, key: &str, default: bool| -> bool {
                props
                    .remove(key)
                    .and_then(|v| bool::try_from(v).ok())
                    .unwrap_or(default)
            };

        let kind = get_str(&mut props, "type");
        if kind == "separator" {
            return Some(TrayMenuItem {
                id: node.id,
                label: String::new(),
                enabled: false,
                separator: true,
                toggle: None,
                children: Vec::new(),
            });
        }
        if !get_bool(&mut props, "visible", true) {
            return None;
        }
        let toggle = match get_str(&mut props, "toggle-type").as_str() {
            "checkmark" | "radio" => Some(
                props
                    .remove("toggle-state")
                    .and_then(|v| i32::try_from(v).ok())
                    .map(|s| s == 1)
                    .unwrap_or(false),
            ),
            _ => None,
        };
        let children = node
            .children
            .into_iter()
            .filter_map(|c| LayoutNode::try_from(c).ok())
            .filter_map(TrayMenuItem::from_node)
            .collect();
        Some(TrayMenuItem {
            id: node.id,
            label: clean_label(&get_str(&mut props, "label")),
            enabled: get_bool(&mut props, "enabled", true),
            separator: false,
            toggle,
            children,
        })
    }
}

/// Strip mnemonics: single `_` marks the hotkey, `__` is a literal `_`.
fn clean_label(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '_' {
            match chars.next() {
                Some('_') => out.push('_'),
                Some(next) => out.push(next),
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert SNI `IconPixmap` ARGB bytes to a GPU texture.
///
/// The pixmap arrives as `(width, height, bytes)` with bytes in A,R,G,B
/// order; GPUI's `RenderImage` expects B,G,R,A, so each word is reversed.
fn pixmap_to_image(width: i32, height: i32, argb: &[u8]) -> Option<Arc<RenderImage>> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let (w, h) = (width as u32, height as u32);
    if argb.len() != (w as usize) * (h as usize) * 4 {
        return None;
    }
    let mut bgra = Vec::with_capacity(argb.len());
    for px in argb.chunks_exact(4) {
        bgra.extend_from_slice(&[px[3], px[2], px[1], px[0]]);
    }
    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}

/// Best-effort `IconName` lookup: `<name>.png` in the standard theme dirs.
/// SVG names are skipped (no renderer on board).
fn lookup_icon_file(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.contains('/') {
        // Absolute or relative path: only PNG is decodable here.
        let p = PathBuf::from(format!("{name}.png"));
        if p.is_file() {
            return Some(p);
        }
        let p = PathBuf::from(name);
        if p.is_file() && p.extension().is_some_and(|e| e == "png") {
            return Some(p);
        }
        return None;
    }
    let mut dirs = vec![PathBuf::from("/usr/share/icons"), PathBuf::from("/usr/share/pixmaps")];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(&home).join(".icons"));
        let xdg = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".local/share"));
        dirs.push(xdg.join("icons"));
    }
    for dir in &dirs {
        for size in ["22x22", "24x24", "32x32", "48x48"] {
            let p = dir
                .join("hicolor")
                .join(size)
                .join("apps")
                .join(format!("{name}.png"));
            if p.is_file() {
                return Some(p);
            }
        }
        let p = dir.join(format!("{name}.png"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn load_png(path: &PathBuf) -> Option<Arc<RenderImage>> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    // `to_rgba8` yields R,G,B,A; GPUI wants B,G,R,A.
    let mut bgra = img.into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buf: ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![Frame::new(buf)])))
}

/// Read one item's properties. Every step is optional: a missing property
/// degrades the item instead of dropping it.
async fn fetch_item(conn: &Connection, bus: &str, path: &str) -> Option<TrackedItem> {
    let proxy = zbus::Proxy::new(conn, bus, path, "org.kde.StatusNotifierItem")
        .await
        .ok()?;
    let title: String = proxy.get_property("Title").await.unwrap_or_default();
    let title = if title.is_empty() {
        proxy.get_property::<String>("Id").await.unwrap_or_default()
    } else {
        title
    };
    let status = TrayStatus::from_str(&proxy.get_property::<String>("Status").await.unwrap_or_default());
    let menu_path: Option<String> = proxy
        .get_property::<OwnedObjectPath>("Menu")
        .await
        .ok()
        .map(|p| p.to_string())
        .filter(|p| p != "/");

    // Pixmap first (exact pixels, no theme needed), IconName as fallback.
    let mut icon = None;
    if let Ok(pixmaps) = proxy
        .get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap")
        .await
    {
        // Largest pixmap wins; tray sizes are small either way.
        if let Some((w, h, bytes)) = pixmaps
            .iter()
            .max_by_key(|(w, h, b)| (*w as usize) * (*h as usize) * b.len())
        {
            icon = pixmap_to_image(*w, *h, bytes);
        }
    }
    if icon.is_none() {
        if let Ok(name) = proxy.get_property::<String>("IconName").await {
            icon = lookup_icon_file(&name).and_then(|p| load_png(&p));
        }
    }

    Some(TrackedItem {
        bus: bus.to_string(),
        path: path.to_string(),
        title,
        status,
        icon,
        menu_path,
    })
}

/// Events from the D-Bus watcher object to the service task.
enum TrayBusEvent {
    ItemRegistered { bus: String, path: String },
}

/// The `org.kde.StatusNotifierWatcher` object. Methods only forward to the
/// service task; all state lives in the [`Tray`] entity.
struct Watcher {
    tx: async_channel::Sender<TrayBusEvent>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
        service: String,
    ) {
        // `service` is either a bus name (object is `/StatusNotifierItem`)
        // or an object path (bus is the sender).
        let (bus, path) = if let Some(path) = service.strip_prefix('/') {
            let sender = hdr
                .sender()
                .map(|s| s.to_string())
                .unwrap_or_default();
            (sender, format!("/{path}").replace("//", "/"))
        } else {
            (service.clone(), String::from("/StatusNotifierItem"))
        };
        if bus.is_empty() {
            return;
        }
        let _ = self
            .tx
            .send(TrayBusEvent::ItemRegistered { bus: bus.clone(), path: path.clone() })
            .await;
        let _ = Self::status_notifier_item_registered(&ctxt, service.as_str()).await;
    }

    async fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    async fn registered_status_notifier_items(&self) -> Vec<String> {
        // Membership is tracked in the service task; items never read this.
        Vec::new()
    }

    #[zbus(property)]
    async fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn protocol_version(&self) -> String {
        String::from("0")
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        ctxt: &SignalContext<'_>,
        service: &str,
    ) -> zbus::Result<()>;
}

impl Service for Tray {
    /// Actions for `widgetctl service tray …` (same code paths as clicks):
    /// `list`, `activate <id>`, `menu-open <id>`, `menu-click <id>`,
    /// `menu-close`. Item ids come from the topbar tooltips; `list` prints
    /// the inventory to the daemon log.
    fn ipc_action(&mut self, action: &str, payload: Option<&str>) -> bool {
        match action {
            "list" => {
                tracing::info!("Tray items: {}", self.describe());
                true
            }
            "activate" => {
                if let Some(id) = payload {
                    self.activate(id);
                    true
                } else {
                    false
                }
            }
            "menu-open" => {
                if let Some(id) = payload {
                    self.open_menu(id);
                    true
                } else {
                    false
                }
            }
            "menu-click" => {
                if let Some(id) = payload {
                    if let Ok(item_id) = id.parse::<i32>() {
                        self.click_menu_item(item_id);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            "menu-close" => {
                self.close_menu();
                true
            }
            _ => false,
        }
    }

    fn init(cx: &mut Context<Self>) -> Self {
        // The builder publishes the window control before starting services.
        let initial = Tray {
            control: cx
                .try_global::<crate::window::WindowControlGlobal>()
                .map(|g| g.0.clone()),
            ..Tray::default()
        };
        let (tx, rx) = async_channel::unbounded::<TrayBusEvent>();

        cx.spawn(async move |this, cx| {
            let Ok(conn) = Connection::session().await else {
                warn!("Tray: no session bus; tray disabled");
                return;
            };

            // Become the tray host. If the name is taken, another host
            // (e.g. waybar) owns the items and there is nothing to show.
            match conn.request_name(WATCHER_BUS).await {
                Ok(_) => tracing::info!("Tray: hosting {WATCHER_BUS}"),
                Err(err) => {
                    warn!("Tray: cannot own {WATCHER_BUS} ({err:?}); tray disabled");
                    return;
                }
            }
            if conn
                .object_server()
                .at(WATCHER_PATH, Watcher { tx })
                .await
                .is_err()
            {
                warn!("Tray: cannot serve {WATCHER_PATH}");
                return;
            }

            // Items already present never re-register: scan the bus for names
            // exposing the item interface. Well-known `...StatusNotifierItem`
            // names cover fresh registrations, but an app that was already
            // running (unique `:1.x` name) stays silent after a shell
            // restart, so unique names are probed too. Only
            // `/StatusNotifierItem` is checked and failures are instant
            // (unknown path on a live name), so the sweep is fast.
            if let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await {
                if let Ok(names) = dbus.list_names().await {
                    for name in names {
                        let name = name.to_string();
                        if name == "org.freedesktop.DBus" {
                            continue;
                        }
                        if name.starts_with(':')
                            || name.starts_with("org.kde.StatusNotifierItem")
                            || name.starts_with("org.ayatana.StatusNotifierItem")
                        {
                            track_if_item(&conn, &this, cx, &name, "/StatusNotifierItem").await;
                        }
                    }
                }
            }

            // Membership events from our own watcher object. This is the
            // ONLY loop of this task: sharing it with the removal watch
            // below would park registrations behind an idle signal stream.
            while let Ok(ev) = rx.recv().await {
                match ev {
                    TrayBusEvent::ItemRegistered { bus, path } => {
                        track_if_item(&conn, &this, cx, &bus, &path).await;
                    }
                }
            }
        })
        .detach();

        // Removals live in their own task: an item that quits loses its bus
        // name, and that stream only wakes then.
        cx.spawn(async move |this, cx| {
            let Ok(conn) = Connection::session().await else {
                return;
            };
            let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
                return;
            };
            let Ok(mut lost) = dbus.receive_name_owner_changed().await else {
                return;
            };
            use futures_util::StreamExt as _;
            while let Some(change) = lost.next().await {
                let Ok(args) = change.args() else {
                    continue;
                };
                let name: String = args.name().to_string();
                let gone: Option<zbus::names::UniqueName> = args.new_owner().clone().into();
                if gone.is_some() {
                    continue;
                }
                let _ = this.update(cx, |tray, cx| {
                    let before = tray.items.len();
                    tray.items.retain(|_, it| it.bus != name);
                    tray.order.retain(|k| tray.items.contains_key(k));
                    if tray.items.len() != before {
                        if let Some(menu) = &tray.open_menu {
                            if !tray.items.contains_key(&menu.owner) {
                                tray.open_menu = None;
                            }
                        }
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        initial
    }
}

/// Run `fut`, giving up after `dur`. A dead or wedged peer would otherwise
/// park the caller for the full 25 s D-Bus method timeout — and with it the
/// whole membership task. Both futures are boxed so `select` gets `Unpin`.
async fn with_timeout<T>(dur: Duration, fut: impl std::future::Future<Output = T>) -> Option<T> {
    use futures_util::future::{select, Either};
    let timer = Box::pin(async move {
        async_io::Timer::after(dur).await;
    });
    let fut = Box::pin(fut);
    match select(fut, timer).await {
        Either::Left((value, _)) => Some(value),
        Either::Right(_) => None,
    }
}

/// Verify the path really is an item, then fetch and store it.
async fn track_if_item(
    conn: &Connection,
    this: &gpui::WeakEntity<Tray>,
    cx: &mut gpui::AsyncApp,
    bus: &str,
    path: &str,
) {
    // Confirm the interface exists before tracking (a bus name alone
    // proves nothing). Bounded: a wedged peer answers Introspect with the
    // 25 s bus timeout, and parking here would stall all membership traffic
    // (measured on a Qt helper connection that never answers).
    let xml = with_timeout(Duration::from_secs(3), async {
        let proxy = zbus::Proxy::new(
            conn,
            bus,
            path,
            "org.freedesktop.DBus.Introspectable",
        )
        .await
        .ok()?;
        let reply = proxy.call_method("Introspect", &()).await.ok()?;
        reply.body().deserialize::<String>().ok()
    })
    .await
    .flatten();
    let Some(xml) = xml else {
        return;
    };
    if !xml.contains("org.kde.StatusNotifierItem") {
        return;
    }
    if let Some(item) = fetch_item(conn, bus, path).await {
        let key = key_of(bus, path);
        tracing::info!("Tray: tracking '{}' ({} {})", item.title, bus, path);
        let _ = this.update(cx, |tray, cx| {
            if !tray.items.contains_key(&key) {
                tray.order.push(key.clone());
            }
            tray.items.insert(key, item);
            cx.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_words_reverse_to_bgra() {
        // One red pixel: A=255 R=255 G=0 B=0.
        let img = pixmap_to_image(1, 1, &[255, 255, 0, 0]).expect("convert");
        let bytes = img.as_bytes(0).expect("frame");
        assert_eq!(bytes, &[0, 0, 255, 255]);
    }

    #[test]
    fn pixmap_rejects_bad_length() {
        assert!(pixmap_to_image(2, 2, &[0u8; 15]).is_none());
        assert!(pixmap_to_image(0, 8, &[]).is_none());
    }

    #[test]
    fn label_mnemonics_stripped() {
        assert_eq!(clean_label("_Open"), "Open");
        assert_eq!(clean_label("__literal"), "_literal");
        assert_eq!(clean_label("a_b__c"), "ab_c");
        assert_eq!(clean_label("plain"), "plain");
    }
}
