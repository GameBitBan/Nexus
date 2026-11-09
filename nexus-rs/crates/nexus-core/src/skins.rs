//! Skins (`.nexusskin` = a zip): skin.json (name, colours, whitelisted look settings), optional style.qss,
//! assets/ (background image or video) and fonts/. No code, so imported skins are safe. Same format and
//! rules as the Python version, so skins move freely between the two.

use crate::config::{Config, defaults};
use crate::paths::{resource_dir, skins_dir};
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const SKIN_EXT: &str = "nexusskin";
pub const FORMAT: i64 = 1;
const LIMIT: u64 = 150_000_000;
pub const COLOR_KEYS: [&str; 7] = ["bg", "panel", "text", "dim", "accent", "select", "border"];
const FILE_OK: [&str; 16] = ["json", "qss", "css", "txt", "md", "png", "jpg", "jpeg", "webp", "gif", "svg", "ttf", "otf", "mp4", "webm", "mov"];
pub const VIDEO_EXT: [&str; 5] = ["mp4", "webm", "mov", "m4v", "mkv"];

#[derive(Clone, Copy, PartialEq)]
enum T {
    Str,
    Bool,
    Int,
    Num,
    Dict,
}

/// Look settings a skin may set, with the type each must have.
const SKIN_KEYS: [(&str, T); 36] = [
    ("layout", T::Str),
    ("now_panel", T::Bool),
    ("dial_size", T::Int),
    ("dial_spin", T::Bool),
    ("dial_spin_seconds", T::Int),
    ("theme", T::Str),
    ("font_family", T::Str),
    ("display_font", T::Str),
    ("font_size", T::Num),
    ("uppercase", T::Bool),
    ("brackets", T::Str),
    ("bracket_padding", T::Bool),
    ("nav_marker", T::Str),
    ("header_case", T::Str),
    ("header_text", T::Str),
    ("corner_radius", T::Int),
    ("cover_size", T::Int),
    ("list_cover_size", T::Int),
    ("placeholder", T::Str),
    ("ui_case", T::Str),
    ("control_style", T::Str),
    ("pill_buttons", T::Bool),
    ("progress_style", T::Str),
    ("volume_style", T::Str),
    ("progress_chars", T::Str),
    ("glyphs", T::Dict),
    ("sidebar_width", T::Int),
    ("ambient_background", T::Bool),
    ("ambient_strength", T::Int),
    ("dial_visualizer", T::Bool),
    ("vis_style", T::Str),
    ("vis_color", T::Str),
    ("vis_sensitivity", T::Int),
    ("background_alpha", T::Int),
    ("skin_bg_opacity", T::Int),
    ("skin_bg_blur", T::Bool),
];

pub fn skin_keys() -> impl Iterator<Item = &'static str> {
    SKIN_KEYS.iter().map(|(k, _)| *k)
}

#[derive(Clone, Debug, Default)]
pub struct Skin {
    pub slug: String,
    pub path: PathBuf,
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub background: String,
    pub colors: Map<String, Value>,
    pub settings: Map<String, Value>,
    pub qss: String,
}

impl Skin {
    pub fn background_path(&self) -> Option<PathBuf> {
        if self.background.is_empty() {
            return None;
        }
        let p = self.path.join(&self.background);
        p.exists().then_some(p)
    }
}

pub fn slug(name: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
            dash = false;
        } else if !dash && !s.is_empty() {
            s.push('-');
            dash = true;
        }
    }
    let s: String = s.trim_end_matches('-').chars().take(48).collect();
    if s.is_empty() { "skin".into() } else { s }
}

/// A path inside a skin folder, or "" — no absolute paths, drives, network shares or "..".
pub fn skin_rel(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    let parts: Vec<&str> = p.split('/').filter(|x| !x.is_empty() && *x != ".").collect();
    if p.is_empty() || p.starts_with('/') || p.contains(':') || parts.contains(&"..") || parts.is_empty() {
        return String::new();
    }
    parts.join("/")
}

/// "#abc" / "#rrggbb" / "#aarrggbb" → "#rrggbb" (what Qt's QColor.name() gives).
pub fn norm_color(v: &str) -> Option<String> {
    let s = v.trim().trim_start_matches('#');
    let ok = s.chars().all(|c| c.is_ascii_hexdigit());
    match (ok, s.len()) {
        (true, 3) => Some(format!("#{}", s.chars().flat_map(|c| [c, c]).collect::<String>().to_lowercase())),
        (true, 6) => Some(format!("#{}", s.to_lowercase())),
        (true, 8) => Some(format!("#{}", s[2..].to_lowercase())),
        _ => None,
    }
}

fn trunc(v: &Value, n: usize) -> String {
    v.as_str().unwrap_or("").chars().take(n).collect()
}

/// Keeps only known keys with the right types; colours must look like colours.
pub fn clean_meta(meta: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    let name = trunc(meta.get("name").unwrap_or(&Value::Null), 60);
    out.insert("name".into(), json!(if name.is_empty() { "Untitled skin".to_string() } else { name }));
    out.insert("author".into(), json!(trunc(meta.get("author").unwrap_or(&Value::Null), 60)));
    out.insert("description".into(), json!(trunc(meta.get("description").unwrap_or(&Value::Null), 300)));
    let ver = trunc(meta.get("version").unwrap_or(&Value::Null), 20);
    out.insert("version".into(), json!(if ver.is_empty() { "1.0".to_string() } else { ver }));
    out.insert("format".into(), json!(FORMAT));
    out.insert("background".into(), json!(skin_rel(meta.get("background").and_then(Value::as_str).unwrap_or(""))));
    let mut settings = Map::new();
    if let Some(Value::Object(s)) = meta.get("settings") {
        for (k, v) in s {
            let Some((_, t)) = SKIN_KEYS.iter().find(|(key, _)| key == k) else { continue };
            let ok = match t {
                T::Bool => v.is_boolean(),
                T::Str => v.is_string(),
                T::Int => v.is_i64() || v.is_u64(),
                T::Num => v.is_number(),
                T::Dict => v.is_object(),
            };
            if !ok {
                continue;
            }
            let v = match (t, v) {
                (T::Dict, Value::Object(m)) => Value::Object(
                    m.iter().map(|(a, b)| (a.chars().take(12).collect(), json!(b.as_str().unwrap_or("").chars().take(12).collect::<String>()))).collect(),
                ),
                (T::Str, Value::String(s)) => json!(s.chars().take(200).collect::<String>()),
                _ => v.clone(),
            };
            settings.insert(k.clone(), v);
        }
    }
    out.insert("settings".into(), Value::Object(settings));
    let mut colors = Map::new();
    if let Some(Value::Object(c)) = meta.get("colors") {
        for (k, v) in c {
            if COLOR_KEYS.contains(&k.as_str()) {
                if let Some(n) = v.as_str().and_then(norm_color) {
                    colors.insert(k.clone(), json!(n));
                }
            }
        }
    }
    out.insert("colors".into(), Value::Object(colors));
    out
}

pub fn read_dir(path: &Path) -> Option<Skin> {
    let text = std::fs::read_to_string(path.join("skin.json")).ok()?;
    let meta: Value = serde_json::from_str(&text).ok()?;
    let m = clean_meta(&meta);
    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let obj = |k: &str| m.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
    Some(Skin {
        slug: path.file_name()?.to_string_lossy().to_string(),
        path: path.to_path_buf(),
        name: s("name"),
        author: s("author"),
        description: s("description"),
        version: s("version"),
        background: s("background"),
        colors: obj("colors"),
        settings: obj("settings"),
        qss: std::fs::read(path.join("style.qss")).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default(),
    })
}

pub fn list() -> Vec<Skin> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(skins_dir())
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs.iter().filter_map(|d| read_dir(d)).collect()
}

pub fn active(cfg: &Config) -> Option<Skin> {
    let slug = cfg.s("skin");
    if slug.is_empty() {
        return None;
    }
    let d = skins_dir().join(&slug);
    if d.is_dir() { read_dir(&d) } else { None }
}

/// What's behind the window: your own background if set, else the skin's.
pub fn background_path(cfg: &Config) -> Option<PathBuf> {
    let own = cfg.s("custom_background");
    let own = own.trim();
    if !own.is_empty() && Path::new(own).exists() {
        return Some(PathBuf::from(own));
    }
    active(cfg).and_then(|s| s.background_path())
}

pub fn is_video(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| VIDEO_EXT.contains(&e.as_str()))
}

fn zname<R: std::io::Read>(e: &zip::read::ZipFile<'_, R>) -> String {
    e.name().map(|n| n.to_string()).unwrap_or_default()
}

/// Does a zip contain `name` at its top level (or one folder down)?
pub fn zip_has(file: &Path, name: &str) -> bool {
    let Ok(f) = std::fs::File::open(file) else { return false };
    let Ok(mut z) = zip::ZipArchive::new(f) else { return false };
    (0..z.len()).any(|i| {
        z.by_index(i).map(|e| {
            let n = zname(&e);
            n.rsplit('/').next() == Some(name) && n.matches('/').count() <= 1
        })
        .unwrap_or(false)
    })
}

/// Unzips a .nexusskin into the skins folder (safely). Returns the skin's slug.
pub fn import_file(file: &Path) -> Result<String, String> {
    if std::fs::metadata(file).map(|m| m.len()).unwrap_or(0) > LIMIT {
        return Err("that skin is too big (150 MB max)".into());
    }
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("not a zip: {e}"))?;
    let mut meta_name = None;
    let mut total = 0u64;
    for i in 0..z.len() {
        let e = z.by_index(i).map_err(|e| e.to_string())?;
        if e.is_dir() {
            continue;
        }
        total += e.size();
        let n = zname(&e);
        if meta_name.is_none() && n.rsplit('/').next() == Some("skin.json") && n.matches('/').count() <= 1 {
            meta_name = Some(n);
        }
    }
    let meta_name = meta_name.ok_or("not a Nexus skin (no skin.json inside)")?;
    if total > LIMIT {
        return Err("that skin unpacks too big".into());
    }
    let prefix = meta_name[..meta_name.len() - "skin.json".len()].to_string();
    let mut raw = String::new();
    z.by_name(&meta_name).map_err(|e| e.to_string())?.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let meta = clean_meta(&serde_json::from_str::<Value>(&raw).map_err(|e| format!("bad skin.json: {e}"))?);
    let name = meta.get("name").and_then(Value::as_str).unwrap_or("skin").to_string();
    let dest = skins_dir().join(slug(&name));
    let tmp = dest.with_file_name(format!("{}.tmp", slug(&name)));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        let name = zname(&e);
        if e.is_dir() || !name.starts_with(&prefix) {
            continue;
        }
        let rel = name[prefix.len()..].replace('\\', "/");
        let parts: Vec<&str> = rel.split('/').collect();
        if rel.is_empty() || rel.starts_with('/') || rel.contains(':') || parts.contains(&"..") {
            continue;
        }
        let ext = Path::new(&rel).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !FILE_OK.contains(&ext.as_str()) && ext != "m4v" {
            continue; // skins carry no code: anything else is dropped
        }
        let out = parts.iter().fold(tmp.clone(), |p, x| p.join(x));
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut data = Vec::new();
        e.read_to_end(&mut data).map_err(|e| e.to_string())?;
        std::fs::write(&out, data).map_err(|e| e.to_string())?;
    }
    std::fs::write(tmp.join("skin.json"), serde_json::to_string_pretty(&Value::Object(meta)).unwrap_or_default()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
}

pub fn export(skin: &Skin, file: &Path) -> Result<(), String> {
    let f = std::fs::File::create(file).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&skin.path).into_iter().flatten().filter(|e| e.file_type().is_file()).map(|e| e.into_path()).collect();
    files.sort();
    for p in files {
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !FILE_OK.contains(&ext.as_str()) && ext != "m4v" {
            continue;
        }
        let rel = p.strip_prefix(&skin.path).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        z.start_file(rel, opts).map_err(|e| e.to_string())?;
        z.write_all(&std::fs::read(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Skin settings on top of the defaults (so nothing from the old look leaks through), plus its colours.
/// The layout stays if you picked one yourself (`layout_pref`).
pub fn apply(cfg: &mut Config, slug_: &str) -> Result<(), String> {
    let m = read_dir(&skins_dir().join(slug_)).ok_or("skin not found")?;
    let keep: Vec<(String, Value)> =
        if cfg.b("layout_pref") { ["layout", "now_panel"].iter().filter_map(|k| cfg.get(k).map(|v| (k.to_string(), v.clone()))).collect() } else { Vec::new() };
    let d = defaults();
    for k in skin_keys() {
        if let Some(v) = d.get(k) {
            cfg.set(k, v.clone());
        }
    }
    cfg.set("skin_bg_opacity", 60);
    cfg.set("skin_bg_blur", false);
    for (k, v) in &m.settings {
        if k == "glyphs" {
            let mut g = d.get("glyphs").and_then(Value::as_object).cloned().unwrap_or_default();
            if let Value::Object(o) = v {
                g.extend(o.clone());
            }
            cfg.set(k, Value::Object(g));
        } else {
            cfg.set(k, v.clone());
        }
    }
    for (k, v) in keep {
        cfg.set(&k, v);
    }
    cfg.set("color_overrides", Value::Object(m.colors.clone()));
    cfg.set("skin", slug_);
    cfg.set("preset", "");
    cfg.save();
    Ok(())
}

pub enum Background {
    Keep,
    Remove,
    File(PathBuf),
    Bytes(Vec<u8>, &'static str),
}

/// Saves the current look (settings + colours) as a skin folder; returns its slug.
pub fn save_current(cfg: &mut Config, colors: Map<String, Value>, name: &str, author: &str, description: &str, qss: &str, background: Background) -> Result<String, String> {
    let settings: Map<String, Value> = skin_keys().filter_map(|k| cfg.get(k).map(|v| (k.to_string(), v.clone()))).collect();
    let old = active(cfg);
    let s = slug(name);
    let d = skins_dir().join(&s);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    if let Some(o) = &old {
        if o.path != d {
            for sub in ["assets", "fonts"] {
                let src = o.path.join(sub);
                if src.is_dir() {
                    for e in walkdir::WalkDir::new(&src).into_iter().flatten().filter(|e| e.file_type().is_file()) {
                        let rel = e.path().strip_prefix(&o.path).unwrap_or(e.path());
                        let to = d.join(rel);
                        if let Some(p) = to.parent() {
                            let _ = std::fs::create_dir_all(p);
                        }
                        let _ = std::fs::copy(e.path(), to);
                    }
                }
            }
        }
    }
    let mut bg = old.as_ref().map(|o| o.background.clone()).unwrap_or_default();
    let own = cfg.s("custom_background");
    let background = match background {
        Background::Keep if !own.trim().is_empty() && Path::new(own.trim()).exists() => Background::File(PathBuf::from(own.trim())),
        b => b,
    };
    match background {
        Background::Keep => {}
        Background::Remove => bg.clear(),
        Background::File(src) => {
            let assets = d.join("assets");
            std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
            let ext = src.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_else(|| "jpg".into());
            for e in std::fs::read_dir(&assets).map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default() {
                let p = e.path();
                if p.file_stem().is_some_and(|s| s == "background") && p.canonicalize().ok() != src.canonicalize().ok() {
                    let _ = std::fs::remove_file(p);
                }
            }
            bg = format!("assets/background.{ext}");
            let dst = d.join(&bg);
            if dst.canonicalize().ok() != src.canonicalize().ok() {
                std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
            }
            if !own.is_empty() {
                cfg.set("custom_background", "");
            }
        }
        Background::Bytes(data, ext) => {
            let assets = d.join("assets");
            std::fs::create_dir_all(&assets).map_err(|e| e.to_string())?;
            bg = format!("assets/background.{ext}");
            std::fs::write(d.join(&bg), data).map_err(|e| e.to_string())?;
        }
    }
    let meta = json!({"name": name, "author": author, "description": description, "version": "1.0", "format": FORMAT,
                      "background": bg, "colors": colors, "settings": settings});
    let mut clean = clean_meta(&meta);
    clean.insert("made_with".into(), json!(format!("Nexus {}", crate::VERSION)));
    std::fs::write(d.join("skin.json"), serde_json::to_string_pretty(&Value::Object(clean)).unwrap_or_default()).map_err(|e| e.to_string())?;
    if qss.trim().is_empty() {
        let _ = std::fs::remove_file(d.join("style.qss"));
    } else {
        std::fs::write(d.join("style.qss"), qss).map_err(|e| e.to_string())?;
    }
    cfg.set("skin", s.clone());
    cfg.set("preset", "");
    cfg.save();
    Ok(s)
}

/// First run: copy the skins that ship with Nexus into the skins folder (each one only once ever).
pub fn install_bundled(cfg: &mut Config) {
    let src = resource_dir().join("skins");
    let Ok(rd) = std::fs::read_dir(&src) else { return };
    let have: Vec<String> = std::fs::read_dir(skins_dir()).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    let mut seen: Vec<String> = cfg.strings("skins_installed");
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == SKIN_EXT)).collect();
    files.sort();
    let mut changed = false;
    for f in files {
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if seen.contains(&stem) {
            continue;
        }
        if !have.contains(&slug(&stem)) {
            if let Err(e) = import_file(&f) {
                log::warn!("bundled skin {}: {e}", f.display());
            }
        }
        seen.push(stem);
        changed = true;
    }
    if changed {
        seen.sort();
        cfg.set("skins_installed", json!(seen));
        cfg.save();
    }
}

// ---------------------------------------------------------------- the stylesheet, as far as Nexus 2 can use it

/// What Nexus 2 takes from a skin's Qt stylesheet: panel borders and the primary button's colours.
/// (Qt stylesheets can't apply to the new UI wholesale; these are the parts the bundled skins use.)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkinStyle {
    /// border around the rail / card / now-playing panels: (width px, "#rrggbb", alpha 0..1)
    pub panel_border: Option<(f32, String, f32)>,
    pub primary_bg: Option<String>,
    pub primary_fg: Option<String>,
    pub primary_border: Option<String>,
    pub h1_color: Option<String>,
}

fn parse_color(v: &str, colors: &Map<String, Value>) -> Option<(String, f32)> {
    let v = v.trim();
    if let Some(k) = v.strip_prefix('@') {
        return colors.get(k).and_then(Value::as_str).map(|c| (c.to_string(), 1.0));
    }
    if let Some(inner) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let n: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if n.len() >= 3 {
            let a = n.get(3).copied().unwrap_or(1.0);
            let a = if a > 1.0 { a / 255.0 } else { a };
            return Some((format!("#{:02x}{:02x}{:02x}", n[0] as u8, n[1] as u8, n[2] as u8), a));
        }
    }
    norm_color(v).map(|c| (c, 1.0))
}

pub fn style(qss: &str, colors: &Map<String, Value>) -> SkinStyle {
    let mut st = SkinStyle::default();
    // strip comments
    let mut text = String::new();
    let mut rest = qss;
    while let Some(i) = rest.find("/*") {
        text.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map(|j| &rest[i + j + 2..]).unwrap_or("");
    }
    text.push_str(rest);
    for block in text.split('}') {
        let Some((sel, body)) = block.split_once('{') else { continue };
        let sels: Vec<String> = sel.split(',').map(|s| s.trim().to_string()).collect();
        let props: Vec<(String, String)> = body
            .split(';')
            .filter_map(|p| p.split_once(':').map(|(a, b)| (a.trim().to_lowercase(), b.trim().to_string())))
            .collect();
        let panel = sels.iter().any(|s| ["QWidget#card", "QWidget#nowpanel", "QWidget#rail", "#card", "#nowpanel", "#rail"].contains(&s.as_str()));
        let primary = sels.iter().any(|s| s == "QPushButton#pillPrimary" || s == "#pillPrimary");
        let h1 = sels.iter().any(|s| s == "QLabel#h1" || s == "#h1");
        for (k, v) in &props {
            if panel && k == "border" {
                let parts: Vec<&str> = v.splitn(3, ' ').collect();
                let w = parts.first().and_then(|x| x.trim_end_matches("px").parse::<f32>().ok()).unwrap_or(1.0);
                if let Some((c, a)) = parts.get(2).and_then(|c| parse_color(c, colors)) {
                    st.panel_border = Some((w, c, a));
                }
            }
            if primary {
                match k.as_str() {
                    "background" | "background-color" => st.primary_bg = parse_color(v, colors).map(|x| x.0),
                    "color" => st.primary_fg = parse_color(v, colors).map(|x| x.0),
                    "border-color" => st.primary_border = parse_color(v, colors).map(|x| x.0),
                    _ => {}
                }
            }
            if h1 && k == "color" {
                st.h1_color = parse_color(v, colors).map(|x| x.0);
            }
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_paths() {
        assert_eq!(slug("Amber CRT"), "amber-crt");
        assert_eq!(slug("  !!"), "skin");
        assert_eq!(skin_rel("assets/bg.jpg"), "assets/bg.jpg");
        assert_eq!(skin_rel("../x"), "");
        assert_eq!(skin_rel("C:/x"), "");
        assert_eq!(skin_rel("/etc/passwd"), "");
    }

    #[test]
    fn meta_is_cleaned() {
        let m = clean_meta(&json!({"name": "X", "colors": {"bg": "#ABC", "evil": "#fff", "text": "red"},
                                    "settings": {"corner_radius": 4, "font_size": 12.5, "uppercase": "yes", "rm_rf": true}}));
        assert_eq!(m["colors"], json!({"bg": "#aabbcc"}));
        assert_eq!(m["settings"], json!({"corner_radius": 4, "font_size": 12.5}));
    }

    #[test]
    fn stylesheet_subset() {
        let colors: Map<String, Value> = serde_json::from_value(json!({"accent": "#3ef0c0", "bg": "#000000"})).unwrap();
        let st = style(
            "/* x */ QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid rgba(62, 240, 192, 0.18); }\nQPushButton#pillPrimary { background: @accent; border-color: @accent; color: @bg; }",
            &colors,
        );
        assert_eq!(st.panel_border, Some((1.0, "#3ef0c0".into(), 0.18)));
        assert_eq!(st.primary_bg.as_deref(), Some("#3ef0c0"));
        assert_eq!(st.primary_fg.as_deref(), Some("#000000"));
    }
}

// ---------------------------------------------------------------- built-in looks (presets)

const PRESETS_JSON: &str = include_str!("../../../assets/presets.json");

pub fn preset_names() -> Vec<String> {
    let v: Map<String, Value> = serde_json::from_str(PRESETS_JSON).unwrap_or_default();
    v.keys().cloned().collect()
}

pub fn preset(name: &str) -> Option<Map<String, Value>> {
    let v: Map<String, Value> = serde_json::from_str(PRESETS_JSON).unwrap_or_default();
    v.get(name).and_then(Value::as_object).cloned()
}

/// A built-in look: sets many keys at once (all still editable after), drops any skin and colour overrides.
pub fn apply_preset(cfg: &mut Config, name: &str) {
    let Some(p) = preset(name) else { return };
    let pref = cfg.b("layout_pref");
    for (k, v) in p {
        if pref && (k == "layout" || k == "now_panel") {
            continue;
        }
        match (&v, cfg.get(&k).cloned()) {
            (Value::Object(new), Some(Value::Object(mut old))) => {
                old.extend(new.clone());
                cfg.set(&k, Value::Object(old));
            }
            _ => cfg.set(&k, v),
        }
    }
    cfg.set("preset", name);
    cfg.set("color_overrides", json!({}));
    cfg.set("skin", "");
    cfg.save();
}
