//! Jam (listen together), the app side: what the host tells listeners, what a listener shows, requests to
//! download songs from the host. The connection itself is `nexus_core::jam`; the page is `pages/jam.rs`.
//!
//! While listening along, the player follows the host: the bar shows their song, play/pause pauses just
//! for you, volume is yours, "next" asks the host to skip (if they allow it), seeking is theirs.

use crate::app::{Nexus, Page};
use base64::Engine as _;
use nexus_core::jam::{Event, Jam, Role};
use nexus_core::library::{COVER_NAMES, IMG_EXT, Track};
use nexus_core::lyrics::Lyrics;
use nexus_core::util::safe_name;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub struct Np {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub fmt: String,
    pub cover_key: String,
    pub pos_ms: f64,
    pub dur_ms: f64,
    pub playing: bool,
    pub at: f64,
    pub id: Option<i64>,
}

pub struct LibItem {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
}

pub struct Request {
    pub rid: String,
    pub peer: String,
    pub name: String,
    pub files: Vec<(PathBuf, String, u64)>,
    pub label: String,
    pub text: String,
    pub started: bool,
    pub finished: Option<f64>,
    pub line: String,
}

pub struct JamState {
    pub jam: Jam,
    /// "off" / "joining" / "hosting" / "guest" — which screen the page shows
    pub screen: String,
    pub status: String,
    pub code_in: String,
    pub name_in: String,
    pub chat: Vec<(String, String)>,
    pub chat_in: String,
    pub chat_new: bool,
    // host
    pub people: Vec<(String, String)>,
    pub requests: Vec<Request>,
    lib_paths: Vec<String>,
    lib_rev: HashMap<String, usize>,
    np_next: f64,
    np_seen: (Option<String>, bool),
    np_pos: (f64, f64),
    cover_cache: (String, String),
    // guest
    pub np: Option<Np>,
    pub track: Option<Arc<Track>>,
    pub lib: Vec<LibItem>,
    pub lib_search: String,
    pub lib_view: (String, usize, Vec<usize>),
    pub perm_add: bool,
    pub perm_skip: bool,
    pub dl_mode: String,
    pub my_reqs: HashMap<String, String>,
    pub dl_line: String,
    pub dl_hide_at: f64,
    pub queue: Vec<(String, String)>,
}

impl JamState {
    pub fn new(name: String) -> Self {
        JamState {
            jam: Jam::new(),
            screen: "off".into(),
            status: String::new(),
            code_in: String::new(),
            name_in: name,
            chat: Vec::new(),
            chat_in: String::new(),
            chat_new: false,
            people: Vec::new(),
            requests: Vec::new(),
            lib_paths: Vec::new(),
            lib_rev: HashMap::new(),
            np_next: 0.0,
            np_seen: (None, false),
            np_pos: (0.0, 0.0),
            cover_cache: (String::new(), String::new()),
            np: None,
            track: None,
            lib: Vec::new(),
            lib_search: String::new(),
            lib_view: (String::new(), usize::MAX, Vec::new()),
            perm_add: false,
            perm_skip: false,
            dl_mode: "no".into(),
            my_reqs: HashMap::new(),
            dl_line: String::new(),
            dl_hide_at: 0.0,
            queue: Vec::new(),
        }
    }

    pub fn role(&self) -> Role {
        self.jam.role()
    }

    /// The host's library, filtered by the search box (cached).
    pub fn lib_filtered(&mut self) -> &[usize] {
        if self.lib_view.0 != self.lib_search || self.lib_view.1 != self.lib.len() {
            let words: Vec<String> = self.lib_search.to_lowercase().split_whitespace().map(str::to_string).collect();
            let view: Vec<usize> = self
                .lib
                .iter()
                .enumerate()
                .filter(|(_, x)| {
                    let hay = format!("{} {} {}", x.title, x.artist, x.album).to_lowercase();
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .map(|(i, _)| i)
                .take(300)
                .collect();
            self.lib_view = (self.lib_search.clone(), self.lib.len(), view);
        }
        &self.lib_view.2
    }
}

fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

fn s(d: &Map<String, Value>, k: &str) -> String {
    match d.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

impl Nexus {
    pub fn jam_guest(&self) -> bool {
        self.jam.role() == Role::Guest
    }

    pub fn jam_host(&self) -> bool {
        self.jam.role() == Role::Host
    }

    pub fn jam_name(&self) -> String {
        let n = self.cfg.s("jam_name");
        let n = if n.trim().is_empty() { self.cfg.s("slsk_user") } else { n };
        let n = if n.trim().is_empty() { std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "friend".into()) } else { n };
        n.trim().chars().take(24).collect()
    }

    /// The listener's gain: the volume setting, perceptual, up to 200 %.
    pub fn jam_gain(&self) -> f32 {
        nexus_core::audio::volume_to_gain(self.cfg.f("volume") as f32).min(2.0)
    }

    pub fn jam_start_host(&mut self) {
        let name = self.jam.name_in.trim().to_string();
        self.cfg.set("jam_name", name);
        self.cfg.save();
        let name = self.jam_name();
        match self.jam.jam.host(&name) {
            Ok(tap) => self.engine.set_tap(Some(tap)),
            Err(e) => {
                self.jam.status = format!("couldn't start the jam: {e}");
                self.toast(self.jam.status.clone());
            }
        }
    }

    pub fn jam_join(&mut self) {
        if self.engine.state() == nexus_core::audio::State::Playing {
            self.engine.pause();
        }
        let name = self.jam_name();
        let dev = self.cfg.s("audio_device");
        let code = self.jam.code_in.clone();
        let gain = self.jam_gain();
        if let Err(e) = self.jam.jam.join(&code, &name, &dev, gain) {
            self.jam.status = e.clone();
            self.toast(e);
        }
    }

    pub fn jam_leave(&mut self) {
        self.jam.jam.leave("");
    }

    pub fn jam_say(&mut self) {
        let text: String = self.jam.chat_in.trim().chars().take(500).collect();
        if text.is_empty() {
            return;
        }
        self.jam.chat_in.clear();
        let me = self.jam_name();
        self.jam.jam.send(&json!({"t": "chat", "text": text, "from_name": me}), None, None);
        self.jam_chat_line(&me, &text);
    }

    fn jam_chat_line(&mut self, who: &str, text: &str) {
        self.jam.chat.push((who.to_string(), text.to_string()));
        if self.jam.chat.len() > 500 {
            self.jam.chat.remove(0);
        }
        self.jam.chat_new = true;
    }

    pub fn jam_toggle_pause(&mut self) {
        let ctl = &self.jam.jam.ctl;
        let paused = !ctl.paused.load(Ordering::Relaxed);
        ctl.paused.store(paused, Ordering::Relaxed);
        self.push_presence();
        self.toast_ms(if paused { "paused just for you — the jam keeps going" } else { "back in the jam" }, 2.2);
    }

    pub fn jam_paused(&self) -> bool {
        self.jam.jam.ctl.paused.load(Ordering::Relaxed)
    }

    pub fn jam_host_label(&self) -> String {
        let h = self.jam.jam.host_name();
        if h.is_empty() { "the host".into() } else { h }
    }

    /// Where the host's song is now (ms), and its length (ms).
    pub fn jam_pos(&self) -> (f64, f64) {
        let Some(np) = &self.jam.np else { return (0.0, 0.0) };
        let now = self.ctx.input(|i| i.time);
        let pos = np.pos_ms + if np.playing { (now - np.at) * 1000.0 } else { 0.0 };
        let pos = if np.dur_ms > 0.0 { pos.min(np.dur_ms) } else { pos };
        (pos.max(0.0), np.dur_ms)
    }

    /// The last `n` samples heard (visualizers): the jam's when listening along.
    pub fn vis_source(&self, n: usize) -> (Vec<f32>, u32) {
        if self.jam_guest() {
            (self.jam.jam.ctl.vis_samples(n), 48_000)
        } else {
            (self.engine.vis_samples(n), self.engine.sample_rate())
        }
    }

    // ---------------------------------------------------------------- the frame

    pub fn jam_tick(&mut self, now: f64) {
        for e in self.jam.jam.poll() {
            match e {
                Event::State(st, detail) => self.jam_state(&st, &detail),
                Event::Msg(m) => self.jam_msg(m, now),
            }
        }
        match self.jam.role() {
            Role::Host => {
                // tell listeners about play/pause and seeks straight away, and everything every 5 s
                let seen = (self.current_track().map(|t| t.path.clone()), self.playing());
                let jumped = {
                    let (p0, t0) = self.jam.np_pos;
                    self.playing() && (self.pos - (p0 + (now - t0))).abs() > 2.5
                };
                self.jam.np_pos = (self.pos, now);
                if seen.1 != self.jam.np_seen.1 || jumped || now >= self.jam.np_next {
                    self.jam.np_seen = seen;
                    self.jam_broadcast_np(false, now);
                }
                if !self.jam.people.is_empty() {
                    self.ctx.request_repaint_after(std::time::Duration::from_secs(1));
                }
                let forget: Vec<String> = self.jam.requests.iter().filter(|r| r.finished.is_some_and(|f| now - f > 15.0)).map(|r| r.rid.clone()).collect();
                self.jam.requests.retain(|r| !forget.contains(&r.rid));
            }
            Role::Guest => {
                let (pos, dur) = self.jam_pos();
                self.pos = pos / 1000.0;
                self.dur = dur / 1000.0;
                if self.jam.np.as_ref().is_some_and(|n| n.playing) {
                    self.ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
                if !self.jam.dl_line.is_empty() && self.jam.dl_hide_at > 0.0 && now >= self.jam.dl_hide_at {
                    self.jam.dl_line.clear();
                    self.jam.dl_hide_at = 0.0;
                }
            }
            Role::Off => {}
        }
    }

    fn jam_state(&mut self, st: &str, detail: &str) {
        let was_guest = self.jam.screen == "guest" || self.jam.screen == "joining";
        match st {
            "hosting" => {
                let (code, err) = detail.split_once('|').unwrap_or((detail, ""));
                log::debug!("jam: hosting {code}");
                if self.jam.screen != "hosting" {
                    self.jam.people.clear();
                    self.jam.requests.clear();
                    self.jam.chat.clear();
                }
                self.jam.screen = "hosting".into();
                self.jam.status = if err.is_empty() {
                    "send this code to your friend — they open Nexus → Jam → Join. they'll hear whatever you play".into()
                } else {
                    err.to_string()
                };
                self.jam.np_next = 0.0;
            }
            "joining" => {
                self.jam.screen = "joining".into();
                self.jam.status = format!("connecting to {detail}…");
                self.jam.chat.clear();
            }
            "guest" => {
                self.jam.screen = "guest".into();
                self.jam.status = "connected — music starts in a moment".into();
                let me = self.jam_name();
                self.jam.jam.send(&json!({"t": "hello", "name": me}), None, None);
                if self.page != Page::Jam {
                    self.go(Page::Jam, true);
                }
            }
            "error" => {
                self.jam.screen = "off".into();
                self.jam.status = detail.to_string();
                let short: String = detail.chars().take(140).collect();
                self.toast(short);
            }
            _ => {
                // off
                if self.jam.screen == "hosting" {
                    self.engine.set_tap(None);
                }
                self.jam.screen = "off".into();
                self.jam.status = if detail.is_empty() { "listen together with a friend — one of you hosts, the other joins".into() } else { detail.to_string() };
                self.jam.people.clear();
                self.jam.requests.clear();
            }
        }
        if was_guest && !matches!(st, "guest" | "joining") {
            self.jam_guest_ended();
        }
        self.push_presence();
    }

    /// Back to your own player.
    fn jam_guest_ended(&mut self) {
        let dir = nexus_core::paths::covers_dir();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with("jam-") && n.ends_with(".jpg") {
                    let _ = std::fs::remove_file(e.path());
                    self.covers.forget(n.trim_end_matches(".jpg"));
                }
            }
        }
        self.jam.np = None;
        self.jam.track = None;
        self.jam.lib.clear();
        self.jam.queue.clear();
        self.jam.my_reqs.clear();
        self.jam.dl_line.clear();
        self.jam.perm_add = false;
        self.jam.perm_skip = false;
        self.jam.dl_mode = "no".into();
        self.jam.jam.ctl.paused.store(false, Ordering::Relaxed);
        self.pos = self.engine.position();
        self.dur = self.current_track().map(|t| t.duration).unwrap_or(0.0);
        if self.engine.duration() > 0.0 {
            self.dur = self.engine.duration();
        }
        if let Some(t) = self.current_track().cloned() {
            self.set_ambient(&t.cover.clone());
        } else {
            self.set_ambient("");
        }
    }

    fn jam_msg(&mut self, d: Map<String, Value>, now: f64) {
        let t = s(&d, "t");
        match self.jam.role() {
            Role::Host => {
                let peer = s(&d, "peer");
                let name = {
                    let n = s(&d, "name");
                    if n.is_empty() { "friend".to_string() } else { n }
                };
                match t.as_str() {
                    "joined" => {
                        self.jam.people.retain(|(p, _)| *p != peer);
                        self.jam.people.push((peer.clone(), name.clone()));
                        self.toast(format!("{name} joined your jam"));
                        for part in self.jam_library_payload() {
                            self.jam.jam.send(&part, Some(&peer), None);
                        }
                        self.jam_send_perm(Some(&peer));
                        let np = self.jam_np_payload(true);
                        self.jam.jam.send(&np, Some(&peer), None);
                    }
                    "left" => {
                        self.jam.people.retain(|(p, _)| *p != peer);
                        let failed = d.get("failed").and_then(Value::as_bool).unwrap_or(false);
                        self.toast(format!("{name} {}", if failed { "lost the connection" } else { "left" }));
                    }
                    "chat" => {
                        let from = {
                            let f = s(&d, "from_name");
                            if f.is_empty() { name.clone() } else { f }
                        };
                        let text = s(&d, "text");
                        self.jam_chat_line(&from, &text);
                        self.jam.jam.send(&json!({"t": "chat", "text": text, "from_name": from}), None, Some(&peer));
                    }
                    "add" if self.cfg.b("jam_allow_add") => {
                        let tr = d.get("id").and_then(Value::as_u64).and_then(|i| self.jam.lib_paths.get(i as usize)).and_then(|p| self.lib.track(p).cloned());
                        if let Some(tr) = tr {
                            self.queue_add(vec![tr.clone()]);
                            self.toast(format!("{name} added “{}”", tr.title));
                            self.jam_chat_line("♪", &format!("{name} added {} — {}", tr.title, tr.artist));
                            self.jam_broadcast_np(false, now);
                        }
                    }
                    "skip" if self.cfg.b("jam_allow_skip") => {
                        self.toast(format!("{name} skipped"));
                        self.next(false);
                    }
                    "dlreq" => self.jam_got_request(&peer, &name, &d),
                    "dlcancel" => self.jam.jam.cancel_send(&s(&d, "rid")),
                    "fsent" => self.jam_sent_progress(&d, now),
                    _ => {}
                }
            }
            Role::Guest => match t.as_str() {
                "np" => self.jam_got_np(&d, now),
                "lib" => {
                    if self.jam.lib.len() < 100_000 {
                        if let Some(Value::Array(items)) = d.get("items") {
                            for x in items {
                                let Some(a) = x.as_array() else { continue };
                                let Some(id) = a.first().and_then(Value::as_i64) else { continue };
                                let txt = |i: usize| a.get(i).and_then(Value::as_str).unwrap_or("").chars().take(200).collect::<String>();
                                self.jam.lib.push(LibItem { id, title: txt(1), artist: txt(2), album: txt(3) });
                            }
                        }
                    }
                }
                "perm" => {
                    self.jam.dl_mode = {
                        let m = s(&d, "dl");
                        if m.is_empty() { "no".into() } else { m }
                    };
                    self.jam.perm_add = d.get("add").and_then(Value::as_bool).unwrap_or(false);
                    self.jam.perm_skip = d.get("skip").and_then(Value::as_bool).unwrap_or(false);
                }
                "chat" => {
                    let from = s(&d, "from_name");
                    let text = s(&d, "text");
                    self.jam_chat_line(if from.is_empty() { "?" } else { &from }, &text);
                }
                "dlno" => {
                    let label = self.jam.my_reqs.remove(&s(&d, "rid")).unwrap_or_else(|| "that".into());
                    let why = s(&d, "why");
                    let msg = format!("{} didn't send {label}{}", self.jam_host_label(), if why.is_empty() { String::new() } else { format!(" ({why})") });
                    self.jam_dl_line(&msg, 6.0, now);
                    self.toast(msg);
                }
                "fprog" => {
                    let label = self.jam.my_reqs.get(&s(&d, "rid")).cloned().unwrap_or_default();
                    let num = |k: &str| d.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    let pct = (100.0 * num("got") / num("total").max(1.0)) as i64;
                    self.jam.dl_line = format!("⤓ {label} — song {}/{} · {pct}%  ({:.0} / {:.0} MB)", num("i") as i64 + 1, num("n") as i64, num("got") / 1e6, num("total") / 1e6);
                    self.jam.dl_hide_at = 0.0;
                }
                "fdone" => self.jam_received(&d, now),
                "kicked" => {
                    self.jam.jam.leave("the host removed you from the jam");
                    self.toast("the host removed you from the jam");
                }
                _ => {}
            },
            Role::Off => {}
        }
    }

    fn jam_dl_line(&mut self, text: &str, secs: f64, now: f64) {
        self.jam.dl_line = text.to_string();
        self.jam.dl_hide_at = if secs > 0.0 { now + secs } else { 0.0 };
    }

    // ---------------------------------------------------------------- host: telling listeners what's on

    fn jam_library_payload(&mut self) -> Vec<Value> {
        self.jam.lib_paths = self.lib.tracks.iter().map(|t| t.path.clone()).collect();
        self.jam.lib_rev = self.jam.lib_paths.iter().enumerate().map(|(i, p)| (p.clone(), i)).collect();
        let items: Vec<Value> = self.lib.tracks.iter().enumerate().map(|(i, t)| json!([i, t.title, t.artist, t.album, t.duration as i64])).collect();
        items.chunks(250).map(|c| json!({"t": "lib", "items": c})).collect()
    }

    pub fn jam_send_perm(&mut self, peer: Option<&str>) {
        if !self.jam_host() {
            return;
        }
        let dl = {
            let m = self.cfg.s("jam_allow_download");
            if m.is_empty() { "ask".to_string() } else { m }
        };
        let v = json!({"t": "perm", "add": self.cfg.b("jam_allow_add"), "skip": self.cfg.b("jam_allow_skip"), "dl": dl});
        self.jam.jam.send(&v, peer, None);
    }

    /// The host's cover as a small JPEG (base64), cached for the current cover.
    fn jam_cover_b64(&mut self, key: &str) -> String {
        if key.is_empty() {
            return String::new();
        }
        if self.jam.cover_cache.0 == key {
            return self.jam.cover_cache.1.clone();
        }
        let path = nexus_core::paths::covers_dir().join(format!("{key}.jpg"));
        let b64 = image::open(&path)
            .ok()
            .map(|img| img.thumbnail(300, 300).to_rgb8())
            .and_then(|img| {
                let mut out = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&img).ok()?;
                Some(base64::engine::general_purpose::STANDARD.encode(out))
            })
            .unwrap_or_default();
        self.jam.cover_cache = (key.to_string(), b64.clone());
        b64
    }

    pub fn jam_np_payload(&mut self, with_cover: bool) -> Value {
        let cur = self.current_track().cloned();
        let mut d = json!({"t": "np", "playing": self.playing(), "pos": (self.pos * 1000.0) as i64, "dur": (self.dur * 1000.0) as i64});
        if let Some(t) = &cur {
            d["title"] = json!(t.title);
            d["artist"] = json!(t.artist);
            d["album"] = json!(t.album);
            d["fmt"] = json!(t.quality());
            if let Some(i) = self.jam.lib_rev.get(&t.path) {
                d["id"] = json!(i);
            }
            if with_cover {
                let b = self.jam_cover_b64(&t.cover);
                if !b.is_empty() {
                    d["cover"] = json!(b);
                }
                if let Some(ly) = self.lyrics.get(&t.path) {
                    let plain: Option<String> = ly.plain.as_ref().map(|p| p.chars().take(20000).collect());
                    d["lyrics"] = json!({"synced": ly.synced.as_ref().map(|s| s.iter().map(|(ms, l)| json!([ms, l])).collect::<Vec<_>>()), "plain": plain, "source": "host"});
                }
            }
            let qi = self.qi.unwrap_or(0);
            d["queue"] = json!(self.queue.iter().skip(qi + 1).take(8).map(|x| json!([x.title, x.artist])).collect::<Vec<_>>());
        } else {
            d["queue"] = json!([]);
        }
        d
    }

    pub fn jam_broadcast_np(&mut self, with_cover: bool, now: f64) {
        self.jam.np_next = now + 5.0;
        if self.jam_host() && !self.jam.people.is_empty() {
            let v = self.jam_np_payload(with_cover);
            self.jam.jam.send(&v, None, None);
        }
    }

    /// The host's song changed: everyone gets the new cover and lyrics.
    pub fn jam_track_changed(&mut self) {
        if self.jam_host() {
            let now = self.ctx.input(|i| i.time);
            self.jam_broadcast_np(true, now);
        }
    }

    // ---------------------------------------------------------------- guest: what's on at the host

    fn jam_save_cover(&self, b64: &str) -> String {
        use sha2::Digest;
        let Ok(data) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) else { return String::new() };
        if image::load_from_memory(&data).is_err() {
            return String::new();
        }
        let h = sha2::Sha256::digest(&data);
        let key = format!("jam-{}", h.iter().take(7).map(|b| format!("{b:02x}")).collect::<String>());
        let p = nexus_core::paths::covers_dir().join(format!("{key}.jpg"));
        if !p.exists() {
            let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new(".")));
            if std::fs::write(&p, &data).is_err() {
                return String::new();
            }
        }
        key
    }

    fn jam_got_np(&mut self, d: &Map<String, Value>, now: f64) {
        let title = s(d, "title");
        let artist = s(d, "artist");
        let old = self.jam.np.as_ref();
        let new_song = old.is_none_or(|o| o.title != title || o.artist != artist);
        let cover_key = match d.get("cover").and_then(Value::as_str) {
            Some(c) if !c.is_empty() => self.jam_save_cover(c),
            _ if !new_song => old.map(|o| o.cover_key.clone()).unwrap_or_default(),
            _ => String::new(),
        };
        let num = |k: &str| d.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let np = Np {
            title: title.clone(),
            artist: artist.clone(),
            album: s(d, "album"),
            fmt: s(d, "fmt"),
            cover_key: cover_key.clone(),
            pos_ms: num("pos"),
            dur_ms: num("dur"),
            playing: d.get("playing").and_then(Value::as_bool).unwrap_or(false),
            at: now,
            id: d.get("id").and_then(Value::as_i64),
        };
        self.jam.queue = d
            .get("queue")
            .and_then(Value::as_array)
            .map(|q| q.iter().filter_map(|x| x.as_array()).map(|a| (a[0].as_str().unwrap_or("").to_string(), a[1].as_str().unwrap_or("").to_string())).collect())
            .unwrap_or_default();
        let path = format!("jam:{artist}:{title}");
        if new_song || d.contains_key("cover") || d.contains_key("lyrics") {
            let ly = d.get("lyrics").filter(|l| l.is_object()).map(|l| Lyrics {
                synced: l.get("synced").and_then(Value::as_array).map(|a| {
                    a.iter().filter_map(|x| x.as_array()).filter_map(|x| Some((x.first()?.as_f64()? as u64, x.get(1)?.as_str()?.to_string()))).collect()
                }),
                plain: l.get("plain").and_then(Value::as_str).map(str::to_string),
                source: "host".into(),
            });
            if new_song || ly.is_some() {
                self.lyrics.put(&path, ly);
            }
        }
        let track = Track {
            path,
            title: if title.is_empty() { "nothing playing".into() } else { title.clone() },
            artist: artist.clone(),
            album: np.album.clone(),
            albumartist: artist,
            fmt: np.fmt.clone(),
            cover: cover_key.clone(),
            duration: np.dur_ms / 1000.0,
            ..Default::default()
        };
        let changed = new_song || self.jam.track.as_ref().is_none_or(|t| t.cover != track.cover);
        self.jam.track = if title.is_empty() { None } else { Some(Arc::new(track)) };
        self.jam.np = Some(np);
        if changed {
            self.set_ambient(&cover_key);
        }
        self.push_presence();
    }

    // ---------------------------------------------------------------- downloads between jam members

    /// Guest: ask the host for a song or its whole album.
    pub fn jam_request_download(&mut self, id: Option<i64>, what: &str) {
        let now = self.ctx.input(|i| i.time);
        let Some(id) = id else {
            self.toast("can't tell which song that is yet — wait a second and try again");
            return;
        };
        let info = self.jam.lib.iter().find(|x| x.id == id);
        let label = match info {
            Some(x) if what == "album" => format!("“{}”", x.album),
            Some(x) => format!("“{}”", x.title),
            None => format!("this {what}"),
        };
        let rid: String = format!("{:08x}", fastrand::u32(..));
        self.jam.my_reqs.insert(rid.clone(), label.clone());
        self.jam.jam.expect_files(&rid);
        self.jam.jam.send(&json!({"t": "dlreq", "rid": rid, "id": id, "what": what}), None, None);
        let host = self.jam_host_label();
        let line = if self.jam.dl_mode == "ask" { format!("asked {host} for {label}…") } else { format!("getting {label}…") };
        self.jam_dl_line(&line, 0.0, now);
    }

    fn jam_received(&mut self, d: &Map<String, Value>, now: f64) {
        let rid = s(d, "rid");
        let label = self.jam.my_reqs.remove(&rid).unwrap_or_else(|| "the download".into());
        let paths: Vec<PathBuf> = d.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect()).unwrap_or_default();
        let incoming = Jam::incoming_dir(&rid);
        // only files the jam itself put in our incoming folder
        let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.starts_with(&incoming)).collect();
        let root = self.jam_import_root();
        let done = if paths.is_empty() { Default::default() } else { nexus_core::importer::import_files(&paths, &root, true) };
        let _ = std::fs::remove_dir_all(&incoming);
        let n = done.files.len();
        let msg = if n > 0 {
            format!("saved {label} to your library ({})", plural(n, "song"))
        } else {
            format!("{label} didn't come through{}", if d.get("ok").and_then(Value::as_bool).unwrap_or(false) { "" } else { " (stopped)" })
        };
        if n > 0 {
            if let Some(im) = &mut self.importing {
                im.request_rescan(now);
            }
            self.ctx.request_repaint_after(std::time::Duration::from_millis(2600));
        }
        self.jam_dl_line(&msg, 8.0, now);
        self.toast(msg);
    }

    fn jam_import_root(&mut self) -> PathBuf {
        let folders = self.cfg.strings("library_folders");
        let root = nexus_core::importer::root(&self.cfg.s("import_root"), &folders);
        let rs = root.to_string_lossy().to_string();
        if !folders.iter().any(|f| rs.starts_with(f.as_str())) {
            let mut f = folders;
            f.push(rs);
            self.cfg.set("library_folders", json!(f));
            self.cfg.save();
        }
        root
    }

    fn jam_got_request(&mut self, peer: &str, name: &str, d: &Map<String, Value>) {
        let rid: String = s(d, "rid").chars().take(16).collect();
        let mode = {
            let m = self.cfg.s("jam_allow_download");
            if m.is_empty() { "ask".to_string() } else { m }
        };
        let tr = d.get("id").and_then(Value::as_u64).and_then(|i| self.jam.lib_paths.get(i as usize)).and_then(|p| self.lib.track(p).cloned());
        let Some(tr) = tr.filter(|_| mode != "no") else {
            let why = if mode == "no" { "downloads are off" } else { "song not found" };
            self.jam.jam.send(&json!({"t": "dlno", "rid": rid, "why": why}), Some(peer), None);
            return;
        };
        if self.jam.requests.iter().filter(|r| r.peer == peer && r.finished.is_none()).count() >= 3 {
            self.jam.jam.send(&json!({"t": "dlno", "rid": rid, "why": "one at a time — wait for the others to finish"}), Some(peer), None);
            return;
        }
        let album = s(d, "what") == "album";
        let tracks: Vec<Arc<Track>> = if album {
            match self.lib.album_index.get(&tr.album_key).and_then(|&i| self.lib.albums.get(i)) {
                Some(a) => a.tracks.iter().filter_map(|&i| self.lib.tracks.get(i).cloned()).collect(),
                None => vec![tr.clone()],
            }
        } else {
            vec![tr.clone()]
        };
        let who = if tr.albumartist.is_empty() { tr.artist.clone() } else { tr.albumartist.clone() };
        let label = if album { format!("“{}” by {who}", tr.album) } else { format!("“{}” by {}", tr.title, tr.artist) };
        let folder = format!("{}/{}", safe_name(&who, "Unknown Artist"), safe_name(&tr.album, "Unknown Album"));
        let mut files: Vec<(PathBuf, String, u64)> = Vec::new();
        for x in &tracks {
            let p = PathBuf::from(&x.path);
            if let Ok(m) = std::fs::metadata(&p) {
                let base = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                files.push((p, format!("{folder}/{base}"), m.len()));
            }
        }
        if album && !files.is_empty() {
            if let Some(dir) = files[0].0.parent().map(|d| d.to_path_buf()) {
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        let p = e.path();
                        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                        let stem = p.file_stem().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                        if IMG_EXT.contains(&ext.as_str()) && COVER_NAMES.contains(&stem.as_str()) {
                            if let Ok(m) = e.metadata() {
                                let fname = e.file_name().to_string_lossy().to_string();
                                files.push((p, format!("{folder}/{fname}"), m.len()));
                            }
                        }
                    }
                }
            }
        }
        if files.is_empty() {
            self.jam.jam.send(&json!({"t": "dlno", "rid": rid, "why": "files missing"}), Some(peer), None);
            return;
        }
        let n = files.iter().filter(|f| nexus_core::library::is_audio(&f.0)).count();
        let size = nexus_core::util::human_size(files.iter().map(|f| f.2).sum::<u64>());
        let text = format!("{name} · {label} ({}, {size})", plural(n, "song"));
        self.jam.requests.push(Request {
            rid: rid.clone(),
            peer: peer.to_string(),
            name: name.to_string(),
            files,
            label: label.clone(),
            text,
            started: false,
            finished: None,
            line: "waiting for you to send or decline".into(),
        });
        if mode == "yes" {
            self.jam_allow_request(&rid);
        } else {
            let r2 = rid.clone();
            self.toast_action(format!("{name} wants {label} ({size})"), "send", move |app| app.jam_allow_request(&r2), 20.0);
            self.jam_chat_line("⤓", &format!("{name} asked to download {label}"));
        }
    }

    pub fn jam_allow_request(&mut self, rid: &str) {
        let Some(r) = self.jam.requests.iter_mut().find(|r| r.rid == rid && !r.started) else { return };
        r.started = true;
        r.line = "sending…".into();
        let (peer, files) = (r.peer.clone(), r.files.clone());
        self.jam.jam.send_files(&peer, rid, files);
    }

    pub fn jam_decline_request(&mut self, rid: &str) {
        let Some(i) = self.jam.requests.iter().position(|r| r.rid == rid) else { return };
        let r = self.jam.requests.remove(i);
        if r.started {
            self.jam.jam.cancel_send(rid);
        } else {
            self.jam.jam.send(&json!({"t": "dlno", "rid": rid, "why": "declined"}), Some(&r.peer), None);
        }
    }

    fn jam_sent_progress(&mut self, d: &Map<String, Value>, now: f64) {
        let rid = s(d, "rid");
        let Some(r) = self.jam.requests.iter_mut().find(|r| r.rid == rid) else { return };
        let num = |k: &str| d.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        if d.get("done").and_then(Value::as_bool).unwrap_or(false) {
            let ok = d.get("ok").and_then(Value::as_bool).unwrap_or(false);
            r.finished = Some(now);
            r.line = if ok { "sent ✓".into() } else { "stopped".into() };
            if ok {
                let msg = format!("sent {} to {}", r.label, r.name);
                self.toast(msg);
            }
        } else {
            let pct = (100.0 * num("sent") / num("total").max(1.0)) as i64;
            r.line = format!("sending {}/{} · {pct}%", num("i") as i64 + 1, num("n") as i64);
        }
    }

    pub fn jam_kick(&mut self, peer: &str) {
        self.jam.jam.kick(peer);
    }

    pub fn jam_add_song(&mut self, idx: usize) {
        let Some(x) = self.jam.lib.get(idx) else { return };
        let (id, title) = (x.id, x.title.clone());
        self.jam.jam.send(&json!({"t": "add", "id": id}), None, None);
        self.toast(format!("asked to add “{title}”"));
    }

    /// The nav button's tooltip while a jam is on ("" otherwise).
    pub fn jam_nav_tip(&self) -> String {
        match self.jam.role() {
            Role::Host => format!("jam: hosting · code {}", self.jam.jam.code()),
            Role::Guest => "jam: listening along".to_string(),
            Role::Off => String::new(),
        }
    }

    pub fn jam_invite_text(&self) -> String {
        format!("join my Nexus jam: open Nexus → Jam (the people icon) → code {}\ndon't have Nexus? https://github.com/GameBitBan/Nexus", self.jam.jam.code())
    }
}
