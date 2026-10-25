//! # Kumizo (V1)
//!
//! **Kumizo** is a high-performance, pure-Rust SDK dedicated to crafting modern
//! Wayland desktop environments (status bars, docks, on-screen displays, notifications).
//!
//! Unlike existing web- or interpreted-based solutions (AGS, Eww, Waybar), Kumizo is powered
//! by the **GPUI** rendering engine (Vulkan/wgpu GPU acceleration) and embraces a pure **"Headless"**
//! philosophy:
//! - **Zero Idle CPU**: Interfaces never redraw unless explicitly requested via `cx.notify()`.
//! - **100% GPU Rendering**: Direct rendering pipeline via wgpu.
//! - **Unified Event Loop**: GPUI's native async executor drives both UI layout and background D-Bus/IPC requests.
//! - **Pinned upstream engine**: built on a `gpui-pre` snapshot of Zed's GPUI, pinned to an exact version rather than a floating `main`.
//! - **Bring Your Own UI**: No bloated UI widget kit — you design your interface freely with GPUI primitives.
//!
//! ## Layout
//!
//! Everything lives in this crate, grouped by domain:
//! - [`ipc`] — UNIX socket protocol, client, server, window control.
//! - [`ext`] — `wlr-layer-shell` window/anchor/margin wrappers.
//! - [`ui`] — headless UI primitives (text input, actions).
//! - [`services`] — long-lived async services (clock, battery, audio, MPRIS, network, tray…).
//! - [`compositor`] — Sway / Hyprland / Niri IPC integrations.
//! - [`pam`] — minimal Linux-PAM password check.
//! - [`app`] — the [`Kumizo`] builder, window animation and the service registry.
//!
//! ## Features
//!
//! | Feature | Brings |
//! |---------|--------|
//! | `app` (default) | builder, registry, animation, prelude; implies all the rest |
//! | `ipc` | protocol, client, server, `WindowControl` |
//! | `ext` | layer-shell wrappers (pulls `gpui`) |
//! | `ui` | text input primitives (pulls `gpui`) |
//! | `services` | system services (pulls `zbus`, `image`, `resvg`) |
//! | `compositor` | Sway/Hyprland/Niri IPC |
//! | `pam` | Linux-PAM check |
//! | `layer-shell` (default) | real `wlr-layer-shell` surfaces instead of pop-ups |
//! | `full` | alias for the defaults (`app` + `layer-shell`) |
//!
//! A CLI that only talks to the socket can depend on `default-features = false,
//! features = ["ipc"]` and avoid compiling GPUI, wgpu, zbus and friends.
//!
//! ## Quickstart
//!
//! ```no_run
//! use kumizo_kit::prelude::*;
//! use gpui::prelude::*;
//!
//! fn main() {
//!     Kumizo::new()
//!         .service::<Clock>()
//!         .service::<Battery>()
//!         .window(
//!             LayerWindow::new("topbar")
//!                 .layer(Layer::Top)
//!                 .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
//!                 .height(32.0)
//!                 .exclusive_zone(32)
//!                 .namespace("wk-topbar"),
//!             |cx: &mut gpui::App| cx.new(TopBar::new),
//!         )
//!         .run();
//! }
//!
//! struct TopBar {
//!     battery: Entity<Battery>,
//! }
//!
//! impl TopBar {
//!     fn new(cx: &mut Context<Self>) -> Self {
//!         Self { battery: cx.service::<Battery>() }
//!     }
//! }
//!
//! impl Render for TopBar {
//!     fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
//!         let bat = self.battery.read(cx);
//!         div()
//!             .flex()
//!             .justify_between()
//!             .w_full()
//!             .bg(rgb(0x1e1e2e))
//!             .text_color(rgb(0xcdd6f4))
//!             .p_2()
//!             .child(format!("Battery: {}%", bat.percent()))
//!     }
//! }
//! ```

#[cfg(feature = "compositor")]
pub mod compositor;
#[cfg(feature = "ext")]
pub mod ext;
#[cfg(feature = "ipc")]
pub mod ipc;
#[cfg(feature = "pam")]
pub mod pam;
#[cfg(feature = "services")]
pub mod services;
#[cfg(feature = "ui")]
pub mod ui;

#[cfg(feature = "app")]
pub mod anim;
#[cfg(feature = "app")]
pub mod app;
#[cfg(feature = "app")]
pub mod prelude;
#[cfg(feature = "app")]
pub mod registry;

#[cfg(feature = "app")]
pub use anim::{AnimatedRoot, CLOSE_MS, OPEN_MS};
#[cfg(feature = "app")]
pub use app::{Kumizo, WindowControlExt, WindowControlGlobal};
#[cfg(feature = "app")]
pub use registry::{AppContextServiceExt, ServiceContextExt, ServiceRegistry};


