//! Free and legal sources for the Download page: the Internet Archive (incl. the Live Music Archive)
//! and Jamendo (Creative Commons). Albums are fetched one file at a time, checked, then imported.

use crate::library::AUDIO_EXT;
use crate::net;
use crate::util::safe_name;
use parking_lot::Mutex;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Default)]
pub struct WebRow {
    pub source: String,
    pub id: String,
    pub name: String,
    pub artist: String,
    pub year: String,
    pub fmt: String,
    pub size: u64,
    pub info: String,
    pub license: String,
    pub page: String,
    pub cover: String,
    pub files: Option<Vec<WebFile>>,
}

#[derive(Clone, Debug, Default)]
pub struct WebFile {
    pub url: String,
    pub name: String,
    pub size: u64,
    pub md5: String,
    pub title: String,
    pub track: String,
}

/// Only https from the source's own hosts (archive.org serves files from iaNNN.us.archive.org).
pub fn host_ok(url: &str, source: &str) -> bool {
    let Ok(uri) = url.parse::<ureq::http::Uri>() else { return false };
    if uri.scheme_str() != Some("https") {
        return false;
    }
    let h = uri.host().unwrap_or("").to_lowercase();
    let domains: &[&str] = match source {
        "archive" => &["archive.org"],
        "jamendo" => &["jamendo.com"],
        _ => &["archive.org", "jamendo.com"],
    };
    domains.iter().any(|d| h == *d || h.ends_with(&format!(".{d}")))
}

fn json(url: &str) -> Result<Value, String> {
    net::get_json_with(url, &[], 20)
}

fn one(v: Option<&Value>) -> String {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(", "),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(x) => x.to_string(),
    }
}

const AUDIO_FORMATS: [(&str, &str); 7] = [
    ("24bit Flac", "FLAC 24-bit"),
    ("Flac", "FLAC"),
    ("Apple Lossless Audio", "ALAC"),
    ("VBR MP3", "MP3 VBR"),
    ("320Kbps MP3", "MP3 320k"),
    ("Ogg Vorbis", "OGG"),
    ("MP3", "MP3"),
];

pub fn archive_search(text: &str, lossless: bool) -> Result<Vec<WebRow>, String> {
    let words: String = text.chars().map(|c| if c.is_alphanumeric() || c.is_whitespace() || c == '\'' || c == '-' { c } else { ' ' }).collect();
    let words = words.trim();
    if words.is_empty() {
        return Ok(vec![]);
    }
    let fmt = if lossless { r#"(Flac OR "24bit Flac" OR "Apple Lossless Audio")"# } else { r#"(Flac OR "24bit Flac" OR "VBR MP3" OR MP3 OR "Ogg Vorbis")"# };
    let q = format!("({words}) AND mediatype:audio AND format:{fmt} AND -collection:librivoxaudio AND -collection:podcasts");
    let url = format!(
        "https://archive.org/advancedsearch.php?{}",
        net::query(&[("q", &q), ("rows", "60"), ("output", "json"), ("sort[]", "downloads desc"), ("fl", "identifier,title,creator,year,date,item_size,downloads,collection,licenseurl")])
    );
    let d = json(&url)?;
    let re = regex::Regex::new(r"^[A-Za-z0-9._-]{1,120}$").expect("regex");
    let mut rows = Vec::new();
    for x in d.pointer("/response/docs").and_then(Value::as_array).into_iter().flatten() {
        let ident = one(x.get("identifier"));
        if !re.is_match(&ident) {
            continue;
        }
        let coll = x.get("collection");
        let live = match coll {
            Some(Value::Array(a)) => a.iter().any(|c| c.as_str() == Some("etree")),
            Some(Value::String(s)) => s == "etree",
            _ => false,
        };
        let year = Some(one(x.get("year"))).filter(|y| !y.is_empty()).unwrap_or_else(|| one(x.get("date")).chars().take(4).collect());
        let title = one(x.get("title"));
        rows.push(WebRow {
            source: "archive".into(),
            name: if title.is_empty() { ident.clone() } else { title },
            artist: one(x.get("creator")),
            fmt: if lossless { "FLAC".into() } else { String::new() },
            size: x.get("item_size").and_then(Value::as_u64).unwrap_or(0),
            info: format!("{}{}", if live { "live music archive" } else { "internet archive" }, if year.is_empty() { String::new() } else { format!(" · {year}") }),
            license: one(x.get("licenseurl")),
            page: format!("https://archive.org/details/{ident}"),
            cover: format!("https://archive.org/services/img/{ident}"),
            year,
            id: ident,
            files: None,
        });
    }
    Ok(rows)
}

pub fn archive_files(row: &mut WebRow, lossless: bool) -> Result<(Vec<WebFile>, String), String> {
    let d = json(&format!("https://archive.org/metadata/{}", row.id))?;
    let files: Vec<&Value> = d.get("files").and_then(Value::as_array).into_iter().flatten().filter(|f| one(f.get("private")).to_lowercase() != "true").collect();
    let pick = AUDIO_FORMATS
        .iter()
        .find(|(key, label)| files.iter().any(|f| one(f.get("format")) == *key) && (!lossless || label.contains("FLAC") || label.contains("ALAC")));
    let Some((key, label)) = pick else { return Ok((vec![], String::new())) };
    let mut out = Vec::new();
    for f in files.iter().filter(|f| one(f.get("format")) == *key) {
        let name = one(f.get("name"));
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        if !AUDIO_EXT.contains(&ext.as_str()) || name.replace('\\', "/").split('/').any(|c| c == "..") {
            continue;
        }
        let path_enc: Vec<String> = name.split('/').map(net::enc).collect();
        out.push(WebFile {
            url: format!("https://archive.org/download/{}/{}", net::enc(&row.id), path_enc.join("/")),
            name: name.rsplit('/').next().unwrap_or(&name).to_string(),
            size: one(f.get("size")).parse().unwrap_or(0),
            md5: one(f.get("md5")),
            title: one(f.get("title")),
            track: one(f.get("track")),
        });
    }
    let num = |t: &str| t.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u32>().unwrap_or(9999);
    out.sort_by(|a, b| (num(&a.track), a.name.to_lowercase()).cmp(&(num(&b.track), b.name.to_lowercase())));
    if row.artist.is_empty() {
        row.artist = one(d.pointer("/metadata/creator"));
    }
    Ok((out, label.to_string()))
}

fn check_jamendo(d: &Value) -> Result<(), String> {
    match d.pointer("/headers/status").and_then(Value::as_str) {
        None | Some("success") => Ok(()),
        Some(s) => Err(format!("Jamendo: {}", d.pointer("/headers/error_message").and_then(Value::as_str).unwrap_or(s))),
    }
}

pub fn jamendo_search(text: &str, lossless: bool, cid: &str) -> Result<Vec<WebRow>, String> {
    if cid.is_empty() {
        return Err("Jamendo needs a free app id — settings → downloads → Jamendo".into());
    }
    let url = format!(
        "https://api.jamendo.com/v3.0/tracks/?{}",
        net::query(&[
            ("client_id", cid),
            ("format", "json"),
            ("limit", "200"),
            ("search", text),
            ("groupby", "album_id"),
            ("include", "licenses"),
            ("audiodlformat", if lossless { "flac" } else { "mp32" }),
            ("order", "relevance"),
            ("imagesize", "600"),
        ])
    );
    let d = json(&url)?;
    check_jamendo(&d)?;
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in d.get("results").and_then(Value::as_array).into_iter().flatten() {
        let aid = one(t.get("album_id"));
        if aid.is_empty() || !seen.insert(aid.clone()) || t.get("audiodownload_allowed").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = Some(one(t.get("album_name"))).filter(|s| !s.is_empty()).unwrap_or_else(|| one(t.get("name")));
        rows.push(WebRow {
            source: "jamendo".into(),
            name,
            artist: one(t.get("artist_name")),
            year: one(t.get("releasedate")).chars().take(4).collect(),
            fmt: if lossless { "FLAC".into() } else { "MP3".into() },
            info: "jamendo · creative commons".into(),
            license: one(t.get("license_ccurl")),
            page: format!("https://www.jamendo.com/album/{aid}"),
            cover: Some(one(t.get("album_image"))).filter(|s| !s.is_empty()).unwrap_or_else(|| one(t.get("image"))),
            id: aid,
            ..Default::default()
        });
    }
    Ok(rows)
}

pub fn jamendo_files(row: &mut WebRow, lossless: bool, cid: &str) -> Result<(Vec<WebFile>, String), String> {
    let url = format!(
        "https://api.jamendo.com/v3.0/albums/tracks/?{}",
        net::query(&[("client_id", cid), ("format", "json"), ("id", &row.id), ("audiodlformat", if lossless { "flac" } else { "mp32" }), ("imagesize", "600")])
    );
    let d = json(&url)?;
    check_jamendo(&d)?;
    let ext = if lossless { ".flac" } else { ".mp3" };
    let alb = d.pointer("/results/0").cloned().unwrap_or(Value::Null);
    let img = one(alb.get("image"));
    if !img.is_empty() && host_ok(&img, "jamendo") {
        row.cover = img;
    }
    let mut tracks: Vec<Value> = alb.get("tracks").and_then(Value::as_array).cloned().unwrap_or_default();
    tracks.sort_by_key(|t| one(t.get("position")).parse::<u32>().unwrap_or(0));
    let mut out = Vec::new();
    for t in tracks {
        let url = one(t.get("audiodownload"));
        if url.is_empty() || t.get("audiodownload_allowed").and_then(Value::as_bool) == Some(false) || !host_ok(&url, "jamendo") {
            continue;
        }
        let pos: u32 = one(t.get("position")).parse().unwrap_or(0);
        let title = one(t.get("name"));
        out.push(WebFile {
            url,
            name: safe_name(&format!("{pos:02} {}", if title.is_empty() { "track" } else { &title }), "track") + ext,
            title,
            track: pos.to_string(),
            ..Default::default()
        });
    }
    Ok((out, if lossless { "FLAC".into() } else { "MP3".into() }))
}

pub fn search(source: &str, text: &str, lossless: bool, jamendo_id: &str) -> Result<Vec<WebRow>, String> {
    if source == "archive" { archive_search(text, lossless) } else { jamendo_search(text, lossless, jamendo_id) }
}

pub fn files(row: &mut WebRow, lossless: bool, jamendo_id: &str) -> Result<(Vec<WebFile>, String), String> {
    if row.source == "archive" { archive_files(row, lossless) } else { jamendo_files(row, lossless, jamendo_id) }
}

// ------------------------------------------------------------------ downloading

#[derive(Clone, Debug)]
pub struct WebJob {
    pub id: u64,
    pub row: WebRow,
    pub files: Vec<WebFile>,
    pub done: usize,
    pub got: u64,
    pub total: u64,
    /// waiting / downloading / importing / in your library / cancelled / failed (…) / some failed — …
    pub state: String,
    pub error: String,
    pub paths: Vec<PathBuf>,
    pub failed: usize,
}

#[derive(Default)]
struct Shared {
    jobs: Vec<WebJob>,
    cancel: std::collections::HashSet<u64>,
    next_id: u64,
}

/// Downloads web albums one file at a time on a worker thread. Finished albums wait in
/// `take_finished()` for the app to import them.
#[derive(Clone)]
pub struct WebDownloads {
    shared: Arc<Mutex<Shared>>,
    running: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl WebDownloads {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        WebDownloads { shared: Arc::default(), running: Arc::default(), wake: Arc::new(wake) }
    }

    pub fn jobs(&self) -> Vec<WebJob> {
        self.shared.lock().jobs.clone()
    }

    pub fn active(&self) -> bool {
        self.shared.lock().jobs.iter().any(|j| j.state == "waiting" || j.state == "downloading")
    }

    pub fn add(&self, row: WebRow, files: Vec<WebFile>) -> u64 {
        let mut s = self.shared.lock();
        s.next_id += 1;
        let id = s.next_id;
        let total = files.iter().map(|f| f.size).sum();
        s.jobs.push(WebJob { id, row, files, done: 0, got: 0, total, state: "waiting".into(), error: String::new(), paths: vec![], failed: 0 });
        drop(s);
        self.ensure_thread();
        id
    }

    pub fn cancel(&self, id: u64) {
        let mut s = self.shared.lock();
        s.cancel.insert(id);
        if let Some(j) = s.jobs.iter_mut().find(|j| j.id == id && j.state == "waiting") {
            j.state = "cancelled".into();
        }
    }

    pub fn retry(&self, id: u64) {
        let mut s = self.shared.lock();
        s.cancel.remove(&id);
        if let Some(j) = s.jobs.iter_mut().find(|j| j.id == id && (j.state.starts_with("failed") || j.state.starts_with("cancelled") || j.state.starts_with("some"))) {
            j.state = "waiting".into();
            j.done = 0;
            j.got = 0;
            j.failed = 0;
            j.paths.clear();
        }
        drop(s);
        self.ensure_thread();
    }

    pub fn clear_finished(&self) {
        self.shared.lock().jobs.retain(|j| !(j.state == "in your library" || j.state == "cancelled" || j.state.starts_with("failed")));
    }

    /// Albums whose files are all here, ready to import (state becomes "importing").
    pub fn take_finished(&self) -> Vec<WebJob> {
        let mut s = self.shared.lock();
        s.jobs.iter_mut().filter(|j| j.state == "ready").map(|j| {
            j.state = "importing".into();
            j.clone()
        }).collect()
    }

    pub fn set_state(&self, id: u64, state: String) {
        if let Some(j) = self.shared.lock().jobs.iter_mut().find(|j| j.id == id) {
            j.state = state;
        }
    }

    fn ensure_thread(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("nexus-webdl".into())
            .spawn(move || {
                me.run();
                me.running.store(false, Ordering::SeqCst);
            })
            .ok();
    }

    fn run(&self) {
        loop {
            let next = {
                let s = self.shared.lock();
                s.jobs.iter().find(|j| j.state == "waiting" && !s.cancel.contains(&j.id)).cloned()
            };
            let Some(job) = next else { return };
            self.fetch(job);
            (self.wake)();
        }
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut WebJob)) {
        if let Some(j) = self.shared.lock().jobs.iter_mut().find(|j| j.id == id) {
            f(j);
        }
    }

    fn cancelled(&self, id: u64) -> bool {
        self.shared.lock().cancel.contains(&id)
    }

    fn fetch(&self, job: WebJob) {
        let src = job.row.source.clone();
        let folder = crate::paths::data_dir().join("web-downloads").join(job.id.to_string());
        let _ = std::fs::create_dir_all(&folder);
        self.update(job.id, |j| j.state = "downloading".into());
        let mut last = std::time::Instant::now();
        for f in &job.files {
            if self.cancelled(job.id) {
                break;
            }
            let ext = f.name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
            if !host_ok(&f.url, &src) || !AUDIO_EXT.contains(&ext.as_str()) {
                self.update(job.id, |j| j.failed += 1);
                continue;
            }
            let dest = folder.join(safe_name(&f.name, "track.flac"));
            let part = dest.with_file_name(format!("{}.part", dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()));
            let res = (|| -> Result<(), String> {
                let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
                let mut md5 = md5::Context::new();
                let mut w = TeeMd5 { out: &mut out, md5: &mut md5 };
                let id = job.id;
                net::download_from(&f.url, &mut w, 120, |u| host_ok(u, "any"), |done, _| {
                    if last.elapsed().as_millis() > 500 {
                        last = std::time::Instant::now();
                        (self.wake)();
                    }
                    self.update(id, |j| j.got = j.got.max(done));
                    done < 3_000_000_000 && !self.cancelled(id)
                })?;
                let sum = format!("{:x}", md5.finalize());
                if !f.md5.is_empty() && sum != f.md5.to_lowercase() {
                    return Err("checksum mismatch".into());
                }
                std::fs::rename(&part, &dest).map_err(|e| e.to_string())
            })();
            match res {
                Ok(()) => self.update(job.id, |j| {
                    j.paths.push(dest.clone());
                    j.done += 1;
                }),
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    self.update(job.id, |j| {
                        j.failed += 1;
                        j.error = e.chars().take(160).collect();
                    });
                }
            }
            (self.wake)();
        }
        if self.cancelled(job.id) {
            let _ = std::fs::remove_dir_all(&folder);
            self.update(job.id, |j| j.state = "cancelled".into());
            return;
        }
        // cover art next to the files, so it gets imported too
        if !job.row.cover.is_empty() && host_ok(&job.row.cover, &src) {
            if let Ok(d) = net::get_bytes(&job.row.cover, 30, 12_000_000) {
                if d.starts_with(b"\xff\xd8\xff") || d.starts_with(b"\x89PNG\r\n\x1a\n") {
                    let _ = std::fs::write(folder.join(if d.starts_with(b"\x89PNG") { "cover.png" } else { "cover.jpg" }), d);
                }
            }
        }
        self.update(job.id, |j| {
            j.state = if j.paths.is_empty() { format!("failed ({})", if j.error.is_empty() { "nothing downloaded" } else { &j.error }) } else { "ready".into() };
        });
    }
}

struct TeeMd5<'a> {
    out: &'a mut std::fs::File,
    md5: &'a mut md5::Context,
}

impl std::io::Write for TeeMd5<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.out.write(buf)?;
        self.md5.consume(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert!(host_ok("https://ia800.us.archive.org/x.flac", "archive"));
        assert!(!host_ok("http://archive.org/x.flac", "archive"));
        assert!(!host_ok("https://archive.org.evil.com/x", "archive"));
        assert!(host_ok("https://prod-1.storage.jamendo.com/?trackid=1", "jamendo"));
    }
}
