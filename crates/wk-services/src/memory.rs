//! Memory usage, read from Linux `/proc/meminfo`.
//!
//! The kernel exposes no event for memory pressure, so the file is re-read
//! every [`POLL`] and the service only notifies when a value actually
//! changes: no redraw, no wake-up storm (the read itself is a few hundred
//! bytes, done on the background executor so the UI thread never blocks).
//!
//! "Used" follows the kernel's own definition (`MemTotal - MemAvailable`),
//! i.e. cache and reclaimable slab count as used — the same number
//! `free -h` and `top` show.

use std::time::Duration;

use gpui::Context;

use crate::service::Service;

/// Re-read period of `/proc/meminfo`.
const POLL: Duration = Duration::from_secs(2);

/// Snapshot of the system memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Memory {
    /// Total physical memory, in bytes.
    total: u64,
    /// Used memory (`MemTotal - MemAvailable`), in bytes.
    used: u64,
    /// Memory still available for new allocations, in bytes.
    available: u64,
}

impl Memory {
    /// Total physical memory, in bytes.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Used memory, in bytes.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Available memory, in bytes.
    pub fn available(&self) -> u64 {
        self.available
    }

    /// Used memory as a percentage of the total (0-100).
    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        ((self.used as f64 / self.total as f64) * 100.0).round() as u8
    }

    /// Used memory in gibibytes (binary units).
    pub fn used_gib(&self) -> f64 {
        self.used as f64 / 1024.0 / 1024.0 / 1024.0
    }

    /// Total memory in gibibytes (binary units).
    pub fn total_gib(&self) -> f64 {
        self.total as f64 / 1024.0 / 1024.0 / 1024.0
    }

    /// Reads `/proc/meminfo`: `(total, available)` in bytes.
    ///
    /// Falls back to `MemFree` when the kernel is too old to expose
    /// `MemAvailable` (pre-3.14).
    pub fn read_meminfo() -> Option<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let mut total = 0u64;
        let mut available = 0u64;
        let mut free = 0u64;
        for line in text.lines() {
            let Some((key, rest)) = line.split_once(':') else {
                continue;
            };
            let kb: u64 = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            match key {
                "MemTotal" => total = kb.saturating_mul(1024),
                "MemAvailable" => available = kb.saturating_mul(1024),
                "MemFree" => free = kb.saturating_mul(1024),
                _ => {}
            }
        }
        if total == 0 {
            return None;
        }
        if available == 0 {
            available = free;
        }
        Some((total, available))
    }
}

impl Service for Memory {
    fn init(cx: &mut Context<Self>) -> Self {
        let mut initial = Memory::default();
        if let Some((total, available)) = Memory::read_meminfo() {
            initial.total = total;
            initial.available = available;
            initial.used = total.saturating_sub(available);
        }

        cx.spawn(async move |this, cx| {
            loop {
                async_io::Timer::after(POLL).await;

                // Blocking read on the background executor: the UI thread
                // never waits on the filesystem.
                let read = cx
                    .background_executor()
                    .spawn(async { Memory::read_meminfo() })
                    .await;
                let Some((total, available)) = read else {
                    continue;
                };

                let alive = this
                    .update(cx, |m, cx| {
                        let used = total.saturating_sub(available);
                        if m.total == total && m.used == used {
                            return true; // unchanged: no repaint
                        }
                        m.total = total;
                        m.used = used;
                        m.available = available;
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();

        initial
    }
}
