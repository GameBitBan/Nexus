//! Reads tags and audio properties from files and walks music folders (incrementally: unchanged files are
//! taken from the previous scan).

use crate::library::{COVER_NAMES, IMG_EXT, Track, album_key, ext_of, is_audio};
use lofty::file::FileType;
use lofty::prelude::*;
use lofty::tag::ItemKey;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn mtime_of(p: &Path) -> Option<f64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64())
}

fn first_number(s: &str) -> u32 {
    s.split('/').next().unwrap_or("").trim().parse().unwrap_or(0)
}

fn format_name(ft: &FileType, tagged: &lofty::file::TaggedFile) -> String {
    match ft {
        FileType::Flac => "FLAC".into(),
        FileType::Mpeg => "MP3".into(),
        FileType::Opus => "OPUS".into(),
        FileType::Vorbis | FileType::Speex => "OGG".into(),
        FileType::Wav => "WAV".into(),
        FileType::Aiff => "AIFF".into(),
        FileType::Ape => "APE".into(),
        FileType::WavPack => "WV".into(),
        FileType::Aac => "AAC".into(),
        FileType::Mpc => "MPC".into(),
        FileType::Mp4 => {
            // ALAC and AAC share the .m4a container; lossless files have a real bit depth
            if tagged.properties().bit_depth().unwrap_or(0) > 0 { "ALAC".into() } else { "AAC".into() }
        }
        FileType::Custom(n) => n.to_ascii_uppercase(),
        _ => "AUDIO".into(),
    }
}

/// Reads one file. `None` when it isn't readable audio.
pub fn read_track(path: &Path) -> Option<Track> {
    if ext_of(path).as_deref() == Some("dsf") {
        return crate::dsf::read_track(path);
    }
    let tagged = lofty::read_from_path(path).ok()?;
    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let get = |k: &ItemKey| tag.and_then(|t| t.get_string(*k)).map(|s| s.trim().to_string()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let folder = path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let artist = Some(get(&ItemKey::TrackArtist)).filter(|s| !s.is_empty()).unwrap_or_else(|| "Unknown Artist".into());
    let album = Some(get(&ItemKey::AlbumTitle)).filter(|s| !s.is_empty()).unwrap_or(folder);
    let albumartist = Some(get(&ItemKey::AlbumArtist)).filter(|s| !s.is_empty()).unwrap_or_else(|| artist.clone());
    let date = Some(get(&ItemKey::RecordingDate)).filter(|s| !s.is_empty()).unwrap_or_else(|| get(&ItemKey::Year));
    let year: String = date.chars().take(4).collect();
    let bitrate = props.audio_bitrate().or(props.overall_bitrate()).unwrap_or(0) * 1000;
    let fmt = format_name(&tagged.file_type(), &tagged);
    Some(Track {
        path: path.to_string_lossy().to_string(),
        title: Some(get(&ItemKey::TrackTitle)).filter(|s| !s.is_empty()).unwrap_or(stem),
        album_key: album_key(&albumartist, &album),
        artist,
        album,
        albumartist,
        year,
        genre: get(&ItemKey::Genre),
        track: first_number(&get(&ItemKey::TrackNumber)),
        disc: first_number(&get(&ItemKey::DiscNumber)).max(1),
        duration: props.duration().as_secs_f64(),
        sr: props.sample_rate().unwrap_or(0),
        bits: u32::from(props.bit_depth().unwrap_or(0)),
        channels: u32::from(props.channels().unwrap_or(0)),
        bitrate,
        fmt,
        cover: String::new(),
        mtime: mtime_of(path).unwrap_or(0.0),
        extra: Default::default(),
    })
}

/// Embedded front cover, or a `cover.jpg` / `folder.png` / … next to the file.
pub fn cover_bytes(path: &Path) -> Option<Vec<u8>> {
    if let Ok(tagged) = lofty::read_from_path(path) {
        for tag in tagged.tags() {
            let pics = tag.pictures();
            let front = pics.iter().find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront);
            if let Some(p) = front.or_else(|| pics.first()) {
                return Some(p.data().to_vec());
            }
        }
    }
    folder_cover(path.parent()?)
}

pub fn folder_cover(dir: &Path) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut fallback = None;
    for e in entries.flatten() {
        let p = e.path();
        let (Some(ext), Some(stem)) = (ext_of(&p), p.file_stem().map(|s| s.to_string_lossy().to_lowercase())) else {
            continue;
        };
        if !IMG_EXT.contains(&ext.as_str()) {
            continue;
        }
        if COVER_NAMES.contains(&stem.as_str()) {
            return std::fs::read(&p).ok();
        }
        fallback.get_or_insert(p);
    }
    let _ = fallback; // only well-known names count, like the Python version
    None
}

/// Saves an album's cover as `covers/<key>.jpg` (max 800 px, like the Python version). Returns true on success.
pub fn save_cover(track_path: &Path, dest: &Path) -> bool {
    let Some(data) = cover_bytes(track_path) else { return false };
    save_cover_bytes(&data, dest, 800)
}

pub fn save_cover_bytes(data: &[u8], dest: &Path, max_side: u32) -> bool {
    let Ok(img) = image::load_from_memory(data) else { return false };
    let img = if img.width().max(img.height()) > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let Ok(file) = std::fs::File::create(dest) else { return false };
    let mut w = std::io::BufWriter::new(file);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 90).encode_image(&rgb).is_ok()
}

#[derive(Default)]
pub struct ScanProgress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

/// Lists audio files under `folders` (following links, skipping duplicates reached twice).
pub fn list_audio_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for folder in folders {
        for e in walkdir::WalkDir::new(folder).follow_links(true).into_iter().flatten() {
            if e.file_type().is_file() && is_audio(e.path()) {
                let real = e.path().canonicalize().unwrap_or_else(|_| e.path().to_path_buf());
                if seen.insert(real) {
                    files.push(e.path().to_path_buf());
                }
            }
        }
    }
    files
}

/// Scans `folders`. Files whose modification time didn't change are reused from `old` unless `full`.
pub fn scan(folders: &[PathBuf], old: &[Arc<Track>], covers: &Path, full: bool, progress: &ScanProgress) -> Vec<Track> {
    let files = list_audio_files(folders);
    progress.total.store(files.len(), Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let old: HashMap<&str, &Arc<Track>> =
        if full { HashMap::new() } else { old.iter().map(|t| (t.path.as_str(), t)).collect() };
    let mut out: Vec<Track> = files
        .par_iter()
        .filter_map(|p| {
            if progress.cancel.load(Ordering::Relaxed) {
                return None;
            }
            progress.done.fetch_add(1, Ordering::Relaxed);
            let mtime = mtime_of(p)?;
            let key = p.to_string_lossy();
            if let Some(t) = old.get(key.as_ref()) {
                if (t.mtime - mtime).abs() < 0.001 {
                    return Some(Track::clone(t));
                }
            }
            let mut t = read_track(p)?;
            t.mtime = mtime;
            Some(t)
        })
        .collect();
    // one cover per album, taken from its first file that has one
    let mut by_album: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, t) in out.iter().enumerate() {
        by_album.entry(t.album_key.clone()).or_default().push(i);
    }
    let have: HashSet<String> = by_album
        .par_iter()
        .filter_map(|(key, idx)| {
            let dest = covers.join(format!("{key}.jpg"));
            if dest.exists() && !full {
                return Some(key.clone());
            }
            idx.iter().any(|&i| save_cover(Path::new(&out[i].path), &dest)).then(|| key.clone())
        })
        .collect();
    for t in &mut out {
        t.cover = if have.contains(&t.album_key) { t.album_key.clone() } else { String::new() };
    }
    out.sort_by_key(Track::sort_key);
    out
}
