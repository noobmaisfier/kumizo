//! # WidgetKit (V1)
//!
//! **WidgetKit** is a high-performance, pure-Rust SDK dedicated to crafting modern
//! Wayland desktop environments (status bars, docks, on-screen displays, notifications).
//!
//! Unlike existing web- or interpreted-based solutions (AGS, Eww, Waybar), WidgetKit is powered
//! by the **GPUI** rendering engine (Vulkan/wgpu GPU acceleration) and embraces a pure **"Headless"**
//! philosophy:
//! - **Zero Idle CPU**: Interfaces never redraw unless explicitly requested via `cx.notify()`.
//! - **100% GPU Rendering**: Direct rendering pipeline via wgpu.
//! - **Unified Event Loop**: GPUI's native async executor drives both UI layout and background D-Bus/IPC requests.
//! - **Zero Fork**: Uses upstream GPUI main branch directly.
//! - **Bring Your Own UI**: No bloated UI widget kit — you design your interface freely with GPUI primitives.
//!
//! ## Quickstart
//!
//! ```no_run
//! use widgetkit::prelude::*;
//! use gpui::prelude::*;
//!
//! fn main() {
//!     WidgetKit::new()
//!         .service::<Clock>()
//!         .service::<Battery>()
//!         .window(
//!             LayerWindow::new("topbar")
//!                 .layer(Layer::Top)
//!                 .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
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

pub mod app;
pub mod prelude;
pub mod registry;

pub use app::WidgetKit;
pub use registry::{AppContextServiceExt, ServiceContextExt, ServiceRegistry};
pub use wk_compositor as compositor;
pub use wk_gpui_ext as ext;
pub use wk_ipc as ipc;
pub use wk_services as services;
