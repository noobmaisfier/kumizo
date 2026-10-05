use gpui::prelude::*;
use widgetkit::prelude::*;

fn main() {
    WidgetKit::new()
        .service::<Compositor>()
        .window(
            LayerWindow::new("dock")
                .layer(Layer::Top)
                .anchor(Anchor::BOTTOM)
                .exclusive_zone(64)
                .height(58.0)
                .width(480.0)
                .margins(Margins::new(0, 0, 10, 0))
                .namespace("wk-dock"),
            |cx: &mut gpui::App| cx.new(Dock::new),
        )
        .run();
}

struct DockItem {
    icon: &'static str,
    command: &'static str,
    app_class: &'static str,
}

struct Dock {
    items: Vec<DockItem>,
    compositor: Entity<Compositor>,
}

impl Dock {
    fn new(cx: &mut Context<Self>) -> Self {
        let items = vec![
            DockItem {
                icon: "",
                command: "ghostty",
                app_class: "ghostty",
            },
            DockItem {
                icon: "󰈹",
                command: "firefox",
                app_class: "firefox",
            },
            DockItem {
                icon: "󰨞",
                command: "zed",
                app_class: "dev.zed.Zed",
            },
            DockItem {
                icon: "󰉋",
                command: "nautilus",
                app_class: "org.gnome.Nautilus",
            },
            DockItem {
                icon: "󰓇",
                command: "spotify",
                app_class: "spotify",
            },
            DockItem {
                icon: "󰙯",
                command: "discord",
                app_class: "discord",
            },
        ];

        Self {
            items,
            compositor: cx.service::<Compositor>(),
        }
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let comp = self.compositor.read(cx);
        let active_win_class = comp.active_window().map(|w| w.class.to_lowercase()).unwrap_or_default();

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
                        let is_active = !active_win_class.is_empty()
                            && active_win_class.contains(&item.app_class.to_lowercase());
                        let cmd = item.command;

                        div()
                            .id(format!("dock-{}", item.command))
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
                            // Running indicator dot
                            .child(
                                div()
                                    .w(px(4.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .when(is_active, |s| s.bg(rgb(0x89b4fa)))
                                    .when(!is_active, |s| s.bg(rgba(0x00000000))),
                            )
                            .on_click(move |_, _window, _cx| {
                                let _ = std::process::Command::new(cmd).spawn();
                            })
                    })),
            )
    }
}
