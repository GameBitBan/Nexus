//! Reading and writing tags + cover art for the tag editor. Works on each format's own tag type
//! (Vorbis comments, MP4 atoms, APE, ID3v2) so fields Nexus doesn't know about are kept untouched.

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::id3::v2::{CommentFrame, Frame, FrameId, Id3v2Tag, TextInformationFrame, TimestampFrame, UnsynchronizedTextFrame};
use lofty::ogg::OggPictureStorage;
use lofty::ogg::tag::VorbisComments;
use lofty::picture::{MimeType, Picture, PictureInformation, PictureType};
use lofty::tag::{Accessor, ItemValue};
use lofty::TextEncoding;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const FIELDS: [(&str, &str); 11] = [
    ("title", "title"),
    ("artist", "artist"),
    ("album", "album"),
    ("albumartist", "album artist"),
    ("tracknumber", "track"),
    ("discnumber", "disc"),
    ("date", "year / date"),
    ("genre", "genre"),
    ("composer", "composer"),
    ("comment", "comment"),
    ("lyrics", "lyrics"),
];

fn vorbis_keys(field: &str) -> &'static [&'static str] {
    match field {
        "title" => &["TITLE"],
        "artist" => &["ARTIST"],
        "album" => &["ALBUM"],
        "albumartist" => &["ALBUMARTIST", "ALBUM ARTIST", "ALBUM_ARTIST"],
        "tracknumber" => &["TRACKNUMBER"],
        "discnumber" => &["DISCNUMBER"],
        "date" => &["DATE", "YEAR"],
        "genre" => &["GENRE"],
        "composer" => &["COMPOSER"],
        "comment" => &["COMMENT", "DESCRIPTION"],
        "lyrics" => &["LYRICS", "UNSYNCEDLYRICS"],
        _ => &[],
    }
}

fn id3_frame(field: &str) -> Option<&'static str> {
    Some(match field {
        "title" => "TIT2",
        "artist" => "TPE1",
        "album" => "TALB",
        "albumartist" => "TPE2",
        "tracknumber" => "TRCK",
        "discnumber" => "TPOS",
        "date" => "TDRC",
        "genre" => "TCON",
        "composer" => "TCOM",
        _ => return None,
    })
}

fn mp4_atom(field: &str) -> Option<[u8; 4]> {
    Some(match field {
        "title" => *b"\xa9nam",
        "artist" => *b"\xa9ART",
        "album" => *b"\xa9alb",
        "albumartist" => *b"aART",
        "date" => *b"\xa9day",
        "genre" => *b"\xa9gen",
        "composer" => *b"\xa9wrt",
        "comment" => *b"\xa9cmt",
        "lyrics" => *b"\xa9lyr",
        _ => return None,
    })
}

fn ape_keys(field: &str) -> &'static [&'static str] {
    match field {
        "title" => &["Title"],
        "artist" => &["Artist"],
        "album" => &["Album"],
        "albumartist" => &["Album Artist", "AlbumArtist"],
        "tracknumber" => &["Track"],
        "discnumber" => &["Disc"],
        "date" => &["Year", "Date"],
        "genre" => &["Genre"],
        "composer" => &["Composer"],
        "comment" => &["Comment"],
        "lyrics" => &["Lyrics"],
        _ => &[],
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tags {
    pub fields: BTreeMap<String, String>,
    pub embedded: Option<Vec<u8>>,
    pub folder: Option<Vec<u8>>,
    /// vorbis | mp4 | ape | id3
    pub family: String,
}

impl Tags {
    pub fn get(&self, f: &str) -> &str {
        self.fields.get(f).map(String::as_str).unwrap_or("")
    }
}

pub enum Cover {
    Keep,
    Remove,
    Set(Vec<u8>, &'static str),
}

enum Fam {
    Flac,
    Ogg(FileType),
    Mp4,
    Ape(FileType),
    Id3(FileType),
    Dsf,
}

fn family(path: &Path) -> Result<Fam, String> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dsf")) {
        return Ok(Fam::Dsf);
    }
    let ft = lofty::probe::Probe::open(path).map_err(|e| e.to_string())?.guess_file_type().map_err(|e| e.to_string())?.file_type().ok_or("not an audio file Nexus can tag")?;
    Ok(match ft {
        FileType::Flac => Fam::Flac,
        FileType::Vorbis | FileType::Opus | FileType::Speex => Fam::Ogg(ft),
        FileType::Mp4 => Fam::Mp4,
        FileType::Ape | FileType::WavPack | FileType::Mpc => Fam::Ape(ft),
        FileType::Mpeg | FileType::Aiff | FileType::Wav => Fam::Id3(ft),
        other => return Err(format!("can't edit {other:?} tags")),
    })
}

fn opts() -> ParseOptions {
    ParseOptions::new().read_properties(false)
}

fn open_rw(path: &Path) -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| e.to_string())
}

/// Image size and kind, if the bytes are a picture we can read.
pub fn image_info(data: &[u8]) -> Option<(u32, u32, &'static str)> {
    let img = image::load_from_memory(data).ok()?;
    let kind = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "PNG"
    } else if data.starts_with(b"\xff\xd8") {
        "JPEG"
    } else {
        "image"
    };
    Some((img.width(), img.height(), kind))
}

/// Keeps JPEG/PNG as they are; converts anything else (webp, bmp…) to JPEG; shrinks to `max_side` if set.
pub fn prepare_cover(data: &[u8], max_side: u32) -> Result<(Vec<u8>, &'static str), String> {
    let is_png = data.starts_with(b"\x89PNG\r\n\x1a\n");
    let is_jpg = data.starts_with(b"\xff\xd8");
    let img = image::load_from_memory(data).map_err(|_| "that isn't an image Nexus can read".to_string())?;
    let img = if max_side > 0 && img.width().max(img.height()) > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::CatmullRom)
    } else if is_png || is_jpg {
        return Ok((data.to_vec(), if is_png { "image/png" } else { "image/jpeg" }));
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 93))
        .map_err(|e| e.to_string())?;
    Ok((out.into_inner(), "image/jpeg"))
}

fn picture(data: Vec<u8>, mime: &str) -> Picture {
    Picture::unchecked(data)
        .pic_type(PictureType::CoverFront)
        .mime_type(if mime == "image/png" { MimeType::Png } else { MimeType::Jpeg })
        .build()
}

fn pic_info(p: &Picture) -> PictureInformation {
    PictureInformation::from_picture(p).unwrap_or_default()
}

fn front<'a>(mut it: impl Iterator<Item = &'a Picture> + Clone) -> Option<Vec<u8>> {
    it.clone().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| it.next()).map(|p| p.data().to_vec())
}

// ------------------------------------------------------------------ vorbis

fn read_vorbis(vc: &VorbisComments, out: &mut Tags) {
    for (f, _) in FIELDS {
        for k in vorbis_keys(f) {
            let vals: Vec<&str> = vc.get_all(k).collect();
            if !vals.is_empty() {
                out.fields.insert(f.into(), vals.join("; "));
                break;
            }
        }
    }
    for (f, keys) in [("tracknumber", ["TRACKTOTAL", "TOTALTRACKS"]), ("discnumber", ["DISCTOTAL", "TOTALDISCS"])] {
        let n = out.get(f).to_string();
        if !n.is_empty() && !n.contains('/') {
            if let Some(t) = keys.iter().find_map(|k| vc.get(k)) {
                out.fields.insert(f.into(), format!("{n}/{t}"));
            }
        }
    }
}

fn write_vorbis(vc: &mut VorbisComments, changes: &[(String, String)]) {
    for (field, val) in changes {
        for k in vorbis_keys(field) {
            let _ = vc.remove(k).count();
        }
        let mut val = val.clone();
        if field == "tracknumber" || field == "discnumber" {
            let tot = if field == "tracknumber" { ["TRACKTOTAL", "TOTALTRACKS"] } else { ["DISCTOTAL", "TOTALDISCS"] };
            for k in tot {
                let _ = vc.remove(k).count();
            }
            if let Some((n, t)) = val.clone().split_once('/') {
                val = n.trim().to_string();
                if !t.trim().is_empty() {
                    vc.insert(tot[0].to_string(), t.trim().to_string());
                }
            }
        }
        if val.is_empty() {
            continue;
        }
        let key = vorbis_keys(field)[0].to_string();
        if (field == "artist" || field == "genre") && val.contains(';') {
            for v in val.split(';').map(str::trim).filter(|v| !v.is_empty()) {
                vc.push(key.clone(), v.to_string());
            }
        } else {
            vc.insert(key, val);
        }
    }
}

// ------------------------------------------------------------------ id3

fn read_id3(t: &Id3v2Tag, out: &mut Tags) {
    for (f, _) in FIELDS {
        let Some(id) = id3_frame(f) else { continue };
        let fid = FrameId::new(id).expect("valid id");
        let v = match t.get(&fid) {
            Some(Frame::Text(tf)) => tf.value.replace('\0', "; "),
            Some(Frame::Timestamp(ts)) => ts.timestamp.to_string(),
            _ => continue,
        };
        out.fields.insert(f.into(), v);
    }
    if out.get("date").is_empty() {
        if let Some(y) = t.get_text(&FrameId::new("TYER").expect("valid id")) {
            out.fields.insert("date".into(), y.to_string());
        }
    }
    let comms: Vec<&CommentFrame> = t.comments().collect();
    if let Some(c) = comms.iter().find(|c| c.description.is_empty()).or(comms.first()) {
        out.fields.insert("comment".into(), c.content.to_string());
    }
    if let Some(u) = t.unsync_text().next() {
        out.fields.insert("lyrics".into(), u.content.to_string());
    }
    let pics: Vec<&Picture> = t.iter().filter_map(|f| if let Frame::Picture(p) = f { Some(p.picture.as_ref()) } else { None }).collect();
    out.embedded = front(pics.into_iter());
}

fn write_id3(t: &mut Id3v2Tag, changes: &[(String, String)], cover: &Cover) {
    for (field, val) in changes {
        if let Some(id) = id3_frame(field) {
            let fid = FrameId::new(id).expect("valid id");
            let _ = t.remove(&fid).count();
            if field == "date" {
                let _ = t.remove(&FrameId::new("TYER").expect("valid id")).count();
            }
            if val.is_empty() {
                continue;
            }
            if field == "date" {
                match val.parse::<lofty::tag::items::Timestamp>() {
                    Ok(ts) => {
                        t.insert(Frame::Timestamp(TimestampFrame::new(fid, TextEncoding::UTF8, ts)));
                    }
                    Err(_) => {
                        t.insert(Frame::Text(TextInformationFrame::new(FrameId::new("TYER").expect("valid id"), TextEncoding::UTF8, val.clone())));
                    }
                }
            } else {
                t.insert(Frame::Text(TextInformationFrame::new(fid, TextEncoding::UTF8, val.clone())));
            }
        } else if field == "comment" {
            t.retain(|f| !matches!(f, Frame::Comment(_)));
            if !val.is_empty() {
                t.insert(Frame::Comment(CommentFrame::new(TextEncoding::UTF8, *b"eng", Cow::Borrowed(""), Cow::Owned(val.clone()))));
            }
        } else if field == "lyrics" {
            t.retain(|f| !matches!(f, Frame::UnsynchronizedText(_)));
            if !val.is_empty() {
                t.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(TextEncoding::UTF8, *b"eng", Cow::Borrowed(""), Cow::Owned(val.clone()))));
            }
        }
    }
    match cover {
        Cover::Keep => {}
        Cover::Remove => t.retain(|f| !matches!(f, Frame::Picture(_))),
        Cover::Set(data, mime) => {
            t.retain(|f| !matches!(f, Frame::Picture(_)));
            let mut p = picture(data.clone(), mime);
            p.set_description(Some("Cover".into()));
            t.insert_picture(p);
        }
    }
}

// ------------------------------------------------------------------ dsf (ID3 at the end of the file)

fn dsf_tag(path: &Path) -> Result<(Option<Id3v2Tag>, u64, u64), String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = [0u8; 28];
    f.read_exact(&mut h).map_err(|e| e.to_string())?;
    if &h[..4] != b"DSD " {
        return Err("not a DSF file".into());
    }
    let size = u64::from_le_bytes(h[12..20].try_into().unwrap());
    let at = u64::from_le_bytes(h[20..28].try_into().unwrap());
    if at == 0 {
        return Ok((None, size, 0));
    }
    f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let tag = lofty::mpeg::MpegFile::read_from(&mut std::io::Cursor::new(buf), opts()).ok().and_then(|m| m.id3v2().cloned());
    Ok((tag, size, at))
}

fn dsf_write(path: &Path, tag: &Id3v2Tag, at: u64) -> Result<(), String> {
    use lofty::tag::TagExt;
    let mut bytes = Vec::new();
    tag.dump_to(&mut bytes, WriteOptions::default()).map_err(|e| e.to_string())?;
    let mut f = open_rw(path)?;
    let audio_end = if at > 0 { at } else { f.metadata().map_err(|e| e.to_string())?.len() };
    f.set_len(audio_end).map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(audio_end)).map_err(|e| e.to_string())?;
    f.write_all(&bytes).map_err(|e| e.to_string())?;
    let total = audio_end + bytes.len() as u64;
    f.seek(SeekFrom::Start(12)).map_err(|e| e.to_string())?;
    f.write_all(&total.to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&audio_end.to_le_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

// ------------------------------------------------------------------ public

pub fn folder_cover(path: &Path) -> Option<Vec<u8>> {
    crate::scanner::folder_cover(path.parent()?)
}

pub fn read_tags(path: &Path, folder: bool) -> Result<Tags, String> {
    let mut out = Tags::default();
    for (f, _) in FIELDS {
        out.fields.insert(f.into(), String::new());
    }
    let fam = family(path)?;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    match fam {
        Fam::Flac => {
            out.family = "vorbis".into();
            let f = lofty::flac::FlacFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
            if let Some(vc) = f.vorbis_comments() {
                read_vorbis(vc, &mut out);
            }
            out.embedded = front(f.pictures().iter().map(|(p, _)| p));
        }
        Fam::Ogg(ft) => {
            out.family = "vorbis".into();
            let vc = match ft {
                FileType::Opus => lofty::ogg::OpusFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.vorbis_comments().clone(),
                FileType::Speex => lofty::ogg::SpeexFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.vorbis_comments().clone(),
                _ => lofty::ogg::VorbisFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.vorbis_comments().clone(),
            };
            read_vorbis(&vc, &mut out);
            out.embedded = front(vc.pictures().iter().map(|(p, _)| p));
        }
        Fam::Mp4 => {
            out.family = "mp4".into();
            let f = lofty::mp4::Mp4File::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
            if let Some(il) = f.ilst() {
                for (fld, _) in FIELDS {
                    if let Some(code) = mp4_atom(fld) {
                        if let Some(atom) = il.get(&lofty::mp4::AtomIdent::Fourcc(code)) {
                            let vals: Vec<String> = atom
                                .data()
                                .filter_map(|d| match d {
                                    lofty::mp4::AtomData::UTF8(s) | lofty::mp4::AtomData::UTF16(s) => Some(s.clone()),
                                    _ => None,
                                })
                                .collect();
                            if !vals.is_empty() {
                                out.fields.insert(fld.into(), vals.join("; "));
                            }
                        }
                    }
                }
                let pair = |n: Option<u32>, t: Option<u32>| match (n.unwrap_or(0), t.unwrap_or(0)) {
                    (0, 0) => String::new(),
                    (n, 0) => n.to_string(),
                    (n, t) => format!("{n}/{t}"),
                };
                out.fields.insert("tracknumber".into(), pair(il.track(), il.track_total()));
                out.fields.insert("discnumber".into(), pair(il.disk(), il.disk_total()));
                out.embedded = il.pictures().and_then(|p| front(p.collect::<Vec<_>>().into_iter()));
            }
        }
        Fam::Ape(ft) => {
            out.family = "ape".into();
            let tag = match ft {
                FileType::WavPack => lofty::wavpack::WavPackFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.ape().cloned(),
                FileType::Mpc => lofty::musepack::MpcFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.ape().cloned(),
                _ => lofty::ape::ApeFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.ape().cloned(),
            };
            if let Some(t) = tag {
                for (fld, _) in FIELDS {
                    for k in ape_keys(fld) {
                        if let Some(ItemValue::Text(s)) = t.get(k).map(|i| i.value()) {
                            out.fields.insert(fld.into(), s.clone());
                            break;
                        }
                    }
                }
                if let Some(ItemValue::Binary(raw)) = t.get("Cover Art (Front)").map(|i| i.value()) {
                    let head = &raw[..raw.len().min(256)];
                    out.embedded = Some(match head.iter().position(|b| *b == 0) {
                        Some(i) => raw[i + 1..].to_vec(),
                        None => raw.clone(),
                    });
                }
            }
        }
        Fam::Id3(ft) => {
            out.family = "id3".into();
            let tag = match ft {
                FileType::Aiff => lofty::iff::aiff::AiffFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.id3v2().cloned(),
                FileType::Wav => lofty::iff::wav::WavFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.id3v2().cloned(),
                _ => lofty::mpeg::MpegFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?.id3v2().cloned(),
            };
            if let Some(t) = tag {
                read_id3(&t, &mut out);
            }
        }
        Fam::Dsf => {
            out.family = "id3".into();
            if let Some(t) = dsf_tag(path)?.0 {
                read_id3(&t, &mut out);
            }
        }
    }
    if folder {
        out.folder = folder_cover(path);
    }
    Ok(out)
}

/// `changes`: field → text ("" deletes it). Leaves every other field alone.
pub fn write_tags(path: &Path, changes: &[(String, String)], cover: &Cover) -> Result<(), String> {
    let fam = family(path)?;
    let wo = WriteOptions::default();
    let mut file = open_rw(path)?;
    match fam {
        Fam::Flac => {
            let mut f = lofty::flac::FlacFile::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
            if f.vorbis_comments().is_none() {
                f.set_vorbis_comments(VorbisComments::default());
            }
            write_vorbis(f.vorbis_comments_mut().expect("just set"), changes);
            match cover {
                Cover::Keep => {}
                Cover::Remove => {
                    f.remove_pictures();
                }
                Cover::Set(d, m) => {
                    f.remove_pictures();
                    let p = picture(d.clone(), m);
                    let info = pic_info(&p);
                    f.insert_picture(p, Some(info)).map_err(|e| e.to_string())?;
                }
            }
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            f.save_to(&mut file, wo).map_err(|e| e.to_string())?;
        }
        Fam::Ogg(ft) => {
            macro_rules! ogg {
                ($t:ty) => {{
                    let mut f = <$t>::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
                    let vc = f.vorbis_comments_mut();
                    write_vorbis(vc, changes);
                    match cover {
                        Cover::Keep => {}
                        Cover::Remove => {
                            vc.remove_pictures();
                        }
                        Cover::Set(d, m) => {
                            vc.remove_pictures();
                            let p = picture(d.clone(), m);
                            let info = pic_info(&p);
                            vc.insert_picture(p, Some(info)).map_err(|e| e.to_string())?;
                        }
                    }
                    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                    f.save_to(&mut file, wo).map_err(|e| e.to_string())?;
                }};
            }
            match ft {
                FileType::Opus => ogg!(lofty::ogg::OpusFile),
                FileType::Speex => ogg!(lofty::ogg::SpeexFile),
                _ => ogg!(lofty::ogg::VorbisFile),
            }
        }
        Fam::Mp4 => {
            use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};
            let mut f = lofty::mp4::Mp4File::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
            if f.ilst().is_none() {
                f.set_ilst(Ilst::default());
            }
            let il = f.ilst_mut().expect("just set");
            for (field, val) in changes {
                if field == "tracknumber" || field == "discnumber" {
                    let (n, t) = val.split_once('/').unwrap_or((val.as_str(), ""));
                    let (n, t) = (n.trim().parse::<u32>().unwrap_or(0), t.trim().parse::<u32>().unwrap_or(0));
                    if field == "tracknumber" {
                        il.remove_track();
                        il.remove_track_total();
                        if n > 0 {
                            il.set_track(n);
                        }
                        if t > 0 {
                            il.set_track_total(t);
                        }
                    } else {
                        il.remove_disk();
                        il.remove_disk_total();
                        if n > 0 {
                            il.set_disk(n);
                        }
                        if t > 0 {
                            il.set_disk_total(t);
                        }
                    }
                    continue;
                }
                let Some(code) = mp4_atom(field) else { continue };
                let _ = il.remove(&AtomIdent::Fourcc(code)).count();
                if !val.is_empty() {
                    il.insert(Atom::new(AtomIdent::Fourcc(code), AtomData::UTF8(val.clone())));
                }
            }
            match cover {
                Cover::Keep => {}
                Cover::Remove => il.remove_pictures(),
                Cover::Set(d, m) => {
                    il.remove_pictures();
                    il.insert_picture(picture(d.clone(), m));
                }
            }
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            f.save_to(&mut file, wo).map_err(|e| e.to_string())?;
        }
        Fam::Ape(ft) => {
            use lofty::ape::{ApeItem, ApeTag};
            macro_rules! ape {
                ($t:ty) => {{
                    let mut f = <$t>::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
                    let mut tag = f.ape().cloned().unwrap_or_else(ApeTag::default);
                    for (field, val) in changes {
                        for k in ape_keys(field) {
                            tag.remove(k);
                        }
                        if !val.is_empty() {
                            if let Ok(item) = ApeItem::new(ape_keys(field)[0].to_string(), ItemValue::Text(val.clone())) {
                                tag.insert(item);
                            }
                        }
                    }
                    match cover {
                        Cover::Keep => {}
                        Cover::Remove => tag.remove("Cover Art (Front)"),
                        Cover::Set(d, m) => {
                            tag.remove("Cover Art (Front)");
                            let mut raw = format!("cover.{}", if *m == "image/png" { "png" } else { "jpg" }).into_bytes();
                            raw.push(0);
                            raw.extend_from_slice(d);
                            if let Ok(item) = ApeItem::new("Cover Art (Front)".into(), ItemValue::Binary(raw)) {
                                tag.insert(item);
                            }
                        }
                    }
                    f.set_ape(tag);
                    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                    f.save_to(&mut file, wo).map_err(|e| e.to_string())?;
                }};
            }
            match ft {
                FileType::WavPack => ape!(lofty::wavpack::WavPackFile),
                FileType::Mpc => ape!(lofty::musepack::MpcFile),
                _ => ape!(lofty::ape::ApeFile),
            }
        }
        Fam::Id3(ft) => {
            macro_rules! id3 {
                ($t:ty) => {{
                    let mut f = <$t>::read_from(&mut file, opts()).map_err(|e| e.to_string())?;
                    let mut tag = f.id3v2().cloned().unwrap_or_default();
                    write_id3(&mut tag, changes, cover);
                    f.set_id3v2(tag);
                    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                    f.save_to(&mut file, wo).map_err(|e| e.to_string())?;
                }};
            }
            match ft {
                FileType::Aiff => id3!(lofty::iff::aiff::AiffFile),
                FileType::Wav => id3!(lofty::iff::wav::WavFile),
                _ => id3!(lofty::mpeg::MpegFile),
            }
        }
        Fam::Dsf => {
            drop(file);
            let (tag, _, at) = dsf_tag(path)?;
            let mut tag = tag.unwrap_or_default();
            write_id3(&mut tag, changes, cover);
            dsf_write(path, &tag, at)?;
        }
    }
    Ok(())
}

/// Album lookup result for the tag editor.
#[derive(Clone, Debug, Default)]
pub struct AlbumInfo {
    pub name: String,
    pub artist: String,
    pub year: String,
    pub genre: String,
    pub cover_url: String,
    pub source: String,
}

pub fn norm(s: &str) -> String {
    crate::util::title_norm(s)
}

/// Best match on iTunes (then Deezer).
pub fn online_album_lookup(artist: &str, album: &str) -> Option<AlbumInfo> {
    let (a, al) = (norm(artist), norm(album));
    if al.is_empty() {
        return None;
    }
    let score = |name: &str, who: &str| {
        let (n, w) = (norm(name), norm(who));
        let mut s = 0;
        if n == al {
            s += 3;
        } else if al.len() > 3 && (al.contains(&n) || n.contains(&al)) && !n.is_empty() {
            s += 1;
        }
        if !a.is_empty() && !w.is_empty() && (a.contains(&w) || w.contains(&a)) {
            s += 2;
        }
        s
    };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let q = crate::net::query(&[("term", &format!("{artist} {album}")), ("entity", "album"), ("limit", "15")]);
    if let Ok(v) = crate::net::get_json(&format!("https://itunes.apple.com/search?{q}")) {
        let res = v.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
        if let Some(b) = res.iter().max_by_key(|x| score(&s(x, "collectionName"), &s(x, "artistName"))) {
            if score(&s(b, "collectionName"), &s(b, "artistName")) >= 3 {
                return Some(AlbumInfo {
                    name: s(b, "collectionName"),
                    artist: s(b, "artistName"),
                    year: s(b, "releaseDate").chars().take(4).collect(),
                    genre: s(b, "primaryGenreName"),
                    cover_url: s(b, "artworkUrl100").replace("100x100bb", "1200x1200bb"),
                    source: "iTunes".into(),
                });
            }
        }
    }
    let q = crate::net::query(&[("q", &format!("artist:\"{artist}\" album:\"{album}\"")), ("limit", "15")]);
    if let Ok(v) = crate::net::get_json(&format!("https://api.deezer.com/search/album?{q}")) {
        let res = v.get("data").and_then(|r| r.as_array()).cloned().unwrap_or_default();
        let who = |x: &serde_json::Value| x.get("artist").map(|a| s(a, "name")).unwrap_or_default();
        if let Some(b) = res.iter().max_by_key(|x| score(&s(x, "title"), &who(x))) {
            if score(&s(b, "title"), &who(b)) >= 3 {
                let mut out = AlbumInfo { name: s(b, "title"), artist: who(b), cover_url: s(b, "cover_xl"), source: "Deezer".into(), ..Default::default() };
                if let Some(id) = b.get("id").and_then(|x| x.as_i64()) {
                    if let Ok(d) = crate::net::get_json(&format!("https://api.deezer.com/album/{id}")) {
                        out.year = s(&d, "release_date").chars().take(4).collect();
                        out.genre = d.pointer("/genres/data/0/name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    }
                }
                return Some(out);
            }
        }
    }
    None
}

/// Same rules as the Python version: no path characters, no trailing dots, at most 150 chars.
pub fn safe_filename(s: &str) -> String {
    let s: String = s.chars().filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && (*c as u32) >= 0x20).collect();
    let s = s.trim().trim_end_matches('.');
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(150).collect();
    if s.is_empty() { "untitled".into() } else { s }
}

/// Fills a rename pattern like `{disc}-{track:02} {artist} - {title}`.
pub fn fill_pattern(pattern: &str, vals: &[(&str, String)]) -> Result<String, String> {
    let mut out = String::new();
    let mut it = pattern.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' if it.peek() == Some(&'{') => {
                it.next();
                out.push('{');
            }
            '}' if it.peek() == Some(&'}') => {
                it.next();
                out.push('}');
            }
            '{' => {
                let mut spec = String::new();
                loop {
                    match it.next() {
                        Some('}') => break,
                        Some(ch) => spec.push(ch),
                        None => return Err("pattern problem: a { isn't closed".into()),
                    }
                }
                let (name, fmt) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
                let v = vals.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone()).ok_or_else(|| format!("pattern problem: '{name}'"))?;
                if fmt.is_empty() {
                    out.push_str(&v);
                } else {
                    let zero = fmt.starts_with('0');
                    let w: usize = fmt.trim_start_matches('0').parse().map_err(|_| format!("pattern problem: bad format '{fmt}'"))?;
                    let pad = w.saturating_sub(v.chars().count());
                    out.push_str(&(if zero { "0" } else { " " }).repeat(pad));
                    out.push_str(&v);
                }
            }
            '}' => return Err("pattern problem: a single } ".into()),
            c => out.push(c),
        }
    }
    Ok(out)
}

pub fn first_int(s: &str) -> u32 {
    s.split('/').next().unwrap_or("").trim().parse().unwrap_or(0)
}

/// Title Case that leaves small words and ALL-CAPS words alone.
pub fn title_case(s: &str) -> String {
    const SMALL: [&str; 16] = ["a", "an", "the", "and", "but", "or", "for", "nor", "on", "at", "to", "of", "in", "by", "vs", "feat."];
    s.split(' ')
        .enumerate()
        .map(|(i, w)| {
            let lw = w.to_lowercase();
            if i > 0 && SMALL.contains(&lw.as_str()) {
                lw
            } else if w.chars().count() > 1 && w.chars().any(char::is_alphabetic) && w.chars().all(|c| !c.is_lowercase()) {
                w.to_string()
            } else {
                let mut cs = w.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "03 - Song name" → "Song name"
pub fn title_from_stem(stem: &str) -> String {
    let re = regex::Regex::new(r"^\s*(\d+[-.]?)?\d*\s*[-._)]*\s*").expect("valid regex");
    let t = re.replace(stem, "");
    let t = if t.is_empty() { stem.to_string() } else { t.to_string() };
    t.replace('_', " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalising_names() {
        assert_eq!(norm("Beyoncé — Lemonade (Deluxe Edition)"), "beyoncé lemonade");
        assert_eq!(norm("Song feat. Someone"), "song");
    }

    #[test]
    fn helpers() {
        let v = [("track", "3".to_string()), ("title", "Song".to_string()), ("disc", "1".to_string())];
        assert_eq!(fill_pattern("{disc}-{track:02} {title}", &v).unwrap(), "1-03 Song");
        assert!(fill_pattern("{nope}", &v).is_err());
        assert_eq!(safe_filename("a/b: c?. "), "ab c");
        assert_eq!(title_case("the end of the WORLD"), "The End of the WORLD");
        assert_eq!(title_from_stem("03 - Song_name"), "Song name");
        assert_eq!(title_from_stem("1-02 Intro"), "Intro");
    }
}
