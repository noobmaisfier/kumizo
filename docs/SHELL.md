# WidgetKit Shell Creation Guide (AI Agents & Developers Reference)

> **Role of this document**: This document is a complete, self-sufficient developer and AI prompt specification for building custom Wayland desktop shells using **WidgetKit**. Any AI agent or developer provided with this guide has full technical and design freedom to propose, architect, and implement complete desktop environments (bars, docks, launchers, notifications, panels, OSDs, control centers).

---

## 1. Core Architecture & Philosophy

WidgetKit is a high-performance Wayland desktop shell SDK written in pure Rust, powered by **GPUI** (the GPU-accelerated engine behind Zed) and the Wayland `wlr-layer-shell` protocol.

### Key Tenets
1. **Single Process Architecture**:
   Unlike legacy shells (Waybar, Eww, AGS) that spawn multiple daemons, WidgetKit runs the **entire shell** (bars, docks, popups, launchers, notifications, OSD) in **one single process**.
   - One GPU / Vulkan context initialized.
   - One font cache and layout pipeline.
   - One system connection per service (D-Bus, Sway/Hyprland IPC).
   - In-memory zero-latency communication between windows (`cx.toggle_window("launcher")`).
   - ~80 MB RAM total footprint vs 250+ MB for multi-daemon setups.
   *(Exception: Lockscreen runs as an isolated binary for PAM security reasons).*

2. **Bring Your Own UI ("Headless SDK")**:
   WidgetKit imposes **no styling framework, no CSS stylesheets, no fixed layouts, and no default themes**.
   You build the UI using GPUI's declarative, Tailwind-like layout engine directly in Rust.
   Whether you want:
   - Minimalist text-only tiling bars,
   - Mac-style floating pill docks with magnification,
   - Cyberpunk neo-brutalist interfaces with harsh borders and misregistered offset shadows (like the reference *Riso* theme),
   - Glassmorphism / Frosted acrylic blur with gradients,
   - Dynamic Island style expandable status bubbles,
   **you have 100% aesthetic and layout freedom.**

3. **Zero Idle CPU**:
   Every service is strictly event-driven (kernel uevents, D-Bus signal subscription, compositor IPC sockets). If nothing on the system changes, the shell consumes **0.0% CPU** and does not wake the GPU. Views only redraw when notified (`cx.notify()`).

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
        
        // 2. Declare layer-shell surfaces (windows)
        .window(bar_window(), |cx| cx.new(TopBar::new))
        .window(launcher_window(), |cx| cx.new(AppLauncher::new))
        .window(tasks_window(), |cx| cx.new(TaskSwitcher::new))
        .window(notifications_window(), |cx| cx.new(NotificationToasts::new))
        .window(osd_window(), |cx| cx.new(VolumeOsd::new))
        
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
    .namespace("wk-mybar")                     // Compositor layer-rule selector (blur/shadow)
    .keyboard_interactivity(KeyboardInteractivity::None) // None | OnDemand | Exclusive
    .transparent(true)                         // Transparent background buffer
    .start_hidden(true)                        // Surface not created until first opened
```

### Essential Window Archetypes

| Archetype | Layer | Anchor | Exclusive Zone | Keyboard Interactivity | Sizing / Notes |
|---|---|---|---|---|---|
| **Top/Bottom Bar** | `Top` | `TOP_BAR` (`TOP \| LEFT \| RIGHT`) | `height` (e.g. `32`) | `None` | `.height(32.0)` (width stretches automatically) |
| **Vertical Rail / Dock** | `Top` | `LEFT_BAR` (`LEFT \| TOP \| BOTTOM`) | `width + margins` | `None` | `.width(64.0)` (height stretches automatically) |
| **Floating Island Bar** | `Top` | `TOP` | `0` or `height` | `None` | `.size(Some(600.0), Some(40.0))` with margins |
| **Fullscreen Modal** (Launcher, Lock) | `Overlay` | `FULLSCREEN` | `-1` (covers bars) | `OnDemand` or `Exclusive` | `.start_hidden(true)`, no size needed |
| **Popup / Dropdown** (Calendar, Tray) | `Overlay` | `TOP \| RIGHT` | `0` | `None` or `OnDemand` | `.size(Some(320.0), Some(400.0))`, `.start_hidden(true)` |
| **OSD** (Volume, Brightness) | `Overlay` | `BOTTOM` | `0` | `None` | `.start_hidden(true)` |

> [!IMPORTANT]
> **Opposite Anchors = Automatic Expansion**:
> - `Anchor::LEFT | Anchor::RIGHT`: Stretches across the full screen width. Do not set `.width()`.
> - `Anchor::TOP | Anchor::BOTTOM`: Stretches across the full screen height. Do not set `.height()`.
> - `Anchor::FULLSCREEN`: Covers the entire monitor.

---

## 4. In-Memory Window Control

Windows declared with `.start_hidden(true)` do not occupy Wayland resources until displayed. You can toggle or show them from any event handler **without IPC**:

```rust
// Inside any GPUI event handler or context:
cx.toggle_window("launcher");
cx.show_window("calendar");
cx.hide_window("notifications");
```

### External CLI / Hotkey Control (`widgetctl` / `kumizo-ctl`)
The shell automatically exposes a single UNIX socket at `$XDG_RUNTIME_DIR/widgetkit.sock`:
```bash
widgetctl ping                   # Health check
widgetctl toggle launcher        # Bound to Super+Space / Super+a in sway/hyprland
widgetctl toggle tasks           # Bound to Super+w / Alt+Tab
widgetctl show notification_center
widgetctl service audio toggle_mute
```

---

## 5. Built-In Reactive Services

Access any registered service in a view's constructor:
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

### Services Reference Table

| Service | Methods & Types | Typical Usage |
|---|---|---|
| **`Clock`** | `time_hm()`, `time_hms()`, `date_formatted()`, `hour()`, `minute()`, `second()`, `datetime()` | Time & date widgets. Zero drift, wakes up exactly on boundary. |
| **`Battery`** | `percent() -> u8`, `is_charging() -> bool`, `state() -> BatteryState`, `energy_rate_watts()`, `time_to_empty()`, `time_to_full()` | Battery badges, power warnings, charging icons. |
| **`Audio`** | `volume() -> u8`, `is_muted() -> bool`, `sink_name()`, `set_volume(pct)`, `toggle_mute()` | Volume sliders, mute indicators, OSD overlay. |
| **`Brightness`** | `percent() -> u8`, `device()`, `set_percent(pct)`, `increase(step)`, `decrease(step)` | Backlight brightness bar/OSD. 100% event-driven kernel uevents. |
| **`Network`** | `is_connected() -> bool`, `ssid() -> Option<&str>`, `signal_percent() -> u8`, `backend()` | Wi-Fi status, network indicators. |
| **`Compositor`** | `workspaces() -> &[Workspace]`, `switch_workspace(id)`, `windows() -> &[AppWindow]`, `focus_window(con_id)`, `active_window() -> Option<&ActiveWindow>` | Workspace switcher buttons, active window title, task switcher list with Sway/Hyprland IPC. |
| **`Apps`** | `apps() -> &[AppInfo]`, `search(query) -> Vec<AppInfo>`, `launch(&app)`, `find_icon(app_id_or_class) -> Option<Arc<RenderImage>>` | Application launcher, task switcher icons with fuzzy matching and GPU textures. |
| **`Mpris`** | `title() -> Option<&str>`, `artist() -> Option<&str>`, `status() -> PlaybackStatus`, `play_pause()`, `next()`, `previous()` | Media player widget, music equalizer, dynamic topbar label. |
| **`Tray`** | `items() -> Vec<TrayItem>`, `activate(id)`, `open_menu(id)`, `close_menu()` | System tray icons (StatusNotifierItem) and context menus. |
| **`Notifications`**| `toasts() -> &[Notification]`, `history() -> &[Notification]`, `unread_count() -> usize`, `is_dnd() -> bool`, `toggle_dnd()`, `dismiss_toast(id)`, `remove_history(id)`, `clear_history()`, `invoke_action(id, key)` | In-process notification server (replaces dunst/mako). |
| **`Clipboard`** | `items() -> &[ClipboardItem]`, `search(query)`, `copy_item(id)`, `clear()` | Clipboard history manager overlay. |

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

        // 2. Inotify file watching (re-reads when writer closes or renames)
        let cpu_temp = Source::watch_file(cx, "/sys/class/thermal/thermal_zone0/temp", |text| {
            text.trim().parse::<i32>().ok().map(|milli| milli / 1000)
        }).track(cx);

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
        cx.toggle_window("launcher");
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
  In GPUI, `cx.spawn` takes an `async move` closure receiving `(this, cx)` where `this` is `WeakEntity<Self>` and `cx` is `&mut AsyncContext<Self>`:
  ```rust
  cx.spawn(async move |this, cx| {
      // Async work (channels, timers, IO)...
      let _ = this.update(cx, |view, cx| {
          // Mutate view state
          cx.notify();
      });
  }).detach();
  ```

- **Focus Management (`window.focus`)**:
  In GPUI's `Render::render(&mut self, window: &mut Window, cx: &mut Context<Self>)`, focusing an element is done through the `window` reference, NOT `cx.focus`:
  ```rust
  if !self.focus.is_focused(window) {
      window.focus(&self.focus, cx);
  }
  ```
  Elements track focus using `.track_focus(&self.focus)`.

- **View Re-rendering (`cx.notify()`)**:
  Call `cx.notify()` on the context whenever local view state changes to trigger a redraw.
  There is no `cx.refresh_windows()`; each window/view re-renders reactively upon `cx.notify()` or when an observed service/source updates.

- **Relative & Absolute Positioning**:
  Positioning modifiers are methods on element builders, NOT standalone functions:
  ```rust
  div().relative()  // Element sets position: relative
  div().absolute()  // Element sets position: absolute
  ```
  Do not use `gpui::relative(...)` or `gpui::absolute(...)` as standalone functions.

---

## 8. Theme System

To keep styling decoupled and modular:

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

When building dynamic shells, `pal::...` functions read from an active theme state (or a theme entity registered in GPUI globals), allowing the entire shell to re-render instantly on theme changes (`cx.notify()`).

### 2. External Themes Configuration

Themes are stored as `.toml` files in `~/.config/<shell-name>/themes/*.toml` (with built-in embedded strings as fallback if the directory is empty).

---

## 9. Mechanical Starter Skeleton (Structure & Wiring Reference)

```rust
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        // 1. Register required background services
        .service::<Clock>()
        .service::<Battery>()
        .service::<Compositor>()

        // 2. Register layer-shell surfaces (bars, islands, rails, pods, drawers, overlays)
        .window(primary_surface_window(), |cx| cx.new(PrimarySurface::new))
        .window(overlay_surface_window(), |cx| cx.new(OverlaySurface::new))

        // 3. Start unified event loop
        .run();
}

fn primary_surface_window() -> LayerWindow {
    // Configure anchors, exclusive zone, and dimensions according to your topology:
    LayerWindow::new("primary_surface")
        .layer(Layer::Top)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT) // Or LEFT_BAR, or TOP floating island, or Corner Pod
        .exclusive_zone(36)
        .namespace("my-shell-surface")
}

fn overlay_surface_window() -> LayerWindow {
    LayerWindow::new("overlay_surface")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::OnDemand)
        .start_hidden(true)
        .namespace("my-shell-overlay")
}

/// Structural component demonstrating service access and event handling.
/// The layout, orientation, and visual structure inside `render()` are completely yours to define.
struct PrimarySurface {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    compositor: Entity<Compositor>,
}

impl PrimarySurface {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            compositor: cx.service::<Compositor>(),
        }
    }
}

impl Render for PrimarySurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let bat = self.battery.read(cx);
        let comp = self.compositor.read(cx);

        // Blank layout container: shape, flex direction, and alignment are yours to decide
        div()
            .size_full()
            .bg(pal::bg())
            .text_color(pal::text())
            // 1. Triggering another shell window (zero-latency in-memory call)
            .child(
                div()
                    .id("action-trigger")
                    .cursor_pointer()
                    .child("Toggle Overlay")
                    .on_click(|_, _, cx| cx.toggle_window("overlay_surface")),
            )
            // 2. Interacting with compositor workspaces
            .children(
                comp.workspaces().iter().map(|ws| {
                    let id = ws.id;
                    let is_active = ws.is_active;
                    div()
                        .id(format!("ws-{}", id))
                        .cursor_pointer()
                        .when(is_active, |s| s.text_color(pal::accent()))
                        .child(ws.name.clone())
                        .on_click(move |_, _, cx| {
                            cx.service::<Compositor>().update(cx, |c, _| c.switch_workspace(id));
                        })
                })
            )
            // 3. Displaying reactive system telemetry
            .child(clock.time_hm())
            .child(format!("{}%", bat.percent()))
    }
}

/// Interactive modal/drawer overlay component.
struct OverlaySurface;

impl OverlaySurface {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for OverlaySurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Fullscreen backdrop
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000bb))
            .on_click(|_, _, cx| cx.hide_window("overlay_surface"))
            .child(
                div()
                    .p_8()
                    .bg(pal::surface())
                    .border_1()
                    .border_color(pal::border())
                    .rounded_lg()
                    .child("Interactive Modal / Drawer Canvas"),
            )
    }
}
```

---

## 10. Verification & Quality Checklist

Before considering your shell complete:
- [ ] **Build Check**: `cargo check --package <your-shell> --offline` compiles with 0 errors and warnings.
- [ ] **Run & Socket Test**: `widgetctl ping` returns `PONG`.
- [ ] **Zero Idle CPU**: Check with `top` or `htop` — when undisturbed, your process stays at `0.0%` CPU.
- [ ] **Hotkeys**: Sway/Hyprland hotkeys can toggle your windows (`widgetctl toggle <id>`).
- [ ] **Text Boundaries**: All titles use `.truncate()` and `.min_w_0()`; no long URL or song title overflows off-screen.
- [ ] **Clean Hover**: No double `.hover()` calls on the same element.
