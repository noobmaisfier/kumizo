# Kumizo Guide — concepts, widget recipe, windows & services, GPUI pitfalls

> **MUST** = hard constraint: violation fails to compile, panics, or silently
> shows nothing. **SHOULD** = a starting point that worked once on one
> machine: adapt numbers, names, palettes to your hardware and taste.
> NEVER treat pixel values, window ids or hex colors below as defaults —
> they are examples. When this guide and the code disagree, **the code wins**.

Contents:

1. [Core concepts](#1-core-concepts)
2. [Widget recipe, step by step](#2-widget-recipe-step-by-step)
3. [Windows & services reference](#3-windows--services-reference)
4. [Appendix: GPUI pitfalls & method rules](#4-appendix-gpui-pitfalls--method-rules)

---

## 1. Core concepts

Three things combine in every shell:

1. **Window** — a `LayerWindow` surface (layer-shell), declared once in
   `main()` via the `Kumizo` builder: position, size, layer, margins.
2. **View** — a plain Rust struct implementing `gpui::Render`. Its `render`
   method rebuilds the element tree every frame. Treat it as a pure function
   of state: NEVER a syscall, D-Bus call or file read inside `render`. All
   data comes from struct fields or from a service read via
   `entity.read(cx)`.
3. **Service** (optional but the general case) — a reactive data source
   (`Clock`, `Battery`, `Memory`, `Audio`, `Brightness`, `Mpris`, `Network`,
   `Tray`, `Compositor`, or a custom one). The only way to redraw is `cx.notify()`:
   called directly in the view, in a closure passed to `update`, or emitted
   by an observed service.

Architecture rules:

- **Single process**: one process (one GPU init, one font stack, one socket)
  hosts every window and widget. NEVER one binary per widget. A widget is a
  reusable `Render` view; a window hosts *one* root view (which may contain
  any number of widgets); a shell is a `main()` declaring services and
  windows once. Measured: ~110 MB for a whole shell in one process vs ~210 MB
  PSS for three separate processes.
- **Zero idle CPU**: everything is event-driven (kernel uevents, D-Bus
  signals, compositor sockets). Exception: a clock bar wakes at 1 Hz (~0.4 %
  CPU) — never claim strict 0.0 % with one present.
- **Lockscreen stays a separate binary** (`examples/lockscreen`):
  passwords and exclusive keyboard MUST NEVER share memory, socket or service
  registry with widgets. (Limit: a layer-shell surface is not
  `ext-session-lock`; strict locking would need that backend, which does not
  exist here.)
- Pinned engine: `gpui-pre =0.3.8` (crates.io snapshot of upstream Zed
  `gpui`, same code, Apache-2.0). Exactly ONE `gpui-pre` version in the whole
  graph, or Cargo compiles two GPUI copies and their types stop being
  interchangeable. NEVER run `cargo update -p gpui-pre -p gpui-pre-platform`
  outside a deliberate bump (workspace `Cargo.toml` + `cargo check
  --workspace --all-targets`).

## 2. Widget recipe, step by step

### Step 1 — Decide where the widget lives

- Widget inside an existing window (e.g. a pill in the bar): write the view
  and add it as `.child(...)` of that window's root view. No new window.
- New surface (popup, panel, OSD): write a root view and declare a new
  window in the shell's `main()`.
- Reference shell: `examples/shell/src/main.rs` (`examples/shell/Cargo.toml`
  lists the dependencies). Only extra dependency a view may need is
  `async-io = { workspace = true }` for timers. Reuse workspace dependencies
  (`gpui`, `gpui_platform`, `zbus`, `async-io`, `serde`, `image` (workspace
  default: PNG-only; `examples/shell` additionally enables `jpeg`, `webp`),
  …); NEVER add one without reason.

### Step 2 — `main()`: services, windows, `run()`

```rust
use gpui::prelude::*;
use kumizo_kit::prelude::*;

fn main() {
    Kumizo::new()
        .service::<Clock>()     // one call per consumed service, BEFORE the windows
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

Builder order (`crates/kumizo-kit/src/app.rs`): `Kumizo::new()` →
`.service::<S>()` per consumed service (`S: Service`, any order, or
`cx.service::<S>()` panics at startup with an explicit message) →
`.window(config, builder)` (named `config.id`; builder returns `Entity<V>`,
`V: Render`) → `.ipc(true/false)` (IPC server on by default; off only for
the lockscreen) → `.run()` (never returns: inits services, binds the single
socket `$XDG_RUNTIME_DIR/kumizo.sock` (`$KUMIZO_SOCKET` overrides it,
e.g. for tests), opens windows once the screen is announced; a second launch
exits if an instance already answers).

### Step 3 — Window archetypes (`LayerWindow`)

Start from `LayerWindow::new("<id>")` (neutral: empty anchors, exclusive 0,
dimensions `None`; every setter exists, see
`crates/wk-gpui-ext/src/layer_window.rs`):

```rust
LayerWindow::new("unique_window_id")
    .layer(Layer::Top)                         // Background | Bottom | Top | Overlay
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT) // bitflags; TOP_BAR = TOP|LEFT|RIGHT
    .exclusive_zone(32)                        // pixels reserved from tiled windows
    .size(Some(800.0), Some(32.0))             // width, height in logical pixels
    .margins(Margins::new(8, 8, 0, 8))         // top, right, bottom, left
    .namespace("my-shell-bar")                 // compositor layer-rule selector; keep wk- / shell prefix
    .keyboard_interactivity(KeyboardInteractivity::None) // None | OnDemand | Exclusive
    .transparent(true)
    .start_hidden(true)                        // no surface until first shown
    // .monitor(MonitorTarget::Primary)        // default Primary
```

Rules:

- Opposite anchors stretch: `LEFT|RIGHT` = full width, `TOP|BOTTOM` = full
  height, `FULLSCREEN` = whole monitor. Sizing priority: explicit
  width/height first, then opposite-anchor stretch once the output is known,
  then fallback (800 px wide, 32 px tall).
- The compositor (Sway) honors anchors for **position only** — it never
  stretches your surface. NEVER put margins into `origin` (double counting).
- `exclusive_zone` = panel size for bars/rails, `0` for popups/OSD.
- `start_hidden(true)` for popups only: a normally-hidden window keeps its
  surface and eats clicks in its zone.
- Archetypes: top bar (`Top`, `TOP_BAR`, exclusive=height, `.height(h)`);
  bottom dock (`Top`, `BOTTOM`, explicit `.size(w,h)`, bottom margin);
  fullscreen modal (`Overlay`, `FULLSCREEN`, `Exclusive`, no explicit size —
  `FULLSCREEN` stretches over the whole output, `start_hidden(true)`);
  popup/OSD
  (`Overlay`, chosen corner, exclusive 0, explicit size, `start_hidden`).
- Window ids are global on the single socket: prefix per shell
  (`aurora-launcher`, …). Check `kumizo status` to see which shell answers
  before binding hotkeys.

### Step 4 — The view: struct + `new` + `Render`

```rust
struct MyView {
    clock: Entity<Clock>,   // one field per consumed service
    visible: bool,           // optional local state
}

impl MyView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),  // panics if not registered (Step 2)
            visible: false,
        }
    }
}

impl Render for MyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);  // subscribes the view: each notify() redraws
        div().child(clock.time_hms())
    }
}
```

- `entity.read(cx)` gives `&S`. To write:
  `entity.update(cx, |s, cx| { s.set_x(); cx.notify(); })` — `update` does
  NOT notify by itself. Borrowed returns (`&str`, `&[...]`) MUST NOT escape
  `render` (E0521): copy to `String`/`Vec` or format inline.
- Fine-grained reactions: `cx.observe(&entity, |view, svc, cx| { ... })` in
  `new`, ending with `.detach()`.
- Timers: `cx.spawn(async move |this, cx| { async_io::Timer::after(...).await;
  let _ = this.update(cx, |v, cx| { ...; cx.notify(); }); }).detach();`
  (needs `async-io` in the shell crate).
- `render` returns a single type: two branches building different `div()`s
  (with/without `.id()`) do not compile together — unify branches.
- Clone each `Entity` into `move` closures separately (cloning is free).

### Step 5 — Interact: clicks, hover, state

- Left click: `.id("unique").cursor_pointer().on_click(|_, _, cx| {...})`.
  `.id()` is REQUIRED — without it `on_click` does not exist (`E0599`).
  Right click: `.on_mouse_down(gpui::MouseButton::Right, ...)`.
- Hover: `.hover(|style| style.bg(...))`. NEVER call `.hover()` twice on the
  same element — `debug_assert` fires in debug builds (`hover style already
  set`), silently overwritten in release; keep shared tile builders
  hover-free, set hover at the callsite.
- Text fields: GPUI ships none — use headless `wk-ui` (`Entity<TextBuffer>`,
  `TextBuffer::masked()` for passwords, route keys via `b.handle_key(ev)`
  matching `TextAction::{Submit, Cancel, Changed, Ignored}`; `Changed` MUST
  `cx.notify()`; draw with `text_input(...)` / `text_area(...)` and your own
  `TextStyle`). NEVER use `gpui-component` `Input`/`Textarea` (they impose a
  full theme). Caret is a **char** index; Enter submits (single-line) or
  inserts `\n`; Tab is never consumed (`Ignored`); Escape returns `Cancel`
  (clear + close it yourself) so parent shortcuts keep working.
- Sizes: `px(13.0)`, `w(px(...))`, `gap_2()`, `rounded_md()`/`rounded_lg()`.

### Step 6 — Text: bound it, never slice it

- Bound every text by layout: `.max_w(px(...)).truncate()` on the text
  `div` (GPUI truncates char-accurately with `…`). Unbounded text pushes the
  rest of the bar off-screen. Inside flex, add `.min_w_0()` to allow
  shrinking.
- NEVER slice a `String` by byte index (`&s[..27]`): it cuts mid-UTF-8-char
  and panics. Iterate `.chars()` if you truly need a char limit.
- Progress bars: container `w_full().h(px(8.)).rounded_full()` + fill
  `.w(gpui::DefiniteLength::Fraction(ratio.clamp(0.0, 1.0)))`.

### Step 7 — Bitmap images (app icons, covers, pixmaps)

GPUI expects bytes in **B,G,R,A** order. Paint via `gpui::img(source)`:

- From RGBA memory: swap R↔B per 4-byte chunk, wrap in `image::Frame`, then
  `Arc::new(RenderImage::new(smallvec![frame]))`, show with
  `gpui::img(arc.clone())` sized via `.w()`/`.h()`.
- PNG: `image::open(path)` → `to_rgba8()` → swap to BGRA.
- SVG (FreeDesktop icons): parse and rasterize on the fly (`resvg` →
  `tiny_skia::Pixmap`), convert to BGRA, pack into `RenderImage`, cache in
  memory for zero-cost redraws. From ARGB (dbusmenu, tray): reverse each
  4-byte word.
- Icon glyphs (Nerd Font present: JetBrainsMono): use ONLY verified
  codepoints — render once with `pango-view` in the local font, then copy the
  hex (`U+Fxxxx`) into source. NEVER guess a codepoint; in doubt use text or
  basic shapes.

### Step 8 — Read services, write a custom one, or use `Source<T>`

Register ONLY services your views consume (each opens real system
connections). Built-ins (`crates/wk-services/src/`, plus `Compositor` in
`crates/wk-compositor/src/`; all re-exported by the prelude):
`Clock` (`time_hms()`, `set_resolution()` `[&mut]`),
`Battery` (`percent()`, `state()`, `is_charging()`), `Memory` (`percent()`,
`used()`, `used_gib()`, `total_gib()`), `Audio` (`volume()`,
`toggle_mute()` `[&mut]`, IPC `toggle_mute/volume_set/…`),
`Brightness` (`percent()`, `set_percent()` `[&mut]`), `Mpris` (`title()`,
`display_label()` = `"artist - title"`, `play_pause()`), `Network`
(`ssid()`, `signal_percent()` → `Option`, backends NM/iwd/fallback,
`connect_with_password()` — PSK never stored, no password over CLI),
`Compositor` (Sway/Hyprland/Niri: `workspaces()`, `switch_workspace()`
`[&mut]`, `focus_window()`), `Apps` (`search()` fuzzy, `launch()`),
`Notifications` (in-process server replacing dunst/mako: `toasts()`,
`toggle_dnd()` `[&mut]`), `Tray` (`items()`, `open_menu()` `[&mut]`),
`Clipboard` (`items()`, `copy_item()` `[&mut]`).

Call rule (receiver decides): `&self` methods run on the `read(cx)` guard;
`&mut self` methods (marked `[&mut]`) MUST go through `entity.update(...)`.
Borrowed returns (`&str`, `&[...]`) MUST NOT escape `render` (E0521).

```rust
// Clock — exact-second (or minute) wakeups, zero drift
time_hms(), time_hm(), date_formatted(), to_rfc3339(), hour(), minute(),
second(), datetime(),
[&mut] set_resolution(ClockResolution::Seconds | Minutes)

// Battery (UPower D-Bus + sysfs)
percent(), state() -> BatteryState, is_charging(), energy_rate_watts(),
time_to_empty(), time_to_full(), read_sysfs()
// BatteryState::{Charging, Discharging, Empty, FullyCharged, PendingCharge,
//   PendingDischarge, Unknown} + as_str(), icon(percent)

// Memory (/proc/meminfo, relu toutes les 2 s, notify seulement si ça change)
percent(), used(), available(), total(), used_gib(), total_gib(),
read_meminfo() -> Option<(total, available)>

// Audio (PipeWire wpctl, PulseAudio pactl fallback)
volume(), is_muted(), sink_name() -> &str, icon(),
[&mut] set_volume(pct), increase_volume(step), decrease_volume(step),
toggle_mute()
// query_state() is an associated fn: Audio::query_state(), no update() needed
// IPC: toggle_mute, mute, unmute, volume_up, volume_down, volume_set

// Brightness (kernel uevents on actual_brightness + netlink, zero idle poll)
percent(), icon(), device(),
[&mut] set_percent(pct), increase(), decrease()   // ±5 steps, no args
// IPC: up, down, set <pct>; KUMIZO_BRIGHTNESS_POLL_MS fallback (2 s poll)

// Mpris (Spotify, Firefox, mpv…)
title(), artist(), album(), player_name() -> Option<&str>,
display_label() // "artist - title"
status() -> PlaybackStatus, play_pause(), next(), previous(), stop()
// PlaybackStatus::{Playing, Paused, Stopped} + icon(), is_playing()
// IPC: play_pause (alias play-pause), next, previous (alias prev), stop

// Network (NetworkManager, iwd, /proc/net/wireless fallback, auto-detected)
backend() -> WifiBackend, ssid() -> Option<&str>,
signal_percent() -> Option<u8>, // None = no signal, NEVER unwrap blindly
display_label(), icon(), has_radio(), device() -> Option<&str>,
networks() -> &[WifiNetwork], wifi_enabled(), request_scan(),
[&mut] set_enabled(on),   // the ONLY &mut one below; the rest are &self
connect(ssid), connect_with_password(ssid, password), disconnect()
// WifiBackend::{NetworkManager, Iwd, Fallback}; passwords NEVER stored,
// NEVER over CLI. IPC: scan, on, off, toggle, connect <ssid>, disconnect

// Bluetooth (BlueZ, all &self: callable on the read() guard)
is_available(), is_powered(), is_discovering(), devices() -> &[BluetoothDevice],
connected_devices(), paired_devices(), connected_count(), icon(),
toggle_power(), toggle_scan(), toggle_device_connection(path: String, connected: bool)
// IPC: toggle (alias toggle_power), scan (alias toggle_scan)

// Compositor (Sway / Hyprland / Niri + fallback, auto-detected)
kind(), workspaces() -> &[Workspace], active_workspace_id(),
active_window() -> Option<&ActiveWindow>, windows() -> &[AppWindow],
focus_window(con_id: i64), [&mut] switch_workspace(id), close_window()

// Apps (FreeDesktop .desktop catalog, PNG+SVG icons decoded to GPU)
apps() -> &[AppInfo], search(query) -> Vec<AppInfo> (fuzzy, realtime),
find_icon(class_or_app_id), launch(app)

// Notifications (in-process org.freedesktop.Notifications server)
toasts(), history() (newest-first), unread_count(), is_dnd(),
[&mut] toggle_dnd(), dismiss_toast(id) (keeps history),
[&mut] remove_history(id), clear_history(),
invoke_action(id, key)   // &self
// IPC: toggle_dnd, clear

// Tray (StatusNotifierItem + dbusmenu host; Passive items excluded)
items() -> Vec<TrayItem>, menu() -> Option<OpenMenu>, activate(id),
[&mut] open_menu(id), close_menu(), click_menu_item(id)
// The shell view observes menu() and opens/closes its own window itself.
// IPC: list, activate, menu-open, menu-click, menu-close

// Clipboard (native Wayland via wl-clipboard, deduped history)
items(), search(query), copy_text(text),
[&mut] copy_item(id), remove_item(id), clear()
// IPC: clear, copy <text>
```

Custom service (`crates/wk-services/src/<name>.rs`): simple state struct
(`Clone + Default` if possible), public immutable getters, `impl Service`
with `init` (build initial state without blocking, then
`cx.spawn(...).detach()` a background loop) and `ipc_action` (return `true`
if recognized → `kumizo service <name> <action> [payload]`, `false`
otherwise — NEVER silent success). Background task updates via
`entity.update(cx, |s, cx| { ...; cx.notify(); })` ONLY when the value
changed. Prefer events (D-Bus streams, `pactl subscribe`) over polling; if
polling, justify the cadence in a comment. Register the module in
`crates/wk-services/src/lib.rs` AND in `crates/kumizo-kit/src/prelude.rs`.

Ad-hoc value without a full `Service`: `Source<T>` from the prelude, held in
an `Entity<Source<T>>`:

```rust
// 1. Process: each stdout line becomes a T (child auto-killed via PDEATHSIG)
let ticker = ProcessSource::new("sh")
    .args(["-c", "while true; do date +%s; sleep 1; done"])
    .restart_after(Duration::from_secs(1))
    .lines(cx, |line| Some(line.to_string()))
    .track(cx); // re-notifies the parent view on each value

// 2. Regular file: re-read when a writer closes/renames it (NOT /proc or /sys:
//    they never raise inotify events)
let temp = Source::watch_file(cx, "/tmp/temperature", |text| text.trim().parse::<i32>().ok())
    .track(cx);

// 3. Kernel file without inotify (/proc, /sys): add an explicit poll interval
let cpu = FileSource::new("/proc/loadavg")
    .poll_every(Duration::from_secs(2))
    .watch(cx, |text| text.split_whitespace().next().map(str::to_string))
    .track(cx);

// 4. UNIX socket: each line sent by a client becomes a T
let inbox = Source::listen_unix(cx, "/tmp/my-widget.sock", |line| (!line.is_empty()).then(|| line.to_string()))
    .track(cx);
```

Read in `render` via `entity.read(cx).value()` (`Option<&T>`); `.error()`,
`.state()` and `.updates()` available. Cleanup is automatic: on drop, the
child gets `SIGTERM` and the socket file is removed.

### Step 9 — Show, hide, control windows

- From a widget (general case): direct in-memory calls, no IPC, via
  `WindowControlExt` (prelude): `cx.toggle_window("calendar")`,
  `cx.show_window(..)` / `cx.hide_window(..)`. Never blocks (dedicated
  thread + 5 s timeout); unknown names log a `warn!`. `hide` destroys the
  surface but keeps state; `show` recreates it.
- Service-triggered popups (e.g. tray menu): services don't own windows — the
  `WindowControl` handle may be shared with a service for exactly this case,
  but the pattern stays: the view observes the service state and opens/closes
  its own window. The service MUST `cx.notify()` after every state change.
- From scripts/compositor hotkeys: `kumizo ping|status|show|hide|toggle
  <window>|reload`, plus `kumizo service <name> <action> [payload]`.
  Single socket (`$KUMIZO_SOCKET` overrides, e.g. for tests/nested
  sessions), no `--socket` option.
- OSD pattern (ephemeral content in a permanent window): window always open,
  `visible` field, empty `render` when hidden, auto-hide via `Timer` with a
  **generation token** (increment `generation` with `wrapping_add(1)`, hide
  only if it still matches — NEVER an uncancellable timer, or the first
  timer hides the OSD mid-adjustment). Reserved for small surfaces: an open
  surface captures clicks over its whole zone even when transparent.
- Panel scrolling: constrain to the viewport every `render`
  (`panel_max_h = window.viewport_size().height - px(82.)`, `.max_h(...)`
  NEVER fixed), header + scroll div with `.id()` + `.flex_1()` +
  `.min_h_0()` + `.overflow_y_scroll()` — all three required. Verify with a
  screenshot at the SMALLEST supported height and LONGEST content.

### Step 10 — Verify (nothing is "done" before)

1. `cargo check --workspace --all-targets`: 0 errors, 0 warnings
   (`--offline` if the network is uncertain).
2. Concerned `cargo test` (`-p wk-services` for pure logic: parsing,
   conversions, thresholds — pure functions test without hardware).
3. `cargo build --workspace --release`.
4. Restart the shell (single process): kill the old one (`pkill -x <bin>`
   or exact PID — NEVER `pkill -f` with a pattern matching your own command
   line), no need to delete the socket (dead instances are detected and
   replaced; a live one makes the new launch quit — check `kumizo ping`
   first). Relaunch detached (`setsid … &` + `disown`), wait ~12 s
   (GPU + `wl_output`).
5. Screenshot (`grim out.png`) and LOOK: the widget must be visibly there
   before any measurement. Fullscreen/locked/empty captures prove nothing —
   retake, don't interpret.
6. Measure in pixels, not by eye: diff two states (before/after
   `hide`/`show`), highlight position, remaining margin. Pure-PNG script
   (zlib+struct) is enough.
7. Logs: the daemon writes to stderr (`tracing`, `RUST_LOG=debug` for
   detail). If behavior is dubious, read the log before concluding —
   services announce backends and followed items there.
8. Error paths: unknown window, unknown action, dead socket, missing bus,
   disappearing item — test every fallback, not just the nominal path.

## 3. Windows & services reference

### Minimal wiring skeleton (copy this)

```rust
use gpui::prelude::*;
use kumizo_kit::prelude::*;

fn main() {
    Kumizo::new()
        .service::<Clock>()
        .service::<Battery>()
        .window(
            LayerWindow::new("my_bar")
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .exclusive_zone(36)
                .namespace("my-shell-bar"),
            |cx: &mut gpui::App| cx.new(MyBar::new),
        )
        .window(
            LayerWindow::new("my_popup")
                .layer(Layer::Overlay)
                .anchor(Anchor::TOP | Anchor::RIGHT)
                .exclusive_zone(0)
                .size(Some(320.0), Some(400.0))
                .keyboard_interactivity(KeyboardInteractivity::OnDemand)
                .start_hidden(true)
                .namespace("my-shell-popup"),
            |cx: &mut gpui::App| cx.new(MyPopup::new),
        )
        .run();
}

struct MyBar {
    clock: Entity<Clock>,
}

impl MyBar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { clock: cx.service::<Clock>() }
    }
}

impl Render for MyBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("open-popup") // MUST: .id() before any .on_click
            .child(self.clock.read(cx).time_hm())
            .on_click(|_, _, cx| cx.toggle_window("my_popup"))
    }
}
```

### Key types (exact fields)

```rust
pub struct Workspace { pub id: i32, pub name: String, pub is_active: bool,
    pub is_urgent: bool, pub window_count: usize, pub monitor: String }
pub struct AppWindow { pub id: i64, pub title: String, pub class: String,
    pub workspace_id: i32, pub workspace_name: String, pub is_focused: bool }
pub struct ActiveWindow { pub title: String, pub class: String, pub workspace_id: i32 }

pub enum Urgency { Low, Normal, Critical }
pub struct NotificationAction { pub key: String, pub label: String }
pub struct Notification { pub id: u32, pub app_name: String, pub summary: String,
    pub body: String, pub app_icon: Option<Arc<RenderImage>>, pub urgency: Urgency,
    pub actions: Vec<NotificationAction>, pub created_at: time::OffsetDateTime,
    pub generation: u64 }

pub struct BluetoothDevice { pub path: String, pub address: String, pub name: String,
    pub icon: Option<String>, pub paired: bool, pub connected: bool,
    pub trusted: bool, pub battery_percent: Option<u8> }
pub struct AppInfo { pub id: String, pub name: String, pub comment: String,
    pub exec: String, pub icon_name: String, pub icon: Option<Arc<RenderImage>>,
    pub categories: Vec<String>, pub keywords: Vec<String> }
pub struct TrayItem { pub id: String, pub title: String,
    pub icon: Option<Arc<RenderImage>>, pub attention: bool }
pub struct TrayMenuItem { pub id: i32, pub label: String, pub enabled: bool,
    pub separator: bool, pub toggle: Option<bool>, pub children: Vec<TrayMenuItem> }
pub struct OpenMenu { pub owner: String, pub title: String, pub items: Vec<TrayMenuItem> }
pub enum TrayStatus { Active, Passive, NeedsAttention }
pub enum PlaybackStatus { Playing, Paused, Stopped }
pub enum BatteryState { Charging, Discharging, Empty, FullyCharged,
    PendingCharge, PendingDischarge, Unknown }
pub enum WifiBackend { NetworkManager, Iwd, Fallback }
pub struct WifiNetwork { pub ssid: String, pub strength: u8, pub secured: bool, pub active: bool }
pub enum ClockResolution { Seconds, Minutes }
pub struct ClipboardItem { pub id: u64, pub text: String, pub timestamp: time::OffsetDateTime }
```

> `AppWindow` is the only type above missing from the flat prelude — import
> it as `kumizo_kit::compositor::types::AppWindow`.

`Notifications` semantics: `toasts()` = ephemeral on-screen, `history()` =
newest-first, `dismiss_toast(id)` keeps history, `remove_history(id)` removes
from screen AND history.

### Theme system (optional)

Skip it for a fixed look — hardcoded colors are fine. Only when themes must
switch at runtime without recompiling: dedicated `mod pal` with semantic
roles (`bg()`, `surface()`, `text()`, `accent()`, `border()`), full repaint
via `cx.refresh_windows()` on `&mut App`, theme definitions stored on disk
with an embedded fallback (aurora keeps it simple: the active theme name in
`~/.config/aurora/theme`, 11 themes hardcoded in `theme.rs` — one file per
theme under `~/.config/<shell>/themes/` when definitions must be editable),
IPC switching through a small `Service` (`set <name|index>`, `next`, `prev`;
see `examples/aurora/src/theme_ctl.rs`, 59 lines).

### Screens, scale, accessibility

- Per window decide: span (`LEFT|RIGHT` stretch), mirror (one surface per
  monitor via `MonitorTarget`), or primary-only. Test with a second output
  plugged AND unplugged — unplugging MUST NOT strand hidden windows.
- Verify HiDPI logical pixels at scale 1 AND 2; derive borders/gaps/fonts
  from one scale factor.
- Every pointer action gets a key path (`kumizo` command + compositor
  binding); focus order keyboard-reachable alone; NEVER encode state by color
  only (pair with text/icon).

## 4. Appendix: GPUI pitfalls & method rules

Pinned snapshot: `gpui-pre =0.3.8`. NEVER guess a GPUI API from memory —
verify in local sources (`~/.cargo/registry/src/*/gpui-pre-*/`), not docs.rs
(which lies: e.g. `Proxy::new` shows `Connection` instead of `&Connection`).

**Compile traps (symptom → cause → fix):**

- `primary_display()` returns `None` unconditionally on Wayland (hardcoded in
  `gpui-pre-linux`). Use `cx.displays()`, and wait for `wl_output` globals
  (async, up to ~100×20 ms) before opening windows — `LayerWindow` already
  handles this; raw `WindowOptions` builders need an explicit wait loop.
- `Application::new()` no longer exists → `gpui_platform::application()
  .with_quit_mode(gpui::QuitMode::Explicit).run(|cx| …)`.
- `QuitMode::Default` kills the daemon when the last window closes (it is
  `true` on Linux) → always force `Explicit` for a shell.
- `Context<'_, V>` needs the explicit lifetime (E0726 otherwise).
- `cx.spawn` arity depends on the receiver: on `Context` two args
  (`async move |this, cx|`), on `App`/`AsyncApp` one arg (`async move |cx|`);
  it MUST be an async closure, NOT a closure returning an async block
  (`|this, cx| async move {…}` fails with misleading E0282). Inside,
  `this.update(cx, …)` — `cx` is already `&mut AsyncApp`. On generic
  `Source<T>`, annotate (`this: WeakEntity<Source<T>>, cx: &mut AsyncApp`).
  `WeakEntity::update` returns `Result` — handle with `let _ =` / `.ok()`.
- `on_click` takes 3 args (`Fn(&ClickEvent, &mut Window, &mut App)`) and
  exists only on elements with `.id()`; use the 3rd arg for
  `entity.update(cx, …)`.
- No `.scale()` on styles; `.hover()` takes `StyleRefinement` by value and
  MUST be called once per element (`debug_assert` in debug builds, silent
  overwrite in release).
- Raw `WindowOptions` has ~17 fields, not 10 (`WindowBackgroundAppearance`,
  `WindowKind::LayerShell(…)`); `..Default::default()` is mandatory.
- `Window` is NOT an `AppContext`: in `render`, use `cx` (which derefs to
  `&App`) for `entity.read/update` — NEVER `window`.
- `AnyWindowHandle` survives `remove_window()`: a handle is NOT proof the
  window exists — verify with `cx.windows().contains(&handle)`.
- Every used dependency MUST be declared in the employing crate's
  `Cargo.toml` — workspace membership does not propagate them.
- Single instance: if the socket file exists, send `Ping` first — an answer
  means live (second launch exits "already running"); silence means stale
  (replace). NEVER delete a live shell's socket by hand.

**Non-GPUI traps:**

- IPC borrow conflict: `BufReader::new(&sock)` holds an immutable borrow
  across the read loop → `sock.write_all` fails (E0502). Fix: `try_clone()`
  a separate writer.
- `UnixListener` MUST be owned by a living struct (`IpcServer`), or the
  socket can go silent while the process lives (observed operationally).
- Compositor workspace lists MUST follow the compositor: re-query on
  init/empty/destroy events; a `focus` change event is authoritative, not the
  `focused` flag; filter out non-workspace window events.
- Battery: listen to `State`, not just `Percentage` (plugging changes state,
  not level).
- Wi-Fi passwords: one-shot agents/profiles only, NEVER stored, NEVER over
  CLI.
- Tray SNI: startup scan needs per-connection timeouts (a dead Qt connection
  blocks `Introspect` ~25 s); dbusmenu children come double-boxed
  (`Value::Value(Structure)`); `IconPixmap` is ARGB, `icon-data` is encoded
  PNG (`89 50 4E 47`).
- PAM codes: `PAM_CRED_EXPIRED`=16, `PAM_CRED_ERR`=17, `PAM_CONV_ERR`=19;
  `PAM_CRED_INSUFFICIENT`(8) = caller lacks privileges, NEVER a wrong
  password. Conversations replies go through `calloc`+`strdup` (PAM frees
  with `free`).
- Logs: install a `tracing_subscriber` in `run()` or all `warn!`/`info!` go
  nowhere; `RUST_LOG=debug` raises the level.

**Method rules (how not to fool yourself):**

- Proof before claims: a screenshot proves only if the widget is visibly
  identifiable in it. Reliable hide/show test = pixel difference between two
  states.
- Compiling + a `Success` reply is NOT proof — only observable effect counts.
- NEVER silent code: unknown action → `false`, failed call → `warn!`,
  fallback documented in a comment.
- NEVER conclude from absent logs: empty log + dubious behavior = missing
  observability, not proof of health.
- The user looking at the screen is the reference observer: if automated
  measurement disagrees, question the measurement first.
- Measure idle CPU in `/proc` ticks over 15 s (HZ=100); compare PSS, not RSS
  (shared libs triple-count RSS).

**Bump procedure (`gpui-pre`):** update `gpui` (`package = "gpui-pre"`) and
`gpui_platform` (`package = "gpui-pre-platform"`) together to the same
`=X.Y.Z` in workspace `Cargo.toml`, run
`cargo update -p gpui-pre -p gpui-pre-platform`, then
`cargo check --workspace --all-targets`, and fix using this appendix.
