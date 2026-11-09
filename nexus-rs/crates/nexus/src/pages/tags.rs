//! Tag editor: one song, a selection or a whole album. Nothing is written until save.

use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::{Theme, W, alpha};
use crate::widgets::{self, Pill, elided};
use crossbeam_channel::{Receiver, bounded};
use egui::{Align, ColorImage, CursorIcon, Key, Layout, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, pos2, vec2};
use nexus_core::library::Track;
use nexus_core::tags::{self, AlbumInfo, Cover, FIELDS, Tags};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hasher;
use std::path::Path;
use std::sync::Arc;

const MULTI: &str = "‹multiple values›";
/// (field, header, fixed width or 0 = stretch)
const COLS: [(&str, &str, f32); 7] = [
    ("tracknumber", "#", 54.0),
    ("discnumber", "disc", 50.0),
    ("title", "title", 0.0),
    ("artist", "artist", 0.0),
    ("album", "album", 0.0),
    ("date", "year", 64.0),
    ("_file", "file", 0.0),
];

fn hash(d: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write(d);
    h.finish()
}

/// "saved {} file" → "saved 3 files"
fn plural(n: usize, pattern: &str) -> String {
    pattern.replace("{}", &n.to_string()) + if n == 1 { "" } else { "s" }
}

fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

pub struct Orig {
    tags: Tags,
    emb: Option<u64>,
    folder: Option<u64>,
}

impl Orig {
    fn new(tags: Tags) -> Self {
        Orig { emb: tags.embedded.as_deref().map(hash), folder: tags.folder.as_deref().map(hash), tags }
    }
}

#[derive(Clone)]
struct NewCover {
    data: Arc<Vec<u8>>,
    mime: &'static str,
    hash: u64,
}

#[derive(PartialEq, Clone, Copy, Default)]
enum Bar {
    #[default]
    None,
    Replace,
    Rename,
}

enum Online {
    Searching(String),
    Found(AlbumInfo, Option<(Arc<Vec<u8>>, u32, u32)>),
    Nothing,
}

type LoadResult = (Vec<(String, Orig)>, Vec<String>);
type OnlineResult = Option<(AlbumInfo, Option<(Vec<u8>, u32, u32)>)>;

struct SaveResult {
    ok: Vec<String>,
    bad: Vec<String>,
    fresh: Vec<(String, Orig)>,
    covers_changed: HashSet<String>,
    tracks: Vec<Track>,
}

#[derive(Default)]
pub struct TagState {
    requested: Vec<String>,
    paths: Vec<String>,
    orig: HashMap<String, Orig>,
    pending: HashMap<String, BTreeMap<String, String>>,
    covers_new: HashMap<String, Option<NewCover>>,
    sel: BTreeSet<usize>,
    anchor: Option<usize>,
    form: HashMap<String, String>,
    multi: HashSet<String>,
    refill: bool,
    load_rx: Option<Receiver<LoadResult>>,
    save_rx: Option<Receiver<SaveResult>>,
    online_rx: Option<Receiver<OnlineResult>>,
    online: Option<Online>,
    status: String,
    shrink: bool,
    bar: Bar,
    rep_field: usize,
    rep_find: String,
    rep_with: String,
    pattern: Option<String>,
    /// cover preview: hash → texture (None if it didn't decode), info line
    tex: Option<(u64, Option<TextureHandle>, String)>,
    /// inline cell edit: row, column, text, needs focus
    edit: Option<(usize, usize, String, bool)>,
}

impl TagState {
    pub fn dirty(&self) -> bool {
        self.pending.values().any(|m| !m.is_empty()) || !self.covers_new.is_empty()
    }

    fn files_changed(&self) -> usize {
        self.paths.iter().filter(|p| self.pending.get(*p).is_some_and(|m| !m.is_empty()) || self.covers_new.contains_key(*p)).count()
    }

    fn value(&self, p: &str, k: &str) -> String {
        if let Some(v) = self.pending.get(p).and_then(|m| m.get(k)) {
            return v.clone();
        }
        self.orig.get(p).map(|o| o.tags.get(k).to_string()).unwrap_or_default()
    }

    fn orig_value(&self, p: &str, k: &str) -> String {
        self.orig.get(p).map(|o| o.tags.get(k).to_string()).unwrap_or_default()
    }

    fn selected(&self) -> Vec<String> {
        self.sel.iter().filter_map(|&r| self.paths.get(r).cloned()).collect()
    }

    fn load(&mut self, paths: &[String], ctx: &egui::Context) -> bool {
        let discarded = self.dirty();
        *self = TagState { shrink: self.shrink, bar: self.bar, pattern: self.pattern.take(), rep_field: self.rep_field, ..Default::default() };
        self.requested = paths.to_vec();
        let paths: Vec<String> = paths.iter().filter(|p| Path::new(p).exists()).cloned().collect();
        self.status = format!("reading {} files…", paths.len());
        let (tx, rx) = bounded(1);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("nexus-tags-read".into())
            .spawn(move || {
                let mut out = Vec::new();
                let mut bad = Vec::new();
                for p in paths {
                    match tags::read_tags(Path::new(&p), true) {
                        Ok(t) => out.push((p, Orig::new(t))),
                        Err(e) => bad.push(format!("{}: {e}", file_name(&p))),
                    }
                }
                let _ = tx.send((out, bad));
                ctx.request_repaint();
            })
            .expect("spawn tag reader");
        self.load_rx = Some(rx);
        discarded
    }

    fn poll(&mut self) -> Option<SaveResult> {
        if let Some(rx) = &self.load_rx {
            if let Ok((list, bad)) = rx.try_recv() {
                self.load_rx = None;
                self.paths = list.iter().map(|(p, _)| p.clone()).collect();
                self.orig = list.into_iter().collect();
                self.status = if bad.is_empty() { String::new() } else { format!("couldn't read {} files · {}", bad.len(), bad[0]) };
                self.sel = (0..self.paths.len()).collect();
                self.anchor = Some(0);
                self.refill = true;
            }
        }
        if let Some(rx) = &self.online_rx {
            if let Ok(res) = rx.try_recv() {
                self.online_rx = None;
                self.online = Some(match res {
                    Some((info, cov)) => Online::Found(info, cov.map(|(d, w, h)| (Arc::new(d), w, h))),
                    None => Online::Nothing,
                });
            }
        }
        if let Some(rx) = &self.save_rx {
            if let Ok(res) = rx.try_recv() {
                self.save_rx = None;
                return Some(res);
            }
        }
        None
    }

    fn refill_form(&mut self) {
        let sel = self.selected();
        self.multi.clear();
        for (k, _) in FIELDS {
            let vals: BTreeSet<String> = sel.iter().map(|p| self.value(p, k)).collect();
            if vals.len() > 1 {
                self.multi.insert(k.to_string());
            }
            self.form.insert(k.to_string(), if vals.len() == 1 { vals.into_iter().next().unwrap_or_default() } else { String::new() });
        }
        self.refill = false;
    }

    fn field_edited(&mut self, k: &str, text: &str) {
        let v = if k == "comment" || k == "lyrics" { text.to_string() } else { text.trim().to_string() };
        for p in self.selected() {
            self.pending.entry(p).or_default().insert(k.to_string(), v.clone());
        }
        self.multi.remove(k);
        self.after_edit();
    }

    /// Drops edits that put things back how they were.
    fn after_edit(&mut self) {
        let orig = &self.orig;
        for (p, m) in self.pending.iter_mut() {
            m.retain(|k, v| orig.get(p).map(|o| o.tags.get(k)) != Some(v.as_str()));
        }
        self.pending.retain(|_, m| !m.is_empty());
    }

    fn set_for_selection(&mut self, f: impl Fn(&TagState, &str, usize, usize) -> Vec<(&'static str, String)>) -> bool {
        let sel = self.selected();
        if sel.is_empty() {
            return false;
        }
        let n = sel.len();
        let mut all = Vec::new();
        for (i, p) in sel.iter().enumerate() {
            all.push((p.clone(), f(self, p, i, n)));
        }
        for (p, ch) in all {
            for (k, v) in ch {
                self.pending.entry(p.clone()).or_default().insert(k.to_string(), v);
            }
        }
        self.after_edit();
        self.refill = true;
        true
    }

    fn cover_of(&self, p: &str) -> (Option<&[u8]>, Option<u64>, &'static str) {
        if let Some(c) = self.covers_new.get(p) {
            return match c {
                Some(n) => (Some(&n.data[..]), Some(n.hash), "new"),
                None => (None, None, "removed"),
            };
        }
        let Some(o) = self.orig.get(p) else { return (None, None, "none") };
        if let Some(e) = &o.tags.embedded {
            return (Some(e), o.emb, "embedded");
        }
        if let Some(f) = &o.tags.folder {
            return (Some(f), o.folder, "folder");
        }
        (None, None, "none")
    }

    /// A new cover for every selected song (dropped, pasted, chosen, found online).
    pub fn set_cover_bytes(&mut self, data: &[u8]) -> Result<(), String> {
        let sel = self.selected();
        if sel.is_empty() {
            return Err("select some songs first".into());
        }
        let (bytes, mime) = tags::prepare_cover(data, if self.shrink { 1200 } else { 0 })?;
        let nc = NewCover { hash: hash(&bytes), data: Arc::new(bytes), mime };
        for p in sel {
            self.covers_new.insert(p, Some(nc.clone()));
        }
        Ok(())
    }

    fn remove_cover(&mut self) {
        for p in self.selected() {
            if self.orig.get(&p).is_some_and(|o| o.tags.embedded.is_some()) {
                self.covers_new.insert(p, None);
            } else {
                self.covers_new.remove(&p);
            }
        }
    }

    fn new_name(&self, p: &str, pattern: &str) -> Result<String, String> {
        let stem = Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = Path::new(p).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
        let v = |k: &str| self.value(p, k);
        let title = Some(v("title")).filter(|s| !s.is_empty()).unwrap_or(stem);
        let artist = v("artist");
        let vals = [
            ("track", tags::first_int(&v("tracknumber")).to_string()),
            ("disc", tags::first_int(&v("discnumber")).max(1).to_string()),
            ("title", title),
            ("albumartist", Some(v("albumartist")).filter(|s| !s.is_empty()).unwrap_or_else(|| artist.clone())),
            ("artist", artist),
            ("album", v("album")),
            ("year", v("date").chars().take(4).collect()),
        ];
        Ok(tags::safe_filename(&tags::fill_pattern(pattern, &vals)?) + &ext)
    }

    fn start_save(&mut self, lib_tracks: Vec<Arc<Track>>, covers_dir: std::path::PathBuf, ctx: &egui::Context) {
        let jobs: Vec<(String, Vec<(String, String)>, Option<Option<NewCover>>)> = self
            .paths
            .iter()
            .filter(|p| self.pending.get(*p).is_some_and(|m| !m.is_empty()) || self.covers_new.contains_key(*p))
            .map(|p| (p.clone(), self.pending.get(p).map(|m| m.clone().into_iter().collect()).unwrap_or_default(), self.covers_new.get(p).cloned()))
            .collect();
        if jobs.is_empty() {
            return;
        }
        self.status = format!("saving {} files…", jobs.len());
        let (tx, rx) = bounded(1);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("nexus-tags-write".into())
            .spawn(move || {
                let mut res = SaveResult { ok: vec![], bad: vec![], fresh: vec![], covers_changed: HashSet::new(), tracks: vec![] };
                for (p, ch, cov) in jobs {
                    let cover = match &cov {
                        None => Cover::Keep,
                        Some(None) => Cover::Remove,
                        Some(Some(n)) => Cover::Set(n.data.to_vec(), n.mime),
                    };
                    match tags::write_tags(Path::new(&p), &ch, &cover) {
                        Ok(()) => {
                            if cov.is_some() {
                                res.covers_changed.insert(p.clone());
                            }
                            if let Ok(t) = tags::read_tags(Path::new(&p), true) {
                                res.fresh.push((p.clone(), Orig::new(t)));
                            }
                            res.ok.push(p);
                        }
                        Err(e) => {
                            let low = e.to_lowercase();
                            let msg = if low.contains("denied") || low.contains("os error 32") || low.contains("os error 5)") {
                                "in use — try again when it isn't playing".to_string()
                            } else {
                                e
                            };
                            res.bad.push(format!("{}: {msg}", file_name(&p)));
                        }
                    }
                }
                if !res.ok.is_empty() {
                    res.tracks = nexus_core::scanner::refresh_files(&lib_tracks, &res.ok, &res.covers_changed, &covers_dir);
                }
                let _ = tx.send(res);
                ctx.request_repaint();
            })
            .expect("spawn tag writer");
        self.save_rx = Some(rx);
    }

    fn find_online(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let sel = self.selected();
        let p = sel.first().or(self.paths.first()).ok_or("nothing to search for")?.clone();
        let artist = Some(self.value(&p, "albumartist")).filter(|s| !s.is_empty()).unwrap_or_else(|| self.value(&p, "artist"));
        let album = self.value(&p, "album");
        if album.is_empty() {
            return Err("needs an album name to search for".into());
        }
        self.online = Some(Online::Searching(album.clone()));
        let (tx, rx) = bounded(1);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("nexus-tags-online".into())
            .spawn(move || {
                let res = tags::online_album_lookup(&artist, &album).map(|info| {
                    let cov = if info.cover_url.is_empty() {
                        None
                    } else {
                        nexus_core::net::get_bytes(&info.cover_url, 20, 25 << 20).ok().and_then(|d| tags::image_info(&d).map(|(w, h, _)| (d, w, h)))
                    };
                    (info, cov)
                });
                let _ = tx.send(res);
                ctx.request_repaint();
            })
            .expect("spawn lookup");
        self.online_rx = Some(rx);
        Ok(())
    }

    fn ensure_tex(&mut self, ctx: &egui::Context, h: u64, data: &[u8]) {
        if self.tex.as_ref().is_some_and(|(x, ..)| *x == h) {
            return;
        }
        let kind = if data.starts_with(b"\x89PNG") {
            "PNG"
        } else if data.starts_with(b"\xff\xd8") {
            "JPEG"
        } else {
            "image"
        };
        self.tex = Some(match image::load_from_memory(data) {
            Ok(img) => {
                let (w, hh) = (img.width(), img.height());
                let small = img.thumbnail(400, 400).to_rgba8();
                let ci = ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw());
                (h, Some(ctx.load_texture("tag-cover", ci, TextureOptions::LINEAR)), format!("{w}×{hh} {kind} · {} KB", data.len() / 1024))
            }
            Err(_) => (h, None, "can't read this image".into()),
        });
    }
}

// ------------------------------------------------------------------ app hooks

impl Nexus {
    /// Image dropped on the window while the tag editor is open.
    pub fn tags_drop_image(&mut self, path: &Path) {
        match std::fs::read(path).map_err(|e| e.to_string()).and_then(|d| self.tags.set_cover_bytes(&d)) {
            Ok(()) => {}
            Err(e) => self.toast(e),
        }
    }

    fn tags_paste(&mut self) {
        let got: Option<Vec<u8>> = (|| {
            let mut cb = arboard::Clipboard::new().ok()?;
            if let Ok(img) = cb.get_image() {
                let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
                let mut out = std::io::Cursor::new(Vec::new());
                rgba.write_to(&mut out, image::ImageFormat::Png).ok()?;
                return Some(out.into_inner());
            }
            let text = cb.get_text().ok()?;
            let line = text.lines().next()?.trim();
            let path = line.strip_prefix("file://").unwrap_or(line);
            let path = Path::new(path);
            nexus_core::library::is_image(path).then(|| std::fs::read(path).ok()).flatten()
        })();
        match got {
            Some(d) => {
                if let Err(e) = self.tags.set_cover_bytes(&d) {
                    self.toast(e);
                }
            }
            None => self.toast("no image on the clipboard"),
        }
    }

    fn tags_choose_image(&mut self) {
        let start = self.tags.selected().first().and_then(|p| Path::new(p).parent().map(Path::to_path_buf)).unwrap_or_else(nexus_core::paths::home_dir);
        if let Some(f) = rfd::FileDialog::new().set_title("cover image").set_directory(start).add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp"]).pick_file() {
            self.tags_drop_image(&f);
        }
    }

    fn tags_rename(&mut self) {
        let st = &mut self.tags;
        let sel = st.selected();
        if sel.is_empty() {
            return;
        }
        if st.dirty() {
            self.toast("save your tag edits first, then rename");
            return;
        }
        let pattern = st.pattern.clone().unwrap_or_default();
        let plan: Result<Vec<(String, String)>, String> = sel
            .iter()
            .map(|p| st.new_name(p, &pattern).map(|n| (p.clone(), Path::new(p).with_file_name(n).to_string_lossy().to_string())))
            .collect();
        let plan = match plan {
            Ok(v) => v,
            Err(e) => {
                st.status = e;
                return;
            }
        };
        let plan: Vec<(String, String)> = plan.into_iter().filter(|(a, b)| a != b).collect();
        self.cfg.set("rename_pattern", serde_json::json!(pattern));
        self.cfg.save();
        let st = &mut self.tags;
        if plan.is_empty() {
            st.status = "names already match".into();
            return;
        }
        let mut moved: HashMap<String, String> = HashMap::new();
        let mut failed = Vec::new();
        for (a, b) in plan {
            if Path::new(&b).exists() && a.to_lowercase() != b.to_lowercase() {
                failed.push(format!("{} already exists", file_name(&b)));
                continue;
            }
            match std::fs::rename(&a, &b) {
                Ok(()) => {
                    let lrc = Path::new(&a).with_extension("lrc");
                    if lrc.exists() {
                        let _ = std::fs::rename(&lrc, Path::new(&b).with_extension("lrc"));
                    }
                    moved.insert(a, b);
                }
                Err(e) => failed.push(format!("{}: {}", file_name(&a), if e.raw_os_error() == Some(32) { "in use (playing?)".to_string() } else { e.to_string() })),
            }
        }
        if !moved.is_empty() {
            st.paths = st.paths.iter().map(|p| moved.get(p).cloned().unwrap_or_else(|| p.clone())).collect();
            st.orig = std::mem::take(&mut st.orig).into_iter().map(|(p, o)| (moved.get(&p).cloned().unwrap_or(p), o)).collect();
        }
        st.status = plural(moved.len(), "renamed {} file") + &if failed.is_empty() { String::new() } else { format!(" · {} skipped: {}", failed.len(), failed[0]) };
        if !moved.is_empty() {
            self.paths_moved(&moved);
        }
    }

    fn tags_saved(&mut self, res: SaveResult) {
        let st = &mut self.tags;
        for p in &res.ok {
            st.pending.remove(p);
            st.covers_new.remove(p);
        }
        for (p, o) in res.fresh {
            st.orig.insert(p, o);
        }
        st.status = plural(res.ok.len(), "saved {} file") + &if res.bad.is_empty() { String::new() } else { format!(" · {} failed: {}", res.bad.len(), res.bad[0]) };
        st.refill = true;
        st.tex = None;
        if !res.ok.is_empty() {
            self.set_library(nexus_core::library::Library::from_tracks(res.tracks));
            self.lib.save();
            self.lyrics.forget();
            if let Some(cur) = self.current_track().cloned() {
                if res.ok.contains(&cur.path) {
                    self.now_playing_changed();
                }
            }
        }
    }
}

// ------------------------------------------------------------------ drawing

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect, paths: &[String]) {
    if app.tags.requested != paths && app.tags.save_rx.is_none() {
        let ctx = ui.ctx().clone();
        if app.tags.load(paths, &ctx) {
            app.toast("unsaved tag edits were discarded");
        }
        if app.tags.pattern.is_none() {
            let p = app.cfg.s("rename_pattern");
            app.tags.pattern = Some(if p.is_empty() { "{track:02} {title}".into() } else { p });
        }
    }
    if let Some(res) = app.tags.poll() {
        app.tags_saved(res);
    }
    if app.tags.refill {
        app.tags.refill_form();
    }
    let t = app.t.clone();
    let c = t.c;
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 18.0));
    let busy = app.tags.load_rx.is_some() || app.tags.save_rx.is_some();

    // keyboard: ctrl+s saves
    if ui.input(|i| i.modifiers.command && i.key_pressed(Key::S)) && app.tags.dirty() && !busy {
        let ctx = ui.ctx().clone();
        app.tags.start_save(app.lib.tracks.clone(), nexus_core::paths::covers_dir(), &ctx);
    }

    let mut do_save = false;
    let mut do_revert = false;
    region(ui, inner, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 12.0);
        // ---------------- header
        ui.horizontal(|ui| {
            let st = &app.tags;
            let title = header_title(st);
            let n = st.files_changed();
            let save_label = if st.dirty() { format!("save {n} file{}", if n == 1 { "" } else { "s" }) } else { "save".into() };
            let status_w = if st.status.is_empty() { 0.0 } else { widgets::text_width(ui, &t.l(&st.status), &t.body()).min(280.0) + 12.0 };
            let right_w = widgets::pills_width(ui, &t, &["revert", &save_label, "done"], 18.0, 8.0) + status_w + 24.0;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.set_width((ui.available_width() - right_w).max(120.0));
                widgets::eyebrow(ui, &t, &t.l("EDIT TAGS"));
                let g = elided(ui, &title, t.font(W::Display, 27.0), c.text, ui.available_width());
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, c.text);
            });
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                if widgets::pill(ui, &t, "done", Pill::Normal).clicked() {
                    app.acts.push(Act::Back);
                }
                let dirty = app.tags.dirty() && !busy;
                let n = app.tags.files_changed();
                let label = if app.tags.dirty() { format!("save {n} file{}", if n == 1 { "" } else { "s" }) } else { "save".into() };
                if ui.add_enabled_ui(dirty, |ui| widgets::pill(ui, &t, &label, Pill::Primary)).inner.clicked() {
                    do_save = true;
                }
                if ui.add_enabled_ui(dirty, |ui| widgets::pill(ui, &t, "revert", Pill::Normal)).inner.clicked() {
                    do_revert = true;
                }
                if !app.tags.status.is_empty() {
                    let g = elided(ui, &t.l(&app.tags.status), t.body(), c.dim, (ui.available_width() - 8.0).min(280.0));
                    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(r.min, g, c.dim);
                }
            });
        });
        if app.tags.load_rx.is_some() {
            return;
        }
        // ---------------- body: cover | fields
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 30.0;
            ui.vertical(|ui| {
                ui.set_width(190.0);
                cover_column(app, ui, &t);
            });
            ui.vertical(|ui| {
                fields(app, ui, &t);
            });
        });
        // ---------------- tools
        tools(app, ui, &t);
        online_box(app, ui, &t);
        match app.tags.bar {
            Bar::Replace => replace_bar(app, ui, &t),
            Bar::Rename => rename_bar(app, ui, &t),
            Bar::None => {}
        }
        // ---------------- the files
        table(app, ui, &t);
    });
    if do_revert {
        let st = &mut app.tags;
        st.pending.clear();
        st.covers_new.clear();
        st.refill = true;
        st.status = "changes undone".into();
    }
    if do_save {
        let ctx = ui.ctx().clone();
        app.tags.start_save(app.lib.tracks.clone(), nexus_core::paths::covers_dir(), &ctx);
    }
}

fn header_title(st: &TagState) -> String {
    if st.paths.len() == 1 {
        let v = st.value(&st.paths[0], "title");
        return if v.is_empty() { file_name(&st.paths[0]) } else { v };
    }
    let albums: BTreeSet<String> = st.paths.iter().map(|p| st.value(p, "album")).collect();
    if albums.len() == 1 && !albums.iter().next().is_some_and(String::is_empty) {
        return albums.into_iter().next().unwrap_or_default();
    }
    if st.paths.is_empty() && st.load_rx.is_some() {
        return String::new();
    }
    format!("{} songs", st.paths.len())
}

fn cover_column(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let c = t.c;
    let ctx = ui.ctx().clone();
    let st = &mut app.tags;
    let sel = st.selected();
    let covers: Vec<(Option<u64>, &'static str)> = sel.iter().map(|p| {
        let (_, h, k) = st.cover_of(p);
        (h, k)
    }).collect();
    let hashes: HashSet<Option<u64>> = covers.iter().map(|(h, _)| *h).collect();
    let (msg, info_where, show): (String, String, Option<(u64, Vec<u8>)>) = if sel.is_empty() {
        (String::new(), String::new(), None)
    } else if hashes.len() > 1 {
        (t.l("different covers"), t.l("drop an image to give them all the same cover"), None)
    } else {
        let (d, h, k) = st.cover_of(&sel[0]);
        let where_ = match k {
            "embedded" => "embedded in the file",
            "folder" => "folder image only (not in the file)",
            "new" => "new — not saved yet",
            "removed" => "will be removed",
            _ => "",
        };
        (t.l("no cover — drop an image here"), t.l(where_), d.zip(h).map(|(d, h)| (h, d.to_vec())))
    };
    // the drop box
    let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 190.0), Sense::click());
    let hovering_files = ui.input(|i| !i.raw.hovered_files.is_empty());
    let p = ui.painter();
    let rad = t.cover_radius(190.0);
    p.rect(rect, rad, c.panel, Stroke::new(1.0, if hovering_files || resp.hovered() { c.dim } else { c.border }), StrokeKind::Inside);
    let mut info = String::new();
    match &show {
        Some((h, d)) => {
            st.ensure_tex(&ctx, *h, d);
            if let Some((_, tex, line)) = &st.tex {
                info = line.clone();
                if let Some(tex) = tex {
                    let s = tex.size_vec2();
                    let k = (rect.width() / s.x).min(rect.height() / s.y);
                    let r = Rect::from_center_size(rect.center(), s * k);
                    egui::Image::new((tex.id(), s)).corner_radius(rad).paint_at(ui, r);
                }
            }
        }
        None => {
            let g = ui.fonts_mut(|f| f.layout(msg.clone(), t.font(W::Regular, -1.5), c.dim, 160.0));
            ui.painter().galley(rect.center() - g.size() / 2.0, g, c.dim);
        }
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(t.l("drop an image here, or click to choose one"));
    if resp.clicked() && !sel.is_empty() {
        app.tags_choose_image();
    }
    let st = &mut app.tags;
    let text = [info, info_where].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
    let g = ui.fonts_mut(|f| f.layout(text, t.font(W::Regular, -1.5), c.dim, 190.0));
    let (r, _) = ui.allocate_exact_size(vec2(190.0, g.size().y.max(30.0)), Sense::hover());
    ui.painter().galley(r.min, g, c.dim);
    let has_folder = sel.iter().any(|p| st.orig.get(p).is_some_and(|o| o.tags.folder.is_some() && o.tags.embedded.is_none()));
    let any_new = covers.iter().any(|(_, k)| *k == "new");
    let enabled = !sel.is_empty();
    ui.add_enabled_ui(enabled, |ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        let mut act = 0;
        ui.horizontal(|ui| {
            if widgets::pill_pad(ui, t, "choose image", Pill::Normal, 12.0).clicked() {
                act = 1;
            }
            if widgets::pill_pad(ui, t, "paste", Pill::Normal, 12.0).clicked() {
                act = 2;
            }
        });
        ui.horizontal(|ui| {
            if widgets::pill_pad(ui, t, "find online", Pill::Normal, 12.0).clicked() {
                act = 3;
            }
            if widgets::pill_pad(ui, t, "remove", Pill::Normal, 12.0).clicked() {
                act = 4;
            }
        });
        if has_folder && !any_new && widgets::pill_pad(ui, t, "use folder image", Pill::Normal, 12.0).clicked() {
            act = 5;
        }
        ui.horizontal(|ui| {
            widgets::switch(ui, t, &mut app.tags.shrink).on_hover_text(t.l("big covers make every file bigger; 1200 px still looks sharp everywhere"));
            widgets::label(ui, &t.l("shrink to 1200 px"), t.font(W::Regular, -1.0), c.dim, Sense::hover());
        });
        match act {
            1 => app.tags_choose_image(),
            2 => app.tags_paste(),
            3 => {
                let ctx = ui.ctx().clone();
                if let Err(e) = app.tags.find_online(&ctx) {
                    app.toast(e);
                }
            }
            4 => app.tags.remove_cover(),
            5 => {
                let sel = app.tags.selected();
                let data = sel.iter().find_map(|p| app.tags.orig.get(p).and_then(|o| o.tags.folder.clone()));
                if let Some(d) = data {
                    if let Err(e) = app.tags.set_cover_bytes(&d) {
                        app.toast(e);
                    }
                }
            }
            _ => {}
        }
    });
}

fn fields(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let c = t.c;
    let st = &mut app.tags;
    let n = st.sel.len();
    let total = st.paths.len();
    let applies = if n == 0 {
        "select songs below".to_string()
    } else if total == 1 {
        "editing this song".to_string()
    } else if n == total {
        format!("editing all {n} songs")
    } else {
        format!("editing {n} of {total} songs")
    };
    widgets::eyebrow(ui, t, &t.l(&applies).to_uppercase());
    ui.add_space(-6.0);
    let w = ui.available_width();
    let keys: Vec<(&str, &str)> = FIELDS.iter().copied().filter(|(k, _)| *k != "lyrics").collect();
    let label_w = keys.iter().map(|(_, l)| widgets::text_width(ui, &t.l(l), &t.body())).fold(0.0f32, f32::max) + 6.0;
    let gap = 14.0;
    let field_w = ((w - label_w * 2.0 - gap * 3.0) / 2.0).max(80.0);
    let field_h = ui.fonts_mut(|f| f.row_height(&t.body())) + 12.0;
    let mut edited: Option<(String, String)> = None;
    ui.add_enabled_ui(n > 0, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        for pair in keys.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (j, (k, label)) in pair.iter().enumerate() {
                    let g = elided(ui, &t.l(label), t.body(), c.dim, label_w);
                    let (r, _) = ui.allocate_exact_size(vec2(label_w, field_h), Sense::hover());
                    ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, c.dim);
                    let short = matches!(*k, "tracknumber" | "discnumber" | "date");
                    let fw = if short { field_w.min(140.0) } else { field_w };
                    let multi = st.multi.contains(*k);
                    let buf = st.form.entry(k.to_string()).or_default();
                    let resp = ui.add_sized(
                        vec2(fw, field_h),
                        egui::TextEdit::singleline(buf)
                            .id(egui::Id::unique(("tagf", *k)))
                            .hint_text(if multi { MULTI } else { "" })
                            .margin(egui::Margin::symmetric(8, 5)),
                    );
                    if resp.changed() {
                        edited = Some((k.to_string(), buf.clone()));
                    }
                    if fw < field_w && j == 0 {
                        ui.add_space(field_w - fw);
                    }
                }
            });
        }
        ui.add_space(-4.0);
        ui.horizontal(|ui| {
            widgets::label(ui, &t.l("lyrics"), t.body(), c.dim, Sense::hover());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::small_dim(ui, t, &t.l("plain text, or [mm:ss.xx] timed lines for synced lyrics"));
            });
        });
        let multi = st.multi.contains("lyrics");
        let buf = st.form.entry("lyrics".into()).or_default();
        let resp = ui.add_sized(
            vec2(ui.available_width(), 64.0),
            egui::TextEdit::multiline(buf).id(egui::Id::unique(("tagf", "lyrics"))).hint_text(if multi { MULTI } else { "" }).margin(egui::Margin::symmetric(8, 5)),
        );
        if resp.changed() {
            edited = Some(("lyrics".into(), buf.clone()));
        }
    });
    if let Some((k, v)) = edited {
        st.field_edited(&k, &v);
    }
}

fn tools(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let mut act = 0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let items = [
            ("number 1…n", "number the selected songs in the order shown"),
            ("titles from file names", "“03 - Song name.flac” → Song name"),
            ("Title Case", "capitalise titles in the selection"),
            ("find / replace", ""),
            ("rename files", ""),
        ];
        for (i, (label, tip)) in items.iter().enumerate() {
            let r = widgets::pill_pad(ui, t, label, Pill::Normal, 12.0);
            let r = if tip.is_empty() { r } else { r.on_hover_text(t.l(tip)) };
            if r.clicked() {
                act = i + 1;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::pill_pad(ui, t, "select all", Pill::Normal, 12.0).clicked() {
                act = 9;
            }
        });
    });
    let st = &mut app.tags;
    let ok = match act {
        1 => st.set_for_selection(|s, p, i, n| vec![("tracknumber", if s.value(p, "tracknumber").contains('/') { format!("{}/{n}", i + 1) } else { (i + 1).to_string() })]),
        2 => st.set_for_selection(|_, p, _, _| {
            let stem = Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            vec![("title", tags::title_from_stem(&stem))]
        }),
        3 => st.set_for_selection(|s, p, _, _| vec![("title", tags::title_case(&s.value(p, "title")))]),
        4 => {
            st.bar = if st.bar == Bar::Replace { Bar::None } else { Bar::Replace };
            true
        }
        5 => {
            st.bar = if st.bar == Bar::Rename { Bar::None } else { Bar::Rename };
            true
        }
        9 => {
            st.sel = (0..st.paths.len()).collect();
            st.refill = true;
            true
        }
        _ => true,
    };
    if !ok {
        app.toast("select some songs first");
    }
}

fn online_box(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let c = t.c;
    let Some(state) = &app.tags.online else { return };
    let (text, cover, info) = match state {
        Online::Searching(album) => (format!("searching for “{album}”…"), false, false),
        Online::Nothing => (t.l("nothing matching found on iTunes or Deezer"), false, false),
        Online::Found(r, cov) => {
            let bits: Vec<String> = [r.year.clone(), r.genre.clone(), cov.as_ref().map(|(_, w, h)| format!("{w}×{h} cover")).unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect();
            (format!("{}:  {} — {}   ·   {}", r.source, r.name, r.artist, bits.join("  ·  ")), cov.is_some(), !r.year.is_empty() || !r.genre.is_empty())
        }
    };
    let mut act = 0;
    egui::Frame::new()
        .fill(c.panel)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(if t.pills { 18.0 } else { t.radius })
        .inner_margin(egui::Margin { left: 14, right: 8, top: 6, bottom: 6 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                let bw = 300.0;
                let g = elided(ui, &text, t.body(), c.dim, (ui.available_width() - bw).max(100.0));
                let (r, _) = ui.allocate_exact_size(vec2(g.size().x, 28.0), Sense::hover());
                ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, c.dim);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::pill_pad(ui, t, "✕", Pill::Normal, 10.0).clicked() {
                        act = 3;
                    }
                    if info && widgets::pill_pad(ui, t, "use year + genre", Pill::Normal, 12.0).clicked() {
                        act = 2;
                    }
                    if cover && widgets::pill_pad(ui, t, "use cover", Pill::Normal, 12.0).clicked() {
                        act = 1;
                    }
                });
            });
        });
    let st = &mut app.tags;
    match act {
        1 => {
            if let Some(Online::Found(_, Some((d, _, _)))) = &st.online {
                let d = d.clone();
                if let Err(e) = st.set_cover_bytes(&d) {
                    app.toast(e);
                }
            }
        }
        2 => {
            if let Some(Online::Found(r, _)) = &st.online {
                let (year, genre) = (r.year.clone(), r.genre.clone());
                let ok = st.set_for_selection(|s, p, _, _| {
                    let mut out = Vec::new();
                    if !year.is_empty() && s.value(p, "date").is_empty() {
                        out.push(("date", year.clone()));
                    }
                    if !genre.is_empty() {
                        out.push(("genre", genre.clone()));
                    }
                    out
                });
                if !ok {
                    app.toast("select some songs first");
                }
            }
        }
        3 => {
            st.online = None;
            st.online_rx = None;
        }
        _ => {}
    }
}

fn replace_bar(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let keys: Vec<(&str, &str)> = FIELDS.iter().copied().filter(|(k, _)| *k != "lyrics").collect();
    let st = &mut app.tags;
    let mut go = false;
    ui.horizontal(|ui| {
        widgets::dim(ui, t, &t.l("in"));
        let cur = st.rep_field.min(keys.len() - 1);
        egui::ComboBox::from_id_salt("tag-rep-field").selected_text(t.l(keys[cur].1)).width(130.0).show_ui(ui, |ui| {
            for (i, (_, label)) in keys.iter().enumerate() {
                ui.selectable_value(&mut st.rep_field, i, t.l(label));
            }
        });
        let w = ((ui.available_width() - 200.0) / 2.0).max(80.0);
        ui.add(egui::TextEdit::singleline(&mut st.rep_find).hint_text(t.l("find")).desired_width(w).margin(egui::Margin::symmetric(8, 5)));
        ui.add(egui::TextEdit::singleline(&mut st.rep_with).hint_text(t.l("replace with")).desired_width(w).margin(egui::Margin::symmetric(8, 5)));
        if widgets::pill_pad(ui, t, "replace in selection", Pill::Normal, 12.0).clicked() {
            go = true;
        }
    });
    if go && !st.rep_find.is_empty() {
        let k = keys[st.rep_field.min(keys.len() - 1)].0;
        let (a, b) = (st.rep_find.clone(), st.rep_with.clone());
        let hits = st.selected().iter().filter(|p| st.value(p, k).contains(&a)).count();
        st.set_for_selection(|s, p, _, _| {
            let v = s.value(p, k);
            if v.contains(&a) { vec![(k, v.replace(&a, &b))] } else { vec![] }
        });
        st.status = plural(hits, "replaced in {} song");
    }
}

fn rename_bar(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let c = t.c;
    let mut go = false;
    ui.horizontal(|ui| {
        let st = &mut app.tags;
        widgets::dim(ui, t, &t.l("file name"));
        let pat = st.pattern.get_or_insert_with(|| "{track:02} {title}".into());
        let w = ((ui.available_width() - 220.0) / 2.0).max(120.0);
        ui.add(egui::TextEdit::singleline(pat).desired_width(w).margin(egui::Margin::symmetric(8, 5)))
            .on_hover_text("{track} {disc} {title} {artist} {album} {albumartist} {year} — e.g. {disc}-{track:02} {artist} - {title}");
        let pat = pat.clone();
        let preview = match st.selected().first() {
            Some(p) => match st.new_name(p, &pat) {
                Ok(n) => format!("→ {n}"),
                Err(e) => e,
            },
            None => String::new(),
        };
        let g = elided(ui, &preview, t.font(W::Regular, -1.0), c.dim, w);
        let (r, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, c.dim);
        if widgets::pill_pad(ui, t, "rename selected files", Pill::Normal, 12.0).clicked() {
            go = true;
        }
    });
    if go {
        app.tags_rename();
    }
}

fn table(app: &mut Nexus, ui: &mut Ui, t: &Theme) {
    let c = t.c;
    let full = ui.available_rect_before_wrap();
    if full.height() < 40.0 {
        return;
    }
    let width = full.width() - 10.0;
    let fixed: f32 = COLS.iter().map(|(_, _, w)| *w).sum();
    let n_stretch = COLS.iter().filter(|(_, _, w)| *w == 0.0).count() as f32;
    let stretch = ((width - fixed) / n_stretch).max(60.0);
    let widths: Vec<f32> = COLS.iter().map(|(_, _, w)| if *w == 0.0 { stretch } else { *w }).collect();
    let hfont = t.font(W::Semi, -1.5);
    let hh = ui.fonts_mut(|f| f.row_height(&hfont)) + 13.0;
    let (hrect, _) = ui.allocate_exact_size(vec2(full.width(), hh), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(Rect::from_min_size(pos2(hrect.left(), hrect.bottom() - 1.0), vec2(width, 1.0)), 0.0, c.border);
    let mut x = hrect.left();
    for (i, (_, label, _)) in COLS.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, hrect.top()), vec2(widths[i], hh - 1.0));
        x += widths[i];
        let g = elided(ui, &t.header(&t.l(label)), hfont.clone(), c.dim, r.width() - 16.0);
        p.galley(pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, c.dim);
    }
    ui.add_space(-14.0);
    let body = t.body();
    let bold = t.font(W::Bold, 0.0);
    let row_h = (ui.fonts_mut(|f| f.row_height(&body)) + 14.0).round();
    let st = &mut app.tags;
    let n = st.paths.len();
    let mut click: Option<(usize, egui::Modifiers)> = None;
    let mut start_edit: Option<(usize, usize)> = None;
    let mut commit: Option<(usize, usize, String)> = None;
    let mut cancel = false;
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().id_salt("tag-rows").auto_shrink([false, false]).show_rows(ui, row_h, n, |ui, range| {
        let p = ui.painter().clone();
        for row in range {
            let path = st.paths[row].clone();
            let (rect, resp) = ui.allocate_exact_size(vec2(full.width().min(width + 10.0), row_h), Sense::click());
            let rect = Rect::from_min_size(rect.min, vec2(width, row_h));
            let selected = st.sel.contains(&row);
            if selected {
                p.rect_filled(rect, 0.0, c.select);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, alpha(c.select, 0.6));
            }
            let hover_x = resp.hover_pos().map(|h| h.x);
            let mut x = rect.left();
            let mut tip: Option<String> = None;
            for (ci, (k, _, _)) in COLS.iter().enumerate() {
                let w = widths[ci];
                let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, row_h));
                x += w;
                if let Some((er, ec, buf, focus)) = &mut st.edit {
                    if *er == row && *ec == ci {
                        let id = egui::Id::unique(("tag-cell", row, ci));
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell.shrink2(vec2(2.0, 3.0))).layout(Layout::left_to_right(Align::Center)));
                        let r = child.add(egui::TextEdit::singleline(buf).id(id).desired_width(cell.width() - 6.0).margin(egui::Margin::symmetric(5, 2)));
                        if *focus {
                            r.request_focus();
                            *focus = false;
                        }
                        if child.input(|i| i.key_pressed(Key::Escape)) {
                            cancel = true;
                        } else if r.lost_focus() {
                            commit = Some((row, ci, buf.clone()));
                        }
                        continue;
                    }
                }
                let text = if *k == "_file" { file_name(&path) } else { st.value(&path, k) };
                let changed = *k != "_file" && st.pending.get(&path).is_some_and(|m| m.contains_key(*k));
                if hover_x.is_some_and(|hx| hx >= cell.left() && hx < cell.right()) {
                    if *k == "_file" {
                        tip = Some(path.clone());
                    } else if changed {
                        let was = st.orig_value(&path, k);
                        tip = Some(format!("was: {}", if was.is_empty() { "—".into() } else { was }));
                    }
                    if resp.double_clicked() && !matches!(*k, "_file" | "album") {
                        start_edit = Some((row, ci));
                    }
                }
                if text.is_empty() {
                    continue;
                }
                let col = if *k == "_file" { c.dim } else { c.text };
                let g = elided(ui, &text.replace('\n', " "), if changed { bold.clone() } else { body.clone() }, col, w - 16.0);
                p.galley(pos2(cell.left() + 8.0, cell.center().y - g.size().y / 2.0), g, col);
            }
            let resp = match tip {
                Some(tip) => resp.on_hover_text(tip),
                None => resp,
            };
            if resp.clicked() {
                click = Some((row, ui.input(|i| i.modifiers)));
            }
        }
    });
    if cancel {
        st.edit = None;
    } else if let Some((row, ci, text)) = commit {
        st.edit = None;
        if let Some(path) = st.paths.get(row).cloned() {
            let k = COLS[ci].0;
            st.pending.entry(path).or_default().insert(k.to_string(), text.trim().to_string());
            st.after_edit();
            st.refill = true;
        }
    }
    if let Some((row, ci)) = start_edit {
        if let Some(path) = st.paths.get(row) {
            let v = st.value(path, COLS[ci].0);
            st.edit = Some((row, ci, v, true));
        }
    }
    if let Some((row, m)) = click {
        if m.shift {
            let a = st.anchor.unwrap_or(row);
            let (lo, hi) = (a.min(row), a.max(row));
            if !m.command {
                st.sel.clear();
            }
            st.sel.extend(lo..=hi);
        } else if m.command {
            if !st.sel.remove(&row) {
                st.sel.insert(row);
            }
            st.anchor = Some(row);
        } else {
            st.sel = [row].into();
            st.anchor = Some(row);
        }
        st.refill = true;
    }
}
