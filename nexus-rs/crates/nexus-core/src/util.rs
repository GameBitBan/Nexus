//! Small shared helpers: safe file names, atomic JSON files, text normalisation, formatting.

use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;

const WIN_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A string that is safe as one path component on every OS (no separators, no `..`, no reserved names).
pub fn safe_name(s: &str, fallback: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_string();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if WIN_RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out = fallback.to_string();
    }
    out.chars().take(120).collect()
}

/// Reads JSON, falling back to `default` when the file is missing or broken.
pub fn load_json<T: DeserializeOwned>(path: &Path, default: T) -> T {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!("{}: {e}", path.display());
            default
        }),
        Err(_) => default,
    }
}

/// Writes JSON through a temp file + rename, so a crash never leaves a half-written file.
pub fn save_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

/// `125.4` → `2:05`, `3725` → `1:02:05`.
pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, x) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{x:02}") } else { format!("{m}:{x:02}") }
}

pub fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// Lower-case, accent-free, punctuation-free text for forgiving matches ("Beyoncé (Remastered)" ≈ "beyonce").
pub fn fold(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mut out = String::with_capacity(s.len());
    for c in s.nfkd() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c.is_alphanumeric() && !c.is_ascii() && !unicode_normalization::char::is_combining_mark(c) {
            out.extend(c.to_lowercase());
        } else if !unicode_normalization::char::is_combining_mark(c) {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Album/title matching key (Python's `DiscordPresence._norm`): drops "(Deluxe Edition)"-style tags and
/// featured artists, lower-cases, punctuation → spaces.
pub fn title_norm(x: &str) -> String {
    use std::sync::LazyLock;
    static TAGS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)\s*[\(\[][^)\]]*(edition|deluxe|remaster|version|explicit|expanded|anniversary)[^)\]]*[\)\]]").expect("regex"));
    static FEAT: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?i)\s+(?:feat\.?|ft\.?|featuring|with|&|x|,)\s+").expect("regex"));
    let x = TAGS.replace_all(x, "");
    let x = FEAT.split(&x).next().unwrap_or("").to_lowercase();
    let x: String = x.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { ' ' }).collect();
    x.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Artist matching key (Python's `_norm_name`).
pub fn name_norm(s: &str) -> String {
    let x: String = s.to_lowercase().replace('$', "s").chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { ' ' }).collect();
    x.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn md5_hex(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(safe_name("a/b:c", "x"), "a_b_c");
        assert_eq!(safe_name("..", "x"), "x");
        assert_eq!(safe_name("CON.flac", "x"), "_CON.flac");
        assert_eq!(fmt_time(125.4), "2:05");
        assert_eq!(fmt_time(3725.0), "1:02:05");
        assert_eq!(fold("Beyoncé — Halo!"), "beyonce halo");
    }
}
