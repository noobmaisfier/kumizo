use std::time::Duration;
use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Audio>()
        .service::<Brightness>()
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
    brightness: Entity<Brightness>,
    visible: bool,
    shown: OsdKind,
    last_volume: u8,
    last_muted: bool,
    last_brightness: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OsdKind {
    Volume,
    Brightness,
}

fn reveal_osd(osd: &mut VolumeOsd, cx: &mut Context<VolumeOsd>, kind: OsdKind) {
    osd.shown = kind;
    osd.visible = true;
    cx.notify();

    cx.spawn(async move |this, cx| {
        async_io::Timer::after(Duration::from_millis(1800)).await;
        let _ = this.update(cx, |osd, cx| {
            osd.visible = false;
            cx.notify();
        });
    })
    .detach();
}

impl VolumeOsd {
    fn new(cx: &mut Context<Self>) -> Self {
        let audio = cx.service::<Audio>();
        let brightness = cx.service::<Brightness>();
        let initial_vol = audio.read(cx).volume();
        let initial_muted = audio.read(cx).is_muted();
        let initial_brightness = brightness.read(cx).percent();

        let osd = Self {
            audio,
            brightness,
            visible: false,
            shown: OsdKind::Volume,
            last_volume: initial_vol,
            last_muted: initial_muted,
            last_brightness: initial_brightness,
        };

        // Watch audio updates: volume AND mute reveal the OSD.
        let audio_ref = osd.audio.clone();
        cx.observe(&audio_ref, |this, audio, cx| {
            let current_vol = audio.read(cx).volume();
            let current_muted = audio.read(cx).is_muted();
            if current_vol != this.last_volume || current_muted != this.last_muted {
                this.last_volume = current_vol;
                this.last_muted = current_muted;
                reveal_osd(this, cx, OsdKind::Volume);
            }
        })
        .detach();

        let brightness_ref = osd.brightness.clone();
        cx.observe(&brightness_ref, |this, brightness, cx| {
            let level = brightness.read(cx).percent();
            if level != this.last_brightness {
                this.last_brightness = level;
                reveal_osd(this, cx, OsdKind::Brightness);
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

        let (icon, label, pct, accent) = match self.shown {
            OsdKind::Volume => {
                let audio = self.audio.read(cx);
                let muted = audio.is_muted();
                (
                    audio.icon(),
                    if muted { "Muted" } else { "Volume" },
                    audio.volume(),
                    if muted { rgb(0xf38ba8) } else { rgb(0x89b4fa) },
                )
            }
            OsdKind::Brightness => {
                let brightness = self.brightness.read(cx);
                (
                    brightness.icon(),
                    "Brightness",
                    brightness.percent(),
                    rgb(0x89b4fa),
                )
            }
        };

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
                                    .text_color(accent)
                                    .text_size(px(18.0))
                                    .child(icon)
                                    .child(
                                        div()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(0xcdd6f4))
                                            .child(label),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(rgb(0xa6adc8))
                                    .child(format!("{}%", pct)),
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
                                    .w(gpui::DefiniteLength::Fraction(pct as f32 / 100.0))
                                    .bg(accent)
                                    .rounded_full(),
                            ),
                    ),
            )
    }
}
