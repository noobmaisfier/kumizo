# Aurora — example shell built on `kumizo-kit`

Aurora is a complete Wayland desktop shell and the living proof that
`kumizo-kit` scales to daily use: bar, launcher, music player, clipboard
manager, notifications, control center, tasks, pomodoro, OSD, lockscreen —
one process, GPU-rendered.

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
bindsym $mod+m exec ~/.local/bin/kumizo toggle aurora-music
```

Check: `kumizo ping` → `PONG`.

## Build from source

```bash
cargo build --release -p aurora -p wk-ctl
cp target/release/aurora target/release/aurora-lockscreen target/release/kumizo ~/.local/bin/
```

Requirements: Linux + Wayland (Sway), Rust stable, `gcc` (rockbox compiles
its C codecs), PipeWire/ALSA, `playerctl` (web pill), `JetBrainsMono Nerd
Font` (icons).

## What's inside

| File | Window / feature |
|---|---|
| `bar.rs` | Top bar + unified media pill (local if playing, else MPRIS, else icon) |
| `launcher.rs` | Fullscreen app launcher with glide scrolling |
| `music/` | Local music player (see below) |
| `control.rs` | Control center (audio, bluetooth, network, brightness, DnD) |
| `clipboard.rs` | Clipboard history manager |
| `notifs.rs` / `toasts.rs` | Notification history + ephemeral toasts |
| `tasks.rs` | Window switcher |
| `power.rs` | Power menu |
| `pomodoro.rs` | Pomodoro timer |
| `osd.rs` | Volume/brightness OSD |
| `theme.rs` / `theme_ctl.rs` | 11 hardcoded themes, `kumizo service theme …` |
| `lockscreen.rs` | Lockscreen wiring (separate `aurora-lockscreen` binary) |

## Music player

Local library player (`~/Music` by default; override with `$AURORA_MUSIC_DIR`
or `~/.config/aurora/music_dir`). Engine: `rockbox-playback` (GPL-2.0 —
hence the prebuilt binary is GPL-licensed; the SDK itself is MIT/Apache).
Supported: opus, mp3, m4a/3GP, and whatever rockbox decodes.

```bash
kumizo toggle aurora-music        # show/hide the panel
kumizo service music toggle       # play/pause
kumizo service music play | pause | next | prev | stop
kumizo service music seek 60      # seconds
kumizo service music volume 15    # 0-100
```

Tabs: queue (glide list, keyboard follows the view), lyrics (`.lrc`/`.txt`/
tags), big 300 px cover anchor, seek bar, volume.

## Drive it without keys

```bash
kumizo ping
kumizo status
kumizo toggle aurora-music
kumizo service theme next
kumizo service theme set "Flexoki Light"
```

## Layout

```
src/main.rs       services + windows + run()
src/ui.rs         window ids (MUSIC = "aurora-music", …)
src/music/        model.rs (state + IPC) / view.rs (panel) /
                  engine.rs (rockbox worker) / disc.rs (cover, seek,
                  controls) / library.rs (scan) / lyrics.rs
tests/rockbox_probe.rs   sequential 3-format playback probe
```
