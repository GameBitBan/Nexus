//! The music library: tracks read from your folders, grouped into albums and artists.
//!
//! `library.json` uses the same layout as the Python version, so the Rust version starts instantly with the
//! library the Python one already scanned (and vice versa).

use crate::util::{fold, load_json, md5_hex, save_json};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const AUDIO_EXT: &[&str] = &[
    "aac", "aif", "aiff", "alac", "ape", "dsf", "flac", "m4a", "mp3", "mp4", "oga", "ogg", "opus", "wav", "wv",
];
pub const IMG_EXT: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp"];
pub const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "art", "albumartsmall"];
pub const LOSSY_FMTS: &[&str] = &["MP3", "AAC", "OPUS", "OGG"];

pub fn is_audio(p: &Path) -> bool {
    ext_of(p).is_some_and(|e| AUDIO_EXT.contains(&e.as_str()))
}

pub fn is_image(p: &Path) -> bool {
    ext_of(p).is_some_and(|e| IMG_EXT.contains(&e.as_str()))
}

pub fn ext_of(p: &Path) -> Option<String> {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub albumartist: String,
    pub year: String,
    pub genre: String,
    pub track: u32,
    pub disc: u32,
    pub duration: f64,
    pub sr: u32,
    pub bits: u32,
    pub channels: u32,
    /// bits per second (as the Python version stores it)
    pub bitrate: u32,
    pub fmt: String,
    pub album_key: String,
    /// album key of a cached cover in `covers/`, or empty
    pub cover: String,
    pub mtime: f64,
    /// anything else the other version wrote — kept untouched
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Track {
    pub fn is_lossy(&self) -> bool {
        LOSSY_FMTS.contains(&self.fmt.as_str())
    }

    /// `FLAC 24/96`, `FLAC 16/44.1`, `MP3 320k`
    pub fn quality(&self) -> String {
        if self.bits > 0 && self.sr > 0 {
            let khz = f64::from(self.sr) / 1000.0;
            let khz = if khz.fract() == 0.0 { format!("{khz:.0}") } else { format!("{}", (khz * 10.0).round() / 10.0) };
            format!("{} {}/{}", self.fmt, self.bits, khz)
        } else if self.bitrate > 0 {
            format!("{} {}k", self.fmt, self.bitrate / 1000)
        } else {
            self.fmt.clone()
        }
    }

    /// Lower-cased text searched by the search box.
    pub fn search_blob(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {}",
            self.title, self.artist, self.album, self.albumartist, self.genre, self.year, self.fmt,
            if self.is_lossy() { "lossy" } else { "lossless" }
        )
        .to_lowercase()
    }

    pub fn sort_key(&self) -> (String, String, u32, u32, String) {
        (self.albumartist.to_lowercase(), self.album.to_lowercase(), self.disc, self.track, self.title.to_lowercase())
    }
}

pub fn album_key(albumartist: &str, album: &str) -> String {
    md5_hex(&format!("{albumartist}|{album}").to_lowercase())[..16].to_string()
}

#[derive(Clone, Debug)]
pub struct Album {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub year: String,
    pub cover: String,
    /// indices into `Library::tracks`, in disc/track order
    pub tracks: Vec<usize>,
    /// newest file time, for "recently added"
    pub added: f64,
}

#[derive(Default)]
pub struct Library {
    pub tracks: Vec<Arc<Track>>,
    pub albums: Vec<Album>,
    pub album_index: HashMap<String, usize>,
    pub by_path: HashMap<String, usize>,
    /// artist → album indices
    pub artists: Vec<(String, Vec<usize>)>,
    blobs: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumSort {
    Artist,
    Title,
    Year,
    Recent,
}

impl AlbumSort {
    pub fn from_config(s: &str) -> Self {
        match s {
            "title" => Self::Title,
            "year" => Self::Year,
            "recent" => Self::Recent,
            _ => Self::Artist,
        }
    }
}

impl Library {
    pub fn path() -> PathBuf {
        crate::paths::data_dir().join("library.json")
    }

    pub fn load() -> Self {
        let tracks: Vec<Track> = load_json(&Self::path(), Vec::new());
        Self::from_tracks(tracks)
    }

    pub fn save(&self) {
        let list: Vec<&Track> = self.tracks.iter().map(Arc::as_ref).collect();
        if let Err(e) = save_json(&Self::path(), &list) {
            log::error!("saving library: {e}");
        }
    }

    pub fn from_tracks(mut tracks: Vec<Track>) -> Self {
        tracks.sort_by_key(Track::sort_key);
        let tracks: Vec<Arc<Track>> = tracks.into_iter().map(Arc::new).collect();
        let mut lib = Library { tracks, ..Default::default() };
        lib.index();
        lib
    }

    fn index(&mut self) {
        self.by_path = self.tracks.iter().enumerate().map(|(i, t)| (t.path.clone(), i)).collect();
        self.blobs = self.tracks.iter().map(|t| t.search_blob()).collect();
        let mut albums: Vec<Album> = Vec::new();
        let mut idx: HashMap<String, usize> = HashMap::new();
        for (i, t) in self.tracks.iter().enumerate() {
            let ai = *idx.entry(t.album_key.clone()).or_insert_with(|| {
                albums.push(Album {
                    key: t.album_key.clone(),
                    title: t.album.clone(),
                    artist: t.albumartist.clone(),
                    year: t.year.clone(),
                    cover: t.cover.clone(),
                    tracks: Vec::new(),
                    added: 0.0,
                });
                albums.len() - 1
            });
            let a = &mut albums[ai];
            a.tracks.push(i);
            a.added = a.added.max(t.mtime);
            if a.year.is_empty() {
                a.year = t.year.clone();
            }
            if a.cover.is_empty() {
                a.cover = t.cover.clone();
            }
        }
        for a in &mut albums {
            let tr = &self.tracks;
            a.tracks.sort_by_key(|&i| (tr[i].disc, tr[i].track, tr[i].title.to_lowercase()));
        }
        let mut artists: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, a) in albums.iter().enumerate() {
            artists.entry(a.artist.clone()).or_default().push(i);
        }
        let mut artists: Vec<(String, Vec<usize>)> = artists.into_iter().collect();
        artists.sort_by_key(|(name, _)| name.to_lowercase());
        self.album_index = idx;
        self.albums = albums;
        self.artists = artists;
    }

    pub fn sorted_albums(&self, sort: AlbumSort) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.albums.len()).collect();
        let a = &self.albums;
        match sort {
            AlbumSort::Title => v.sort_by_key(|&i| a[i].title.to_lowercase()),
            AlbumSort::Year => v.sort_by_key(|&i| {
                let y = if a[i].year.is_empty() { "9999".to_string() } else { a[i].year.clone() };
                (y, a[i].artist.to_lowercase(), a[i].title.to_lowercase())
            }),
            AlbumSort::Recent => v.sort_by(|&x, &y| a[y].added.total_cmp(&a[x].added)),
            AlbumSort::Artist => {
                v.sort_by_key(|&i| (a[i].artist.to_lowercase(), a[i].year.clone(), a[i].title.to_lowercase()))
            }
        }
        v
    }

    pub fn album(&self, key: &str) -> Option<&Album> {
        self.album_index.get(key).map(|&i| &self.albums[i])
    }

    pub fn track(&self, path: &str) -> Option<&Arc<Track>> {
        self.by_path.get(path).map(|&i| &self.tracks[i])
    }

    /// Every word must appear somewhere in the track's text (`aphex 24` → Aphex Twin in 24-bit).
    pub fn search(&self, query: &str) -> Vec<usize> {
        let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            return (0..self.tracks.len()).collect();
        }
        self.blobs
            .iter()
            .enumerate()
            .filter(|(_, b)| words.iter().all(|w| b.contains(w.as_str())))
            .map(|(i, _)| i)
            .collect()
    }

    /// Finds a track for "artist – title" that came from somewhere else (imports, Discover, Jam).
    pub fn find_by_name(&self, artist: &str, title: &str) -> Option<usize> {
        let (fa, ft) = (fold(artist), fold(title));
        self.tracks.iter().position(|t| fold(&t.title) == ft && (fa.is_empty() || fold(&t.artist).contains(&fa)))
    }
}
