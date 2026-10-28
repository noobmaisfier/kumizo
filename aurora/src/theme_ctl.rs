//! `theme` service — drives the aurora theme from the command line.
//!
//! The theme is a global state (`theme::CURRENT`); this service only exposes
//! it to `kumizo`'s IPC actions:
//!
//! ```text
//! kumizo service theme set "Flexoki Light"
//! kumizo service theme set 3
//! kumizo service theme next
//! kumizo service theme prev
//! ```
//!
//! This works because `Service::ipc_action` now receives a
//! `&mut Context<Self>` that dereferences to `App`, so we can call
//! `theme::set(index, cx)` (which repaints all windows).

use kumizo_kit::prelude::*;

use crate::theme;

/// The type name determines the CLI name: `short_service_name` takes the last
/// path segment, lowercased. `Theme` → `kumizo service theme …`.
pub struct Theme;

impl Service for Theme {
    fn init(_cx: &mut Context<Self>) -> Self {
        Self
    }

    fn ipc_action(
        &mut self,
        cx: &mut Context<Self>,
        action: &str,
        payload: Option<&str>,
    ) -> bool {
        let n = theme::THEMES.len();
        match action {
            "set" => {
                let Some(target) = payload else {
                    return false;
                };
                let Some(index) = theme::resolve(target) else {
                    return false;
                };
                theme::set(index, cx);
                true
            }
            "next" => {
                theme::set((theme::current_index() + 1) % n, cx);
                true
            }
            "prev" => {
                theme::set((theme::current_index() + n - 1) % n, cx);
                true
            }
            _ => false,
        }
    }
}
