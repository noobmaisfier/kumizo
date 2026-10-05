use std::time::Duration;
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Audio>()
        .window(
            LayerWindow::new("osd")
                .layer(Layer::Overlay)
                .anchor(Anchor::BOTTOM)
                .exclusive_zone(0)
                .height(80.0)
                .width(280.0)
                .margins(Margins::new(0, 0, 100, 0))
                .namespace("wk-osd"),
            |cx: &mut gpui::App| cx.new(VolumeOsd::new),
        )
        .run();
}

struct VolumeOsd {
    audio: Entity<Audio>,
    visible: bool,
    last_volume: u8,
}

impl VolumeOsd {
    fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let initial_vol = audio.read(cx).volume();

        let osd = Self {
            audio,
            visible: false,
            last_volume: initial_vol,
        };

        // Watch audio updates: when volume changes, show OSD and auto-hide after 2s
        let audio_ref = osd.audio.clone();
        cx.observe(&audio_ref, |this, audio, cx| {
            let current_vol = audio.read(cx).volume();
            if current_vol != this.last_volume {
                this.last_volume = current_vol;
                this.visible = true;
                cx.notify();

                // Spawn a timer to auto-hide
                cx.spawn(async move |this, cx| {
                    async_io::Timer::after(Duration::from_millis(1800)).await;
                    let _ = this.update(cx, |osd, cx| {
                        osd.visible = false;
                        cx.notify();
                    });
                })
                .detach();
            }
        })
        .detach();

        osd
    }
}

impl Render for VolumeOsd {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            // Under Zero Idle CPU: render empty transparent container when invisible
            return div().w_full().h_full();
        }

        let audio = self.audio.read(cx);
        let vol = audio.volume();
        let icon = audio.icon();
        let is_muted = audio.is_muted();

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
                    .w(px(260.0))
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
                                    .text_color(if is_muted { rgb(0xf38ba8) } else { rgb(0x89b4fa) })
                                    .text_size(px(18.0))
                                    .child(icon)
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xcdd6f4))
                                            .child(if is_muted { "Muted" } else { "Volume" }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(format!("{}%", vol)),
                            ),
                    )
                    // Visual progress bar
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
                                    .w(gpui::DefiniteLength::Fraction(vol as f32 / 100.0))
                                    .bg(if is_muted { rgb(0xf38ba8) } else { rgb(0x89b4fa) })
                                    .rounded_full(),
                            ),
                    ),
            )
    }
}
