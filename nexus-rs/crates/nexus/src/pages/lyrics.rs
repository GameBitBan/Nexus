//! Lyrics: synced lines light up and scroll along (click one to jump there); plain lyrics just show.

use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::{W, alpha};
use crate::widgets::{self, Pill};
use egui::{Align, CursorIcon, Layout, Rect, Sense, Ui, pos2, vec2};
use nexus_core::lyrics::{Lyrics, current_line, read_lyrics};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

#[derive(Default)]
pub struct LyricsState {
    cache: HashMap<String, Option<Lyrics>>,
    order: VecDeque<String>,
    shown_path: String,
    cur: Option<usize>,
    user_until: f64,
    target: Option<f32>,
}

impl LyricsState {
    /// Lyrics for a song (cached; reading is quick but not free).
    pub fn get(&mut self, path: &str) -> Option<Lyrics> {
        if !self.cache.contains_key(path) {
            let l = read_lyrics(Path::new(path));
            self.cache.insert(path.to_string(), l);
            self.order.push_back(path.to_string());
            if self.order.len() > 300 {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
        self.cache.get(path).cloned().flatten()
    }

    pub fn forget(&mut self) {
        self.cache.clear();
        self.order.clear();
    }
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let c = t.c;
    let cur = app.current_track().cloned();
    let path = cur.as_ref().map(|t| t.path.clone()).unwrap_or_default();
    let data = if path.is_empty() { None } else { app.lyrics.get(&path) };
    if app.lyrics.shown_path != path {
        app.lyrics.shown_path = path.clone();
        app.lyrics.cur = None;
        app.lyrics.user_until = 0.0;
        app.lyrics.target = Some(0.0);
    }
    let inner = Rect::from_min_max(rect.min + vec2(40.0, 26.0), rect.max - vec2(28.0, 0.0));
    let head = Rect::from_min_size(inner.min, vec2(inner.width(), 56.0));
    let p = ui.painter().clone();
    let cr = Rect::from_min_size(head.min, vec2(52.0, 52.0));
    if let Some(tr) = &cur {
        app.covers.paint(&p, &t, &tr.cover, cr);
    }
    let kind = if data.as_ref().is_some_and(|d| d.synced.is_some()) { "LYRICS · SYNCED" } else { "LYRICS" };
    region(ui, Rect::from_min_max(pos2(cr.right() + 10.0, head.top()), head.max), Layout::left_to_right(Align::Min), |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.set_width((head.width() - 160.0).max(100.0));
            widgets::eyebrow(ui, &t, kind);
            match &cur {
                Some(tr) => {
                    widgets::label(ui, &tr.title, t.font(W::Semi, 0.0), c.text, Sense::hover());
                    widgets::dim(ui, &t, &format!("{} — {}", tr.artist, tr.album));
                }
                None => {
                    widgets::label(ui, &t.l("nothing playing"), t.font(W::Semi, 0.0), c.text, Sense::hover());
                }
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if widgets::pill(ui, &t, "done", Pill::Primary).clicked() {
                app.acts.push(Act::Back);
            }
        });
    });
    let area = Rect::from_min_max(pos2(inner.left(), head.bottom() + 16.0), inner.max);
    let Some(d) = data else {
        region(ui, area, Layout::top_down(Align::Min), |ui| {
            ui.add_space(40.0);
            let msg = format!("{}\n\n{}", t.l("no lyrics for this song."), t.l("drop a .lrc file with the same name next to the song, or embed lyrics in its tags."));
            let g = ui.fonts_mut(|f| f.layout(msg, t.body(), c.dim, area.width() - 40.0));
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, c.dim);
        });
        return;
    };
    let now = ui.input(|i| i.time);
    let ms = (app.pos * 1000.0) as u64;
    let synced = d.synced.clone();
    let line_now = synced.as_ref().and_then(|s| current_line(s, ms));
    let big = t.font(W::Bold, 13.0);
    let plain_f = t.font(W::Medium, 5.0);
    let mut seek: Option<u64> = None;
    let mut scroll_to = app.lyrics.target.take();
    region(ui, area, Layout::top_down(Align::Min), |ui| {
        let mut sa = egui::ScrollArea::vertical().id_salt(("lyrics", path.as_str())).auto_shrink([false, false]);
        if line_now != app.lyrics.cur {
            app.lyrics.cur = line_now;
            if now > app.lyrics.user_until {
                scroll_to = None; // computed below from the line's position
                ui.data_mut(|dd| dd.insert_temp(egui::Id::unique("lyr-follow"), true));
            }
        }
        if let Some(y) = scroll_to {
            sa = sa.vertical_scroll_offset(y);
        }
        let follow: bool = ui.data(|dd| dd.get_temp(egui::Id::unique("lyr-follow")).unwrap_or(false));
        let out = sa.show(ui, |ui| {
            ui.set_width(area.width() - 40.0);
            ui.add_space(40.0);
            ui.spacing_mut().item_spacing.y = 14.0;
            let mut cur_y = None;
            match &synced {
                Some(lines) => {
                    for (i, (t_ms, text)) in lines.iter().enumerate() {
                        let state_c = match line_now {
                            Some(n) if n == i => c.text,
                            Some(n) if i < n => alpha(c.dim, 0.6),
                            _ => c.dim,
                        };
                        let text = if text.is_empty() { " " } else { text.as_str() };
                        let g = ui.fonts_mut(|f| f.layout(text.to_string(), big.clone(), state_c, area.width() - 80.0));
                        let (r, resp) = ui.allocate_exact_size(g.size(), Sense::click());
                        let col = if resp.hovered() { c.text } else { state_c };
                        ui.painter().galley(r.min, g, col);
                        if Some(i) == line_now {
                            cur_y = Some(r.center().y);
                        }
                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            seek = Some(*t_ms);
                        }
                    }
                }
                None => {
                    for l in d.plain.as_deref().unwrap_or("").lines() {
                        let l = if l.is_empty() { " " } else { l };
                        let g = ui.fonts_mut(|f| f.layout(l.to_string(), plain_f.clone(), c.text, area.width() - 80.0));
                        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                        ui.painter().galley(r.min, g, c.text);
                    }
                }
            }
            ui.add_space(260.0);
            cur_y
        });
        // the user scrolled: let them, follow again after a few seconds
        if ui.rect_contains_pointer(area) && ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.0) {
            app.lyrics.user_until = now + 4.0;
        }
        if follow {
            ui.data_mut(|dd| dd.insert_temp(egui::Id::unique("lyr-follow"), false));
            if let Some(y) = out.inner {
                let content_y = y - out.inner_rect.top() + out.state.clamped_offset().y;
                app.lyrics.target = Some((content_y - area.height() / 3.0).max(0.0));
                ui.ctx().request_repaint();
            }
        }
    });
    if let Some(ms) = seek {
        app.seek(ms as f64 / 1000.0);
        app.lyrics.user_until = 0.0;
        if !app.playing() {
            app.toggle_play();
        }
    }
}
