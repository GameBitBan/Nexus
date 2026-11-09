//! Bring playlists, liked songs and artists over from Spotify / Apple Music / anything that exports a
//! CSV or M3U. No logins: everything is read from the export files on this computer.

use crate::library::{Library, Track};
use crate::paths::data_dir;
use crate::playlists::Playlists;
use crate::util::{load_json, save_json};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, LazyLock};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Item {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ImportResult {
    pub source: String,
    pub playlists: Vec<(String, Vec<Item>)>,
    pub liked: Vec<Item>,
    pub artists: Vec<String>,
    /// saved albums (title empty)
    pub albums: Vec<Item>,
}

impl ImportResult {
    fn new(source: &str) -> Self {
        ImportResult { source: source.into(), ..Default::default() }
    }

    fn add_playlist(&mut self, name: &str, items: Vec<Option<Item>>) {
        let items: Vec<Item> = items.into_iter().flatten().collect();
        if !items.is_empty() {
            self.playlists.push((if name.is_empty() { "imported".into() } else { name.to_string() }, items));
        }
    }

    pub fn merge(&mut self, other: ImportResult) {
        self.playlists.extend(other.playlists);
        self.liked.extend(other.liked);
        self.artists.extend(other.artists);
        self.albums.extend(other.albums);
        if self.source == "files" && other.source != "files" {
            self.source = other.source;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.playlists.is_empty() && self.liked.is_empty() && self.artists.is_empty() && self.albums.is_empty()
    }
}

const TITLE: &[&str] = &["track name", "trackname", "track", "song title", "song name", "song", "title", "name", "track title"];
const ARTIST: &[&str] = &["artist name(s)", "artist names", "artist name", "artistname", "artists", "artist", "album artist", "creator"];
const ALBUM: &[&str] = &["album name", "album title", "albumname", "album"];
const PLAYLIST: &[&str] = &["playlist name", "playlist", "playlist title"];

fn val_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::Object(o) => o.get("name").map(val_str).unwrap_or_default(),
                other => val_str(other),
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

fn pick_json(d: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    let low: HashMap<String, &Value> = d.iter().map(|(k, v)| (k.trim().to_lowercase(), v)).collect();
    for k in keys {
        if let Some(v) = low.get(*k) {
            let s = val_str(v);
            if !s.is_empty() {
                return s;
            }
        }
    }
    String::new()
}

fn pick_row(d: &HashMap<String, String>, keys: &[&str]) -> String {
    for k in keys {
        if let Some(v) = d.get(*k) {
            let v = v.trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    String::new()
}

fn item_json(d: &serde_json::Map<String, Value>) -> Option<Item> {
    let title = pick_json(d, TITLE);
    (!title.is_empty()).then(|| Item { title, artist: pick_json(d, ARTIST), album: pick_json(d, ALBUM), path: None })
}

fn item_row(d: &HashMap<String, String>) -> Option<Item> {
    let title = pick_row(d, TITLE);
    (!title.is_empty()).then(|| Item { title, artist: pick_row(d, ARTIST), album: pick_row(d, ALBUM), path: None })
}

const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
    '\u{9d}', 'ž', 'Ÿ',
];

/// Text in whatever encoding an export tool picked (UTF-8 with or without BOM, UTF-16, Windows-1252).
pub fn read_text(data: &[u8]) -> String {
    let utf16 = |le: bool, d: &[u8]| -> String {
        let u: Vec<u16> = d.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&u)
    };
    if let Some(rest) = data.strip_prefix(b"\xef\xbb\xbf") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = data.strip_prefix(b"\xff\xfe") {
        return utf16(true, rest);
    }
    if let Some(rest) = data.strip_prefix(b"\xfe\xff") {
        return utf16(false, rest);
    }
    if let Ok(s) = std::str::from_utf8(data) {
        return s.to_string();
    }
    // BOM-less UTF-16: every other byte is zero for Latin text
    let zeros = data.iter().take(400).skip(1).step_by(2).filter(|b| **b == 0).count();
    if data.len() > 4 && zeros * 4 > data.len().min(400) {
        return utf16(true, data);
    }
    data.iter().map(|&b| if (0x80..0xa0).contains(&b) { CP1252[(b - 0x80) as usize] } else { b as char }).collect()
}

fn stem_of(name: &str) -> String {
    Path::new(&name.replace('\\', "/")).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// One export file → what's in it. Zips are opened (including zips inside zips).
pub fn parse_import(name: &str, data: &[u8]) -> Result<ImportResult, String> {
    parse_depth(name, data, 0)
}

fn parse_depth(name: &str, data: &[u8], depth: u32) -> Result<ImportResult, String> {
    let low = name.to_lowercase().replace('\\', "/");
    let base = low.rsplit('/').next().unwrap_or("").to_string();
    let stem = stem_of(name);
    let mut res = ImportResult::new("files");
    if low.ends_with(".zip") {
        if depth >= 3 {
            return Ok(res);
        }
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("not a zip I can read: {e}"))?;
        let mut total = 0u64;
        let mut names = Vec::new();
        for i in 0..z.len() {
            let Ok(f) = z.by_index(i) else { continue };
            total += f.size();
            if !f.is_dir() && f.size() < 300_000_000 {
                if let Ok(n) = f.name() {
                    names.push(n.to_string());
                }
            }
        }
        if total > 2_000_000_000 {
            return Err("that zip unpacks way too big".into());
        }
        let read = |z: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, n: &str| -> Option<Vec<u8>> {
            let mut f = z.by_name(n).ok()?;
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf).ok()?;
            Some(buf)
        };
        // Apple's privacy export: tracks and playlists are separate files that refer to each other
        let apple_tracks = names.iter().find(|n| n.to_lowercase().ends_with("apple music library tracks.json")).cloned();
        let apple_lists = names.iter().find(|n| n.to_lowercase().ends_with("apple music library playlists.json")).cloned();
        for n in &names {
            let nl = n.to_lowercase();
            if apple_tracks.is_some() && apple_lists.as_ref() == Some(n) {
                continue;
            }
            let ext = Path::new(&nl).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            if !matches!(ext.as_str(), "json" | "xml" | "csv" | "txt" | "m3u" | "m3u8" | "zip" | "tsv") {
                continue;
            }
            let Some(bytes) = read(&mut z, n) else { continue };
            let sub = if apple_tracks.as_ref() == Some(n) {
                let lists = apple_lists.as_ref().and_then(|l| read(&mut z, l));
                apple_privacy(&bytes, lists.as_deref())
            } else {
                match parse_depth(n, &bytes, depth + 1) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            };
            res.merge(sub);
        }
        return Ok(res);
    }
    let head = &data[..data.len().min(400)];
    let looks_plist = String::from_utf8_lossy(&head[..head.len().min(200)]).trim_start().starts_with("<?xml") && String::from_utf8_lossy(head).contains("plist");
    if low.ends_with(".xml") || looks_plist {
        return Ok(match plist::Value::from_reader_xml(std::io::Cursor::new(data)) {
            Ok(v) => itunes_xml(&v),
            Err(_) => res,
        });
    }
    if low.ends_with(".json") {
        let Ok(j) = serde_json::from_str::<Value>(&read_text(data)) else { return Ok(res) };
        if base == "yourlibrary.json" {
            if let Value::Object(o) = &j {
                res.source = "Spotify".into();
                let arr = |k: &str| o.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
                let s = |x: &Value, k: &str| x.get(k).map(val_str).unwrap_or_default();
                res.liked = arr("tracks")
                    .iter()
                    .filter(|x| !s(x, "track").is_empty())
                    .map(|x| Item { title: s(x, "track"), artist: s(x, "artist"), album: s(x, "album"), path: None })
                    .collect();
                res.artists = arr("artists").iter().map(|x| s(x, "name")).filter(|n| !n.is_empty()).collect();
                res.albums = arr("albums")
                    .iter()
                    .filter(|x| !s(x, "album").is_empty())
                    .map(|x| Item { title: String::new(), artist: s(x, "artist"), album: s(x, "album"), path: None })
                    .collect();
                return Ok(res);
            }
        }
        if let Some(pls) = j.get("playlists").and_then(Value::as_array) {
            res.source = "Spotify".into();
            for p in pls {
                let s = |x: &Value, k: &str| x.get(k).map(val_str).unwrap_or_default();
                let items: Vec<Option<Item>> = p
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|it| it.get("track").filter(|t| t.is_object()))
                            .map(|tr| Some(Item { title: s(tr, "trackName"), artist: s(tr, "artistName"), album: s(tr, "albumName"), path: None }).filter(|i| !i.title.is_empty()))
                            .collect()
                    })
                    .unwrap_or_default();
                res.add_playlist(&s(p, "name"), items);
            }
            return Ok(res);
        }
        let rows: Vec<Value> = match &j {
            Value::Array(a) => a.clone(),
            Value::Object(o) => o.values().find_map(|v| v.as_array().cloned()).unwrap_or_default(),
            _ => vec![],
        };
        res.add_playlist(&stem, rows.iter().filter_map(Value::as_object).map(item_json).collect());
        return Ok(res);
    }
    if low.ends_with(".m3u") || low.ends_with(".m3u8") {
        let mut items = Vec::new();
        let mut pending: Option<Item> = None;
        for ln in read_text(data).lines() {
            let ln = ln.trim();
            if ln.to_uppercase().starts_with("#EXTINF") {
                let info = ln.split_once(',').map(|(_, r)| r).unwrap_or("");
                let (a, t) = info.split_once(" - ").unwrap_or((info, ""));
                pending = Some(Item {
                    title: if t.trim().is_empty() { a.trim().to_string() } else { t.trim().to_string() },
                    artist: if t.is_empty() { String::new() } else { a.trim().to_string() },
                    ..Default::default()
                });
            } else if !ln.is_empty() && !ln.starts_with('#') {
                let mut item = pending.take().unwrap_or_else(|| Item { title: stem_of(ln), ..Default::default() });
                item.path = Some(ln.to_string());
                items.push(Some(item));
            }
        }
        res.add_playlist(&stem, items);
        return Ok(res);
    }
    if low.ends_with(".csv") || low.ends_with(".txt") || low.ends_with(".tsv") {
        let txt = read_text(data);
        let first = txt.lines().next().unwrap_or("").to_string();
        let delim = if first.matches('\t').count() > first.matches(',').count() {
            b'\t'
        } else if first.contains(',') {
            b','
        } else {
            b';'
        };
        let mut rdr = csv::ReaderBuilder::new().delimiter(delim).flexible(true).from_reader(txt.as_bytes());
        let headers: Vec<String> = match rdr.headers() {
            Ok(h) => h.iter().map(|x| x.trim().to_lowercase()).collect(),
            Err(_) => return Ok(res),
        };
        let rows: Vec<HashMap<String, String>> = rdr
            .records()
            .flatten()
            .map(|r| headers.iter().cloned().zip(r.iter().map(str::to_string)).collect())
            .collect();
        if rows.is_empty() || pick_row(&rows[0], TITLE).is_empty() {
            return Ok(res);
        }
        let fl = first.to_lowercase();
        if fl.contains("spotify") || fl.contains("track uri") {
            res.source = "Spotify".into();
        }
        let mut groups: Vec<(String, Vec<Item>)> = Vec::new();
        for r in &rows {
            if let Some(item) = item_row(r) {
                let g = Some(pick_row(r, PLAYLIST)).filter(|s| !s.is_empty()).unwrap_or_else(|| stem.clone());
                match groups.iter_mut().find(|(n, _)| *n == g) {
                    Some((_, v)) => v.push(item),
                    None => groups.push((g, vec![item])),
                }
            }
        }
        const LIKED: [&str; 9] = ["liked songs", "liked", "music library songs", "loved", "favorites", "favourites", "your music", "saved tracks", "liked songs"];
        for (g, items) in groups {
            if LIKED.contains(&g.to_lowercase().replace('_', " ").as_str()) {
                res.liked.extend(items);
            } else {
                res.add_playlist(&g, items.into_iter().map(Some).collect());
            }
        }
        return Ok(res);
    }
    Ok(res)
}

/// iTunes / Apple Music "Export Library…" XML.
fn itunes_xml(pl: &plist::Value) -> ImportResult {
    let mut res = ImportResult::new("Apple Music");
    let Some(d) = pl.as_dictionary() else { return res };
    let empty = plist::Dictionary::new();
    let tracks = d.get("Tracks").and_then(|t| t.as_dictionary()).unwrap_or(&empty);
    let s = |t: &plist::Dictionary, k: &str| t.get(k).and_then(|v| v.as_string()).unwrap_or("").to_string();
    let b = |t: &plist::Dictionary, k: &str| t.get(k).and_then(|v| v.as_boolean()).unwrap_or(false);
    let item = |id: &str| -> Option<Item> {
        let t = tracks.get(id)?.as_dictionary()?;
        let title = s(t, "Name");
        if title.is_empty() {
            return None;
        }
        let artist = Some(s(t, "Artist")).filter(|a| !a.is_empty()).unwrap_or_else(|| s(t, "Album Artist"));
        Some(Item { title, artist, album: s(t, "Album"), path: None })
    };
    res.liked = tracks
        .iter()
        .filter(|(_, t)| t.as_dictionary().is_some_and(|t| b(t, "Loved") || b(t, "Favorited")))
        .filter_map(|(k, _)| item(k))
        .collect();
    if let Some(lists) = d.get("Playlists").and_then(|p| p.as_array()) {
        for p in lists.iter().filter_map(|p| p.as_dictionary()) {
            if b(p, "Master") || p.contains_key("Distinguished Kind") || b(p, "Folder") || s(p, "Name") == "Library" {
                continue;
            }
            let items: Vec<Option<Item>> = p
                .get("Playlist Items")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_dictionary()?.get("Track ID").and_then(|v| v.as_signed_integer().or_else(|| v.as_unsigned_integer().map(|u| u as i64))))
                        .map(|id| item(&id.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            res.add_playlist(&s(p, "Name"), items);
        }
    }
    if res.playlists.is_empty() && res.liked.is_empty() {
        let mut a: Vec<String> = tracks.values().filter_map(|t| t.as_dictionary()).map(|t| s(t, "Artist")).filter(|a| !a.is_empty()).collect::<HashSet<_>>().into_iter().collect();
        a.sort();
        res.artists = a;
    }
    res
}

/// Apple's privacy-portal export ("Apple Music Library Tracks.json" + "…Playlists.json").
fn apple_privacy(tracks: &[u8], lists: Option<&[u8]>) -> ImportResult {
    let mut res = ImportResult::new("Apple Music");
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(&read_text(tracks)) else { return res };
    let mut by_id: HashMap<String, Item> = HashMap::new();
    for r in rows.iter().filter_map(Value::as_object) {
        let Some(it) = item_json(r) else { continue };
        for k in ["Track Identifier", "Apple Music Track Identifier", "Item Identifier"] {
            if let Some(v) = r.get(k).filter(|v| !v.is_null()) {
                by_id.insert(val_str(v), it.clone());
            }
        }
        let fav = r.get("Favorite Status").or_else(|| r.get("Is Favorite")).map(val_str).unwrap_or_default().to_lowercase();
        if matches!(fav.as_str(), "true" | "favorited" | "loved" | "1") {
            res.liked.push(it);
        }
    }
    if let Some(Ok(Value::Array(pls))) = lists.map(|l| serde_json::from_str::<Value>(&read_text(l))) {
        for p in pls.iter().filter_map(Value::as_object) {
            let ids = p.get("Playlist Item Identifiers").or_else(|| p.get("Track Identifiers")).and_then(Value::as_array).cloned().unwrap_or_default();
            let name = p.get("Title").or_else(|| p.get("Name")).map(val_str).unwrap_or_default();
            res.add_playlist(&name, ids.iter().map(|i| by_id.get(&val_str(i)).cloned()).collect());
        }
    }
    if res.playlists.is_empty() && res.liked.is_empty() {
        let mut a: Vec<String> = by_id.values().map(|i| i.artist.clone()).filter(|a| !a.is_empty()).collect::<HashSet<_>>().into_iter().collect();
        a.sort();
        res.artists = a;
    }
    res
}

// ------------------------------------------------------------------ matching

static RE_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\(\[][^\)\]]*(feat|ft\.|with |remaster|version|edit|mix|live|mono|stereo|deluxe|bonus)[^\)\]]*[\)\]]").expect("regex"));
static RE_DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s-\s.*(remaster|version|edit|mix|live|mono|stereo).*$").expect("regex"));
static RE_FEAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(feat|ft)\.?\s.*$").expect("regex"));
static RE_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",|;| & | x | feat\.? | ft\.? ").expect("regex"));

/// "Song (feat. X) - 2011 Remaster" → "song"; accents and punctuation dropped.
pub fn norm(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let s: String = s.nfkd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase();
    let s = RE_PAREN.replace_all(&s, " ");
    let s = RE_DASH.replace_all(&s, " ");
    let s = RE_FEAT.replace_all(&s, " ");
    let s = s.replace('&', " and ");
    let s: String = s.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn artist_tokens(a: &str) -> HashSet<String> {
    let low = a.to_lowercase();
    let first = RE_SPLIT.split(&low).next().unwrap_or("");
    norm(first).split_whitespace().filter(|w| *w != "the" && *w != "and").map(str::to_string).collect()
}

fn path_key(p: &str) -> String {
    let n = crate::paths::normalize(Path::new(p)).to_string_lossy().to_string();
    if cfg!(windows) { n.to_lowercase() } else { n }
}

/// Finds an imported song in the library: title + artist, forgiving about remasters, feat., accents.
pub struct Matcher {
    by_title: HashMap<String, Vec<Arc<Track>>>,
    by_path: HashMap<String, Arc<Track>>,
}

impl Matcher {
    pub fn new(tracks: &[Arc<Track>]) -> Self {
        let mut by_title: HashMap<String, Vec<Arc<Track>>> = HashMap::new();
        let mut by_path = HashMap::new();
        for t in tracks {
            by_title.entry(norm(&t.title)).or_default().push(t.clone());
            by_path.insert(path_key(&t.path), t.clone());
        }
        Matcher { by_title, by_path }
    }

    pub fn find(&self, item: &Item) -> Option<Arc<Track>> {
        if let Some(p) = &item.path {
            if let Some(t) = self.by_path.get(&path_key(p)) {
                return Some(t.clone());
            }
        }
        let cands = self.by_title.get(&norm(&item.title))?;
        let want = artist_tokens(&item.artist);
        let alb = norm(&item.album);
        let mut best: Option<(&Arc<Track>, i32)> = None;
        for t in cands {
            let mut have = artist_tokens(&t.artist);
            have.extend(artist_tokens(&t.albumartist));
            if !want.is_empty() && want.is_disjoint(&have) {
                continue;
            }
            let sc = if !alb.is_empty() && norm(&t.album) == alb { 2 } else { 0 } + if t.is_lossy() { 0 } else { 1 };
            if best.is_none_or(|(_, b)| sc > b) {
                best = Some((t, sc));
            }
        }
        best.map(|(t, _)| t.clone())
    }
}

/// An album you don't have yet (from imported songs or saved albums).
#[derive(Clone, Debug)]
pub struct Missing {
    pub artist: String,
    pub album: String,
    pub title: String,
    pub songs: usize,
}

pub fn first_artist(a: &str) -> String {
    a.split([',', ';']).next().unwrap_or("").trim().to_string()
}

/// Lists to offer (liked songs first) with how many songs of each are already here, and the missing albums.
pub fn summarize(res: &ImportResult, m: &Matcher, lib: &Library) -> (Vec<(String, Vec<Item>, usize)>, Vec<Missing>) {
    let src = if res.source == "files" { "your files".to_string() } else { res.source.clone() };
    let mut lists: Vec<(String, Vec<Item>)> = res.playlists.clone();
    if !res.liked.is_empty() {
        lists.insert(0, (if src == "your files" { "Liked Songs".into() } else { format!("Liked Songs ({src})") }, res.liked.clone()));
    }
    let mut rows = Vec::new();
    let mut miss: Vec<((String, String), Missing)> = Vec::new();
    let mut add_miss = |artist: String, album: String, title: String, n: usize| {
        let key = (norm(&artist), if norm(&album).is_empty() { norm(&title) } else { norm(&album) });
        match miss.iter_mut().find(|(k, _)| *k == key) {
            Some((_, m)) => m.songs += n,
            None => miss.push((key, Missing { artist, album, title, songs: n })),
        }
    };
    for (name, items) in lists {
        let mut hit = 0;
        for i in &items {
            if m.find(i).is_some() {
                hit += 1;
            } else {
                add_miss(first_artist(&i.artist), i.album.clone(), i.title.clone(), 1);
            }
        }
        rows.push((name, items, hit));
    }
    for a in &res.albums {
        if !a.album.is_empty() {
            add_miss(a.artist.clone(), a.album.clone(), String::new(), 0);
        }
    }
    let have: HashSet<(String, String)> =
        lib.tracks.iter().map(|t| (norm(if t.albumartist.is_empty() { &t.artist } else { &t.albumartist }), norm(&t.album))).collect();
    let mut missing: Vec<Missing> = miss.into_iter().map(|(_, m)| m).filter(|m| !m.album.is_empty() && !have.contains(&(norm(&m.artist), norm(&m.album)))).collect();
    missing.sort_by(|a, b| b.songs.cmp(&a.songs));
    (rows, missing)
}

// ------------------------------------------------------------------ imports.json

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Imported {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub items: Vec<Item>,
}

pub type Imports = indexmap::IndexMap<String, Imported>;

pub fn load() -> Imports {
    load_json(&data_dir().join("imports.json"), Imports::new())
}

pub fn save(imp: &Imports) {
    if let Err(e) = save_json(&data_dir().join("imports.json"), imp) {
        log::error!("saving imports: {e}");
    }
}

/// Makes the chosen playlists (adding to ones made by an earlier import). Returns how many.
pub fn apply(chosen: &[(String, Vec<Item>)], source: &str, m: &Matcher, pls: &mut Playlists) -> usize {
    let mut imports = load();
    let mut made = 0;
    for (name, items) in chosen {
        let mut fin = name.clone();
        if pls.contains(&fin) && !imports.contains_key(&fin) {
            fin = format!("{name} (imported)");
        }
        imports.insert(fin.clone(), Imported { source: source.to_string(), items: items.clone() });
        let mut paths: Vec<String> = pls.get(&fin).cloned().unwrap_or_default();
        let mut have: HashSet<String> = paths.iter().cloned().collect();
        for i in items {
            if let Some(t) = m.find(i) {
                if have.insert(t.path.clone()) {
                    paths.push(t.path.clone());
                }
            }
        }
        if let Some(v) = pls.get_mut(&fin) {
            *v = paths;
        } else {
            pls.add(&fin, paths);
        }
        made += 1;
    }
    save(&imports);
    made
}

/// After a rescan: put newly arrived songs into the playlists they were imported for.
pub fn rematch(tracks: &[Arc<Track>], pls: &mut Playlists) -> usize {
    let imports = load();
    if imports.is_empty() {
        return 0;
    }
    let m = Matcher::new(tracks);
    let mut added = 0;
    for (name, info) in &imports {
        let Some(paths) = pls.get_mut(name) else { continue };
        let mut have: HashSet<String> = paths.iter().cloned().collect();
        for i in &info.items {
            if let Some(t) = m.find(i) {
                if have.insert(t.path.clone()) {
                    paths.push(t.path.clone());
                    added += 1;
                }
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalising() {
        assert_eq!(norm("Song (feat. Someone) - 2011 Remaster"), "song");
        assert_eq!(norm("Beyoncé & Jay"), "beyonce and jay");
        assert_eq!(norm("Halo (Live)"), "halo");
        assert_eq!(artist_tokens("The Weeknd, Daft Punk"), ["weeknd".to_string()].into());
    }

    #[test]
    fn csv_and_m3u() {
        let csv = b"Track Name,Artist Name(s),Album Name,Playlist\nHalo,Beyonce,I Am,Gym\nXtal,Aphex Twin,SAW,Liked Songs\n";
        let r = parse_import("export.csv", csv).unwrap();
        assert_eq!(r.playlists.len(), 1);
        assert_eq!(r.playlists[0].0, "Gym");
        assert_eq!(r.liked.len(), 1);
        let m3u = b"#EXTM3U\n#EXTINF:123,Aphex Twin - Xtal\n/music/a.flac\n/music/b.flac\n";
        let r = parse_import("mix.m3u8", m3u).unwrap();
        assert_eq!(r.playlists[0].1[0].title, "Xtal");
        assert_eq!(r.playlists[0].1[1].title, "b");
        assert_eq!(read_text(b"caf\xe9 \x93x\x94"), "café “x”");
    }
}
