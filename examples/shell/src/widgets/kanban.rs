use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    Context, Div, ElementId, FocusHandle, KeyDownEvent, ParentElement, Render, Rgba, Stateful,
    Styled, Window,
};
use serde::{Deserialize, Serialize};
use widgetkit::prelude::*;

use crate::theme::{fullscreen, hard, pal};

pub fn window() -> LayerWindow {
    fullscreen("kanban", "wk-kanban")
}

// ---------------------------------------------------------------------------
// Data Model & Persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteColor {
    Yellow,
    Peach,
    Pink,
    Green,
    Blue,
    Purple,
}

impl NoteColor {
    pub const ALL: [NoteColor; 6] = [
        NoteColor::Yellow,
        NoteColor::Peach,
        NoteColor::Pink,
        NoteColor::Green,
        NoteColor::Blue,
        NoteColor::Purple,
    ];

    pub fn bg(&self) -> Rgba {
        match self {
            NoteColor::Yellow => pal::yellow(),
            NoteColor::Peach => pal::peach(),
            NoteColor::Pink => pal::pink(),
            NoteColor::Green => pal::green(),
            NoteColor::Blue => pal::blue(),
            NoteColor::Purple => pal::purple(),
        }
    }

    pub fn paper(&self) -> Rgba {
        match self {
            NoteColor::Yellow => gpui::rgb(0xfef8dd),
            NoteColor::Peach => gpui::rgb(0xfdedde),
            NoteColor::Pink => gpui::rgb(0xfce8e6),
            NoteColor::Green => gpui::rgb(0xedf5db),
            NoteColor::Blue => gpui::rgb(0xe4edf7),
            NoteColor::Purple => gpui::rgb(0xf0ebf8),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            NoteColor::Yellow => "Amber",
            NoteColor::Peach => "Peach",
            NoteColor::Pink => "Terracotta",
            NoteColor::Green => "Sage",
            NoteColor::Blue => "Cyan",
            NoteColor::Purple => "Iris",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnId {
    Todo,
    Doing,
    Done,
    Notes,
}

impl ColumnId {
    pub const ALL: [ColumnId; 4] = [ColumnId::Todo, ColumnId::Doing, ColumnId::Done, ColumnId::Notes];

    pub fn label(&self) -> &'static str {
        match self {
            ColumnId::Todo => "To Do",
            ColumnId::Doing => "In Progress",
            ColumnId::Done => "Done",
            ColumnId::Notes => "Sticky Notes",
        }
    }

    pub fn accent(&self) -> Rgba {
        match self {
            ColumnId::Todo => pal::peach(),
            ColumnId::Doing => pal::blue(),
            ColumnId::Done => pal::green(),
            ColumnId::Notes => pal::yellow(),
        }
    }

    pub fn next(&self) -> Option<ColumnId> {
        match self {
            ColumnId::Todo => Some(ColumnId::Doing),
            ColumnId::Doing => Some(ColumnId::Done),
            ColumnId::Done | ColumnId::Notes => None,
        }
    }

    pub fn prev(&self) -> Option<ColumnId> {
        match self {
            ColumnId::Doing => Some(ColumnId::Todo),
            ColumnId::Done => Some(ColumnId::Doing),
            ColumnId::Todo | ColumnId::Notes => None,
        }
    }

    fn empty_hint(&self) -> &'static str {
        match self {
            ColumnId::Todo => "Nothing planned. Press N to add a card.",
            ColumnId::Doing => "Nothing in flight right now.",
            ColumnId::Done => "Finished cards land here.",
            ColumnId::Notes => "No notes yet. Press S to pin one.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanCard {
    pub id: String,
    pub title: String,
    pub body: String,
    pub column: ColumnId,
    pub color: NoteColor,
    pub is_sticky: bool,
    pub updated_at: String,
}

impl KanbanCard {
    fn is_note(&self) -> bool {
        self.is_sticky || self.column == ColumnId::Notes
    }

    /// `q` must already be lowercase.
    fn matches(&self, q: &str) -> bool {
        q.is_empty() || self.title.to_lowercase().contains(q) || self.body.to_lowercase().contains(q)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanStore {
    pub cards: Vec<KanbanCard>,
}

fn stamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute()
    )
}

impl Default for KanbanStore {
    fn default() -> Self {
        let mk = |id: &str, title: &str, body: &str, column, color, is_sticky| KanbanCard {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            column,
            color,
            is_sticky,
            updated_at: stamp(),
        };
        Self {
            cards: vec![
                mk(
                    "card-welcome",
                    "Welcome to Kumizo Kanban",
                    "Use this fullscreen board for agile tasks, quick sticky memos, and sprint planning.",
                    ColumnId::Todo,
                    NoteColor::Blue,
                    false,
                ),
                mk(
                    "note-scratchpad",
                    "💡 Fast Idea Pin",
                    "Press N for a new card, S for a sticky note, / to search, Ctrl+Z to undo a delete.",
                    ColumnId::Notes,
                    NoteColor::Yellow,
                    true,
                ),
                mk(
                    "card-sample-doing",
                    "Deep focus session",
                    "Move cards across columns using the ← and → buttons.",
                    ColumnId::Doing,
                    NoteColor::Peach,
                    false,
                ),
                mk(
                    "card-sample-done",
                    "Risograph theme setup",
                    "Neobrutalist cards with hard ink shadows and tactile Flexoki colors.",
                    ColumnId::Done,
                    NoteColor::Green,
                    false,
                ),
            ],
        }
    }
}

impl KanbanStore {
    fn path() -> PathBuf {
        let base = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".local/share")
            });
        let dir = base.join("kumizo");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("kanban.json")
    }

    pub fn load() -> Self {
        let p = Self::path();
        if let Ok(bytes) = std::fs::read(&p) {
            match serde_json::from_slice::<KanbanStore>(&bytes) {
                Ok(store) => return store,
                Err(e) => {
                    // Never silently clobber a file we failed to parse.
                    eprintln!("kanban: failed to parse {}: {e}; keeping a .bak copy", p.display());
                    let _ = std::fs::copy(&p, p.with_extension("json.bak"));
                }
            }
        }
        let def = KanbanStore::default();
        def.save();
        def
    }

    /// Atomic save: write to a temp file, then rename over the original.
    pub fn save(&self) {
        let p = Self::path();
        let tmp = p.with_extension("json.tmp");
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &p)) {
                    eprintln!("kanban: failed to save {}: {e}", p.display());
                }
            }
            Err(e) => eprintln!("kanban: failed to serialize store: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// View & State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Board,      // To Do, In Progress, Done, Sticky Notes
    StickyWall, // Wall of colored sticky notes
    Flow,       // To Do, In Progress, Done only
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Title,
    Body,
}

impl EditField {
    fn toggle(self) -> Self {
        match self {
            EditField::Title => EditField::Body,
            EditField::Body => EditField::Title,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EditDraft {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    pub column: ColumnId,
    pub color: NoteColor,
    pub is_sticky: bool,
    pub field: EditField,
}

impl EditDraft {
    fn active_mut(&mut self) -> &mut String {
        match self.field {
            EditField::Title => &mut self.title,
            EditField::Body => &mut self.body,
        }
    }
}

/// Text produced by a keystroke, ignoring command chords.
fn typed_text(event: &KeyDownEvent) -> Option<String> {
    let m = &event.keystroke.modifiers;
    if m.control || m.platform {
        return None;
    }
    if let Some(ref text) = event.keystroke.key_char {
        let s: String = text.chars().filter(|c| !c.is_control()).collect();
        if !s.is_empty() {
            return Some(s);
        }
    }
    let key = event.keystroke.key.as_str();
    if key == "space" {
        return Some(" ".into());
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() => Some(c.to_string()),
        _ => None,
    }
}

pub struct KanbanScreen {
    focus: FocusHandle,
    pub store: KanbanStore,
    pub view_mode: ViewMode,
    pub search_query: String,
    pub search_active: bool,
    pub draft: Option<EditDraft>,
    /// Most recently deleted card + its index, for undo.
    pub last_deleted: Option<(usize, KanbanCard)>,
}

impl KanbanScreen {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            store: KanbanStore::load(),
            view_mode: ViewMode::Board,
            search_query: String::new(),
            search_active: false,
            draft: None,
            last_deleted: None,
        }
    }

    // ---- Keyboard ---------------------------------------------------------

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.is_some() {
            self.on_key_draft(event, cx);
            return;
        }

        let key = event.keystroke.key.as_str();
        let m = &event.keystroke.modifiers;
        let cmd = m.control || m.platform;

        // Search mode: every printable key goes into the query, so words
        // containing "n" or "s" no longer trigger shortcuts.
        if self.search_active {
            match key {
                "escape" => {
                    self.search_query.clear();
                    self.search_active = false;
                }
                "enter" => self.search_active = false,
                "backspace" => {
                    self.search_query.pop();
                }
                _ => match typed_text(event) {
                    Some(t) => self.search_query.push_str(&t),
                    None => return,
                },
            }
            cx.notify();
            return;
        }

        match key {
            "escape" => {
                if !self.search_query.is_empty() {
                    self.search_query.clear();
                    cx.notify();
                } else {
                    cx.hide_window("kanban");
                }
            }
            "/" => {
                self.search_active = true;
                cx.notify();
            }
            "f" if cmd => {
                self.search_active = true;
                cx.notify();
            }
            "z" if cmd => self.undo_delete(cx),
            "n" if !cmd => self.start_new_card(ColumnId::Todo, false, cx),
            "s" if !cmd => self.start_new_card(ColumnId::Notes, true, cx),
            "1" if !cmd => self.set_view(ViewMode::Board, cx),
            "2" if !cmd => self.set_view(ViewMode::StickyWall, cx),
            "3" if !cmd => self.set_view(ViewMode::Flow, cx),
            _ => {}
        }
    }

    fn on_key_draft(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let m = &event.keystroke.modifiers;
        let cmd = m.control || m.platform;

        if key == "escape" {
            self.draft = None;
            cx.notify();
            return;
        }
        if key == "enter" && cmd {
            self.save_draft(cx);
            return;
        }

        let Some(draft) = self.draft.as_mut() else { return };
        match key {
            "enter" => {
                if draft.field == EditField::Title {
                    draft.field = EditField::Body;
                } else {
                    draft.body.push('\n');
                }
            }
            "tab" => draft.field = draft.field.toggle(),
            "backspace" => {
                draft.active_mut().pop();
            }
            "v" if cmd => {
                let single_line = draft.field == EditField::Title;
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = if single_line { text.replace(['\n', '\r'], " ") } else { text };
                    draft.active_mut().push_str(&text);
                }
            }
            _ if cmd => return,
            _ => match typed_text(event) {
                Some(t) => draft.active_mut().push_str(&t),
                None => return,
            },
        }
        cx.notify();
    }

    // ---- Actions ----------------------------------------------------------

    fn set_view(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        cx.notify();
    }

    fn start_new_card(&mut self, column: ColumnId, is_sticky: bool, cx: &mut Context<Self>) {
        self.draft = Some(EditDraft {
            id: None,
            title: String::new(),
            body: String::new(),
            column,
            color: if is_sticky { NoteColor::Yellow } else { NoteColor::Blue },
            is_sticky,
            field: EditField::Title,
        });
        cx.notify();
    }

    fn start_edit_card(&mut self, card: &KanbanCard, cx: &mut Context<Self>) {
        self.draft = Some(EditDraft {
            id: Some(card.id.clone()),
            title: card.title.clone(),
            body: card.body.clone(),
            column: card.column,
            color: card.color,
            is_sticky: card.is_sticky,
            field: EditField::Title,
        });
        cx.notify();
    }

    fn save_draft(&mut self, cx: &mut Context<Self>) {
        if let Some(draft) = self.draft.take() {
            let title = draft.title.trim();
            let body = draft.body.trim();
            if title.is_empty() && body.is_empty() {
                cx.notify();
                return;
            }
            let title = if title.is_empty() { "Untitled Note".to_string() } else { title.to_string() };
            let body = body.to_string();
            let timestamp = stamp();

            match draft.id {
                Some(id) => {
                    if let Some(card) = self.store.cards.iter_mut().find(|c| c.id == id) {
                        card.title = title;
                        card.body = body;
                        card.column = draft.column;
                        card.color = draft.color;
                        card.is_sticky = draft.is_sticky;
                        card.updated_at = timestamp;
                    }
                }
                None => {
                    let id = format!("card-{}", time::OffsetDateTime::now_utc().unix_timestamp_nanos());
                    self.store.cards.push(KanbanCard {
                        id,
                        title,
                        body,
                        column: draft.column,
                        color: draft.color,
                        is_sticky: draft.is_sticky,
                        updated_at: timestamp,
                    });
                }
            }
            self.store.save();
        }
        cx.notify();
    }

    fn delete_card(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.store.cards.iter().position(|c| c.id == id) {
            let card = self.store.cards.remove(pos);
            self.last_deleted = Some((pos, card));
            self.store.save();
            cx.notify();
        }
    }

    fn undo_delete(&mut self, cx: &mut Context<Self>) {
        if let Some((pos, card)) = self.last_deleted.take() {
            let pos = pos.min(self.store.cards.len());
            self.store.cards.insert(pos, card);
            self.store.save();
            cx.notify();
        }
    }

    /// Moves a card to another column; it lands at the bottom of that column.
    fn move_card(&mut self, id: &str, to_column: ColumnId, cx: &mut Context<Self>) {
        if let Some(pos) = self.store.cards.iter().position(|c| c.id == id) {
            let mut card = self.store.cards.remove(pos);
            card.column = to_column;
            card.updated_at = stamp();
            self.store.cards.push(card);
            self.store.save();
            cx.notify();
        }
    }

    /// (done, total) over the flow columns, ignoring sticky notes.
    fn progress(&self) -> (usize, usize) {
        let flow = self.store.cards.iter().filter(|c| !c.is_note());
        let total = flow.clone().count();
        let done = flow.filter(|c| c.column == ColumnId::Done).count();
        (done, total)
    }
}

// ---------------------------------------------------------------------------
// Small UI helpers
// ---------------------------------------------------------------------------

fn icon_btn(id: impl Into<ElementId>, label: &'static str, hover: Rgba) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .cursor_pointer()
        .px_1p5()
        .bg(pal::white())
        .border_1()
        .border_color(pal::ink())
        .hover(move |h| h.bg(hover))
        .text_size(gpui::px(10.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(pal::ink())
        .child(label)
}

// ---------------------------------------------------------------------------
// Render Implementation
// ---------------------------------------------------------------------------

impl Render for KanbanScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        let search = self.search_query.trim().to_lowercase();

        let visible_columns: Vec<ColumnId> = match self.view_mode {
            ViewMode::Board => ColumnId::ALL.to_vec(),
            ViewMode::StickyWall => vec![ColumnId::Notes],
            ViewMode::Flow => vec![ColumnId::Todo, ColumnId::Doing, ColumnId::Done],
        };

        let deleted_title = self.last_deleted.as_ref().map(|(_, c)| c.title.clone());

        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .w_full()
            .h_full()
            .bg(pal::paper())
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .child(self.render_header(cx))
            .child(if self.view_mode == ViewMode::StickyWall {
                self.render_sticky_wall(&search, cx).into_any_element()
            } else {
                self.render_columns(&visible_columns, &search, cx).into_any_element()
            })
            .child(self.render_hint_bar())
            .when_some(deleted_title, |this, title| this.child(self.render_toast(title, cx)))
            .when_some(self.draft.clone(), |this, draft| {
                this.child(self.render_draft_modal(&draft, cx))
            })
    }
}

impl KanbanScreen {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let q = &self.search_query;
        let mode = self.view_mode;
        let total = self.store.cards.len();
        let search_active = self.search_active;
        let (done, flow_total) = self.progress();
        let frac = if flow_total == 0 { 0.0 } else { done as f32 / flow_total as f32 };

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_4()
            .pb_4()
            .border_b_2()
            .border_color(pal::ink())
            // Left: title, count badge, progress
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(24.0))
                            .font_weight(gpui::FontWeight::BLACK)
                            .text_color(pal::ink())
                            .child("KANBAN // NOTES"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .bg(pal::yellow())
                            .border_2()
                            .border_color(pal::ink())
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .child(format!("{total} items")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(100.0))
                                    .h(px(10.0))
                                    .bg(pal::white())
                                    .border_1()
                                    .border_color(pal::ink())
                                    .child(div().h_full().w(px(98.0 * frac)).bg(pal::green())),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(format!("{done}/{flow_total} done")),
                            ),
                    ),
            )
            // Center: view tabs + search
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 3.0))
                            .child(self.render_tab_btn("Board", ViewMode::Board, mode == ViewMode::Board, cx))
                            .child(self.render_tab_btn(
                                "Sticky Wall",
                                ViewMode::StickyWall,
                                mode == ViewMode::StickyWall,
                                cx,
                            ))
                            .child(self.render_tab_btn("Flow", ViewMode::Flow, mode == ViewMode::Flow, cx)),
                    )
                    .child(
                        div()
                            .id("kanban-search-box")
                            .cursor_pointer()
                            .flex()
                            .flex_row()
                            .items_center()
                            .w(px(260.0))
                            .h(px(36.0))
                            .px_3()
                            .bg(pal::white())
                            .border_2()
                            .border_color(if search_active { pal::blue() } else { pal::ink() })
                            .shadow(hard(pal::ink(), 2.0))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.search_active = true;
                                cx.notify();
                            }))
                            .child(div().text_color(pal::gray()).pr_2().child("\u{f002}"))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if q.is_empty() { pal::gray() } else { pal::ink() })
                                    .child(if q.is_empty() {
                                        if search_active { "Type to search...".into() } else { "Press / to search...".into() }
                                    } else {
                                        q.clone()
                                    }),
                            )
                            .when(search_active, |s| s.child(div().w(px(2.0)).h(px(16.0)).bg(pal::blue())))
                            .when(!q.is_empty(), |s| {
                                s.child(
                                    div()
                                        .id("clear-kanban-search-btn")
                                        .cursor_pointer()
                                        .pl_2()
                                        .text_size(px(11.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(pal::ink())
                                        .child("✕")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.search_query.clear();
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            // Right: actions
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("header-new-card-btn")
                            .cursor_pointer()
                            .h(px(36.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .bg(pal::peach())
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 3.0))
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .hover(|h| h.bg(pal::yellow()))
                            .child("+ New Card")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_new_card(ColumnId::Todo, false, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("header-new-sticky-btn")
                            .cursor_pointer()
                            .h(px(36.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .bg(pal::yellow())
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 3.0))
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .hover(|h| h.bg(pal::pink()))
                            .child("📌 Sticky Note")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_new_card(ColumnId::Notes, true, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("header-close-kanban-btn")
                            .cursor_pointer()
                            .w(px(36.0))
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(pal::white())
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 3.0))
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .hover(|h| h.bg(pal::pink()))
                            .child("✕")
                            .on_click(|_, _, cx| cx.hide_window("kanban")),
                    ),
            )
    }

    fn render_tab_btn(
        &self,
        label: &'static str,
        mode: ViewMode,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(format!("tab-btn-{}", label))
            .cursor_pointer()
            .h(px(32.0))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .font_weight(gpui::FontWeight::BOLD)
            .when(active, |s| s.bg(pal::blue()).text_color(pal::paper()))
            .when(!active, |s| {
                s.bg(pal::white()).text_color(pal::ink()).hover(|h| h.bg(pal::tint()))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.set_view(mode, cx)))
    }

    fn render_hint_bar(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .gap_4()
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(pal::gray())
            .child("N new card")
            .child("S sticky note")
            .child("/ search")
            .child("1 / 2 / 3 views")
            .child("Ctrl+Z undo delete")
            .child("Esc close")
    }

    fn render_toast(&self, title: String, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .bottom(px(28.0))
            .left(px(0.0))
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .h(px(40.0))
                    .bg(pal::ink())
                    .border_2()
                    .border_color(pal::ink())
                    .shadow(hard(pal::yellow(), 3.0))
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(pal::paper())
                    .child(format!("Deleted “{}”", title))
                    .child(
                        div()
                            .id("toast-undo-btn")
                            .cursor_pointer()
                            .px_2()
                            .bg(pal::yellow())
                            .text_color(pal::ink())
                            .child("Undo")
                            .on_click(cx.listener(|this, _, _, cx| this.undo_delete(cx))),
                    )
                    .child(
                        div()
                            .id("toast-dismiss-btn")
                            .cursor_pointer()
                            .child("✕")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.last_deleted = None;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_columns(&self, cols: &[ColumnId], q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .gap_4()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .children(cols.iter().map(|&col| self.render_column(col, q, cx)))
    }

    fn render_column(&self, col: ColumnId, q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<_> = self
            .store
            .cards
            .iter()
            .filter(|c| c.column == col && c.matches(q))
            .cloned()
            .collect();

        let count = cards.len();
        let accent = col.accent();
        let searching = !q.is_empty();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(pal::white())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 4.0))
            .p_3()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .border_b_2()
                    .border_color(pal::ink())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(div().w(px(10.0)).h(px(10.0)).bg(accent).border_1().border_color(pal::ink()))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::BLACK)
                                    .text_color(pal::ink())
                                    .child(col.label()),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .bg(pal::paper())
                                    .border_2()
                                    .border_color(pal::ink())
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child(format!("{count}")),
                            ),
                    )
                    .child(
                        div()
                            .id(format!("col-add-btn-{}", col.label()))
                            .cursor_pointer()
                            .px_2()
                            .py_0p5()
                            .bg(pal::paper())
                            .border_2()
                            .border_color(pal::ink())
                            .hover(|h| h.bg(pal::yellow()))
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .child("+")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_new_card(col, col == ColumnId::Notes, cx);
                            })),
                    ),
            )
            // Scrollable card list (previously clipped with overflow_hidden)
            .child(
                div()
                    .id(format!("col-scroll-{}", col.label()))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(cards.is_empty(), |this| {
                        this.child(
                            div()
                                .p_4()
                                .border_1()
                                .border_color(pal::faint())
                                .text_size(px(12.0))
                                .text_color(pal::gray())
                                .child(if searching { "No matches" } else { col.empty_hint() }),
                        )
                    })
                    .children(cards.into_iter().map(|card| self.render_card(&card, cx))),
            )
    }

    fn render_card(&self, card: &KanbanCard, cx: &mut Context<Self>) -> impl IntoElement {
        let col = card.column;
        let is_sticky = card.is_note();
        let prev_col = col.prev();
        let next_col = col.next();

        let edit_card = card.clone();
        let left_id = card.id.clone();
        let right_id = card.id.clone();
        let del_id = card.id.clone();

        div()
            .id(format!("kanban-card-{}", card.id))
            .flex()
            .flex_col()
            .bg(if is_sticky { card.color.paper() } else { pal::paper() })
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 3.0))
            .p_3()
            .gap_2()
            .cursor_pointer()
            .hover(|h| h.shadow(hard(pal::ink(), 5.0)))
            .when(col == ColumnId::Done, |s| s.opacity(0.7))
            .on_click(cx.listener(move |this, _, _, cx| this.start_edit_card(&edit_card, cx)))
            .when(is_sticky, |this| {
                this.child(
                    div().w_full().flex().justify_center().mb_1().child(
                        div()
                            .w(px(38.0))
                            .h(px(8.0))
                            .bg(card.color.bg())
                            .border_1()
                            .border_color(pal::ink())
                            .opacity(0.85),
                    ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .child(card.title.clone()),
                    )
                    .child(div().w(px(12.0)).h(px(12.0)).bg(card.color.bg()).border_1().border_color(pal::ink())),
            )
            .when(!card.body.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(pal::ink())
                        .opacity(0.8)
                        .child(card.body.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .pt_1()
                    .border_t_1()
                    .border_color(pal::faint())
                    .child(
                        div()
                            .text_size(px(10.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(pal::gray())
                            .child(card.updated_at.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1p5()
                            .when_some(prev_col, |this, prev| {
                                this.child(
                                    icon_btn(format!("card-left-{}", left_id), "←", pal::yellow()).on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.move_card(&left_id, prev, cx);
                                        }),
                                    ),
                                )
                            })
                            .when_some(next_col, |this, next| {
                                this.child(
                                    icon_btn(format!("card-right-{}", right_id), "→", pal::yellow()).on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.move_card(&right_id, next, cx);
                                        }),
                                    ),
                                )
                            })
                            .child(
                                icon_btn(format!("card-del-{}", del_id), "✕", pal::pink()).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.delete_card(&del_id, cx);
                                    }),
                                ),
                            ),
                    ),
            )
    }

    fn render_sticky_wall(&self, q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let notes: Vec<_> = self
            .store
            .cards
            .iter()
            .filter(|c| c.is_note() && c.matches(q))
            .cloned()
            .collect();
        let empty = notes.is_empty();
        let searching = !q.is_empty();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(pal::white())
            .border_2()
            .border_color(pal::ink())
            .shadow(hard(pal::ink(), 5.0))
            .p_4()
            .gap_4()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .child("📌 STICKY WALL // QUICK PINNED MEMOS"),
                    )
                    .child(
                        div()
                            .id("btn-pin-sticky-wall")
                            .cursor_pointer()
                            .px_3()
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .bg(pal::yellow())
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 2.0))
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(pal::ink())
                            .hover(|h| h.bg(pal::pink()))
                            .child("+ Pin New Sticky Note")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_new_card(ColumnId::Notes, true, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("sticky-wall-scroll")
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .content_start()
                    .gap_4()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(empty, |this| {
                        this.child(
                            div()
                                .text_size(px(13.0))
                                .text_color(pal::gray())
                                .child(if searching { "No matching notes" } else { "The wall is empty. Press S to pin a note." }),
                        )
                    })
                    .children(notes.into_iter().map(|note| div().w(px(280.0)).child(self.render_card(&note, cx)))),
            )
    }

    fn render_draft_modal(&self, draft: &EditDraft, cx: &mut Context<Self>) -> impl IntoElement {
        let is_editing = draft.id.is_some();
        let field = draft.field;
        let selected_col = draft.column;
        let selected_color = draft.color;

        let section_label = |text: &'static str| {
            div()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(pal::ink())
                .child(text)
        };

        div()
            .id("draft-backdrop")
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .w_full()
            .h_full()
            .bg(pal::veil())
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                this.draft = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("draft-modal-body")
                    .w(px(520.0))
                    .bg(pal::paper())
                    .border_2()
                    .border_color(pal::ink())
                    .shadow(hard(pal::ink(), 8.0))
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .pb_2()
                            .border_b_2()
                            .border_color(pal::ink())
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(gpui::FontWeight::BLACK)
                                    .text_color(pal::ink())
                                    .child(if is_editing { "EDIT NOTE / CARD" } else { "NEW NOTE / CARD" }),
                            )
                            .child(
                                div()
                                    .id("draft-modal-close-btn")
                                    .cursor_pointer()
                                    .px_2()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("✕")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.draft = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    // Title
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(section_label("TITLE (Enter or Tab to move to body)"))
                            .child(
                                div()
                                    .id("draft-title-input-box")
                                    .h(px(40.0))
                                    .px_3()
                                    .bg(pal::white())
                                    .border_2()
                                    .border_color(if field == EditField::Title { pal::blue() } else { pal::ink() })
                                    .shadow(hard(pal::ink(), 2.0))
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(ref mut d) = this.draft {
                                            d.field = EditField::Title;
                                            cx.notify();
                                        }
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(14.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(if draft.title.is_empty() { pal::gray() } else { pal::ink() })
                                            .child(if draft.title.is_empty() {
                                                "Enter task or note title...".into()
                                            } else {
                                                draft.title.clone()
                                            }),
                                    )
                                    .when(field == EditField::Title, |s| {
                                        s.child(div().w(px(2.0)).h(px(16.0)).bg(pal::blue()))
                                    }),
                            ),
                    )
                    // Body
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(section_label("CONTENT / MEMO"))
                            .child(
                                div()
                                    .id("draft-body-input-box")
                                    .min_h(px(100.0))
                                    .p_3()
                                    .bg(pal::white())
                                    .border_2()
                                    .border_color(if field == EditField::Body { pal::blue() } else { pal::ink() })
                                    .shadow(hard(pal::ink(), 2.0))
                                    .flex()
                                    .flex_col()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(ref mut d) = this.draft {
                                            d.field = EditField::Body;
                                            cx.notify();
                                        }
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(13.0))
                                            .line_height(px(18.0))
                                            .text_color(if draft.body.is_empty() { pal::gray() } else { pal::ink() })
                                            .child(if draft.body.is_empty() {
                                                "Write task details or sticky notes here...".into()
                                            } else {
                                                draft.body.clone()
                                            }),
                                    )
                                    .when(field == EditField::Body, |s| {
                                        s.child(div().w(px(2.0)).h(px(14.0)).bg(pal::blue()))
                                    }),
                            ),
                    )
                    // Column pills
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(section_label("DESTINATION COLUMN"))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .children(
                                        ColumnId::ALL
                                            .iter()
                                            .map(|&c| self.render_col_select_btn(c, selected_col == c, cx)),
                                    ),
                            ),
                    )
                    // Color swatches
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(section_label("NOTE COLOR TAG"))
                            .child(div().flex().flex_row().gap_2().children(NoteColor::ALL.iter().map(|&color| {
                                let is_sel = selected_color == color;
                                div()
                                    .id(format!("color-swatch-{}", color.label()))
                                    .cursor_pointer()
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .bg(color.bg())
                                    .border_2()
                                    .border_color(pal::ink())
                                    .when(is_sel, |s| s.shadow(hard(pal::ink(), 3.0)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(ref mut d) = this.draft {
                                            d.color = color;
                                            cx.notify();
                                        }
                                    }))
                            }))),
                    )
                    // Footer
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .pt_2()
                            .border_t_2()
                            .border_color(pal::ink())
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(pal::gray())
                                    .child("Ctrl+Enter save · Esc cancel · Ctrl+V paste"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id("draft-cancel-btn")
                                            .cursor_pointer()
                                            .px_4()
                                            .h(px(36.0))
                                            .flex()
                                            .items_center()
                                            .bg(pal::white())
                                            .border_2()
                                            .border_color(pal::ink())
                                            .shadow(hard(pal::ink(), 2.0))
                                            .text_size(px(13.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .hover(|h| h.bg(pal::tint()))
                                            .child("Cancel")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.draft = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("draft-save-btn")
                                            .cursor_pointer()
                                            .px_5()
                                            .h(px(36.0))
                                            .flex()
                                            .items_center()
                                            .bg(pal::yellow())
                                            .border_2()
                                            .border_color(pal::ink())
                                            .shadow(hard(pal::ink(), 3.0))
                                            .text_size(px(13.0))
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(pal::ink())
                                            .hover(|h| h.bg(pal::pink()))
                                            .child("Save Card")
                                            .on_click(cx.listener(|this, _, _, cx| this.save_draft(cx))),
                                    ),
                            ),
                    ),
            )
    }

    fn render_col_select_btn(&self, col: ColumnId, is_selected: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(format!("col-sel-btn-{}", col.label()))
            .cursor_pointer()
            .px_3()
            .h(px(28.0))
            .flex()
            .items_center()
            .border_2()
            .border_color(pal::ink())
            .text_size(px(12.0))
            .font_weight(gpui::FontWeight::BOLD)
            .when(is_selected, |s| s.bg(col.accent()).text_color(pal::paper()).shadow(hard(pal::ink(), 2.0)))
            .when(!is_selected, |s| {
                s.bg(pal::white()).text_color(pal::ink()).hover(|h| h.bg(pal::tint()))
            })
            .child(col.label())
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(ref mut d) = this.draft {
                    d.column = col;
                    d.is_sticky = col == ColumnId::Notes;
                    cx.notify();
                }
            }))
    }
}
