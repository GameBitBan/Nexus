//! Where Nexus keeps its data and finds its bundled resources.
//!
//! The data folder is shared with the Python version of Nexus, so switching between them keeps
//! settings, playlists, play counts and skins:
//! - `NEXUS_HOME` / `MONOTONE_HOME` (portable mode) if set
//! - Windows: `%APPDATA%\nexus`
//! - elsewhere: `$XDG_CONFIG_HOME/nexus` or `~/.config/nexus`

use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    let dir = env_dir("NEXUS_HOME")
        .or_else(|| env_dir("MONOTONE_HOME"))
        .unwrap_or_else(default_data_dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[cfg(windows)]
fn default_data_dir() -> PathBuf {
    env_dir("APPDATA")
        .unwrap_or_else(|| home_dir().join("AppData").join("Roaming"))
        .join("nexus")
}

#[cfg(not(windows))]
fn default_data_dir() -> PathBuf {
    env_dir("XDG_CONFIG_HOME")
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("nexus")
}

pub fn home_dir() -> PathBuf {
    #[cfg(windows)]
    let h = env_dir("USERPROFILE");
    #[cfg(not(windows))]
    let h = env_dir("HOME");
    h.unwrap_or_else(|| PathBuf::from("."))
}

pub fn music_dir() -> PathBuf {
    #[cfg(not(windows))]
    if let Some(d) = xdg_user_dir("MUSIC") {
        return d;
    }
    home_dir().join("Music")
}

#[cfg(not(windows))]
fn xdg_user_dir(name: &str) -> Option<PathBuf> {
    let cfg = env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home_dir().join(".config"));
    let text = std::fs::read_to_string(cfg.join("user-dirs.dirs")).ok()?;
    let key = format!("XDG_{name}_DIR=");
    let line = text.lines().find(|l| l.trim_start().starts_with(&key))?;
    let v = line.trim_start()[key.len()..].trim().trim_matches('"');
    Some(PathBuf::from(v.replace("$HOME", &home_dir().to_string_lossy())))
}

/// Bundled fonts, icons and skins. Looked up in order:
/// `NEXUS_RESOURCES`, `<exe>/../share/nexus` (Nix and other Unix installs), next to the executable,
/// and the source tree (development builds).
pub fn resource_dir() -> PathBuf {
    if let Some(d) = env_dir("NEXUS_RESOURCES") {
        return d;
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.canonicalize().unwrap_or(exe);
        if let Some(bin) = exe.parent() {
            candidates.push(bin.join("../share/nexus"));
            candidates.push(bin.to_path_buf());
            candidates.push(bin.join("resources"));
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    candidates
        .into_iter()
        .find(|d| d.join("fonts").is_dir() && d.join("icons").is_dir())
        .map(|d| d.canonicalize().unwrap_or(d))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn covers_dir() -> PathBuf {
    let d = data_dir().join("covers");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn skins_dir() -> PathBuf {
    let d = data_dir().join("skins");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// `~/Music` → `/home/you/Music`.
pub fn expand_user(p: &str) -> PathBuf {
    match p.strip_prefix("~") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => home_dir().join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(p),
    }
}

/// `a/b/../c` → `a/c` without touching the disk.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
