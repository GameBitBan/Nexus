//! The Download page's brains: turning search answers into album rows, picking the best copy,
//! remembering unfinished downloads across restarts (`downloads_pending.json`, same as Python Nexus).

use crate::library::{AUDIO_EXT, IMG_EXT};
use crate::paths::data_dir;
use crate::slsk::{RFile, Response, remote_split};
use crate::util::{load_json, save_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const LOSSLESS_EXT: [&str; 8] = ["flac", "wav", "aiff", "aif", "wv", "ape", "alac", "dsf"];
pub const NOISE_WORDS: [&str; 19] =
    ["deluxe", "edition", "remaster", "remastered", "version", "explicit", "expanded", "anniversary", "bonus", "tracks", "album", "ep", "lp", "cd", "flac", "mp3", "the", "official", "full"];

fn ext(name: &str) -> String {
    let base = remote_split(name).1;
    base.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

pub fn is_audio_name(name: &str) -> bool {
    AUDIO_EXT.contains(&ext(name).as_str())
}

pub fn is_image_name(name: &str) -> bool {
    IMG_EXT.contains(&ext(name).as_str())
}

/// An album-ish search row: one user's folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Row {
    pub user: String,
    pub dir: String,
    pub name: String,
    pub audio: Vec<RFile>,
    pub images: Vec<RFile>,
    pub fmt: String,
    pub size: u64,
    pub free: bool,
    pub speed: u64,
    pub queue: u32,
    pub lossless: bool,
    pub score: f64,
    pub length: u64,
}

/// Search answers → rows (one per user + folder), best first.
pub fn group_results(responses: &[Response], lossless_only: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    for r in responses {
        let mut dirs: Vec<(String, Vec<&RFile>)> = Vec::new();
        for f in &r.files {
            let d = remote_split(&f.filename).0;
            match dirs.iter_mut().find(|(x, _)| *x == d) {
                Some((_, v)) => v.push(f),
                None => dirs.push((d, vec![f])),
            }
        }
        for (d, fs) in dirs {
            let audio: Vec<RFile> = fs.iter().filter(|f| is_audio_name(&f.filename)).map(|f| (*f).clone()).collect();
            if audio.is_empty() {
                continue;
            }
            let exts: std::collections::BTreeSet<String> = audio.iter().map(|f| ext(&f.filename)).collect();
            let lossless = exts.iter().all(|e| LOSSLESS_EXT.contains(&e.as_str()));
            if lossless_only && !lossless {
                continue;
            }
            let bd = audio.iter().filter_map(|f| f.bit_depth).max().unwrap_or(0);
            let sr = audio.iter().filter_map(|f| f.sample_rate).max().unwrap_or(0);
            let br = audio.iter().filter_map(|f| f.bit_rate).max().unwrap_or(0);
            let mut fmt = exts.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join("/");
            if bd > 0 && sr > 0 {
                let k = f64::from(sr) / 1000.0;
                fmt += &format!(" {bd}/{}", if k.fract() == 0.0 { format!("{k:.0}") } else { format!("{k}") });
            } else if br > 0 {
                fmt += &format!(" {br}k");
            }
            let images: Vec<RFile> = fs.iter().filter(|f| is_image_name(&f.filename)).map(|f| (*f).clone()).collect();
            let size: u64 = audio.iter().map(|f| f.size).sum();
            let score = if lossless { 2.0 } else { 0.0 } + if bd >= 24 { 1.0 } else { 0.0 } + if r.free_slot { 3.0 } else { 0.0 } + (r.speed as f64 / 2_000_000.0).min(2.0)
                - (f64::from(r.queue) / 20.0).min(2.0)
                + (audio.len() as f64 / 10.0).min(1.0);
            let name = remote_split(&d).1;
            rows.push(Row {
                user: r.username.clone(),
                name: if name.is_empty() { d.clone() } else { name },
                dir: d,
                length: audio.iter().filter_map(|f| f.length).map(u64::from).sum(),
                audio,
                images,
                fmt,
                size,
                free: r.free_slot,
                speed: r.speed,
                queue: r.queue,
                lossless,
                score,
            });
        }
    }
    rows.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    rows
}

/// Tidies what people type: lone dashes ("Artist - Album") and punctuation. "-word" (exclude) is kept.
pub fn clean_query(text: &str) -> String {
    let re_dash = regex::Regex::new(r"(^|\s)[-–—]+(\s|$)").expect("regex");
    let mut t = text.to_string();
    // repeated because matches can't overlap ("a - - b")
    for _ in 0..3 {
        t = re_dash.replace_all(&t, " ").into_owned();
    }
    let t: String = t.chars().map(|c| if "\"“”(){}[]:;,!?.".contains(c) { ' ' } else { c }).collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Fallbacks when a search finds nothing: drop deluxe/edition-type words, then keep the first few.
pub fn broader_queries(text: &str) -> Vec<String> {
    let q = clean_query(text);
    let words: Vec<&str> = q.split_whitespace().collect();
    let core: Vec<&str> = words.iter().copied().filter(|w| !NOISE_WORDS.contains(&w.to_lowercase().as_str())).collect();
    let mut out: Vec<String> = Vec::new();
    if !core.is_empty() && core != words {
        out.push(core.join(" "));
    }
    if core.len() > 3 {
        out.push(core[..3].join(" "));
    }
    let mut seen = HashSet::new();
    out.into_iter().filter(|x| !x.is_empty() && x.to_lowercase() != q.to_lowercase() && seen.insert(x.clone())).collect()
}

/// Words of an album name without "(FLAC 24-bit)"-type tags.
pub fn tokens(name: &str) -> HashSet<String> {
    let re = regex::Regex::new(r"[\(\[][^)\]]*(flac|mp3|web|cd|vinyl|24|16|bit|khz|lossless|320|v0)[^)\]]*[\)\]]").expect("regex");
    let n = re.replace_all(&name.to_lowercase(), " ").into_owned();
    let mut out = HashSet::new();
    let mut cur = String::new();
    for c in n.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '$' {
            cur.push(c);
        } else if !cur.is_empty() {
            if cur.len() > 1 && !matches!(cur.as_str(), "the" | "and" | "flac" | "mp3") {
                out.insert(cur.clone());
            }
            cur.clear();
        }
    }
    out
}

/// Same album, probably (names overlap, track counts close).
pub fn similar(a_name: &str, a_tracks: usize, b: &Row) -> bool {
    let (ta, tb) = (tokens(a_name), tokens(&b.name));
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    let j = ta.intersection(&tb).count() as f64 / ta.union(&tb).count() as f64;
    let (na, nb) = (a_tracks as i64, b.audio.len() as i64);
    j >= 0.5 && (na - nb).abs() <= 2.max((na as f64 * 0.25) as i64)
}

/// "03 - Song.flac" → "song" (to match the same track across copies).
pub fn stem(fname: &str) -> String {
    let base = remote_split(fname).1;
    let base = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(&base).to_lowercase();
    let base = base.trim_start_matches(|c: char| c.is_ascii_digit() || " ._-".contains(c));
    base.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Does a row look like "artist – album"? → (album words matched ≥ 70 %, artist named in it)
pub fn fits(row: &Row, artist: &str, album: &str) -> (bool, bool) {
    let want: HashSet<String> = {
        let t = tokens(album);
        let w: HashSet<String> = t.iter().filter(|x| !NOISE_WORDS.contains(&x.as_str())).cloned().collect();
        if w.is_empty() { t } else { w }
    };
    let art: HashSet<String> = tokens(artist).into_iter().filter(|x| !NOISE_WORDS.contains(&x.as_str())).collect();
    let mut have = tokens(&row.name);
    have.extend(tokens(&row.dir));
    let hit = want.intersection(&have).count() as f64 / want.len().max(1) as f64;
    (hit >= 0.7, !art.is_disjoint(&have))
}

pub fn job_key(user: &str, folder: &str) -> String {
    format!("{user}\u{0}{}", folder.replace('/', "\\"))
}

pub fn split_key(key: &str) -> (String, String) {
    let (u, f) = key.split_once('\u{0}').unwrap_or((key, ""));
    (u.to_string(), f.to_string())
}

/// An album being fetched, with backups to switch to if it doesn't start.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub name: String,
    pub user: String,
    pub dir: String,
    pub alts: Vec<Row>,
    pub tried: Vec<String>,
    pub started: f64,
    pub auto: bool,
    pub cover_url: String,
}

pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Unfinished downloads and what's been imported, kept across restarts.
#[derive(Default)]
pub struct Pending {
    /// (user, remote file) → (size, album)
    pub pending: BTreeMap<(String, String), (u64, String)>,
    pub imported: HashSet<(String, String)>,
    pub jobs: BTreeMap<String, Job>,
}

impl Pending {
    fn path() -> std::path::PathBuf {
        data_dir().join("downloads_pending.json")
    }

    pub fn exists() -> bool {
        Self::path().exists()
    }

    pub fn load() -> Self {
        let v: Value = load_json(&Self::path(), Value::Null);
        let mut p = Pending::default();
        let pend = if v.is_array() { v.clone() } else { v.get("pending").cloned().unwrap_or(Value::Null) };
        for e in pend.as_array().into_iter().flatten() {
            let (Some(u), Some(f)) = (e.get(0).and_then(Value::as_str), e.get(1).and_then(Value::as_str)) else { continue };
            let info = e.get(2).cloned().unwrap_or(Value::Null);
            let size = info.get("size").and_then(Value::as_u64).unwrap_or(0);
            let album = info.get("album").and_then(Value::as_str).unwrap_or("").to_string();
            p.pending.insert((u.into(), f.into()), (size, album));
        }
        for e in v.get("imported").and_then(Value::as_array).into_iter().flatten() {
            if let (Some(u), Some(f)) = (e.get(0).and_then(Value::as_str), e.get(1).and_then(Value::as_str)) {
                p.imported.insert((u.into(), f.into()));
            }
        }
        if let Some(Value::Object(jobs)) = v.get("jobs") {
            for (k, j) in jobs {
                if let Ok(mut job) = serde_json::from_value::<Job>(j.clone()) {
                    job.started = now(); // fresh clock after a restart: peers need a moment again
                    p.jobs.insert(k.clone(), job);
                }
            }
        }
        p
    }

    pub fn save(&self) {
        let imp: Vec<Value> = self.imported.iter().take(4000).map(|(u, f)| json!([u, f])).collect();
        let v = json!({
            "pending": self.pending.iter().map(|((u, f), (s, a))| json!([u, f, {"size": s, "album": a}])).collect::<Vec<_>>(),
            "imported": imp,
            "jobs": self.jobs,
        });
        if let Err(e) = save_json(&Self::path(), &v) {
            log::warn!("saving downloads: {e}");
        }
    }
}

/// Picks the files of a fresh copy that are still missing after switching users.
pub fn still_needed(new: &Row, done_stems: &HashSet<String>) -> Vec<RFile> {
    let all: Vec<RFile> = new.audio.iter().chain(new.images.iter()).cloned().collect();
    let want: Vec<RFile> = all.iter().filter(|f| !done_stems.contains(&stem(&f.filename))).cloned().collect();
    if !done_stems.is_empty() && want.len() == all.len() {
        let imgs: Vec<RFile> = want.iter().filter(|f| is_image_name(&f.filename)).cloned().collect();
        if !imgs.is_empty() {
            return imgs;
        }
    }
    want
}

/// Groups finished audio files per job key: (key → local paths).
pub fn finished_groups(done: &[(String, String, std::path::PathBuf)]) -> HashMap<String, Vec<std::path::PathBuf>> {
    let mut out: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for (u, f, p) in done {
        if crate::library::is_audio(p) {
            out.entry(job_key(u, &remote_split(f).0)).or_default().push(p.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries() {
        assert_eq!(clean_query("Burial - Untrue (Deluxe Edition)!"), "Burial Untrue Deluxe Edition");
        assert_eq!(broader_queries("Burial - Untrue (Deluxe Edition)"), vec!["Burial Untrue".to_string()]);
        assert_eq!(stem("Music\\A\\03 - Song Name.flac"), "songname");
        assert!(tokens("Untrue (FLAC 24bit)").contains("untrue"));
        assert!(!tokens("Untrue (FLAC 24bit)").contains("24bit"));
    }

    #[test]
    fn grouping() {
        let f = |n: &str, bd| RFile { filename: n.into(), size: 1000, bit_depth: bd, sample_rate: bd.map(|_| 44100), ..Default::default() };
        let r = Response {
            username: "u".into(),
            free_slot: true,
            speed: 1_000_000,
            queue: 0,
            files: vec![f("M\\A\\Al\\01.flac", Some(16)), f("M\\A\\Al\\02.flac", Some(16)), f("M\\A\\Al\\cover.jpg", None), f("M\\B\\x.mp3", None)],
        };
        let rows = group_results(&[r.clone()], true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fmt, "FLAC 16/44.1");
        assert_eq!(rows[0].images.len(), 1);
        assert_eq!(group_results(&[r], false).len(), 2);
    }
}
