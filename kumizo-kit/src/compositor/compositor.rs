use gpui::Context;
use tracing::{info, warn};

use crate::compositor::hyprland::HyprlandIpc;
use crate::compositor::niri::NiriIpc;
use crate::compositor::sway::SwayIpc;
use crate::compositor::types::{
    ActiveWindow, AppWindow, CompositorEvent, CompositorKind, Monitor, Workspace,
};
use crate::services::Service;

/// Detected backend, kept alive so every action reuses the same socket paths and
/// dispatcher-style detection instead of re-probing the compositor per click.
enum Backend {
    Hyprland(HyprlandIpc),
    Sway(SwayIpc),
    Niri(NiriIpc),
    Fallback,
}

impl Backend {
    fn kind(&self) -> CompositorKind {
        match self {
            Self::Hyprland(_) => CompositorKind::Hyprland,
            Self::Sway(_) => CompositorKind::Sway,
            Self::Niri(_) => CompositorKind::Niri,
            Self::Fallback => CompositorKind::Fallback,
        }
    }

    /// Whether the backend's workspace JSON carries an urgency flag.
    ///
    /// Sway and Niri do, so their list is authoritative. Hyprland only reports
    /// urgency through the `urgent` event and would otherwise lose it on every
    /// refresh, so the service has to carry it over itself.
    fn reports_workspace_urgency(&self) -> bool {
        matches!(self, Self::Sway(_) | Self::Niri(_))
    }
}

/// Asynchronous compositor state service.
///
/// Automatically detects Hyprland, Sway, or Niri. Manages active workspaces,
/// focused windows, monitors and window management commands with Zero Idle CPU.
///
/// The state is always replaced from the compositor's authoritative JSON replies
/// on state-changing events; deltas are never replayed. That is what makes named
/// and special Hyprland workspaces (`web`, `special:magic`) safe: they have no
/// numeric id, so a replayed event stream would inevitably drift.
pub struct Compositor {
    backend: Backend,
    workspaces: Vec<Workspace>,
    active_workspace_id: i32,
    active_window: Option<ActiveWindow>,
    windows: Vec<AppWindow>,
    monitors: Vec<Monitor>,
}

impl Compositor {
    /// Create a new compositor service instance with fallback defaults.
    pub fn new() -> Self {
        let default_workspaces = (1..=5)
            .map(|i| Workspace::new(i, i.to_string(), i == 1))
            .collect();

        Self {
            backend: Backend::Fallback,
            workspaces: default_workspaces,
            active_workspace_id: 1,
            active_window: None,
            windows: Vec::new(),
            monitors: Vec::new(),
        }
    }

    /// Detected compositor kind.
    pub fn kind(&self) -> CompositorKind {
        self.backend.kind()
    }

    /// List of current workspaces.
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Currently active workspace ID.
    pub fn active_workspace_id(&self) -> i32 {
        self.active_workspace_id
    }

    /// The active workspace descriptor, when it is in the current list.
    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.id == self.active_workspace_id)
    }

    /// Currently active window title and class (if any).
    pub fn active_window(&self) -> Option<&ActiveWindow> {
        self.active_window.as_ref()
    }

    /// List of all currently open application windows.
    pub fn windows(&self) -> &[AppWindow] {
        &self.windows
    }

    /// List of monitors (outputs) and their active workspaces.
    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// The focused monitor, when the backend reports one.
    pub fn focused_monitor(&self) -> Option<&Monitor> {
        self.monitors
            .iter()
            .find(|monitor| monitor.focused)
            .or_else(|| self.monitors.first())
    }

    /// Resolve the backend workspace selector for a [`Workspace::id`].
    ///
    /// Named and special workspaces have no numeric id; their real selector lives
    /// in [`Workspace::address`].
    fn workspace_address(&self, id: i32) -> String {
        self.workspaces
            .iter()
            .find(|workspace| workspace.id == id)
            .map(|workspace| {
                if workspace.address.is_empty() {
                    workspace.name.clone()
                } else {
                    workspace.address.clone()
                }
            })
            .filter(|address| !address.is_empty())
            .unwrap_or_else(|| id.to_string())
    }

    fn dispatch_workspace(&self, address: &str) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.switch_workspace_address(address),
            Backend::Sway(sway) => sway.switch_workspace_address(address),
            Backend::Niri(niri) => niri.switch_workspace_address(address),
            Backend::Fallback => Ok(()),
        }
    }

    /// Switch workspace by ID.
    ///
    /// The highlight is updated optimistically on every backend. Waiting for the
    /// compositor to echo a `focus` event would leave the click feeling dead,
    /// since the round trip includes a compositor round trip; if the command
    /// fails or the workspace does not exist, the next event corrects it.
    pub fn switch_workspace(&mut self, id: i32) {
        let _ = self.try_switch_workspace(id);
    }

    /// Switch workspace by ID and report a dispatch failure.
    pub fn try_switch_workspace(&mut self, id: i32) -> anyhow::Result<()> {
        let address = self.workspace_address(id);
        self.dispatch_workspace(&address)?;

        self.active_workspace_id = id;
        for workspace in &mut self.workspaces {
            workspace.is_active = workspace.id == id;
        }
        Ok(())
    }

    /// Switch to a workspace using its backend-native selector.
    ///
    /// Hyprland accepts `"3"`, `"web"` and `"special:magic"` (and
    /// `"name:special:x"` for a named workspace whose name collides with a
    /// special address). Sway and Niri accept the workspace name.
    pub fn switch_workspace_address(&mut self, address: &str) -> anyhow::Result<()> {
        self.dispatch_workspace(address)?;

        if let Some(workspace) = self
            .workspaces
            .iter()
            .find(|workspace| workspace.address == address || workspace.name == address)
        {
            self.active_workspace_id = workspace.id;
            let id = workspace.id;
            for workspace in &mut self.workspaces {
                workspace.is_active = workspace.id == id;
            }
        }
        Ok(())
    }

    /// Focus an application window by its backend window id.
    ///
    /// Hyprland: the 64-bit address (`AppWindow::id`). Sway: node id. Niri:
    /// window id.
    pub fn focus_window(&self, con_id: i64) {
        let _ = self.try_focus_window(con_id);
    }

    /// Focus a window and report a dispatch failure.
    pub fn try_focus_window(&self, con_id: i64) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.focus_window(con_id),
            Backend::Sway(sway) => sway.focus_window(con_id),
            Backend::Niri(niri) => niri.focus_window(con_id),
            Backend::Fallback => Ok(()),
        }
    }

    /// Close the active window.
    pub fn close_window(&self) {
        let _ = self.try_close_window();
    }

    /// Close the active window and report a dispatch failure.
    pub fn try_close_window(&self) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.close_window(),
            Backend::Sway(sway) => sway.close_window(),
            Backend::Niri(niri) => niri.close_window(),
            Backend::Fallback => Ok(()),
        }
    }

    /// Close a specific window by its backend window id.
    pub fn close_window_id(&self, con_id: i64) {
        let _ = self.try_close_window_id(con_id);
    }

    /// Close a specific window and report a dispatch failure.
    pub fn try_close_window_id(&self, con_id: i64) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.close_window_id(con_id),
            Backend::Sway(sway) => sway.close_window_id(con_id),
            Backend::Niri(niri) => niri.close_window_id(con_id),
            Backend::Fallback => Ok(()),
        }
    }

    /// Toggle floating state for a window (or the active one).
    pub fn toggle_floating(&self, con_id: Option<i64>) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.toggle_floating(con_id),
            Backend::Sway(sway) => sway.toggle_floating(con_id),
            Backend::Niri(niri) => niri.toggle_floating(con_id),
            Backend::Fallback => Ok(()),
        }
    }

    /// Toggle fullscreen for a window (or the active one).
    ///
    /// Hyprland before 0.55 (legacy `hyprlang`) only supports the active window,
    /// so asking for a specific window there returns an error rather than
    /// toggling the wrong one.
    pub fn toggle_fullscreen(&self, con_id: Option<i64>) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.toggle_fullscreen(con_id),
            Backend::Sway(sway) => sway.toggle_fullscreen(con_id),
            Backend::Niri(niri) => niri.toggle_fullscreen(con_id),
            Backend::Fallback => Ok(()),
        }
    }

    /// Move a window (or the active one) to a workspace.
    pub fn move_window_to_workspace(
        &self,
        con_id: Option<i64>,
        workspace: &str,
        follow: bool,
    ) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => {
                hypr.move_window_to_workspace(con_id, workspace, follow)
            }
            Backend::Sway(sway) => sway.move_window_to_workspace(con_id, workspace, follow),
            Backend::Niri(niri) => niri.move_window_to_workspace(con_id, workspace, follow),
            Backend::Fallback => Ok(()),
        }
    }

    /// Focus a monitor/output by name (`DP-1`, `eDP-1`, …).
    pub fn focus_monitor(&self, name: &str) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.focus_monitor(name),
            Backend::Sway(sway) => sway.focus_monitor(name),
            Backend::Niri(niri) => niri.focus_monitor(name),
            Backend::Fallback => Ok(()),
        }
    }

    /// Toggle a Hyprland special (scratchpad) workspace by name.
    ///
    /// Accepts both `"magic"` and `"special:magic"`. Other backends have no
    /// scratchpad concept and return an error.
    pub fn toggle_special_workspace(&self, name: &str) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.toggle_special_workspace(name),
            _ => anyhow::bail!("special workspaces are a Hyprland feature"),
        }
    }

    /// Reload the compositor configuration.
    pub fn reload_config(&self) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Hyprland(hypr) => hypr.reload_config(),
            Backend::Sway(sway) => sway.reload_config(),
            Backend::Niri(niri) => niri.reload_config(),
            Backend::Fallback => Ok(()),
        }
    }

    /// Whether a Hyprland special (scratchpad) workspace is currently visible.
    ///
    /// A special workspace is shown *on top of* the regular one, so the active
    /// regular workspace stays highlighted; this reports the scratchpad.
    pub fn active_workspace_is_special(&self) -> bool {
        self.workspaces
            .iter()
            .any(|workspace| workspace.is_active && workspace.is_special())
    }

    /// Replace the window list, then keep the active window and workspace window
    /// counts consistent with it.
    fn apply_windows(&mut self, mut windows: Vec<AppWindow>) {
        // Backends that only expose a workspace id (Niri) leave the display name
        // as the numeric id. Fill it from the authoritative workspace list.
        for window in &mut windows {
            if window.workspace_name.is_empty()
                || window.workspace_name == window.workspace_id.to_string()
            {
                if let Some(workspace) = self
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.id == window.workspace_id)
                {
                    window.workspace_name = workspace.name.clone();
                }
            }
        }

        self.active_window = windows
            .iter()
            .find(|window| window.is_focused)
            .map(|window| ActiveWindow {
                title: window.title.clone(),
                class: window.class.clone(),
                workspace_id: window.workspace_id,
            });

        // Count windows per workspace. Match on the backend id first: it is
        // unique, whereas two workspaces can share a display name (a real
        // special workspace `magic` and a named workspace `magic`). Only fall
        // back to the name when the backend gave no usable id.
        let mut counts = vec![0usize; self.workspaces.len()];
        for window in &windows {
            let index = self
                .workspaces
                .iter()
                .position(|workspace| workspace.id != 0 && workspace.id == window.workspace_id)
                .or_else(|| {
                    if window.workspace_name.is_empty() {
                        return None;
                    }
                    self.workspaces.iter().position(|workspace| {
                        workspace.address == window.workspace_name
                            || workspace.name == window.workspace_name
                    })
                });
            if let Some(index) = index {
                counts[index] += 1;
            }
        }
        for (workspace, count) in self.workspaces.iter_mut().zip(counts) {
            workspace.window_count = count;
        }

        self.windows = windows;
    }

    /// Clear the urgency hint of a workspace once the user is looking at it.
    fn clear_workspace_urgency(&mut self, workspace_id: i32) {
        if let Some(workspace) = self
            .workspaces
            .iter_mut()
            .find(|workspace| workspace.id == workspace_id)
        {
            workspace.is_urgent = false;
        }
    }

    /// Mark the workspace owning `window_id` as urgent.
    fn mark_window_urgent(&mut self, window_id: i64) {
        let Some(window) = self.windows.iter().find(|window| window.id == window_id) else {
            return;
        };

        let workspace_id = window.workspace_id;
        let workspace_name = window.workspace_name.clone();

        // Prefer the unique id; only fall back to the display name when the
        // backend had none, so a named workspace cannot mark its same-named
        // special twin as urgent.
        if workspace_id != 0 {
            if let Some(workspace) = self
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == workspace_id)
            {
                workspace.is_urgent = true;
                return;
            }
        }

        if workspace_name.is_empty() {
            return;
        }
        for workspace in &mut self.workspaces {
            if workspace.name == workspace_name || workspace.address == workspace_name {
                workspace.is_urgent = true;
            }
        }
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Service for Compositor {
    fn init(cx: &mut Context<Self>) -> Self {
        let mut comp = Compositor::new();
        let (tx, rx) = async_channel::bounded::<CompositorEvent>(64);

        // Detect backend. Keep the handle: re-detecting on every action would
        // rebuild the Hyprland dispatcher-style cache and could pick a different
        // socket after a compositor restart.
        if let Some(hypr) = HyprlandIpc::try_new() {
            info!("Kumizo: Connected to Hyprland IPC");
            if let Ok(workspaces) = hypr.fetch_workspaces() {
                if !workspaces.is_empty() {
                    comp.workspaces = workspaces;
                }
            }
            comp.active_workspace_id =
                reported_active_workspace_id(&comp.workspaces).unwrap_or(comp.active_workspace_id);
            if let Ok(windows) = hypr.fetch_windows() {
                comp.apply_windows(windows);
            }
            if let Ok(monitors) = hypr.fetch_monitors() {
                comp.monitors = monitors;
            }
            if comp.active_window.is_none() {
                if let Ok(window) = hypr.fetch_active_window() {
                    comp.active_window = window;
                }
            }
            let _ = hypr.start_event_loop(tx);
            comp.backend = Backend::Hyprland(hypr);
        } else if let Some(sway) = SwayIpc::try_new() {
            info!("Kumizo: Connected to Sway IPC");
            if let Ok(workspaces) = sway.fetch_workspaces() {
                if !workspaces.is_empty() {
                    comp.workspaces = workspaces;
                }
            }
            comp.active_workspace_id =
                reported_active_workspace_id(&comp.workspaces).unwrap_or(comp.active_workspace_id);
            if let Ok(windows) = sway.fetch_windows() {
                comp.apply_windows(windows);
            }
            if let Ok(monitors) = sway.fetch_monitors() {
                comp.monitors = monitors;
            }
            let _ = sway.start_event_loop(tx);
            comp.backend = Backend::Sway(sway);
        } else if let Some(niri) = NiriIpc::try_new() {
            info!("Kumizo: Connected to Niri IPC");
            if let Ok(workspaces) = niri.fetch_workspaces() {
                if !workspaces.is_empty() {
                    comp.workspaces = workspaces;
                }
            }
            comp.active_workspace_id =
                reported_active_workspace_id(&comp.workspaces).unwrap_or(comp.active_workspace_id);
            if let Ok(windows) = niri.fetch_windows_default() {
                comp.apply_windows(windows);
            } else if let Ok(windows) = niri.fetch_windows(&comp.workspaces) {
                comp.apply_windows(windows);
            } else if let Ok(windows) = niri.fetch_windows(&[]) {
                comp.apply_windows(windows);
            }
            if let Ok(monitors) = niri.fetch_monitors(&[]) {
                comp.monitors = monitors;
            } else if let Ok(monitors) = niri.fetch_monitors_default() {
                comp.monitors = monitors;
            }
            let _ = niri.start_event_loop(tx);
            comp.backend = Backend::Niri(niri);
        } else {
            warn!("Kumizo: No supported Wayland compositor IPC socket found. Using fallback mock workspaces.");
        }

        cx.spawn(async move |this, cx| {
            while let Ok(event) = rx.recv().await {
                let res = this.update(cx, |comp, cx| {
                    match event {
                        CompositorEvent::WorkspaceActivated(id) => {
                            comp.active_workspace_id = id;
                            for workspace in &mut comp.workspaces {
                                workspace.is_active = workspace.id == id;
                            }
                            // Looking at the workspace acknowledges its urgency.
                            comp.clear_workspace_urgency(id);
                            cx.notify();
                        }
                        // A workspace was created, removed or moved: replace the
                        // list outright rather than patching it, so a stale entry
                        // can never survive. The fetched flags come from a
                        // synchronous GET_WORKSPACES, so they are the
                        // compositor's truth at fetch time; adopt them and sync
                        // the active id. Only when nothing is marked (transient
                        // fetch mid-switch) keep the previous active id, so the
                        // highlight flickers rather than vanishes.
                        CompositorEvent::WorkspacesChanged(mut workspaces) => {
                            if !workspaces.is_empty() {
                                // Preserve urgency recorded from an `urgent`
                                // event: Hyprland does not report it in
                                // `j/workspaces`, and a refresh between the hint
                                // and the user reacting would otherwise erase it.
                                // Sway/Niri do report urgency, so their fetched
                                // flag is authoritative and must be able to clear.
                                if !comp.backend.reports_workspace_urgency() {
                                    for workspace in &mut workspaces {
                                        if let Some(previous) = comp.workspaces.iter().find(|old| {
                                            old.id == workspace.id
                                                || (!old.address.is_empty()
                                                    && old.address == workspace.address)
                                        }) {
                                            workspace.is_urgent |= previous.is_urgent;
                                        }
                                    }
                                }

                                if let Some(focused) =
                                    reported_active_workspace_id(&workspaces)
                                {
                                    comp.active_workspace_id = focused;
                                } else if !workspaces
                                    .iter()
                                    .any(|workspace| workspace.id == comp.active_workspace_id)
                                {
                                    comp.active_workspace_id =
                                        workspaces.first().map(|w| w.id).unwrap_or(0);
                                    let id = comp.active_workspace_id;
                                    for workspace in &mut workspaces {
                                        workspace.is_active = workspace.id == id;
                                    }
                                } else {
                                    let id = comp.active_workspace_id;
                                    for workspace in &mut workspaces {
                                        workspace.is_active = workspace.id == id;
                                    }
                                }
                                comp.workspaces = workspaces;
                                comp.clear_workspace_urgency(comp.active_workspace_id);
                                cx.notify();
                            }
                        }
                        CompositorEvent::WindowFocused(window) => {
                            if window.workspace_id != 0 {
                                comp.clear_workspace_urgency(window.workspace_id);
                            }
                            comp.active_window = Some(window);
                            cx.notify();
                        }
                        CompositorEvent::WindowFocusCleared => {
                            comp.active_window = None;
                            for window in &mut comp.windows {
                                window.is_focused = false;
                            }
                            cx.notify();
                        }
                        CompositorEvent::WindowUrgent(id) => {
                            comp.mark_window_urgent(id);
                            cx.notify();
                        }
                        CompositorEvent::WindowOpenedOrChanged(window) => {
                            if let Some(existing) = comp.windows.iter_mut().find(|w| w.id == window.id) {
                                *existing = window.clone();
                            } else {
                                comp.windows.push(window.clone());
                            }
                            if window.is_focused {
                                comp.active_window = Some(ActiveWindow {
                                    title: window.title.clone(),
                                    class: window.class.clone(),
                                    workspace_id: window.workspace_id,
                                });
                                if window.workspace_id != 0 {
                                    comp.clear_workspace_urgency(window.workspace_id);
                                }
                            }
                            // Recompute window counts
                            let mut counts = vec![0usize; comp.workspaces.len()];
                            for w in &comp.windows {
                                if let Some(index) = comp
                                    .workspaces
                                    .iter()
                                    .position(|ws| ws.id != 0 && ws.id == w.workspace_id)
                                    .or_else(|| {
                                        if w.workspace_name.is_empty() {
                                            return None;
                                        }
                                        comp.workspaces.iter().position(|ws| {
                                            ws.address == w.workspace_name
                                                || ws.name == w.workspace_name
                                        })
                                    })
                                {
                                    counts[index] += 1;
                                }
                            }
                            for (workspace, count) in comp.workspaces.iter_mut().zip(counts) {
                                workspace.window_count = count;
                            }
                            cx.notify();
                        }
                        CompositorEvent::WindowClosed(id) => {
                            comp.windows.retain(|w| w.id != id);
                            if let Some(active) = comp.active_window.as_ref() {
                                // If active window was closed, we don't know new focus yet; events will update
                            }
                            let mut counts = vec![0usize; comp.workspaces.len()];
                            for w in &comp.windows {
                                if let Some(index) = comp
                                    .workspaces
                                    .iter()
                                    .position(|ws| ws.id != 0 && ws.id == w.workspace_id)
                                    .or_else(|| {
                                        if w.workspace_name.is_empty() {
                                            return None;
                                        }
                                        comp.workspaces.iter().position(|ws| {
                                            ws.address == w.workspace_name
                                                || ws.name == w.workspace_name
                                        })
                                    })
                                {
                                    counts[index] += 1;
                                }
                            }
                            for (workspace, count) in comp.workspaces.iter_mut().zip(counts) {
                                workspace.window_count = count;
                            }
                            cx.notify();
                        }
                        CompositorEvent::WindowsChanged(windows) => {
                            comp.apply_windows(windows);
                            cx.notify();
                        }
                        CompositorEvent::MonitorsChanged(monitors) => {
                            comp.monitors = monitors;
                            // A monitor's active workspace is authoritative for
                            // multi-output setups; align the highlighted id.
                            if let Some(id) = comp
                                .focused_monitor()
                                .and_then(|monitor| {
                                    comp.workspaces
                                        .iter()
                                        .find(|workspace| {
                                            workspace.address == monitor.active_workspace
                                                || workspace.name
                                                    == monitor.active_workspace_name
                                        })
                                        .map(|workspace| workspace.id)
                                })
                            {
                                comp.active_workspace_id = id;
                                comp.clear_workspace_urgency(id);
                                // Keep every visible special workspace marked:
                                // it sits on top of the regular one.
                                let specials: Vec<String> = comp
                                    .monitors
                                    .iter()
                                    .filter_map(|monitor| monitor.special_workspace.clone())
                                    .collect();
                                for workspace in &mut comp.workspaces {
                                    workspace.is_active = workspace.id == id
                                        || specials
                                            .iter()
                                            .any(|address| *address == workspace.address);
                                }
                            }
                            cx.notify();
                        }
                    }
                });

                if res.is_err() {
                    break;
                }
            }
        })
        .detach();

        comp
    }

    /// Expose compositor actions through `kumizo service compositor <action>`.
    ///
    /// Accepted actions: `switch_workspace <id-or-address>`, `focus_window <id>`,
    /// `close_window [<id>]`, `toggle_floating [<id>]`,
    /// `toggle_fullscreen [<id>]`, `move_window <workspace> [<id>]`,
    /// `focus_monitor <name>`, `toggle_special <name>`, `reload_config`.
    fn ipc_action(
        &mut self,
        cx: &mut Context<Self>,
        action: &str,
        payload: Option<&str>,
    ) -> bool {
        let payload = payload.map(str::trim).filter(|value| !value.is_empty());

        match action {
            "switch_workspace" | "switch-workspace" => {
                let Some(value) = payload else {
                    return false;
                };
                if let Ok(id) = value.parse::<i32>() {
                    self.switch_workspace(id);
                } else if self.switch_workspace_address(value).is_err() {
                    return false;
                }
                cx.notify();
                true
            }
            "focus_window" | "focus-window" => {
                let Some(id) = payload.and_then(|value| parse_window_id(value)) else {
                    return false;
                };
                if self.try_focus_window(id).is_err() {
                    return false;
                }
                true
            }
            "close_window" | "close-window" => match payload {
                Some(value) => {
                    let Some(id) = parse_window_id(value) else {
                        return false;
                    };
                    self.try_close_window_id(id).is_ok()
                }
                None => self.try_close_window().is_ok(),
            },
            "toggle_floating" | "toggle-floating" => {
                let id = match payload {
                    Some(value) => match parse_window_id(value) {
                        Some(id) => Some(id),
                        None => return false,
                    },
                    None => None,
                };
                self.toggle_floating(id).is_ok()
            }
            "toggle_fullscreen" | "toggle-fullscreen" => {
                let id = match payload {
                    Some(value) => match parse_window_id(value) {
                        Some(id) => Some(id),
                        None => return false,
                    },
                    None => None,
                };
                self.toggle_fullscreen(id).is_ok()
            }
            "move_window" | "move-window" => {
                let Some(value) = payload else {
                    return false;
                };
                // `move_window <workspace>` or `move_window <workspace> <window-id>`.
                let mut parts = value.split_whitespace();
                let Some(workspace) = parts.next() else {
                    return false;
                };
                let id = match parts.next() {
                    Some(value) => match parse_window_id(value) {
                        Some(id) => Some(id),
                        None => return false,
                    },
                    None => None,
                };
                self.move_window_to_workspace(id, workspace, true).is_ok()
            }
            "focus_monitor" | "focus-monitor" => {
                let Some(name) = payload else {
                    return false;
                };
                self.focus_monitor(name).is_ok()
            }
            "toggle_special" | "toggle-special" => {
                let Some(name) = payload else {
                    return false;
                };
                self.toggle_special_workspace(name).is_ok()
            }
            "reload_config" | "reload-config" | "reload" => self.reload_config().is_ok(),
            _ => false,
        }
    }
}

/// Pick the workspace the bar should highlight.
///
/// A visible special (scratchpad) workspace is also `is_active`, but the regular
/// workspace underneath it owns the highlight; the special stays reachable
/// through its own pill. Prefer a regular workspace and only fall back to a
/// special one when no regular workspace is marked active.
fn reported_active_workspace_id(workspaces: &[Workspace]) -> Option<i32> {
    workspaces
        .iter()
        .find(|workspace| workspace.is_active && !workspace.is_special())
        .or_else(|| workspaces.iter().find(|workspace| workspace.is_active))
        .map(|workspace| workspace.id)
}

/// Parse a window id from decimal (`1234`) or Hyprland address (`0x1234`).
fn parse_window_id(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return i64::from_str_radix(hex, 16).ok();
    }
    value
        .parse::<i64>()
        .ok()
        .or_else(|| i64::from_str_radix(value, 16).ok())
}
