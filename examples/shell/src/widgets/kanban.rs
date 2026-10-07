use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    Context, FocusHandle, KeyDownEvent, ParentElement, Render, Rgba, Styled,
    Window,
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

    pub const ALL: [NoteColor; 6] = [
        NoteColor::Yellow,
        NoteColor::Peach,
        NoteColor::Pink,
        NoteColor::Green,
        NoteColor::Blue,
        NoteColor::Purple,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnId {
    Todo,
    Doing,
    Done,
    Notes,
}

impl ColumnId {
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
            ColumnId::Done => None,
            ColumnId::Notes => None,
        }
    }

    pub fn prev(&self) -> Option<ColumnId> {
        match self {
            ColumnId::Todo => None,
            ColumnId::Doing => Some(ColumnId::Todo),
            ColumnId::Done => Some(ColumnId::Doing),
            ColumnId::Notes => None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanStore {
    pub cards: Vec<KanbanCard>,
}

impl Default for KanbanStore {
    fn default() -> Self {
        Self {
            cards: vec![
                KanbanCard {
                    id: "card-welcome".into(),
                    title: "Welcome to Kumizo Kanban".into(),
                    body: "Use this fullscreen board for agile tasks, quick sticky memos, and sprint planning."
                        .into(),
                    column: ColumnId::Todo,
                    color: NoteColor::Blue,
                    is_sticky: false,
                    updated_at: "Today".into(),
                },
                KanbanCard {
                    id: "note-scratchpad".into(),
                    title: "💡 Fast Idea Pin".into(),
                    body: "Sticky notes can be created with 📌 and pinned anywhere. Press N for new card, S for note."
                        .into(),
                    column: ColumnId::Notes,
                    color: NoteColor::Yellow,
                    is_sticky: true,
                    updated_at: "Today".into(),
                },
                KanbanCard {
                    id: "card-sample-doing".into(),
                    title: "Deep focus session".into(),
                    body: "Move cards across columns using the ← and → arrows or keyboard shortcuts."
                        .into(),
                    column: ColumnId::Doing,
                    color: NoteColor::Peach,
                    is_sticky: false,
                    updated_at: "Today".into(),
                },
                KanbanCard {
                    id: "card-sample-done".into(),
                    title: "Risograph theme setup".into(),
                    body: "Neobrutalist cards with hard ink shadows and tactile Flexoki colors."
                        .into(),
                    column: ColumnId::Done,
                    color: NoteColor::Green,
                    is_sticky: false,
                    updated_at: "Today".into(),
                },
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
            if let Ok(store) = serde_json::from_slice::<KanbanStore>(&bytes) {
                return store;
            }
        }
        let def = KanbanStore::default();
        def.save();
        def
    }

    pub fn save(&self) {
        let p = Self::path();
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }
}

// ---------------------------------------------------------------------------
// View & State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Board,      // 4 columns: To Do, In Progress, Done, Sticky Notes
    StickyWall, // Wall of colored sticky notes
    Flow,       // Kanban flow only (To Do, Doing, Done)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Title,
    Body,
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

pub struct KanbanScreen {
    focus: FocusHandle,
    pub store: KanbanStore,
    pub view_mode: ViewMode,
    pub search_query: String,
    pub draft: Option<EditDraft>,
}

impl KanbanScreen {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            store: KanbanStore::load(),
            view_mode: ViewMode::Board,
            search_query: String::new(),
            draft: None,
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();

        // If editing a card/note in modal:
        if let Some(ref mut draft) = self.draft {
            match key {
                "escape" => {
                    self.draft = None;
                    cx.notify();
                    return;
                }
                "enter" => {
                    if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
                        self.save_draft(cx);
                        return;
                    }
                    if draft.field == EditField::Title {
                        draft.field = EditField::Body;
                        cx.notify();
                        return;
                    } else {
                        draft.body.push('\n');
                        cx.notify();
                        return;
                    }
                }
                "tab" => {
                    draft.field = match draft.field {
                        EditField::Title => EditField::Body,
                        EditField::Body => EditField::Title,
                    };
                    cx.notify();
                    return;
                }
                "backspace" => {
                    match draft.field {
                        EditField::Title => {
                            draft.title.pop();
                        }
                        EditField::Body => {
                            draft.body.pop();
                        }
                    }
                    cx.notify();
                    return;
                }
                _ => {
                    if let Some(ref text) = event.keystroke.key_char {
                        for c in text.chars() {
                            if !c.is_control() || c == '\n' {
                                match draft.field {
                                    EditField::Title => draft.title.push(c),
                                    EditField::Body => draft.body.push(c),
                                }
                            }
                        }
                        cx.notify();
                        return;
                    } else if key == "space" {
                        match draft.field {
                            EditField::Title => draft.title.push(' '),
                            EditField::Body => draft.body.push(' '),
                        }
                        cx.notify();
                        return;
                    } else if key.chars().count() == 1 {
                        if let Some(c) = key.chars().next() {
                            if !c.is_control() {
                                match draft.field {
                                    EditField::Title => draft.title.push(c),
                                    EditField::Body => draft.body.push(c),
                                }
                                cx.notify();
                                return;
                            }
                        }
                    }
                }
            }
            return;
        }

        // Global shortcuts when no draft is open:
        match key {
            "escape" => {
                if !self.search_query.is_empty() {
                    self.search_query.clear();
                    cx.notify();
                } else {
                    cx.hide_window("kanban");
                }
            }
            "n" if !event.keystroke.modifiers.control => {
                self.start_new_card(ColumnId::Todo, false, cx);
            }
            "s" if !event.keystroke.modifiers.control => {
                self.start_new_card(ColumnId::Notes, true, cx);
            }
            "backspace" => {
                self.search_query.pop();
                cx.notify();
            }
            "space" => {
                self.search_query.push(' ');
                cx.notify();
            }
            _ => {
                if let Some(ref text) = event.keystroke.key_char {
                    for c in text.chars() {
                        if !c.is_control() {
                            self.search_query.push(c);
                        }
                    }
                    cx.notify();
                } else if key.chars().count() == 1 {
                    if let Some(c) = key.chars().next() {
                        if !c.is_control() {
                            self.search_query.push(c);
                            cx.notify();
                        }
                    }
                }
            }
        }
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
            if draft.title.trim().is_empty() && draft.body.trim().is_empty() {
                cx.notify();
                return;
            }
            let title = if draft.title.trim().is_empty() {
                "Untitled Note".to_string()
            } else {
                draft.title.trim().to_string()
            };

            let now = time::OffsetDateTime::now_utc();
            let timestamp = format!("{:02}:{:02}", now.hour(), now.minute());

            if let Some(id) = draft.id {
                if let Some(card) = self.store.cards.iter_mut().find(|c| c.id == id) {
                    card.title = title;
                    card.body = draft.body.trim().to_string();
                    card.column = draft.column;
                    card.color = draft.color;
                    card.is_sticky = draft.is_sticky;
                    card.updated_at = timestamp;
                }
            } else {
                let id = format!("card-{}", time::OffsetDateTime::now_utc().unix_timestamp_nanos());
                self.store.cards.push(KanbanCard {
                    id,
                    title,
                    body: draft.body.trim().to_string(),
                    column: draft.column,
                    color: draft.color,
                    is_sticky: draft.is_sticky,
                    updated_at: timestamp,
                });
            }
            self.store.save();
        }
        cx.notify();
    }

    fn delete_card(&mut self, id: &str, cx: &mut Context<Self>) {
        self.store.cards.retain(|c| c.id != id);
        self.store.save();
        cx.notify();
    }

    fn move_card(&mut self, id: &str, to_column: ColumnId, cx: &mut Context<Self>) {
        if let Some(card) = self.store.cards.iter_mut().find(|c| c.id == id) {
            card.column = to_column;
            self.store.save();
            cx.notify();
        }
    }
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
            ViewMode::Board => vec![ColumnId::Todo, ColumnId::Doing, ColumnId::Done, ColumnId::Notes],
            ViewMode::StickyWall => vec![ColumnId::Notes],
            ViewMode::Flow => vec![ColumnId::Todo, ColumnId::Doing, ColumnId::Done],
        };

        // Fullscreen backdrop container
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .w_full()
            .h_full()
            .bg(pal::paper())
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .child(self.render_header(cx))
            .child(
                if self.view_mode == ViewMode::StickyWall {
                    self.render_sticky_wall(&search, cx).into_any_element()
                } else {
                    self.render_columns(&visible_columns, &search, cx).into_any_element()
                }
            )
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

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_4()
            .pb_4()
            .border_b_2()
            .border_color(pal::ink())
            // Left: Title & Badge
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .relative()
                            .text_size(px(24.0))
                            .font_weight(gpui::FontWeight::BLACK)
                            .text_color(pal::ink())
                            .child("KANBAN // NOTES")
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
                            .child(format!("{total} items"))
                    )
            )
            // Center: View switcher tabs & Search input
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    // View tabs
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 3.0))
                            .child(
                                self.render_tab_btn("Board", ViewMode::Board, mode == ViewMode::Board, cx)
                            )
                            .child(
                                self.render_tab_btn("Sticky Wall", ViewMode::StickyWall, mode == ViewMode::StickyWall, cx)
                            )
                            .child(
                                self.render_tab_btn("Flow", ViewMode::Flow, mode == ViewMode::Flow, cx)
                            )
                    )
                    // Search box
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .w(px(260.0))
                            .h(px(36.0))
                            .px_3()
                            .bg(pal::white())
                            .border_2()
                            .border_color(pal::ink())
                            .shadow(hard(pal::ink(), 2.0))
                            .child(
                                div()
                                    .text_color(pal::gray())
                                    .pr_2()
                                    .child("\u{f002}")
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if q.is_empty() { pal::gray() } else { pal::ink() })
                                    .child(if q.is_empty() { "Search cards & memos...".into() } else { q.clone() })
                            )
                            .when(!q.is_empty(), |s| {
                                s.child(
                                    div()
                                        .id("clear-kanban-search-btn")
                                        .cursor_pointer()
                                        .text_size(px(11.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(pal::ink())
                                        .child("✕")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.search_query.clear();
                                            cx.notify();
                                        }))
                                )
                            })
                    )
            )
            // Right: New Card / Sticky Note buttons + Close
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
                            .gap_2()
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
                            }))
                    )
                    .child(
                        div()
                            .id("header-new-sticky-btn")
                            .cursor_pointer()
                            .h(px(36.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_2()
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
                            }))
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
                            .on_click(|_, _, cx| cx.hide_window("kanban"))
                    )
            )
    }

    fn render_tab_btn(&self, label: &'static str, mode: ViewMode, active: bool, cx: &mut Context<Self>) -> impl IntoElement {
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
            .when(active, |s| {
                s.bg(pal::blue()).text_color(pal::paper())
            })
            .when(!active, |s| {
                s.bg(pal::white()).text_color(pal::ink()).hover(|h| h.bg(pal::tint()))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.view_mode = mode;
                cx.notify();
            }))
    }

    fn render_columns(&self, cols: &[ColumnId], q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .gap_4()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .children(cols.iter().map(|&col| self.render_column(col, q, cx)))
    }

    fn render_column(&self, col: ColumnId, q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<_> = self.store.cards.iter()
            .filter(|c| c.column == col)
            .filter(|c| {
                if q.is_empty() {
                    true
                } else {
                    c.title.to_lowercase().contains(q) || c.body.to_lowercase().contains(q)
                }
            })
            .cloned()
            .collect();

        let count = cards.len();
        let accent = col.accent();

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
            // Column Header
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
                            .child(
                                div()
                                    .w(px(10.0))
                                    .h(px(10.0))
                                    .bg(accent)
                                    .border_1()
                                    .border_color(pal::ink())
                            )
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(gpui::FontWeight::BLACK)
                                    .text_color(pal::ink())
                                    .child(col.label())
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
                                    .child(format!("{count}"))
                            )
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
                            }))
                    )
            )
            // Cards List
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    .overflow_hidden()
                    .children(cards.into_iter().map(|card| self.render_card(&card, cx)))
            )
    }

    fn render_card(&self, card: &KanbanCard, cx: &mut Context<Self>) -> impl IntoElement {
        let card_id = card.id.clone();
        let card_clone = card.clone();
        let card_clone2 = card.clone();
        let card_clone3 = card.clone();
        let col = card.column;
        let is_sticky = card.is_sticky || col == ColumnId::Notes;

        let next_col = col.next();
        let prev_col = col.prev();

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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.start_edit_card(&card_clone, cx);
            }))
            // Sticky note physical tape accent at the top
            .when(is_sticky, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .mt(px(-16.0))
                        .mb_1()
                        .child(
                            div()
                                .w(px(38.0))
                                .h(px(8.0))
                                .bg(pal::paper())
                                .border_1()
                                .border_color(pal::ink())
                                .opacity(0.85)
                        )
                )
            })
            // Card Title & Tag
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
                            .child(card.title.clone())
                    )
                    .child(
                        div()
                            .w(px(12.0))
                            .h(px(12.0))
                            .bg(card.color.bg())
                            .border_1()
                            .border_color(pal::ink())
                    )
            )
            // Card Body (if not empty)
            .when(!card.body.is_empty(), |this| {
                this.child(
                    div()
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(pal::ink())
                        .opacity(0.8)
                        .child(card.body.clone())
                )
            })
            // Footer: timestamp & quick actions
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
                            .child(card.updated_at.clone())
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1p5()
                            // Move Left button
                            .when_some(prev_col, |this, prev| {
                                this.child(
                                    div()
                                        .id(format!("card-left-{}", card_clone2.id))
                                        .cursor_pointer()
                                        .px_1p5()
                                        .bg(pal::white())
                                        .border_1()
                                        .border_color(pal::ink())
                                        .hover(|h| h.bg(pal::yellow()))
                                        .text_size(px(10.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(pal::ink())
                                        .child("←")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.move_card(&card_clone2.id, prev, cx);
                                        }))
                                )
                            })
                            // Move Right button
                            .when_some(next_col, |this, next| {
                                this.child(
                                    div()
                                        .id(format!("card-right-{}", card_clone3.id))
                                        .cursor_pointer()
                                        .px_1p5()
                                        .bg(pal::white())
                                        .border_1()
                                        .border_color(pal::ink())
                                        .hover(|h| h.bg(pal::yellow()))
                                        .text_size(px(10.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(pal::ink())
                                        .child("→")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.move_card(&card_clone3.id, next, cx);
                                        }))
                                )
                            })
                            // Delete button
                            .child(
                                div()
                                    .id(format!("card-del-{}", card_id))
                                    .cursor_pointer()
                                    .px_1p5()
                                    .bg(pal::white())
                                    .border_1()
                                    .border_color(pal::ink())
                                    .hover(|h| h.bg(pal::pink()))
                                    .text_size(px(10.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("✕")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.delete_card(&card_id, cx);
                                    }))
                            )
                    )
            )
    }

    fn render_sticky_wall(&self, q: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let notes: Vec<_> = self.store.cards.iter()
            .filter(|c| c.is_sticky || c.column == ColumnId::Notes)
            .filter(|c| {
                if q.is_empty() {
                    true
                } else {
                    c.title.to_lowercase().contains(q) || c.body.to_lowercase().contains(q)
                }
            })
            .cloned()
            .collect();

        div()
            .flex()
            .flex_col()
            .flex_1()
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
                            .child("📌 STICKY WALL // QUICK PINNED MEMOS")
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
                            }))
                    )
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_4()
                    .flex_1()
                    .children(notes.into_iter().map(|note| {
                        div()
                            .w(px(280.0))
                            .child(self.render_card(&note, cx))
                    }))
            )
    }

    fn render_draft_modal(&self, draft: &EditDraft, cx: &mut Context<Self>) -> impl IntoElement {
        let is_editing = draft.id.is_some();
        let field = draft.field;
        let selected_col = draft.column;
        let selected_color = draft.color;

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
                    // Modal Header
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
                                    .child(if is_editing { "EDIT NOTE / CARD" } else { "NEW NOTE / CARD" })
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
                                    }))
                            )
                    )
                    // Title Input Box
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("TITLE (press Enter to move to body)")
                            )
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
                                            .child(if draft.title.is_empty() { "Enter task or note title...".into() } else { draft.title.clone() })
                                    )
                                    .when(field == EditField::Title, |s| {
                                        s.child(div().w(px(2.0)).h(px(16.0)).bg(pal::blue()))
                                    })
                            )
                    )
                    // Body / Notes Input Box
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("CONTENT / MEMO")
                            )
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
                                            .child(if draft.body.is_empty() { "Write task details or sticky notes here...".into() } else { draft.body.clone() })
                                    )
                                    .when(field == EditField::Body, |s| {
                                        s.child(div().w(px(2.0)).h(px(14.0)).bg(pal::blue()))
                                    })
                            )
                    )
                    // Column selection pills
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("DESTINATION COLUMN")
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(self.render_col_select_btn(ColumnId::Todo, selected_col == ColumnId::Todo, cx))
                                    .child(self.render_col_select_btn(ColumnId::Doing, selected_col == ColumnId::Doing, cx))
                                    .child(self.render_col_select_btn(ColumnId::Done, selected_col == ColumnId::Done, cx))
                                    .child(self.render_col_select_btn(ColumnId::Notes, selected_col == ColumnId::Notes, cx))
                            )
                    )
                    // Color selection swatches
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(pal::ink())
                                    .child("NOTE COLOR TAG")
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .children(NoteColor::ALL.iter().map(|&color| {
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
                                    }))
                            )
                    )
                    // Actions Footer
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_end()
                            .gap_3()
                            .pt_2()
                            .border_t_2()
                            .border_color(pal::ink())
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
                                    }))
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
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_draft(cx);
                                    }))
                            )
                    )
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
            .when(is_selected, |s| {
                s.bg(col.accent()).text_color(pal::paper()).shadow(hard(pal::ink(), 2.0))
            })
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
