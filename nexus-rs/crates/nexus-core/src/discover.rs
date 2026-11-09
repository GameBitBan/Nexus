//! Recommendations: looks at who's in your library (and what you actually play), asks Deezer's free
//! public API who's similar, and lists artists you don't have yet and albums you're missing from artists
//! you do. Only artist names are sent. Results are cached in `discover.json` (same file as Python Nexus).

use crate::library::Library;
use crate::net;
use crate::paths::data_dir;
use crate::playlists::Stats;
use crate::util::{load_json, name_norm, save_json, title_norm};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const SKIP: [&str; 5] = ["unknown artist", "various artists", "various", "va", ""];

/// `NEXUS_DEEZER` points at another server (tests).
pub fn deezer_base() -> String {
    std::env::var("NEXUS_DEEZER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "https://api.deezer.com".into()).trim_end_matches('/').to_string()
}

/// GET on the Deezer API, waiting out its quota (50 requests / 5 s).
pub fn deezer_get(path: &str) -> Result<Value, String> {
    let url = if path.starts_with("http") { path.to_string() } else { format!("{}{path}", deezer_base()) };
    for i in 0..3 {
        let d = net::get_json_with(&url, &[], 10)?;
        match d.get("error") {
            None => return Ok(d),
            Some(e) => {
                if e.get("code").and_then(Value::as_i64) == Some(4) && i < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(1500 * (i + 1)));
                    continue;
                }
                return Err(e.get("message").and_then(Value::as_str).unwrap_or("deezer error").to_string());
            }
        }
    }
    Ok(Value::Null)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Seed {
    pub name: String,
    pub picture: String,
    pub albums: u32,
    pub tracks: u32,
    pub plays: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TopTrack {
    pub title: String,
    pub preview: String,
    pub tid: Option<i64>,
    pub album: String,
    pub cover: String,
    pub cover_big: String,
    pub duration: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rec {
    pub id: i64,
    pub name: String,
    pub picture: String,
    pub fans: u64,
    pub because: Vec<String>,
    pub top: Vec<TopTrack>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MissingAlbum {
    pub artist: String,
    pub title: String,
    pub year: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub cover: String,
    pub cover_big: String,
    pub have: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoverData {
    pub time: f64,
    pub seeds: Vec<Seed>,
    pub recs: Vec<Rec>,
    pub missing: Vec<MissingAlbum>,
}

pub fn load() -> Option<DiscoverData> {
    let d: Option<DiscoverData> = load_json(&data_dir().join("discover.json"), None);
    d.filter(|d| d.time > 0.0)
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub fn age_secs(d: &DiscoverData) -> f64 {
    (now() - d.time).max(0.0)
}

/// Your artists, best first: plays count most, then how much you own; followed (imported) artists get a boost.
pub fn library_artists(lib: &Library, stats: &Stats, fav: &[String]) -> Vec<(String, f64, u32, u32, u64)> {
    let mut by: HashMap<String, (HashSet<String>, u32, u64)> = HashMap::new();
    let mut order = Vec::new();
    for t in &lib.tracks {
        let a = if t.albumartist.is_empty() { t.artist.clone() } else { t.albumartist.clone() };
        if SKIP.contains(&name_norm(&a).as_str()) {
            continue;
        }
        let e = by.entry(a.clone()).or_insert_with(|| {
            order.push(a.clone());
            Default::default()
        });
        e.0.insert(t.album_key.clone());
        e.1 += 1;
        e.2 += stats.plays(&t.path);
    }
    let favs: HashMap<String, String> = fav.iter().map(|a| (name_norm(a), a.clone())).collect();
    let mut out: Vec<(String, f64, u32, u32, u64)> = order
        .iter()
        .map(|a| {
            let (albums, tracks, plays) = &by[a];
            let w = *plays as f64 * 2.0 + albums.len() as f64 * 3.0 + f64::from(*tracks) * 0.3 + if favs.contains_key(&name_norm(a)) { 8.0 } else { 0.0 };
            (a.clone(), w, albums.len() as u32, *tracks, *plays)
        })
        .collect();
    let have: HashSet<String> = order.iter().map(|a| name_norm(a)).collect();
    for (k, name) in &favs {
        if !have.contains(k) && !SKIP.contains(&k.as_str()) {
            out.push((name.clone(), 7.0, 0, 0, 0));
        }
    }
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn find_artist(name: &str) -> Option<Value> {
    let d = deezer_get(&format!("/search/artist?{}", net::query(&[("q", name), ("limit", "6")]))).ok()?;
    let want = name_norm(name);
    let res = d.get("data")?.as_array()?.clone();
    if let Some(a) = res.iter().find(|a| name_norm(&s(a, "name")) == want) {
        return Some(a.clone());
    }
    res.into_iter().find(|a| {
        let n = name_norm(&s(a, "name"));
        want.chars().count() >= 4 && (n.contains(&want) || want.contains(&n))
    })
}

/// The whole refresh (runs on a worker thread; `progress` gets short status lines).
pub fn run(lib: &Library, stats: &Stats, fav: &[String], progress: &dyn Fn(String)) -> Result<DiscoverData, String> {
    const SEEDS: usize = 15;
    let mine = library_artists(lib, stats, fav);
    let mut have: HashSet<String> = mine.iter().map(|m| name_norm(&m.0)).collect();
    for t in &lib.tracks {
        have.insert(name_norm(&t.artist));
    }
    let mut lib_albums: HashMap<String, HashSet<String>> = HashMap::new();
    for t in &lib.tracks {
        lib_albums.entry(name_norm(&t.albumartist)).or_default().insert(title_norm(&t.album));
    }
    struct S {
        name: String,
        id: i64,
        picture: String,
        albums: u32,
        tracks: u32,
        plays: u64,
        weight: f64,
    }
    let mut seeds = Vec::new();
    for (name, w, albums, tracks, plays) in mine.into_iter().take(SEEDS) {
        progress(format!("looking up {name}…"));
        if let Some(a) = find_artist(&name) {
            seeds.push(S { id: a.get("id").and_then(Value::as_i64).unwrap_or(0), picture: s(&a, "picture_medium"), name, albums, tracks, plays, weight: w });
        }
    }
    if seeds.is_empty() {
        return Err("couldn't find any of your artists on Deezer (or no internet)".into());
    }
    let top_w = seeds.iter().map(|s| s.weight).fold(0.0f64, f64::max).max(1.0);
    let mut scores: HashMap<i64, f64> = HashMap::new();
    let mut because: HashMap<i64, Vec<String>> = HashMap::new();
    let mut info: HashMap<i64, Value> = HashMap::new();
    for sd in &seeds {
        progress(format!("who sounds like {}…", sd.name));
        let rel = deezer_get(&format!("/artist/{}/related?limit=25", sd.id)).ok().and_then(|d| d.get("data").and_then(Value::as_array).cloned()).unwrap_or_default();
        let sw = 0.5 + sd.weight / top_w;
        for (rank, r) in rel.iter().enumerate() {
            if have.contains(&name_norm(&s(r, "name"))) {
                continue;
            }
            let Some(rid) = r.get("id").and_then(Value::as_i64) else { continue };
            *scores.entry(rid).or_default() += sw / (1.0 + rank as f64 * 0.12);
            let b = because.entry(rid).or_default();
            if !b.contains(&sd.name) {
                b.push(sd.name.clone());
            }
            info.insert(rid, r.clone());
        }
    }
    let fans = |id: &i64| info[id].get("nb_fan").and_then(Value::as_f64).unwrap_or(0.0);
    let mut ranked: Vec<i64> = scores.keys().copied().collect();
    ranked.sort_by(|a, b| {
        let ka = scores[a] * (1.0 + 0.15 * (1.0 + fans(a)).log10());
        let kb = scores[b] * (1.0 + 0.15 * (1.0 + fans(b)).log10());
        kb.partial_cmp(&ka).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut recs = Vec::new();
    for rid in ranked.into_iter().take(18) {
        let r = &info[&rid];
        let top = deezer_get(&format!("/artist/{rid}/top?limit=5")).ok().and_then(|d| d.get("data").and_then(Value::as_array).cloned()).unwrap_or_default();
        let pic = Some(s(r, "picture_big")).filter(|p| !p.is_empty()).unwrap_or_else(|| s(r, "picture_medium"));
        recs.push(Rec {
            id: rid,
            name: s(r, "name"),
            picture: pic,
            fans: fans(&rid) as u64,
            because: because[&rid].iter().take(3).cloned().collect(),
            top: top
                .iter()
                .map(|x| {
                    let al = x.get("album").cloned().unwrap_or(Value::Null);
                    TopTrack {
                        title: s(x, "title"),
                        preview: s(x, "preview"),
                        tid: x.get("id").and_then(Value::as_i64),
                        album: s(&al, "title"),
                        cover: s(&al, "cover_medium"),
                        cover_big: s(&al, "cover_xl"),
                        duration: x.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
                    }
                })
                .collect(),
        });
        progress(format!("getting {}'s top songs…", s(r, "name")));
    }
    let mut missing = Vec::new();
    for sd in seeds.iter().take(10) {
        let Ok(d) = deezer_get(&format!("/artist/{}/albums?limit=60", sd.id)) else { continue };
        let mut albs = d.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        albs.sort_by_key(|a| std::cmp::Reverse(s(a, "release_date")));
        let empty = HashSet::new();
        let mine = lib_albums.get(&name_norm(&sd.name)).unwrap_or(&empty);
        let mut seen = HashSet::new();
        let mut got = 0;
        for a in &albs {
            let kind = s(a, "record_type");
            if kind != "album" && kind != "ep" {
                continue;
            }
            let n = title_norm(&s(a, "title"));
            if n.is_empty() || seen.contains(&n) || mine.contains(&n) || mine.iter().any(|m| m.chars().count() > 3 && (n.contains(m.as_str()) || m.contains(n.as_str()))) {
                continue;
            }
            seen.insert(n);
            let big = [s(a, "cover_xl"), s(a, "cover_big")].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
            missing.push(MissingAlbum {
                artist: sd.name.clone(),
                title: s(a, "title"),
                year: s(a, "release_date").chars().take(4).collect(),
                kind,
                cover: s(a, "cover_medium"),
                cover_big: big,
                have: sd.albums,
            });
            got += 1;
            if got >= 3 {
                break;
            }
        }
    }
    missing.sort_by(|a, b| b.year.cmp(&a.year));
    missing.truncate(24);
    let out = DiscoverData {
        time: now(),
        seeds: seeds.iter().map(|s| Seed { name: s.name.clone(), picture: s.picture.clone(), albums: s.albums, tracks: s.tracks, plays: s.plays }).collect(),
        recs,
        missing,
    };
    if let Err(e) = save_json(&data_dir().join("discover.json"), &out) {
        log::warn!("discover cache: {e}");
    }
    Ok(out)
}

/// Preview links expire: ask for a current one.
pub fn fresh_preview(tid: i64) -> Option<String> {
    deezer_get(&format!("/track/{tid}")).ok().map(|d| s(&d, "preview")).filter(|p| !p.is_empty())
}

/// Deezer cover at 1000 px.
pub fn big_cover(url: &str) -> String {
    if url.contains("dzcdn") {
        static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"/\d+x\d+-").expect("regex"));
        RE.replace(url, "/1000x1000-").into_owned()
    } else {
        url.to_string()
    }
}
