//! Discord Rich Presence over the local IPC pipe/socket — no bot, no token. Friends see "Listening to
//! Nexus" with the song; the cover is either your own (uploaded once per album to catbox.moe) or a
//! matching one found on iTunes/Deezer, because Discord can only show public https images.

use crate::net;
use crate::paths::data_dir;
use crate::util::{load_json, save_json, title_norm};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Nexus's own Discord app, so anyone can show what they play without making one.
pub const APP_ID: &str = "1566983013445931018";

/// Everything one update needs (taken from the settings on the UI thread).
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub activity: Option<Value>,
    pub artist: String,
    pub album: String,
    pub title: String,
    pub art_key: String,
    pub cover_file: String,
    pub art_source: String,
    pub art_lookup: bool,
    pub client_id: String,
    pub large_image: String,
    pub small_image: String,
}

enum Msg {
    Update(Box<Update>),
    Reset,
    Quit,
}

pub struct Presence {
    tx: Sender<Msg>,
    status: Arc<Mutex<String>>,
    last: Option<Update>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Presence {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let status = Arc::new(Mutex::new("off".to_string()));
        let st = status.clone();
        let thread = std::thread::Builder::new().name("nexus-discord".into()).spawn(move || Worker::new(st).run(rx)).ok();
        Presence { tx, status, last: None, thread }
    }

    pub fn status(&self) -> String {
        self.status.lock().clone()
    }

    pub fn send(&mut self, u: Update) {
        self.last = Some(u.clone());
        let _ = self.tx.send(Msg::Update(Box::new(u)));
    }

    /// Reconnect (e.g. the app id changed) and re-send the last state.
    pub fn reset(&self) {
        let _ = self.tx.send(Msg::Reset);
        if let Some(u) = &self.last {
            let _ = self.tx.send(Msg::Update(Box::new(u.clone())));
        }
    }

    pub fn clear(&mut self, client_id: &str) {
        self.send(Update { client_id: client_id.into(), ..Default::default() });
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(t) = self.thread.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !t.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

/// Is the Discord app running (its IPC endpoint answers)?
pub fn discord_running() -> bool {
    open().is_some()
}

// ------------------------------------------------------------------ IPC

enum Conn {
    #[cfg(unix)]
    Sock(std::os::unix::net::UnixStream),
    #[cfg_attr(unix, allow(dead_code))]
    Pipe(std::fs::File),
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Conn::Sock(s) => s.read(buf),
            Conn::Pipe(f) => f.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Conn::Sock(s) => s.write(buf),
            Conn::Pipe(f) => f.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Conn::Sock(s) => s.flush(),
            Conn::Pipe(f) => f.flush(),
        }
    }
}

fn candidates() -> Vec<std::path::PathBuf> {
    if cfg!(windows) {
        return (0..10).map(|i| std::path::PathBuf::from(format!(r"\\?\pipe\discord-ipc-{i}"))).collect();
    }
    let mut roots: Vec<String> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(|k| std::env::var(k).ok()).filter(|s| !s.is_empty()).collect();
    roots.push("/tmp".into());
    let subs = ["", "app/com.discordapp.Discord", "snap.discord", "app/dev.vencord.Vesktop", ".flatpak/dev.vencord.Vesktop/xdg-run", "app/com.discordapp.DiscordCanary"];
    let mut out = Vec::new();
    for r in &roots {
        for s in subs {
            for i in 0..10 {
                let p = std::path::Path::new(r).join(s).join(format!("discord-ipc-{i}"));
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn open() -> Option<Conn> {
    for p in candidates() {
        #[cfg(unix)]
        {
            if p.exists() {
                if let Ok(s) = std::os::unix::net::UnixStream::connect(&p) {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
                    return Some(Conn::Sock(s));
                }
            }
        }
        #[cfg(not(unix))]
        {
            if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&p) {
                return Some(Conn::Pipe(f));
            }
        }
    }
    None
}

fn send(c: &mut Conn, op: u32, payload: &Value) -> std::io::Result<()> {
    let data = serde_json::to_vec(payload).unwrap_or_default();
    let mut frame = Vec::with_capacity(8 + data.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(&data);
    c.write_all(&frame)?;
    c.flush()
}

fn recv(c: &mut Conn) -> std::io::Result<(u32, Value)> {
    let mut h = [0u8; 8];
    c.read_exact(&mut h)?;
    let op = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
    let len = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize;
    if len > 1 << 20 {
        return Err(std::io::Error::other("discord sent something huge"));
    }
    let mut body = vec![0u8; len];
    c.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{t:x}-{:x}", N.fetch_add(1, Ordering::Relaxed))
}

// ------------------------------------------------------------------ worker

struct Worker {
    conn: Option<Conn>,
    client: String,
    status: Arc<Mutex<String>>,
    user: String,
    sdt_ok: bool,
    assets: Option<Vec<String>>,
    art_cache: HashMap<String, Option<String>>,
    art_saved: serde_json::Map<String, Value>,
    last: Option<Update>,
}

impl Worker {
    fn new(status: Arc<Mutex<String>>) -> Self {
        Worker {
            conn: None,
            client: String::new(),
            status,
            user: String::new(),
            sdt_ok: true,
            assets: None,
            art_cache: HashMap::new(),
            art_saved: load_json(&data_dir().join("discord_art.json"), serde_json::Map::new()),
            last: None,
        }
    }

    fn set_status(&self, s: impl Into<String>) {
        *self.status.lock() = s.into();
    }

    fn connect(&mut self, cid: &str) -> bool {
        let cid = if cid.trim().is_empty() { APP_ID.to_string() } else { cid.trim().to_string() };
        if !cid.chars().all(|c| c.is_ascii_digit()) {
            self.set_status("add your discord application id in settings → discord");
            return false;
        }
        let Some(mut c) = open() else {
            self.set_status("discord isn't running (or isn't the desktop app)");
            return false;
        };
        let res = send(&mut c, 0, &json!({"v": 1, "client_id": cid})).and_then(|_| recv(&mut c));
        match res {
            Ok((_, msg)) if msg.get("evt").and_then(Value::as_str) == Some("READY") => {
                let user = msg.pointer("/data/user").cloned().unwrap_or(Value::Null);
                let name = |k: &str| user.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
                self.user = name("global_name").or_else(|| name("username")).unwrap_or_else(|| "you".into());
                self.set_status(format!("connected as {}", self.user));
                self.assets = asset_names(&cid);
                self.client = cid;
                self.conn = Some(c);
                true
            }
            Ok((_, msg)) => {
                let m = msg.get("message").or_else(|| msg.pointer("/data/message")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| msg.to_string());
                self.set_status(format!("discord refused: {m}"));
                false
            }
            Err(e) => {
                self.set_status(format!("discord connection failed: {e}"));
                false
            }
        }
    }

    fn set(&mut self, activity: Option<Value>) -> std::io::Result<()> {
        let Some(c) = self.conn.as_mut() else { return Err(std::io::Error::other("not connected")) };
        let payload = json!({"cmd": "SET_ACTIVITY", "args": {"pid": std::process::id(), "activity": activity}, "nonce": nonce()});
        send(c, 1, &payload)?;
        let (_, msg) = recv(c)?;
        if msg.get("evt").and_then(Value::as_str) == Some("ERROR") {
            let err = msg.pointer("/data/message").and_then(Value::as_str).unwrap_or("").to_string();
            if let Some(mut a) = activity {
                if self.sdt_ok && a.get("status_display_type").is_some() {
                    // older Discord: try again without the newer field
                    self.sdt_ok = false;
                    if let Some(o) = a.as_object_mut() {
                        o.remove("status_display_type");
                    }
                    return self.set(Some(a));
                }
            }
            return Err(std::io::Error::other(format!("discord rejected the status: {err}")));
        }
        Ok(())
    }

    fn asset_key(&self, want: &str) -> Option<String> {
        let want = if want.trim().is_empty() { "nexus" } else { want.trim() };
        let Some(names) = &self.assets else { return Some(want.to_string()) };
        for n in [want, "nexus"] {
            if names.iter().any(|x| x == n) {
                return Some(n.to_string());
            }
        }
        names.first().cloned()
    }

    fn run(mut self, rx: Receiver<Msg>) {
        let mut pending: Option<Update> = None;
        let mut retry_at = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(m) => {
                    let mut m = m;
                    while let Ok(n) = rx.try_recv() {
                        m = n; // only the latest matters
                    }
                    match m {
                        Msg::Quit => break,
                        Msg::Reset => {
                            self.conn = None;
                            pending = self.last.clone();
                            retry_at = Instant::now();
                        }
                        Msg::Update(u) => pending = Some(*u),
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let Some(p) = pending.clone() else { continue };
            if Instant::now() < retry_at {
                continue;
            }
            self.last = Some(p.clone());
            let want_client = if p.client_id.trim().is_empty() { APP_ID } else { p.client_id.trim() };
            if self.conn.is_some() && self.client != want_client {
                self.conn = None;
            }
            if self.conn.is_none() && !self.connect(&p.client_id) {
                retry_at = Instant::now() + Duration::from_secs(15);
                continue;
            }
            let res = (|| -> std::io::Result<Option<String>> {
                let Some(mut act) = p.activity.clone() else {
                    self.set(None)?;
                    return Ok(None);
                };
                let logo = self.asset_key(&p.large_image);
                if let Some(assets) = act.get_mut("assets").and_then(Value::as_object_mut) {
                    match logo {
                        Some(l) => assets.insert("large_image".into(), json!(l)),
                        None => assets.remove("large_image"),
                    };
                }
                if p.art_lookup {
                    if !self.art_known(&p) {
                        // show the song right away; the cover follows
                        self.set(Some(act.clone()))?;
                        if !rx.is_empty() {
                            return Ok(Some(String::new()));
                        }
                    }
                    if let Some(art) = self.art(&p) {
                        let small = self.asset_key(&p.small_image);
                        if let Some(assets) = act.get_mut("assets").and_then(Value::as_object_mut) {
                            assets.insert("large_image".into(), json!(art));
                            if let Some(s) = small {
                                assets.insert("small_image".into(), json!(s));
                                assets.insert("small_text".into(), json!("Nexus"));
                            }
                        }
                    }
                }
                if !self.sdt_ok {
                    if let Some(o) = act.as_object_mut() {
                        o.remove("status_display_type");
                    }
                }
                let details = act.get("details").and_then(Value::as_str).unwrap_or("").trim().to_string();
                self.set(Some(act))?;
                Ok(Some(details))
            })();
            match res {
                Ok(Some(d)) if d.is_empty() => continue, // a newer update is waiting
                Ok(Some(d)) => self.set_status(format!("connected as {} · showing “{d}” (discord accepted it)", self.user)),
                Ok(None) => self.set_status(format!("connected as {} · nothing showing (paused or stopped)", self.user)),
                Err(e) => {
                    self.set_status(format!("lost discord: {e}"));
                    self.conn = None;
                    retry_at = Instant::now() + Duration::from_secs(10);
                    continue;
                }
            }
            pending = None;
        }
        if self.conn.is_some() {
            let _ = self.set(None);
        }
    }

    // ---- cover art
    fn art_key(p: &Update) -> String {
        let key = if p.art_key.is_empty() { format!("{}|{}", p.artist, p.album).to_lowercase() } else { p.art_key.clone() };
        format!("{}:{key}", if p.art_source.is_empty() { "upload" } else { &p.art_source })
    }

    fn art_known(&self, p: &Update) -> bool {
        let k = Self::art_key(p);
        self.art_cache.contains_key(&k) || self.art_saved.contains_key(&k)
    }

    fn art(&mut self, p: &Update) -> Option<String> {
        let k = Self::art_key(p);
        if let Some(v) = self.art_cache.get(&k) {
            return v.clone();
        }
        let mut url = self.art_saved.get(&k).and_then(Value::as_str).map(str::to_string);
        if url.is_none() {
            url = if p.art_source == "online" { None } else { upload(&p.cover_file) }.or_else(|| online(&p.artist, &p.album));
            if let Some(u) = &url {
                self.art_saved.insert(k.clone(), json!(u));
                let _ = save_json(&data_dir().join("discord_art.json"), &self.art_saved);
            }
        }
        self.art_cache.insert(k, url.clone());
        url
    }
}

fn asset_names(cid: &str) -> Option<Vec<String>> {
    let v = net::get_json(&format!("https://discord.com/api/v9/oauth2/applications/{cid}/assets")).ok()?;
    Some(v.as_array()?.iter().filter_map(|a| a.get("name").and_then(Value::as_str).map(str::to_string)).collect())
}

fn raw_norm(s: &str) -> String {
    let x: String = s.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { ' ' }).collect();
    x.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A matching cover on iTunes, then Deezer: same artist, exact album title first, then close ones.
pub fn online(artist: &str, album: &str) -> Option<String> {
    let (a, al) = (title_norm(artist), title_norm(album));
    if a.is_empty() || al.is_empty() || al == "music" || al == "unknown album" {
        return None;
    }
    let pick = |items: Vec<(String, String, String)>| -> Option<String> {
        let items: Vec<(String, String, String, String)> = items
            .into_iter()
            .filter(|(_, _, u)| !u.is_empty())
            .map(|(n, w, u)| (title_norm(&n), title_norm(&w), u, raw_norm(&n)))
            .filter(|(_, w, _, _)| !w.is_empty() && (a.contains(w.as_str()) || w.contains(a.as_str())))
            .collect();
        let raw_album = raw_norm(album);
        items
            .iter()
            .find(|x| x.3 == raw_album)
            .or_else(|| items.iter().find(|x| x.0 == al))
            .or_else(|| items.iter().find(|x| al.chars().count() > 3 && (x.0.contains(al.as_str()) || al.contains(x.0.as_str()))))
            .map(|x| x.2.clone())
    };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let q = net::query(&[("term", &format!("{a} {al}")), ("entity", "album"), ("limit", "10")]);
    if let Ok(v) = net::get_json(&format!("https://itunes.apple.com/search?{q}")) {
        let res = v.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
        if let Some(u) = pick(res.iter().map(|x| (s(x, "collectionName"), s(x, "artistName"), s(x, "artworkUrl100"))).collect()) {
            return Some(u.replace("100x100bb", "600x600bb"));
        }
    }
    let q = net::query(&[("q", &format!("artist:\"{a}\" album:\"{al}\"")), ("limit", "10")]);
    if let Ok(v) = net::get_json(&format!("https://api.deezer.com/search/album?{q}")) {
        let res = v.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        return pick(res.iter().map(|x| (s(x, "title"), x.get("artist").map(|a| s(a, "name")).unwrap_or_default(), s(x, "cover_xl"))).collect());
    }
    None
}

/// Your exact cover → catbox.moe (free, no account), shrunk to 512 px. Once per album.
fn upload(cover_file: &str) -> Option<String> {
    if cover_file.is_empty() {
        return None;
    }
    let img = image::open(cover_file).ok()?;
    let img = if img.width().max(img.height()) > 512 { img.resize(512, 512, image::imageops::FilterType::CatmullRom) } else { img };
    let mut jpg = std::io::Cursor::new(Vec::new());
    img.to_rgb8().write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)).ok()?;
    let b = nonce().replace('-', "");
    let mut body = format!("--{b}\r\nContent-Disposition: form-data; name=\"reqtype\"\r\n\r\nfileupload\r\n--{b}\r\nContent-Disposition: form-data; name=\"fileToUpload\"; filename=\"cover.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").into_bytes();
    body.extend_from_slice(&jpg.into_inner());
    body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    let url = std::env::var("NEXUS_ART_UPLOAD").unwrap_or_else(|_| "https://catbox.moe/user/api.php".into());
    let ctype = format!("multipart/form-data; boundary={b}");
    let (code, resp) = net::post(&url, &[("Content-Type", &ctype)], &body, 20).ok()?;
    let text = String::from_utf8_lossy(&resp).trim().to_string();
    (code == 200 && text.starts_with("https://")).then_some(text)
}
