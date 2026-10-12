use std::time::Duration;

use gpui::prelude::*;
use gpui::{Animation, AnimationExt, Context, Div, Entity, FocusHandle, KeyDownEvent, px, Window};
use kumizo_kit::prelude::*;

use crate::theme::{hard, out_back, pal, POPUP_TOP};

pub const BLUETOOTH_WIDTH: f32 = 360.0;
pub const BLUETOOTH_HEIGHT: f32 = 490.0;

pub fn window() -> LayerWindow {
    LayerWindow::new("bluetooth")
        .layer(Layer::Overlay)
        .anchor(Anchor::TOP | Anchor::LEFT)
        .exclusive_zone(0)
        .width(BLUETOOTH_WIDTH)
        .height(BLUETOOTH_HEIGHT)
        .margins(Margins::new(POPUP_TOP, 0, 0, 80))
        .keyboard_interactivity(KeyboardInteractivity::Exclusive)
        .namespace("wk-bluetooth")
        .start_hidden(true)
}

pub struct BluetoothMenu {
    bluetooth: Entity<Bluetooth>,
    focus: FocusHandle,
}

impl BluetoothMenu {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            bluetooth: cx.service::<Bluetooth>(),
            focus: cx.focus_handle(),
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.hide_window("bluetooth");
        }
    }
}

impl Render for BluetoothMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bt = self.bluetooth.read(cx);
        let powered = bt.is_powered();
        let discovering = bt.is_discovering();
        let bt_entity = self.bluetooth.clone();
        let devices = bt.devices().to_vec();

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .bg(pal::white())
            .border_b_2()
            .border_color(pal::ink())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(16.0))
                            .text_color(if powered { pal::blue() } else { pal::gray() })
                            .child("\u{f294}"), // 
                    )
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .child("BLUETOOTH"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // Scan / Refresh Button
                    .when(powered, |p| {
                        let bt_e = bt_entity.clone();
                        p.child(
                            div()
                                .id("wk-bt-scan-btn")
                                .flex()
                                .items_center()
                                .justify_center()
                                .px_2()
                                .py_0p5()
                                .border_2()
                                .border_color(pal::ink())
                                .bg(if discovering { pal::peach() } else { pal::paper() })
                                .hover(|s| s.bg(pal::yellow()))
                                .cursor_pointer()
                                .text_size(px(11.0))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(if discovering { "Scanning…" } else { "Scan" })
                                .on_click(move |_, _w, cx| {
                                    bt_e.update(cx, |b, _| b.toggle_scan());
                                }),
                        )
                    })
                    // Power Switch Toggle
                    .child({
                        let bt_e = bt_entity.clone();
                        div()
                            .id("wk-bt-power-toggle")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(52.0))
                            .h(px(24.0))
                            .border_2()
                            .border_color(pal::ink())
                            .bg(if powered { pal::blue() } else { pal::paper() })
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(if powered { pal::paper() } else { pal::ink() })
                            .child(if powered { "ON" } else { "OFF" })
                            .on_click(move |_, _w, cx| {
                                bt_e.update(cx, |b, _| b.toggle_power());
                            })
                    }),
            );

        // Body with device list
        let content: Div = if !powered {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .h(px(320.0))
                .gap_2()
                .child(
                    div()
                        .text_size(px(36.0))
                        .text_color(pal::gray())
                        .child("\u{f294}"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal::gray())
                        .child("Bluetooth is turned off"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(pal::gray())
                        .child("Turn ON to discover & pair devices"),
                )
        } else if devices.is_empty() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .h(px(320.0))
                .gap_2()
                .child(
                    div()
                        .text_size(px(32.0))
                        .text_color(pal::peach())
                        .child("\u{f002}"), // Search icon
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(pal::ink())
                        .child("No devices found"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(pal::gray())
                        .child("Click 'Scan' to discover nearby devices"),
                )
        } else {
            div()
                .flex()
                .flex_col()
                .p_2()
                .gap_1p5()
                .overflow_hidden()
                .children(devices.into_iter().map(|dev| {
                    let bt_e = bt_entity.clone();
                    let path = dev.path.clone();
                    let connected = dev.connected;
                    let paired = dev.paired;
                    let display_name = dev.display_name().to_string();
                    let type_icon = dev.type_icon();
                    let battery = dev.battery_percent;

                    div()
                        .id(format!("bt-dev-{}", dev.address.replace(':', "-")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .p_2()
                        .bg(if connected { pal::white() } else { pal::paper() })
                        .border_2()
                        .border_color(pal::ink())
                        .hover(|s| s.bg(pal::tint()))
                        // Left: Icon + Name + Battery / Status
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2p5()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(28.0))
                                        .h(px(28.0))
                                        .bg(if connected { pal::blue() } else { pal::white() })
                                        .border_1()
                                        .border_color(pal::ink())
                                        .text_color(if connected { pal::paper() } else { pal::ink() })
                                        .text_size(px(14.0))
                                        .child(type_icon),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(pal::ink())
                                                .child(display_name),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .text_size(px(10.0))
                                                .text_color(if connected { pal::green() } else { pal::gray() })
                                                .child(if connected { "Connected" } else if paired { "Paired" } else { "Available" })
                                                .when_some(battery, |b, pct| {
                                                    b.child(format!(" • 󰁹 {pct}%"))
                                                }),
                                        ),
                                ),
                        )
                        // Right: Connect / Disconnect action button
                        .child(
                            div()
                                .id(format!("bt-action-{}", dev.address.replace(':', "-")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .px_2p5()
                                .py_1()
                                .border_1()
                                .border_color(pal::ink())
                                .bg(if connected { pal::pink() } else { pal::yellow() })
                                .hover(|s| s.bg(pal::tint()))
                                .cursor_pointer()
                                .text_size(px(10.5))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(pal::ink())
                                .child(if connected { "Disconnect" } else { "Connect" })
                                .on_click(move |_, _w, cx| {
                                    bt_e.update(cx, |b, _| {
                                        b.toggle_device_connection(path.clone(), connected);
                                    });
                                }),
                        )
                }))
        };

        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_1p5()
            .bg(pal::white())
            .border_t_2()
            .border_color(pal::ink())
            .text_size(px(10.0))
            .text_color(pal::gray())
            .child("Esc to close")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(if discovering { "Scanning active" } else { "Idle" }),
            );

        div()
            .id("wk-bluetooth-popup")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w_full()
            .h_full()
            .bg(pal::paper())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 6.0))
            .flex()
            .flex_col()
            .child(header)
            .child(content.flex_1())
            .child(footer)
            .with_animation(
                "bt-popup-in",
                Animation::new(Duration::from_millis(220)),
                |el, d| el.opacity(out_back(d).min(1.0)),
            )
    }
}
