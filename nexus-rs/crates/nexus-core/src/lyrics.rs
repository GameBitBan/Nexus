//! Lyrics: a `.lrc` file next to the song, or lyrics embedded in its tags (synced SYLT, LRC text, plain).

use lofty::file::TaggedFileExt;
use lofty::tag::ItemKey;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static LRC_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{1,2}(?:[.:]\d{1,3})?)\]").unwrap());
static LRC_OFFSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[offset:\s*([+-]?\d+)\s*\]").unwrap());
static WORD_STAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?>").unwrap());

#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    /// (milliseconds, line), sorted — when the lyrics have timestamps
    pub synced: Option<Vec<(u64, String)>>,
    pub plain: Option<String>,
    pub source: String,
}

/// Sorted `(ms, line)` pairs, or None if the text has no timestamps.
pub fn parse_lrc(text: &str) -> Option<Vec<(u64, String)>> {
    if text.is_empty() || !LRC_TIME.is_match(text) {
        return None;
    }
    let offset: i64 = LRC_OFFSET.captures(text).and_then(|c| c[1].parse().ok()).unwrap_or(0);
    let mut out = Vec::new();
    for raw in text.lines() {
        let stamps: Vec<_> = LRC_TIME.captures_iter(raw).collect();
        if stamps.is_empty() {
            continue;
        }
        let line = LRC_TIME.replace_all(raw, "");
        let line = WORD_STAMP.replace_all(line.trim(), "").trim().to_string();
        for m in stamps {
            let min: f64 = m[1].parse().unwrap_or(0.0);
            let sec: f64 = m[2].replace(':', ".").parse().unwrap_or(0.0);
            let ms = ((min * 60.0 + sec) * 1000.0) as i64 - offset;
            out.push((ms.max(0) as u64, line.clone()));
        }
    }
    out.sort_by_key(|x| x.0);
    if out.is_empty() { None } else { Some(out) }
}

fn sidecar(path: &Path) -> Option<Lyrics> {
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    for e in std::fs::read_dir(path.parent()?).ok()?.flatten() {
        let p = e.path();
        let is_lrc = p.extension().is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case("lrc"));
        if is_lrc && p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == stem) {
            let bytes = std::fs::read(&p).ok()?;
            let text = String::from_utf8_lossy(&bytes).to_string();
            let synced = parse_lrc(&text);
            let plain = if synced.is_some() { None } else { Some(text.trim().to_string()).filter(|s| !s.is_empty()) };
            return Some(Lyrics { synced, plain, source: ".lrc file".into() });
        }
    }
    None
}

/// ID3 SYLT (synced lyrics with millisecond stamps), MP3 only.
fn sylt(path: &Path) -> Option<Vec<(u64, String)>> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, SynchronizedTextFrame, TimestampFormat};
    let mut f = std::fs::File::open(path).ok()?;
    let mp3 = lofty::mpeg::MpegFile::read_from(&mut f, ParseOptions::new().read_properties(false)).ok()?;
    let tag = mp3.id3v2()?;
    for frame in tag {
        if let Frame::Binary(b) = frame {
            if frame.id_str() == "SYLT" {
                if let Ok(s) = SynchronizedTextFrame::parse(&b.data, frame.flags()) {
                    if s.timestamp_format == TimestampFormat::MS && !s.content.is_empty() {
                        let mut v: Vec<(u64, String)> = s.content.iter().map(|(t, l)| (u64::from(*t), l.trim().to_string())).collect();
                        v.sort_by_key(|x| x.0);
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

pub fn read_lyrics(path: &Path) -> Option<Lyrics> {
    if let Some(l) = sidecar(path) {
        return Some(l);
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "mp3" {
        if let Some(s) = sylt(path) {
            return Some(Lyrics { synced: Some(s), plain: None, source: "tag (SYLT)".into() });
        }
    }
    let tagged = lofty::read_from_path(path).ok()?;
    let mut texts: Vec<String> = Vec::new();
    for tag in tagged.tags() {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            texts.extend(tag.get_strings(key).map(str::to_string));
        }
    }
    // Vorbis comments under names lofty doesn't map (SYNCEDLYRICS, LYRIC, "UNSYNCED LYRICS")
    if let Some(vc) = vorbis_comments(path, &ext) {
        for k in ["SYNCEDLYRICS", "LYRIC", "UNSYNCED LYRICS"] {
            texts.extend(vc.get_all(k).map(str::to_string));
        }
    }
    if let Some(s) = texts.iter().find_map(|t| parse_lrc(t)) {
        return Some(Lyrics { synced: Some(s), plain: None, source: "tag".into() });
    }
    texts.into_iter().find(|t| !t.trim().is_empty()).map(|t| Lyrics { synced: None, plain: Some(t.trim().to_string()), source: "tag".into() })
}

fn vorbis_comments(path: &Path, ext: &str) -> Option<lofty::ogg::tag::VorbisComments> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    let mut f = std::fs::File::open(path).ok()?;
    let opts = ParseOptions::new().read_properties(false);
    match ext {
        "flac" => lofty::flac::FlacFile::read_from(&mut f, opts).ok()?.vorbis_comments().cloned(),
        "opus" => Some(lofty::ogg::OpusFile::read_from(&mut f, opts).ok()?.vorbis_comments().clone()),
        "ogg" | "oga" => Some(lofty::ogg::VorbisFile::read_from(&mut f, opts).ok()?.vorbis_comments().clone()),
        _ => None,
    }
}

/// Index of the line to light up at `ms` (a little early so it doesn't lag).
pub fn current_line(synced: &[(u64, String)], ms: u64) -> Option<usize> {
    let i = synced.partition_point(|(t, _)| *t <= ms + 150);
    i.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_parsing() {
        let t = "[ar:x]\n[offset:+500]\n[00:01.00][00:03.50]hello <00:01.20>world\n[00:02.000]second\nno stamp";
        let v = parse_lrc(t).unwrap();
        assert_eq!(v, vec![(500, "hello world".into()), (1500, "second".into()), (3000, "hello world".into())]);
        assert_eq!(current_line(&v, 0), None);
        assert_eq!(current_line(&v, 400), Some(0));
        assert_eq!(current_line(&v, 1400), Some(1));
        assert!(parse_lrc("just words").is_none());
    }
}
