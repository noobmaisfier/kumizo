//! Minimal headless single-line / multi-line text inputs.
//!
//! [`TextBuffer`] owns the text, the caret (as a **char** index, never a
//! byte index — slicing a `String` by bytes panics on multi-byte UTF-8)
//! and optional masking (passwords). [`TextBuffer::handle_key`] and
//! [`TextBuffer::handle_key_multiline`] implement the editing semantics;
//! [`text_input`] / [`text_area`] only draw buffer + caret with
//! caller-provided colours. Focus and event dispatch stay with the parent
//! view (window-level `on_key_down`, as in the aurora launcher/clipboard).

use gpui::{
    App, Div, ElementId, Entity, InteractiveElement, KeyDownEvent, MouseButton, ParentElement,
    Rgba, Stateful, Styled, div, px,
};

/// What a keypress meant for the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAction {
    /// Text or caret moved: the parent must `cx.notify()`.
    Changed,
    /// Single-line Enter: the parent should submit (then usually `clear()`).
    Submit,
    /// Escape: the parent should cancel/close the field.
    Cancel,
    /// Nothing consumed (arrows at the edge, Tab, modified keys, ...):
    /// let the event propagate to the parent/window handlers.
    Ignored,
}

/// Plain editing state. Cheap to clone, `'static`, so it can live in an
/// `Entity<TextBuffer>` owned by a view.
#[derive(Debug, Clone, Default)]
pub struct TextBuffer {
    text: String,
    /// Caret position as a char index (`0` = before the first char).
    cursor: usize,
    masked: bool,
    /// Reject insertions past this many chars (`0` = unlimited).
    max_chars: usize,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Password-style buffer: [`TextBuffer::display`] shows `•` per char.
    pub fn masked() -> Self {
        Self {
            masked: true,
            ..Self::default()
        }
    }

    pub fn value(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// What to draw: the text, or one `•` per char when masked.
    pub fn display(&self) -> String {
        if self.masked {
            "•".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
    }

    pub fn set_max_chars(&mut self, max: usize) {
        self.max_chars = max;
    }

    /// Insert (possibly multi-char) text at the caret. Returns whether
    /// anything changed (a `max_chars` cap can swallow the insertion).
    pub fn insert_str(&mut self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let room = if self.max_chars == 0 {
            usize::MAX
        } else {
            self.max_chars.saturating_sub(self.text.chars().count())
        };
        if room == 0 {
            return false;
        }
        let take: String = s.chars().take(room).collect();
        if take.is_empty() {
            return false;
        }
        // Char-boundary byte offset of the caret (never splits UTF-8).
        let byte: usize = self
            .text
            .char_indices()
            .map(|(b, _)| b)
            .nth(self.cursor)
            .unwrap_or(self.text.len());
        self.text.insert_str(byte, &take);
        self.cursor += take.chars().count();
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        let byte: usize = self
            .text
            .char_indices()
            .map(|(b, _)| b)
            .nth(self.cursor)
            .unwrap_or(self.text.len());
        // End byte of that char = start of the next one (or string end).
        let end: usize = self
            .text
            .char_indices()
            .map(|(b, _)| b)
            .nth(self.cursor + 1)
            .unwrap_or(self.text.len());
        self.text.drain(byte..end);
        true
    }

    pub fn delete(&mut self) -> bool {
        let len = self.text.chars().count();
        if self.cursor >= len {
            return false;
        }
        let byte: usize = self
            .text
            .char_indices()
            .map(|(b, _)| b)
            .nth(self.cursor)
            .unwrap_or(self.text.len());
        let end: usize = self
            .text
            .char_indices()
            .map(|(b, _)| b)
            .nth(self.cursor + 1)
            .unwrap_or(self.text.len());
        self.text.drain(byte..end);
        true
    }

    pub fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    pub fn move_right(&mut self) -> bool {
        if self.cursor >= self.text.chars().count() {
            return false;
        }
        self.cursor += 1;
        true
    }

    pub fn move_home(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor = 0;
        true
    }

    pub fn move_end(&mut self) -> bool {
        let len = self.text.chars().count();
        if self.cursor >= len {
            return false;
        }
        self.cursor = len;
        true
    }

    /// Place the caret (mouse click). Clamped to the text; returns whether
    /// it moved. Char index, like everywhere else here.
    pub fn set_cursor(&mut self, at: usize) -> bool {
        let at = at.min(self.text.chars().count());
        if at == self.cursor {
            return false;
        }
        self.cursor = at;
        true
    }

    /// Single-line semantics: printable chars edit, Enter submits.
    pub fn handle_key(&mut self, ev: &KeyDownEvent) -> TextAction {
        self.handle_key_inner(ev, false)
    }

    /// Multi-line semantics: Enter inserts a newline instead of submitting.
    pub fn handle_key_multiline(&mut self, ev: &KeyDownEvent) -> TextAction {
        self.handle_key_inner(ev, true)
    }

    fn changed(moved: bool) -> TextAction {
        if moved {
            TextAction::Changed
        } else {
            TextAction::Ignored
        }
    }

    fn handle_key_inner(&mut self, ev: &KeyDownEvent, multiline: bool) -> TextAction {
        match ev.keystroke.key.as_str() {
            "escape" => TextAction::Cancel,
            "enter" => {
                if multiline {
                    Self::changed(self.insert_str("\n"))
                } else {
                    TextAction::Submit
                }
            }
            "tab" => TextAction::Ignored,
            "backspace" => Self::changed(self.backspace()),
            "delete" => Self::changed(self.delete()),
            "left" => Self::changed(self.move_left()),
            "right" => Self::changed(self.move_right()),
            "home" => Self::changed(self.move_home()),
            "end" => Self::changed(self.move_end()),
            // Sway/`key_char` leaves space out: same special case as the
            // aurora launcher/clipboard search fields.
            "space" => Self::changed(self.insert_str(" ")),
            _ => {
                let m = &ev.keystroke.modifiers;
                if !m.control && !m.alt && !m.platform {
                    if let Some(ch) = ev.keystroke.key_char.as_ref() {
                        return Self::changed(self.insert_str(ch));
                    }
                }
                TextAction::Ignored
            }
        }
    }
}

fn split_at_char(s: &str, at: usize) -> (String, String) {
    let at = at.min(s.chars().count());
    let mut before = String::new();
    let mut after = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i < at {
            before.push(ch);
        } else {
            after.push(ch);
        }
    }
    (before, after)
}

/// Caller-provided look. No defaults: headless means the shell decides.
#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub text: Rgba,
    pub placeholder: Rgba,
    pub caret: Rgba,
    pub size_px: f32,
}

fn text_span(s: String, color: Rgba, size_px: f32) -> Div {
    div()
        .text_size(px(size_px))
        .text_color(color)
        .child(s)
}

/// Single-line field: `[before][caret][after]`, or placeholder + caret
/// when empty. Render-only: keys stay with the parent (`handle_key`).
/// One element type on every branch (GPUI `render` rule).
pub fn text_input(
    id: impl Into<ElementId>,
    buf: &Entity<TextBuffer>,
    style: TextStyle,
    placeholder: &str,
    cx: &App,
) -> Stateful<Div> {
    let guard = buf.read(cx);
    let size = px(style.size_px);
    let caret = div().w(px(2.)).h(size).bg(style.caret);
    let shown = guard.display();
    let mut kids = Vec::new();
    if shown.is_empty() {
        kids.push(text_span(placeholder.to_string(), style.placeholder, style.size_px));
        kids.push(caret);
    } else {
        let (before, after) = split_at_char(&shown, guard.cursor());
        kids.push(text_span(before, style.text, style.size_px));
        kids.push(caret);
        kids.push(text_span(after, style.text, style.size_px));
    }
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .children(kids)
}

/// Multi-line field: same caret logic per line, lines stacked.
///
/// Unlike [`text_input`], every character is its own clickable span: a
/// left-click moves the caret before that char (clicking a collapsed
/// empty line moves it to the line start). No text measurement involved,
/// so the mapping is exact for any font — at the cost of one element per
/// char (fine for notes-size buffers). The click mutates through the
/// buffer entity, so the parent view MUST `cx.observe(&buf, ..)` itself:
/// `update()` alone never re-renders the parent.
pub fn text_area(
    id: &'static str,
    buf: &Entity<TextBuffer>,
    style: TextStyle,
    placeholder: &str,
    cx: &App,
) -> Stateful<Div> {
    let size = px(style.size_px);
    let guard = buf.read(cx);
    let shown = guard.display();
    let at = guard.cursor().min(shown.chars().count());
    // (`guard` is dead after this: rows only capture `buf`, the entity.)

    let caret = |tag: usize| {
        div()
            .id((id, tag))
            .w(px(2.))
            .h(size)
            .bg(style.caret)
    };
    let mut rows: Vec<Div> = Vec::new();
    // Single tag counter for every span under this textarea: char spans,
    // carets and empty-line fillers can never collide. Passed by value
    // (no shared closure) to keep the borrow checker happy.
    let mut tag = 0usize;
    let char_span =
        |kids: &mut Vec<Stateful<Div>>, ch: char, idx: usize, tag: usize| {
            let click = buf.clone();
            kids.push(
                div()
                    .id((id, tag))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        click.update(cx, |b, cx| {
                            if b.set_cursor(idx) {
                                cx.notify();
                            }
                        });
                    })
                    .child(text_span(ch.to_string(), style.text, style.size_px)),
            );
        };

    if shown.is_empty() {
        rows.push(
            div()
                .flex()
                .flex_row()
                .items_center()
                .child(text_span(
                    placeholder.to_string(),
                    style.placeholder,
                    style.size_px,
                ))
                .child(caret({ tag += 1; tag })),
        );
    } else {
        // Char index of each line start, to locate the caret line.
        let mut line_starts = vec![0usize];
        for (i, ch) in shown.chars().enumerate() {
            if ch == '\n' {
                line_starts.push(i + 1);
            }
        }
        let caret_line = line_starts.iter().rposition(|&s| s <= at).unwrap_or(0);
        for (li, line) in shown.split('\n').enumerate() {
            let line_start = line_starts[li];
            let col = if li == caret_line {
                Some(at - line_start)
            } else {
                None
            };
            let mut kids: Vec<Stateful<Div>> = Vec::new();
            for (ci, ch) in line.chars().enumerate() {
                if Some(ci) == col {
                    tag += 1;
                    kids.push(caret(tag));
                }
                tag += 1;
                char_span(&mut kids, ch, line_start + ci, tag);
            }
            if Some(line.chars().count()) == col {
                tag += 1;
                kids.push(caret(tag));
            }
            let mut row = div().flex().flex_row().items_center().children(kids);
            if line.is_empty() {
                // Collapsed line: keep a clickable height, click = line start.
                let idx = line_start;
                let click = buf.clone();
                tag += 1;
                row = row.child(
                    div()
                        .id((id, tag))
                        .min_h(size)
                        .w_full()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            click.update(cx, |b, cx| {
                                if b.set_cursor(idx) {
                                    cx.notify();
                                }
                            });
                        }),
                );
            }
            rows.push(row);
        }
    }
    div().id(id).flex().flex_col().children(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_chars_not_bytes() {
        let mut b = TextBuffer::new();
        assert!(b.insert_str("héllo→"));
        // 6 chars, more bytes: the caret counts chars.
        assert_eq!(b.cursor(), 6);
        assert!(b.move_left());
        assert_eq!(b.cursor(), 5);
        assert!(b.backspace()); // caret was before `→`: eats the `o`
        assert_eq!(b.value(), "héll→");
        assert_eq!(b.cursor(), 4);
    }

    #[test]
    fn set_cursor_clamps() {
        let mut b = TextBuffer::new();
        b.insert_str("abc");
        assert!(b.set_cursor(1));
        assert_eq!(b.cursor(), 1);
        assert!(!b.set_cursor(1)); // no move, no change
        assert!(b.set_cursor(99)); // clamps to end
        assert_eq!(b.cursor(), 3);
    }

    #[test]
    fn masked_display_hides_text() {
        let mut b = TextBuffer::masked();
        b.insert_str("secret");
        assert_eq!(b.value(), "secret");
        assert_eq!(b.display(), "••••••");
    }

    #[test]
    fn max_chars_caps_insertion() {
        let mut b = TextBuffer::new();
        b.set_max_chars(3);
        assert!(b.insert_str("abcdef"));
        assert_eq!(b.value(), "abc");
        assert!(!b.insert_str("x"));
    }

    #[test]
    fn delete_and_home_end() {
        let mut b = TextBuffer::new();
        b.insert_str("abc");
        assert!(b.move_home());
        assert!(b.delete());
        assert_eq!(b.value(), "bc");
        assert!(b.move_end());
        assert_eq!(b.cursor(), 2);
        assert!(!b.delete()); // at end: no-op
        assert!(!b.move_right());
    }
}
