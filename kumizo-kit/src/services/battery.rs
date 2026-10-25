use std::path::Path;
use std::time::Duration;
use futures_util::{FutureExt, StreamExt};
use gpui::Context;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use zbus::Connection;
use crate::services::service::Service;

/// Battery charging state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BatteryState {
    /// Battery is actively charging.
    Charging,
    /// Battery is discharging / powering the system.
    Discharging,
    /// Battery is completely empty.
    Empty,
    /// Battery is plugged in and fully charged.
    FullyCharged,
    /// Pending charge.
    PendingCharge,
    /// Pending discharge.
    PendingDischarge,
    /// State is unavailable or no battery present.
    #[default]
    Unknown,
}

impl BatteryState {
    /// Map the numeric UPower device state to `BatteryState`.
    ///
    /// Shared by the initial read and the live update path so the two can
    /// never disagree on what a value means.
    fn from_upower(value: u32) -> Self {
        match value {
            1 => BatteryState::Charging,
            2 => BatteryState::Discharging,
            3 => BatteryState::Empty,
            4 => BatteryState::FullyCharged,
            5 => BatteryState::PendingCharge,
            6 => BatteryState::PendingDischarge,
            _ => BatteryState::Unknown,
        }
    }

    /// Friendly display string for UI widgets.
    pub fn as_str(&self) -> &'static str {
        match self {
            BatteryState::Charging => "Charging",
            BatteryState::Discharging => "Discharging",
            BatteryState::Empty => "Empty",
            BatteryState::FullyCharged => "Full",
            BatteryState::PendingCharge => "Pending Charge",
            BatteryState::PendingDischarge => "Pending Discharge",
            BatteryState::Unknown => "Unknown",
        }
    }

    /// Icon helper for status bar glyphs.
    pub fn icon(&self, percent: u8) -> &'static str {
        match self {
            BatteryState::Charging => "󰂄",
            BatteryState::FullyCharged => "󰁹",
            _ => {
                if percent >= 90 {
                    "󰁹"
                } else if percent >= 80 {
                    "󰂂"
                } else if percent >= 70 {
                    "󰂁"
                } else if percent >= 60 {
                    "󰂀"
                } else if percent >= 50 {
                    "󰁿"
                } else if percent >= 40 {
                    "󰁾"
                } else if percent >= 30 {
                    "󰁽"
                } else if percent >= 20 {
                    "󰁼"
                } else if percent >= 10 {
                    "󰁻"
                } else {
                    "󰂃"
                }
            }
        }
    }
}

/// Asynchronous battery status tracker.
///
/// Subscribes to UPower D-Bus property changes for true Zero Idle CPU operation.
/// Falls back to `/sys/class/power_supply` when D-Bus is unavailable.
#[derive(Debug, Clone, Default)]
pub struct Battery {
    percentage: u8,
    state: BatteryState,
    energy_rate_watts: Option<f64>,
    time_to_empty: Option<Duration>,
    time_to_full: Option<Duration>,
}

impl Battery {
    /// Battery level percentage (0 to 100).
    pub fn percent(&self) -> u8 {
        self.percentage
    }

    /// Current charging/discharging state.
    pub fn state(&self) -> BatteryState {
        self.state
    }

    /// Whether the battery is currently connected to AC power and charging.
    pub fn is_charging(&self) -> bool {
        matches!(self.state, BatteryState::Charging | BatteryState::FullyCharged)
    }

    /// Power consumption or charge rate in Watts.
    pub fn energy_rate_watts(&self) -> Option<f64> {
        self.energy_rate_watts
    }

    /// Estimated remaining duration until empty.
    pub fn time_to_empty(&self) -> Option<Duration> {
        self.time_to_empty
    }

    /// Estimated duration until 100% charged.
    pub fn time_to_full(&self) -> Option<Duration> {
        self.time_to_full
    }

    /// Attempts to read battery information from Linux `/sys/class/power_supply`.
    pub fn read_sysfs() -> Option<(u8, BatteryState)> {
        let base = Path::new("/sys/class/power_supply");
        if !base.exists() {
            return None;
        }

        // Check common battery names: BAT0, BAT1, etc.
        for name in &["BAT0", "BAT1", "BAT2"] {
            let bat_dir = base.join(name);
            if bat_dir.exists() {
                let cap_file = bat_dir.join("capacity");
                let status_file = bat_dir.join("status");

                let cap = std::fs::read_to_string(cap_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<u8>().ok())
                    .unwrap_or(100);

                let status_str = std::fs::read_to_string(status_file).unwrap_or_default();
                let state = match status_str.trim() {
                    "Charging" => BatteryState::Charging,
                    "Discharging" => BatteryState::Discharging,
                    "Full" => BatteryState::FullyCharged,
                    "Not charging" => BatteryState::FullyCharged,
                    _ => BatteryState::Unknown,
                };

                return Some((cap, state));
            }
        }

        None
    }
}

impl Service for Battery {
    fn init(cx: &mut Context<Self>) -> Self {
        let mut initial = Battery::default();

        // Perform initial read from sysfs if available
        if let Some((cap, st)) = Battery::read_sysfs() {
            initial.percentage = cap;
            initial.state = st;
        }

        cx.spawn(async move |this, cx| {
            // Attempt UPower D-Bus connection for zero-idle signals
            match Connection::system().await {
                Ok(conn) => {
                    debug!("Connected to system D-Bus for UPower");
                    let proxy_res = zbus::Proxy::new(
                        &conn,
                        "org.freedesktop.UPower",
                        "/org/freedesktop/UPower/devices/DisplayDevice",
                        "org.freedesktop.UPower.Device",
                    )
                    .await;

                    if let Ok(proxy) = proxy_res {
                        // Read initial properties from UPower
                        let pct_res: zbus::Result<f64> = proxy.get_property("Percentage").await;
                        let state_res: zbus::Result<u32> = proxy.get_property("State").await;

                        let initial_pct = pct_res.map(|p| p.round() as u8).unwrap_or(100);
                        let initial_state =
                            BatteryState::from_upower(state_res.unwrap_or(0));

                        let _ = this.update(cx, |bat, cx| {
                            bat.percentage = initial_pct;
                            bat.state = initial_state;
                            cx.notify();
                        });

                        // Zero idle CPU: listen for property changes. Plugging
                        // or unplugging the charger changes `State`, not
                        // `Percentage` — subscribing to the level alone leaves
                        // the icon frozen on charger events while the number
                        // still moves. Both streams must be watched.
                        let mut pct_changes =
                            proxy.receive_property_changed::<f64>("Percentage").await;
                        let mut state_changes =
                            proxy.receive_property_changed::<u32>("State").await;
                        loop {
                            futures_util::select! {
                                pct = pct_changes.next().fuse() => {
                                    let Some(change) = pct else { break };
                                    if let Ok(new_val) = change.get().await {
                                        let new_pct = new_val.round() as u8;
                                        let _ = this.update(cx, |bat, cx| {
                                            if bat.percentage != new_pct {
                                                bat.percentage = new_pct;
                                                cx.notify();
                                            }
                                        });
                                    }
                                }
                                st = state_changes.next().fuse() => {
                                    let Some(change) = st else { break };
                                    if let Ok(new_val) = change.get().await {
                                        let new_state = BatteryState::from_upower(new_val);
                                        let _ = this.update(cx, |bat, cx| {
                                            if bat.state != new_state {
                                                bat.state = new_state;
                                                cx.notify();
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!("System D-Bus unavailable for UPower ({:?}), falling back to sysfs", err);
                }
            }

            // Fallback polling loop if D-Bus connection closes or fails
            loop {
                async_io::Timer::after(Duration::from_secs(30)).await;
                if let Some((cap, state)) = Battery::read_sysfs() {
                    let res = this.update(cx, |bat, cx| {
                        if bat.percentage != cap || bat.state != state {
                            bat.percentage = cap;
                            bat.state = state;
                            cx.notify();
                        }
                    });
                    if res.is_err() {
                        break;
                    }
                }
            }
        })
        .detach();

        initial
    }
}
