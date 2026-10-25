/// Defines how the layer surface interacts with keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyboardInteractivity {
    /// Surface never receives keyboard focus. Ideal for status bars and docks.
    #[default]
    None,
    /// Surface exclusively grabs all keyboard input while active.
    /// Essential for app launchers, run dialogs, and screen lockers.
    Exclusive,
    /// Surface receives keyboard focus interactively on click or demand.
    OnDemand,
}
