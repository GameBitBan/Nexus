//! Browse: albums on the left, the selected album's tracks on the right. Arrow keys move between them.

use super::albums::album_menu;
use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::table::{self, Col, Mode, Spec};
use crate::theme::{W, alpha};
use crate::widgets::{self, elided, galley};
use egui::{Align, CursorIcon, Key, Layout, Rect, Sense, Ui, pos2, vec2};
use nexus_core::util::fmt_time;
use std::hash::{Hash, Hasher};

fn column_head(app: &Nexus, ui: &mut Ui, title: &str, count: &str) {
    let t = &app.t;
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        widgets::eyebrow(ui, t, title);
        ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
            ui.add_space(10.0);
            let g = galley(ui, count, t.font(W::Display, 3.0), t.c.dim);
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, t.c.dim);
        });
    });
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let c = t.c;
    let albums = app.sorted_albums();
    if app.browse_album.as_ref().is_none_or(|k| app.lib.album(k).is_none()) {
        app.browse_album = albums.first().map(|&i| app.lib.albums[i].key.clone());
        app.browse_scroll = true;
    }
    let sel = app.browse_album.clone().unwrap_or_default();
    let left = Rect::from_min_size(rect.min, vec2(360.0f32.min(rect.width() * 0.45), rect.height()));
    let focus_id = egui::Id::unique("browse-albums-focus");
    let mut focused: bool = ui.data(|d| d.get_temp(focus_id).unwrap_or(false));
    let playlists = app.playlists.names();

    // keyboard on the album column
    let sel_idx = albums.iter().position(|&i| app.lib.albums[i].key == sel).unwrap_or(0);
    if focused && !ui.ctx().egui_wants_keyboard_input() && !albums.is_empty() {
        let (up, down, right, enter) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowRight), i.key_pressed(Key::Enter)));
        let to = if up { Some(sel_idx.saturating_sub(1)) } else if down { Some((sel_idx + 1).min(albums.len() - 1)) } else { None };
        if let Some(j) = to {
            app.browse_album = Some(app.lib.albums[albums[j]].key.clone());
            app.browse_scroll = true;
        }
        if enter {
            app.acts.push(Act::PlayList(app.album_tracks(&sel), 0));
        }
        if right {
            focused = false;
            app.browse_tbl.focus = true;
            app.browse_tbl.sel = [0].into();
        }
    }
    if app.browse_tbl.focus && ui.input(|i| i.key_pressed(Key::ArrowLeft)) && !ui.ctx().egui_wants_keyboard_input() {
        app.browse_tbl.focus = false;
        focused = true;
    }

    let n_albums = albums.len();
    region(ui, Rect::from_min_max(left.min + vec2(8.0, 16.0), left.max - vec2(8.0, 0.0)), Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        column_head(app, ui, "ALBUMS", &n_albums.to_string());
        let row_h = 62.0;
        let mut sa = egui::ScrollArea::vertical().id_salt("browse-albums").auto_shrink([false, false]);
        if app.browse_scroll {
            app.browse_scroll = false;
            let vis = ui.available_height();
            let i = albums.iter().position(|&i| app.lib.albums[i].key == sel).unwrap_or(0);
            sa = sa.vertical_scroll_offset((i as f32 * row_h - vis / 2.0 + row_h / 2.0).max(0.0));
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        sa.show_rows(ui, row_h, n_albums, |ui, range| {
            for i in range {
                let a = app.lib.albums[albums[i]].clone();
                let (full, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                let r = Rect::from_min_max(full.min + vec2(4.0, 2.0), full.max - vec2(4.0, 2.0));
                let is_sel = a.key == sel;
                let hov = resp.hovered();
                let p = ui.painter().clone();
                let rad = t.radius.min(9.0);
                if is_sel || hov {
                    p.rect_filled(r, rad, if is_sel { c.text } else { c.select });
                }
                let tc = if is_sel { c.bg } else { c.text };
                let sc = if is_sel { alpha(c.bg, 0.65) } else { c.dim };
                let ts = 46.0;
                let thumb = Rect::from_min_size(pos2(r.left() + 8.0, r.top() + (r.height() - ts) / 2.0), vec2(ts, ts));
                app.covers.paint(&p, &t, &a.cover, thumb);
                let x = thumb.right() + 12.0;
                let tw = r.right() - x - 8.0;
                let body = t.body();
                let fh = ui.fonts_mut(|f| f.row_height(&body));
                let tg = elided(ui, &a.title, t.font(W::Semi, 0.0), tc, tw);
                p.galley(pos2(x, r.top() + 10.0), tg, tc);
                let q = a.tracks.first().map(|&ti| app.lib.tracks[ti].quality()).unwrap_or_default();
                let sub: Vec<&str> = [a.artist.as_str(), a.year.as_str(), q.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
                let sg = elided(ui, &sub.join(" · "), body, sc, tw);
                p.galley(pos2(x, r.top() + 10.0 + fh + 2.0), sg, sc);
                let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                if resp.clicked() || resp.secondary_clicked() {
                    app.browse_album = Some(a.key.clone());
                    focused = true;
                    app.browse_tbl.focus = false;
                }
                if resp.double_clicked() {
                    app.acts.push(Act::PlayList(app.album_tracks(&a.key), 0));
                }
                album_menu(app, &resp, &a.key, &a.artist, &playlists, true);
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(focus_id, focused));
    widgets::vline_at(ui, &t, left.right(), rect.top(), rect.bottom());

    // tracks of the selected album
    let tracks = app.album_tracks(&sel);
    let multi = {
        let mut a: Vec<&str> = tracks.iter().map(|t| t.artist.as_str()).collect();
        a.sort_unstable();
        a.dedup();
        a.len() > 1
    };
    let total: f64 = tracks.iter().map(|t| t.duration).sum();
    let right = Rect::from_min_max(pos2(left.right() + 1.0 + 8.0, rect.top() + 16.0), rect.max - vec2(8.0, 0.0));
    let mut hide = vec![Col::Album, Col::Year, Col::Genre, Col::Format];
    if !multi {
        hide.push(Col::Artist);
    }
    let cols = table::columns(&app.cfg.strings("song_columns"), &hide);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (app.lib_ver, &sel).hash(&mut h);
    let stamp = h.finish();
    region(ui, right, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let count = if tracks.is_empty() { String::new() } else { format!("{} · {}", tracks.len(), fmt_time(total)) };
        column_head(app, ui, "TRACKS", &count);
        let names = playlists.clone();
        let current = app.current_path.clone();
        let mut env = table::Env {
            t: &t,
            covers: &mut app.covers,
            current_path: current.as_deref(),
            qi: app.qi,
            playlists: &names,
            single_click: app.cfg.get("single_click_play").and_then(|v| v.as_bool()).unwrap_or(true),
            acts: &mut app.acts,
            playlist: None,
        };
        table::show(ui, &mut env, &mut app.browse_tbl, Spec { id: "browse-tracks", tracks: &tracks, stamp, mode: Mode::Browse, cols, thumbs: false, plain: true });
    });
}
