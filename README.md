# Kumizo

[![Rust](https://img.shields.io/badge/rust-edition%202021-orange.svg)](https://www.rust-lang.org)
[![Wayland](https://img.shields.io/badge/wayland-layer--shell-blue.svg)](https://wayland.freedesktop.org/)
[![GPUI](https://img.shields.io/badge/render-GPUI%20(wgpu)-green.svg)](https://github.com/zed-industries/zed)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-lightgrey.svg)](LICENSE-MIT)

**Kumizo** (`kumizo-kit`) is a pure-Rust SDK for building your own Wayland desktop shell — status bars, docks, launchers, OSDs, popups, panels — as **one single process**, rendered with GPU acceleration (GPUI/wgpu).

- **One process** — one GPU init, one font stack, one socket; every window and widget lives in it. A shell is a single `main()` declaring services and windows once.
- **GPU-accelerated rendering** — GPUI/wgpu, no web or interpreted layer.
- **Pure Rust widgets** — a widget is a plain struct implementing `gpui::Render`; total layout freedom, no imposed UI kit.
- **Zero idle CPU** — services are event-driven (D-Bus signals, compositor sockets); nothing redraws unless `cx.notify()` is called.
- **Scriptable CLI** — `kumizo ping`, `kumizo toggle <window>`, `kumizo service <name> <action>` drive a running shell over its socket.

## Build your own shell

Requirements: Linux + Wayland, a Rust stable toolchain, and `gcc`.

```bash
cargo new my-shell && cd my-shell
cargo add kumizo-kit
cargo add gpui-pre --rename gpui --version "=0.3.8"
```

A shell is services + windows + `run()`:

```rust
use gpui::prelude::*;
use kumizo_kit::prelude::*;

fn main() {
    Kumizo::new()
        .service::<Clock>()
        .window(
            LayerWindow::new("bar")
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(36),
            |cx: &mut gpui::App| cx.new(MyBar::new),
        )
        .run();
}
```

A widget is a view reading services:

```rust
struct MyBar { clock: Entity<Clock> }

impl MyBar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { clock: cx.service::<Clock>() }
    }
}

impl Render for MyBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(self.clock.read(cx).time_hms())
    }
}
```

See `examples/aurora` for a complete example shell, and [`docs/GUIDE.md`](docs/GUIDE.md) for the widget recipe, services reference, and GPUI pitfalls.

### Install the CLI

```bash
cargo install kumizo
```

This installs the `kumizo` binary (send commands to a running shell over its socket).

## CLI

```bash
kumizo ping                       # PONG
kumizo toggle aurora-launcher     # show/hide a window
kumizo service audio toggle_mute  # scriptable, key-free control
```

`kumizo` (`wk-ctl`) talks to your shell over its UNIX socket (`$XDG_RUNTIME_DIR/kumizo.sock`).

- **Native (no code needed):** `ping`, `status`, `show <window>`, `hide <window>`, `toggle <window>`, `reload` — handled by the IPC core for every window you declare.
- **Service actions:** `kumizo service <name> <action>` calls `ipc_action` on the registered service. SDK services (`Audio`, `Battery`, `Mpris`, `Brightness`, `Network`, `Tray`, `Clipboard`, `Notifications`, …) already implement their actions, so `kumizo service audio toggle_mute` works out of the box once you register `.service::<Audio>()`. For your own custom services, implement `Service::ipc_action` to expose CLI actions.

IPC is on by default (`.ipc(true)`).

## License

MIT OR Apache-2.0 (see `LICENSE-MIT`, `LICENSE-APACHE`).
