# Aurora — example shell built on `kumizo-kit`

Aurora is a complete Wayland desktop shell and the living proof that
`kumizo-kit` scales to daily use: bar, launcher, clipboard manager,
notifications, control center, tasks, pomodoro, OSD, lockscreen — one
process, GPU-rendered.

> Aurora is an **example**, not the product. The product is the SDK
> (`kumizo-kit`, see the [repo README](../README.md)). If Aurora suffices
> for you as-is, install it below; otherwise copy it as a starting point for
> your own shell.

## Install (no code)

```bash
cargo install --locked kumizo-aurora kumizo
```

This compiles the shell locally from the crates.io sources (GPUI/wgpu, ~15 min
the first time) and installs `aurora`, `aurora-lockscreen` and the `kumizo` CLI
into `~/.cargo/bin`. To land in `~/.local/bin` instead (absolute paths used by
the Sway snippet below):

```bash
cargo install --locked --root "$HOME/.local" kumizo-aurora kumizo
```

Then add to your Sway config:

```
exec ~/.local/bin/aurora
```

Check: `kumizo ping` → `PONG`.

## Build from the repository

```bash
cargo build --release -p kumizo-aurora -p kumizo
cp target/release/aurora target/release/aurora-lockscreen target/release/kumizo ~/.local/bin/
```

Requirements: Linux + Wayland (Sway), Rust stable, `pkg-config`, Wayland/font/
audio headers (Arch: `wayland libxkbcommon fontconfig alsa-lib`; Debian:
`libwayland-dev libxkbcommon-dev libfontconfig1-dev libasound2-dev`), `libpam`
(Arch `pam`, Debian `libpam0g-dev`) for the lockscreen, PipeWire/ALSA,
`playerctl` (MPRIS pill), `JetBrainsMono Nerd Font` (icons).

> The `install-aurora.sh` script from the repository root still works as a
> fallback (pre-built release binary or git source build), but crates.io is the
> supported path since 0.11.0.

## What's inside

| File | Window / feature |
|---|---|
| `bar.rs` | Top bar (time only) + MPRIS media pill + RAM/volume/battery cluster |
| `launcher.rs` | Fullscreen app launcher with glide scrolling |
| `control.rs` | Control center (audio, bluetooth, network, brightness, DnD) |
| `clipboard.rs` | Clipboard history manager |
| `notifs.rs` / `toasts.rs` | Notification history + ephemeral toasts |
| `tasks.rs` | Window switcher |
| `power.rs` | Power menu |
| `pomodoro.rs` | Pomodoro timer |
| `osd.rs` | Volume/brightness OSD |
| `theme.rs` / `theme_ctl.rs` | 11 hardcoded themes, `kumizo service theme …` |
| `lockscreen.rs` | Lockscreen wiring (separate `aurora-lockscreen` binary) |

## Media (MPRIS)

The shell shows and drives media through **MPRIS** only — whatever player
answers on the session bus (browser, external player…). The bar's media pill
shows the title and offers previous / play-pause / next.

There is **no local music player inside the shell**: a local player is its own
program (`kumizo-music`, separate process, GPL-2.0 because of
`rockbox-playback`) that publishes itself in MPRIS — the shell then displays
it like any other player, and stays MIT OR Apache-2.0 itself.

```bash
playerctl -l                 # players on the bus
playerctl -p kumizo play     # drive one directly
kumizo service mpris next    # and through the shell
```

## Drive it without keys

```bash
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
