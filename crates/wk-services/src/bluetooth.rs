//! Asynchronous BlueZ Bluetooth management service.
//!
//! Subscribes to `org.bluez` D-Bus interfaces to track adapter status,
//! discovering state, and known/connected Bluetooth devices.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use gpui::Context;
use serde::{Deserialize, Serialize};
use tracing::warn;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::Connection;

use crate::service::Service;

/// Information about a discovered or paired Bluetooth device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothDevice {
    pub path: String,
    pub address: String,
    pub name: String,
    pub icon: Option<String>,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub battery_percent: Option<u8>,
}

impl BluetoothDevice {
    pub fn display_name(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else {
            &self.address
        }
    }

    pub fn type_icon(&self) -> &'static str {
        match self.icon.as_deref() {
            Some("audio-headset") | Some("audio-headphones") => "\u{f025}", // 
            Some("input-keyboard") => "\u{f11c}",                           // 
            Some("input-mouse") | Some("input-gaming") => "\u{f8c0}",       // 
            Some("phone") => "\u{f10b}",                                    // 
            Some("computer") => "\u{f108}",                                 // 
            _ => "\u{f294}",                                                // 
        }
    }
}

/// Asynchronous Bluetooth status tracker and controller.
#[derive(Debug, Clone, Default)]
pub struct Bluetooth {
    adapter_available: bool,
    adapter_path: Option<String>,
    powered: bool,
    discovering: bool,
    devices: Vec<BluetoothDevice>,
}

impl Bluetooth {
    pub fn is_available(&self) -> bool {
        self.adapter_available
    }

    pub fn is_powered(&self) -> bool {
        self.powered
    }

    pub fn is_discovering(&self) -> bool {
        self.discovering
    }

    pub fn devices(&self) -> &[BluetoothDevice] {
        &self.devices
    }

    pub fn connected_devices(&self) -> impl Iterator<Item = &BluetoothDevice> {
        self.devices.iter().filter(|d| d.connected)
    }

    pub fn paired_devices(&self) -> impl Iterator<Item = &BluetoothDevice> {
        self.devices.iter().filter(|d| d.paired)
    }

    pub fn connected_count(&self) -> usize {
        self.connected_devices().count()
    }

    /// Primary glyph for status bar.
    pub fn icon(&self) -> &'static str {
        if !self.adapter_available || !self.powered {
            "\u{f294}" //  (dimmed or strikethrough in UI)
        } else if self.connected_count() > 0 {
            "\u{f293}" //  Bluetooth active & connected
        } else {
            "\u{f294}" //  Bluetooth on
        }
    }

    /// Toggle adapter power asynchronously via D-Bus.
    pub fn toggle_power(&self) {
        let Some(path) = self.adapter_path.clone() else {
            return;
        };
        let target = !self.powered;
        std::thread::spawn(move || {
            let rt = async_io::block_on(async {
                if let Ok(conn) = Connection::system().await {
                    let proxy_res = zbus::Proxy::new(
                        &conn,
                        "org.bluez",
                        path.as_str(),
                        "org.freedesktop.DBus.Properties",
                    )
                    .await;
                    if let Ok(proxy) = proxy_res {
                        let _ = proxy
                            .call_method(
                                "Set",
                                &("org.bluez.Adapter1", "Powered", zbus::zvariant::Value::Bool(target)),
                            )
                            .await;
                    }
                }
            });
            let _ = rt;
        });
    }

    /// Toggle device discovery (scan) asynchronously.
    pub fn toggle_scan(&self) {
        let discovering = self.discovering;
        std::thread::spawn(move || {
            if discovering {
                let _ = std::process::Command::new("pkill")
                    .args(["-f", "bluetoothctl.*scan on"])
                    .output();
            } else {
                // Launch bluetoothctl scan with 15s timeout
                let _ = std::process::Command::new("bluetoothctl")
                    .args(["--timeout", "15", "scan", "on"])
                    .spawn();
            }
        });
    }

    /// Connect or disconnect a device.
    pub fn toggle_device_connection(&self, dev_path: String, currently_connected: bool) {
        std::thread::spawn(move || {
            async_io::block_on(async {
                if let Ok(conn) = Connection::system().await {
                    if let Ok(proxy) = zbus::Proxy::new(
                        &conn,
                        "org.bluez",
                        dev_path.as_str(),
                        "org.bluez.Device1",
                    )
                    .await
                    {
                        if currently_connected {
                            let _ = proxy.call_method("Disconnect", &()).await;
                        } else {
                            let _ = proxy.call_method("Connect", &()).await;
                        }
                    }
                }
            });
        });
    }
}

type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

fn val_to_string(val: &OwnedValue) -> Option<String> {
    if let zbus::zvariant::Value::Str(s) = &**val {
        Some(s.as_str().to_string())
    } else {
        None
    }
}

async fn fetch_snapshot(conn: &Connection) -> Option<(bool, Option<String>, bool, bool, Vec<BluetoothDevice>)> {
    let root = zbus::Proxy::new(
        conn,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )
    .await
    .ok()?;

    let reply = root.call_method("GetManagedObjects", &()).await.ok()?;
    let objects: ManagedObjects = reply.body().deserialize().ok()?;

    let mut adapter_found = false;
    let mut adapter_path = None;
    let mut powered = false;
    let mut discovering = false;
    let mut devices = Vec::new();

    // Map to hold battery percent by device path
    let mut batteries: HashMap<String, u8> = HashMap::new();

    for (path, ifaces) in &objects {
        let p_str = path.as_str();
        if let Some(batt_props) = ifaces.get("org.bluez.Battery1") {
            if let Some(val) = batt_props.get("Percentage") {
                if let Ok(pct) = u8::try_from(val) {
                    batteries.insert(p_str.to_string(), pct);
                }
            }
        }
    }

    for (path, ifaces) in &objects {
        let p_str = path.as_str();

        // Check for Adapter1
        if let Some(adapter_props) = ifaces.get("org.bluez.Adapter1") {
            if adapter_path.is_none() {
                adapter_found = true;
                adapter_path = Some(p_str.to_string());
                if let Some(val) = adapter_props.get("Powered") {
                    if let Ok(p) = bool::try_from(val) {
                        powered = p;
                    }
                }
                if let Some(val) = adapter_props.get("Discovering") {
                    if let Ok(d) = bool::try_from(val) {
                        discovering = d;
                    }
                }
            }
        }

        // Check for Device1
        if let Some(dev_props) = ifaces.get("org.bluez.Device1") {
            let address = dev_props
                .get("Address")
                .and_then(val_to_string)
                .unwrap_or_default();

            let alias = dev_props.get("Alias").and_then(val_to_string);
            let name = dev_props
                .get("Name")
                .and_then(val_to_string)
                .or(alias)
                .unwrap_or_else(|| address.clone());

            let icon = dev_props.get("Icon").and_then(val_to_string);

            let paired = dev_props
                .get("Paired")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false);

            let connected = dev_props
                .get("Connected")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false);

            let trusted = dev_props
                .get("Trusted")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false);

            let battery_percent = batteries.get(p_str).copied();

            devices.push(BluetoothDevice {
                path: p_str.to_string(),
                address,
                name,
                icon,
                paired,
                connected,
                trusted,
                battery_percent,
            });
        }
    }

    // Sort devices: connected first, then paired, then alphabetically
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.paired.cmp(&a.paired))
            .then_with(|| a.name.cmp(&b.name))
    });

    Some((adapter_found, adapter_path, powered, discovering, devices))
}

impl Service for Bluetooth {
    fn init(cx: &mut Context<Self>) -> Self {
        let initial = Self::default();
        let this = cx.weak_entity();

        cx.spawn(async move |_, cx| {
            let conn = match Connection::system().await {
                Ok(c) => c,
                Err(err) => {
                    warn!("Bluetooth service: D-Bus system bus unavailable: {err}");
                    return;
                }
            };

            // Initial snapshot
            if let Some((avail, path, pwr, disc, devs)) = fetch_snapshot(&conn).await {
                let _ = this.update(cx, |bt, cx| {
                    bt.adapter_available = avail;
                    bt.adapter_path = path;
                    bt.powered = pwr;
                    bt.discovering = disc;
                    bt.devices = devs;
                    cx.notify();
                });
            }

            // Listen for BlueZ signals reactively using MessageStream
            let rule = match zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.bluez")
            {
                Ok(b) => b.build(),
                Err(e) => {
                    warn!("Bluetooth service: invalid match rule builder: {e}");
                    return;
                }
            };

            let mut stream = match zbus::MessageStream::for_match_rule(rule, &conn, Some(16)).await {
                Ok(s) => s,
                Err(err) => {
                    warn!("Bluetooth service: could not listen to BlueZ signals: {err}");
                    return;
                }
            };

            loop {
                // Wait for any BlueZ signal, or periodic 5s keepalive
                let _ = futures_lite::future::or(
                    async {
                        let _ = stream.next().await;
                    },
                    async {
                        async_io::Timer::after(Duration::from_secs(5)).await;
                    },
                )
                .await;

                if this.upgrade().is_none() {
                    return;
                }

                if let Some((avail, path, pwr, disc, new_devs)) = fetch_snapshot(&conn).await {
                    let _ = this.update(cx, |bt, cx| {
                        // Merge devices: update existing ones, insert new ones
                        let mut merged = bt.devices.clone();
                        for d in new_devs {
                            if let Some(pos) = merged.iter().position(|m| m.address == d.address) {
                                merged[pos] = d;
                            } else {
                                merged.push(d);
                            }
                        }
                        // Sort: connected first, paired second, then by name
                        merged.sort_by(|a, b| {
                            b.connected
                                .cmp(&a.connected)
                                .then_with(|| b.paired.cmp(&a.paired))
                                .then_with(|| a.name.cmp(&b.name))
                        });

                        let changed = bt.adapter_available != avail
                            || bt.adapter_path != path
                            || bt.powered != pwr
                            || bt.discovering != disc
                            || bt.devices != merged;
                        if changed {
                            bt.adapter_available = avail;
                            bt.adapter_path = path;
                            bt.powered = pwr;
                            bt.discovering = disc;
                            bt.devices = merged;
                            cx.notify();
                        }
                    });
                }
            }
        })
        .detach();

        initial
    }

    fn ipc_action(&mut self, _cx: &mut Context<Self>, action: &str, _payload: Option<&str>) -> bool {
        match action {
            "toggle" | "toggle_power" => {
                self.toggle_power();
                true
            }
            "scan" | "toggle_scan" => {
                self.toggle_scan();
                true
            }
            _ => false,
        }
    }
}
