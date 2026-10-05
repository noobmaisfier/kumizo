use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Clock>()
        .service::<Battery>()
        .service::<Network>()
        .service::<Audio>()
        .service::<Mpris>()
        .service::<Compositor>()
        .window(
            LayerWindow::new("topbar")
                .layer(Layer::Top)
                .anchor(Anchor::TOP_BAR)
                .exclusive_zone(36)
                .height(36.0)
                .margins(Margins::new(6, 12, 0, 12))
                .namespace("wk-topbar"),
            |cx: &mut gpui::App| cx.new(TopBar::new),
        )
        .run();
}

struct TopBar {
    clock: Entity<Clock>,
    battery: Entity<Battery>,
    network: Entity<Network>,
    audio: Entity<Audio>,
    mpris: Entity<Mpris>,
    compositor: Entity<Compositor>,
}

impl TopBar {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            clock: cx.service::<Clock>(),
            battery: cx.service::<Battery>(),
            network: cx.service::<Network>(),
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
        let network = self.network.read(cx);
        let audio = self.audio.read(cx);
        let mpris = self.mpris.read(cx);
        let comp = self.compositor.read(cx);

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(36.0))
            // Catppuccin Mocha Crust / Mantle with subtle transparency
            .bg(rgba(0x181825ee))
            .border_1()
            .border_color(rgba(0x31324488))
            .rounded_lg()
            .px_3()
            .text_size(px(13.0))
            .text_color(rgb(0xcdd6f4))
            // Left Section: Workspaces & Active Window Title
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        // Workspaces pill container
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
                                let comp_entity = self.compositor.clone();

                                div()
                                    .id(format!("workspace-{}", ws_id))
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .when(is_active, |s| {
                                        s.bg(rgb(0x89b4fa)).text_color(rgb(0x11111b)).font_weight(gpui::FontWeight::BOLD)
                                    })
                                    .when(!is_active, |s| {
                                        s.hover(|h| h.bg(rgb(0x313244)))
                                    })
                                    .child(ws.name.clone())
                                    .on_click(move |_, _window, cx| {
                                        comp_entity.update(cx, |c, _| {
                                            c.switch_workspace(ws_id);
                                        });
                                    })
                            })),
                    )
                    // Active Window Title
                    .when_some(comp.active_window(), |this, win| {
                        let truncated = if win.title.len() > 40 {
                            format!("{}...", &win.title[..37])
                        } else {
                            win.title.clone()
                        };
                        this.child(
                            div()
                                .text_color(rgb(0xa6adc8))
                                .px_2()
                                .child(format!("• {}", truncated)),
                        )
                    }),
            )
            // Center Section: Date & Time Clock
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
            // Right Section: MPRIS, Audio, Battery
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // MPRIS Media Player
                    .when(mpris.title().is_some(), |this| {
                        let mpris_entity = self.mpris.clone();
                        // Each `on_click` closure takes ownership, so the "next track"
                        // button needs its own handle.
                        let mpris_next_entity = mpris_entity.clone();
                        let status_icon = mpris.status().icon();
                        let display_label = mpris.display_label();
                        let truncated = if display_label.len() > 30 {
                            format!("{}...", &display_label[..27])
                        } else {
                            display_label
                        };

                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .bg(rgb(0x1e1e2e))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .child(div().text_color(rgb(0xf5c2e7)).child("󰎆"))
                                .child(div().text_color(rgb(0xcdd6f4)).child(truncated))
                                .child(
                                    div()
                                        .id("wk-mpris-toggle")
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(0x89b4fa)))
                                        .child(status_icon)
                                        .on_click(move |_, _window, cx| {
                                            mpris_entity.read(cx).play_pause();
                                        }),
                                )
                                .child(
                                    div()
                                        .id("wk-mpris-next")
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(0x89b4fa)))
                                        .child("󰒭")
                                        .on_click(move |_, _window, cx| {
                                            mpris_next_entity.read(cx).next();
                                        }),
                                ),
                        )
                    })
                    // Audio Volume & Mute Toggle
                    .child({
                        let audio_entity = self.audio.clone();
                        let icon = audio.icon();
                        let vol = audio.volume();
                        let is_muted = audio.is_muted();

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
                                    .text_color(if is_muted { rgb(0xf38ba8) } else { rgb(0x89b4fa) })
                                    .child(icon),
                            )
                            .child(format!("{}%", vol))
                            .on_click(move |_, _window, cx| {
                                audio_entity.update(cx, |a, _| {
                                    a.toggle_mute();
                                });
                            })
                    })
                    // Wi-Fi Indicator
                    .child({
                        let connected = network.is_connected();

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
                                    .text_color(if connected {
                                        rgb(0x89b4fa)
                                    } else {
                                        rgb(0xf38ba8)
                                    })
                                    .child(network.icon()),
                            )
                            .child(network.display_label())
                    })
                    // Battery Indicator
                    .child({
                        let pct = battery.percent();
                        let icon = battery.state().icon(pct);
                        let is_charging = battery.is_charging();

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
                                    .text_color(if is_charging {
                                        rgb(0xa6e3a1)
                                    } else if pct <= 20 {
                                        rgb(0xf38ba8)
                                    } else {
                                        rgb(0xf9e2af)
                                    })
                                    .child(icon),
                            )
                            .child(format!("{}%", pct))
                    }),
            )
    }
}
