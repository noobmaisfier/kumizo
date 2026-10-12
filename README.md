# WidgetKit (V1)

[![Rust](https://img.shields.io/badge/rust-edition%202021-orange.svg)](https://www.rust-lang.org)
[![Wayland](https://img.shields.io/badge/wayland-layer--shell-blue.svg)](https://wayland.freedesktop.org/)
[![GPUI](https://img.shields.io/badge/render-GPUI%20(wgpu)-green.svg)](https://github.com/zed-industries/zed)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-lightgrey.svg)](LICENSE)

**WidgetKit** is a high-performance, pure-Rust SDK for building your own Wayland desktop shell (status bars, docks, OSDs, popups, panels) as **one single process**.

Unlike existing web- or interpreted-based solutions (AGS, Eww, Waybar, QuickShell), WidgetKit is powered by the **GPUI** rendering engine (Vulkan/wgpu GPU acceleration):
- **Single instance**: one process, one GPU/font stack, one socket. Every window and every widget of your shell lives in it. ~89 MB instead of ~210 MB for three separate processes.
- **You write the widgets**: a widget is a plain Rust struct implementing `gpui::Render`. Total layout freedom, no imposed UI kit.
- **Zero Idle CPU**: services are event-driven (D-Bus signals, compositor sockets, `pactl subscribe`); nothing redraws unless `cx.notify()` is called.
- **Unified Event Loop**: GPUI's native async executor drives both UI layout and background D-Bus/IPC requests.
- **Pinned upstream engine**: built on the `gpui-pre` snapshot of Zed's GPUI, pinned to an exact version (`=0.3.8`) instead of a floating git `main`.

> **Mental model.** A *widget* is a GPUI view. A *window* (`LayerWindow`) is a Wayland layer-shell surface that hosts one root view. The *shell* is your `main()`: it registers services and windows once, in one process. Windows talk to each other in-memory (`cx.toggle_window("calendar")`), never through IPC.

---

## Contributing

**Before touching this codebase, read [`docs/REFERENTIEL.md`](docs/REFERENTIEL.md).** GPUI has no stable API; this project pins an exact `gpui-pre` snapshot (`=0.3.8`) instead of tracking Zed's `main`, so upgrades are deliberate and the diff between snapshots is large. The reference documents the API traps, the mistakes not to repeat, and how to verify a change actually works.

**To create a widget, read [`docs/CREER-UN-WIDGET.md`](docs/CREER-UN-WIDGET.md).** It is self-sufficient: recipe, exact signatures, services, verification. The running reference is [`examples/shell`](examples/shell/src/main.rs).

---

## Workspace Architecture

```
widgetkit-workspace/
├── Cargo.toml                # Workspace manifest
├── crates/
│   ├── widgetkit/            # Public façade (Builder API, prelude, service registry, window control)
│   ├── wk-gpui-ext/          # LayerWindow, Anchor, Layer, Margins builder
│   ├── wk-services/          # Event-driven system services (Clock, Battery, Audio, Brightness, Network, Tray, MPRIS)
│   ├── wk-compositor/        # Native IPC sockets for Hyprland, Sway, and Niri
│   ├── wk-ipc/               # Single UNIX socket: protocol, server, client, window control
│   ├── wk-pam/               # PAM FFI for the lockscreen
│   └── wk-ctl/               # widgetctl command-line utility
└── examples/
    ├── shell/                # THE reference shell: topbar + dock + OSD + tray menu in ONE process
    └── lockscreen/           # PAM locker; deliberately a separate process with no IPC (security)
```

The lockscreen is the only intentional second binary: it handles passwords and takes exclusive keyboard focus, so it must not share a process, a socket or a service registry with the shell.

---

## The "Bring Your Own UI" Model

WidgetKit abstracts all Wayland layer-shell complexity and asynchronous D-Bus services, while leaving complete aesthetic freedom to you:

```rust
use kumizo_kit::prelude::*;
use gpui::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .window(
            LayerWindow::new("topbar")
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .height(32.0)
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
| `Brightness` | Kernel Uevents / sysfs | 100% event-driven backlight monitor via `sysfs_notify` + `NETLINK_KOBJECT_UEVENT` (zero busy-polling). |
| `Network` | NM / iwd / sysfs | Auto-detecting Wi-Fi signal, connection status, and SSID with multi-backend fallback. |
| `Tray` | StatusNotifierItem / D-Bus | Complete SNI system tray host with dbusmenu layout tree and GPU icon texture rendering. |
| `Mpris` | `org.mpris.MediaPlayer2` | Manages media player state (Spotify, Firefox, mpv): title, artist, playback status, controls. |
| `Apps` | FreeDesktop `.desktop` | Application discovery, instant fuzzy search, background launch, icon lookup via `find_icon()`, and GPU icon loading (PNG & SVG via `resvg`). |
| `Notifications` | `org.freedesktop.Notifications` | In-process D-Bus notification daemon replacing dunst/mako/swaync: toasts, action handling, history, and DND. |
| `Clipboard` | Wayland `wl-clipboard` | In-memory clipboard manager: history streaming, fuzzy search, deduplication and fast pasteback. |

Access any registered service inside any view with:
```rust
let clock = cx.service::<Clock>();
let audio = cx.service::<Audio>();
let apps = cx.service::<Apps>();
let notifs = cx.service::<Notifications>();
let clip = cx.service::<Clipboard>();
let comp = cx.service::<Compositor>();
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

// Workspaces
for ws in comp_ref.workspaces() {
    println!("Workspace {}: active={}", ws.name, ws.is_active);
}

// Open windows & focus switching
for win in comp_ref.windows() {
    println!("Window [{}]: title='{}', class='{}', ws='{}'", win.id, win.title, win.class, win.workspace_name);
}
comp_ref.focus_window(target_con_id);
```

---

## Opening and closing windows from your widgets

Popups, calendars, control centers: declare them once in `main()` (usually with `.start_hidden(true)`), then drive them **in-memory** from any click handler. No IPC, no globals to plumb:

```rust
use kumizo_kit::prelude::*; // brings in WindowControlExt

div()
    .id("clock")
    .on_click(|_, _window, cx| {
        cx.toggle_window("calendar"); // also: cx.show_window(..), cx.hide_window(..)
    })
```

Inside a `Context<V>` the same methods work (`Context` derefs to `App`).

---

## Control CLI (`widgetctl`)

WidgetKit is **single-instance**: exactly one process owns exactly one socket, `$XDG_RUNTIME_DIR/widgetkit.sock` (override with `$WIDGETKIT_SOCKET`). Launching the shell a second time exits immediately with an error instead of starting a second GPU stack; a stale socket left by a crash is detected and replaced.

Use `widgetctl` from scripts or compositor keybindings. There is no `--socket` option, there is only one target:

```bash
widgetctl ping                        # check the shell responds
widgetctl status                      # uptime, windows, services
widgetctl toggle dock                 # show/hide a window (hotkey in hyprland.conf / sway)
widgetctl service audio toggle_mute   # trigger a service action
widgetctl service mpris next
widgetctl reload                      # re-render every window
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

# Keybinding to toggle the dock
bind = $mainMod, B, exec, widgetctl toggle dock
```

---

## Ad-Hoc Data Sources (`Source<T>`)

A [`Service`](crates/wk-services/src/service.rs) is the right tool for shared, type-keyed desktop subsystems (battery, audio, mpris). For ad-hoc data needs (temperature in sysfs, streaming script stdout, or pushing messages over a socket), [`Source<T>`](crates/wk-services/src/source.rs) lets you stream events directly into a plain GPUI entity without writing a full service:

```rust
use kumizo_kit::prelude::*;

// 1. Stream stdout lines from a child process (auto-kills child on drop)
let ticker = ProcessSource::new("sh")
    .args(["-c", "while true; do date +%s; sleep 1; done"])
    .restart_after(Duration::from_secs(1))
    .lines(cx, |line| Some(line.to_string()))
    .track(cx);

// 2. Watch a file via inotify (re-read when writers finish)
let temp = Source::watch_file(cx, "/tmp/temperature", |s| s.trim().parse::<i32>().ok())
    .track(cx);

// 3. Listen on a private UNIX socket (mode 0600, unlinked on drop)
let inbox = Source::listen_unix(cx, "/tmp/my-widget.sock", |line| (!line.is_empty()).then(|| line.to_string()))
    .track(cx);
```

In your view's `render(&mut self, _, cx)`:
```rust
let val = self.temp.read(cx).value(); // Option<&i32>
div().child(format!("Temp: {val:?} C"))
```

---

## Running the Examples

```bash
# The whole shell (rail + launcher + tasks switcher + calendar + powermenu + clipboard + notifications) in a single process
cargo run --release --package shell-example

# Event-driven data sources demo (process stdout, inotify file watch, unix socket)
cargo run --package sources-example

# The PAM lockscreen (separate process on purpose, no IPC)
cargo run --release --package lockscreen-example
```

---

## Roadmap

- [x] **V1**: GPUI wgpu backend, Zero Idle CPU, LayerShell wrappers, event-driven UPower/Audio/MPRIS/Tray/Network services, Hyprland/Sway/Niri IPC, **single-instance shell** with a single control socket and CLI, in-process `cx.toggle_window(..)`.
- [x] **V2**: ergonomic helpers to build *your own* event-driven data sources (stream a process' stdout, watch a file, listen on a socket) without writing a full `Service`. No UI components: layout stays entirely yours.


