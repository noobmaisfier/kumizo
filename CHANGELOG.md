# Changelog

Notable changes to the packages published from this repository:
`kumizo-kit` (SDK), `kumizo` (CLI) and `kumizo-aurora` (reference shell).

## 0.12.0 — 2026-10-26

### Fixed

- **Hyprland works again.** Commands were terminated with a newline, which
  Hyprland never answers; they are now framed exactly like `hyprctl` (no
  newline, write side shut down, reply read to EOF, 5 s timeout).
- **Hyprland Lua dispatchers.** The service reads `j/status` (`configProvider`)
  and falls back to `j/version`; when the instance uses the Lua config provider
  it emits `hl.dsp.focus/window.close/window.float/window.fullscreen/window.move`
  and `hl.dsp.workspace.toggle_special` instead of the removed `hyprlang`
  syntax. Instances still on `hyprlang` keep working through the legacy
  dispatchers.
- **Named and special workspaces.** `j/workspaces` reports named workspaces as
  `web` and special ones as `special:magic`, with no numeric id. They now get a
  stable negative id, keep their real selector in the new `Workspace::address`,
  and `switch_workspace(id)` resolves it. A named workspace whose own name
  starts with `special:` is stored as `name:special:x`, so it is not mistaken
  for the actual scratchpad.
- **Hyprland windows, monitors and actions.** `j/clients`, `j/monitors` and
  `j/activewindow` are fetched at startup and on events; `focus_window`,
  `close_window`, floating, fullscreen, move-to-workspace, focus-monitor,
  special-workspace toggle and reload are implemented instead of silently
  doing nothing.
- **Event coverage and reconnection.** All current socket2 events are handled
  (workspace v1/v2, create/destroy/rename/move, focusedmon, activespecial,
  monitor hotplug, open/close/move window, activewindow, title, floating,
  fullscreen, pin, minimized, kill, urgent, config reload). State is replaced
  from authoritative JSON queries rather than replaying deltas, and the event
  socket reconnects with exponential backoff and a full resync.
- **Sway/Niri parity.** Sway named workspaces (`num = -1`) get stable negative
  ids and windows map to them; Sway/Niri gain window lists, focus/close/floating/
  fullscreen/move/focus-monitor/reload actions, monitor lists and reconnect
  handling. Niri uses the stable workspace `id` (not the positional `idx`) and
  handles `ConfigLoaded`/workspace events for output changes.
- **Notification hover opacity.** The shared hover colour is `raised` at 65 %
  alpha; applied to a toast card that lives on a transparent layer surface it
  made the card see-through on hover, with the message barely readable. Cards
  now use `ui::hover_bg_solid()` (opaque).

### Added

- `Workspace::address`, `Workspace::is_special`, public `Monitor`,
  `CompositorEvent::{WindowFocusCleared, WindowUrgent, MonitorsChanged}`, and
  `Compositor::{active_workspace, active_workspace_is_special, monitors,
  focused_monitor, switch_workspace_address, try_* , close_window_id,
  toggle_floating, toggle_fullscreen, move_window_to_workspace, focus_monitor,
  toggle_special_workspace, reload_config}`.
- `Compositor` accepts `kumizo service compositor <action>` IPC commands for all
  of the above.

## 0.11.1 — 2026-10-25

Documentation-only release: no API, behaviour or dependency change, drop-in
upgrade from 0.11.0.

### Changed

- `kumizo-kit` and `kumizo` now publish the repository README on crates.io
  (they previously packaged `readme = false`); licence and guide links in it
  are absolute so they resolve outside the repository.
- `kumizo-kit` crate docs: the feature table now lists `full`.
- `aurora/README.md`: install section rewritten around `cargo install`, build
  prerequisites spelled out.

### Removed

- **Binary install scheme.** `install-aurora.sh` is deleted, along with the
  GitHub/GitLab jobs that produced and published `kumizo-linux-x86_64` /
  `aurora-*-linux-x86_64` tarballs and the GitHub/GitLab releases. CI is now a
  single `check` job (workspace check + lib tests). There are no pre-built
  binaries: `cargo install --locked kumizo-aurora kumizo` (crates.io) or
  `cargo build` from the repository are the only supported install paths.
  This affects the repository, not the published crates.

## 0.11.0 — 2026-10-25

### Changed

- **Three packages instead of nine.** `wk-ui`, `wk-gpui-ext`, `wk-services`,
  `wk-compositor`, `wk-ipc` and `wk-pam` are now modules of `kumizo-kit`
  (`kumizo_kit::{ui,ext,services,compositor,ipc,pam}`), each behind a feature
  flag. Repository layout: `kumizo-kit/` (SDK), `kumizo-cli/` (package
  `kumizo`), `aurora/` (package `kumizo-aurora`).
- Features: `app` (default; implies every module), `ipc`, `ext`, `ui`,
  `services`, `compositor`, `pam`, `layer-shell` (default), `full` (alias of
  the defaults today).
- The `kumizo` CLI depends on `kumizo-kit` with `default-features = false,
  features = ["ipc"]`: no GPUI, wgpu or zbus in its build.

### Added

- `kumizo-aurora` package, shipping the `aurora` and `aurora-lockscreen`
  binaries under their original names.

### Removed

- **Binary install scheme.** `install-aurora.sh` is deleted, along with the
  GitHub/GitLab jobs that produced and published `kumizo-linux-x86_64` /
  `aurora-*-linux-x86_64` tarballs and the GitHub/GitLab releases. CI is now a
  single `check` job (workspace check + lib tests). There are no pre-built
  binaries: `cargo install --locked kumizo-aurora kumizo` (crates.io) or
  `cargo build` from the repository are the only supported install paths.

### Yanked

- `wk-ui`, `wk-gpui-ext`, `wk-services`, `wk-compositor`, `wk-ipc`, `wk-pam`
  **0.1.0** and `kumizo-kit` **0.1.0** are yanked on crates.io. Existing
  `Cargo.lock` files still build them; new projects must use `kumizo-kit`
  0.11.0 or later. Reversible with `cargo yank --undo --version 0.1.0 <crate>`.

---

- `gpui-pre` is pinned to `=0.3.9` (via `gpui-pre-platform` for the platform
  bootstrap). Any update is a documented minor event in this file.
