//! Discover: your top artists, similar artists you don't have yet (with 30-second previews), albums
//! you're missing from artists you do have, and albums in your library to rediscover.

use crate::acts::Act;
use crate::app::{Nexus, Page};
use crate::layout::region;
use crate::netimg::NetImages;
use crate::theme::{Theme, W, alpha};
use crate::widgets::{self, Pill, elided};
use crossbeam_channel::{Receiver, bounded};
use egui::{Align, Color32, CursorIcon, Layout, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, pos2, vec2};
use nexus_core::audio::{Engine, Event, PlaybackOptions};
use nexus_core::discover::{self, DiscoverData, Rec};
use nexus_core::util::fmt_time;
use parking_lot::Mutex;
use std::sync::Arc;

type PreviewResult = Result<(String, String, String), String>;

#[derive(Default)]
pub struct DiscoverState {
    data: Option<DiscoverData>,
    loaded: bool,
    job: Option<Receiver<Result<DiscoverData, String>>>,
    progress: Arc<Mutex<String>>,
    status: String,
    sel: Option<i64>,
    rediscover: Option<Vec<(String, u64)>>,
    imgs: Option<NetImages>,
    // previews
    engine: Option<Engine>,
    playing: String,
    loading: String,
    resume: bool,
    file: Option<std::path::PathBuf>,
    rx: Option<Receiver<PreviewResult>>,
    /// "artist|album" → what the Soulseek fetch is doing
    pub dl: std::collections::HashMap<String, Arc<Mutex<String>>>,
}

impl Nexus {
    fn discover_refresh(&mut self) {
        let st = &mut self.discover;
        if st.job.is_some() {
            return;
        }
        if self.lib.tracks.is_empty() {
            st.status = "add some music first".into();
            return;
        }
        st.status = "finding music for you…".into();
        let (lib, stats, fav) = (self.lib.clone(), self.stats.clone(), self.cfg.strings("fav_artists"));
        let progress = st.progress.clone();
        let (tx, rx) = bounded(1);
        let ctx = self.ctx.clone();
        std::thread::Builder::new()
            .name("nexus-discover".into())
            .spawn(move || {
                let c2 = ctx.clone();
                let res = discover::run(&lib, &stats, &fav, &|s| {
                    *progress.lock() = s;
                    c2.request_repaint();
                });
                let _ = tx.send(res);
                ctx.request_repaint();
            })
            .expect("spawn discover");
        st.job = Some(rx);
    }

    /// ▶ on a song: fetch a fresh preview link + the clip, pause your music, play it.
    fn preview_toggle(&mut self, key: String, url: String, tid: Option<i64>, label: String) {
        if self.discover.playing == key || self.discover.loading == key {
            self.preview_stop();
            return;
        }
        if url.is_empty() && tid.is_none() {
            self.toast("no preview for that one");
            return;
        }
        self.discover.loading = key.clone();
        let (tx, rx) = bounded(1);
        let ctx = self.ctx.clone();
        let dir = nexus_core::paths::data_dir().join("discover_img");
        std::thread::Builder::new()
            .name("nexus-preview".into())
            .spawn(move || {
                let url = tid.and_then(discover::fresh_preview).unwrap_or(url);
                let res = nexus_core::net::get_bytes(&url, 15, 8_000_000).and_then(|d| {
                    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                    let f = dir.join(format!("preview-{stamp}.mp3"));
                    std::fs::write(&f, d).map_err(|e| e.to_string())?;
                    Ok((key, f.to_string_lossy().to_string(), label))
                });
                let _ = tx.send(res);
                ctx.request_repaint();
            })
            .expect("spawn preview");
        self.discover.rx = Some(rx);
    }

    pub fn preview_stop(&mut self) {
        let st = &mut self.discover;
        if let Some(e) = &st.engine {
            e.stop();
        }
        let was = !st.playing.is_empty();
        st.playing.clear();
        st.loading.clear();
        st.rx = None;
        if let Some(f) = st.file.take() {
            let _ = std::fs::remove_file(f);
        }
        if was && std::mem::take(&mut st.resume) {
            self.engine.play();
        }
    }

    /// Every frame: preview finished? left the page?
    pub fn discover_tick(&mut self) {
        if let Some(rx) = &self.discover.rx {
            if let Ok(res) = rx.try_recv() {
                self.discover.rx = None;
                match res {
                    Ok((key, path, label)) if self.discover.loading == key => {
                        self.discover.loading.clear();
                        if self.discover.playing.is_empty() {
                            self.discover.resume = self.playing();
                            if self.discover.resume {
                                self.engine.pause();
                            }
                        }
                        let dev = self.cfg.s("audio_device");
                        let eng = self.discover.engine.get_or_insert_with(|| {
                            let e = Engine::new(PlaybackOptions { device: dev, gapless: false, ..Default::default() });
                            e.set_volume(0.64);
                            e
                        });
                        eng.load(&path, 0.0, true);
                        if let Some(old) = self.discover.file.replace(path.into()) {
                            let _ = std::fs::remove_file(old);
                        }
                        self.discover.playing = key;
                        let text = if label.is_empty() { "previewing".to_string() } else { format!("previewing {label}") };
                        self.toast_action(text, "stop", |app| app.preview_stop(), 31.0);
                    }
                    Ok((_, path, _)) => {
                        let _ = std::fs::remove_file(path);
                    }
                    Err(e) => {
                        self.discover.loading.clear();
                        self.toast(format!("couldn't get the preview: {e}"));
                    }
                }
            }
        }
        if let Some(e) = &self.discover.engine {
            let ended = e.poll().iter().any(|ev| matches!(ev, Event::Ended | Event::Error(_)));
            if ended && !self.discover.playing.is_empty() {
                self.preview_stop();
            }
            if !self.discover.playing.is_empty() {
                self.ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        if self.page != Page::Discover {
            if !self.discover.playing.is_empty() || !self.discover.loading.is_empty() {
                self.preview_stop();
            }
            self.discover.rediscover = None;
        }
    }
}

fn paint_pic(ui: &Ui, t: &Theme, tex: Option<&TextureHandle>, rect: Rect, round: bool) {
    let rad = if round { rect.width() / 2.0 } else { 10.0f32.min(t.cover_radius(rect.width()).max(4.0)) };
    match tex {
        Some(tex) => {
            let s = tex.size_vec2();
            // centre-crop to a square
            let uv = if s.x > s.y {
                let k = (s.y / s.x) / 2.0;
                Rect::from_min_max(pos2(0.5 - k, 0.0), pos2(0.5 + k, 1.0))
            } else {
                let k = (s.x / s.y) / 2.0;
                Rect::from_min_max(pos2(0.0, 0.5 - k), pos2(1.0, 0.5 + k))
            };
            ui.painter().add(egui::epaint::RectShape::filled(rect, rad, Color32::WHITE).with_texture(tex.id(), uv));
        }
        None => {
            ui.painter().rect_filled(rect, rad, t.c.select);
        }
    }
}

fn line(ui: &Ui, text: &str, font: egui::FontId, color: Color32, at: egui::Pos2, w: f32, center: bool) -> f32 {
    let g = elided(ui, text, font, color, w);
    let h = g.size().y;
    let x = if center { at.x + (w - g.size().x) / 2.0 } else { at.x };
    ui.painter().galley(pos2(x, at.y), g, color);
    h
}

fn small_btn(ui: &mut Ui, t: &Theme, text: &str, rect: Rect, id: egui::Id) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let c = t.c;
    let hover = resp.hovered();
    let rad = if t.pills { rect.height() / 2.0 } else { t.radius.min(8.0) };
    ui.painter().rect(rect, rad, if hover { c.select } else { Color32::TRANSPARENT }, Stroke::new(1.0, if hover { c.dim } else { c.border }), StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, t.font(W::Semi, -1.0), c.text);
    resp
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    {
        let st = &mut app.discover;
        if st.imgs.is_none() {
            st.imgs = Some(NetImages::new(&ctx));
        }
        if let Some(i) = &mut st.imgs {
            i.pump(&ctx);
        }
        if !st.loaded {
            st.loaded = true;
            st.data = discover::load();
        }
        if let Some(rx) = &st.job {
            if let Ok(res) = rx.try_recv() {
                st.job = None;
                match res {
                    Ok(d) => {
                        st.data = Some(d);
                        st.status.clear();
                    }
                    Err(e) => st.status = format!("couldn't refresh: {e}").chars().take(120).collect(),
                }
            } else {
                st.status = st.progress.lock().clone();
            }
        }
    }
    let stale = app.discover.data.as_ref().is_none_or(|d| discover::age_secs(d) > 3.0 * 86400.0);
    if stale && app.discover.job.is_none() && !app.lib.tracks.is_empty() && app.discover.status.is_empty() {
        app.discover_refresh();
    }
    if app.discover.rediscover.is_none() {
        app.discover.rediscover = Some(pick_rediscover(app));
    }
    let t = app.t.clone();
    let c = t.c;
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(20.0, 0.0));
    let mut act: Option<Box<dyn FnOnce(&mut Nexus)>> = None;
    region(ui, inner, Layout::top_down(Align::Min), |ui| {
        egui::ScrollArea::vertical().id_salt("discover").auto_shrink([false, false]).show(ui, |ui| {
            let w = inner.width() - 16.0;
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = 10.0;
            // header
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    widgets::eyebrow(ui, &t, &t.l("FOR YOU"));
                    widgets::label(ui, &t.l("Discover"), t.font(W::Display, 27.0), c.text, Sense::hover());
                });
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    let busy = app.discover.job.is_some();
                    if ui.add_enabled_ui(!busy, |ui| widgets::pill(ui, &t, "refresh", Pill::Normal)).inner.clicked() {
                        act = Some(Box::new(|a: &mut Nexus| a.discover_refresh()));
                    }
                    if widgets::pill(ui, &t, "import from Spotify…", Pill::Normal).on_hover_text(t.l("bring playlists, liked songs and artists over from Spotify, Apple Music and others")).clicked() {
                        act = Some(Box::new(|a: &mut Nexus| a.go(Page::Import, true)));
                    }
                    let status = if !app.discover.status.is_empty() {
                        app.discover.status.clone()
                    } else {
                        match &app.discover.data {
                            Some(d) => {
                                let h = (discover::age_secs(d) / 3600.0) as u64;
                                if h > 0 { format!("updated {h} h ago") } else { "just updated".into() }
                            }
                            None => "hit refresh to get recommendations".into(),
                        }
                    };
                    let g = elided(ui, &t.l(&status), t.body(), c.dim, (ui.available_width() - 8.0).clamp(40.0, 300.0));
                    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(r.min, g, c.dim);
                });
            });
            let note = t.l("Based on the artists in your library and what you play most. Similar artists come from Deezer (only artist names are sent). ▶ plays a 30-second preview.");
            let g = ui.fonts_mut(|f| f.layout(note, t.font(W::Regular, -1.5), c.dim, w));
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, c.dim);
            if let Some(a) = sections(app, ui, &t, w) {
                act = Some(a);
            }
            if let Some(a) = rediscover(app, ui, &t) {
                act = Some(a);
            }
            ui.add_space(24.0);
        });
    });
    if let Some(f) = act {
        f(app);
    }
}

fn eyebrow_gap(ui: &mut Ui, t: &Theme, text: &str) {
    ui.add_space(14.0);
    widgets::eyebrow(ui, t, &t.l(text).to_uppercase());
}

type ActFn = Box<dyn FnOnce(&mut Nexus)>;

fn sections(app: &mut Nexus, ui: &mut Ui, t: &Theme, w: f32) -> Option<ActFn> {
    let c = t.c;
    let data = app.discover.data.clone()?;
    let mut act: Option<ActFn> = None;
    let imgs = app.discover.imgs.as_mut()?;
    // ---- your top artists
    if !data.seeds.is_empty() {
        eyebrow_gap(ui, t, "your top artists");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(18.0, 14.0);
            for s in data.seeds.iter().take(12) {
                let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 122.0), Sense::click());
                let pic = Rect::from_min_size(pos2(rect.center().x - 38.0, rect.top()), vec2(76.0, 76.0));
                paint_pic(ui, t, imgs.get(&s.picture).as_ref(), pic, true);
                if resp.hovered() {
                    ui.painter().circle_stroke(pic.center(), 38.0, Stroke::new(1.5, c.dim));
                }
                let y = pic.bottom() + 6.0;
                let h = line(ui, &s.name, t.body(), c.text, pos2(rect.left(), y), 96.0, true);
                let sub = if s.plays > 0 { format!("{} plays", s.plays) } else { format!("{} album{}", s.albums, if s.albums == 1 { "" } else { "s" }) };
                line(ui, &sub, t.font(W::Regular, -1.5), c.dim, pos2(rect.left(), y + h + 2.0), 96.0, true);
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    let a = s.name.clone();
                    act = Some(Box::new(move |app: &mut Nexus| app.go(Page::Browse(Some(a)), true)));
                }
            }
        });
    }
    // ---- detail of the selected recommendation
    if let Some(r) = app.discover.sel.and_then(|id| data.recs.iter().find(|r| r.id == id)) {
        if let Some(a) = detail(app, ui, t, r, w) {
            act = Some(a);
        }
    }
    let imgs = app.discover.imgs.as_mut()?;
    // ---- artists you might like
    if !data.recs.is_empty() {
        eyebrow_gap(ui, t, "artists you might like");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
            for r in &data.recs {
                let (rect, _) = ui.allocate_exact_size(vec2(190.0, 268.0), Sense::hover());
                ui.painter().rect(rect, t.radius.max(8.0), c.panel, Stroke::new(1.0, c.border), StrokeKind::Inside);
                let pic = Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(166.0, 166.0));
                paint_pic(ui, t, imgs.get(&r.picture).as_ref(), pic, false);
                let mut y = pic.bottom() + 8.0;
                y += line(ui, &r.name, t.font(W::Semi, 0.0), c.text, pos2(pic.left(), y), 166.0, false) + 2.0;
                line(ui, &format!("like {}", r.because.join(", ")), t.font(W::Regular, -1.5), c.dim, pos2(pic.left(), y), 166.0, false);
                let by = rect.bottom() - 40.0;
                let top = r.top.first().cloned().unwrap_or_default();
                let key = top.tid.map(|x| x.to_string()).unwrap_or_else(|| top.preview.clone());
                let on = !key.is_empty() && (app.discover.playing == key || app.discover.loading == key);
                let pb = Rect::from_min_size(pos2(pic.left(), by), vec2(40.0, 28.0));
                if small_btn(ui, t, if on { "■" } else { "▶" }, pb, ui.scope_id().with(("recplay", r.id))).on_hover_text(t.l("30-second preview")).clicked() {
                    let label = format!("{} — {}", r.name, top.title);
                    act = Some(Box::new(move |app: &mut Nexus| app.preview_toggle(key, top.preview, top.tid, label)));
                }
                let open = app.discover.sel == Some(r.id);
                let sb = Rect::from_min_size(pos2(pb.right() + 6.0, by), vec2(70.0, 28.0));
                if small_btn(ui, t, &t.l(if open { "close" } else { "songs" }), sb, ui.scope_id().with(("recsongs", r.id))).clicked() {
                    let id = r.id;
                    act = Some(Box::new(move |app: &mut Nexus| app.discover.sel = if app.discover.sel == Some(id) { None } else { Some(id) }));
                }
            }
        });
    }
    // ---- albums you don't have yet
    if !data.missing.is_empty() {
        eyebrow_gap(ui, t, "albums you don't have yet — from artists you do");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
            for a in &data.missing {
                let (rect, resp) = ui.allocate_exact_size(vec2(170.0, 212.0), Sense::click());
                ui.painter().rect(rect, t.radius.max(8.0), c.panel, Stroke::new(1.0, if resp.hovered() { c.dim } else { c.border }), StrokeKind::Inside);
                let pic = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(150.0, 150.0));
                paint_pic(ui, t, imgs.get(&a.cover).as_ref(), pic, false);
                let mut y = pic.bottom() + 6.0;
                y += line(ui, &a.title, t.font(W::Semi, 0.0), c.text, pos2(pic.left(), y), 150.0, false) + 2.0;
                let kind = if a.kind != "album" { a.kind.as_str() } else { "" };
                let sub: Vec<&str> = [a.artist.as_str(), a.year.as_str(), kind].into_iter().filter(|s| !s.is_empty()).collect();
                let dkey = format!("{}|{}", a.artist, a.title).to_lowercase();
                let state = app.discover.dl.get(&dkey).map(|s| s.lock().clone());
                match &state {
                    Some(s) => {
                        line(ui, &format!("⤓ {s}"), t.font(W::Regular, -1.5), c.text, pos2(pic.left(), y), 150.0, false);
                    }
                    None => {
                        line(ui, &sub.join(" · "), t.font(W::Regular, -1.5), c.dim, pos2(pic.left(), y), 150.0, false);
                    }
                }
                if resp.hovered() && state.is_none() {
                    ui.painter().rect_filled(pic, 8.0, alpha(Color32::BLACK, 0.35));
                    ui.painter().text(pic.center(), egui::Align2::CENTER_CENTER, "⤓", t.font(W::Bold, 14.0), Color32::WHITE);
                }
                let tip = format!("download “{}” from Soulseek into your library · right-click to copy the name", a.title);
                let resp = resp.on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand);
                if resp.secondary_clicked() {
                    app.acts.push(Act::CopyText(format!("{} — {}", a.artist, a.title)));
                    app.acts.push(Act::Toast("copied".into()));
                } else if resp.clicked() && state.as_ref().is_none_or(|s| !s.starts_with("searching") && !s.starts_with("downloading")) {
                    let (artist, album) = (a.artist.clone(), a.title.clone());
                    let cover = discover::big_cover(if a.cover_big.is_empty() { &a.cover } else { &a.cover_big });
                    let status = Arc::new(Mutex::new(String::new()));
                    app.discover.dl.insert(dkey, status.clone());
                    act = Some(Box::new(move |app: &mut Nexus| app.auto_download(&artist, &album, &cover, false, Some(status))));
                }
            }
        });
    }
    act
}

fn detail(app: &mut Nexus, ui: &mut Ui, t: &Theme, r: &Rec, w: f32) -> Option<ActFn> {
    let c = t.c;
    let mut act: Option<ActFn> = None;
    ui.add_space(10.0);
    egui::Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)).corner_radius(t.radius.max(8.0)).inner_margin(egui::Margin::symmetric(18, 16)).show(ui, |ui| {
        ui.set_width(w - 36.0);
        let imgs = app.discover.imgs.as_mut().expect("images");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let (pic, _) = ui.allocate_exact_size(vec2(92.0, 92.0), Sense::hover());
            paint_pic(ui, t, imgs.get(&r.picture).as_ref(), pic, true);
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 120.0).max(100.0));
                ui.add_space(18.0);
                widgets::label(ui, &r.name, t.font(W::Display, 11.0), c.text, Sense::hover());
                let fans = r.fans.to_string().as_bytes().rchunks(3).rev().map(|c| std::str::from_utf8(c).unwrap_or("")).collect::<Vec<_>>().join(",");
                widgets::dim(ui, t, &format!("{fans} fans on Deezer  ·  because you have {}", r.because.join(", ")));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::pill(ui, t, "close", Pill::Normal).clicked() {
                    act = Some(Box::new(|a: &mut Nexus| a.discover.sel = None));
                }
            });
        });
        ui.add_space(6.0);
        for (i, tr) in r.top.iter().enumerate() {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
            let mut x = row.left();
            let cy = row.center().y;
            ui.painter().text(pos2(x, cy), egui::Align2::LEFT_CENTER, format!("{:02}", i + 1), t.font(W::Semi, -1.0), c.dim);
            x += 30.0;
            paint_pic(ui, t, imgs.get(&tr.cover).as_ref(), Rect::from_min_size(pos2(x, cy - 18.0), vec2(36.0, 36.0)), false);
            x += 46.0;
            let rest = row.right() - x - 110.0;
            let g = elided(ui, &tr.title, t.body(), c.text, rest * 0.5 - 10.0);
            ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g, c.text);
            let g = elided(ui, &tr.album, t.body(), c.dim, rest * 0.5 - 10.0);
            ui.painter().galley(pos2(x + rest * 0.5, cy - g.size().y / 2.0), g, c.dim);
            ui.painter().text(pos2(row.right() - 60.0, cy), egui::Align2::RIGHT_CENTER, fmt_time(tr.duration), t.font(W::Regular, -1.0), c.dim);
            let key = tr.tid.map(|x| x.to_string()).unwrap_or_else(|| tr.preview.clone());
            let on = !key.is_empty() && (app.discover.playing == key || app.discover.loading == key);
            let pb = Rect::from_min_size(pos2(row.right() - 44.0, cy - 14.0), vec2(40.0, 28.0));
            if small_btn(ui, t, if on { "■" } else { "▶" }, pb, ui.scope_id().with(("toprow", r.id, i))).clicked() {
                let (url, tid, label) = (tr.preview.clone(), tr.tid, format!("{} — {}", r.name, tr.title));
                act = Some(Box::new(move |app: &mut Nexus| app.preview_toggle(key, url, tid, label)));
            }
        }
    });
    act
}

/// Albums you haven't played in a month (or ever): favourites you've drifted from first.
fn pick_rediscover(app: &Nexus) -> Vec<(String, u64)> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut cands: Vec<(String, u64)> = app
        .lib
        .albums
        .iter()
        .filter_map(|a| {
            let tracks: Vec<&str> = a.tracks.iter().map(|&i| app.lib.tracks[i].path.as_str()).collect();
            let last = tracks.iter().map(|p| app.stats.last(p)).max().unwrap_or(0);
            let plays: u64 = tracks.iter().map(|p| app.stats.plays(p)).sum();
            (now.saturating_sub(last) > 30 * 86400).then(|| (a.key.clone(), plays))
        })
        .collect();
    fastrand::shuffle(&mut cands);
    cands.sort_by_key(|(_, p)| std::cmp::Reverse((*p).min(5)));
    cands.truncate(8);
    cands
}

fn rediscover(app: &mut Nexus, ui: &mut Ui, t: &Theme) -> Option<ActFn> {
    let c = t.c;
    let picks = app.discover.rediscover.clone().unwrap_or_default();
    if picks.is_empty() {
        return None;
    }
    let mut act: Option<ActFn> = None;
    eyebrow_gap(ui, t, "rediscover in your library");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
        for (key, plays) in &picks {
            let Some(a) = app.lib.album(key) else { continue };
            let (title, artist, cover) = (a.title.clone(), a.artist.clone(), a.cover.clone());
            let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 186.0), Sense::click());
            ui.painter().rect(rect, t.radius.max(8.0), c.panel, Stroke::new(1.0, if resp.hovered() { c.dim } else { c.border }), StrokeKind::Inside);
            let pic = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(134.0, 134.0));
            let p = ui.painter().clone();
            app.covers.paint(&p, t, &cover, pic);
            let mut y = pic.bottom() + 5.0;
            y += line(ui, &title, t.font(W::Semi, 0.0), c.text, pos2(pic.left(), y), 134.0, false) + 1.0;
            let sub = format!("{artist} · {}", if *plays > 0 { format!("{plays} plays") } else { "never played".into() });
            line(ui, &sub, t.font(W::Regular, -1.5), c.dim, pos2(pic.left(), y), 134.0, false);
            if resp.hovered() && resp.hover_pos().is_some_and(|h| h.y < pic.bottom()) {
                ui.painter().rect_filled(pic, t.cover_radius(134.0), alpha(Color32::BLACK, 0.25));
                ui.painter().circle_filled(pic.center(), 20.0, c.text);
                ui.painter().text(pic.center() + vec2(1.5, 0.0), egui::Align2::CENTER_CENTER, "▶", t.font(W::Semi, 2.0), c.bg);
            }
            let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(t.l("click the cover to play · click the name to open"));
            if resp.clicked() {
                let k = key.clone();
                let on_cover = resp.interact_pointer_pos().is_some_and(|p| p.y < pic.bottom());
                act = Some(Box::new(move |app: &mut Nexus| {
                    if on_cover {
                        let tracks = app.album_tracks(&k);
                        app.play_list(tracks, 0);
                    } else {
                        app.go(Page::Album(k), true);
                    }
                }));
            }
        }
    });
    act
}
