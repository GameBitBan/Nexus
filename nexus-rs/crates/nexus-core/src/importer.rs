//! Brings new music into the library as `<root>/<Album Artist>/<Album>/<file>`: dropped album zips
//! (Bandcamp etc.), finished downloads, and new files in a watched folder (e.g. Downloads).

use crate::library::{AUDIO_EXT, COVER_NAMES, IMG_EXT, ext_of, is_audio};
use crate::scanner::read_track;
use crate::util::{load_json, safe_name, save_json};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const TEMP_EXT: [&str; 6] = [".crdownload", ".part", ".partial", ".tmp", ".download", ".opdownload"];

/// Where imports go: the `import_root` setting, else the first library folder, else ~/Music.
pub fn root(import_root: &str, folders: &[String]) -> PathBuf {
    let r = import_root.trim();
    let r = if !r.is_empty() {
        crate::paths::expand_user(r)
    } else if let Some(f) = folders.first() {
        PathBuf::from(f)
    } else {
        crate::paths::home_dir().join("Music")
    };
    let _ = std::fs::create_dir_all(&r);
    r
}

#[derive(Default, Debug)]
pub struct Imported {
    pub files: Vec<String>,
    pub log: Vec<String>,
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // across drives: copy, then remove
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

/// Moves (or copies) audio files into the library, bringing `cover.jpg`-style art along.
pub fn import_files(paths: &[PathBuf], root: &Path, mv: bool) -> Imported {
    let mut out = Imported::default();
    for p in paths {
        if !p.is_file() || !is_audio(p) {
            continue;
        }
        let Some(t) = read_track(p) else { continue };
        let dest_dir = root.join(safe_name(&t.albumartist, "Unknown Artist")).join(safe_name(&t.album, "Unknown Album"));
        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            out.log.push(format!("couldn't import {}: {e}", p.display()));
            continue;
        }
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let ext = ext_of(p).unwrap_or_default();
        let dest = dest_dir.join(safe_name(&name, &format!("track.{ext}")));
        let res = if dest.exists() && dest.metadata().map(|m| m.len()).ok() == p.metadata().map(|m| m.len()).ok() {
            if mv && !same_file(&dest, p) { std::fs::remove_file(p) } else { Ok(()) }
        } else if mv {
            move_file(p, &dest)
        } else {
            std::fs::copy(p, &dest).map(|_| ())
        };
        if let Err(e) = res {
            out.log.push(format!("couldn't import {name}: {e}"));
            continue;
        }
        if let Some(parent) = p.parent() {
            if let Ok(rd) = std::fs::read_dir(parent) {
                for img in rd.flatten().map(|e| e.path()) {
                    let ok = ext_of(&img).is_some_and(|e| IMG_EXT.contains(&e.as_str()))
                        && img.file_stem().is_some_and(|s| COVER_NAMES.contains(&s.to_string_lossy().to_lowercase().as_str()));
                    if let (true, Some(fname)) = (ok, img.file_name()) {
                        if !dest_dir.join(fname).exists() {
                            let _ = std::fs::copy(&img, dest_dir.join(fname));
                        }
                    }
                }
            }
        }
        out.log.push(format!("imported {} — {}", t.artist, t.title));
        out.files.push(dest.to_string_lossy().to_string());
    }
    out
}

/// Does this zip hold music (an album) rather than a skin or an export?
pub fn zip_has_audio(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    let Ok(z) = zip::ZipArchive::new(f) else { return false };
    z.file_names().filter_map(|n| n.ok()).any(|n| is_audio(Path::new(n.as_ref())))
}

/// Unpacks an album zip (audio + images only, no path tricks) and imports it.
pub fn import_zip(zpath: &Path, root: &Path) -> Imported {
    let tmp = std::env::temp_dir().join(format!("nexus-import-{}-{}", std::process::id(), crate::util::md5_hex(&zpath.to_string_lossy())));
    let _ = std::fs::remove_dir_all(&tmp);
    let res = (|| -> Result<Imported, String> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(zpath).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut total = 0u64;
        let mut files = Vec::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| e.to_string())?;
            let name = f.name().map_err(|e| e.to_string())?.replace('\\', "/");
            let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if f.is_dir() || name.split('/').any(|c| c == "..") || !(AUDIO_EXT.contains(&ext.as_str()) || IMG_EXT.contains(&ext.as_str())) {
                continue;
            }
            total += f.size();
            if total > 20_000_000_000 {
                return Err("that zip unpacks way too big".into());
            }
            let (dir, base) = name.rsplit_once('/').unwrap_or(("_", name.as_str()));
            let out = tmp.join(safe_name(dir, "_")).join(safe_name(base, "file"));
            std::fs::create_dir_all(out.parent().unwrap_or(&tmp)).map_err(|e| e.to_string())?;
            let mut w = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 1 << 16];
            loop {
                let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                std::io::Write::write_all(&mut w, &buf[..n]).map_err(|e| e.to_string())?;
            }
            if AUDIO_EXT.contains(&ext.as_str()) {
                files.push(out);
            }
        }
        Ok(import_files(&files, root, true))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    res.unwrap_or_else(|e| Imported { files: vec![], log: vec![format!("couldn't read {}: {e}", zpath.display())] })
}

/// New audio files / zips in a folder (only new ones — what was there when watching started stays).
pub struct Watch {
    seen: HashSet<String>,
    sizes: HashMap<PathBuf, u64>,
    marked: String,
}

impl Watch {
    fn seen_file() -> PathBuf {
        crate::paths::data_dir().join("watch_seen.json")
    }

    /// `marked` = the folder whose existing files were already noted (config `_watch_marked`).
    pub fn new(marked: &str) -> Self {
        let seen: Vec<String> = load_json(&Self::seen_file(), Vec::new());
        Watch { seen: seen.into_iter().collect(), sizes: HashMap::new(), marked: marked.to_string() }
    }

    fn key(p: &Path) -> String {
        match p.metadata() {
            Ok(m) => {
                let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                format!("{}|{}|{}", p.display(), m.len(), mtime)
            }
            Err(_) => p.display().to_string(),
        }
    }

    fn save(&self) {
        let mut v: Vec<&String> = self.seen.iter().collect();
        v.sort();
        let start = v.len().saturating_sub(5000);
        let _ = save_json(&Self::seen_file(), &v[start..]);
    }

    /// Notes what's in `folder` now as old. Returns the folder to remember as marked.
    pub fn mark(&mut self, folder: &Path) -> String {
        let f = folder.to_string_lossy().to_string();
        if f != self.marked {
            if let Ok(rd) = std::fs::read_dir(folder) {
                for e in rd.flatten() {
                    self.seen.insert(Self::key(&e.path()));
                }
            }
            self.marked = f.clone();
            self.save();
        }
        f
    }

    /// One look at the folder. Returns what got imported and whether something is still arriving.
    pub fn process(&mut self, folder: &Path, root: &Path) -> (Imported, bool) {
        let mut out = Imported::default();
        let mut again = false;
        let mut pending = Vec::new();
        let Ok(rd) = std::fs::read_dir(folder) else { return (out, false) };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let zip = name.ends_with(".zip");
            if TEMP_EXT.iter().any(|t| name.ends_with(t)) || !(is_audio(&p) || zip) {
                continue;
            }
            let k = Self::key(&p);
            if self.seen.contains(&k) {
                continue;
            }
            let size = p.metadata().map(|m| m.len()).unwrap_or(0);
            if self.sizes.get(&p) != Some(&size) {
                // still downloading? wait for the size to settle
                self.sizes.insert(p.clone(), size);
                again = true;
                continue;
            }
            pending.push((p, zip, k));
        }
        for (p, zip, k) in &pending {
            self.seen.insert(k.clone());
            self.sizes.remove(p);
            let r = if *zip { import_zip(p, root) } else { import_files(std::slice::from_ref(p), root, true) };
            out.files.extend(r.files);
            out.log.extend(r.log);
        }
        if !pending.is_empty() {
            self.save();
        }
        (out, again)
    }
}
