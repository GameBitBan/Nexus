//! Import: playlists, liked songs and followed artists from Spotify / Apple Music / CSV / M3U exports.

use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::{Theme, W};
use crate::widgets::{self, Pill};
use crossbeam_channel::{Receiver, bounded};
use egui::{Align, Layout, Rect, Sense, Stroke, Ui, vec2};
use nexus_core::imports::{self, ImportResult, Item, Matcher, Missing};
use std::path::PathBuf;

struct Loaded {
    res: ImportResult,
    matcher: Matcher,
    rows: Vec<(String, Vec<Item>, usize)>,
    missing: Vec<Missing>,
}

#[derive(Default)]
pub struct ImportState {
    job: Option<Receiver<Result<Loaded, String>>>,
    loaded: Option<Loaded>,
    checks: Vec<bool>,
    use_artists: bool,
    show_missing: bool,
    miss_checks: Vec<bool>,
    /// albums waiting to be fetched from Soulseek, one at a time
    dl_queue: std::collections::VecDeque<Missing>,
    dl_active: Option<(Missing, std::sync::Arc<parking_lot::Mutex<String>>)>,
    dl_next_at: f64,
    dl_done: usize,
    dl_line: String,
    status: String,
}

impl Nexus {
    /// Export files picked or dropped on the import page.
    pub fn import_load(&mut self, files: Vec<PathBuf>) {
        let st = &mut self.import;
        st.status = "reading…".into();
        let tracks = self.lib.tracks.clone();
        let lib = self.lib.clone();
        let (tx, rx) = bounded(1);
        let ctx = self.ctx.clone();
        std::thread::Builder::new()
            .name("nexus-import".into())
            .spawn(move || {
                let mut res = ImportResult { source: "files".into(), ..Default::default() };
                let mut err = None;
                for f in &files {
                    let name = f.to_string_lossy().to_string();
                    match std::fs::read(f).map_err(|e| e.to_string()).and_then(|d| imports::parse_import(&name, &d)) {
                        Ok(r) => res.merge(r),
                        Err(e) => err = Some(e),
                    }
                }
                let out = if res.is_empty() && err.is_some() {
                    Err(err.unwrap_or_default())
                } else {
                    let matcher = Matcher::new(&tracks);
                    let (rows, missing) = imports::summarize(&res, &matcher, &lib);
                    Ok(Loaded { res, matcher, rows, missing })
                };
                let _ = tx.send(out);
                ctx.request_repaint();
            })
            .expect("spawn import");
        st.job = Some(rx);
    }

    fn import_pick(&mut self) {
        let start = nexus_core::paths::home_dir().join("Downloads");
        let start = if start.is_dir() { start } else { nexus_core::paths::home_dir() };
        if let Some(files) = rfd::FileDialog::new()
            .set_title("import from Spotify / Apple Music / other players")
            .set_directory(start)
            .add_filter("exports", &["zip", "json", "xml", "csv", "txt", "tsv", "m3u", "m3u8"])
            .add_filter("all files", &["*"])
            .pick_files()
        {
            self.import_load(files);
        }
    }

    fn import_apply(&mut self) {
        let Some(l) = &self.import.loaded else { return };
        let chosen: Vec<(String, Vec<Item>)> =
            l.rows.iter().zip(&self.import.checks).filter(|(_, on)| **on).map(|((n, items, _), _)| (n.clone(), items.clone())).collect();
        let made = imports::apply(&chosen, &l.res.source, &l.matcher, &mut self.playlists);
        let artists = l.res.artists.clone();
        self.save_playlists();
        let mut msg = format!("made {made} playlist{}", if made == 1 { "" } else { "s" });
        if !artists.is_empty() && self.import.use_artists {
            let mut fav = self.cfg.strings("fav_artists");
            for a in &artists {
                if !fav.contains(a) {
                    fav.push(a.clone());
                }
            }
            fav.truncate(2000);
            self.cfg.set("fav_artists", serde_json::json!(fav));
            self.cfg.save();
            msg += &format!(" · Discover now knows your {} artists", artists.len());
        }
        self.import.status = format!("{msg}.");
        self.toast(msg);
    }

    fn import_get_missing(&mut self) {
        let Some(l) = &self.import.loaded else { return };
        let chosen: Vec<Missing> = l.missing.iter().enumerate().filter(|(i, _)| self.import.miss_checks.get(*i).copied().unwrap_or(true)).map(|(_, m)| m.clone()).collect();
        if chosen.is_empty() {
            return;
        }
        let have: std::collections::HashSet<(String, String)> = self.import.dl_queue.iter().map(|m| (m.artist.clone(), m.album.clone())).collect();
        let n0 = self.import.dl_queue.len();
        self.import.dl_queue.extend(chosen.into_iter().filter(|m| !have.contains(&(m.artist.clone(), m.album.clone()))));
        let n = self.import.dl_queue.len() + usize::from(self.import.dl_active.is_some());
        if n > n0 {
            self.toast_action(format!("getting {n} albums one by one — you can keep using Nexus"), "downloads", |a| a.go(crate::app::Page::Download, true), 6.0);
        }
    }

    /// Every frame: fetch the queued missing albums one after another.
    pub fn import_dl_tick(&mut self, now: f64) {
        let st = &mut self.import;
        if let Some((m, status)) = &st.dl_active {
            let s = status.lock().clone();
            if s.is_empty() || s.starts_with("searching") {
                self.ctx.request_repaint_after(std::time::Duration::from_millis(500));
                return;
            }
            st.dl_line = format!("{} — {}: {s}", m.artist, m.album);
            st.dl_done += 1;
            st.dl_active = None;
            st.dl_next_at = now + 1.5;
        }
        if st.dl_queue.is_empty() {
            if st.dl_done > 0 && !st.dl_line.starts_with("done looking") {
                st.dl_line = format!("done looking: {} album{} handled. playlists fill in as downloads land in your library.", st.dl_done, if st.dl_done == 1 { "" } else { "s" });
            }
            return;
        }
        if now < st.dl_next_at {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        let Some(m) = st.dl_queue.pop_front() else { return };
        let status = std::sync::Arc::new(parking_lot::Mutex::new("searching Soulseek…".to_string()));
        st.dl_line = format!("looking for {} — {}  ({} more after this)", m.artist, m.album, st.dl_queue.len());
        st.dl_active = Some((m.clone(), status.clone()));
        self.auto_download(&m.artist, &m.album, "", true, Some(status));
        if self.import.dl_active.as_ref().is_some_and(|(_, s)| s.lock().starts_with("set up")) {
            // no login: stop
            self.import.dl_queue.clear();
        }
    }

    /// After a rescan: songs that arrived go into the playlists they were imported for.
    pub fn import_rematch(&mut self) {
        let n = imports::rematch(&self.lib.tracks, &mut self.playlists);
        if n > 0 {
            self.save_playlists();
            self.toast(format!("{n} new song{} added to your imported playlists", if n == 1 { "" } else { "s" }));
        }
    }
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    if let Some(rx) = &app.import.job {
        if let Ok(out) = rx.try_recv() {
            app.import.job = None;
            match out {
                Err(e) => app.import.status = format!("couldn't read that: {e}"),
                Ok(l) if l.res.is_empty() => {
                    app.import.status = "didn't find any songs or playlists in that. for spotify pick the zip they emailed; for apple music the Library.xml.".into();
                }
                Ok(l) => {
                    app.import.checks = l.rows.iter().map(|(_, _, hit)| *hit > 0).collect();
                    app.import.use_artists = true;
                    app.import.show_missing = false;
                    app.import.miss_checks = vec![true; l.missing.len()];
                    app.import.status = "pick which playlists to make, then “make playlists”. missing songs get added to them later, when they show up in your library.".into();
                    app.import.loaded = Some(l);
                }
            }
        }
    }
    let t = app.t.clone();
    let c = t.c;
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 0.0));
    let mut act = 0;
    region(ui, inner, Layout::top_down(Align::Min), |ui| {
        egui::ScrollArea::vertical().id_salt("import-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(inner.width() - 12.0);
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    widgets::eyebrow(ui, &t, &t.l("BRING YOUR MUSIC OVER"));
                    widgets::label(ui, &t.l("Import"), t.font(W::Display, 27.0), c.text, Sense::hover());
                });
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    if widgets::pill(ui, &t, "choose files…", Pill::Primary).clicked() {
                        act = 1;
                    }
                });
            });
            let status = if app.import.status.is_empty() {
                "drop an export here, or choose files. nothing is uploaded anywhere — it's all read on your computer.".to_string()
            } else {
                app.import.status.clone()
            };
            wrapped(ui, &t, &t.l(&status), t.body(), c.dim);
            match &app.import.loaded {
                None => howto(ui, &t),
                Some(_) => act = results(app, ui, &t).max(act),
            }
            ui.add_space(20.0);
        });
    });
    match act {
        1 => app.import_pick(),
        2 => app.import_apply(),
        3 => app.import_get_missing(),
        _ => {}
    }
}

fn wrapped(ui: &mut Ui, _t: &Theme, text: &str, font: egui::FontId, color: egui::Color32) {
    let w = ui.available_width();
    let g = ui.fonts_mut(|f| f.layout(text.to_string(), font, color, w));
    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(r.min, g, color);
}

fn howto(ui: &mut Ui, t: &Theme) {
    let c = t.c;
    let guides = [
        (
            "Spotify",
            "playlists, liked songs, followed artists",
            "spotify.com → account → privacy settings → “download your data” → tick “account data” and request. Spotify emails you a zip (can take a few days). Pick that zip here as-is.\n\nFaster for single playlists: any CSV export tool (Exportify, TuneMyMusic, Soundiiz).",
        ),
        (
            "Apple Music",
            "playlists, loved songs",
            "Windows: in iTunes or the Apple Music app, File → Library → Export Library… and pick the Library.xml it saves.\n\nOr privacy.apple.com → “get a copy of your data” → Apple Media Services; pick the zip you get.",
        ),
        (
            "Anything else",
            "YouTube Music, Tidal, Deezer, other players",
            "CSV/TXT exports with title and artist columns (Google Takeout's YouTube Music files, TuneMyMusic, Soundiiz…), and .m3u / .m3u8 playlists from other players (Harmonoid, foobar2000, MusicBee, VLC).",
        ),
    ];
    let w = ui.available_width();
    let cols = if w > 760.0 { 3 } else { 1 };
    let gap = 14.0;
    let card_w = (w - gap * (cols - 1) as f32) / cols as f32;
    ui.add_space(4.0);
    for chunk in guides.chunks(cols) {
        // measure so the cards in a row share a height
        let pad = 18.0;
        let heights: Vec<f32> = chunk
            .iter()
            .map(|(_, _, how)| {
                let g = ui.fonts_mut(|f| f.layout(t.l(how), t.body(), c.dim, card_w - pad * 2.0));
                g.size().y + 70.0
            })
            .collect();
        let h = heights.iter().cloned().fold(0.0f32, f32::max);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (name, what, how) in chunk {
                let (rect, _) = ui.allocate_exact_size(vec2(card_w, h), Sense::hover());
                let p = ui.painter();
                p.rect(rect, t.radius.max(8.0), c.panel, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
                let mut y = rect.top() + 16.0;
                let g = ui.fonts_mut(|f| f.layout_no_wrap(name.to_string(), t.font(W::Display, 11.0), c.text));
                let gh = g.size().y;
                p.galley(egui::pos2(rect.left() + pad, y), g, c.text);
                y += gh + 4.0;
                let g = ui.fonts_mut(|f| f.layout_no_wrap(t.l(what).to_uppercase(), t.font(W::Semi, -1.5), c.dim));
                let gh = g.size().y;
                p.galley(egui::pos2(rect.left() + pad, y), g, c.dim);
                y += gh + 8.0;
                let g = ui.fonts_mut(|f| f.layout(t.l(how), t.body(), c.dim, card_w - pad * 2.0));
                p.galley(egui::pos2(rect.left() + pad, y), g, c.dim);
            }
        });
    }
}

/// Returns 2 when "make playlists" was clicked.
fn results(app: &mut Nexus, ui: &mut Ui, t: &Theme) -> usize {
    let c = t.c;
    let st = &mut app.import;
    let Some(l) = &st.loaded else { return 0 };
    let src = if l.res.source == "files" { "your files".to_string() } else { l.res.source.clone() };
    let mut bits = Vec::new();
    let n = |k: usize, w: &str| format!("{k} {w}{}", if k == 1 { "" } else { "s" });
    if !l.res.playlists.is_empty() {
        bits.push(n(l.res.playlists.len(), "playlist"));
    }
    if !l.res.liked.is_empty() {
        bits.push(n(l.res.liked.len(), "liked song"));
    }
    if !l.res.artists.is_empty() {
        bits.push(n(l.res.artists.len(), "artist"));
    }
    if !l.res.albums.is_empty() {
        bits.push(n(l.res.albums.len(), "saved album"));
    }
    ui.add_space(4.0);
    wrapped(ui, t, &format!("{src}: {}", bits.join(", ")), t.font(W::Display, 11.0), c.text);
    let all: usize = l.rows.iter().map(|(_, i, _)| i.len()).sum();
    let hit: usize = l.rows.iter().map(|(_, _, h)| h).sum();
    let found = if all > 0 {
        format!("{hit} of {all} songs are already in your library ({}%). {} missing.", hit * 100 / all, n(l.missing.len(), "album"))
    } else {
        format!("{} you don't have yet.", n(l.missing.len(), "album"))
    };
    wrapped(ui, t, &t.l(&found), t.body(), c.dim);
    let mut act = 0;
    if !l.rows.is_empty() {
        widgets::eyebrow(ui, t, &t.l("PLAYLISTS TO MAKE"));
        egui::Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)).corner_radius(t.radius.max(8.0)).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            egui::ScrollArea::vertical().id_salt("import-lists").max_height(260.0).show(ui, |ui| {
                for (i, (name, items, hit)) in l.rows.iter().enumerate() {
                    if let Some(on) = st.checks.get_mut(i) {
                        ui.checkbox(on, format!("{name}   ·   {hit} / {} in your library", items.len()));
                    }
                }
            });
        });
    }
    if !l.res.artists.is_empty() {
        ui.checkbox(&mut st.use_artists, t.l("use your followed artists for Discover"));
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let any = st.checks.iter().any(|b| *b);
        if ui.add_enabled_ui(any || !l.res.artists.is_empty(), |ui| widgets::pill(ui, t, "make playlists", Pill::Primary)).inner.clicked() {
            act = 2;
        }
        if !l.missing.is_empty() {
            let label = if st.show_missing { "hide missing" } else { "show missing" };
            if widgets::pill(ui, t, label, Pill::Normal).clicked() {
                st.show_missing = !st.show_missing;
            }
            if widgets::pill(ui, t, "get missing albums", Pill::Normal)
                .on_hover_text(t.l("finds the albums you don't have on Soulseek and downloads them (lossless if settings → downloads says so). the playlists fill in as they arrive."))
                .clicked()
            {
                act = 3;
            }
            if widgets::pill(ui, t, "copy list", Pill::Normal).on_hover_text(t.l("copies “artist — album” lines, e.g. for a shopping list")).clicked() {
                let text: Vec<String> = l.missing.iter().map(|m| format!("{} — {}", m.artist, m.album)).collect();
                app.acts.push(Act::CopyText(text.join("\n")));
            }
        }
    });
    if st.show_missing && !l.missing.is_empty() {
        widgets::eyebrow(ui, t, &t.l("MISSING ALBUMS"));
        egui::Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)).corner_radius(t.radius.max(8.0)).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt("import-missing").max_height(300.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (i, m) in l.missing.iter().take(1000).enumerate() {
                    let songs = if m.songs > 0 { format!("   ·   {} song{}", m.songs, if m.songs == 1 { "" } else { "s" }) } else { String::new() };
                    if let Some(on) = st.miss_checks.get_mut(i) {
                        ui.checkbox(on, format!("{}  —  {}{songs}", m.artist, m.album));
                    }
                }
            });
        });
        widgets::small_dim(ui, t, &t.l("untick any you don't want, then “get missing albums”."));
    }
    if !st.dl_line.is_empty() {
        widgets::small_dim(ui, t, &t.l(&st.dl_line));
    }
    act
}
