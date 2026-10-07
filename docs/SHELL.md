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
| **`Notifications`**| `toasts() -> &[Notification]`, `unread_count() -> usize`, `is_dnd() -> bool`, `toggle_dnd()`, `dismiss_toast(id)` | In-process notification server (replaces dunst/mako). |
| **`Clipboard`** | `items() -> &[ClipboardItem]`, `search(query)`, `copy_item(id)`, `clear()` | Clipboard history manager overlay. |

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

```

---

## 8. Theme System, TOML Presets & Wallpapers (Mandatory Standard)

To give users total customization and prevent hardcoded styling spaghetti, **every shell must implement a decoupled Theme System**.

### 1. Code Architecture: Design Tokens Module (`mod pal`)

Never hardcode colors directly in views. Structure tokens in a dedicated `mod pal` or `ThemeManager`:

```rust
// ---------------------------------------------------------------------------
// Tokens and helpers
// ---------------------------------------------------------------------------

mod pal {
    use gpui::{rgb, rgba, Rgba};

    // Semantic roles, not raw color names:
    pub fn bg() -> Rgba { rgb(0x1e1e2e) }
    pub fn surface() -> Rgba { rgb(0x313244) }
    pub fn text() -> Rgba { rgb(0xcdd6f4) }
    pub fn text_muted() -> Rgba { rgb(0x6c7086) }
    pub fn accent() -> Rgba { rgb(0x89b4fa) }
    pub fn secondary() -> Rgba { rgb(0xf38ba8) }
    pub fn border() -> Rgba { rgb(0x45475a) }
    pub fn urgent() -> Rgba { rgb(0xf38ba8) }
}
```

When building dynamic shells, `pal::...` functions read from an active theme state (or a theme entity registered in GPUI globals), allowing the entire shell to re-render instantly on theme changes (`cx.notify()`).

---

### 2. External Themes Specification: TOML Files

Themes are stored as `.toml` files in `~/.config/<shell-name>/themes/*.toml` (with built-in embedded strings as fallback if the directory is empty).

#### Standard Theme TOML Schema:
```toml
[meta]
name = "Catppuccin Mocha"
author = "Kumizo Community"
variant = "dark" # "dark" | "light"

# Mandatory: wallpaper is an integral part of the desktop identity!
[wallpaper]
path = "~/Pictures/wallpapers/catppuccin.png"
mode = "fill" # "fill" | "fit" | "stretch" | "center"

[colors]
bg = "#1e1e2e"
surface = "#313244"
text = "#cdd6f4"
text_muted = "#6c7086"
accent = "#89b4fa"
secondary = "#f38ba8"
border = "#45475a"
urgent = "#f38ba8"

[style]
border_width = 2.0
corner_radius = 8.0
shadow_size = 4.0
```

---

### 3. Automatic Desktop Wallpaper Synchronization

A Wayland shell and the desktop wallpaper form a single cohesive aesthetic. When a theme is loaded or switched:
1. The shell parses the `[wallpaper]` section of the theme TOML.
2. The shell automatically invokes the compositor's wallpaper command:
   - **Sway**: `swaymsg output * bg "<expanded_path>" fill`
   - **Hyprland / Generic Wayland**: `swww img "<expanded_path>"` or `wbg "<expanded_path>"`
3. The desktop background changes instantly in sync with the bar and widgets.

---

### 4. Mandatory Requirements for AI Agents & Developers

When an AI agent or developer creates a shell proposal, they **must** follow these two rules:

1. **Provide At Least 3 Curated TOML Presets**:
   Always ship at least 3 distinct built-in presets representing varied moods:
   - A dark/modern theme (e.g. `catppuccin-mocha.toml` or `tokyo-night.toml`)
   - A light/clean paper theme (e.g. `paper-light.toml` or `latte.toml`)
   - A bold/vibrant or retro theme (e.g. `riso-electric.toml` or `cyberpunk.toml`)

2. **Always Implement a `ThemeSwitcher` Widget**:
   Every shell must include an interactive `ThemeSwitcher` modal or popup window:
   - Scans the themes directory (or lists built-in presets).
   - Displays each theme with live visual color swatches and the wallpaper filename.
   - Interactive keyboard navigation (arrow keys + `Enter` to apply, `Esc` to close) and click support.
   - Toggled via a dedicated hotkey (e.g. `Super+t` / `kumizo-ctl toggle theme`) and a button in the bar/rail.
   - Applies the new palette **in-memory with zero process restart**, and updates the wallpaper immediately.

---

## 9. Complete Starter Template: Clean Shell

Below is a complete, minimal, working multi-window shell ready to be customized:

```rust
use std::time::Duration;
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .service::<Compositor>()
        .window(bar_window(), |cx| cx.new(StatusBar::new))
        .window(launcher_window(), |cx| cx.new(LauncherModal::new))
        .run();
}

fn bar_window() -> LayerWindow {
    LayerWindow::new("bar")
        .layer(Layer::Top)
        .anchor(Anchor::TOP_BAR)
        .height(36.0)
        .exclusive_zone(36)
        .namespace("wk-bar")
}

fn launcher_window() -> LayerWindow {
    LayerWindow::new("launcher")
        .layer(Layer::Overlay)
        .anchor(Anchor::FULLSCREEN)
        .exclusive_zone(-1)
        .keyboard_interactivity(KeyboardInteractivity::OnDemand)
        .start_hidden(true)
        .namespace("wk-launcher")
}

struct StatusBar {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    compositor: Entity<Compositor>,
}

impl StatusBar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            compositor: cx.service::<Compositor>(),
        }
    }
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let bat = self.battery.read(cx);
        let comp = self.compositor.read(cx);

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .size_full()
            .px_4()
            .bg(rgb(0x11111b))
            .text_color(rgb(0xcdd6f4))
            .text_size(px(13.0))
            // Left: Launcher Button & Workspaces
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("launcher-btn")
                            .px_2()
                            .py_1()
                            .bg(rgb(0x89b4fa))
                            .text_color(rgb(0x11111b))
                            .font_weight(gpui::FontWeight::BOLD)
                            .rounded_md()
                            .cursor_pointer()
                            .child("Apps")
                            .on_click(|_, _, cx| cx.toggle_window("launcher")),
                    )
                    .child(
                        div().flex().gap_1().children(
                            comp.workspaces().iter().map(|ws| {
                                let id = ws.id;
                                let is_active = ws.is_active;
                                div()
                                    .id(format!("ws-{}", id))
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .when(is_active, |s| s.bg(rgb(0x313244)).text_color(rgb(0x89b4fa)))
                                    .child(ws.name.clone())
                                    .cursor_pointer()
                                    .on_click(move |_, _, cx| {
                                        let comp = cx.service::<Compositor>();
                                        comp.update(cx, |c, _| c.switch_workspace(id));
                                    })
                            })
                        )
                    )
            )
            // Center: Active Window Title
            .child(
                div()
                    .min_w_0()
                    .max_w(px(300.0))
                    .truncate()
                    .child(comp.active_window().map(|w| w.title.as_str()).unwrap_or("Desktop"))
            )
            // Right: Battery & Clock
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(format!("{}%", bat.percent()))
                    .child(clock.time_hm())
            )
    }
}

struct LauncherModal;

impl LauncherModal {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for LauncherModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Veil covering the background
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000aa))
            .on_click(|_, _, cx| cx.hide_window("launcher"))
            .child(
                div()
                    .w(px(480.0))
                    .p_6()
                    .bg(rgb(0x1e1e2e))
                    .border_1()
                    .border_color(rgb(0x313244))
                    .rounded_xl()
                    .child("Your Custom App Launcher Here")
            )
    }
}
```

---

## 10. Design Explorations & Inspiration

You are encouraged to propose innovative, distinctive shell concepts:

1. **Riso Print / Neo-Brutalist (Reference)**:
   - High-contrast electric colors (fluo pink `#ff2a85`, electric blue `#0055ff`, sunflower yellow `#ffcc00`, ink `#111118`).
   - Square corners, 2px borders, zero-blur hard offset drop shadows (`shadow(hard(color, 4.0))`).
   - Misregistered offset print effects on active typography.
2. **Dynamic Island / Ambient Shell**:
   - No permanent edge bar. A single floating pill at the top that expands dynamically on events (incoming song change, notification arrival, volume change).
3. **Nordic Frosted Glass**:
   - Soft translucent background layers (`rgba(...)`), rounded corners, subtle separators, compositor background blur enabled via `.namespace("wk-island")`.
4. **Tiling Companion**:
   - Ultra-dense vertical strip or status line integrated seamlessly with window borders, displaying git branches, system temperatures, and task windows.

---

## 11. Verification & Quality Checklist

Before considering your shell complete:
- [ ] **Build Check**: `cargo check --package <your-shell> --offline` compiles with 0 errors and warnings.
- [ ] **Run & Socket Test**: `widgetctl ping` returns `PONG`.
- [ ] **Zero Idle CPU**: Check with `top` or `htop` — when undisturbed, your process stays at `0.0%` CPU.
- [ ] **Hotkeys**: Sway/Hyprland hotkeys can toggle your windows (`widgetctl toggle <id>`).
- [ ] **Text Boundaries**: All titles use `.truncate()` and `.min_w_0()`; no long URL or song title overflows off-screen.
- [ ] **Clean Hover**: No double `.hover()` calls on the same element.
