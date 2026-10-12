//! Service `theme` — pilote le thème d'aurora depuis la ligne de commande.
//!
//! Le thème est un état global (`theme::CURRENT`) ; ce service ne fait que
//! l'exposer aux actions IPC de `kumizo` :
//!
//! ```text
//! kumizo service theme set "Flexoki Light"
//! kumizo service theme set 3
//! kumizo service theme next
//! kumizo service theme prev
//! ```
//!
//! C'est possible parce que `Service::ipc_action` reçoit désormais un
//! `&mut Context<Self>` qui déréférence vers `App`, donc on peut appeler
//! `theme::set(index, cx)` (qui repaint toutes les fenêtres).

use kumizo_kit::prelude::*;

use crate::theme;

/// Le nom du type détermine le nom CLI : `short_service_name` prend le dernier
/// segment du chemin, en minuscules. `Theme` → `kumizo service theme …`.
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
