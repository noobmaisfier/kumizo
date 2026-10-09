//! Toasts : popups éphémères en haut à droite (fenêtre ouverte/fermée par la barre).

use gpui::prelude::*;
use gpui::{div, Context, Entity, Window};
use widgetkit::prelude::*;

use crate::notifs;

pub struct NotificationToasts {
    notifs: Entity<Notifications>,
}

impl NotificationToasts {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            notifs: cx.service::<Notifications>(),
        }
    }
}

impl Render for NotificationToasts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toasts = self.notifs.read(cx);
        let mut col = div().size_full().flex().flex_col().gap_2();
        for n in toasts.toasts().iter() {
            col = col.child(notifs::card("toast", n, self.notifs.clone(), false));
        }
        col
    }
}
