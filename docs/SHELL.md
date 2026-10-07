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

### Creative Mandate: Anti-Anchoring Directive (CRITICAL FOR AI AGENTS)

> [!IMPORTANT]
> **DO NOT DEFAULT TO A STANDARD 32px HORIZONTAL TOP-BAR.**
> Large Language Models naturally suffer from *anchoring bias* — instinctively outputting the stereotypical Linux bar pattern:
> `[Workspaces (left) | Window Title (center) | Clock & Battery (right)]`.
>
> **WidgetKit is not a clone of Waybar or Polybar.** It is a blank GPU canvas with Wayland multi-window composition. When designing a shell:
> - **Rethink Topologies**: Does the desktop need a persistent top bar? Or would it be superior with an edge-anchored vertical command rail, decoupled floating corner pods, an event-driven dynamic island, an ambient wallpaper layer dashboard, or zero permanent chrome with sliding gesture drawers?
> - **Rethink Visual Identities**: Avoid regurgitating ubiquitous community palettes (such as generic Catppuccin Mocha or Nord) unless specifically requested. Craft an intentional, bespoke visual universe with distinctive typography, geometry, contrast, and interactive feedback.
> - **Respect Display Ergonomics**: Ultra-wide displays, laptop screens, and multi-monitor setups thrive on non-traditional spatial layouts. Explore designs that maximize usable work area.

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

    // Semantic roles, resolved from active theme or runtime config:
    pub fn bg() -> Rgba { rgb(0x0f111a) }
    pub fn surface() -> Rgba { rgb(0x1a1d2e) }
    pub fn text() -> Rgba { rgb(0xf0f2f5) }
    pub fn text_muted() -> Rgba { rgb(0x5c6370) }
    pub fn accent() -> Rgba { rgb(0x00e8c6) }
    pub fn secondary() -> Rgba { rgb(0xff5370) }
    pub fn border() -> Rgba { rgb(0x282c3f) }
    pub fn urgent() -> Rgba { rgb(0xff5370) }
}
```

When building dynamic shells, `pal::...` functions read from an active theme state (or a theme entity registered in GPUI globals), allowing the entire shell to re-render instantly on theme changes (`cx.notify()`).

---

### 2. External Themes Specification: TOML Files

Themes are stored as `.toml` files in `~/.config/<shell-name>/themes/*.toml` (with built-in embedded strings as fallback if the directory is empty).

#### Standard Theme TOML Schema:
```toml
[meta]
name = "Bespoke Aesthetic Name"
author = "Designer Name"
variant = "dark" # "dark" | "light"

# Mandatory: wallpaper is an integral part of the desktop identity!
[wallpaper]
path = "~/Pictures/wallpapers/bespoke_universe.png"
mode = "fill" # "fill" | "fit" | "stretch" | "center"

[colors]
bg = "#0f111a"
surface = "#1a1d2e"
text = "#f0f2f5"
text_muted = "#5c6370"
accent = "#00e8c6"
secondary = "#ff5370"
border = "#282c3f"
urgent = "#ff5370"

[style]
border_width = 1.5
corner_radius = 6.0
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

1. **Invent At Least 3 Bespoke TOML Presets (Unique Aesthetic Universes)**:
   Do NOT copy standard community palettes (e.g., standard Catppuccin Mocha or Nord) unless explicitly requested. **Invent 3 distinct, thoughtfully curated aesthetic worlds** crafted specifically for the shell's philosophy:
   - **Universe A (Low-luminance / High-focus)**: E.g., deep obsidian with bioluminescent emerald accents or monochromatic carbon.
   - **Universe B (High-luminance / Editorial)**: E.g., warm architectural parchment, warm gray ink, crisp contrasting type.
   - **Universe C (Expressive / Thematic)**: E.g., risograph electric cyan & cadmium red, industrial caution amber & slate, or retro-futuristic CRT amber.
   Each preset must provide its own paired `[wallpaper]` path and layout parameters (`border_width`, `corner_radius`, etc.).

2. **Always Implement a `ThemeSwitcher` Widget**:
   Every shell must include an interactive `ThemeSwitcher` modal or popup window:
   - Scans the themes directory (or lists built-in presets).
   - Displays each theme with live visual color swatches and the wallpaper filename.
   - Interactive keyboard navigation (arrow keys + `Enter` to apply, `Esc` to close) and click support.
   - Toggled via a dedicated hotkey (e.g. `Super+t` / `kumizo-ctl toggle theme`) and a button in the bar/rail.
   - Applies the new palette **in-memory with zero process restart**, and updates the wallpaper immediately.

---

## 9. Mechanical Starter Skeleton (Structure & Wiring Reference)

> [!IMPORTANT]
> **Anti-Anchoring Notice**: This skeleton demonstrates **pure system wiring and architecture** (registering services, configuring layer-shell surfaces, querying service state, and triggering inter-window actions).
> **It intentionally avoids prescribing a visual layout.** Do NOT simply copy-paste a traditional 3-section horizontal bar unless specifically asked — design the spatial form factor that best fits your shell's purpose!

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

## 10. Architectural Topologies & Aesthetic Inspirations

WidgetKit is an unconstrained engine. You are encouraged to propose innovative, distinctive shell concepts across diverse topologies and aesthetic movements:

### 1. Architectural Topologies (Beyond the Traditional Bar)

1. **The Dynamic Island / Ambient Satellite**:
   - **Layer**: `Top` or `Overlay`, anchored `TOP`.
   - **Behavior**: A compact floating capsule (e.g. 240px wide) at the top of the screen that dynamically expands horizontally or drops down cards when events happen (track change, incoming notification, volume adjustment), retracting back to an ambient indicator.
2. **The Vertical Command Rail (Optimized for 16:9 / 21:9 Screens)**:
   - **Layer**: `Top`, anchored `LEFT` or `RIGHT`.
   - **Behavior**: Takes advantage of modern horizontal screen real estate. Thin, high-density utility strip with vertical icon clusters, rotated text or glyphs, and bottom-aligned power & control triggers.
3. **The Corner HUD Pods (Decoupled Multi-Window Architecture)**:
   - **Layer**: Multiple independent `LayerWindow` surfaces.
   - **Behavior**: Instead of one monolithic edge-to-edge bar, declare 2 or 3 tiny floating pods: Pod A at `TOP | LEFT` for workspaces & media, Pod B at `TOP | RIGHT` for telemetry & time, Pod C at `BOTTOM | LEFT` for a quick launcher. Zero reserved exclusive zone between pods leaves the screen uncluttered.
4. **The Ambient Desktop Underlay (Living Wallpaper Layer)**:
   - **Layer**: `Background` or `Bottom`.
   - **Behavior**: Positioned underneath application windows. Displays living system telemetry graphs, large typographic clocks, and calendar agendas directly on the desktop background, visible when workspaces are clear or tiled with margins.
5. **The Zero-Chrome Sliding Command Drawer**:
   - **Layer**: `Overlay`, initially hidden (`.start_hidden(true)`).
   - **Behavior**: No permanent screen chrome reserved. The entire interface reveals itself smoothly from screen edges on hotkey (`Super+Space`), touchpad gesture, or screen edge hover.

### 2. Aesthetic Movements & Visual Identities

- **Neo-Brutalist & Risograph Print**:
  Raw ink outlines (2px borders), zero-blur hard offset drop shadows (`HardShadow`), high-saturation spot inks (cadmium red, electric cyan, sunflower yellow, warm off-white newsprint), bold blocky typography.
- **Industrial Cyberdeck / Mil-Spec Terminal**:
  Monospace data density, tabular telemetry, status tags, caution stripes, high-contrast amber or phosphorus green telemetry with zero decorative fluff.
- **Nordic Editorial & Swiss Modernism**:
  Generous negative space, elegant serif or crisp neo-grotesque type, warm architectural paper backgrounds, subtle hairline dividers, literary poise.
- **Aerospace HUD / Minimalist Wireframe**:
  Monochromatic wireframes, hairline gauges, circular progress rings, clean geometric glyphs, translucent dark backdrops with subtle borders.

---

## 11. Verification & Quality Checklist

Before considering your shell complete:
- [ ] **Build Check**: `cargo check --package <your-shell> --offline` compiles with 0 errors and warnings.
- [ ] **Run & Socket Test**: `widgetctl ping` returns `PONG`.
- [ ] **Zero Idle CPU**: Check with `top` or `htop` — when undisturbed, your process stays at `0.0%` CPU.
- [ ] **Hotkeys**: Sway/Hyprland hotkeys can toggle your windows (`widgetctl toggle <id>`).
- [ ] **Text Boundaries**: All titles use `.truncate()` and `.min_w_0()`; no long URL or song title overflows off-screen.
- [ ] **Clean Hover**: No double `.hover()` calls on the same element.
