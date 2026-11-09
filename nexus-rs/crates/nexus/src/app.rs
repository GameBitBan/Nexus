//! The app: state, the frame loop, navigation, library and playlists. Playback is in `player.rs`,
//! drawing in `layout.rs`, `nowpanel.rs` and `pages/`.

use crate::acts::{Act, Tracks};
use crate::analyzer::Analyzer;
use crate::covers::Covers;
use crate::table::TableState;
use crate::theme::{self, SystemFonts, Theme};
use crossbeam_channel::{Receiver, unbounded};
use egui::Context;
use nexus_core::audio::{Engine, EqSettings, Event, PlaybackOptions, ReplayGain, State};
use nexus_core::config::Config;
use nexus_core::library::{AlbumSort, Library, Track};
use nexus_core::playlists::{Playlists, Stats};
use nexus_core::scanner::ScanProgress;
use nexus_core::{paths, util};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Albums,
    /// albums → tracks columns; Some(artist) jumps to that artist's first album
    Browse(Option<String>),
    Artists(Option<String>),
    Songs,
    Queue,
    Album(String),
    Playlist(String),
    Settings(Option<String>),
    Eq,
    Lyrics,
    Visualizer,
    Inspect(Option<String>),
    Tags(Vec<String>),
    Download,
    Discover,
    Jam,
    Import,
}

impl Page {
    pub fn from_name(s: &str) -> Page {
        match s {
            "albums" => Page::Albums,
            "artists" => Page::Artists(None),
            "songs" => Page::Songs,
            "queue" => Page::Queue,
            "download" => Page::Download,
            "discover" => Page::Discover,
            "jam" => Page::Jam,
            "import" => Page::Import,
            "visualizer" => Page::Visualizer,
            "eq" => Page::Eq,
            "lyrics" => Page::Lyrics,
            "settings" => Page::Settings(None),
            _ => Page::Browse(None),
        }
    }

    /// The nav item that lights up for this page.
    pub fn nav_key(&self) -> &'static str {
        match self {
            Page::Albums | Page::Album(_) => "albums",
            Page::Browse(_) => "browse",
            Page::Artists(_) => "artists",
            Page::Songs => "songs",
            Page::Queue => "queue",
            Page::Download => "download",
            Page::Discover => "discover",
            Page::Jam => "jam",
            _ => "",
        }
    }

    /// Tool pages: opening one again replaces it instead of stacking history.
    fn is_tool(&self) -> bool {
        matches!(self, Page::Inspect(_) | Page::Lyrics | Page::Settings(_) | Page::Eq | Page::Visualizer | Page::Tags(_))
    }
}

pub type ToastAction = Box<dyn FnOnce(&mut Nexus)>;

pub struct Toast {
    pub text: String,
    pub action: Option<(String, ToastAction)>,
    pub until: f64,
}

pub struct ScanJob {
    pub progress: Arc<ScanProgress>,
    pub rx: Receiver<Vec<Track>>,
}

pub struct Nexus {
    pub ctx: Context,
    pub cfg: Config,
    pub t: Theme,
    pub lib: Arc<Library>,
    pub lib_ver: u64,
    pub engine: Engine,
    pub queue: Tracks,
    pub unshuffled: Tracks,
    pub qi: Option<usize>,
    pub queue_ver: u64,
    pub current_path: Option<String>,
    pub counted: bool,
    pub dur: f64,
    pub pos: f64,
    pub errors: u32,
    pub playlists: Playlists,
    pub pl_ver: u64,
    pub stats: Stats,
    pub stats_due: Option<f64>,
    pub page: Page,
    pub history: Vec<Page>,
    pub search: String,
    pub search_focus: bool,
    pub covers: Covers,
    pub analyzer: Analyzer,
    pub vis_wanted: bool,
    pub toast: Option<Toast>,
    pub scan: Option<ScanJob>,
    pub scan_label: String,
    pub acts: Vec<Act>,
    pub sysfonts: Option<Arc<SystemFonts>>,
    sysfonts_rx: Option<Receiver<SystemFonts>>,
    // page state
    pub songs_tbl: TableState,
    pub coll_tbl: TableState,
    pub queue_tbl: TableState,
    pub browse_tbl: TableState,
    pub browse_album: Option<String>,
    pub browse_scroll: bool,
    pub artist_sel: Option<String>,
    pub rename: Option<String>,
    pub rename_focus: bool,
    songs_cache: (u64, String, Tracks),
    albums_cache: (u64, String, Vec<usize>),
    // look
    pub dial_angle: f32,
    pub ambient_key: String,
    pub ambient_old: String,
    pub ambient_t0: f64,
    pub immersive: bool,
    pub pinned: bool,
    pub title: String,
    pub last_time: f64,
    pub bg: crate::background::Background,
    // settings page state
    pub settings_tab: String,
    pub settings_edit: std::collections::HashMap<String, String>,
    pub font_filter: String,
    pub mono_only: bool,
    pub devices: Option<Vec<String>>,
    pub skin_form: crate::pages::settings::SkinForm,
    pub eq_drag: Option<usize>,
    pub eq_save_due: Option<f64>,
    pub lyrics: crate::pages::lyrics::LyricsState,
    pub trash: Vec<(PathBuf, f64)>,
    pub inspect: crate::pages::inspect::InspectState,
}

impl Nexus {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let ctx = cc.egui_ctx.clone();
        let dir = paths::data_dir();
        let mut cfg = Config::load(dir.join("config.json"));
        if cfg.strings("library_folders").is_empty() {
            let music = paths::music_dir();
            if music.is_dir() {
                cfg.set("library_folders", json!([music.to_string_lossy()]));
                cfg.save();
            }
        }
        nexus_core::skins::install_bundled(&mut cfg);
        let t = Theme::from_config(&cfg);
        ctx.set_fonts(theme::font_definitions(&cfg, None));
        theme::apply_style(&ctx, &t);
        // the installed fonts load in the background (it can take a moment); fonts get rebuilt when done
        let (ftx, frx) = unbounded();
        std::thread::spawn({
            let ctx = ctx.clone();
            move || {
                let _ = ftx.send(SystemFonts::load());
                ctx.request_repaint();
            }
        });
        let engine = Engine::new(playback_options(&cfg));
        engine.set_eq(EqSettings::from_json(cfg.get("eq").unwrap_or(&json!({}))));
        let lib = Arc::new(Library::load());
        let covers = Covers::new(&ctx, paths::covers_dir());
        let start = Page::from_name(&cfg.s("start_view"));
        let mut app = Nexus {
            ctx,
            t,
            lib,
            lib_ver: 1,
            engine,
            queue: Vec::new(),
            unshuffled: Vec::new(),
            qi: None,
            queue_ver: 1,
            current_path: None,
            counted: false,
            dur: 0.0,
            pos: 0.0,
            errors: 0,
            playlists: Playlists::load(),
            pl_ver: 1,
            stats: Stats::load(),
            stats_due: None,
            page: Page::Albums,
            history: Vec::new(),
            search: String::new(),
            search_focus: false,
            covers,
            analyzer: Analyzer::default(),
            vis_wanted: false,
            toast: None,
            scan: None,
            scan_label: String::new(),
            acts: Vec::new(),
            sysfonts: None,
            sysfonts_rx: Some(frx),
            songs_tbl: TableState::default(),
            coll_tbl: TableState::default(),
            queue_tbl: TableState::default(),
            browse_tbl: TableState::default(),
            browse_album: None,
            browse_scroll: true,
            artist_sel: None,
            rename: None,
            rename_focus: false,
            songs_cache: (0, String::new(), Vec::new()),
            albums_cache: (0, String::new(), Vec::new()),
            dial_angle: 0.0,
            ambient_key: String::new(),
            ambient_old: String::new(),
            ambient_t0: -10.0,
            immersive: false,
            pinned: false,
            title: String::new(),
            last_time: 0.0,
            bg: Default::default(),
            settings_tab: "look".into(),
            settings_edit: Default::default(),
            font_filter: String::new(),
            mono_only: false,
            devices: None,
            skin_form: Default::default(),
            eq_drag: None,
            eq_save_due: None,
            lyrics: Default::default(),
            trash: Vec::new(),
            inspect: Default::default(),
            cfg,
        };
        app.refresh_background();
        app.set_volume(app.cfg.f("volume"));
        if app.cfg.b("remember_session") {
            app.restore_session();
        }
        app.go(start, false);
        if app.cfg.b("scan_on_startup") || app.lib.tracks.is_empty() {
            app.rescan(false);
        }
        app
    }

    // ------------------------------------------------------------ settings / look

    pub fn studio(&self) -> bool {
        self.cfg.s("layout") != "classic"
    }

    pub fn layout_name(&self) -> String {
        let l = self.cfg.s("layout");
        if ["studio", "classic", "tabs", "focus", "mini"].contains(&l.as_str()) { l } else { "studio".into() }
    }

    /// After any look setting changed: colours, fonts, sizes.
    pub fn restyle(&mut self, fonts: bool) {
        self.t = Theme::from_config(&self.cfg);
        if fonts {
            self.ctx.set_fonts(theme::font_definitions(&self.cfg, self.sysfonts.as_deref()));
        }
        theme::apply_style(&self.ctx, &self.t);
    }

    pub fn refresh_background(&mut self) {
        let p = nexus_core::skins::background_path(&self.cfg);
        let blur = self.cfg.b("skin_bg_blur");
        self.bg.set(&self.ctx.clone(), p, blur);
    }

    /// Something in the settings changed: apply what depends on it.
    pub fn setting_changed(&mut self, key: &str) {
        self.cfg.save();
        let fonts = matches!(key, "font_family" | "display_font" | "*");
        self.restyle(fonts);
        if key.starts_with("skin") || key == "custom_background" || key == "*" {
            self.refresh_background();
        }
        if matches!(key, "audio_device" | "exclusive_mode" | "replaygain" | "replaygain_preamp" | "gapless" | "*") {
            self.engine.set_options(playback_options(&self.cfg));
            self.update_gapless_next();
        }
        if key == "volume_max" {
            let v = self.cfg.f("volume");
            self.set_volume(v);
        }
        if key == "album_sort" || key == "*" {
            self.albums_cache.0 = 0;
        }
        if key == "song_columns" || key == "*" {
            self.songs_tbl.reset();
        }
    }

    /// After a skin or built-in look was applied: fonts, colours, background, maybe the layout.
    pub fn after_look_change(&mut self) {
        self.settings_edit.clear();
        self.setting_changed("*");
        let l = self.layout_name();
        if l == "mini" {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(480.0, 780.0)));
        }
    }

    pub fn import_skin(&mut self, f: &std::path::Path) {
        match nexus_core::skins::import_file(f) {
            Ok(slug) => {
                let _ = nexus_core::skins::apply(&mut self.cfg, &slug);
                self.skin_form.loaded = false;
                self.after_look_change();
                let name = nexus_core::skins::active(&self.cfg).map(|s| s.name).unwrap_or(slug);
                self.toast(format!("skin “{name}” installed"));
            }
            Err(e) => self.toast(format!("couldn't import that skin: {e}")),
        }
    }

    pub fn delete_skin(&mut self, m: &nexus_core::skins::Skin) {
        let trash = m.path.with_file_name(format!("{}.deleted", m.slug));
        let _ = std::fs::remove_dir_all(&trash);
        if std::fs::rename(&m.path, &trash).is_err() {
            self.toast("couldn't delete that skin");
            return;
        }
        let was_active = self.cfg.s("skin") == m.slug;
        if was_active {
            self.cfg.set("skin", "");
            self.cfg.set("color_overrides", serde_json::json!({}));
            self.after_look_change();
        }
        let (slug, path, name) = (m.slug.clone(), m.path.clone(), m.name.clone());
        let due = self.ctx.input(|i| i.time) + 15.0;
        self.trash.push((trash.clone(), due));
        self.toast_action(
            format!("deleted skin “{name}”"),
            "undo",
            move |app| {
                if std::fs::rename(&trash, &path).is_ok() {
                    app.trash.retain(|(p, _)| *p != trash);
                    if was_active {
                        let _ = nexus_core::skins::apply(&mut app.cfg, &slug);
                        app.after_look_change();
                    }
                }
            },
            15.0,
        );
    }

    pub fn set_cfg(&mut self, key: &str, v: impl Into<serde_json::Value>) {
        let v = v.into();
        if self.cfg.get(key) == Some(&v) {
            return;
        }
        self.cfg.set(key, v);
        self.setting_changed(key);
    }

    pub fn dial_size(&self) -> f32 {
        let base = (self.cfg.f("dial_size") as f32).clamp(160.0, 600.0);
        match self.layout_name().as_str() {
            "focus" => base.max(400.0),
            "mini" => 330.0,
            _ => base,
        }
    }

    // ------------------------------------------------------------ toasts

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_full(text, None, 6.0);
    }

    pub fn toast_ms(&mut self, text: impl Into<String>, secs: f64) {
        self.toast_full(text, None, secs);
    }

    pub fn toast_action(&mut self, text: impl Into<String>, label: &str, f: impl FnOnce(&mut Nexus) + 'static, secs: f64) {
        self.toast_full(text, Some((label.to_string(), Box::new(f))), secs);
    }

    fn toast_full(&mut self, text: impl Into<String>, action: Option<(String, ToastAction)>, secs: f64) {
        let text = text.into();
        let text = if text.chars().next().is_some_and(char::is_lowercase) { self.t.l(&text) } else { text };
        let now = self.ctx.input(|i| i.time);
        self.toast = Some(Toast { text, action, until: now + secs });
        self.ctx.request_repaint();
    }

    // ------------------------------------------------------------ navigation

    pub fn go(&mut self, page: Page, push: bool) {
        let mut page = page;
        if let Page::Artists(a) = &page {
            if self.studio() {
                page = Page::Browse(a.clone());
            }
        }
        if self.immersive && page != Page::Visualizer {
            self.set_immersive(false);
        }
        let mut push = push;
        if std::mem::discriminant(&page) == std::mem::discriminant(&self.page) && page.is_tool() {
            push = false;
        }
        match &page {
            Page::Album(k) if self.lib.album(k).is_none() => return,
            Page::Playlist(n) if !self.playlists.contains(n) => return,
            _ => {}
        }
        if push && self.page != page {
            self.history.push(self.page.clone());
            if self.history.len() > 60 {
                self.history.remove(0);
            }
        }
        match &page {
            Page::Browse(Some(artist)) => {
                let albums = self.sorted_albums();
                self.browse_album = albums.iter().map(|&i| &self.lib.albums[i]).find(|a| &a.artist == artist).map(|a| a.key.clone());
                self.browse_scroll = true;
            }
            Page::Artists(Some(a)) => self.artist_sel = Some(a.clone()),
            Page::Queue => self.queue_tbl.scroll_to = self.qi,
            Page::Album(_) | Page::Playlist(_) => {
                self.coll_tbl.reset();
                if !matches!((&self.page, &page), (Page::Playlist(a), Page::Playlist(b)) if a == b) {
                    self.rename = None;
                }
            }
            _ => {}
        }
        self.page = page;
    }

    pub fn back(&mut self) {
        if let Some(p) = self.history.pop() {
            self.go(p, false);
        } else if self.page.is_tool() {
            let start = Page::from_name(&self.cfg.s("start_view"));
            self.go(start, false);
        }
    }

    pub fn set_immersive(&mut self, on: bool) {
        let on = on && self.page == Page::Visualizer;
        if on == self.immersive {
            return;
        }
        self.immersive = on;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    // ------------------------------------------------------------ library

    pub fn sorted_albums(&mut self) -> Vec<usize> {
        let sort = self.cfg.s("album_sort");
        if self.albums_cache.0 != self.lib_ver || self.albums_cache.1 != sort {
            self.albums_cache = (self.lib_ver, sort.clone(), self.lib.sorted_albums(AlbumSort::from_config(&sort)));
        }
        self.albums_cache.2.clone()
    }

    /// The songs page rows: the whole library filtered by the search box.
    pub fn songs_view(&mut self) -> Tracks {
        if self.songs_cache.0 != self.lib_ver || self.songs_cache.1 != self.search {
            let rows = self.lib.search(&self.search).into_iter().map(|i| self.lib.tracks[i].clone()).collect();
            self.songs_cache = (self.lib_ver, self.search.clone(), rows);
        }
        self.songs_cache.2.clone()
    }

    pub fn album_tracks(&self, key: &str) -> Tracks {
        self.lib.album(key).map(|a| a.tracks.iter().map(|&i| self.lib.tracks[i].clone()).collect()).unwrap_or_default()
    }

    pub fn playlist_tracks(&self, name: &str) -> Tracks {
        self.playlists.get(name).map(|v| v.iter().filter_map(|p| self.lib.track(p).cloned()).collect()).unwrap_or_default()
    }

    pub fn playlist_cover(&self, name: &str) -> String {
        self.playlists
            .get(name)
            .and_then(|v| v.iter().filter_map(|p| self.lib.track(p)).find(|t| !t.cover.is_empty()).map(|t| t.cover.clone()))
            .unwrap_or_default()
    }

    pub fn rescan(&mut self, full: bool) {
        if self.scan.is_some() {
            return;
        }
        let folders: Vec<PathBuf> = self.cfg.strings("library_folders").into_iter().map(PathBuf::from).filter(|p| p.is_dir()).collect();
        if folders.is_empty() {
            self.scan_label = self.t.l("no music folders — add one in settings");
            return;
        }
        self.scan_label = self.t.l("scanning…");
        let progress = Arc::new(ScanProgress::default());
        let (tx, rx) = unbounded();
        let old = self.lib.tracks.clone();
        let covers = paths::covers_dir();
        let (pg, ctx) = (progress.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("nexus-scan".into())
            .spawn(move || {
                let tracks = nexus_core::scanner::scan(&folders, &old, &covers, full, &pg);
                let _ = tx.send(tracks);
                ctx.request_repaint();
            })
            .expect("spawn scanner");
        self.scan = Some(ScanJob { progress, rx });
    }

    fn poll_scan(&mut self) {
        let Some(job) = &self.scan else { return };
        match job.rx.try_recv() {
            Ok(tracks) => {
                self.scan = None;
                self.scan_label.clear();
                self.set_library(Library::from_tracks(tracks));
                self.lib.save();
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                let (d, n) = (job.progress.done.load(Ordering::Relaxed), job.progress.total.load(Ordering::Relaxed));
                self.scan_label = if n > 0 { self.t.l(&format!("scanning {d}/{n}")) } else { self.t.l("scanning…") };
                self.ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            Err(_) => {
                self.scan = None;
                self.scan_label.clear();
            }
        }
    }

    /// New library (scan, tag edit): keep the queue pointing at the fresh track objects.
    pub fn set_library(&mut self, lib: Library) {
        self.covers.clear();
        let lib = Arc::new(lib);
        let remap = |v: &mut Tracks| {
            for t in v.iter_mut() {
                if let Some(n) = lib.track(&t.path) {
                    *t = n.clone();
                }
            }
        };
        remap(&mut self.queue);
        remap(&mut self.unshuffled);
        self.lib = lib;
        self.lib_ver += 1;
        self.queue_ver += 1;
        match &self.page {
            Page::Album(k) if self.lib.album(k).is_none() => self.go(Page::Albums, false),
            _ => {}
        }
    }

    pub fn add_folder(&mut self) {
        let start = paths::home_dir();
        if let Some(d) = rfd::FileDialog::new().set_title("music folder").set_directory(start).pick_folder() {
            let d = d.to_string_lossy().to_string();
            let mut f = self.cfg.strings("library_folders");
            if !f.contains(&d) {
                f.push(d);
                self.cfg.set("library_folders", json!(f));
                self.cfg.save();
                self.rescan(false);
            }
        }
    }

    // ------------------------------------------------------------ playlists

    pub fn save_playlists(&mut self) {
        self.playlists.save();
        self.pl_ver += 1;
    }

    pub fn playlist_new(&mut self, tracks: Tracks) {
        let name = self.playlists.free_name(&self.t.l("new playlist"));
        self.playlists.add(&name, tracks.iter().map(|t| t.path.clone()));
        self.save_playlists();
        self.go(Page::Playlist(name.clone()), true);
        self.rename = Some(name);
        self.rename_focus = true;
    }

    pub fn playlist_add(&mut self, name: &str, tracks: Tracks) {
        self.playlists.add(name, tracks.iter().map(|t| t.path.clone()));
        self.save_playlists();
        let n = tracks.len();
        self.toast_ms(format!("added {n} song{} to “{name}”", if n == 1 { "" } else { "s" }), 2.5);
    }

    /// `rows` index the playlist's resolved tracks as shown on the page.
    pub fn playlist_remove_rows(&mut self, name: &str, rows: &[usize]) {
        let shown = self.playlist_tracks(name);
        let mut drop: Vec<Arc<Track>> = rows.iter().filter_map(|&r| shown.get(r).cloned()).collect();
        let Some(list) = self.playlists.get_mut(name) else { return };
        let lib = self.lib.clone();
        list.retain(|p| {
            if let Some(t) = lib.track(p) {
                if let Some(i) = drop.iter().position(|d| Arc::ptr_eq(d, t)) {
                    drop.remove(i);
                    return false;
                }
            }
            true
        });
        self.save_playlists();
    }

    pub fn playlist_rename_to(&mut self, old: &str, new: &str) {
        let new = new.trim();
        if new.is_empty() || new == old {
            return;
        }
        if self.playlists.contains(new) {
            self.toast(format!("there's already a playlist called “{new}”"));
            return;
        }
        if self.playlists.rename(old, new) {
            self.save_playlists();
            for h in self.history.iter_mut() {
                if *h == Page::Playlist(old.to_string()) {
                    *h = Page::Playlist(new.to_string());
                }
            }
            if self.page == Page::Playlist(old.to_string()) {
                self.page = Page::Playlist(new.to_string());
            }
        }
    }

    pub fn playlist_delete(&mut self, name: &str) {
        let Some((idx, paths)) = self.playlists.remove(name) else { return };
        self.save_playlists();
        self.history.retain(|h| *h != Page::Playlist(name.to_string()));
        if self.page == Page::Playlist(name.to_string()) {
            self.go(Page::Albums, false);
        }
        let n = name.to_string();
        self.toast_action(
            format!("deleted “{name}”"),
            "undo",
            move |app| {
                if !app.playlists.contains(&n) {
                    app.playlists.insert_at(idx, n.clone(), paths);
                    app.save_playlists();
                    app.go(Page::Playlist(n), true);
                }
            },
            6.0,
        );
    }

    // ------------------------------------------------------------ session

    fn restore_session(&mut self) {
        let s = self.cfg.map("session");
        let q: Tracks = s
            .get("queue")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|p| p.as_str()).filter_map(|p| self.lib.track(p).cloned()).collect())
            .unwrap_or_default();
        if q.is_empty() {
            return;
        }
        let qi = (s.get("qi").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize).min(q.len() - 1);
        let pos = s.get("pos").and_then(|v| v.as_f64()).unwrap_or(0.0) / 1000.0;
        self.queue = q.clone();
        self.unshuffled = q;
        self.qi = Some(qi);
        self.queue_ver += 1;
        self.play_current_at(pos, false);
    }

    pub fn save_state(&mut self) {
        if self.cfg.b("remember_session") {
            let q: Vec<&str> = self.queue.iter().take(5000).map(|t| t.path.as_str()).collect();
            let qi = self.qi.map(|i| i as i64).unwrap_or(-1);
            self.cfg.set("session", json!({"queue": q, "qi": qi, "pos": (self.engine.position() * 1000.0) as i64}));
        }
        let vp = self.ctx.input(|i| i.viewport().clone());
        if let Some(r) = vp.inner_rect {
            if self.layout_name() != "mini" && !vp.fullscreen.unwrap_or(false) {
                let max = vp.maximized.unwrap_or(false);
                let mut w = self.cfg.map("window");
                if !max {
                    w.insert("w".into(), json!(r.width()));
                    w.insert("h".into(), json!(r.height()));
                    if let Some(o) = vp.outer_rect {
                        w.insert("x".into(), json!(o.min.x));
                        w.insert("y".into(), json!(o.min.y));
                    }
                }
                w.insert("maximized".into(), json!(max));
                self.cfg.set("window", serde_json::Value::Object(w));
            }
        }
        self.cfg.save();
        self.stats.save();
    }

    // ------------------------------------------------------------ the frame

    fn poll_engine(&mut self) {
        for ev in self.engine.poll() {
            match ev {
                Event::Loaded { duration, .. } => {
                    self.dur = duration;
                    self.errors = 0;
                }
                Event::Advanced { path } => self.on_gapless(&path),
                Event::Ended => self.next(true),
                Event::Error(msg) => self.on_error(&msg),
            }
        }
        self.pos = self.engine.position();
        if self.engine.duration() > 0.0 {
            self.dur = self.engine.duration();
        }
        if !self.counted && self.dur > 0.0 && self.current_path.is_some() && self.pos >= 30f64.min(self.dur * 0.5) {
            self.counted = true;
            if let Some(p) = self.current_path.clone() {
                self.stats.count_play(&p);
                self.stats_due = Some(self.ctx.input(|i| i.time) + 5.0);
            }
        }
    }

    fn poll_fonts(&mut self) {
        if let Some(rx) = &self.sysfonts_rx {
            if let Ok(f) = rx.try_recv() {
                self.sysfonts = Some(Arc::new(f));
                self.sysfonts_rx = None;
                let custom = !self.cfg.s("font_family").is_empty() || !self.cfg.s("display_font").is_empty();
                let sys = self.sysfonts.clone();
                let wants = custom
                    || sys.as_ref().is_some_and(|s| {
                        theme::UI_FONTS.iter().take_while(|f| **f != "Inter").any(|f| s.has(f))
                            || theme::DISPLAY_FONTS.iter().take_while(|f| **f != "Doto").any(|f| s.has(f))
                            || theme::MONO_FONTS.iter().any(|f| s.has(f))
                            || theme::FALLBACK_FONTS.iter().any(|f| s.has(f))
                    });
                if wants {
                    self.ctx.set_fonts(theme::font_definitions(&self.cfg, self.sysfonts.as_deref()));
                }
            }
        }
    }

    fn timers(&mut self, now: f64) {
        self.trash.retain(|(p, due)| {
            if now >= *due {
                let _ = std::fs::remove_dir_all(p);
                false
            } else {
                true
            }
        });
        if let Some(due) = self.eq_save_due {
            if now >= due {
                self.eq_save_due = None;
                self.cfg.save();
            } else {
                self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            }
        }
        if let Some(due) = self.stats_due {
            if now >= due {
                self.stats_due = None;
                self.stats.save();
            } else {
                self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            }
        }
        if let Some(tt) = &self.toast {
            if now >= tt.until {
                self.toast = None;
            } else {
                self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(tt.until - now));
            }
        }
    }

    pub fn apply_acts(&mut self) {
        let acts = std::mem::take(&mut self.acts);
        for a in acts {
            self.apply(a);
        }
    }

    fn apply(&mut self, a: Act) {
        match a {
            Act::PlayList(t, i) => self.play_list(t, i),
            Act::PlayShuffled(t) => self.play_shuffled(t),
            Act::PlayQueueIndex(i) => self.play_queue_index(i),
            Act::QueueNext(t) => self.queue_next(t),
            Act::QueueAdd(t) => self.queue_add(t),
            Act::QueueRemove(r) => self.queue_remove(&r),
            Act::QueueClear => self.queue_clear(),
            Act::PlaylistAdd(n, t) => self.playlist_add(&n, t),
            Act::PlaylistNew(t) => self.playlist_new(t),
            Act::PlaylistRemoveRows(n, r) => self.playlist_remove_rows(&n, &r),
            Act::PlaylistRename(n) => {
                if self.page != Page::Playlist(n.clone()) {
                    self.go(Page::Playlist(n.clone()), true);
                }
                self.rename = Some(n);
                self.rename_focus = true;
            }
            Act::PlaylistDelete(n) => self.playlist_delete(&n),
            Act::TogglePlay => self.toggle_play(),
            Act::Next => self.next(false),
            Act::Prev => self.prev(),
            Act::Seek(s) => self.seek(s),
            Act::SeekFrac(f) => self.seek(f64::from(f) * self.dur),
            Act::ToggleShuffle => self.toggle_shuffle(),
            Act::CycleRepeat => self.cycle_repeat(),
            Act::ToggleMute => self.engine.set_muted(!self.engine.muted()),
            Act::SetVolume(v) => self.set_volume(v),
            Act::TogglePanel => {
                let on = !self.cfg.b("now_panel");
                self.cfg.set("now_panel", on);
                self.cfg.save();
            }
            Act::ToggleRemaining => {
                let on = !self.cfg.b("show_remaining");
                self.cfg.set("show_remaining", on);
            }
            Act::Go(p) => self.go(p, true),
            Act::Back => self.back(),
            Act::OpenSettings => {
                if self.layout_name() == "mini" {
                    self.leave_mini();
                }
                if matches!(self.page, Page::Settings(_)) {
                    self.back();
                } else {
                    self.go(Page::Settings(None), true);
                }
            }
            Act::ToggleLyrics => {
                if self.page == Page::Lyrics {
                    self.back();
                } else {
                    self.go(Page::Lyrics, true);
                }
            }
            Act::ToggleVisualizer => {
                if self.page == Page::Visualizer {
                    self.set_immersive(false);
                    self.back();
                } else {
                    self.go(Page::Visualizer, true);
                }
            }
            Act::OpenEq => {
                if self.page == Page::Eq {
                    self.back();
                } else {
                    self.go(Page::Eq, true);
                }
            }
            Act::Rescan(full) => self.rescan(full),
            Act::AddFolder => self.add_folder(),
            Act::OpenFolder(d) => {
                if let Err(e) = open::that_detached(&d) {
                    self.toast(format!("couldn't open {d}: {e}"));
                }
            }
            Act::CopyText(s) => self.ctx.copy_text(s),
            Act::GotoPlayingAlbum => {
                if let Some(t) = self.current_track() {
                    self.go(Page::Album(t.album_key.clone()), true);
                }
            }
            Act::GotoPlayingArtist => {
                if let Some(t) = self.current_track() {
                    self.go(Page::Artists(Some(t.albumartist.clone())), true);
                }
            }
            Act::SwitchLayout(l) => self.switch_layout(&l),
            Act::ToggleMini => {
                if self.layout_name() == "mini" {
                    self.leave_mini();
                } else {
                    self.switch_layout("mini");
                }
            }
            Act::TogglePin => {
                self.pinned = !self.pinned;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.pinned {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
            }
            Act::Toast(s) => self.toast(s),
        }
    }

    pub fn switch_layout(&mut self, name: &str) {
        let old = self.layout_name();
        if name == old {
            return;
        }
        let vp = self.ctx.input(|i| i.viewport().clone());
        if name == "mini" {
            self.cfg.set("layout_before_mini", old.clone());
            if let Some(r) = vp.inner_rect {
                if !vp.maximized.unwrap_or(false) && !vp.fullscreen.unwrap_or(false) {
                    self.cfg.set("size_before_mini", json!([r.width(), r.height()]));
                }
            }
            if vp.maximized.unwrap_or(false) {
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
            self.ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(480.0, 780.0)));
        } else if old == "mini" {
            let size = self.cfg.remove("size_before_mini").and_then(|v| {
                let a = v.as_array()?.clone();
                Some(egui::vec2(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
            });
            let s = size.unwrap_or(egui::vec2(1280.0, 820.0));
            self.ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(s.x.max(900.0), s.y.max(600.0))));
            if self.pinned {
                self.pinned = false;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            }
        }
        self.cfg.set("layout", name);
        self.cfg.save();
        if let Page::Artists(a) = self.page.clone() {
            if name != "classic" {
                self.page = Page::Browse(a);
            }
        }
    }

    pub fn leave_mini(&mut self) {
        let back = self.cfg.s("layout_before_mini");
        let back = if back.is_empty() || back == "mini" { "studio".to_string() } else { back };
        self.switch_layout(&back);
    }

    fn shortcuts(&mut self, ctx: &Context) {
        use egui::{Key, Modifiers};
        let typing = ctx.egui_wants_keyboard_input();
        let mut acts = Vec::new();
        ctx.input_mut(|i| {
            let cmd = Modifiers::COMMAND;
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                acts.push(0);
            }
            if i.consume_key(cmd, Key::F) {
                acts.push(1);
            }
            if typing {
                return;
            }
            let pairs: [(Modifiers, Key, u8); 22] = [
                (Modifiers::NONE, Key::Space, 2),
                (cmd, Key::ArrowRight, 3),
                (cmd, Key::ArrowLeft, 4),
                (Modifiers::NONE, Key::Slash, 1),
                (cmd, Key::Comma, 5),
                (cmd, Key::S, 6),
                (cmd, Key::R, 7),
                (cmd, Key::ArrowUp, 8),
                (cmd, Key::ArrowDown, 9),
                (Modifiers::ALT, Key::ArrowLeft, 10),
                (cmd, Key::B, 11),
                (cmd, Key::Num1, 12),
                (cmd, Key::Num2, 13),
                (cmd, Key::Num3, 14),
                (cmd, Key::Num4, 15),
                (cmd, Key::Num5, 16),
                (cmd, Key::J, 17),
                (cmd, Key::E, 18),
                (cmd, Key::L, 19),
                (cmd, Key::D, 20),
                (cmd, Key::M, 21),
                (cmd, Key::I, 22),
            ];
            for (m, k, id) in pairs {
                if i.consume_key(m, k) {
                    acts.push(id);
                }
            }
            if i.consume_key(cmd, Key::T) {
                acts.push(23);
            }
        });
        for a in acts {
            match a {
                0 => self.on_escape(),
                1 => self.search_focus = true,
                2 => self.toggle_play(),
                3 => self.next(false),
                4 => self.prev(),
                5 => self.apply(Act::OpenSettings),
                6 => self.toggle_shuffle(),
                7 => self.cycle_repeat(),
                8 => self.set_volume(self.cfg.f("volume") + 0.05),
                9 => self.set_volume(self.cfg.f("volume") - 0.05),
                10 => self.back(),
                11 => self.apply(Act::ToggleVisualizer),
                12 => self.go(Page::Albums, true),
                13 => self.go(Page::Browse(None), true),
                14 => self.go(Page::Songs, true),
                15 => self.go(Page::Queue, true),
                16 => self.go(Page::Discover, true),
                17 => self.go(Page::Jam, true),
                18 => self.apply(Act::OpenEq),
                19 => self.apply(Act::ToggleLyrics),
                20 => self.go(Page::Download, true),
                21 => self.apply(Act::ToggleMini),
                22 => {
                    let p = self.current_track().map(|t| t.path.clone());
                    self.go(Page::Inspect(p), true);
                }
                23 => {
                    if let Some(t) = self.current_track() {
                        let p = vec![t.path.clone()];
                        self.go(Page::Tags(p), true);
                    }
                }
                _ => {}
            }
        }
    }

    fn on_escape(&mut self) {
        if self.rename.is_some() {
            self.rename = None;
        } else if self.immersive {
            self.set_immersive(false);
        } else if self.page == Page::Visualizer {
            self.back();
        } else {
            self.search.clear();
        }
    }

    fn handle_drops(&mut self, ctx: &Context) {
        let files: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for f in files {
            let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == nexus_core::skins::SKIN_EXT || (ext == "zip" && nexus_core::skins::zip_has(&f, "skin.json")) {
                self.import_skin(&f);
                return;
            }
            if nexus_core::library::is_audio(&f) {
                self.go(Page::Inspect(Some(f.to_string_lossy().to_string())), true);
                return;
            }
        }
    }

    fn update_title(&mut self) {
        let title = match self.current_track() {
            Some(t) => format!("{} — {}", t.title, t.artist),
            None => "Nexus".to_string(),
        };
        if title != self.title {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// How soon the next frame is needed: the clock, the spinning record, visualizers.
    fn schedule(&mut self, now: f64) {
        let playing = self.engine.state() == State::Playing;
        if self.vis_wanted && (playing || self.analyzer.active()) {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else if playing {
            let spin = self.cfg.b("dial_spin") && self.now_panel_visible();
            self.ctx.request_repaint_after(std::time::Duration::from_millis(if spin { 33 } else { 250 }));
        }
        let fade = now - self.ambient_t0;
        if fade < 1.0 {
            self.ctx.request_repaint();
        }
    }

    pub fn now_panel_visible(&self) -> bool {
        self.cfg.b("now_panel") || matches!(self.layout_name().as_str(), "focus" | "mini")
    }
}

pub fn playback_options(cfg: &Config) -> PlaybackOptions {
    PlaybackOptions {
        replaygain: match cfg.s("replaygain").as_str() {
            "track" => ReplayGain::Track,
            "album" => ReplayGain::Album,
            _ => ReplayGain::Off,
        },
        replaygain_preamp: cfg.f("replaygain_preamp") as f32,
        gapless: cfg.b("gapless"),
        device: cfg.s("audio_device"),
        exclusive: cfg.b("exclusive_mode"),
    }
}

impl eframe::App for Nexus {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let dt = (now - self.last_time).clamp(0.0, 0.2) as f32;
        self.last_time = now;
        if crate::instance::take_show() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.poll_fonts();
        self.covers.pump();
        let hidden = ctx.input(|i| i.viewport().minimized.unwrap_or(false)) && self.cfg.get("bg_video_pause_hidden").map(|v| v.as_bool() != Some(false)).unwrap_or(true);
        self.bg.pump(&ctx, hidden);
        self.poll_engine();
        self.poll_scan();
        self.shortcuts(&ctx);
        self.handle_drops(&ctx);
        if self.engine.state() == State::Playing && self.cfg.b("dial_spin") {
            let secs = (self.cfg.f("dial_spin_seconds") as f32).max(2.0);
            self.dial_angle = (self.dial_angle + dt * 360.0 / secs) % 360.0;
        }
        self.vis_wanted = false;
        crate::layout::draw(self, ui);
        self.apply_acts();
        self.timers(now);
        self.update_title();
        self.schedule(now);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.t.alpha < 1.0 { [0.0, 0.0, 0.0, 0.0] } else { egui::Rgba::from(self.t.c.bg).to_array() }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for (p, _) in self.trash.drain(..) {
            let _ = std::fs::remove_dir_all(p);
        }
        self.save_state();
        self.engine.stop();
        crate::instance::release();
    }
}

/// `{artist} - {title}` style text from a track (the OBS now-playing file).
pub fn format_track(fmt: &str, t: &Track) -> String {
    let mut out = String::new();
    let mut rest = fmt;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let spec = &rest[i + 1..i + j];
        let (name, f) = spec.split_once(':').unwrap_or((spec, ""));
        let num = |n: u32| -> String {
            match f.strip_prefix('0').and_then(|w| w.parse::<usize>().ok()) {
                Some(w) => format!("{n:0w$}"),
                None => n.to_string(),
            }
        };
        out.push_str(&match name {
            "title" => t.title.clone(),
            "artist" => t.artist.clone(),
            "album" => t.album.clone(),
            "albumartist" => t.albumartist.clone(),
            "year" => t.year.clone(),
            "genre" => t.genre.clone(),
            "track" => num(t.track),
            "disc" => num(t.disc),
            "quality" => t.quality(),
            "fmt" | "format" => t.fmt.clone(),
            "duration" => util::fmt_time(t.duration),
            "path" => t.path.clone(),
            _ => String::new(),
        });
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_playing_format() {
        let t = Track { title: "Song".into(), artist: "Me".into(), track: 3, ..Default::default() };
        assert_eq!(format_track("{artist} - {title}", &t), "Me - Song");
        assert_eq!(format_track("{track:02} {title} {nope}", &t), "03 Song ");
    }
}
