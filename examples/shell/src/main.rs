//! A single-process Wayland shell hosting the topbar, the dock and the OSD.
//!
//! Running the three widgets as three separate binaries works, but each one
//! pays the full cost of initialising the GPU stack, the font stack and the
//! compositor connection. In a single process that cost is paid once, and the
//! services (`Clock`, `Audio`, `Compositor`, ...) are instantiated once and
//! shared by every window, which also means one wakeup per second instead of one
//! per binary.
//!
//! The trade-off is the loss of isolation: a panic in any window takes down all
//! of them. That is the same trade-off Waybar and AGS make, and it is the right
//! one for a desktop shell.
//!
//! Run it with:
//!
//! ```sh
//! cargo run --package shell-example
//! ```

use std::time::Duration;

use gpui::prelude::*;
use widgetkit::prelude::*;

/// Height of the top bar, in logical pixels. Also used as its exclusive zone so
/// regular windows are pushed below it.
const TOPBAR_HEIGHT: f32 = 36.0;
/// Height of the dock.
const DOCK_HEIGHT: f32 = 58.0;
/// Size of the volume/brightness OSD.
const OSD_WIDTH: f32 = 280.0;
const OSD_HEIGHT: f32 = 80.0;

fn main() {
    WidgetKit::new()
        // Services are declared once and shared by all three windows: the clock
        // ticks once per second for the whole shell, not once per widget.
        .service::<Clock>()
        .service::<Battery>()
        .service::<Audio>()
        .service::<Mpris>()
        .service::<Compositor>()
        .window(topbar_window(), |cx: &mut gpui::App| {
            cx.new(TopBar::new)
        })
        .window(dock_window(), |cx: &mut gpui::App| cx.new(Dock::new))
        .window(osd_window(), |cx: &mut gpui::App| cx.new(VolumeOsd::new))
        .run();
}

fn topbar_window() -> LayerWindow {
    LayerWindow::new("topbar")
        .layer(Layer::Top)
        .anchor(Anchor::TOP_BAR)
        // Reserving the bar's height keeps maximised windows from overlapping it.
        .exclusive_zone(TOPBAR_HEIGHT as i32)
        .height(TOPBAR_HEIGHT)
        .margins(Margins::new(6, 12, 0, 12))
        .namespace("wk-topbar")
}

fn dock_window() -> LayerWindow {
    LayerWindow::new("dock")
        .layer(Layer::Top)
        .anchor(Anchor::BOTTOM)
        .exclusive_zone(DOCK_HEIGHT as i32)
        .width(480.0)
        .height(DOCK_HEIGHT)
        .margins(Margins::new(0, 0, 10, 0))
        .namespace("wk-dock")
}

fn osd_window() -> LayerWindow {
    LayerWindow::new("osd")
        .layer(Layer::Overlay)
        .anchor(Anchor::BOTTOM)
        // No exclusive zone: the OSD must never reserve screen space.
        .exclusive_zone(0)
        .width(OSD_WIDTH)
        .height(OSD_HEIGHT)
        .margins(Margins::new(0, 0, 100, 0))
        .namespace("wk-osd")
}

// ---------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------

struct TopBar {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    audio: Entity<Audio>,
    mpris: Entity<Mpris>,
    compositor: Entity<Compositor>,
}

impl TopBar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            audio: cx.service::<Audio>(),
            mpris: cx.service::<Mpris>(),
            compositor: cx.service::<Compositor>(),
        }
    }
}

impl Render for TopBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clock = self.clock.read(cx);
        let battery = self.battery.read(cx);
        let audio = self.audio.read(cx);
        let mpris = self.mpris.read(cx);
        let comp = self.compositor.read(cx);

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(TOPBAR_HEIGHT))
            .bg(rgba(0x181825ee))
            .border_1()
            .border_color(rgba(0x31324488))
            .rounded_lg()
            .px_3()
            .text_size(px(13.0))
            .text_color(rgb(0xcdd6f4))
            // Workspaces, from the live compositor connection.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(rgb(0x1e1e2e))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .children(comp.workspaces().iter().map(|ws| {
                        let is_active = ws.is_active;
                        let ws_id = ws.id;
                        let name = ws.name.clone();
                        let compositor = self.compositor.clone();

                        div()
                            .id(format!("workspace-{}", ws_id))
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .cursor_pointer()
                            .when(is_active, |s| {
                                s.bg(rgb(0x89b4fa))
                                    .text_color(rgb(0x11111b))
                                    .font_weight(gpui::FontWeight::BOLD)
                            })
                            .when(!is_active, |s| s.hover(|h| h.bg(rgb(0x313244))))
                            .child(name)
                            .on_click(move |_, _window, cx| {
                                compositor.update(cx, |c, _| c.switch_workspace(ws_id));
                            })
                    })),
            )
            // Date and time.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(0x1e1e2e))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .child(div().text_color(rgb(0xb4befe)).child(clock.date_formatted()))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0xcdd6f4))
                            .child(clock.time_hms()),
                    ),
            )
            // MPRIS, volume and battery.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(mpris.title().is_some(), |this| {
                        let mpris_entity = self.mpris.clone();
                        let status_icon = mpris.status().icon();
                        let label = mpris.display_label();

                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .bg(rgb(0x1e1e2e))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .child(div().text_color(rgb(0xf5c2e7)).child("\u{f00e}"))
                                .child(div().text_color(rgb(0xcdd6f4)).child(label))
                                .child(
                                    div()
                                        .id("wk-mpris-toggle")
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(0x89b4fa)))
                                        .child(status_icon)
                                        .on_click(move |_, _window, cx| {
                                            mpris_entity.read(cx).play_pause();
                                        }),
                                ),
                        )
                    })
                    .child({
                        let audio_entity = self.audio.clone();
                        let icon = audio.icon();
                        let volume = audio.volume();
                        let muted = audio.is_muted();

                        div()
                            .id("wk-audio-toggle")
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .cursor_pointer()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .child(
                                div()
                                    .text_color(if muted {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0x89b4fa)
                                    })
                                    .child(icon),
                            )
                            .child(format!("{}%", volume))
                            .on_click(move |_, _window, cx| {
                                audio_entity.update(cx, |a, _| a.toggle_mute());
                            })
                    })
                    .child({
                        let percent = battery.percent();
                        let charging = battery.is_charging();

                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(0x1e1e2e))
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .child(
                                div()
                                    .text_color(if charging {
                                        rgb(0xa6e3a1)
                                    } else if percent <= 20 {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0xf9e2af)
                                    })
                                    .child(battery.state().icon(percent)),
                            )
                            .child(format!("{}%", percent))
                    }),
            )
    }
}

// ---------------------------------------------------------------------------
// Dock
// ---------------------------------------------------------------------------

struct DockItem {
    icon: &'static str,
    command: &'static str,
    /// Lowercase app_id/class fragment used to detect a running indicator.
    app_class: &'static str,
}

struct Dock {
    items: Vec<DockItem>,
    compositor: Entity<Compositor>,
}

impl Dock {
    fn new(cx: &mut Context<Self>) -> Self {
        let items = vec![
            DockItem { icon: "\u{f1209}", command: "ghostty", app_class: "ghostty" },
            DockItem { icon: "\u{f0219}", command: "firefox", app_class: "firefox" },
            DockItem { icon: "\u{f129e}", command: "zed", app_class: "dev.zed.zed" },
            DockItem { icon: "\u{f024b}", command: "nautilus", app_class: "org.gnome.nautilus" },
            DockItem { icon: "\u{f0243}", command: "spotify", app_class: "spotify" },
            DockItem { icon: "\u{f0266}", command: "discord", app_class: "discord" },
        ];

        Self {
            items,
            compositor: cx.service::<Compositor>(),
        }
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_class = self
            .compositor
            .read(cx)
            .active_window()
            .map(|w| w.class.to_lowercase())
            .unwrap_or_default();

        div()
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(rgba(0x181825ee))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .shadow_lg()
                    .children(self.items.iter().map(|item| {
                        let running = !active_class.is_empty()
                            && active_class.contains(&item.app_class);
                        let command = item.command;

                        div()
                            .id(format!("dock-{}", command))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .w(px(44.0))
                            .h(px(44.0))
                            .rounded_xl()
                            .cursor_pointer()
                            .bg(rgb(0x1e1e2e))
                            .hover(|s| s.bg(rgb(0x313244)))
                            .child(
                                div()
                                    .text_size(px(22.0))
                                    .text_color(rgb(0xcdd6f4))
                                    .child(item.icon),
                            )
                            // Running indicator dot.
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(if running {
                                        rgb(0x89b4fa)
                                    } else {
                                        rgba(0x00000000)
                                    }),
                            )
                            .on_click(move |_, _window, _cx| {
                                let _ = std::process::Command::new(command).spawn();
                            })
                    })),
            )
    }
}

// ---------------------------------------------------------------------------
// Volume / brightness OSD
// ---------------------------------------------------------------------------

struct VolumeOsd {
    audio: Entity<Audio>,
    visible: bool,
    last_volume: u8,
}

impl VolumeOsd {
    fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let last_volume = audio.read(cx).volume();

        let osd = Self {
            audio,
            visible: false,
            last_volume,
        };

        // React to volume changes coming from the shared Audio service: each
        // change reveals the OSD and schedules its auto-hide.
        let watched = osd.audio.clone();
        cx.observe(&watched, |this, audio, cx| {
            let volume = audio.read(cx).volume();
            if volume == this.last_volume {
                return;
            }
            this.last_volume = volume;
            this.visible = true;
            cx.notify();

            cx.spawn(async move |this, cx| {
                async_io::Timer::after(Duration::from_millis(1800)).await;
                let _ = this.update(cx, |osd, cx| {
                    osd.visible = false;
                    cx.notify();
                });
            })
            .detach();
        })
        .detach();

        osd
    }
}

impl Render for VolumeOsd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            // Nothing to show: stay transparent instead of burning frames.
            return div().w_full().h_full();
        }

        let audio = self.audio.read(cx);
        let volume = audio.volume();
        let muted = audio.is_muted();

        div()
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .w(px(OSD_WIDTH - 20.0))
                    .p_3()
                    .bg(rgba(0x181825f2))
                    .border_1()
                    .border_color(rgba(0x45475a88))
                    .rounded_2xl()
                    .shadow_xl()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .w_full()
                            .px_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_color(if muted {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0x89b4fa)
                                    })
                                    .text_size(px(18.0))
                                    .child(audio.icon())
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xcdd6f4))
                                            .child(if muted { "Muted" } else { "Volume" }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(format!("{}%", volume)),
                            ),
                    )
                    // Progress bar.
                    .child(
                        div()
                            .w_full()
                            .h(px(8.0))
                            .bg(rgb(0x313244))
                            .rounded_full()
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::DefiniteLength::Fraction(volume as f32 / 100.0))
                                    .bg(if muted {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0x89b4fa)
                                    })
                                    .rounded_full(),
                            ),
                    ),
            )
    }
}