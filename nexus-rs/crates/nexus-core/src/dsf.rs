//! DSF (DSD stream files): lofty doesn't read them, so the header and the ID3 tag at the end are parsed here.

use crate::library::{Track, album_key};
use lofty::config::ParseOptions;
use lofty::file::FileType;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

struct Header {
    channels: u32,
    sample_rate: u32,
    bits: u32,
    samples: u64,
    metadata_at: u64,
}

fn header(f: &mut std::fs::File) -> Option<Header> {
    let mut b = [0u8; 28 + 52];
    f.read_exact(&mut b).ok()?;
    if &b[0..4] != b"DSD " || &b[28..32] != b"fmt " {
        return None;
    }
    let u32le = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64le = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    Some(Header {
        metadata_at: u64le(20),
        channels: u32le(28 + 24),
        sample_rate: u32le(28 + 28),
        bits: u32le(28 + 32),
        samples: u64le(28 + 36),
    })
}

fn id3_tag(f: &mut std::fs::File, at: u64) -> Option<lofty::tag::Tag> {
    if at == 0 {
        return None;
    }
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut buf = Vec::new();
    f.take(64 * 1024 * 1024).read_to_end(&mut buf).ok()?;
    let tagged = Probe::new(Cursor::new(buf))
        .set_file_type(FileType::Mpeg)
        .options(ParseOptions::new().read_properties(false))
        .read()
        .ok()?;
    tagged.primary_tag().cloned()
}

pub fn read_track(path: &Path) -> Option<Track> {
    let mut f = std::fs::File::open(path).ok()?;
    let h = header(&mut f)?;
    let tag = id3_tag(&mut f, h.metadata_at);
    let get = |k: ItemKey| tag.as_ref().and_then(|t| t.get_string(k)).map(|s| s.trim().to_string()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let folder = path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let artist = Some(get(ItemKey::TrackArtist)).filter(|s| !s.is_empty()).unwrap_or_else(|| "Unknown Artist".into());
    let album = Some(get(ItemKey::AlbumTitle)).filter(|s| !s.is_empty()).unwrap_or(folder);
    let albumartist = Some(get(ItemKey::AlbumArtist)).filter(|s| !s.is_empty()).unwrap_or_else(|| artist.clone());
    let date = Some(get(ItemKey::RecordingDate)).filter(|s| !s.is_empty()).unwrap_or_else(|| get(ItemKey::Year));
    let num = |s: String| s.split('/').next().unwrap_or("").trim().parse::<u32>().unwrap_or(0);
    Some(Track {
        path: path.to_string_lossy().to_string(),
        title: Some(get(ItemKey::TrackTitle)).filter(|s| !s.is_empty()).unwrap_or(stem),
        album_key: album_key(&albumartist, &album),
        artist,
        album,
        albumartist,
        year: date.chars().take(4).collect(),
        genre: get(ItemKey::Genre),
        track: num(get(ItemKey::TrackNumber)),
        disc: num(get(ItemKey::DiscNumber)).max(1),
        duration: if h.sample_rate > 0 { h.samples as f64 / f64::from(h.sample_rate) } else { 0.0 },
        sr: h.sample_rate,
        bits: h.bits,
        channels: h.channels,
        bitrate: h.sample_rate.saturating_mul(h.channels),
        fmt: "DSF".into(),
        ..Default::default()
    })
}

/// Embedded cover of a DSF file.
pub fn cover(path: &Path) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    let h = header(&mut f)?;
    let tag = id3_tag(&mut f, h.metadata_at)?;
    tag.pictures().first().map(|p| p.data().to_vec())
}
