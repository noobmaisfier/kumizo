use gpui::Context;

/// Core trait representing an asynchronous, reactive system service in Kumizo.
///
/// Services represent system data sources (Clock, Battery, Audio, MPRIS, Network)
/// that run asynchronous background listeners and update their state reactively.
///
/// Under Kumizo's Zero-Idle CPU architecture, services never poll needlessly.
/// When system state changes, the service updates its state and calls `cx.notify()`,
/// triggering a selective GPU render cycle.
pub trait Service: 'static + Sized {
    /// Initialize the service within GPUI's context.
    ///
    /// Implementations typically initialize their state and spawn background
    /// async tasks using `cx.spawn(...)` to listen for system events.
    fn init(cx: &mut Context<Self>) -> Self;

    /// Apply an action requested through `kumizo service <name> <action> [payload]`.
    ///
    /// Runs on the GPUI main thread. `cx` derefs to `App`, so implementations
    /// may touch their own state, call `cx.notify()`, or trigger a global
    /// repaint (e.g. `cx.refresh_windows()` for a theme switch). Returns `true`
    /// when the action was recognised; unknown actions must return `false` so
    /// the caller can report them instead of silently doing nothing.
    fn ipc_action(
        &mut self,
        _cx: &mut Context<Self>,
        action: &str,
        payload: Option<&str>,
    ) -> bool {
        let _ = (action, payload);
        false
    }
}
