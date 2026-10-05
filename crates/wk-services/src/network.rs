use std::collections::HashMap;
use std::time::Duration;
use futures_util::{FutureExt, StreamExt};
use gpui::Context;
use tracing::warn;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
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

/// Unified Wi-Fi status shown in the topbar.
///
/// The widget only sees this: SSID, link state and a 0-100 signal level,
/// whatever the backend.
#[derive(Debug, Clone, Default)]
pub struct Network {
    backend: WifiBackend,
    ssid: Option<String>,
    connected: bool,
    signal_percent: Option<u8>,
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
/// NM is not installed on this machine, so this path is compile-checked and
/// defensive (every step optional) but **not live-tested**. It is also
/// polled rather than event-driven on purpose: a polling read is obviously
/// correct, while an untested signal subscription could fail silently.
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
        let initial = Network::default();

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

/// Polling NetworkManager loop. See [`read_nm`]: polling is a deliberate,
/// documented choice here because this path cannot be live-tested on an
/// iwd machine.
async fn nm_loop(conn: &Connection, this: &gpui::WeakEntity<Network>, cx: &mut gpui::AsyncApp) {
    loop {
        if let Some((connected, ssid, signal)) = read_nm(conn).await {
            apply(this, cx, connected, ssid, signal);
        }
        async_io::Timer::after(Duration::from_secs(5)).await;
        if this.upgrade().is_none() {
            return;
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
