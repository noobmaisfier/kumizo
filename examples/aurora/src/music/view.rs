//! Fenêtre `aurora-music` : pochette + karaoké à gauche, bibliothèque / file à droite.
//!
//! Les listes sont PAGINÉES (autant de lignes que la hauteur en permet) au lieu
//! d'être défilantes : une bibliothèque de 20 000 titres ne coûte que ~12 lignes
//! par image, et la sélection au clavier tourne les pages toute seule.
//!
//! Clavier : Espace lecture · ←/→ ±5 s · ↑/↓ choisir · Entrée jouer · / chercher ·
//! a ajouter · x retirer · Tab bascule bibliothèque/file · n/p suivant/précédent ·
//! s aléatoire · r répétition · +/- volume · e lire ensuite · c vider la file ·
//! y paroles · F5 rescanner · Échap fermer.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, px, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, KeyDownEvent, Stateful,
    Window,
};
use widgetkit::prelude::*;

use super::{
    disc, fmt_time, library,
    lyrics::Lyrics,
    model::{handle, Player, Repeat},
};
use crate::{
    icons,
    theme::pal,
    ui::{self, ids},
};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Library,
    Queue,
    Lyrics,
}

struct Row {
    pos: usize,
    lib_idx: usize,
    title: String,
    sub: String,
    secs: u32,
    current: bool,
}

pub struct MusicPanel {
    player: Entity<Player>,
    tab: Tab,
    query: String,
    searching: bool,
    /// Position sélectionnée dans la liste affichée (absolue, pas par page).
    sel: usize,
    /// Lignes par page, recalculées à chaque rendu (pour les touches PgUp/PgDn).
    rows_per_page: usize,
    focus: FocusHandle,
}

impl MusicPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let player = handle(cx);
        cx.observe(&player, |_, _, cx| cx.notify()).detach();
        Self {
            player,
            tab: Tab::Library,
            query: String::new(),
            searching: false,
            sel: 0,
            rows_per_page: 10,
            focus: cx.focus_handle(),
        }
    }

    /// Indices (dans `tracks`) correspondant à la recherche.
    fn filtered(&self, cx: &Context<Self>) -> Vec<usize> {
        let p = self.player.read(cx);
        if self.query.is_empty() {
            return (0..p.tracks.len()).collect();
        }
        let q = self.query.to_lowercase();
        p.tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.hay.contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn list_len(&self, cx: &Context<Self>) -> usize {
        match self.tab {
            Tab::Library => self.filtered(cx).len(),
            Tab::Queue => self.player.read(cx).queue.len(),
            // Défilement par page seulement pour les paroles NON synchronisées.
            Tab::Lyrics => self
                .player
                .read(cx)
                .lyrics
                .as_ref()
                .filter(|l| !l.synced)
                .map_or(0, |l| l.lines.len()),
        }
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.list_len(cx);
        if len > 0 {
            self.sel = (self.sel as isize + delta).clamp(0, len as isize - 1) as usize;
            cx.notify();
        }
    }

    fn switch_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.sel = 0;
        cx.notify();
    }

    fn activate(&mut self, cx: &mut Context<Self>) {
        match self.tab {
            Tab::Library => {
                let list = self.filtered(cx);
                let sel = self.sel;
                self.player.update(cx, |p, cx| p.play_list(list, sel, cx));
            }
            Tab::Queue => {
                let sel = self.sel;
                self.player.update(cx, |p, cx| p.jump(sel, cx));
            }
            Tab::Lyrics => {}
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ui::key(ev);
        let ch = ev.keystroke.key_char.as_deref().unwrap_or("");

        if self.searching {
            match key {
                "escape" => {
                    self.query.clear();
                    self.searching = false;
                    self.sel = 0;
                }
                "enter" => {
                    self.searching = false;
                    self.activate(cx);
                }
                "backspace" => {
                    self.query.pop();
                    self.sel = 0;
                }
                "up" => return self.step(-1, cx),
                "down" => return self.step(1, cx),
                _ => {
                    let m = &ev.keystroke.modifiers;
                    if !m.control && !m.alt && !m.platform && !ch.is_empty()
                        && self.query.chars().count() < 64
                    {
                        self.query.push_str(ch);
                        self.sel = 0;
                    }
                }
            }
            cx.notify();
            return;
        }

        let rows = self.rows_per_page as isize;
        match key {
            "escape" => {
                if self.query.is_empty() {
                    cx.hide_window(ids::MUSIC);
                } else {
                    self.query.clear();
                    self.sel = 0;
                    cx.notify();
                }
            }
            "space" => self.player.update(cx, |p, cx| p.toggle(cx)),
            "enter" => self.activate(cx),
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "pageup" => self.step(-rows, cx),
            "pagedown" => self.step(rows, cx),
            "left" => self.player.update(cx, |p, cx| p.seek_rel(-5.0, cx)),
            "right" => self.player.update(cx, |p, cx| p.seek_rel(5.0, cx)),
            "tab" => {
                let next = match self.tab {
                    Tab::Library => Tab::Queue,
                    Tab::Queue => Tab::Lyrics,
                    Tab::Lyrics => Tab::Library,
                };
                self.switch_tab(next, cx);
            }
            "y" => {
                let next = if self.tab == Tab::Lyrics { Tab::Library } else { Tab::Lyrics };
                self.switch_tab(next, cx);
            }
            "n" => self.player.update(cx, |p, cx| p.next(cx)),
            "p" => self.player.update(cx, |p, cx| p.prev(cx)),
            "s" => self.player.update(cx, |p, cx| p.toggle_shuffle(cx)),
            "r" => self.player.update(cx, |p, cx| p.cycle_repeat(cx)),
            "f5" => self.player.update(cx, |p, cx| p.rescan(cx)),
            "/" => {
                self.tab = Tab::Library;
                self.searching = true;
                cx.notify();
            }
            "=" | "+" => {
                let v = self.player.read(cx).volume.saturating_add(5);
                self.player.update(cx, |p, cx| p.set_volume(v, cx));
            }
            "-" => {
                let v = self.player.read(cx).volume.saturating_sub(5);
                self.player.update(cx, |p, cx| p.set_volume(v, cx));
            }
            "a" if self.tab == Tab::Library => {
                if let Some(&idx) = self.filtered(cx).get(self.sel) {
                    self.player.update(cx, |p, cx| p.enqueue(idx, cx));
                }
            }
            "e" if self.tab == Tab::Library => {
                if let Some(&idx) = self.filtered(cx).get(self.sel) {
                    self.player.update(cx, |p, cx| p.play_next(idx, cx));
                }
            }
            "c" if self.tab == Tab::Queue => {
                self.player.update(cx, |p, cx| p.clear_queue(cx));
                self.sel = 0;
                cx.notify();
            }
            "x" | "delete" if self.tab == Tab::Queue => {
                let sel = self.sel;
                self.player.update(cx, |p, cx| p.remove_from_queue(sel, cx));
                self.step(0, cx);
            }
            _ => {}
        }
    }
}

/// Copie des données nécessaires au rendu (aucun emprunt de `cx` ne survit).
struct Data {
    now: Option<library::Track>,
    playing: bool,
    shuffle: bool,
    repeat: Repeat,
    volume: u8,
    pos: f64,
    dur: f64,
    cover: Option<PathBuf>,
    lyrics: Option<Arc<Lyrics>>,
    error: Option<String>,
    scanning: bool,
    scanned: bool,
    lib_total: usize,
    queue_len: usize,
    list_len: usize,
    rows: Vec<Row>,
}

impl MusicPanel {
    fn collect(&mut self, rows_per_page: usize, cx: &Context<Self>) -> Data {
        let list_all = match self.tab {
            Tab::Library => self.filtered(cx),
            Tab::Queue | Tab::Lyrics => Vec::new(),
        };
        let p = self.player.read(cx);
        let list_len = match self.tab {
            Tab::Library => list_all.len(),
            Tab::Queue => p.queue.len(),
            Tab::Lyrics => p
                .lyrics
                .as_ref()
                .filter(|l| !l.synced)
                .map_or(0, |l| l.lines.len()),
        };
        self.sel = self.sel.min(list_len.saturating_sub(1));

        let start = (self.sel / rows_per_page) * rows_per_page;
        // Les paroles ont leur propre rendu : aucune ligne de liste à construire.
        let end = if self.tab == Tab::Lyrics {
            start
        } else {
            (start + rows_per_page).min(list_len)
        };
        let cur_idx = p.current.and_then(|c| p.queue.get(c).copied());

        let mut rows = Vec::new();
        for pos in start..end {
            let (lib_idx, current) = match self.tab {
                Tab::Library => (list_all[pos], Some(list_all[pos]) == cur_idx),
                Tab::Queue => (p.queue[pos], Some(pos) == p.current),
                Tab::Lyrics => continue,
            };
            let Some(t) = p.tracks.get(lib_idx) else { continue };
            rows.push(Row {
                pos,
                lib_idx,
                title: t.title.clone(),
                sub: format!("{} · {}", t.artist, t.album),
                secs: t.secs,
                current,
            });
        }

        Data {
            now: p.now().cloned(),
            playing: p.playing,
            shuffle: p.shuffle,
            repeat: p.repeat,
            volume: p.volume,
            pos: p.snap.position,
            dur: p.snap.duration,
            cover: p.cover.clone(),
            lyrics: p.lyrics.clone(),
            error: p.error.clone(),
            scanning: p.scanning,
            scanned: p.scanned,
            lib_total: p.tracks.len(),
            queue_len: p.queue.len(),
            list_len,
            rows,
        }
    }

    // ── Colonne gauche : disque, titre, lecture, volume ────────────────────
    fn left_column(&self, d: &Data) -> Div {
        let (title, sub) = match &d.now {
            Some(t) => (t.title.clone(), format!("{} · {}", t.artist, t.album)),
            None => (
                "Aucune lecture".to_string(),
                "Choisis un titre dans la bibliothèque".to_string(),
            ),
        };

        let frac = if d.dur > 0.0 { (d.pos / d.dur) as f32 } else { 0.0 };
        let (p_seek, dur) = (self.player.clone(), d.dur);
        let seek = disc::seek_bar(frac, move |f, cx| {
            p_seek.update(cx, |p, cx| p.seek_to(f as f64 * dur, cx));
        });

        let p_vol = self.player.clone();
        let volume = div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(ui::glyph(ui::vol_glyph(d.volume, false), 14., pal::muted()))
            .child(div().flex_1().child(ui::segments("mu-vol", d.volume, move |v, cx| {
                p_vol.update(cx, |p, cx| p.set_volume(v, cx));
            })))
            .child(
                div()
                    .w(px(40.))
                    .text_xs()
                    .text_color(pal::muted())
                    .child(format!("{}%", d.volume)),
            );

        div()
            .w(px(340.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .p_5()
            .border_r_1()
            .border_color(pal::border())
            .child(disc::cover_large(d.cover.clone()))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .text_center()
                            .text_size(px(18.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(pal::text())
                            .child(title),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .text_center()
                            .text_sm()
                            .text_color(pal::muted())
                            .child(sub),
                    ),
            )
            .children(disc::karaoke(d.lyrics.as_ref(), d.pos))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(seek)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .text_xs()
                            .text_color(pal::muted())
                            .child(fmt_time(d.pos))
                            .child(fmt_time(d.dur)),
                    ),
            )
            .child(disc::controls(&self.player, d.playing, d.shuffle, d.repeat))
            .child(volume)
            .children(
                d.error
                    .clone()
                    .map(|e| div().text_xs().text_color(pal::danger()).child(e)),
            )
    }
}

impl MusicPanel {
    fn tab_button(
        &self,
        id: &'static str,
        label: String,
        tab: Tab,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        // L'onglet actif est dit par le gras ET le soulignement, pas seulement la couleur.
        let active = self.tab == tab;
        ui::btn(id)
            .px_3()
            .h(px(30.))
            .text_sm()
            .font_weight(if active { FontWeight::BOLD } else { FontWeight::NORMAL })
            .text_color(if active { pal::text() } else { pal::muted() })
            .border_b_2()
            .border_color(if active {
                pal::accent()
            } else {
                pal::with_alpha(pal::accent(), 0.0)
            })
            .hover(|s| s.bg(pal::raised()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.switch_tab(tab, cx)))
            .child(label)
    }

    fn row_view(&self, r: &Row, cx: &Context<Self>) -> Stateful<Div> {
        let selected = r.pos == self.sel;
        let pos = r.pos;
        let idx = r.lib_idx;
        let in_queue = self.tab == Tab::Queue;
        let player = self.player.clone();

        let lead = if r.current {
            ui::glyph(icons::PLAY, 11., pal::accent()).w(px(28.))
        } else {
            div()
                .w(px(28.))
                .text_xs()
                .text_color(pal::muted())
                .child((pos + 1).to_string())
        };

        // Bibliothèque : « + » ajoute à la file. File : « × » retire (sauf le titre en cours).
        let action = if in_queue {
            (!r.current).then(|| {
                ui::btn(("mu-rm", pos))
                    .w(px(28.))
                    .hover(|s| s.bg(pal::surface()))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        player.update(cx, |p, cx| p.remove_from_queue(pos, cx));
                    })
                    .child(ui::glyph(icons::CLOSE, 10., pal::muted()))
            })
        } else {
            Some(
                ui::btn(("mu-add", pos))
                    .w(px(28.))
                    .hover(|s| s.bg(pal::surface()))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        player.update(cx, |p, cx| p.enqueue(idx, cx));
                    })
                    .child(ui::glyph(icons::PLUS, 11., pal::muted())),
            )
        };

        div()
            .id((if in_queue { "mu-q" } else { "mu-lib" }, pos))
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .h(px(44.))
            .rounded_lg()
            .cursor_pointer()
            .bg(if selected { pal::raised() } else { pal::with_alpha(pal::raised(), 0.0) })
            .border_l_2()
            .border_color(if r.current {
                pal::accent()
            } else {
                pal::with_alpha(pal::accent(), 0.0)
            })
            .hover(|s| s.bg(pal::raised()))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.sel = pos;
                this.activate(cx);
            }))
            .child(lead)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(if r.current { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                            .text_color(pal::text())
                            .child(r.title.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(pal::muted())
                            .child(r.sub.clone()),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(pal::muted())
                    .child(fmt_time(r.secs as f64)),
            )
            .children(action)
    }

    /// Onglet Paroles. Synchronisées : fenêtre centrée sur la ligne en cours
    /// (clic = aller à cette ligne). Brutes : pages, ↑/↓ pour défiler.
    fn lyrics_list(&self, d: &Data, rows_per_page: usize) -> Div {
        let Some(ly) = d.lyrics.as_ref() else {
            return self.empty_state(d);
        };
        let len = ly.lines.len();
        let rows = rows_per_page.max(1);
        let cur = ly.current(d.pos);
        let start = if ly.synced {
            cur.unwrap_or(0)
                .saturating_sub(rows / 2)
                .min(len.saturating_sub(rows))
        } else {
            (self.sel / rows) * rows
        };
        let end = (start + rows).min(len);

        let mut list = div().flex_1().min_h_0().flex().flex_col().gap(px(2.)).overflow_hidden();
        for i in start..end {
            let (stamp, text) = &ly.lines[i];
            // La ligne en cours est dite par la taille, le gras ET la couleur.
            let is_cur = Some(i) == cur;
            let player = self.player.clone();
            let mut line = div()
                .id(("mu-ly", i))
                .w_full()
                .min_w_0()
                .truncate()
                .px_3()
                .py_1()
                .rounded_lg()
                .text_size(px(if is_cur { 18. } else { 15. }))
                .font_weight(if is_cur { FontWeight::BOLD } else { FontWeight::NORMAL })
                .text_color(if is_cur {
                    pal::accent()
                } else if ly.synced {
                    pal::muted()
                } else {
                    pal::text()
                })
                .child(if text.is_empty() { "♪".to_string() } else { text.clone() });
            if let Some(secs) = *stamp {
                line = line
                    .cursor_pointer()
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(move |_, _, cx| {
                        player.update(cx, |p, cx| p.seek_to(secs, cx));
                    });
            }
            list = list.child(line);
        }
        list
    }

    fn empty_state(&self, d: &Data) -> Div {
        let (glyph, line1, line2) = if d.scanning {
            (icons::FOLDER, "Analyse du dossier…".to_string(), String::new())
        } else if self.tab == Tab::Lyrics {
            if d.now.is_none() {
                (
                    icons::MUSIC,
                    "Aucune lecture".to_string(),
                    "Lance un titre pour voir ses paroles".to_string(),
                )
            } else {
                (
                    icons::MUSIC,
                    "Pas de paroles pour ce titre".to_string(),
                    "Place un .lrc ou .txt du même nom à côté du fichier, ou des paroles dans les tags"
                        .to_string(),
                )
            }
        } else if self.tab == Tab::Queue {
            (
                icons::LIST,
                "La file d'attente est vide".to_string(),
                "Ajoute des titres avec « + » ou la touche a".to_string(),
            )
        } else if d.scanned && d.lib_total == 0 {
            (
                icons::FOLDER,
                format!("Aucune musique dans {}", library::music_dir().display()),
                "Change de dossier avec $AURORA_MUSIC_DIR ou ~/.config/aurora/music_dir".to_string(),
            )
        } else {
            (icons::SEARCH, "Aucun résultat".to_string(), String::new())
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(ui::glyph(glyph, 26., pal::raised()))
            .child(div().text_color(pal::muted()).child(line1))
            .child(div().text_xs().text_color(pal::muted()).child(line2))
    }

    fn right_column(&self, d: &Data, rows_per_page: usize, cx: &Context<Self>) -> Div {
        // L'état des paroles est dit en texte, pas seulement par la couleur.
        let lyrics_label = match &d.lyrics {
            Some(l) if l.synced => "Paroles · synchro".to_string(),
            Some(_) => "Paroles".to_string(),
            None => "Paroles · —".to_string(),
        };

        let tabs = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(self.tab_button(
                "mu-tab-lib",
                format!("Bibliothèque · {}", d.lib_total),
                Tab::Library,
                cx,
            ))
            .child(self.tab_button(
                "mu-tab-q",
                format!("File · {}", d.queue_len),
                Tab::Queue,
                cx,
            ))
            .child(self.tab_button("mu-tab-ly", lyrics_label, Tab::Lyrics, cx))
            .child(div().flex_1())
            .children((self.tab == Tab::Queue && d.queue_len > 1).then(|| {
                ui::btn("mu-clear")
                    .px_2()
                    .text_xs()
                    .text_color(pal::muted())
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.player.update(cx, |p, cx| p.clear_queue(cx));
                        this.sel = 0;
                        cx.notify();
                    }))
                    .child("Vider")
            }))
            .child(
                ui::btn("mu-rescan")
                    .w(px(30.))
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.player.update(cx, |p, cx| p.rescan(cx));
                    }))
                    .child(ui::glyph(icons::REBOOT, 12., pal::muted())),
            );

        let search_text = if self.searching {
            format!("{}▏", self.query)
        } else if !self.query.is_empty() {
            format!("{}   (Échap pour effacer)", self.query)
        } else {
            "Appuie sur / pour rechercher".to_string()
        };
        let search = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(34.))
            .px_3()
            .rounded_full()
            .bg(pal::surface())
            .border_1()
            .border_color(if self.searching { pal::accent() } else { pal::border() })
            .child(ui::glyph(icons::SEARCH, 12., pal::muted()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if self.query.is_empty() && !self.searching {
                        pal::muted()
                    } else {
                        pal::text()
                    })
                    .child(search_text),
            );

        let mut list = div().flex_1().min_h_0().flex().flex_col().gap(px(2.)).overflow_hidden();
        if self.tab == Tab::Lyrics {
            list = self.lyrics_list(d, rows_per_page);
        } else if d.rows.is_empty() {
            list = self.empty_state(d);
        } else {
            for r in &d.rows {
                list = list.child(self.row_view(r, cx));
            }
        }

        // Pagination : « 13–24 / 340 » + boutons.
        let page = self.sel / rows_per_page;
        let pages = d.list_len.div_ceil(rows_per_page).max(1);
        let start = page * rows_per_page;
        let end = (start + rows_per_page).min(d.list_len);
        let synced_lyrics =
            self.tab == Tab::Lyrics && d.lyrics.as_ref().map_or(false, |l| l.synced);
        let range = if synced_lyrics {
            "Synchronisées — clique une ligne pour t'y rendre".to_string()
        } else if d.list_len == 0 {
            "0".to_string()
        } else {
            format!("{}–{} / {}", start + 1, end, d.list_len)
        };
        let prev_sel = page.saturating_sub(1) * rows_per_page;
        let next_sel = ((page + 1) * rows_per_page).min(d.list_len.saturating_sub(1));

        let footer = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(30.))
            .child(
                ui::btn("mu-pg-prev")
                    .w(px(28.))
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.sel = prev_sel;
                        cx.notify();
                    }))
                    .child(ui::glyph(icons::PREV, 10., pal::muted())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(pal::muted())
                    .child(if synced_lyrics {
                        range
                    } else {
                        format!("{range}  ·  page {}/{pages}", page + 1)
                    }),
            )
            .child(
                ui::btn("mu-pg-next")
                    .w(px(28.))
                    .hover(|s| s.bg(pal::raised()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.sel = next_sel;
                        cx.notify();
                    }))
                    .child(ui::glyph(icons::NEXT, 10., pal::muted())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_right()
                    .text_xs()
                    .text_color(pal::muted())
                    .child("Espace ▶/⏸ · ←/→ ±5 s · ↑/↓ · Entrée · / chercher · a ajouter · e ensuite · y paroles · Tab · Échap"),
            );

        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .child(tabs)
            .children((self.tab == Tab::Library).then_some(search))
            .child(list)
            .child(footer)
    }
}

impl Render for MusicPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }

        // Parcours du dossier à la première ouverture seulement (différé : on ne
        // notifie pas une entité pendant son propre rendu).
        let need_scan = {
            let p = self.player.read(cx);
            !p.scanned && !p.scanning
        };
        if need_scan {
            let player = self.player.clone();
            cx.defer(move |cx| {
                player.update(cx, |p, cx| p.ensure_scanned(cx));
            });
        }

        // Carte à taille fixe (même moule que le lanceur : pas de hauteur
        // dérivée du viewport) : 980×620, 9 lignes par page.
        let (card_w, card_h) = (980.0, 620.0);
        let rows_per_page = 9;
        self.rows_per_page = rows_per_page;

        let data = self.collect(rows_per_page, cx);

        ui::backdrop("mu-bg", ids::MUSIC, 0.62)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| this.on_key(ev, cx)))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .occlude()
                    .w(px(card_w))
                    .h(px(card_h))
                    .flex()
                    .flex_row()
                    .rounded_2xl()
                    .bg(pal::bg_a(0.97))
                    .border_1()
                    .border_color(pal::border())
                    .shadow_lg()
                    .overflow_hidden()
                    .child(self.left_column(&data))
                    .child(self.right_column(&data, rows_per_page, cx)),
            )
    }
}
