//! Album page and playlist page: big cover, title, buttons, the tracks.

use crate::acts::{Act, Tracks};
use crate::app::{Nexus, Page};
use crate::layout::region;
use crate::table::{self, Col, Mode, Spec};
use crate::theme::W;
use crate::widgets::{self, Pill};
use egui::{Align, Layout, Rect, Sense, Ui, pos2, vec2};
use nexus_core::util::fmt_time;
use std::hash::{Hash, Hasher};

pub fn album(app: &mut Nexus, ui: &mut Ui, rect: Rect, key: &str) {
    let Some(a) = app.lib.album(key).cloned() else {
        app.go(Page::Albums, false);
        return;
    };
    let tracks = app.album_tracks(key);
    let total: f64 = tracks.iter().map(|t| t.duration).sum();
    let q = tracks.first().map(|t| t.quality()).unwrap_or_default();
    let n = format!("{} tracks", tracks.len());
    let tt = fmt_time(total);
    let meta: Vec<&str> = [a.artist.as_str(), a.year.as_str(), n.as_str(), tt.as_str(), q.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    let meta = meta.join("  ·  ");
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (app.lib_ver, key).hash(&mut h);
    page(app, ui, rect, Kind::Album, &a.title, &meta, &a.cover, tracks, h.finish());
}

pub fn playlist(app: &mut Nexus, ui: &mut Ui, rect: Rect, name: &str) {
    if !app.playlists.contains(name) {
        app.go(Page::Albums, false);
        return;
    }
    let tracks = app.playlist_tracks(name);
    let total: f64 = tracks.iter().map(|t| t.duration).sum();
    let meta = format!("{} tracks  ·  {}", tracks.len(), fmt_time(total));
    let cover = app.playlist_cover(name);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (app.lib_ver, app.pl_ver, name).hash(&mut h);
    page(app, ui, rect, Kind::Playlist, name, &meta, &cover, tracks, h.finish());
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Album,
    Playlist,
}

#[allow(clippy::too_many_arguments)]
fn page(app: &mut Nexus, ui: &mut Ui, rect: Rect, kind: Kind, title: &str, meta: &str, cover: &str, tracks: Tracks, stamp: u64) {
    let t = app.t.clone();
    let c = t.c;
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 0.0));
    let s = (t.cover_size * 1.4).min(inner.width() * 0.4);
    let head = Rect::from_min_size(inner.min, vec2(inner.width(), s));
    let cr = Rect::from_min_size(head.min, vec2(s, s));
    let p = ui.painter().clone();
    app.covers.paint(&p, &t, cover, cr);
    let info = Rect::from_min_max(pos2(cr.right() + 28.0, head.top()), head.max);
    region(ui, info, Layout::bottom_up(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        // bottom-up: buttons first
        let semi = t.font(W::Semi, 0.0);
        let pill_h = ui.fonts_mut(|f| f.row_height(&semi)) + 14.0;
        let mut labels = vec!["play", "shuffle", "add to queue", "edit tags"];
        if kind == Kind::Playlist {
            labels.extend(["rename", "delete"]);
        }
        let mut pad = 18.0;
        while pad > 8.0 && widgets::pills_width(ui, &t, &labels, pad, 8.0) > info.width() {
            pad -= 2.0;
        }
        let gap = if widgets::pills_width(ui, &t, &labels, pad, 8.0) > info.width() { 4.0 } else { 8.0 };
        ui.allocate_ui_with_layout(vec2(info.width(), pill_h), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if widgets::pill_pad(ui, &t, "play", Pill::Primary, pad).clicked() {
                app.acts.push(Act::PlayList(tracks.clone(), 0));
            }
            if widgets::pill_pad(ui, &t, "shuffle", Pill::Normal, pad).clicked() {
                app.acts.push(Act::PlayShuffled(tracks.clone()));
            }
            if widgets::pill_pad(ui, &t, "add to queue", Pill::Normal, pad).clicked() {
                app.acts.push(Act::QueueAdd(tracks.clone()));
            }
            if widgets::pill_pad(ui, &t, "edit tags", Pill::Normal, pad).clicked() {
                app.acts.push(Act::Go(Page::Tags(tracks.iter().map(|t| t.path.clone()).collect())));
            }
            if kind == Kind::Playlist {
                if widgets::pill_pad(ui, &t, "rename", Pill::Normal, pad).clicked() {
                    app.acts.push(Act::PlaylistRename(title.to_string()));
                }
                if widgets::pill_pad(ui, &t, "delete", Pill::Normal, pad).clicked() {
                    app.acts.push(Act::PlaylistDelete(title.to_string()));
                }
            }
        });
        ui.add_space(14.0);
        let mg = ui.fonts_mut(|f| f.layout(meta.to_string(), t.body(), c.dim, info.width()));
        let (mr, _) = ui.allocate_exact_size(mg.size(), Sense::hover());
        ui.painter().galley(mr.min, mg, c.dim);
        let renaming = kind == Kind::Playlist && app.rename.as_deref().is_some();
        if renaming {
            rename_box(app, ui, title, info.width());
        } else {
            let big = t.font(W::Display, 27.0);
            let g = ui.fonts_mut(|f| f.layout(title.to_string(), big, c.text, info.width()));
            let (tr, resp) = ui.allocate_exact_size(g.size(), if kind == Kind::Playlist { Sense::click() } else { Sense::hover() });
            ui.painter().galley(tr.min, g, c.text);
            if kind == Kind::Playlist && resp.on_hover_cursor(egui::CursorIcon::Text).clicked() {
                app.acts.push(Act::PlaylistRename(title.to_string()));
            }
        }
        widgets::eyebrow(ui, &t, if kind == Kind::Album { "ALBUM" } else { "PLAYLIST" });
    });
    let rest = Rect::from_min_max(pos2(inner.left(), head.bottom() + 22.0), inner.max);
    let (mode, hide, thumbs) = match kind {
        Kind::Album => (Mode::Album, vec![Col::Album], false),
        Kind::Playlist => (Mode::Playlist, vec![], true),
    };
    let cols = table::columns(&app.cfg.strings("song_columns"), &hide);
    let names = app.playlists.names();
    let current = app.current_path.clone();
    let single = app.cfg.get("single_click_play").and_then(|v| v.as_bool()).unwrap_or(true);
    region(ui, rest, Layout::top_down(Align::Min), |ui| {
        let mut env = table::Env {
            t: &t,
            covers: &mut app.covers,
            current_path: current.as_deref(),
            qi: app.qi,
            playlists: &names,
            single_click: single,
            acts: &mut app.acts,
            playlist: if kind == Kind::Playlist { Some(title) } else { None },
        };
        table::show(ui, &mut env, &mut app.coll_tbl, Spec { id: "collection", tracks: &tracks, stamp, mode, cols, thumbs, plain: false });
    });
}

/// Rename right on the page: the title becomes a text field.
fn rename_box(app: &mut Nexus, ui: &mut Ui, title: &str, width: f32) {
    let t = app.t.clone();
    let id = egui::Id::unique("rename-playlist");
    let mut text = match &app.rename {
        Some(s) if s == title => title.to_string(),
        Some(s) => s.clone(),
        None => return,
    };
    let buf_id = id.with("buf");
    let mut buf: String = if app.rename_focus { text.clone() } else { ui.data(|d| d.get_temp(buf_id)).unwrap_or_else(|| text.clone()) };
    let big = t.font(W::Display, 27.0);
    let resp = ui.add(
        egui::TextEdit::singleline(&mut buf)
            .id(id)
            .font(big)
            .text_color(t.c.text)
            .frame(egui::Frame::NONE)
            .desired_width(width)
            .margin(egui::Margin::ZERO),
    );
    let r = resp.rect;
    ui.painter().rect_filled(Rect::from_min_size(pos2(r.left(), r.bottom()), vec2(r.width(), 2.0)), 0.0, t.c.text);
    if app.rename_focus {
        app.rename_focus = false;
        resp.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(buf.chars().count()));
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), id);
        }
    }
    let done = resp.lost_focus();
    ui.data_mut(|d| d.insert_temp(buf_id, buf.clone()));
    if done {
        ui.data_mut(|d| d.remove::<String>(buf_id));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        app.rename = None;
        if !esc {
            text = buf.trim().to_string();
            if !text.is_empty() && text != title {
                if app.playlists.contains(&text) {
                    app.toast(format!("there's already a playlist called “{text}”"));
                    app.rename = Some(title.to_string());
                    app.rename_focus = true;
                } else {
                    app.playlist_rename_to(title, &text);
                }
            }
        }
    }
}
