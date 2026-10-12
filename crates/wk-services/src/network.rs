use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
/// whatever the backend. Both NetworkManager and iwd track the visible
/// network list (`networks()`), the radio state (`wifi_enabled()`) and the
/// managing device, so a control panel can offer scan / connect /
/// disconnect on either. Only the procfs fallback stays status-only.
#[derive(Debug, Clone, Default)]
pub struct Network {
    backend: WifiBackend,
    ssid: Option<String>,
    connected: bool,
    signal_percent: Option<u8>,
    networks: Vec<WifiNetwork>,
    wifi_enabled: bool,
    device: Option<String>,
    /// One-shot PSK handoff for the iwd secret agent: `connect_with_password`
    /// fills it just before `Network.Connect()`, the agent callback consumes
    /// it. Never stored beyond one attempt; the NM backend never touches it.
    pending_psk: PendingSecret,
}

/// `(ssid, psk)` waiting for at most one iwd `RequestPassphrase` round-trip.
type PendingSecret = Arc<Mutex<Option<(String, String)>>>;

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

    /// Visible access points, active first then by strength.
    /// Populated on both NetworkManager and iwd; empty on the fallback.
    pub fn networks(&self) -> &[WifiNetwork] {
        &self.networks
    }

    /// Radio state (`WirelessEnabled` on NM, `Adapter.Powered` on iwd).
    /// Defaults to `true` until the first backend read says otherwise;
    /// the fallback never changes it.
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

    /// Managing device object path (NM device on NetworkManager, Station
    /// path on iwd).
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

    /// Ask the backend to (re)scan. Fire-and-forget: results arrive
    /// through the regular loop (NM signals, iwd `Scanning` watch).
    /// Warns and no-ops on the fallback.
    pub fn request_scan(&self) {
        match self.backend {
            WifiBackend::NetworkManager => self.nm_request_scan(),
            WifiBackend::Iwd => iwd_request_scan(),
            WifiBackend::Fallback => warn!("Network::request_scan: no backend"),
        }
    }

    fn nm_request_scan(&self) {
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

    /// Toggle the radio (`WirelessEnabled` on NM, `Adapter.Powered` on
    /// iwd). Returns the requested state on success, `None` on the
    /// fallback or on D-Bus failure. On success the cached flag flips
    /// immediately so the tile re-renders without waiting for the loop
    /// (the loop reconfirms from the daemon right after).
    pub fn set_enabled(&mut self, enabled: bool) -> Option<bool> {
        match self.backend {
            WifiBackend::NetworkManager => Self::nm_set_enabled(enabled),
            WifiBackend::Iwd => iwd_set_enabled(enabled),
            WifiBackend::Fallback => {
                warn!("Network::set_enabled: no backend");
                None
            }
        }?;
        self.wifi_enabled = enabled;
        Some(enabled)
    }

    fn nm_set_enabled(enabled: bool) -> Option<bool> {
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

    /// Connect to a stored profile, a known iwd network, or an open
    /// network. Secured networks without a stored secret need
    /// [`Network::connect_with_password`]: this returns `false` for them
    /// (and warns) rather than failing silently inside the backend.
    pub fn connect(&self, ssid: &str) -> bool {
        match self.backend {
            WifiBackend::NetworkManager => Self::nm_connect(ssid),
            WifiBackend::Iwd => iwd_connect(ssid, None, &self.pending_psk),
            WifiBackend::Fallback => {
                warn!("Network::connect: no backend");
                false
            }
        }
    }

    fn nm_connect(ssid: &str) -> bool {
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

    /// Connect to a secured network with an explicit password. On NM the
    /// secret goes into a one-shot settings profile (`wpa-psk`; NM upgrades
    /// to SAE for WPA3 transition networks). On iwd it is handed to iwd's
    /// `RequestPassphrase` agent call for this attempt only.
    /// The secret lives only in this call — never stored by the shell.
    pub fn connect_with_password(&self, ssid: &str, password: &str) -> bool {
        if password.is_empty() {
            warn!("Network::connect_with_password: empty password");
            return false;
        }
        match self.backend {
            WifiBackend::NetworkManager => Self::nm_connect_with_password(ssid, password),
            WifiBackend::Iwd => {
                iwd_connect(ssid, Some(password), &self.pending_psk)
            }
            WifiBackend::Fallback => {
                warn!("Network::connect_with_password: no backend");
                false
            }
        }
    }

    fn nm_connect_with_password(ssid: &str, password: &str) -> bool {
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
        match self.backend {
            WifiBackend::NetworkManager => Self::nm_disconnect(),
            WifiBackend::Iwd => iwd_disconnect(),
            WifiBackend::Fallback => {
                warn!("Network::disconnect: no backend");
                false
            }
        }
    }

    fn nm_disconnect() -> bool {
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

const IWD_BUS: &str = "net.connman.iwd";
const IWD_STATION_IFACE: &str = "net.connman.iwd.Station";
const IWD_NETWORK_IFACE: &str = "net.connman.iwd.Network";
const IWD_ADAPTER_IFACE: &str = "net.connman.iwd.Adapter";
const IWD_AGENT_MGR_PATH: &str = "/net/connman/iwd";
const IWD_AGENT_MGR_IFACE: &str = "net.connman.iwd.AgentManager";
/// Object path where this process serves the iwd passphrase agent.
/// iwd addresses agents by (sender unique name, path), so a restart
/// re-registers cleanly: the old name dies with the old connection.
const IWD_AGENT_PATH: &str = "/net/connman/aurora/agent";

/// iwd secret agent: answers `RequestPassphrase` with the one-shot secret
/// staged by [`Network::connect_with_password`].
///
/// Only `RequestPassphrase` (PSK) is implemented. Enterprise (8021x)
/// methods are deliberately absent — iwd gets `UnknownMethod` and fails
/// fast with a clear error instead of hanging on an auth flow the shell
/// cannot complete. Documented, not an oversight.
struct IwdAgent {
    conn: Connection,
    pending: PendingSecret,
}

#[zbus::interface(name = "net.connman.iwd.Agent")]
impl IwdAgent {
    fn release(&self) {}

    fn cancel(&self, _reason: String) {
        *self.pending.lock().unwrap() = None;
    }

    async fn request_passphrase(
        &self,
        _device: OwnedObjectPath,
        network: OwnedObjectPath,
    ) -> Result<String, zbus::fdo::Error> {
        let wanted = self.pending.lock().unwrap().clone();
        let Some((ssid, psk)) = wanted else {
            return Err(zbus::fdo::Error::Failed("no password provided".into()));
        };
        // Hygiene: only hand the secret to the network it was entered for.
        // A mismatch fails the attempt instead of leaking the PSK.
        let name = zbus::Proxy::new(&self.conn, IWD_BUS, network.as_str(), IWD_NETWORK_IFACE)
            .await
            .ok()
            .and_then(|net| async_io::block_on(net.get_property::<String>("Name")).ok())
            .unwrap_or_default();
        if name.trim() != ssid {
            return Err(zbus::fdo::Error::Failed("password is for another network".into()));
        }
        // One-shot: whatever happens next, the secret is gone.
        *self.pending.lock().unwrap() = None;
        Ok(psk)
    }
}

/// Serve the agent object and register it with iwd. Called once when the
/// iwd backend wins detection; the `conn` outlives the process so the agent
/// stays reachable.
async fn register_iwd_agent(conn: &Connection, pending: PendingSecret) {
    let agent = IwdAgent {
        conn: conn.clone(),
        pending,
    };
    match conn.object_server().at(IWD_AGENT_PATH, agent).await {
        Ok(true) => {}
        Ok(false) => {
            warn!("iwd agent path already served, skipping registration");
            return;
        }
        Err(err) => {
            warn!("Could not serve iwd agent: {err:?}");
            return;
        }
    }
    let Ok(mgr) = zbus::Proxy::new(conn, IWD_BUS, IWD_AGENT_MGR_PATH, IWD_AGENT_MGR_IFACE).await
    else {
        warn!("iwd AgentManager unreachable");
        return;
    };
    let path = OwnedObjectPath::try_from(IWD_AGENT_PATH).expect("agent path is valid");
    if let Err(err) = mgr.call_method("RegisterAgent", &path).await {
        warn!("iwd RegisterAgent failed: {err:?}");
    }
}

/// Live `(connection, station path, adapter path)` for the sync iwd calls.
/// The adapter is the station's parent (`.../0/4` → `.../0`): iwd lays the
/// tree out as phy/device two levels, verified live. A wrong guess degrades
/// to a warned failure, never a panic.
fn iwd_station_paths() -> Option<(Connection, String, String)> {
    async_io::block_on(async {
        let conn = Connection::system().await.ok()?;
        let station = find_iwd_station(&conn).await?;
        let adapter = station.rsplit_once('/').map(|(p, _)| p.to_string())?;
        Some((conn, station, adapter))
    })
}

fn iwd_request_scan() {
    let Some((conn, station, _)) = iwd_station_paths() else {
        warn!("Network::request_scan: no iwd station");
        return;
    };
    async_io::block_on(async {
        let Ok(st) = zbus::Proxy::new(&conn, IWD_BUS, station.as_str(), IWD_STATION_IFACE).await
        else {
            warn!("Network::request_scan: no Station interface");
            return;
        };
        // Fire-and-forget: results flow through the loop's `Scanning` watch.
        // `Scan` errors while a scan is already running — harmless.
        if let Err(err) = st.call_method("Scan", &()).await {
            warn!("iwd Scan failed: {err:?}");
        }
    });
}

fn iwd_set_enabled(enabled: bool) -> Option<bool> {
    async_io::block_on(async {
        let Ok(conn) = Connection::system().await else {
            warn!("Network::set_enabled: no system bus");
            return None;
        };
        // Resolved directly: the station object is gone while the radio
        // is off, so deriving the adapter from it makes enabling
        // impossible (every attempt failed live with `no iwd station`).
        let Some(adapter) = find_iwd_adapter(&conn).await else {
            warn!("Network::set_enabled: no iwd adapter");
            return None;
        };
        let Ok(ad) = zbus::Proxy::new(&conn, IWD_BUS, adapter.as_str(), IWD_ADAPTER_IFACE).await
        else {
            warn!("Network::set_enabled: no Adapter interface");
            return None;
        };
        match ad.set_property("Powered", enabled).await {
            Ok(()) => Some(enabled),
            Err(err) => {
                warn!("iwd SetPowered failed: {err:?}");
                None
            }
        }
    })
}

/// Object path of the visible network called `ssid`, if any. iwd already
/// merges BSSs per `(SSID, type)`, so one path per network.
async fn iwd_find_network(conn: &Connection, station: &str, ssid: &str) -> Option<OwnedObjectPath> {
    let st = zbus::Proxy::new(conn, IWD_BUS, station, IWD_STATION_IFACE)
        .await
        .ok()?;
    let reply = st.call_method("GetOrderedNetworks", &()).await.ok()?;
    let nets: Vec<(OwnedObjectPath, i16)> = reply.body().deserialize().ok()?;
    for (path, _) in nets {
        let net = zbus::Proxy::new(conn, IWD_BUS, path.as_str(), IWD_NETWORK_IFACE)
            .await
            .ok()?;
        let name: String = net.get_property("Name").await.unwrap_or_default();
        if name.trim() == ssid {
            return Some(path);
        }
    }
    None
}

/// Connect on iwd. Known and open networks need no secret; a new secured
/// network needs `password`, staged for the agent for this attempt only.
/// The slot is always cleared afterwards — success, failure, or unknown
/// network — so a secret never lingers for a later attempt.
fn iwd_connect(ssid: &str, password: Option<&str>, pending: &PendingSecret) -> bool {
    let Some((conn, station, _)) = iwd_station_paths() else {
        warn!("Network::connect: no iwd station");
        return false;
    };
    async_io::block_on(async {
        let Some(path) = iwd_find_network(&conn, &station, ssid).await else {
            warn!("Network::connect: '{ssid}' not found");
            return false;
        };
        if let Some(psk) = password {
            *pending.lock().unwrap() = Some((ssid.to_string(), psk.to_string()));
        }
        let Ok(net) = zbus::Proxy::new(&conn, IWD_BUS, path.as_str(), IWD_NETWORK_IFACE).await
        else {
            *pending.lock().unwrap() = None;
            return false;
        };
        let ok = net.call_method("Connect", &()).await.is_ok();
        // Always clear: a stale secret must never serve a later attempt.
        *pending.lock().unwrap() = None;
        if !ok {
            warn!("iwd Connect failed for '{ssid}' (wrong password or unsupported auth?)");
        }
        ok
    })
}

fn iwd_disconnect() -> bool {
    let Some((conn, station, _)) = iwd_station_paths() else {
        warn!("Network::disconnect: no iwd station");
        return false;
    };
    async_io::block_on(async {
        let Ok(st) = zbus::Proxy::new(&conn, IWD_BUS, station.as_str(), IWD_STATION_IFACE).await
        else {
            return false;
        };
        match st.call_method("Disconnect", &()).await {
            Ok(_) => true,
            Err(err) => {
                warn!("iwd Disconnect failed: {err:?}");
                false
            }
        }
    })
}

/// One synchronous iwd snapshot of radio + visible networks:
/// `(powered, station_path, networks)`. Signal arrives in 1/100 dBm
/// (`-3600` = −36.00 dBm) and goes through the same `rssi_to_percent`
/// as the status read, so tile and list agree.
///
/// The list is `None` when `GetOrderedNetworks` fails (radio off: iwd
/// refuses the call) — the radio flag is still reported so the tile
/// flips instead of sticking on a stale snapshot.
async fn read_iwd_networks(
    conn: &Connection,
    station: &str,
) -> Option<(bool, String, Option<Vec<WifiNetwork>>)> {
    let adapter = station.rsplit_once('/').map(|(p, _)| p.to_string())?;
    let ad = zbus::Proxy::new(conn, IWD_BUS, adapter.as_str(), IWD_ADAPTER_IFACE)
        .await
        .ok()?;
    let enabled: bool = ad.get_property("Powered").await.unwrap_or(true);
    let st = zbus::Proxy::new(conn, IWD_BUS, station, IWD_STATION_IFACE)
        .await
        .ok()?;
    let active: OwnedObjectPath = st
        .get_property("ConnectedNetwork")
        .await
        .unwrap_or_else(|_| OwnedObjectPath::try_from("/").expect("root path is valid"));
    let mut networks: Option<Vec<WifiNetwork>> = None;
    if let Ok(nets) = st
        .call_method("GetOrderedNetworks", &())
        .await
        .map(|reply| reply.body().deserialize::<Vec<(OwnedObjectPath, i16)>>())
    {
        if let Ok(nets) = nets {
            // iwd orders strongest-first; re-sort active-first for panel parity.
            let mut out = Vec::with_capacity(nets.len());
            for (path, sig100) in nets {
                let net = zbus::Proxy::new(conn, IWD_BUS, path.as_str(), IWD_NETWORK_IFACE)
                    .await
                    .ok()?;
                // `Name` carries padding on the bus (verified live: trailing space).
                let name: String = net
                    .get_property::<String>("Name")
                    .await
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                let ty: String = net.get_property("Type").await.unwrap_or_default();
                out.push(WifiNetwork {
                    ssid: name,
                    strength: Network::rssi_to_percent((sig100 / 100) as i16),
                    secured: ty != "open",
                    active: path.as_str() == active.as_str(),
                });
            }
            out.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
            networks = Some(out);
        }
    }
    Some((enabled, station.to_string(), networks))
}

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

/// Find the iwd object exposing `iface` by walking the `Introspect` tree.
///
/// iwd exposes no `ObjectManager`, and device paths embed small integer ids
/// (`/net/connman/iwd/0/4`) that cannot be guessed, so the tree is walked
/// from the root (depth-limited). Done once per loop generation and retried
/// on failure.
async fn find_iwd_iface(conn: &Connection, iface: &str) -> Option<String> {
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
        // `">` so `Station` never matches `StationDiagnostic`.
        if xml.contains(&format!("{iface}\">")) {
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

/// The station object vanishes when the radio is off (iwd removes it —
/// verified live: `iwctl` says `No station`, the tile's enable clicks all
/// logged `no iwd station`). Anything that must work while off resolves
/// the adapter instead; station users treat `None` as radio-off.
async fn find_iwd_station(conn: &Connection) -> Option<String> {
    find_iwd_iface(conn, "net.connman.iwd.Station").await
}

async fn find_iwd_adapter(conn: &Connection) -> Option<String> {
    find_iwd_iface(conn, "net.connman.iwd.Adapter").await
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
            // Same padding as the list read: trim so tile and panel agree.
            net.get_property::<String>("Name")
                .await
                .ok()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
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
                // Secret agent for secured networks (PSK only). Best-effort:
                // a failed registration only disables password connects.
                if let Ok(slot) = this.update(cx, |net, _| net.pending_psk.clone()) {
                    register_iwd_agent(&conn, slot).await;
                }
                iwd_loop(&conn, &this, cx).await;
                return;
            }

            tracing::info!("Network backend: /proc/net/wireless fallback");
            fallback_loop(this, cx).await;
        })
        .detach();

        initial
    }

    /// `kumizo service network <action> [ssid]`:
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

/// Push one network list snapshot, notifying only on change. `networks`
/// is `None` when the list read failed (iwd radio off): the radio flag
/// and device still apply, the stale list is kept instead of sticking
/// the whole snapshot.
fn apply_networks(
    this: &gpui::WeakEntity<Network>,
    cx: &mut gpui::AsyncApp,
    enabled: bool,
    device: String,
    networks: Option<Vec<WifiNetwork>>,
) {
    let _ = this.update(cx, |net, cx| {
        let device = if device.is_empty() { None } else { Some(device) };
        let mut changed = false;
        if net.wifi_enabled != enabled {
            net.wifi_enabled = enabled;
            changed = true;
        }
        if net.device != device {
            net.device = device;
            changed = true;
        }
        if let Some(networks) = networks {
            if net.networks != networks {
                net.networks = networks;
                changed = true;
            }
        }
        if changed {
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
        apply_networks(this, cx, enabled, device, Some(networks));
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
            apply_networks(this, cx, enabled, device, Some(networks));
        }
    }
}

/// Event-driven iwd loop, anchored on the adapter: the station object
/// vanishes when the radio is off, but the adapter (and its `Powered`
/// watch) survives. `State`/`Scanning` arm only while a station exists;
/// without one the loop reports the radio flag + dropped link and waits
/// for `Powered` (or the 15 s tick) to re-resolve. A 15 s refresh keeps
/// the RSSI honest while roaming. Same shape as the battery's UPower
/// stream.
async fn iwd_loop(conn: &Connection, this: &gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp) {
    /// One full refresh: status snapshot + radio/networks snapshot.
    async fn refresh(conn: &Connection, station: &str, this: &gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp) {
        if let Some((connected, ssid, signal)) = read_iwd(conn, station).await {
            apply(this, cx, connected, ssid, signal);
        }
        if let Some((enabled, device, networks)) = read_iwd_networks(conn, station).await {
            apply_networks(this, cx, enabled, device, networks);
        }
    }

    /// `Adapter.Powered`, read directly — the one fact available with no
    /// station (radio off).
    async fn read_powered(conn: &Connection, adapter: &str) -> bool {
        let Ok(ad) = zbus::Proxy::new(conn, IWD_BUS, adapter, IWD_ADAPTER_IFACE).await else {
            return false;
        };
        ad.get_property::<bool>("Powered").await.unwrap_or(false)
    }

    'adapter: loop {
        let Some(adapter) = find_iwd_adapter(conn).await else {
            warn!("iwd adapter not found, retrying");
            async_io::Timer::after(Duration::from_secs(10)).await;
            continue;
        };
        let Ok(power_proxy) = zbus::Proxy::new(
            conn,
            "net.connman.iwd",
            adapter.as_str(),
            "net.connman.iwd.Adapter",
        )
        .await
        else {
            continue;
        };
        let mut power_changes = power_proxy.receive_property_changed::<bool>("Powered").await;

        loop {
            match find_iwd_station(conn).await {
                Some(station) => {
                    refresh(conn, &station, this, cx).await;

                    let Ok(state_proxy) = zbus::Proxy::new(
                        conn,
                        "net.connman.iwd",
                        station.as_str(),
                        "net.connman.iwd.Station",
                    )
                    .await
                    else {
                        continue;
                    };
                    let Ok(scan_proxy) = zbus::Proxy::new(
                        conn,
                        "net.connman.iwd",
                        station.as_str(),
                        "net.connman.iwd.Station",
                    )
                    .await
                    else {
                        continue;
                    };
                    let mut state_changes =
                        state_proxy.receive_property_changed::<String>("State").await;
                    let mut scan_changes =
                        scan_proxy.receive_property_changed::<bool>("Scanning").await;
                    // Full watch while the station lives. A dead adapter
                    // watch re-resolves everything; a changed station set
                    // (off→gone, on→back) just re-arms below.
                    let mut adapter_dead = false;
                    loop {
                        futures_util::select! {
                            ev = state_changes.next().fuse() => {
                                if ev.is_none() {
                                    break;
                                }
                                refresh(conn, &station, this, cx).await;
                            }
                            ev = scan_changes.next().fuse() => {
                                if ev.is_none() {
                                    break;
                                }
                                // Scan finished (or started): the visible set moved.
                                refresh(conn, &station, this, cx).await;
                            }
                            ev = power_changes.next().fuse() => {
                                if ev.is_none() {
                                    adapter_dead = true;
                                    break;
                                }
                                // Radio toggled (tile, `iwctl`, rfkill): the
                                // station set may have changed — re-resolve
                                // instead of refreshing a dead object.
                                break;
                            }
                            _ = FutureExt::fuse(async_io::Timer::after(Duration::from_secs(15))) => {
                                refresh(conn, &station, this, cx).await;
                            }
                        }
                        if this.upgrade().is_none() {
                            return;
                        }
                    }
                    if adapter_dead {
                        continue 'adapter;
                    }
                }
                None => {
                    // No station: radio off (or device gone). The flag and
                    // the dropped link still follow `Powered`; the last
                    // list is kept until the station returns.
                    let powered = read_powered(conn, &adapter).await;
                    apply(this, cx, false, None, None);
                    apply_networks(this, cx, powered, adapter.clone(), None);
                    futures_util::select! {
                        ev = power_changes.next().fuse() => {
                            if ev.is_none() {
                                continue 'adapter;
                            }
                            // `Powered` flipped, most likely back on:
                            // loop around and re-resolve the station.
                        }
                        _ = FutureExt::fuse(async_io::Timer::after(Duration::from_secs(15))) => {}
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
