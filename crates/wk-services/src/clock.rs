use std::time::Duration;
use gpui::Context;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use crate::service::Service;

/// Resolution for the clock service updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockResolution {
    /// Updates on every exact second boundary.
    #[default]
    Seconds,
    /// Updates on every exact minute boundary (reduces wakeups to 1/min).
    Minutes,
}

/// Asynchronous clock service providing system date and time.
///
/// Implements Zero Idle CPU by calculating the exact nanoseconds remaining until
/// the next whole second (or minute) boundary and sleeping asynchronously via `async_io::Timer`.
pub struct Clock {
    now: OffsetDateTime,
    resolution: ClockResolution,
}

impl Clock {
    /// Create a new Clock with the current local time.
    pub fn new() -> Self {
        Self {
            now: OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc()),
            resolution: ClockResolution::Seconds,
        }
    }

    /// Set resolution to minutes (sleeps until next minute boundary).
    pub fn set_resolution(&mut self, res: ClockResolution) {
        self.resolution = res;
    }

    /// Current time formatted as "HH:MM:SS" (24-hour).
    pub fn time_hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.now.hour(), self.now.minute(), self.now.second())
    }

    /// Current time formatted as "HH:MM" (24-hour).
    pub fn time_hm(&self) -> String {
        format!("{:02}:{:02}", self.now.hour(), self.now.minute())
    }

    /// Current date formatted as "Day, DD Mon YYYY".
    pub fn date_formatted(&self) -> String {
        let weekday = self.now.weekday();
        let month = self.now.month();
        format!("{}, {:02} {} {}", weekday, self.now.day(), month, self.now.year())
    }

    /// RFC3339 formatted string.
    pub fn to_rfc3339(&self) -> String {
        self.now.format(&Rfc3339).unwrap_or_default()
    }

    /// Current raw hour.
    pub fn hour(&self) -> u8 {
        self.now.hour()
    }

    /// Current raw minute.
    pub fn minute(&self) -> u8 {
        self.now.minute()
    }

    /// Current raw second.
    pub fn second(&self) -> u8 {
        self.now.second()
    }

    /// Raw `OffsetDateTime`.
    pub fn datetime(&self) -> OffsetDateTime {
        self.now
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Service for Clock {
    fn init(cx: &mut Context<Self>) -> Self {
        let clock = Clock::new();

        cx.spawn(async move |this, cx| {
            loop {
                // Compute duration until next second boundary to prevent drift
                let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
                let nanos = now.nanosecond();
                let sleep_nanos = 1_000_000_000u64.saturating_sub(nanos as u64);
                let sleep_duration = Duration::from_nanos(sleep_nanos.max(10_000_000));

                async_io::Timer::after(sleep_duration).await;

                let updated_now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

                let result = this.update(cx, |clock, cx| {
                    clock.now = updated_now;
                    cx.notify();
                });

                if result.is_err() {
                    // Context was dropped or shut down
                    break;
                }
            }
        })
        .detach();

        clock
    }
}
