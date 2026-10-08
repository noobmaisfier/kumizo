# WidgetKit Shell Creation Guide (AI Agents & Developers Reference)

> **Role of this document**: This document is a complete, self-sufficient developer and AI prompt specification for building custom Wayland desktop shells using **WidgetKit**. Any AI agent or developer provided with this guide has full technical and design freedom to propose, architect, and implement complete desktop environments (bars, docks, launchers, notifications, panels, OSDs, control centers).
>
> **How to read this doc — MUST vs SHOULD:**
> - **MUST** = hard constraint. Violating it fails to compile, panics, or silently shows nothing. Marked with [!CAUTION] / [!WARNING] or the word MUST.
> - **SHOULD** = starting point that worked once on one machine. Adapt numbers, names, palettes and topology to the target hardware and taste. Every pixel value, window id and hex color below is an example, not a default.

---

## 1. Core Architecture & Philosophy

WidgetKit is a high-performance Wayland desktop shell SDK written in pure Rust, powered by **GPUI** (the GPU-accelerated engine behind Zed) and the Wayland `wlr-layer-shell` protocol.

### Key Tenets
1. **Single Process Architecture**:
   Unlike legacy shells (Waybar, Eww, AGS) that spawn multiple daemons, WidgetKit runs the **entire shell** (bars, docks, popups, launchers, notifications, OSD) in **one single process**.
   - One GPU / Vulkan context initialized.
   - One font cache and layout pipeline.
   - One system connection per service (D-Bus, Sway/Hyprland IPC).
   - In-memory zero-latency communication between windows (`cx.toggle_window("my_popup")` — your own ids, see §4).
   - ~80 MB RAM total footprint vs 250+ MB for multi-daemon setups.
   *(Exception: Lockscreen runs as an isolated binary for PAM security reasons).*

2. **Bring Your Own UI ("Headless SDK")**:
   WidgetKit imposes **no styling framework, no CSS stylesheets, no fixed layouts, and no default themes**.
   You build the UI using GPUI's declarative, Tailwind-like layout engine directly in Rust.
   **you have 100% aesthetic and layout freedom.**

3. **Zero Idle CPU**:
   Every service is strictly event-driven (kernel uevents, D-Bus signal subscription, compositor IPC sockets). If nothing on the system changes, the shell stays near idle and does not wake the GPU (a clock bar wakes at 1 Hz, ~0.4% — not strict 0.0%, see §10). Views only redraw when notified (`cx.notify()`).

---

## 2. Shell Structure Anatomy

A shell application is defined in a `main()` function using `WidgetKit::new()`:

```rust
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        // 1. Register shared background services
        .service::<Clock>()
        .service::<Battery>()
        .service::<Audio>()
        .service::<Network>()
        .service::<Compositor>()
        .service::<Apps>()
        .service::<Notifications>()
        .service::<Clipboard>()
        
        // 2. Declare layer-shell surfaces (windows) — ids below are placeholders
        .window(bar_window(), |cx: &mut gpui::App| cx.new(TopBar::new))
        .window(launcher_window(), |cx: &mut gpui::App| cx.new(AppLauncher::new))
        .window(tasks_window(), |cx: &mut gpui::App| cx.new(TaskSwitcher::new))
        .window(notifications_window(), |cx: &mut gpui::App| cx.new(NotificationToasts::new))
        .window(osd_window(), |cx: &mut gpui::App| cx.new(VolumeOsd::new))
        
        // 3. Start the unified event loop (never returns)
        .run();
}
```

---

## 3. Wayland Layer Windows (`LayerWindow`)

A `LayerWindow` is a Wayland surface pinned to a specific layer and screen edge via `wlr-layer-shell`.

### Window Configuration Builder

```rust
LayerWindow::new("unique_window_id")
    .layer(Layer::Top)                         // Background | Bottom | Top | Overlay
    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT) // Bitflags
    .exclusive_zone(32)                        // Reserved space for tiled windows
    .size(Some(800.0), Some(32.0))             // Width, Height in logical pixels
    .margins(Margins::new(8, 8, 0, 8))         // Top, Right, Bottom, Left
    .namespace("my-shell-bar")                    // Compositor layer-rule selector (blur/shadow); prefix per shell (§4)
    .keyboard_interactivity(KeyboardInteractivity::None) // None | OnDemand | Exclusive
    .transparent(true)                         // Transparent background buffer
    .start_hidden(true)                        // Surface not created until first opened
    // .monitor(MonitorTarget::Primary)        // Which output (default: Primary, see §10)
```

### Essential Window Archetypes

| Archetype | Layer | Anchor | Exclusive Zone | Keyboard Interactivity | Sizing / Notes |
|---|---|---|---|---|---|
| **Top/Bottom Bar** | `Top` | `TOP_BAR` (`TOP \| LEFT \| RIGHT`) | `height` | `None` | `.height(h)` for your `h` (width stretches automatically) |
| **Vertical Rail / Dock** | `Top` | `LEFT_BAR` (`LEFT \| TOP \| BOTTOM`) | `width + margins` | `None` | `.width(w)` for your `w` (height stretches automatically) |
| **Floating Island Bar** | `Top` | `TOP` | `0` or `height` | `None` | `.size(Some(w), Some(h))` with margins |
| **Fullscreen Modal** | `Overlay` | `FULLSCREEN` | `-1` (covers bars) | `OnDemand` or `Exclusive` | `.start_hidden(true)`, no size needed |
| **Popup / Dropdown** | `Overlay` | corner of your choice | `0` | `None` or `OnDemand` | explicit `.size(...)`, `.start_hidden(true)` |
| **OSD** | `Overlay` | edge of your choice | `0` | `None` | explicit `.size(...)`, `.start_hidden(true)` |

> [!NOTE]
> All pixel values ever shown here (32, 64, 600x40, 320x400) are starting
> points from one 1080p machine — measure against your own outputs and adapt.
> Pick anchors for YOUR topology, not the table's example widgets.

> [!IMPORTANT]
> **Opposite Anchors = Automatic Expansion**:
> - `Anchor::LEFT | Anchor::RIGHT`: Stretches across the full screen width. Do not set `.width()`.
> - `Anchor::TOP | Anchor::BOTTOM`: Stretches across the full screen height. Do not set `.height()`.
> - `Anchor::FULLSCREEN`: Covers the entire monitor.
>
> **Sizing priority** (`docs/REFERENTIEL.md:252-273`): explicit `width`/`height`
> wins first, then opposite-anchor stretch (`output_width - margins`), then the
> 800px fallback. A layer-shell surface is NOT stretched by the compositor —
> Sway honors anchors for position only. Never put margins into `origin`; the
> compositor applies them and they get counted twice.

### OSD Auto-Hide (Generation Token)

Every volume/brightness keypress spawns a hide-timer. Without cancellation the
first timer hides the OSD mid-adjustment (`docs/REFERENTIEL.md:645-670`):
```rust
osd.generation = osd.generation.wrapping_add(1);
let current_gen = osd.generation;
cx.spawn(async move |this, cx| {
    async_io::Timer::after(Duration::from_millis(2000)).await;
    let _ = this.update(cx, |osd, cx| {
        if osd.generation == current_gen {
            osd.visible = false;
            cx.notify();
        }
    });
}).detach();
```

---

## 4. In-Memory Window Control

Windows declared with `.start_hidden(true)` do not occupy Wayland resources until displayed. You can toggle or show them from any event handler **without IPC**:

> [!IMPORTANT]
> Entity-vs-surface lifecycle (`crates/widgetkit/src/app.rs:107-132`):
> the VIEW (`Entity`, your struct) is built at startup for EVERY window,
> including `start_hidden` ones — `new()` runs and `cx.observe(...)`
> subscriptions are live immediately. Only the Wayland SURFACE is lazy:
> `show` creates it from the existing entity, `hide` destroys it and keeps
> the state. So a hidden OSD/toasts view CAN observe services itself and
> call `cx.show_window("itself")` — driving it from another view (e.g. the
> bar) is a design choice, never a necessity.

```rust
// Inside any GPUI event handler or context (ids are yours — examples only):
cx.toggle_window("my_launcher");
cx.show_window("my_popup");
cx.hide_window("my_toasts");
```

> [!CAUTION]
> Window ids are global on the single socket. Two shells (or stale bindings from
> a previous shell) address the same namespace: `toggle launcher` fires whichever
> shell owns a window called `launcher` today. MUST: prefix ids per shell
> (`kiln-launcher`, `wk-calendar`) or verify with `widgetctl status` which shell
> answers before binding hotkeys.

### External CLI / Hotkey Control (`widgetctl`)
The shell automatically exposes a single UNIX socket at `$XDG_RUNTIME_DIR/widgetkit.sock`
(override with `$WIDGETKIT_SOCKET` for tests/nested sessions).
Single instance is enforced by `Ping`-before-bind (`docs/REFERENTIEL.md:323-341`):
an existing socket that answers keeps its owner and the second launch exits
with "already running"; a silent socket is stale (crash) and gets replaced.
Never `rm widgetkit.sock` of a live shell — it keeps running unreachable.
The lockscreen opts out (`.ipc(false)`): no socket, no remote control, by design.
```bash
widgetctl ping                        # Health check
widgetctl status                      # WHO answers? check window list first
widgetctl toggle my_launcher          # Bind to a free key in sway/hyprland
widgetctl service audio toggle_mute
```

---

## 5. Built-In Reactive Services

Register ONLY the services your views consume — each `.service::<S>()` opens real
system connections. Then access them in a view's constructor:
```rust
struct MyWidget {
    battery: Entity<Battery>,
    compositor: Entity<Compositor>,
    audio: Entity<Audio>,
}

impl MyWidget {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            // cx.service::<S>() automatically observes the service!
            // When the service changes, the view re-renders automatically.
            battery: cx.service::<Battery>(),
            compositor: cx.service::<Compositor>(),
            audio: cx.service::<Audio>(),
        }
    }
}
```

In `render(&mut self, _, cx)`:
```rust
let bat = self.battery.read(cx);
let comp = self.compositor.read(cx);
```

> [!CAUTION]
> The receiver decides HOW you call it (verified in `crates/wk-services/src/`):
> - `&self` (e.g. `volume()`, `toggle_power()`, `play_pause()`) — call directly
>   on a `read(cx)` guard.
> - `&mut self` (e.g. `set_volume(pct: u8)`, `toggle_mute()`, `increase_volume(step: u8)`)
>   — MUST go through `entity.update(cx, |s, _| s.set_volume(50))`; a `read()`
>   guard is read-only and won't compile.
> - Borrowed returns (`-> &str`, `-> &[...]`, e.g. `sink_name()`, `devices()`) —
>   they borrow the `read()` guard and MUST NOT escape `render` (E0521). Copy
>   into a `String`/`Vec` (or format inline) before leaving `render`.
> - Owned args (`path: String` in `toggle_device_connection`) — pass
>   `dev_path.clone()` / `.to_string()` at the callsite.

### Services Reference Table

`[&mut]` = requires `entity.update(cx, |s, _| ...)` (see CAUTION in §5). Unmarked methods are `&self`: callable on a `read(cx)` guard.

| Service | Methods & Types | Typical Usage |
|---|---|---|
| **`Clock`** | `time_hm()`, `time_hms()`, `date_formatted()`, `hour()`, `minute()`, `second()`, `datetime()` | Time & date widgets. Zero drift, wakes up exactly on boundary. |
| **`Battery`** | `percent() -> u8`, `is_charging() -> bool`, `state() -> BatteryState`, `energy_rate_watts()`, `time_to_empty()`, `time_to_full()` | Battery badges, power warnings, charging icons. |
| **`Audio`** | `volume(&self) -> u8`, `is_muted(&self) -> bool`, `sink_name(&self) -> &str`, `icon(&self)`, `set_volume(&mut self, pct: u8)`, `increase_volume(&mut self, step: u8)`, `decrease_volume(&mut self, step: u8)`, `toggle_mute(&mut self)` | Volume sliders, mute indicators, OSD overlay. |
| **`Bluetooth`** | `is_available(&self)`, `is_powered(&self)`, `is_discovering(&self)`, `devices(&self) -> &[BluetoothDevice]`, `connected_devices(&self)`, `paired_devices(&self)`, `connected_count(&self)`, `toggle_power(&self)`, `toggle_scan(&self)`, `toggle_device_connection(&self, path: String, connected: bool)` | Bluetooth toggles, device lists, per-device connect. All `&self`: callable on a `read()` guard. |
| **`Brightness`** | `percent() -> u8`, `device()`, `set_percent(pct)`, `increase()`, `decrease()` (±5) | Backlight brightness bar/OSD. Event-driven kernel uevents (`sysfs_notify` + netlink), poll only as fallback. |
| **`Network`** | `is_connected(&self) -> bool`, `ssid(&self) -> Option<&str>`, `signal_percent(&self) -> Option<u8>`, `backend(&self)` | Wi-Fi status, network indicators. `signal_percent` is `Option`: no signal = `None`, never unwrap blindly. |
| **`Compositor`** | `workspaces(&self) -> &[Workspace]`, `switch_workspace(id: i32) [&mut]`, `windows(&self) -> &[AppWindow]`, `focus_window(&self, con_id: i64)`, `active_window(&self) -> Option<&ActiveWindow>` | Workspace switcher buttons, active window title, task switcher list with Sway/Hyprland IPC. |
| **`Apps`** | `apps() -> &[AppInfo]`, `search(query) -> Vec<AppInfo>`, `launch(&app)`, `find_icon(app_id_or_class) -> Option<Arc<RenderImage>>` | Application launcher, task switcher icons with fuzzy matching and GPU textures. |
| **`Mpris`** | `title() -> Option<&str>`, `artist() -> Option<&str>`, `status() -> PlaybackStatus`, `play_pause()`, `next()`, `previous()` | Media player widget, music equalizer, dynamic topbar label. |
| **`Tray`** | `items(&self) -> Vec<TrayItem>`, `activate(&self, id: &str)`, `open_menu(id: &str) [&mut]`, `close_menu() [&mut]` | System tray icons (StatusNotifierItem) and context menus. |
| **`Notifications`**| `toasts(&self)`, `history(&self)`, `unread_count(&self)`, `is_dnd(&self)`, `toggle_dnd() [&mut]`, `dismiss_toast(id: u32) [&mut]`, `remove_history(id: u32) [&mut]`, `clear_history() [&mut]`, `invoke_action(&self, id: u32, key: &str)` | In-process notification server (replaces dunst/mako). |
| **`Clipboard`** | `items(&self)`, `search(&self, query: &str)`, `copy_item(id: u64) [&mut]`, `clear() [&mut]`, `copy_text(&self, text: &str)` | Clipboard history manager overlay. |

### Key Compositor & Notification Types

#### Compositor Types (`wk-compositor` / `Compositor`)

```rust
/// Wayland workspace descriptor
pub struct Workspace {
    pub id: i32,                // e.g. 1, 2, 3
    pub name: String,           // Display label (e.g. "1", "2:web", "term")
    pub is_active: bool,        // True if workspace has focus on its monitor
    pub is_urgent: bool,        // True if workspace contains an urgent window
    pub window_count: usize,    // Number of managed windows
    pub monitor: String,        // Output/monitor name (e.g. "eDP-1")
}

/// Description of an open application window on the desktop
pub struct AppWindow {
    pub id: i64,                // Compositor container/window ID
    pub title: String,          // Window title
    pub class: String,          // Application class/app_id (e.g. "firefox", "kitty")
    pub workspace_id: i32,      // Workspace ID where window resides
    pub workspace_name: String, // Workspace name
    pub is_focused: bool,       // True if window currently has focus
}

/// Description of the currently focused window on the desktop
pub struct ActiveWindow {
    pub title: String,          // Window title
    pub class: String,          // Application class/app_id
    pub workspace_id: i32,      // Workspace ID
}
```

#### Notification Types (`wk-services` / `Notifications`)

```rust
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

pub struct NotificationAction {
    pub key: String,            // Action identifier (e.g. "default", "reply")
    pub label: String,          // Button label shown to user
}

pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub app_icon: Option<Arc<RenderImage>>,
    pub urgency: Urgency,
    pub actions: Vec<NotificationAction>,
    pub created_at: time::OffsetDateTime,
    pub generation: u64,
}
```

#### Data Types: All Fields & Variants (from `crates/wk-services/src/`, `wk-compositor/src/`)

Methods are not enough — views also read struct FIELDS. Exact shapes:

```rust
// Bluetooth (bluetooth.rs)
pub struct BluetoothDevice {
    pub path: String,       // D-Bus object path — pass to toggle_device_connection
    pub address: String,     // MAC, fallback display name
    pub name: String,        // may be empty: prefer display_name()
    pub icon: Option<String>,// BlueZ icon name: "audio-headset", "input-mouse", ...
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub battery_percent: Option<u8>,
}
// helpers: display_name() -> &str, type_icon() -> glyph

// Apps (apps.rs)
pub struct AppInfo {
    pub id: String,         // desktop file id, e.g. "firefox"
    pub name: String,
    pub comment: String,
    pub exec: String,       // sanitized launch command
    pub icon_name: String,  // original icon name/path
    pub icon: Option<Arc<RenderImage>>, // decoded GPU texture (PNG & SVG)
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
}

// Tray (tray.rs)
pub struct TrayItem {
    pub id: String,   // registry key (bus + path) — use for clicks
    pub title: String, // tooltip
    pub icon: Option<Arc<RenderImage>>, // None while loading
    pub attention: bool,
}
pub struct TrayMenuItem {
    pub id: i32,             // pass to click_menu_item(id: i32)
    pub label: String,
    pub enabled: bool,       // skip interaction when false
    pub separator: bool,
    pub toggle: Option<bool>,// Some(checked) for checkmark/radio items
    pub children: Vec<TrayMenuItem>, // recursive
}
pub struct OpenMenu {
    pub owner: String, // registry key of the owning item
    pub title: String,
    pub items: Vec<TrayMenuItem>,
}
pub enum TrayStatus { Active, Passive, NeedsAttention } // Passive items excluded from items()

// Media (mpris.rs)
pub enum PlaybackStatus { Playing, Paused, Stopped } // helpers: icon(), is_playing()
// title(), artist(), album(), player_name() -> Option<&str> (borrowed!)
// display_label() -> String ("artist - title")

// Battery (battery.rs)
pub enum BatteryState {
    Charging, Discharging, Empty, FullyCharged,
    PendingCharge, PendingDischarge, Unknown, // helpers: as_str(), icon(percent)
}
// energy_rate_watts() -> Option<f64>, time_to_empty/full() -> Option<Duration>

// Network (network.rs)
pub enum WifiBackend { NetworkManager, Iwd, Fallback }
// ssid() -> Option<&str>, signal_percent() -> Option<u8> (both borrowed/optional!)
// display_label() -> String, icon() -> glyph

// Clock (clock.rs)
pub enum ClockResolution { Seconds, Minutes } // set_resolution() lowers wakeups to 1/min

// Clipboard (clipboard.rs)
pub struct ClipboardItem {
    pub id: u64,              // pass to copy_item(id: u64) / remove_item(id: u64)
    pub text: String,
    pub timestamp: time::OffsetDateTime,
}
// helpers: preview() -> String, line_count() -> usize, char_count() -> usize
```

Borrow rule reminder (§5 CAUTION): `&str` / `&[...]` fields and getters borrow
the `read()` guard — copy what you display before leaving `render`.

Methods on `Notifications`:
- `toasts(&self) -> &[Notification]`: Ephemeral popups currently active on screen.
- `history(&self) -> &[Notification]`: Complete notification history (newest first).
- `unread_count(&self) -> usize`: Number of notifications stored in history.
- `is_dnd(&self) -> bool`: Whether Do Not Disturb is enabled.
- `toggle_dnd(&mut self)`: Toggle Do Not Disturb mode.
- `dismiss_toast(&mut self, id: u32)`: Dismiss popup from screen (keeps history).
- `remove_history(&mut self, id: u32)`: Remove notification from history and screen.
- `clear_history(&mut self)`: Clear all history and toasts.
- `invoke_action(&self, id: u32, key: &str)`: Trigger interactive action on notification.

---

## 6. V2 Ad-Hoc Data Sources (`Source<T>`)

Need custom data without writing a full `Service`? Use `Source<T>`:

```rust
use widgetkit::prelude::*;

struct CustomBar {
    uptime: Entity<Source<String>>,
    cpu_temp: Entity<Source<i32>>,
    socket_feed: Entity<Source<String>>,
}

impl CustomBar {
    fn new(cx: &mut Context<Self>) -> Self {
        // 1. Process stdout streaming (auto-kills child with PDEATHSIG on drop)
        let uptime = ProcessSource::new("uptime")
            .arg("-p")
            .restart_after(Duration::from_secs(30))
            .lines(cx, |line| Some(line.to_string()))
            .track(cx); // .track(cx) automatically triggers view re-render!

        // 2. Inotify file watching (re-reads when writer closes or renames).
        // MUST be a regular file: /proc and /sys never raise inotify events.
        let temp = Source::watch_file(cx, "/tmp/temperature", |text| {
            text.trim().parse::<i32>().ok()
        }).track(cx);
        // For /proc or /sys files use FileSource with an explicit poll instead:
        // FileSource::new("/sys/class/thermal/thermal_zone0/temp")
        //     .poll_every(Duration::from_secs(2))
        //     .watch(cx, |text| text.trim().parse::<i32>().ok().map(|milli| milli / 1000))
        //     .track(cx);

        // 3. Private UNIX socket listener (mode 0600, cleaned up on drop)
        let socket_feed = Source::listen_unix(cx, "/tmp/shell-feed.sock", |line| {
            Some(line.to_string())
        }).track(cx);

        Self { uptime, cpu_temp, socket_feed }
    }
}
```

In `render`:
```rust
let temp = self.cpu_temp.read(cx).value(); // Option<&i32>
```

---

## 7. GPUI UI Building & Styling Primitives

WidgetKit views implement `gpui::Render`. GPUI uses a fluent Tailwind-like layout builder.

### Layout & Sizing
```rust
div()
    .flex()                      // Flexbox container
    .flex_col()                  // Column direction (or .flex_row())
    .items_center()              // Align items center
    .justify_between()           // Justify content between (or .justify_center())
    .gap_2()                     // Gap between children (or .gap(px(8.0)))
    .w(px(240.0))                // Explicit width
    .h_full()                    // 100% height
    .size_full()                 // 100% width and height
    .px_3()                      // Horizontal padding (px_1, px_2, px_4...)
    .py_1p5()                    // Vertical padding 6px
```

### Colors & Borders
```rust
// Hex values below are placeholders — bring your own palette (see §8 if you
// need runtime-switchable themes, otherwise hardcode).
div()
    .bg(rgb(0x1e1e2e))           // Hex RGB background
    .bg(rgba(0x181825ee))        // Alpha transparency
    .bg(hsla(0.6, 0.7, 0.5, 1.0))// HSLA color
    .text_color(rgb(0xcdd6f4))   // Text color
    .border_2()                  // Border width (2px)
    .border_color(rgb(0x89b4fa)) // Border color
    .rounded_lg()                // Rounded corners (rounded_sm, rounded_md, rounded_xl, rounded_full)
```

### Interactivity & Hover Rules
```rust
div()
    .id("clickable-tile")        // REQUIRED: .id() is mandatory before any interactive method!
    .cursor_pointer()
    .hover(|s| s.bg(rgb(0x313244)))
    .active(|s| s.top(px(2.0)))  // Physical press sink effect
    .on_click(|event, window, cx| {
        cx.toggle_window("my_popup"); // your own window id (§4)
    })
```

> [!WARNING]
> **GPUI Trap: Never call `.hover()` twice on the same element**:
> Calling `.hover(...)` more than once on the same `div()` will panic with `hover style already set`. Keep reusable tile builder helpers free of hardcoded hover calls, or set hover only at the callsite.

### Text Truncation Rule
```rust
div()
    .min_w_0()                   // Mandatory inside flex containers to allow shrinking!
    .truncate()                  // Overflow hidden + character-accurate ellipsis "..."
    .child(long_window_title)
```
> [!CAUTION]
> **Never slice strings with byte indexes (`&s[..20]`)**: UTF-8 multi-byte characters or emoji will cause a panic. Always use `.truncate()` in layout or iterate over `.chars()`.

### Image Textures (App & Tray Icons)
```rust
if let Some(icon) = app.icon.clone() {
    div().child(gpui::img(icon).w(px(24.0)).h(px(24.0)))
} else {
    div().child("Fallback text or glyph")
}
```

### Smooth Animations
```rust
div().with_animation(
    "pulse-badge",
    Animation::new(Duration::from_millis(800))
        .repeat()
        .with_easing(pulsating_between(0.0, 1.0)),
    |element, delta| {
        element.opacity(0.4 + 0.6 * delta)
    },
)
```

### GPUI Signatures & API Specifics

- **Asynchronous Tasks (`cx.spawn`)**:
  `cx.spawn` takes an `async` closure, NOT a closure returning an `async` block.
  Arity depends on the receiver (`docs/REFERENTIEL.md:97-126`):
  ```rust
  // On Context<Self>: 2 args, first is WeakEntity
  cx.spawn(async move |this, cx| {
      // Async work (channels, timers, IO)...
      let _ = this.update(cx, |view, cx| { // cx is already &mut AsyncApp: NO &mut cx
          // Mutate view state
          cx.notify();
      });
  }).detach();

  // On App / AsyncApp: 1 arg — WRONG: cx.spawn(|this, mut cx| async move { ... })
  cx.spawn(async move |cx| { /* ... */ });
  ```
  On generic `Source<T>` methods annotate explicitly or E0282 fires:
  `this: gpui::WeakEntity<Source<T>>, cx: &mut gpui::AsyncApp`.
  `WeakEntity::update` returns `Result<R>` (entity may be freed) — handle with
  `let _ = ...` / `.ok()`.

- **Focus Management (`window.focus`)**:
  In GPUI's `Render::render(&mut self, window: &mut Window, cx: &mut Context<Self>)`, focusing an element is done through the `window` reference, NOT `cx.focus`:
  ```rust
  if !self.focus.is_focused(window) {
      window.focus(&self.focus, cx);
  }
  ```
  Elements track focus using `.track_focus(&self.focus)`.

- **View Re-rendering (`cx.notify()` vs `cx.refresh_windows()`)**:
  Call `cx.notify()` on the `Context` whenever local view state changes to trigger
  a redraw of that view. Views observing a service re-render automatically.
  For a global restyle (theme switch), call `cx.refresh_windows()` on `&mut App`
  — it exists (`gpui/src/app.rs:1237`, schedules one redraw for all windows).
  `Context<V>` derefs to `App`, so both work from a view.

- **Relative & Absolute Positioning**:
  Positioning modifiers are methods on element builders, NOT standalone functions:
  ```rust
  div().relative()  // Element sets position: relative
  div().absolute()  // Element sets position: absolute
  ```
  Do not use `gpui::relative(...)` or `gpui::absolute(...)` as standalone functions.
  For fractional fill use `gpui::DefiniteLength::Fraction(ratio)`:
  ```rust
  div().h_full().w(gpui::DefiniteLength::Fraction((pct / 100.0).clamp(0.0, 1.0)))
  ```

- **`Window` Is Not an `AppContext` (`docs/REFERENTIEL.md:206-226`)**:
  `Entity::read` needs `&App`, `Entity::update` needs `C: AppContext`.
  `App`, `AsyncApp` implement it — **`Window` does not**. Inside
  `Render::render(&mut self, window: &mut Window, cx: &mut Context<Self>)`
  always use `cx` (deref-coerces to `&App`), never `window`, for
  `entity.read(cx)` / `entity.update(cx, ...)`.

- **Displays Arrive Async; `primary_display()` Is Dead (`docs/REFERENTIEL.md:13-49`)**:
  On Wayland gpui returns `None` unconditionally for `primary_display()`.
  Always use `cx.displays()`. `wl_output` globals arrive after startup, so
  `displays()` is empty in the launch closure — wait before opening windows:
  ```rust
  async fn wait_for_displays(cx: &mut gpui::AsyncApp) {
      for _ in 0..100 {
          if cx.update(|cx| !cx.displays().is_empty()) { return; }
          cx.background_executor().timer(Duration::from_millis(20)).await;
      }
  }
  ```
  `LayerWindow` already handles this in `WidgetKit::run`; only raw
  `WindowOptions` builders need it.

---

## 8. Theme System (OPTIONAL)

Skip this section if your shell has a fixed look — hardcoded colors are simpler
and perfectly fine. Only build a token + TOML system when themes must switch at
runtime without recompiling. To keep styling decoupled and modular:

### 1. Design Tokens Module (`mod pal`)

Structure tokens in a dedicated `mod pal` or palette resolver:

```rust
// ---------------------------------------------------------------------------
// Tokens and helpers
// ---------------------------------------------------------------------------

mod pal {
    use gpui::{rgb, rgba, Rgba};

    // Semantic roles, resolved from active theme or runtime config:
    pub fn bg() -> Rgba { rgb(0x0f111a) }
    pub fn surface() -> Rgba { rgb(0x1a1d2e) }
    pub fn text() -> Rgba { rgb(0xf0f2f5) }
    pub fn text_muted() -> Rgba { rgb(0x5c6370) }
    pub fn accent() -> Rgba { rgb(0x00e8c6) }
    pub fn border() -> Rgba { rgb(0x282c3f) }
}
```

When building dynamic shells, `pal::...` functions read from an active theme state (or a theme entity registered in GPUI globals), allowing the entire shell to re-render instantly on theme changes (`cx.refresh_windows()` on `&mut App`). Static shells: ignore `pal` and inline your colors.

### 2. External Themes Configuration

Themes are stored as `.toml` files in `~/.config/<shell-name>/themes/*.toml` (with built-in embedded strings as fallback if the directory is empty).

---

## 9. Minimal Wiring Skeleton (Copy THIS, Nothing Else Here Has Looks)

Bare wiring — no colors, no layout opinions. For the full recipe see
`docs/CREER-UN-WIDGET.md` §2.

```rust
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
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
                .anchor(Anchor::FULLSCREEN)
                .exclusive_zone(-1)
                .keyboard_interactivity(KeyboardInteractivity::Exclusive)
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

struct MyPopup;

impl MyPopup {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for MyPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("close-popup")
            .size_full()
            .on_click(|_, _, cx| cx.hide_window("my_popup"))
            .child("popup content goes here")
    }
}
```

---
## 10. Screens, Scale & Accessibility (Not Optional for a Daily Driver)

- **Multi-monitor**: layer-shell surfaces open per output. Decide per window:
  span (`LEFT|RIGHT` stretch), mirror (one surface per monitor via
  `MonitorTarget`), or primary-only. Test with a second output plugged AND
  unplugged — unplugging must not strand hidden windows.
- **HiDPI**: sizes are logical pixels; verify on scale 1 AND scale 2. Borders,
  gaps and font sizes that look right at 96 DPI look toy-like at 192 DPI —
  derive them from one scale factor, don't hardcode twice.
- **Keyboard / a11y**: every pointer action needs a key path (`widgetctl`
  command + compositor binding). Focus order must be reachable by keyboard
  alone; never encode state by color only (pair with text/icon).

### Responsive Panels (MUST for Side Panels & Popups)

Never hardcode `max_h(px(820.))`: on any shorter viewport the content is cut
with no way to reach it (seen live: theme grid halved at 768p). Constrain to
the viewport and scroll the overflow:

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // Panel never exceeds the viewport; excess content scrolls inside.
    let panel_max_h = window.viewport_size().height - px(82.); // top offset + margins
    div()
        .w(px(400.))
        .max_h(panel_max_h)   // NOT a fixed pixel value
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .id("my-scroll")
                .flex_1()         // take remaining panel height…
                .min_h_0()        // …and allow shrinking below content size…
                .overflow_y_scroll() // …so excess scrolls instead of overflowing
                .child(content),
        )
}
```

Rules: all three (`flex_1` + `min_h_0` + `overflow_y_scroll`) are required —
drop `min_h_0` and flex refuses to shrink, drop the scroll and content clips.
Recompute from `viewport_size()` every `render` (outputs plug/unplug, scale
changes). Verify by screenshot at the SMALLEST height you support, with the
LONGEST content (full notification history, longest SSID): bottom border of
the panel must stay on screen.

## 11. Lockscreen (Separate Binary, NOT a Shell Window)

The lockscreen (`examples/lockscreen`) MUST stay out of the shell process:
passwords + exclusive keyboard must never share memory, socket or service
registry with widgets. Wiring, from `examples/lockscreen/src/main.rs`:
- `.ipc(false)` — no socket: no conflict with the shell, no remote control.
- Fullscreen `Overlay` + `KeyboardInteractivity::Exclusive`.
- Password verified via real PAM (`wk-pam` raw FFI, `login` or `swaylock`
  service) on the background executor — never block the UI thread.
- Never reuse a shell view or service inside the locker; duplicate the few
  lines (clock display) instead of sharing code that widens the attack surface.
- Honest limit: this is a layer-shell surface, NOT `ext-session-lock`. A
  compositor still honors system shortcuts (TTY switch, etc.) over it. For a
  strict-security locker, WidgetKit would need an `ext-session-lock` backend —
  it does not have one today.

## 12. Verification & Quality Checklist (`docs/REFERENTIEL.md:697-705`)

Before considering your shell complete:
- [ ] **Build Check**: `cargo check --workspace --all-targets` compiles with 0 errors and **0 warnings**.
- [ ] **Run & Socket Test**: `widgetctl ping` returns `PONG`.
- [ ] **Idle CPU**: undisturbed process stays near idle — note a clock bar wakes at
  1 Hz (`~0.4%`, `docs/REFERENTIEL.md:442-444`), not strict `0.0%`. Measure via
  `/proc` ticks over 15 s, compare PSS not RSS (shared libs triple-count RSS).
- [ ] **Hotkeys**: Sway/Hyprland hotkeys can toggle your windows (`widgetctl toggle <id>`).
- [ ] **Proof, not claims**: a screenshot proves only what is visibly identifiable
  in it; hide/show is proven by **pixel difference** between the two states;
  compile + `Success` reply proves nothing without visible effect; fullscreen or
  locked captures prove nothing — retake them.
- [ ] **Text Boundaries**: All titles use `.truncate()` and `.min_w_0()`; no long URL or song title overflows off-screen.
- [ ] **Clean Hover**: No double `.hover()` calls on the same element.
- [ ] **Error paths**: unknown window, unknown service action, dead socket each tested.
