# WidgetKit (V1)

[![Rust](https://img.shields.io/badge/rust-edition%202021-orange.svg)](https://www.rust-lang.org)
[![Wayland](https://img.shields.io/badge/wayland-layer--shell-blue.svg)](https://wayland.freedesktop.org/)
[![GPUI](https://img.shields.io/badge/render-GPUI%20(wgpu)-green.svg)](https://github.com/zed-industries/zed)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-lightgrey.svg)](LICENSE)

**WidgetKit** is a high-performance, pure-Rust SDK dedicated to crafting modern Wayland desktop environments (status bars, docks, on-screen displays, notifications).

Unlike existing web- or interpreted-based solutions (AGS, Eww, Waybar), WidgetKit is powered by the **GPUI** rendering engine (Vulkan/wgpu GPU acceleration) and embraces a pure **"Headless"** philosophy:
- **Zero Idle CPU**: Interfaces never redraw unless explicitly requested via `cx.notify()`.
- **100% GPU Rendering**: Direct rendering pipeline via Vulkan/wgpu.
- **Unified Event Loop**: GPUI's native async executor drives both UI layout and background D-Bus/IPC requests (via `zbus` and `async-io`).
- **Zero Fork**: Depends on upstream `gpui` directly.
- **Bring Your Own UI**: No bloated or restrictive UI widget kit — you design your interface freely with GPUI primitives (`div()`, `text()`, `svg()`, `rgb()`, `px()`).

---

## Contributing

**Before touching this codebase, read [`docs/REFERENTIEL.md`](docs/REFERENTIEL.md).** GPUI has no stable API and this project tracks its `main` branch, so the diff between one working commit and the next is large. The reference documents the API traps, the mistakes not to repeat, and how to verify a change actually works.

---

## Workspace Architecture

```
widgetkit-workspace/
├── Cargo.toml                # Workspace manifest
├── crates/
│   ├── widgetkit/            # Public façade (Builder API, prelude, service registry)
│   ├── wk-gpui-ext/          # LayerShellOptions, Anchor, Layer, Margins builder
│   ├── wk-services/          # Asynchronous system services (Clock, Battery, Audio, MPRIS)
│   ├── wk-compositor/        # Native IPC sockets for Hyprland, Sway, and Niri
│   ├── wk-ipc/               # UNIX domain socket daemon server & client protocol
│   └── wk-ctl/               # widgetctl command-line utility
└── examples/
    ├── topbar/               # Production-ready status bar (Workspaces, Clock, MPRIS, Audio, Battery)
    ├── dock/                 # Interactive macOS/iOS-style application dock with running indicators
    └── osd/                  # Low-latency, zero-idle volume/brightness On-Screen Display
```

---

## The "Bring Your Own UI" Model

WidgetKit abstracts all Wayland layer-shell complexity and asynchronous D-Bus services, while leaving complete aesthetic freedom to you:

```rust
use widgetkit::prelude::*;
use gpui::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .window(
            LayerWindow::new("topbar")
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(32)
                .namespace("wk-topbar"),
            |cx: &mut gpui::App| cx.new(TopBar::new),
        )
        .run();
}

struct TopBar {
    battery: Entity<Battery>,
}

impl TopBar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            battery: cx.service::<Battery>(),
        }
    }
}

// Build your UI with GPUI's Tailwind-like styling primitives
impl Render for TopBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bat = self.battery.read(cx);

        div()
            .flex()
            .justify_between()
            .w_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .p_2()
            .child(
                div()
                    .id("battery") // `.on_click` requires a stateful (`.id(..)`) element
                    .hover(|style| style.bg(rgb(0x313244)))
                    .rounded_md()
                    .px_4()
                    .child(format!("Battery: {}%", bat.percent()))
                    .on_click(move |_, _window, _cx| {
                        println!("Battery clicked!");
                    }),
            )
    }
}
```

---

## Core System Services (`wk-services`)

All services are built with reactive event streaming for **Zero Idle CPU**:

| Service | Protocol / Source | Description |
|---|---|---|
| `Clock` | Monotonic Timer | Nanosecond-precise, zero-drift scheduler waking up exactly on second or minute boundaries. |
| `Battery` | UPower D-Bus / sysfs | Subscribes to `org.freedesktop.UPower` signal stream; falls back to `/sys/class/power_supply`. |
| `Audio` | WirePlumber / PulseAudio | Tracks sink volume, mute state, and device descriptions via `wpctl`/`pactl` event streams. |
| `Mpris` | `org.mpris.MediaPlayer2` | Manages media player state (Spotify, Firefox, mpv): title, artist, playback status, controls. |

Access any registered service inside any view with:
```rust
let clock = cx.service::<Clock>();
let audio = cx.service::<Audio>();
let mpris = cx.service::<Mpris>();
```
Views are automatically re-rendered when services notify updates.

---

## Compositor Integration (`wk-compositor`)

WidgetKit automatically detects and connects to your active Wayland compositor:

- **Hyprland**: Connects to `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock` and `.socket.sock`.
- **Sway / i3**: Connects to `$SWAYSOCK` using the standard framed `i3-ipc` protocol.
- **Niri**: Connects to `$NIRI_SOCKET` and streams JSON events.
- **Fallback**: Local development workspace simulation.

```rust
let comp = cx.service::<Compositor>();
let comp_ref = comp.read(cx);

for ws in comp_ref.workspaces() {
    println!("Workspace {}: active={}", ws.name, ws.is_active);
}
```

---

## Daemon Control CLI (`widgetctl`)

WidgetKit embeds a lightweight UNIX domain socket daemon (`wk-ipc`). You can inspect and control running widgets from scripts or keybindings using `widgetctl`:

```bash
# Check daemon connectivity
widgetctl ping

# Inspect status, uptime, active windows, and services
widgetctl status

# Toggle window visibility (e.g. bind to a hotkey in hyprland.conf)
widgetctl toggle topbar
widgetctl toggle dock

# Trigger service actions
widgetctl service audio toggle_mute
widgetctl service mpris next

# Reload daemon configuration
widgetctl reload
```

---

## Wayland & Compositor Configuration

### Hyprland (`hyprland.conf`)
Enable blur and animations for WidgetKit surfaces using the window namespace:
```ini
# Blur and rounded shadows
layerrule = blur, wk-topbar
layerrule = ignorealpha 0.2, wk-topbar
layerrule = blur, wk-dock
layerrule = blur, wk-osd

# Keybinding to toggle dock or status bar
bind = $mainMod, B, exec, widgetctl toggle topbar
```

---

## Running Examples

```bash
# Run the complete topbar
cargo run --package topbar-example

# Run the dock
cargo run --package dock-example

# Run the volume OSD
cargo run --package osd-example
```

---

## Roadmap

- [x] **V1**: Headless architecture, GPUI wgpu backend, Zero Idle CPU, LayerShell wrappers, UPower/Audio/MPRIS services, Hyprland/Sway/Niri IPC, UNIX socket daemon & CLI.
- [ ] **V2**: `widgetkit-components` (reusable popovers, complex sliders, calendar dialogs) and dynamic CSS/JSON theme variable engines.
