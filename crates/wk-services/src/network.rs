use std::collections::HashMap;
use std::time::Duration;
use futures_util::{FutureExt, StreamExt};
use gpui::Context;
use tracing::warn;
use zbus::zvariant::{Array, OwnedObjectPath, OwnedValue, Value};
use zbus::Connection;
use crate::service::Service;

/// Which Wi-Fi manager owns the radio.
///
/// NetworkManager and iwd expose completely different D-Bus APIs
/// (`org.freedesktop.NetworkManager` vs `net.connman.iwd`), and no crate
/// abstracts both, so each backend is implemented explicitly and the winner
/// is auto-detected from the names present on the system bus. Same pattern
/// as `CompositorKind` (Hyprland / Sway / Niri / Fallback).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiBackend {
    /// `org.freedesktop.NetworkManager` is on the bus.
    NetworkManager,
    /// `net.connman.iwd` is on the bus.
    Iwd,
    /// No manager on D-Bus: parse `/proc/net/wireless`.
    #[default]
    Fallback,
}

/// One visible Wi-Fi network (one SSID: several BSSIDs are merged,
/// keeping the strongest signal).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub strength: u8,
    pub secured: bool,
    pub active: bool,
}

/// Unified Wi-Fi status shown in the topbar.
///
/// The widget only sees this: SSID, link state and a 0-100 signal level,
/// whatever the backend. On NetworkManager the service additionally tracks
/// the visible network list (`networks()`), the radio state
/// (`wifi_enabled()`) and the managing device, so a control panel can offer
/// scan / connect / disconnect. iwd and the procfs fallback stay
/// status-only: the mutating methods warn and return `false` there.
#[derive(Debug, Clone, Default)]
pub struct Network {
    backend: WifiBackend,
    ssid: Option<String>,
    connected: bool,
    signal_percent: Option<u8>,
    networks: Vec<WifiNetwork>,
    wifi_enabled: bool,
    device: Option<String>,
}

impl Network {
    /// Backend that provided the current data (useful for debugging).
    pub fn backend(&self) -> WifiBackend {
        self.backend
    }

    /// Connected network name, if known.
    pub fn ssid(&self) -> Option<&str> {
        self.ssid.as_deref()
    }

    /// Whether the station reports an active connection.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Signal level as 0-100, if known.
    pub fn signal_percent(&self) -> Option<u8> {
        self.signal_percent
    }

    /// Visible access points (NetworkManager only, newest scan first by
    /// strength; empty on iwd / fallback).
    pub fn networks(&self) -> &[WifiNetwork] {
        &self.networks
    }

    /// Radio state (`WirelessEnabled`). Defaults to `true` until the first
    /// NM poll says otherwise; iwd / fallback never change it.
    pub fn wifi_enabled(&self) -> bool {
        self.wifi_enabled
    }

    /// Whether a Wi-Fi radio is known: a managed NM device, or the iwd
    /// station. The procfs fallback cannot know — reports `true` so the
    /// tile stays informative instead of crying unavailable.
    pub fn has_radio(&self) -> bool {
        match self.backend {
            WifiBackend::NetworkManager => self.device.is_some(),
            _ => true,
        }
    }

    /// Managing device object path (NetworkManager only).
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// Text shown next to the icon: the SSID, or a placeholder. Owned, so
    /// widgets can move it into the element tree (a borrow of the read
    /// guard would escape `render`).
    pub fn display_label(&self) -> String {
        self.ssid.clone().unwrap_or_else(|| "Disconnected".to_string())
    }

    /// Signal arcs from Material Design Icons (same codepoints as Nerd Fonts,
    /// verified present in JetBrainsMono Nerd Font: rendered proof before
    /// use, never guessed). Consistent with the battery/volume glyphs.
    pub fn icon(&self) -> &'static str {
        if !self.connected {
            return "󰖪";
        }
        match self.signal_percent.unwrap_or(0) {
            75..=100 => "󰤨",
            50..=74 => "󰤥",
            25..=49 => "󰤢",
            _ => "󰤟",
        }
    }

    /// Map an RSSI in dBm to 0-100. `-50` dBm or better is full scale,
    /// `-100` dBm or worse is zero: `pct = 2 * (rssi + 100)`.
    fn rssi_to_percent(rssi: i16) -> u8 {
        ((rssi as i32 + 100) * 2).clamp(0, 100) as u8
    }

    /// Last-resort read from `/proc/net/wireless` (no D-Bus, no CLI).
    ///
    /// Returns `(connected, signal_percent)`. The SSID is not exposed there.
    /// Format: `wlan0: 0000 40. -70. -256 ...` where `40.` is link quality
    /// out of 70.
    fn read_proc_wireless() -> Option<(bool, u8)> {
        let text = std::fs::read_to_string("/proc/net/wireless").ok()?;
        for line in text.lines().skip(2) {
            let mut parts = line.splitn(2, ':');
            let _iface = parts.next()?.trim();
            let rest = parts.next()?;
            let mut nums = rest.split_whitespace();
            let _status = nums.next()?;
            let link = nums.next()?.trim_end_matches('.').parse::<u32>().ok()?;
            if link > 0 {
                return Some((true, (link * 100 / 70).min(100) as u8));
            }
            return Some((false, 0));
        }
        None
    }

    /// Synchronous D-Bus helper: the mutating calls below are quick
    /// (no scan wait, no secret agent round-trip), so `block_on` from a
    /// click handler is fine — same pattern as the other services.
    fn nm_conn() -> Option<(Connection, String)> {
        async_io::block_on(async {
            let conn = Connection::system().await.ok()?;
            let root = zbus::Proxy::new(&conn, NM_BUS, NM_PATH, NM_IFACE)
                .await
                .ok()?;
            let reply = root.call_method("GetDevices", &()).await.ok()?;
            let devices: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;
            for dev_path in devices {
                let dev = zbus::Proxy::new(
                    &conn,
                    NM_BUS,
                    dev_path.as_str(),
                    "org.freedesktop.NetworkManager.Device",
                )
                .await
                .ok()?;
                if dev.get_property::<u32>("DeviceType").await.unwrap_or(0) == 2 {
                    return Some((conn, dev_path.as_str().to_string()));
                }
            }
            None
        })
    }

    /// Ask NetworkManager to (re)scan. Fire-and-forget: results arrive
    /// through the regular poll. Warns and no-ops off the NM backend.
    pub fn request_scan(&self) {
        if self.backend != WifiBackend::NetworkManager {
            warn!("Network::request_scan needs the NetworkManager backend");
            return;
        }
        let Some((conn, device)) = Self::nm_conn() else {
            warn!("Network::request_scan: no wifi device");
            return;
        };
        async_io::block_on(async {
            let Ok(wifi) = zbus::Proxy::new(&conn, NM_BUS, device.as_str(), NM_WIRELESS_IFACE).await
            else {
                warn!("Network::request_scan: no wireless interface");
                return;
            };
            let opts = HashMap::<String, OwnedValue>::new();
            if let Err(err) = wifi.call_method("RequestScan", &opts).await {
                warn!("Network::request_scan failed: {err:?}");
            }
        });
    }

    /// Toggle the radio (`WirelessEnabled`). Returns the requested state on
    /// success, `None` off the NM backend or on D-Bus failure.
    pub fn set_enabled(&self, enabled: bool) -> Option<bool> {
        if self.backend != WifiBackend::NetworkManager {
            warn!("Network::set_enabled needs the NetworkManager backend");
            return None;
        }
        async_io::block_on(async {
            let Ok(conn) = Connection::system().await else {
                warn!("Network::set_enabled: no system bus");
                return None;
            };
            let Ok(root) = zbus::Proxy::new(&conn, NM_BUS, NM_PATH, NM_IFACE).await else {
                warn!("Network::set_enabled: no NM root");
                return None;
            };
            match root.set_property("WirelessEnabled", enabled).await {
                Ok(()) => Some(enabled),
                Err(err) => {
                    warn!("Network::set_enabled failed: {err:?}");
                    None
                }
            }
        })
    }

    /// Connect to a stored profile or an open network. Secured networks
    /// without a stored profile need [`Network::connect_with_password`]:
    /// this returns `false` for them (and warns) rather than failing
    /// silently inside NM.
    pub fn connect(&self, ssid: &str) -> bool {
        if self.backend != WifiBackend::NetworkManager {
            warn!("Network::connect needs the NetworkManager backend");
            return false;
        }
        async_io::block_on(async {
            let Ok(conn) = Connection::system().await else {
                return false;
            };
            let Some(device) = nm_wifi_device(&conn).await else {
                warn!("Network::connect: no wifi device");
                return false;
            };
            // 1. Stored profile (has the secrets already)?
            if let Some(profile) = nm_known_connection(&conn, ssid).await {
                return nm_activate(&conn, &profile, &device, "/").await;
            }
            // 2. Open network: build a minimal profile on the AP.
            if let Some((ap, secured)) = nm_find_ap(&conn, &device, ssid).await {
                if secured {
                    warn!("Network::connect: '{ssid}' needs a password");
                    return false;
                }
                let mut wifi = HashMap::new();
                wifi.insert("ssid".to_string(), bytes_value(ssid.as_bytes()));
                wifi.insert("mode".to_string(), str_value("infrastructure"));
                let mut conn_map = HashMap::new();
                conn_map.insert("id".to_string(), str_value(ssid));
                conn_map.insert("type".to_string(), str_value("802-11-wireless"));
                let mut settings = HashMap::new();
                settings.insert("connection".to_string(), conn_map);
                settings.insert("802-11-wireless".to_string(), wifi);
                return nm_add_and_activate(&conn, settings, &device, ap.as_str()).await;
            }
            warn!("Network::connect: '{ssid}' not found");
            false
        })
    }

    /// Connect to a secured network with an explicit password
    /// (`wpa-psk`; NM upgrades to SAE for WPA3 transition networks).
    /// The secret lives only in this call — never stored by the shell.
    pub fn connect_with_password(&self, ssid: &str, password: &str) -> bool {
        if self.backend != WifiBackend::NetworkManager {
            warn!("Network::connect_with_password needs the NetworkManager backend");
            return false;
        }
        if password.is_empty() {
            warn!("Network::connect_with_password: empty password");
            return false;
        }
        async_io::block_on(async {
            let Ok(conn) = Connection::system().await else {
                return false;
            };
            let Some(device) = nm_wifi_device(&conn).await else {
                warn!("Network::connect_with_password: no wifi device");
                return false;
            };
            let Some((ap, _)) = nm_find_ap(&conn, &device, ssid).await else {
                warn!("Network::connect_with_password: '{ssid}' not found");
                return false;
            };
            let mut wifi = HashMap::new();
            wifi.insert("ssid".to_string(), bytes_value(ssid.as_bytes()));
            wifi.insert("mode".to_string(), str_value("infrastructure"));
            let mut sec = HashMap::new();
            sec.insert("key-mgmt".to_string(), str_value("wpa-psk"));
            sec.insert("psk".to_string(), str_value(password));
            let mut conn_map = HashMap::new();
            conn_map.insert("id".to_string(), str_value(ssid));
            conn_map.insert("type".to_string(), str_value("802-11-wireless"));
            let mut settings = HashMap::new();
            settings.insert("connection".to_string(), conn_map);
            settings.insert("802-11-wireless".to_string(), wifi);
            settings.insert("802-11-wireless-security".to_string(), sec);
            nm_add_and_activate(&conn, settings, &device, ap.as_str()).await
        })
    }

    /// Drop the current Wi-Fi connection (keeps the radio on).
    pub fn disconnect(&self) -> bool {
        if self.backend != WifiBackend::NetworkManager {
            warn!("Network::disconnect needs the NetworkManager backend");
            return false;
        }
        async_io::block_on(async {
            let Ok(conn) = Connection::system().await else {
                return false;
            };
            let Some(device) = nm_wifi_device(&conn).await else {
                return false;
            };
            let Ok(dev) = zbus::Proxy::new(
                &conn,
                NM_BUS,
                device.as_str(),
                "org.freedesktop.NetworkManager.Device",
            )
            .await
            else {
                return false;
            };
            match dev.call_method("Disconnect", &()).await {
                Ok(_) => true,
                Err(err) => {
                    warn!("Network::disconnect failed: {err:?}");
                    false
                }
            }
        })
    }
}

const NM_BUS: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_IFACE: &str = "org.freedesktop.NetworkManager";
const NM_DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const NM_WIRELESS_IFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const NM_AP_IFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const NM_SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";
const NM_SETTINGS_CONN_IFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";

fn str_value(s: &str) -> OwnedValue {
    OwnedValue::try_from(Value::from(s)).expect("str converts to a D-Bus value")
}

fn bytes_value(b: &[u8]) -> OwnedValue {
    OwnedValue::try_from(Value::Array(Array::from(b.to_vec())))
        .expect("bytes convert to a D-Bus value")
}

fn owned_bytes(v: &OwnedValue) -> Option<Vec<u8>> {
    let val = Value::try_from(v).ok()?;
    Vec::<u8>::try_from(val).ok()
}

/// First Wi-Fi device path, if any.
async fn nm_wifi_device(conn: &Connection) -> Option<String> {
    let root = zbus::Proxy::new(conn, NM_BUS, NM_PATH, NM_IFACE).await.ok()?;
    let reply = root.call_method("GetDevices", &()).await.ok()?;
    let devices: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;
    for dev_path in devices {
        let dev = zbus::Proxy::new(conn, NM_BUS, dev_path.as_str(), NM_DEVICE_IFACE)
            .await
            .ok()?;
        if dev.get_property::<u32>("DeviceType").await.unwrap_or(0) == 2 {
            return Some(dev_path.as_str().to_string());
        }
    }
    None
}

/// Visible access point matching `ssid`: `(object path, secured)`.
async fn nm_find_ap(conn: &Connection, device: &str, ssid: &str) -> Option<(OwnedObjectPath, bool)> {
    let wifi = zbus::Proxy::new(conn, NM_BUS, device, NM_WIRELESS_IFACE)
        .await
        .ok()?;
    let reply = wifi.call_method("GetAllAccessPoints", &()).await.ok()?;
    let aps: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;
    for ap_path in aps {
        let ap = zbus::Proxy::new(conn, NM_BUS, ap_path.as_str(), NM_AP_IFACE)
            .await
            .ok()?;
        let raw: Vec<u8> = ap.get_property("Ssid").await.unwrap_or_default();
        if String::from_utf8_lossy(&raw) != ssid {
            continue;
        }
        let flags: u32 = ap.get_property("Flags").await.unwrap_or(0);
        let wpa: u32 = ap.get_property("WpaFlags").await.unwrap_or(0);
        let rsn: u32 = ap.get_property("RsnFlags").await.unwrap_or(0);
        return Some((ap_path, flags & 0x1 != 0 || wpa != 0 || rsn != 0));
    }
    None
}

/// Stored settings-connection whose 802-11-wireless SSID matches, if any.
async fn nm_known_connection(conn: &Connection, ssid: &str) -> Option<OwnedObjectPath> {
    let settings =
        zbus::Proxy::new(conn, NM_BUS, NM_PATH, NM_SETTINGS_IFACE).await.ok()?;
    let reply = settings.call_method("ListConnections", &()).await.ok()?;
    let profiles: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;
    for profile in profiles {
        let proxy = zbus::Proxy::new(conn, NM_BUS, profile.as_str(), NM_SETTINGS_CONN_IFACE)
            .await
            .ok()?;
        let reply = proxy.call_method("GetSettings", &()).await.ok()?;
        let all: HashMap<String, HashMap<String, OwnedValue>> =
            reply.body().deserialize().ok()?;
        let matches = all
            .get("802-11-wireless")
            .and_then(|w| w.get("ssid"))
            .and_then(owned_bytes)
            .map(|b| String::from_utf8_lossy(&b) == ssid)
            .unwrap_or(false);
        if matches {
            return Some(profile);
        }
    }
    None
}

async fn nm_activate(conn: &Connection, profile: &OwnedObjectPath, device: &str, ap: &str) -> bool {
    let Ok(root) = zbus::Proxy::new(conn, NM_BUS, NM_PATH, NM_IFACE).await else {
        return false;
    };
    let root_path = OwnedObjectPath::try_from("/").expect("root path is valid");
    let ap_path: OwnedObjectPath = if ap == "/" {
        root_path
    } else {
        match OwnedObjectPath::try_from(ap) {
            Ok(p) => p,
            Err(_) => return false,
        }
    };
    match root
        .call_method(
            "ActivateConnection",
            &(profile, device, ap_path.as_str()),
        )
        .await
    {
        Ok(_) => true,
        Err(err) => {
            warn!("NetworkManager ActivateConnection failed: {err:?}");
            false
        }
    }
}

async fn nm_add_and_activate(
    conn: &Connection,
    settings: HashMap<String, HashMap<String, OwnedValue>>,
    device: &str,
    ap: &str,
) -> bool {
    let Ok(root) = zbus::Proxy::new(conn, NM_BUS, NM_PATH, NM_IFACE).await else {
        return false;
    };
    match root
        .call_method("AddAndActivateConnection", &(settings, device, ap))
        .await
    {
        Ok(_) => true,
        Err(err) => {
            warn!("NetworkManager AddAndActivateConnection failed: {err:?}");
            false
        }
    }
}

/// One synchronous snapshot of the NM radio + visible networks:
/// `(wifi_enabled, device_path, networks)`. Merges BSSIDs by SSID
/// (strongest wins), skips hidden (empty-SSID) APs, sorts active first.
async fn read_nm_networks(conn: &Connection) -> Option<(bool, String, Vec<WifiNetwork>)> {
    let root = zbus::Proxy::new(conn, NM_BUS, NM_PATH, NM_IFACE).await.ok()?;
    let enabled: bool = root.get_property("WirelessEnabled").await.unwrap_or(true);
    let reply = root.call_method("GetDevices", &()).await.ok()?;
    let devices: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;

    let mut out: Vec<WifiNetwork> = Vec::new();
    let mut wifi_dev = String::new();
    for dev_path in &devices {
        let dev = zbus::Proxy::new(conn, NM_BUS, dev_path.as_str(), NM_DEVICE_IFACE)
            .await
            .ok()?;
        if dev.get_property::<u32>("DeviceType").await.unwrap_or(0) != 2 {
            continue;
        }
        if wifi_dev.is_empty() {
            wifi_dev = dev_path.as_str().to_string();
        }
        let wifi = zbus::Proxy::new(conn, NM_BUS, dev_path.as_str(), NM_WIRELESS_IFACE)
            .await
            .ok()?;
        let active_ap: OwnedObjectPath = wifi
            .get_property("ActiveAccessPoint")
            .await
            .unwrap_or_else(|_| OwnedObjectPath::try_from("/").expect("root path is valid"));
        let aps: Vec<OwnedObjectPath> = match wifi.call_method("GetAllAccessPoints", &()).await
        {
            Ok(r) => r.body().deserialize().unwrap_or_default(),
            // Older NM: only the active-device list exists.
            Err(_) => wifi
                .call_method("GetAccessPoints", &())
                .await
                .ok()
                .and_then(|r| r.body().deserialize().ok())
                .unwrap_or_default(),
        };
        for ap_path in aps {
            let ap = zbus::Proxy::new(conn, NM_BUS, ap_path.as_str(), NM_AP_IFACE)
                .await
                .ok()?;
            let raw: Vec<u8> = ap.get_property("Ssid").await.unwrap_or_default();
            let ssid = String::from_utf8_lossy(&raw).into_owned();
            if ssid.is_empty() {
                continue;
            }
            let strength: u8 = ap.get_property("Strength").await.unwrap_or(0);
            let flags: u32 = ap.get_property("Flags").await.unwrap_or(0);
            let wpa: u32 = ap.get_property("WpaFlags").await.unwrap_or(0);
            let rsn: u32 = ap.get_property("RsnFlags").await.unwrap_or(0);
            let secured = flags & 0x1 != 0 || wpa != 0 || rsn != 0;
            let active = active_ap.as_str() != "/" && ap_path.as_str() == active_ap.as_str();
            match out.iter_mut().find(|n| n.ssid == ssid) {
                Some(n) => {
                    n.strength = n.strength.max(strength.min(100));
                    n.active |= active;
                    n.secured |= secured;
                }
                None => out.push(WifiNetwork {
                    ssid,
                    strength: strength.min(100),
                    secured,
                    active,
                }),
            }
        }
    }
    out.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
    Some((enabled, wifi_dev, out))
}

/// Find the iwd Station object by walking the `Introspect` tree.
///
/// iwd exposes no `ObjectManager`, and device paths embed small integer ids
/// (`/net/connman/iwd/0/4`) that cannot be guessed, so the tree is walked
/// from the root (depth-limited) looking for the `net.connman.iwd.Station`
/// interface. Done once at startup and retried on failure.
async fn find_iwd_station(conn: &Connection) -> Option<String> {
    /// Bare `<node name="..."/>` children of one Introspect document.
    fn child_nodes(xml: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = xml;
        while let Some(i) = rest.find("<node") {
            rest = &rest[i + "<node".len()..];
            let Some(q1) = rest.find("name=\"") else {
                break;
            };
            let s = &rest[q1 + 6..];
            let Some(q2) = s.find('"') else {
                break;
            };
            // The name must close before the tag does.
            if s[..q2].contains('>') {
                break;
            }
            out.push(s[..q2].to_string());
            rest = &s[q2 + 1..];
        }
        out
    }

    async fn introspect(conn: &Connection, path: &str) -> Option<String> {
        let proxy = zbus::Proxy::new(
            conn,
            "net.connman.iwd",
            path,
            "org.freedesktop.DBus.Introspectable",
        )
        .await
        .ok()?;
        let reply = proxy.call_method("Introspect", &()).await.ok()?;
        reply.body().deserialize::<String>().ok()
    }

    let mut stack = vec![(String::from("/net/connman/iwd"), 0)];
    while let Some((path, depth)) = stack.pop() {
        let Some(xml) = introspect(conn, &path).await else {
            continue;
        };
        if xml.contains("net.connman.iwd.Station") {
            return Some(path);
        }
        if depth < 4 {
            for child in child_nodes(&xml) {
                stack.push((format!("{path}/{child}"), depth + 1));
            }
        }
    }
    None
}

/// One synchronous snapshot from iwd: `(connected, ssid, signal_percent)`.
///
/// Every step is optional: a missing property degrades the snapshot instead
/// of failing it.
async fn read_iwd(conn: &Connection, station: &str) -> Option<(bool, Option<String>, Option<u8>)> {
    let proxy = zbus::Proxy::new(conn, "net.connman.iwd", station, "net.connman.iwd.Station")
        .await
        .ok()?;
    let state: String = proxy.get_property("State").await.unwrap_or_default();
    let connected = state == "connected";

    let ssid = if connected {
        let net_path: OwnedObjectPath = proxy.get_property("ConnectedNetwork").await.ok()?;
        if net_path.as_str() == "/" {
            None
        } else {
            let net = zbus::Proxy::new(
                conn,
                "net.connman.iwd",
                net_path,
                "net.connman.iwd.Network",
            )
            .await
            .ok()?;
            net.get_property::<String>("Name").await.ok()
        }
    } else {
        None
    };

    let signal = if connected {
        let diag = zbus::Proxy::new(
            conn,
            "net.connman.iwd",
            station,
            "net.connman.iwd.StationDiagnostic",
        )
        .await
        .ok()?;
        let reply = diag.call_method("GetDiagnostics", &()).await.ok()?;
        let mut dict: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
        dict.remove("RSSI")
            .and_then(|v| i16::try_from(v).ok())
            .map(Network::rssi_to_percent)
    } else {
        None
    };

    Some((connected, ssid, signal))
}

/// One synchronous snapshot from NetworkManager.
///
/// Every step is optional: a missing property degrades the snapshot instead
/// of failing it. Driven by [`nm_loop`] signal subscriptions; the sync
/// mutating methods (`connect`, ...) re-read through the same path.
async fn read_nm(conn: &Connection) -> Option<(bool, Option<String>, Option<u8>)> {
    const NM_BUS: &str = "org.freedesktop.NetworkManager";
    const NM_PATH: &str = "/org/freedesktop/NetworkManager";
    const NM_IFACE: &str = "org.freedesktop.NetworkManager";

    let root = zbus::Proxy::new(conn, NM_BUS, NM_PATH, NM_IFACE)
        .await
        .ok()?;
    let reply = root.call_method("GetDevices", &()).await.ok()?;
    let devices: Vec<OwnedObjectPath> = reply.body().deserialize().ok()?;

    for dev_path in devices {
        let dev = zbus::Proxy::new(
            conn,
            NM_BUS,
            dev_path,
            "org.freedesktop.NetworkManager.Device",
        )
        .await
        .ok()?;
        // 2 == NM_DEVICE_TYPE_WIFI. Non-wifi devices are skipped.
        if dev.get_property::<u32>("DeviceType").await.unwrap_or(0) != 2 {
            continue;
        }
        let wifi = zbus::Proxy::new(
            conn,
            NM_BUS,
            dev.path().as_str(),
            "org.freedesktop.NetworkManager.Device.Wireless",
        )
        .await
        .ok()?;
        let ap_path: OwnedObjectPath =
            wifi.get_property("ActiveAccessPoint").await.unwrap_or_else(|_| {
                OwnedObjectPath::try_from("/").expect("root path is valid")
            });
        if ap_path.as_str() == "/" {
            continue;
        }
        let ap =
            zbus::Proxy::new(conn, NM_BUS, ap_path, "org.freedesktop.NetworkManager.AccessPoint")
                .await
                .ok()?;
        let raw_ssid: Vec<u8> = ap.get_property("Ssid").await.unwrap_or_default();
        let strength: u8 = ap.get_property("Strength").await.unwrap_or(0);
        let ssid = String::from_utf8_lossy(&raw_ssid).into_owned();
        let ssid = if ssid.is_empty() { None } else { Some(ssid) };
        return Some((true, ssid, Some(strength.min(100))));
    }

    Some((false, None, None))
}

impl Service for Network {
    fn init(cx: &mut Context<Self>) -> Self {
        let mut initial = Network::default();
        // Assume the radio is on until the first NM poll says otherwise;
        // iwd / fallback never touch this field.
        initial.wifi_enabled = true;

        cx.spawn(async move |this, cx| {
            let conn = match Connection::system().await {
                Ok(conn) => conn,
                Err(err) => {
                    warn!("System D-Bus unavailable for Network ({err:?}), sysfs only");
                    return fallback_loop(this, cx).await;
                }
            };

            // Auto-detect the backend from the names on the system bus.
            let dbus = match zbus::fdo::DBusProxy::new(&conn).await {
                Ok(dbus) => dbus,
                Err(err) => {
                    warn!("D-Bus proxy failed ({err:?}), sysfs only");
                    return fallback_loop(this, cx).await;
                }
            };
            let has_nm = dbus
                .name_has_owner("org.freedesktop.NetworkManager".try_into().unwrap())
                .await
                .unwrap_or(false);
            let has_iwd = !has_nm
                && dbus
                    .name_has_owner("net.connman.iwd".try_into().unwrap())
                    .await
                    .unwrap_or(false);

            if has_nm {
                tracing::info!("Network backend: NetworkManager");
                set_backend(&this, cx, WifiBackend::NetworkManager);
                nm_loop(&conn, &this, cx).await;
                return;
            }

            if has_iwd {
                tracing::info!("Network backend: iwd");
                set_backend(&this, cx, WifiBackend::Iwd);
                iwd_loop(&conn, &this, cx).await;
                return;
            }

            tracing::info!("Network backend: /proc/net/wireless fallback");
            fallback_loop(this, cx).await;
        })
        .detach();

        initial
    }

    /// `widgetctl service network <action> [ssid]`:
    /// `scan`, `on`, `off`, `toggle`, `connect <ssid>`, `disconnect`.
    /// Passwords never travel over the CLI: secured new networks connect
    /// from the control-panel UI only.
    fn ipc_action(
        &mut self,
        _cx: &mut Context<Self>,
        action: &str,
        payload: Option<&str>,
    ) -> bool {
        match action {
            "scan" => {
                self.request_scan();
                true
            }
            "on" => self.set_enabled(true).is_some(),
            "off" => self.set_enabled(false).map(|s| !s).unwrap_or(false),
            "toggle" => {
                let target = !self.wifi_enabled;
                self.set_enabled(target).is_some()
            }
            "connect" => match payload {
                Some(ssid) => self.connect(ssid),
                None => false,
            },
            "disconnect" => self.disconnect(),
            _ => false,
        }
    }
}

/// Push one snapshot into the service, notifying only on change.
fn apply(
    this: &gpui::WeakEntity<Network>,
    cx: &mut gpui::AsyncApp,
    connected: bool,
    ssid: Option<String>,
    signal: Option<u8>,
) {
    let _ = this.update(cx, |net, cx| {
        if net.connected != connected || net.ssid != ssid || net.signal_percent != signal {
            net.connected = connected;
            net.ssid = ssid;
            net.signal_percent = signal;
            cx.notify();
        }
    });
}

fn set_backend(this: &gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp, backend: WifiBackend) {
    let _ = this.update(cx, |net, _| {
        net.backend = backend;
    });
}

/// Push one network list snapshot, notifying only on change.
fn apply_networks(
    this: &gpui::WeakEntity<Network>,
    cx: &mut gpui::AsyncApp,
    enabled: bool,
    device: String,
    networks: Vec<WifiNetwork>,
) {
    let _ = this.update(cx, |net, cx| {
        let device = if device.is_empty() { None } else { Some(device) };
        if net.wifi_enabled != enabled || net.device != device || net.networks != networks {
            net.wifi_enabled = enabled;
            net.device = device;
            net.networks = networks;
            cx.notify();
        }
    });
}

/// Event-driven NetworkManager loop: any NM signal (`StateChanged`,
/// `PropertiesChanged`, `AccessPointAdded/Removed`, ...) triggers a
/// re-read. Signals arrive in bursts (a scan emits dozens), so each wake
/// waits 750 ms for quiet before re-reading — trailing-edge debounce,
/// NOT a periodic refresh: a silent bus means zero wakeups.
async fn nm_loop(conn: &Connection, this: &gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp) {
    if let Some((connected, ssid, signal)) = read_nm(conn).await {
        apply(this, cx, connected, ssid, signal);
    }
    if let Some((enabled, device, networks)) = read_nm_networks(conn).await {
        apply_networks(this, cx, enabled, device, networks);
    }

    let rule = match zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NM_BUS)
    {
        Ok(b) => b.build(),
        Err(err) => {
            warn!("Network: invalid NM match rule: {err}");
            return;
        }
    };
    let mut stream = match zbus::MessageStream::for_match_rule(rule, conn, Some(64)).await {
        Ok(s) => s,
        Err(err) => {
            warn!("Network: could not listen to NM signals: {err}");
            return;
        }
    };

    loop {
        // Next NM signal, or stream end.
        if stream.next().await.is_none() {
            return;
        }
        if this.upgrade().is_none() {
            return;
        }
        // Settle: coalesce the burst before paying for a re-read.
        loop {
            futures_util::select! {
                ev = stream.next().fuse() => {
                    if ev.is_none() {
                        return;
                    }
                }
                _ = FutureExt::fuse(async_io::Timer::after(Duration::from_millis(750))) => break,
            }
            if this.upgrade().is_none() {
                return;
            }
        }
        if let Some((connected, ssid, signal)) = read_nm(conn).await {
            apply(this, cx, connected, ssid, signal);
        }
        if let Some((enabled, device, networks)) = read_nm_networks(conn).await {
            apply_networks(this, cx, enabled, device, networks);
        }
    }
}

/// Event-driven iwd loop: `State` changes wake us, a 15 s refresh keeps the
/// RSSI honest while roaming. Same shape as the battery's UPower stream.
async fn iwd_loop(conn: &Connection, this: &gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp) {
    loop {
        let Some(station) = find_iwd_station(conn).await else {
            warn!("iwd station not found, retrying");
            async_io::Timer::after(Duration::from_secs(10)).await;
            continue;
        };

        if let Some((connected, ssid, signal)) = read_iwd(conn, &station).await {
            apply(this, cx, connected, ssid, signal);
        }

        let Ok(proxy) = zbus::Proxy::new(
            conn,
            "net.connman.iwd",
            station.as_str(),
            "net.connman.iwd.Station",
        )
        .await
        else {
            continue;
        };
        let mut changes = proxy.receive_property_changed::<String>("State").await;
        loop {
            futures_util::select! {
                ev = changes.next().fuse() => {
                    // Stream ended: re-resolve from the top.
                    if ev.is_none() {
                        break;
                    }
                    if let Some((connected, ssid, signal)) = read_iwd(conn, &station).await {
                        apply(this, cx, connected, ssid, signal);
                    }
                }
                _ = FutureExt::fuse(async_io::Timer::after(Duration::from_secs(15))) => {
                    if let Some((connected, ssid, signal)) = read_iwd(conn, &station).await {
                        apply(this, cx, connected, ssid, signal);
                    }
                }
            }
            if this.upgrade().is_none() {
                return;
            }
        }
    }
}

/// `/proc/net/wireless` polling. No D-Bus, no CLI, no events: the file is
/// re-read on a timer and the view updates only when values move.
async fn fallback_loop(this: gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp) {
    loop {
        if let Some((connected, signal)) = Network::read_proc_wireless() {
            apply(&this, cx, connected, None, Some(signal));
        }
        async_io::Timer::after(Duration::from_secs(10)).await;
        if this.upgrade().is_none() {
            return;
        }
    }
}
