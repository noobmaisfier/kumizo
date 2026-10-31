# Aurora — example shell built on `kumizo-kit`

Aurora is a complete Wayland desktop shell : bar, launcher, theme picker, clipboard
manager, notifications, control center, tasks, pomodoro, OSD, lockscreen —
one process, GPU-rendered.


## Install

There are **no pre-built binaries**. Needs Linux + Wayland (`wlr-layer-shell`;
Sway, Hyprland or Niri), Rust stable, and:

Arch Linux:

```bash
sudo pacman -S --needed base-devel
```

Debian / Ubuntu:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libwayland-dev \
  libfontconfig1-dev libpam0g-dev
```

Rust stable, if you do not have it yet:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

Runtime: PipeWire (or PulseAudio), `playerctl`, and a Nerd Font
(`JetBrainsMono Nerd Font`).

Compile from crates.io (~15 min the first time for GPUI/wgpu):

```bash
# into ~/.cargo/bin
cargo install --locked kumizo-aurora kumizo
# or into ~/.local/bin
cargo install --locked --root "$HOME/.local" kumizo-aurora kumizo
```

This installs `aurora`, `aurora-lockscreen` and the `kumizo` CLI. Autostart it
from your compositor — in Sway:

```
exec ~/.local/bin/aurora
```

Check: `kumizo ping` → `PONG`.

## Copy it as a starting point

`aurora/` is a **self-contained Cargo package**: no `path` dependency, no
`workspace = true` inheritance. Copy it anywhere and build:

```bash
cp -r aurora ~/my-shell
cd ~/my-shell && cargo build --release
```

It pulls `kumizo-kit` from crates.io (inside this repo a `[patch]` swaps in
the local SDK). Nothing else from the repository is required.

## Build from the repository

```bash
cargo build --release -p kumizo-aurora -p kumizo
cp target/release/{aurora,aurora-lockscreen,kumizo} ~/.local/bin/
```

## What's inside

| File | Window / feature |
|---|---|
| `bar.rs` | Top bar: clock, MPRIS media pill, RAM/volume/battery cluster, tray |
| `launcher.rs` | Fullscreen app launcher with glide scrolling |
| `theme_picker.rs` | Fullscreen theme picker with live preview |
| `control.rs` | Control center (audio, bluetooth, network, brightness, DnD) |
| `clipboard.rs` | Clipboard history manager |
| `notifs.rs` / `toasts.rs` | Notification history + ephemeral toasts |
| `tasks.rs` | Window switcher |
| `power.rs` | Power menu |
| `pomodoro.rs` | Pomodoro timer |
| `osd.rs` | Volume/brightness OSD |
| `theme.rs` / `theme_ctl.rs` | 11 hardcoded themes, `kumizo service theme …` |
| `lockscreen.rs` | Lockscreen wiring (separate `aurora-lockscreen` binary) |

## Drive it

Media goes through **MPRIS** only: whatever player answers on the session bus
(browser, external player…). The bar pill shows the title and controls
previous / play-pause / next.

```bash
playerctl -l                 # players on the bus
playerctl -p spotify play    # drive one directly
kumizo service mpris next    # and through the shell

kumizo ping
kumizo status
kumizo toggle aurora-launcher
kumizo service theme next
kumizo service theme set "Flexoki Light"
```

## Layout

```
src/main.rs       services + windows + run()
src/ui.rs         window ids (BAR = "aurora-bar", …) + shared UI helpers
src/bar.rs        top bar: clock, MPRIS pill, system cluster
src/theme.rs      11 palettes + `pal::` semantic tokens
```
