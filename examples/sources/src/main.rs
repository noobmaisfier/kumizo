//! Data sources: feed a widget from a process, a file and a socket, with no
//! `Service` to write.
//!
//! One panel, three live lines. Feed it from another terminal:
//!
//! ```text
//! echo 42 > /tmp/wk-sources/temp                       # file  -> "temp"
//! echo hello | socat - UNIX-CONNECT:/tmp/wk-sources/in.sock   # socket -> "inbox"
//! ```
//!
//! The "ticker" line is a child process (`sh`) printing a counter.
//!
//! Run (IPC is off so it can run next to the real shell):
//!
//! ```text
//! cargo run --package sources-example
//! ```

use std::time::Duration;

use gpui::prelude::*;
use kumizo_kit::prelude::*;

fn main() {
    let dir = std::path::PathBuf::from("/tmp/wk-sources");
    std::fs::create_dir_all(&dir).expect("cannot create /tmp/wk-sources");

    Kumizo::new()
        .ipc(false)
        .window(
            LayerWindow::new("sources")
                .layer(Layer::Overlay)
                .anchor(Anchor::TOP | Anchor::RIGHT)
                .exclusive_zone(0)
                .size(Some(360.0), Some(150.0))
                .margins(Margins::new(20, 20, 0, 0))
                .namespace("wk-sources"),
            move |cx: &mut gpui::App| cx.new(|cx| Panel::new(&dir, cx)),
        )
        .run();
}

struct Panel {
    /// Latest line a child process printed.
    ticker: Entity<Source<String>>,
    /// A number read from a file, re-read whenever the file is written.
    temp: Entity<Source<i32>>,
    /// Last line received over a UNIX socket.
    inbox: Entity<Source<String>>,
}

impl Panel {
    fn new(dir: &std::path::Path, cx: &mut Context<Self>) -> Self {
        // 1. A process: restarts itself 1s after it exits.
        let ticker = ProcessSource::new("sh")
            .args(["-c", "i=0; while [ $i -lt 5 ]; do i=$((i+1)); echo tick $i; sleep 1; done"])
            .restart_after(Duration::from_secs(1))
            .lines(cx, |line| Some(line.to_string()))
            .track(cx);

        // 2. A file: `parse` returns None for anything that is not a number.
        let temp = Source::watch_file(cx, dir.join("temp"), |text| text.trim().parse().ok()).track(cx);

        // 3. A socket: every line written by any client.
        let inbox = Source::listen_unix(cx, dir.join("in.sock"), |line| {
            (!line.is_empty()).then(|| line.to_string())
        })
        .track(cx);

        Self { ticker, temp, inbox }
    }
}

/// One `label: value` row. Layout is entirely the caller's business.
fn row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .gap_3()
        .child(div().flex_none().text_color(rgb(0x6c7086)).child(label))
        // Bounded by layout, never by slicing the string (see CREER-UN-WIDGET step 6).
        .child(div().min_w_0().truncate().text_color(rgb(0xcdd6f4)).child(value))
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ticker = self.ticker.read(cx);
        let temp = self.temp.read(cx);
        let inbox = self.inbox.read(cx);

        // A source that failed says why: show it instead of a blank.
        let or_error = |err: Option<&str>, value: Option<String>| {
            err.map(|e| format!("error: {e}")).or(value).unwrap_or_else(|| "waiting...".into())
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .border_1()
            .border_color(rgb(0x45475a))
            .rounded_lg()
            .text_size(px(14.0))
            .child(row("ticker", or_error(ticker.error(), ticker.value().cloned())))
            .child(row("temp", or_error(temp.error(), temp.value().map(|t| format!("{t} C")))))
            .child(row("inbox", or_error(inbox.error(), inbox.value().cloned())))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(0x6c7086))
                    .child(format!("{} / {} / {} updates", ticker.updates(), temp.updates(), inbox.updates())),
            )
    }
}
