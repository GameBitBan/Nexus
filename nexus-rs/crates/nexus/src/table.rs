//! The song table used everywhere (songs, album, playlist, queue, browse): sortable columns,
//! multi-select, one-click play, right-click menu. Only the visible rows are drawn.

use crate::acts::Act;
use crate::app::Page;
use crate::covers::Covers;
use crate::theme::{Theme, W};
use crate::widgets::{elided, text_width};
use egui::{CursorIcon, Key, Rect, Sense, Ui, pos2, vec2};
use nexus_core::library::Track;
use nexus_core::util::fmt_time;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Col {
    Num,
    Title,
    Artist,
    Album,
    Year,
    Genre,
    Duration,
    Format,
}

impl Col {
    pub const ALL: [Col; 8] = [Col::Num, Col::Title, Col::Artist, Col::Album, Col::Year, Col::Genre, Col::Duration, Col::Format];

    pub fn key(self) -> &'static str {
        match self {
            Col::Num => "#",
            Col::Title => "title",
            Col::Artist => "artist",
            Col::Album => "album",
            Col::Year => "year",
            Col::Genre => "genre",
            Col::Duration => "duration",
            Col::Format => "format",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Col::Duration => "time",
            c => c.key(),
        }
    }

    pub fn from_key(k: &str) -> Option<Col> {
        Col::ALL.into_iter().find(|c| c.key() == k)
    }

    fn right(self) -> bool {
        matches!(self, Col::Num | Col::Duration | Col::Year)
    }

    fn stretch(self) -> bool {
        matches!(self, Col::Title | Col::Artist | Col::Album)
    }

    fn chars(self) -> f32 {
        match self {
            Col::Num => 5.0,
            Col::Year => 7.0,
            Col::Duration => 8.0,
            Col::Format => 13.0,
            Col::Genre => 16.0,
            _ => 10.0,
        }
    }

    fn dim(self) -> bool {
        matches!(self, Col::Num | Col::Duration | Col::Format | Col::Year | Col::Genre)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    List,
    Album,
    Playlist,
    Queue,
    Browse,
}

#[derive(Default)]
pub struct TableState {
    pub sort: Option<(Col, bool)>,
    order: Vec<usize>,
    stamp: u64,
    pub sel: BTreeSet<usize>,
    anchor: Option<usize>,
    last_click: Option<(usize, f64)>,
    pub scroll_to: Option<usize>,
    pub focus: bool,
}

impl TableState {
    pub fn reset(&mut self) {
        self.sort = None;
        self.sel.clear();
        self.anchor = None;
        self.stamp = 0;
    }

    /// The rows in the order they're shown.
    pub fn view<'a>(&self, tracks: &'a [Arc<Track>]) -> Vec<Arc<Track>> {
        self.order.iter().filter_map(|&i| tracks.get(i).cloned()).collect()
    }

    fn resort(&mut self, tracks: &[Arc<Track>], mode: Mode) {
        let mut order: Vec<usize> = (0..tracks.len()).collect();
        if let Some((col, desc)) = self.sort.filter(|_| mode != Mode::Queue) {
            let key = |t: &Track| -> SortKey {
                match col {
                    Col::Num => SortKey::N(f64::from(t.disc * 1000 + t.track)),
                    Col::Duration => SortKey::N(t.duration),
                    Col::Format => SortKey::F(t.fmt.clone(), t.bits, t.sr),
                    Col::Title => SortKey::S(t.title.to_lowercase()),
                    Col::Artist => SortKey::S(t.artist.to_lowercase()),
                    Col::Album => SortKey::S(t.album.to_lowercase()),
                    Col::Year => SortKey::S(t.year.to_lowercase()),
                    Col::Genre => SortKey::S(t.genre.to_lowercase()),
                }
            };
            let keys: Vec<SortKey> = tracks.iter().map(|t| key(t)).collect();
            order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
            if desc {
                order.reverse();
            }
        }
        self.order = order;
    }
}

#[derive(PartialEq, PartialOrd)]
enum SortKey {
    N(f64),
    S(String),
    F(String, u32, u32),
}

impl Eq for SortKey {}

impl Ord for SortKey {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.partial_cmp(o).unwrap_or(std::cmp::Ordering::Equal)
    }
}

pub struct Env<'a> {
    pub t: &'a Theme,
    pub covers: &'a mut Covers,
    pub current_path: Option<&'a str>,
    pub qi: Option<usize>,
    pub playlists: &'a [String],
    pub single_click: bool,
    pub acts: &'a mut Vec<Act>,
    /// the playlist this table shows (playlist mode)
    pub playlist: Option<&'a str>,
}

pub struct Spec<'a> {
    pub id: &'a str,
    pub tracks: &'a [Arc<Track>],
    /// changes whenever `tracks` does (re-sorts, drops the selection)
    pub stamp: u64,
    pub mode: Mode,
    pub cols: Vec<Col>,
    pub thumbs: bool,
    /// browse look: the playing row is bold instead of coloured, selection is inverted
    pub plain: bool,
}

/// Columns from the `song_columns` setting minus the hidden ones; title is always there.
pub fn columns(cfg_cols: &[String], hide: &[Col]) -> Vec<Col> {
    let mut v: Vec<Col> = cfg_cols.iter().filter_map(|k| Col::from_key(k)).filter(|c| !hide.contains(c)).collect();
    if !v.contains(&Col::Title) {
        v.insert(0, Col::Title);
    }
    v
}

pub fn show(ui: &mut Ui, env: &mut Env, st: &mut TableState, spec: Spec) {
    let t = env.t;
    let c = t.c;
    if st.stamp != spec.stamp || st.order.len() != spec.tracks.len() {
        st.stamp = spec.stamp;
        st.sel.clear();
        st.anchor = None;
        st.resort(spec.tracks, spec.mode);
    }
    let body = t.body();
    let row_font_h = ui.fonts_mut(|f| f.row_height(&body));
    let thumb = if spec.thumbs { t.list_cover_size } else { 0.0 };
    let row_h = (thumb + 8.0).max(row_font_h + 12.0).round();
    let zw = text_width(ui, "0", &body);

    // column geometry
    let full = ui.available_rect_before_wrap();
    let width = full.width() - 10.0; // room for the scrollbar
    let fixed: f32 = spec.cols.iter().filter(|c| !c.stretch()).map(|c| zw * c.chars() + 16.0).sum();
    let n_stretch = spec.cols.iter().filter(|c| c.stretch()).count().max(1) as f32;
    let stretch_w = ((width - fixed) / n_stretch).max(60.0);
    let widths: Vec<f32> = spec.cols.iter().map(|c| if c.stretch() { stretch_w } else { zw * c.chars() + 16.0 }).collect();

    // header
    let hfont = t.font(W::Semi, -1.5);
    let hh = ui.fonts_mut(|f| f.row_height(&hfont)) + 13.0;
    let (hrect, _) = ui.allocate_exact_size(vec2(full.width(), hh), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(Rect::from_min_size(pos2(hrect.left(), hrect.bottom() - 1.0), vec2(width, 1.0)), 0.0, c.border);
    let mut x = hrect.left();
    for (i, col) in spec.cols.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, hrect.top()), vec2(widths[i], hh - 1.0));
        x += widths[i];
        let mut label = t.header(col.label());
        let sortable = spec.mode != Mode::Queue;
        let resp = ui.interact(r, ui.scope_id().with((spec.id, "h", i)), if sortable { Sense::click() } else { Sense::hover() });
        if let Some((sc, desc)) = st.sort {
            if sc == *col {
                label = format!("{label} {}", if desc { "▾" } else { "▴" });
            }
        }
        let col_c = if resp.hovered() && sortable { c.text } else { c.dim };
        let g = elided(ui, &label, hfont.clone(), col_c, r.width() - 16.0);
        let pos = if col.right() { pos2(r.right() - 8.0 - g.size().x, r.center().y - g.size().y / 2.0) } else { pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0) };
        p.galley(pos, g, col_c);
        if resp.clicked() {
            st.sort = match st.sort {
                Some((sc, false)) if sc == *col => Some((*col, true)),
                Some((sc, true)) if sc == *col => None,
                _ => Some((*col, false)),
            };
            st.resort(spec.tracks, spec.mode);
            st.sel.clear();
        }
        if sortable {
            resp.on_hover_cursor(CursorIcon::PointingHand);
        }
    }

    let n = st.order.len();
    // keyboard: arrows move, enter plays (when this table was clicked last)
    if st.focus && !ui.ctx().egui_wants_keyboard_input() && n > 0 {
        let (up, down, enter, home, end) = ui.input(|i| {
            (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::Enter), i.key_pressed(Key::Home), i.key_pressed(Key::End))
        });
        let cur = st.anchor.unwrap_or(0);
        let to = if up {
            Some(cur.saturating_sub(1))
        } else if down {
            Some((cur + 1).min(n - 1))
        } else if home {
            Some(0)
        } else if end {
            Some(n - 1)
        } else {
            None
        };
        if let Some(r) = to {
            st.sel = [r].into();
            st.anchor = Some(r);
            st.scroll_to = Some(r);
        }
        if enter {
            if let Some(r) = st.anchor {
                activate(env, st, &spec, r);
            }
        }
    }

    let mut sa = egui::ScrollArea::vertical().id_salt((spec.id, "rows")).auto_shrink([false, false]);
    if let Some(r) = st.scroll_to.take() {
        let vis = ui.available_height();
        sa = sa.vertical_scroll_offset((r as f32 * row_h - vis / 2.0 + row_h / 2.0).max(0.0));
    }
    let tracks = spec.tracks;
    let mut clicked: Option<(usize, bool)> = None; // (row, secondary)
    let mut dbl: Option<usize> = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    sa.show_rows(ui, row_h, n, |ui, range| {
        let p = ui.painter().clone();
        for row in range {
            let ti = st.order[row];
            let tr = &tracks[ti];
            let (rect, resp) = ui.allocate_exact_size(vec2(full.width().min(width + 10.0), row_h), Sense::click());
            let rect = Rect::from_min_size(rect.min, vec2(width, row_h));
            let playing = match spec.mode {
                Mode::Queue => env.qi == Some(ti),
                _ => env.current_path == Some(tr.path.as_str()),
            };
            let selected = st.sel.contains(&row);
            let inverted = selected && spec.plain;
            if inverted {
                p.rect_filled(rect, t.radius.min(6.0), c.text);
            } else if selected || resp.hovered() {
                p.rect_filled(rect, 0.0, c.select);
            }
            let mut x = rect.left();
            for (i, col) in spec.cols.iter().enumerate() {
                let w = widths[i];
                let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, row_h));
                x += w;
                let mut tx = cell.left() + 8.0;
                if *col == Col::Title && thumb > 0.0 {
                    let tr_rect = Rect::from_min_size(pos2(tx, cell.center().y - thumb / 2.0), vec2(thumb, thumb));
                    env.covers.paint(&p, t, &tr.cover, tr_rect);
                    tx += thumb + 8.0;
                }
                let text = match col {
                    Col::Num => {
                        if playing {
                            t.glyph("now")
                        } else if spec.mode == Mode::Queue {
                            (ti + 1).to_string()
                        } else if tr.track > 0 {
                            tr.track.to_string()
                        } else {
                            String::new()
                        }
                    }
                    Col::Title => tr.title.clone(),
                    Col::Artist => tr.artist.clone(),
                    Col::Album => tr.album.clone(),
                    Col::Year => tr.year.clone(),
                    Col::Genre => tr.genre.clone(),
                    Col::Duration => fmt_time(tr.duration),
                    Col::Format => tr.quality(),
                };
                if text.is_empty() {
                    continue;
                }
                let color = if inverted {
                    if col.dim() { crate::theme::alpha(c.bg, 0.65) } else { c.bg }
                } else if playing && !spec.plain {
                    c.accent
                } else if *col == Col::Format && tr.is_lossy() && !spec.plain {
                    t.lossy
                } else if col.dim() && !spec.plain {
                    c.dim
                } else if col.dim() && spec.plain && !playing {
                    c.dim
                } else {
                    c.text
                };
                let font = if spec.plain && playing { t.font(W::Bold, 0.0) } else { body.clone() };
                let avail = cell.right() - 8.0 - tx;
                let g = elided(ui, &text, font, color, avail);
                let y = cell.center().y - g.size().y / 2.0;
                let pos = if col.right() { pos2(cell.right() - 8.0 - g.size().x, y) } else { pos2(tx, y) };
                p.galley(pos, g, color);
            }
            if resp.double_clicked() {
                dbl = Some(row);
            } else if resp.clicked() {
                clicked = Some((row, false));
            } else if resp.secondary_clicked() {
                clicked = Some((row, true));
                if !st.sel.contains(&row) {
                    st.sel = [row].into();
                    st.anchor = Some(row);
                }
            }
            let resp = resp.on_hover_text_at_pointer(format!(
                "{}\n{}  {}ch  ·  {}",
                tr.path,
                tr.quality(),
                tr.channels,
                if tr.is_lossy() { "lossy (compressed — not lossless)" } else { "lossless" }
            ));
            resp.context_menu(|ui| menu(ui, env, st, &spec));
        }
    });

    let now = ui.input(|i| i.time);
    let mods = ui.input(|i| i.modifiers);
    if let Some((row, secondary)) = clicked {
        st.focus = true;
        if secondary {
            if !st.sel.contains(&row) {
                st.sel = [row].into();
                st.anchor = Some(row);
            }
        } else if mods.command || mods.ctrl {
            if !st.sel.remove(&row) {
                st.sel.insert(row);
            }
            st.anchor = Some(row);
        } else if mods.shift {
            let a = st.anchor.unwrap_or(row);
            st.sel = (a.min(row)..=a.max(row)).collect();
        } else {
            st.sel = [row].into();
            st.anchor = Some(row);
            if env.single_click {
                let second_half = matches!(st.last_click, Some((r, t0)) if r == row && now - t0 < 0.5);
                st.last_click = Some((row, now));
                if !second_half {
                    activate(env, st, &spec, row);
                }
            }
        }
    }
    if let Some(row) = dbl {
        if !env.single_click {
            activate(env, st, &spec, row);
        }
    }
    // clicking anywhere else takes the keyboard focus away
    if ui.input(|i| i.pointer.any_pressed()) && clicked.is_none() && dbl.is_none() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if !full.contains(pos) {
                st.focus = false;
            }
        }
    }
}

fn activate(env: &mut Env, st: &TableState, spec: &Spec, row: usize) {
    if spec.mode == Mode::Queue {
        env.acts.push(Act::PlayQueueIndex(st.order[row]));
    } else {
        env.acts.push(Act::PlayList(st.view(spec.tracks), row));
    }
}

fn menu(ui: &mut Ui, env: &mut Env, st: &mut TableState, spec: &Spec) {
    let t = env.t;
    let rows: Vec<usize> = st.sel.iter().map(|&r| st.order[r]).collect();
    if rows.is_empty() {
        ui.close();
        return;
    }
    let sel: Vec<Arc<Track>> = rows.iter().map(|&i| spec.tracks[i].clone()).collect();
    let acts = &mut *env.acts;
    let item = |ui: &mut Ui, label: &str| ui.button(t.l(label)).clicked();
    if spec.mode != Mode::Queue && item(ui, "play") {
        acts.push(Act::PlayList(sel.clone(), 0));
    }
    if item(ui, "play next") {
        acts.push(Act::QueueNext(sel.clone()));
    }
    if item(ui, "add to queue") {
        acts.push(Act::QueueAdd(sel.clone()));
    }
    ui.menu_button(t.l("add to playlist"), |ui| {
        for name in env.playlists {
            if ui.button(name).clicked() {
                acts.push(Act::PlaylistAdd(name.clone(), sel.clone()));
            }
        }
        ui.separator();
        if ui.button(t.l("+ new playlist")).clicked() {
            acts.push(Act::PlaylistNew(sel.clone()));
        }
    });
    if spec.mode == Mode::Playlist {
        if let Some(pl) = env.playlist {
            if ui.button(t.l("remove from playlist")).clicked() {
                acts.push(Act::PlaylistRemoveRows(pl.to_string(), rows.clone()));
            }
        }
    }
    if spec.mode == Mode::Queue && ui.button(t.l("remove from queue")).clicked() {
        acts.push(Act::QueueRemove(rows.clone()));
    }
    ui.separator();
    let first = sel[0].clone();
    if ui.button(t.l("go to album")).clicked() {
        acts.push(Act::Go(Page::Album(first.album_key.clone())));
    }
    if ui.button(t.l("go to artist")).clicked() {
        acts.push(Act::Go(Page::Artists(Some(first.albumartist.clone()))));
    }
    if ui.button(t.l("inspect (spectrum)")).clicked() {
        acts.push(Act::Go(Page::Inspect(Some(first.path.clone()))));
    }
    if ui.button(t.l("edit tags & cover")).clicked() {
        acts.push(Act::Go(Page::Tags(sel.iter().map(|t| t.path.clone()).collect())));
    }
    if ui.button(t.l("open folder")).clicked() {
        let dir = std::path::Path::new(&first.path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        acts.push(Act::OpenFolder(dir));
    }
    if ui.button(t.l("copy path")).clicked() {
        acts.push(Act::CopyText(sel.iter().map(|t| t.path.clone()).collect::<Vec<_>>().join("\n")));
    }
}
