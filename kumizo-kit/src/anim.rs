//! Open/close animations for every window (option B).
//!
//! Each registered view is wrapped in a [`WindowShell`] root view: opening a
//! window mounts a fresh surface, so the `wk-open` fade replays every time;
//! hiding sets [`AnimatedRoot::set_closing`] then waits [`CLOSE_MS`] so the
//! `wk-close` fade can play before the surface is destroyed.
//!
//! Fade-only on purpose: a slide would point the wrong way for
//! bottom-anchored windows, and this snapshot has no true transform/scale
//! primitive. `with_animation` honours `App::reduce_motion` itself, so
//! reduced-motion users get the end state with zero extra code.

use std::time::Duration;

use gpui::{Animation, AnimationExt, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};

/// Open fade length. Short: popovers must feel instant, not floaty.
pub const OPEN_MS: u64 = 150;
/// Close fade length. Slightly shorter so hiding still feels snappy.
pub const CLOSE_MS: u64 = 130;

/// Root views that can play a close animation before their surface dies.
pub trait AnimatedRoot: Render {
    fn set_closing(&mut self, closing: bool);
}

/// Generic root wrapping one registered view. Owned by the window control
/// task; the inner view never knows it is wrapped.
pub struct WindowShell<V: Render> {
    inner: Entity<V>,
    closing: bool,
}

impl<V: Render> WindowShell<V> {
    pub fn new(inner: Entity<V>) -> Self {
        Self {
            inner,
            closing: false,
        }
    }
}

impl<V: Render + 'static> AnimatedRoot for WindowShell<V> {
    fn set_closing(&mut self, closing: bool) {
        self.closing = closing;
    }
}

impl<V: Render + 'static> Render for WindowShell<V> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let inner = self.inner.clone();
        if self.closing {
            // Fresh element id (vs `wk-open`): mounting it replays the fade
            // from 1 to 0 over CLOSE_MS, then the control task removes the
            // surface.
            div().child(inner).with_animation(
                "wk-close",
                Animation::new(Duration::from_millis(CLOSE_MS))
                    .with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(1.0 - delta),
            )
        } else {
            div().child(inner).with_animation(
                "wk-open",
                Animation::new(Duration::from_millis(OPEN_MS))
                    .with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(delta),
            )
        }
    }
}
