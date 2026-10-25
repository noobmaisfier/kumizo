# Aurora — example shell built on `kumizo-kit`

Aurora is a complete Wayland desktop shell and the living proof that
`kumizo-kit` scales to daily use: bar, launcher, clipboard manager,
notifications, control center, tasks, pomodoro, OSD, lockscreen — one
process, GPU-rendered.

> Aurora is an **example**, not the product. The product is the SDK
> (`kumizo-kit`, see the [repo README](../../README.md)). If Aurora suffices
> for you as-is, install it below; otherwise copy it as a starting point for
> your own shell.

## Install (no code)

```bash
curl -fsSL https://raw.githubusercontent.com/noobmaisfier/kumizo/main/install-aurora.sh | sh
```

Installs the latest release binary when available, otherwise builds from
source (~10 min first time). Then add to your Sway config (the installer
proposes it, with a backup):

```
exec ~/.local/bin/aurora
```

Check: `kumizo ping` → `PONG`.

## Build from source

```bash
cargo build --release -p kumizo-aurora -p kumizo
cp target/release/aurora target/release/aurora-lockscreen target/release/kumizo ~/.local/bin/
```

Requirements: Linux + Wayland (Sway), Rust stable, PipeWire/ALSA, `playerctl`
(MPRIS pill), `JetBrainsMono Nerd Font` (icons).

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
