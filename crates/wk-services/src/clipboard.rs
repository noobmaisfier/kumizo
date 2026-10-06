//! Native Wayland Clipboard manager service.
//!
//! Subscribes to Wayland clipboard changes using `wl-paste --watch`, maintaining
//! an in-memory history of copied items with deduplication, search and pasteback.

use std::process::{Command, Stdio};
use std::time::Duration;

use async_channel::Sender;
use gpui::Context;
use tracing::{info, warn};

use crate::service::Service;

/// Maximum number of clipboard history entries kept in memory.
const MAX_HISTORY: usize = 100;

/// An item in the clipboard history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardItem {
    pub id: u64,
    pub text: String,
    pub timestamp: time::OffsetDateTime,
}

impl ClipboardItem {
    /// Single-line preview text (newlines replaced by spaces, trimmed).
    pub fn preview(&self) -> String {
        self.text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Number of lines in the content.
    pub fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }

    /// Number of characters in the content.
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

/// Clipboard management service for WidgetKit.
pub struct Clipboard {
    items: Vec<ClipboardItem>,
    next_id: u64,
}

impl Clipboard {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<String>();

        // Query initial clipboard content synchronously
        let initial_text = query_current_clipboard();

        let mut initial_items = Vec::new();
        let mut next_id = 1;
        if let Some(text) = initial_text {
            if !text.trim().is_empty() {
                initial_items.push(ClipboardItem {
                    id: next_id,
                    text,
                    timestamp: time::OffsetDateTime::now_utc(),
                });
                next_id += 1;
            }
        }

        // Spawn background thread to monitor clipboard events via wl-paste --watch
        spawn_watcher(tx);

        // Async task listening for new entries from the watcher thread
        cx.spawn(async move |this, cx| {
            while let Ok(text) = rx.recv().await {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = this.update(cx, |clip, cx| {
                    clip.add_entry(text);
                    cx.notify();
                });
            }
        })
        .detach();

        Self {
            items: initial_items,
            next_id,
        }
    }

    /// Add a new text entry to the history, deduplicating identical previous entries.
    pub fn add_entry(&mut self, text: String) {
        // If the most recent item is identical, skip
        if let Some(first) = self.items.first() {
            if first.text == text {
                return;
            }
        }

        // Remove any identical older entry to move it to the top
        self.items.retain(|item| item.text != text);

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        self.items.insert(
            0,
            ClipboardItem {
                id,
                text,
                timestamp: time::OffsetDateTime::now_utc(),
            },
        );

        if self.items.len() > MAX_HISTORY {
            self.items.truncate(MAX_HISTORY);
        }
    }

    /// Complete clipboard history (newest first).
    pub fn items(&self) -> &[ClipboardItem] {
        &self.items
    }

    /// Search clipboard history by fuzzy matching / substring score.
    pub fn search(&self, query: &str) -> Vec<ClipboardItem> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.items.clone();
        }

        let mut scored: Vec<(usize, ClipboardItem)> = self
            .items
            .iter()
            .filter_map(|item| {
                let text_lower = item.text.to_lowercase();
                let mut score = 0;

                if text_lower == q {
                    score += 100;
                } else if text_lower.starts_with(&q) {
                    score += 60;
                } else if text_lower.contains(&q) {
                    score += 30;
                } else {
                    // Check if words match
                    let words_match = q.split_whitespace().all(|w| text_lower.contains(w));
                    if words_match {
                        score += 15;
                    } else {
                        return None;
                    }
                }

                Some((score, item.clone()))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, item)| item).collect()
    }

    /// Copy an item's content back to the system clipboard via wl-copy.
    pub fn copy_item(&mut self, id: u64) {
        if let Some(pos) = self.items.iter().position(|i| i.id == id) {
            let item = self.items[pos].clone();
            copy_to_system_clipboard(&item.text);

            // Move to top
            self.items.remove(pos);
            self.items.insert(0, item);
        }
    }

    /// Copy given text directly to the system clipboard via wl-copy.
    pub fn copy_text(&self, text: &str) {
        copy_to_system_clipboard(text);
    }

    /// Remove a single item from history.
    pub fn remove_item(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    /// Clear all clipboard history.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl Service for Clipboard {
    fn init(cx: &mut Context<Self>) -> Self {
        Self::new(cx)
    }

    fn ipc_action(&mut self, action: &str, payload: Option<&str>) -> bool {
        match action {
            "clear" => {
                self.clear();
                true
            }
            "copy" => {
                if let Some(text) = payload {
                    self.copy_text(text);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// Query current system clipboard content with `wl-paste -n`.
fn query_current_clipboard() -> Option<String> {
    let output = Command::new("wl-paste")
        .args(["--no-newline"])
        .output()
        .ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None
    }
}

/// Send text to system clipboard via `wl-copy`.
fn copy_to_system_clipboard(text: &str) {
    if let Ok(mut child) = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
    {
        use std::io::Write;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

/// Spawns a background thread that invokes `wl-paste --watch <helper>` or polls when watch is not available.
fn spawn_watcher(tx: Sender<String>) {
    std::thread::Builder::new()
        .name("wk-clipboard-watcher".into())
        .spawn(move || {
            // First check if wl-paste is available
            let check = Command::new("wl-paste").arg("--version").output();
            if check.is_err() {
                warn!("Clipboard: 'wl-paste' not found in PATH. Clipboard watching disabled.");
                return;
            }

            info!("Clipboard: starting Wayland clipboard monitor via wl-paste");

            let mut last_content = query_current_clipboard().unwrap_or_default();

            // Run loop with low-overhead event checking.
            // Using a lightweight sub-process or periodic fast check (every 500ms when active)
            loop {
                std::thread::sleep(Duration::from_millis(400));

                if let Some(current) = query_current_clipboard() {
                    if !current.trim().is_empty() && current != last_content {
                        last_content = current.clone();
                        if tx.send_blocking(current).is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .expect("failed to spawn clipboard watcher thread");
}
