# Kumizo Guide

Build your own Wayland desktop shell with Kumizo — one process, GPU-accelerated (GPUI/wgpu), event-driven, zero idle CPU.

> **Two documents, two usages**
> - This guide explains **why** and **how** to build with Kumizo (tutorial + explanation).
> - For **exact signatures** of types and methods, see [`API.md`](./API.md). This guide intentionally does not include an exhaustive reference, to stay readable.

---

## 📖 Table of contents

1. [Introduction: why Kumizo?](#1-introduction-why-kumizo)
2. [Prerequisites](#2-prerequisites)
3. [The three fundamental concepts](#3-the-three-fundamental-concepts)
4. [Recipe: build your first widget](#4-recipe-build-your-first-widget)
5. [Services reference](#5-services-reference)
6. [GPUI pitfalls and method rules](#6-gpui-pitfalls-and-method-rules)
7. [Recommended architecture](#7-recommended-architecture)
8. [Final checklist](#8-final-checklist)

---

## 1. Introduction: why Kumizo? <a name="1-introduction-why-kumizo"></a>

Kumizo is **blazingly fast**, **lightweight**, and **feature rich**. It is a single-process, GPU-accelerated (GPUI/wgpu), event-driven SDK for building your own Wayland desktop shell — status bars, docks, launchers, OSDs, popups, panels — with **zero idle CPU**.

When it comes to design, you are limited only by your imagination: every widget is a plain Rust `struct` implementing `gpui::Render`, giving you total layout freedom with no imposed UI kit.

> 💡 **In short**: Kumizo is an SDK for **desktop artisans**. It does not give you a ready-made bar; it gives you the tools to build your own, without compromising on performance.
>
> 📘 **Note on language**: this guide is in English. The API reference [`API.md`](./API.md) is also in English, as it mirrors `rustdoc` / `docs.rs` signatures.

---

## 2. Prerequisites <a name="2-prerequisites"></a>

Before diving in, be comfortable with:

- **Rust**: traits, lifetimes, `Rc`/`Arc`, closures. GPUI code uses explicit lifetimes heavily.
- **GPUI basics**: `Entity<T>`, `Context<T>`, `Render`, `View`. If these terms are unfamiliar, read the GPUI documentation first.
- **Wayland basics**: `layer-shell` (surfaces placed above/below normal windows) and D-Bus (system service bus).

You do **not** need to know low-level Wayland details: Kumizo encapsulates them.

---

## 3. The three fundamental concepts <a name="3-the-three-fundamental-concepts"></a>

Everything in Kumizo rests on three bricks. Understanding their role and boundaries is **the key** to writing sound code.

### 🪟 3.1 The window (`LayerWindow`)

A **window** is a Wayland `LayerWindow` surface declared once in `main()`. It is the physical container of your widgets.

```rust
// Declaration typically in main()
let kumizo = Kumizo::new()
    .window(
        LayerWindow::new("bar")
            .layer(Layer::Top)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .height(36.0)
            .exclusive_zone(36),
        |cx: &mut gpui::App| cx.new(MyBar::new),
    );
```

**Rules to remember:**

- One window = one Wayland surface. Declare as many as needed.
- Position, size, layer (Top/Bottom/Overlay) and margins are defined **here**, not in widgets.
- The **lockscreen is deliberately excluded** from this model: it is a separate binary for security reasons (passwords, exclusive keyboard).

### 🎨 3.2 The view

A **view** is a Rust `struct` implementing `gpui::Render`. It is the "painter" of your widget.

```rust
struct ClockView {
    now: String,  // pure state, injected by a service
}

impl Render for ClockView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .child(self.now.clone())
    }
}
```

**Golden rule: a view is a pure function of its state.**

Inside `render()`, you must **never**:
- ❌ Read a file
- ❌ Call D-Bus
- ❌ Block on I/O

You must **only**:
- ✅ Read the struct fields
- ✅ Build the GPUI element tree
- ✅ Use data provided by services

> 💡 **Analogy**: think of `render()` as a mathematical function `f(state) → pixels`. If the state does not change, the result must be identical. This guarantees performance and predictability.

### 🔌 3.3 The service

A **service** is a reactive data source. It is **the only way** to trigger a redraw.

```rust
// Conceptual example: a clock service
struct ClockService {
    now: String,
}

impl ClockService {
    fn tick(&mut self, cx: &mut Context<Self>) {
        self.now = format_current_time();
        cx.notify();  // ← triggers redraw of subscribed views
    }
}
```

**Characteristics:**

- **Event-driven**: a service emits only when it has something to say.
- **Decoupled**: it does not know the views that listen to it.
- **Reusable**: one service (Clock, Battery, Audio) can serve multiple views and projects.

---

## 4. Recipe: build your first widget <a name="4-recipe-build-your-first-widget"></a>

Here is the full path, step by step, to add a "Battery" widget to your shell.

### Step 1 — Declare the window

In `main()`, add (if not already done) a window for the bar:

```rust
.window(
    LayerWindow::new("bar")
        .layer(Layer::Top)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .height(32.0)
        .exclusive_zone(32),
    |cx: &mut gpui::App| cx.new(MyBar::new),
)
```

### Step 2 — Choose (or write) the service

Kumizo provides ready-to-use system services (see [API.md](./API.md)), including `Battery`.
If you need a **custom** service — say, reading an exotic sensor — here is the minimal shape
every service must have:

```rust
struct BatteryService {
    level: u8,
    charging: bool,
}

impl BatteryService {
    fn on_update(&mut self, level: u8, charging: bool, cx: &mut Context<Self>) {
        self.level = level;
        self.charging = charging;
        cx.notify();
    }
}
```

### Step 3 — Create the view

```rust
struct BatteryView {
    battery: Entity<Battery>,
}

impl BatteryView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { battery: cx.service::<Battery>() }
    }
}

impl Render for BatteryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let level = self.battery.read(cx).percent();
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(format!("{}%", level))
            .child(if self.battery.read(cx).is_charging() { "⚡" } else { "" })
    }
}
```

### Step 4 — Wire view ↔ service

In `main()`, subscribe the view to the service:

```rust
fn main() {
    Kumizo::new()
        .service::<Battery>()
        .window(
            LayerWindow::new("bar")
                .layer(Layer::Top)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .height(32.0),
            |cx: &mut gpui::App| cx.new(BatteryView::new),
        )
        .run();
}
```

At each `cx.notify()`, GPUI re-invokes `render()` on the persisted view — the view is not recreated.

### Step 5 — Verify the result

```bash
cargo run --example aurora  # or your binary
```

You should see the battery display and update in real time. If not, consult [GPUI pitfalls](#6-gpui-pitfalls-and-method-rules).

---

## 5. Services reference <a name="5-services-reference"></a>

Kumizo provides ready-to-use system services (see [`API.md`](./API.md)), including `Clock`,
`Battery`, `Audio`, `Memory`, `Network`, `Bluetooth`, `Compositor`, `Apps`, `Notifications`,
`Tray`, `Clipboard`, `Mpris`, `Brightness`.

| Service | Data exposed | Typical events |
|---|---|---|
| **Clock** | Time, date, timezone | Tick every second |
| **Battery** | Level %, charging state, time remaining | State change, plug/unplug |
| **Audio** | Volume, mute, active sink | Volume change, sink change |
| **Network** | SSID, IP, connection state | Connect/disconnect, IP change |
| **Bluetooth** | Connected devices, state | Pairing, disconnection |
| **Compositor** | Workspaces, active window | Workspace change, focus change |
| **SystemTray** | Icons and menus of applications | Icon add/remove |

**Typical usage:**

```rust
// Subscribe to a service
let audio = cx.service::<Audio>();

// In a view, read state without blocking
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let audio = self.audio.read(cx);
    div().child(format!("🔊 {}%", audio.volume()))
}
```

> ⚠️ **Important**: services are **event-driven**. Do not poll them in a loop. Subscribe to their notifications and let `cx.notify()` trigger redraws.

---

## 6. GPUI pitfalls and method rules <a name="6-gpui-pitfalls-and-method-rules"></a>

This section condenses production pitfalls. Read it **before** debugging.

### 🐛 Pitfall 1: `primary_display()` returns `None` on Wayland

**Symptom**: your window never appears, or a startup `panic`.
**Cause**: on Wayland, "primary display" does not exist reliably.
**Solution**: never depend on `primary_display()`. Use dimensions provided by `LayerWindow`.

### 🐛 Pitfall 2: lifetimes and `Context`

**Symptom**: compile errors `cannot borrow ... as mutable more than once`.
**Cause**: `render()` takes `&mut self` and `&mut Context<Self>` simultaneously.
**Solution**: clone data you need **before** calling methods that borrow `cx`. Structure code into short steps.

### 🐛 Pitfall 3: system calls in `render()`

**Symptom**: CPU at 100%, UI freezing.
**Cause**: file read or D-Bus call inside `render()`.
**Solution**: move all I/O into a service. `render()` must be **pure**.

### 🐛 Pitfall 4: forgetting `cx.notify()`

**Symptom**: data changed in the service, but UI does not update.
**Cause**: service updated state without notifying.
**Solution**: call `cx.notify()` systematically after a state change.

### 🐛 Pitfall 5: redraw loops

**Symptom**: high CPU at rest, stuttering animations.
**Cause**: a `render()` that indirectly triggers `notify()`.
**Solution**: verify that `render()` does not modify **any** observable state.

### ✅ Method rules to engrave

1. **`render()` is pure.** No I/O, no system calls.
2. **Services are event-driven.** No polling.
3. **One view = one state.** If you need multiple sources, compose views.
4. **Clone early, borrow late.** Avoid borrow conflicts in `render()`.
5. **Test at idle.** A healthy shell runs at ~0% CPU when nothing changes.

---

## 7. Recommended architecture <a name="7-recommended-architecture"></a>

Kumizo enforces a strict architecture (called **"Architecture B"**) for maintainability:

```
┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
│    Services     │─────▶│      Views      │─────▶│    Windows      │
│  (pure data)    │      │  (GPUI render)  │      │  (Wayland)      │
└─────────────────┘      └─────────────────┘      └─────────────────┘
        ▲                                                  │
        │                                                  │
        └────────── system events ◀────────────────────────┘
```

**Principles:**

- **Services** hold pure data. They do not know about views.
- **Views** consume services and produce GPUI elements.
- **Windows** are declared once in `main()`.
- The **lockscreen** is a **separate binary**: it shares neither memory nor service registry with the shell, for security reasons (passwords, exclusive keyboard).

**Why this separation?**

- A service can be tested without UI.
- A view can be previewed without service (with fake data).
- A window can be reconfigured without touching widget code.

---

## 8. Final checklist <a name="8-final-checklist"></a>

Before considering your widget "done":

- [ ] The view has **no** I/O in `render()`.
- [ ] The service calls `cx.notify()` at each state change.
- [ ] The shell runs at **~0% CPU at idle** (check with `htop`).
- [ ] Lifetimes are handled without unnecessary `clone()` in `render()`.
- [ ] No dependency on `primary_display()`.
- [ ] The widget works after workspace/theme changes.
- [ ] Code respects the separation services / views / windows.

---

## 🎓 To go further

- **Read `examples/aurora`**: the complete demonstration shell, used in production. Each widget there is a concrete example.
- **Explore services**: start with `Clock` (simplest), then `Battery`, then `Audio`.
- **Contribute**: the project is young; documentation feedback is valuable.

> 💡 **Last advice**: do not try to understand everything at once. Build a trivial widget (a clock), make it work, then complexify. Kumizo rewards iteration.

*This guide is based on the real Kumizo codebase. For exact signatures, up-to-date method names and implementation details, consult [`docs/API.md`](./API.md) and read the code of `examples/aurora`, the best living documentation of the SDK.*
