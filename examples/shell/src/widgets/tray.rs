use gpui::prelude::*;
use gpui::{Context, Entity, px, Window};
use kumizo_kit::prelude::*;

use crate::theme::{pal, printed};

pub fn window() -> LayerWindow {
    LayerWindow::new("traymenu")
        .layer(Layer::Overlay)
        .anchor(Anchor::LEFT | Anchor::BOTTOM)
        .exclusive_zone(0)
        .width(310.0)
        .height(430.0)
        .margins(Margins::new(0, 0, 110, 88))
        .namespace("wk-traymenu")
        .start_hidden(true)
}

pub struct TrayMenu {
    tray: Entity<Tray>,
    menu_open: bool,
}

impl TrayMenu {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tray = cx.service::<Tray>();
        // The service is pure data: this view owns the menu window and only
        // opens it while a menu is open.
        cx.observe(&tray, |this, tray, cx| {
            let open = tray.read(cx).menu().is_some();
            if open != this.menu_open {
                this.menu_open = open;
                if open {
                    cx.show_window("traymenu");
                } else {
                    cx.hide_window("traymenu");
                }
            }
        })
        .detach();
        Self { tray, menu_open: false }
    }
}

fn render_menu_items(items: &[TrayMenuItem], tray: &Entity<Tray>) -> Vec<gpui::AnyElement> {
    let mut out = Vec::new();
    for item in items {
        if item.separator {
            out.push(div().h(px(2.0)).my_1().bg(pal::ink()).w_full().into_any_element());
            continue;
        }
        let id = item.id;
        let mut row = div()
            .id(format!("traymenu-item-{id}"))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .text_size(px(13.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(if item.enabled { pal::ink() } else { pal::gray() });
        if item.enabled {
            let tray_entity = tray.clone();
            row = row
                .hover(|h| h.bg(pal::yellow()))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    tray_entity.update(cx, |t, cx| {
                        t.click_menu_item(id);
                        cx.notify();
                    });
                });
        }
        let check = match item.toggle {
            Some(true) => "■ ",
            Some(false) => "□ ",
            None => "",
        };
        row = row.child(format!("{}{}", check, item.label));
        out.push(row.into_any_element());
        if !item.children.is_empty() {
            out.push(
                div()
                    .pl_4()
                    .flex()
                    .flex_col()
                    .children(render_menu_items(&item.children, tray))
                    .into_any_element(),
            );
        }
    }
    out
}

impl Render for TrayMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tray_entity = self.tray.clone();
        let Some(menu) = self.tray.read(cx).menu() else {
            return div().id("traymenu-empty").w_full().h_full();
        };
        div()
            .id("traymenu-backdrop")
            .w_full()
            .h_full()
            .pr(px(8.0))
            .pb(px(8.0))
            .on_click(move |_, _, cx| {
                tray_entity.update(cx, |t, cx| {
                    t.close_menu();
                    cx.notify();
                });
            })
            .child(printed(
                div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .bg(pal::white())
                    .border_2()
                    .border_color(pal::ink())
                    .child(
                        div()
                            .px_3()
                            .py_1p5()
                            .bg(pal::blue())
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::paper())
                            .child(menu.title.clone()),
                    )
                    .child(div().h(px(2.0)).my_1().bg(pal::ink()).w_full())
                    .children(render_menu_items(&menu.items, &self.tray)),
                "traymenu",
                pal::pink(),
            ))
    }
}
