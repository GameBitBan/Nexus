//! The album grid (home), and the artists page of the classic layout.

use crate::acts::Act;
use crate::app::{Nexus, Page};
use crate::layout::{list_item, region};
use crate::theme::W;
use crate::widgets::{self, elided};
use egui::{Align, Color32, CursorIcon, Layout, Rect, Sense, Ui, pos2, vec2};

pub fn albums_page(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let inner = Rect::from_min_max(rect.min + vec2(22.0, 10.0), rect.max - vec2(8.0, 0.0));
    if app.lib.tracks.is_empty() {
        empty(app, ui, inner);
        return;
    }
    let albums = app.sorted_albums();
    let keys: Vec<String> = albums.iter().map(|&i| app.lib.albums[i].key.clone()).collect();
    grid(app, ui, inner, &keys, "albums-grid");
}

fn empty(app: &mut Nexus, ui: &mut Ui, r: Rect) {
    let t = app.t.clone();
    region(ui, r, Layout::top_down(Align::Center), |ui| {
        let text = t.l("No music yet.\nPoint me at the folder with your FLAC files.");
        let g = ui.fonts_mut(|f| f.layout(text, t.body(), t.c.dim, r.width() - 40.0));
        ui.add_space(((r.height() - g.size().y - 60.0) / 2.0).max(0.0));
        let (gr, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        ui.painter().galley(gr.min, g, t.c.dim);
        ui.add_space(10.0);
        if widgets::pill(ui, &t, "add music folder", widgets::Pill::Primary).clicked() {
            app.acts.push(Act::AddFolder);
        }
        if !app.scan_label.is_empty() {
            ui.add_space(8.0);
            let sl = app.scan_label.clone();
            widgets::small_dim(ui, &t, &sl);
        }
    });
}

/// A scrolling grid of album covers. Only the visible rows are drawn.
pub fn grid(app: &mut Nexus, ui: &mut Ui, rect: Rect, keys: &[String], id: &str) {
    let t = app.t.clone();
    let c = t.c;
    let s = t.cover_size;
    let body = t.body();
    let fh = ui.fonts_mut(|f| f.row_height(&body));
    let (cw, ch) = (s + 28.0, s + fh * 2.0 + 34.0);
    let playing_album = app.current_track().map(|t| t.album_key.clone());
    let playlists = app.playlists.names();
    region(ui, rect, Layout::top_down(Align::Min), |ui| {
        let width = ui.available_width() - 10.0;
        let cols = ((width / cw) as usize).max(1);
        let rows = keys.len().div_ceil(cols);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show_rows(ui, ch, rows, |ui, range| {
            for row in range {
                let (rr, _) = ui.allocate_exact_size(vec2(ui.available_width(), ch), Sense::hover());
                for col in 0..cols {
                    let i = row * cols + col;
                    let Some(key) = keys.get(i) else { break };
                    let Some(a) = app.lib.album(key).cloned() else { continue };
                    let cell = Rect::from_min_size(pos2(rr.left() + col as f32 * cw, rr.top()), vec2(cw, ch));
                    let cover = Rect::from_min_size(pos2(cell.left() + (cw - s) / 2.0, cell.top() + 10.0), vec2(s, s));
                    let resp = ui.interact(cell, egui::Id::unique((id, key.as_str())), Sense::click());
                    let p = ui.painter().clone();
                    app.covers.paint(&p, &t, &a.cover, cover);
                    let hot = resp.hovered();
                    let d = (cover.width() / 4.0).clamp(34.0, 48.0);
                    let play_r = Rect::from_min_size(pos2(cover.right() - d - 9.0, cover.bottom() - d - 9.0), vec2(d, d));
                    if hot {
                        p.rect_filled(cover, t.cover_radius(s), Color32::from_black_alpha(70));
                        p.circle_filled(play_r.center(), d / 2.0, Color32::WHITE);
                        let (cx, cy) = (play_r.center().x + 0.5, play_r.center().y + 0.5);
                        p.add(egui::Shape::convex_polygon(
                            vec![pos2(cx - d * 0.12, cy - d * 0.18), pos2(cx - d * 0.12, cy + d * 0.18), pos2(cx + d * 0.2, cy)],
                            Color32::BLACK,
                            egui::Stroke::NONE,
                        ));
                    }
                    if playing_album.as_deref() == Some(a.key.as_str()) {
                        p.circle_filled(pos2(cover.left() + 15.0, cover.top() + 15.0), 7.0, c.bg);
                        p.circle_filled(pos2(cover.left() + 15.0, cover.top() + 15.0), 3.5, c.accent);
                    }
                    let tg = elided(ui, &a.title, t.font(W::Medium, 0.0), c.text, s);
                    p.galley(pos2(cover.left(), cover.bottom() + 8.0 + (fh - tg.size().y) / 2.0), tg, c.text);
                    let sub: Vec<&str> = [a.artist.as_str(), a.year.as_str()].into_iter().filter(|x| !x.is_empty()).collect();
                    let sg = elided(ui, &sub.join(" · "), body.clone(), c.dim, s);
                    p.galley(pos2(cover.left(), cover.bottom() + 8.0 + fh + (fh - sg.size().y) / 2.0), sg, c.dim);
                    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                    if resp.clicked() {
                        let on_play = resp.interact_pointer_pos().is_some_and(|p| play_r.contains(p));
                        if on_play {
                            app.acts.push(Act::PlayList(app.album_tracks(key), 0));
                        } else {
                            app.acts.push(Act::Go(Page::Album(key.clone())));
                        }
                    }
                    album_menu(app, &resp, key, &a.artist, &playlists, false);
                }
            }
        });
    });
}

/// Right-click menu for an album (grid, browse column).
pub fn album_menu(app: &mut Nexus, resp: &egui::Response, key: &str, artist: &str, playlists: &[String], browse: bool) {
    let t = app.t.clone();
    let lib = app.lib.clone();
    let acts = &mut app.acts;
    resp.context_menu(|ui| {
        let tracks: crate::acts::Tracks = lib.album(key).map(|a| a.tracks.iter().map(|&i| lib.tracks[i].clone()).collect()).unwrap_or_default();
        if ui.button(t.l("play")).clicked() {
            acts.push(Act::PlayList(tracks.clone(), 0));
        }
        if ui.button(t.l("shuffle")).clicked() {
            acts.push(Act::PlayShuffled(tracks.clone()));
        }
        if ui.button(t.l("play next")).clicked() {
            acts.push(Act::QueueNext(tracks.clone()));
        }
        if ui.button(t.l("add to queue")).clicked() {
            acts.push(Act::QueueAdd(tracks.clone()));
        }
        ui.menu_button(t.l("add to playlist"), |ui| {
            for name in playlists {
                if ui.button(name).clicked() {
                    acts.push(Act::PlaylistAdd(name.clone(), tracks.clone()));
                }
            }
            ui.separator();
            if ui.button(t.l("+ new playlist")).clicked() {
                acts.push(Act::PlaylistNew(tracks.clone()));
            }
        });
        if browse && ui.button(t.l("open album page")).clicked() {
            acts.push(Act::Go(Page::Album(key.to_string())));
        }
        if ui.button(t.l("edit album tags & cover")).clicked() {
            acts.push(Act::Go(Page::Tags(tracks.iter().map(|t| t.path.clone()).collect())));
        }
        if !browse && ui.button(t.l("go to artist")).clicked() {
            acts.push(Act::Go(Page::Artists(Some(artist.to_string()))));
        }
    });
}

/// Classic layout: artists on the left, their albums on the right.
pub fn artists_page(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let list_w = 240.0f32.min(rect.width() * 0.35);
    let lr = Rect::from_min_size(rect.min, vec2(list_w, rect.height()));
    let artists: Vec<(String, usize)> = app.lib.artists.iter().map(|(n, a)| (n.clone(), a.len())).collect();
    if app.artist_sel.as_ref().is_none_or(|s| !artists.iter().any(|(n, _)| n == s)) {
        app.artist_sel = artists.first().map(|(n, _)| n.clone());
    }
    region(ui, lr.shrink2(vec2(6.0, 8.0)), Layout::top_down(Align::Min), |ui| {
        egui::ScrollArea::vertical().id_salt("artists-list").auto_shrink([false, false]).show(ui, |ui| {
            for (name, n) in &artists {
                let on = app.artist_sel.as_deref() == Some(name.as_str());
                if list_item(ui, &t, &format!(" {name}  ({n})"), on).clicked() {
                    app.artist_sel = Some(name.clone());
                }
            }
        });
    });
    widgets::vline_at(ui, &t, lr.right(), rect.top(), rect.bottom());
    let sel = app.artist_sel.clone().unwrap_or_default();
    let mut keys: Vec<String> = Vec::new();
    for i in app.sorted_albums() {
        let a = &app.lib.albums[i];
        if a.artist == sel {
            keys.push(a.key.clone());
        }
    }
    let gr = Rect::from_min_max(pos2(lr.right() + 1.0, rect.top()), rect.max);
    grid(app, ui, gr, &keys, "artist-grid");
}
