# Kumizo

[![Rust](https://img.shields.io/badge/rust-edition%202021-orange.svg)](https://www.rust-lang.org)
[![Wayland](https://img.shields.io/badge/wayland-layer--shell-blue.svg)](https://wayland.freedesktop.org/)
[![GPUI](https://img.shields.io/badge/render-GPUI%20(wgpu)-green.svg)](https://github.com/zed-industries/zed)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-lightgrey.svg)](LICENSE-MIT)

**Kumizo** (`kumizo-kit`) is a high-performance, pure-Rust SDK for building
your own Wayland desktop shell — status bars, docks, launchers, OSDs, popups,
panels — as **one single process**, rendered with GPU acceleration (GPUI/wgpu).

Unlike web- or interpreted-based solutions (AGS, Eww, Waybar, QuickShell):

- **Single instance**: one process, one GPU/font stack, one socket. Every
  window and every widget of your shell lives in it (~110 MB instead of
  ~210 MB for three separate processes).
- **You write the widgets**: a widget is a plain Rust struct implementing
  `gpui::Render`. Total layout freedom, no imposed UI kit.
- **Zero idle CPU**: services are event-driven (D-Bus signals, compositor
  sockets); nothing redraws unless `cx.notify()` is called.

**Aurora**, the example shell in `examples/aurora`, is the living proof: it is
used daily as a real desktop shell. If it suffices for you, install it
directly — otherwise build your own on `kumizo-kit`.

## Build your own shell in 5 minutes

Requirements: Linux + Wayland (Sway), a Rust stable toolchain and `gcc`.

```bash
cargo new my-shell && cd my-shell
cargo add kumizo-kit
cargo add gpui-pre --rename gpui --version "=0.3.8"   # same snapshot the SDK uses: exactly ONE gpui copy
```

A shell is services + windows + `run()`:

```rust
use gpui::prelude::*;
use kumizo_kit::prelude::*;

fn main() {
    Kumizo::new()
        .service::<Clock>()     // one call per service, BEFORE the windows
        .window(
            LayerWindow::new("<id>")...,
            |cx: &mut gpui::App| cx.new(MyView::new),
        )
        .window(                // as many windows as needed, same process
            LayerWindow::new("<popup>")....start_hidden(true),
            |cx: &mut gpui::App| cx.new(MyPopup::new),
        )
        .run();
}
```

Browse the [`examples/shell`](https://github.com/noobmaisfier/kumizo/tree/main/examples/shell)
crate to see a complete minimal shell. Rules that keep the codebase healthy:

- **Architecture B**: services own pure data, shell views own windows.
- `wk-ui` stays headless and hand-written; mouse caret positioning only on
  long-text components.
- Read [`docs/GUIDE.md`](https://github.com/noobmaisfier/kumizo/blob/main/docs/GUIDE.md) before
  adding a widget — it documents the builder order, services and lifecycle.

## Try Aurora (no code)

Aurora is a complete shell: bar, launcher, music player, clipboard manager,
notifications, lockscreen.

```bash
curl -fsSL https://raw.githubusercontent.com/noobmaisfier/kumizo/main/install-aurora.sh | sh
```

This installs the latest release binary when available, otherwise it builds
from source (~10 min first time). Flags: `--bin` (binary only), `--source`
(source only), `--to DIR`, `--check` (prerequisites only). The installer
never touches your sway config — it prints the 2 lines to add, unless you
pass `--sway`.

Add to your Sway config (the installer proposes it, with a backup):

```
exec ~/.local/bin/aurora
bindsym $mod+m exec ~/.local/bin/kumizo toggle aurora-music
```

Then `kumizo ping` should answer `PONG`. The music player reads `~/Music`
(or `$AURORA_MUSIC_DIR`).

> **License note**: the prebuilt `aurora` binary links
> `rockbox-playback` (GPL-2.0), so the binary is GPL-2.0-licensed. No
> constraint for personal use; the source stays public here. The SDK crates
> themselves are MIT OR Apache-2.0 and link nothing GPL.

## The crates

| Crate | Role |
|---|---|
| `kumizo-kit` | SDK facade: `Kumizo` builder, prelude, app wiring |
| `wk-gpui-ext` | GPUI Wayland layer-shell extension traits and wrappers |
| `wk-services` | Async system services (Clock, Battery, Audio, MPRIS…) |
| `wk-compositor` | Compositor IPC (Hyprland, Sway, Niri) |
| `wk-ipc` | UNIX-socket daemon protocol + server |
| `wk-ui` | Shared headless UI primitives |
| `wk-pam` | Minimal Linux-PAM check, no bindgen/libclang |
| `wk-ctl` | `kumizo` CLI to drive a running shell |

`kumizo` talks to the running shell over its socket, no synthetic input
involved:

```bash
kumizo ping                            # PONG
kumizo toggle aurora-music             # show/hide the music panel
kumizo service music toggle|next|prev  # key-free control, scriptable
```

## Repository layout

```
crates/          the SDK (publishable to crates.io)
examples/aurora  the living example shell (daily driver)
examples/shell   minimal shell example — start here to learn
docs/            the developer guide (concepts, recipe, reference, pitfalls)
install-aurora.sh  one-line installer for aurora
.github/workflows/ci.yml  check → builds → release on every v* tag
```

Branches: `main` (stable) and `develop` (work in progress). CI runs on
`main` only, plus a release job per `v*` tag.

## Contributing

Keep `main` green: `cargo check --workspace` and
`cargo test --workspace --lib` must pass. Confirm scope before large builds,
verify behavior on a real session, and never commit generated binaries
(`dist/` stays untracked except the installer scripts).

## License

SDK crates: MIT OR Apache-2.0 (see `LICENSE-MIT`, `LICENSE-APACHE`).
Aurora binary: GPL-2.0 (via `rockbox-playback`), see note above.
