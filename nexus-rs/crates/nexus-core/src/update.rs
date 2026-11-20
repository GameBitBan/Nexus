//! Self-update from GitHub Releases: the asset must come from the repo's own release downloads and
//! match GitHub's sha256 digest. Nix installs are updated by Nix, never by Nexus.

use crate::net;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const GITHUB_REPO: &str = "GameBitBan/Nexus";
/// Release asset names for Nexus 2 (Python Nexus uses nexus.zip / NexusSetup.exe).
pub const ASSET_WINDOWS: &str = "nexus2-setup-x64.exe";
pub const ASSET_LINUX: &str = "nexus2-linux-x86_64.zip";
const MAX_SIZE: u64 = 300_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// /nix/store: read-only, updated with the flake / configuration
    Nix,
    /// the Windows installer build
    WindowsInstaller,
    /// a plain binary we can swap in place
    Binary(PathBuf),
    /// `cargo run` and friends
    Dev,
}

pub fn install_kind() -> Install {
    let exe = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()).unwrap_or_default();
    let s = exe.to_string_lossy().replace('\\', "/");
    if s.starts_with("/nix/store/") {
        Install::Nix
    } else if s.contains("/target/debug/") || s.contains("/target/release/") {
        Install::Dev
    } else if cfg!(windows) {
        Install::WindowsInstaller
    } else {
        Install::Binary(exe)
    }
}

pub fn vtuple(v: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in v.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(0));
            cur.clear();
            if out.len() == 4 {
                break;
            }
        }
    }
    out
}

pub fn newer(remote: &str, current: &str) -> bool {
    !remote.is_empty() && vtuple(remote) > vtuple(current)
}

/// The newest published release of this major version that has our download: (version, url, digest).
/// Nexus 1 releases (v1.*) and drafts are skipped.
pub fn pick_release(list: &Value, want: &str, current: &str) -> (String, String, String) {
    let major = vtuple(current).first().copied().unwrap_or(2);
    let mut best: Option<(Vec<u64>, String, String, String)> = None;
    for rel in list.as_array().into_iter().flatten() {
        if rel.get("draft").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let ver = rel.get("tag_name").and_then(Value::as_str).unwrap_or("").trim_start_matches(['v', 'V']).to_string();
        let vt = vtuple(&ver);
        if vt.first().copied() != Some(major) {
            continue;
        }
        let asset = rel.get("assets").and_then(Value::as_array).and_then(|a| a.iter().find(|x| x.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(want))));
        let Some(asset) = asset else { continue };
        let url = asset.get("browser_download_url").and_then(Value::as_str).unwrap_or("").to_string();
        let digest = asset.get("digest").and_then(Value::as_str).unwrap_or("").to_string();
        if best.as_ref().is_none_or(|b| vt > b.0) {
            best = Some((vt, ver, url, digest));
        }
    }
    best.map(|b| (b.1, b.2, b.3)).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub enum Check {
    UpToDate(String),
    /// a newer version exists but this install can't update itself (Nix, dev build)
    Available(String),
    /// downloaded and checked: (version, file)
    Ready(String, PathBuf),
}

/// Looks for a newer release and downloads + verifies it. `feed` (optional) is a version.json url:
/// `{"version": "…", "zip": "<url>", "sha256": "…"}`.
pub fn check(repo: &str, feed: &str, dest: &Path) -> Result<Check, String> {
    let current = crate::VERSION;
    let kind = install_kind();
    let want = if cfg!(windows) { ASSET_WINDOWS } else { ASSET_LINUX };
    let (ver, url, digest) = if !feed.trim().is_empty() {
        let meta = net::get_json(feed.trim())?;
        let s = |k: &str| meta.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let sha = s("sha256");
        (s("version"), s("zip"), if sha.is_empty() { String::new() } else { format!("sha256:{sha}") })
    } else {
        let repo = if repo.trim().is_empty() { GITHUB_REPO } else { repo.trim().trim_matches('/') };
        // every release, not just "latest": Nexus 2 ships as pre-releases next to Nexus 1's releases
        let list = net::get_json_with(&format!("https://api.github.com/repos/{repo}/releases?per_page=40"), &[("Accept", "application/vnd.github+json")], 10)?;
        let (ver, url, digest) = pick_release(&list, want, current);
        if !url.is_empty() && !url.starts_with(&format!("https://github.com/{repo}/releases/download/")) {
            return Err("the release points somewhere other than GitHub — not installing it".into());
        }
        (ver, url, digest)
    };
    if !newer(&ver, current) {
        return Ok(Check::UpToDate(current.to_string()));
    }
    if url.is_empty() || matches!(kind, Install::Nix | Install::Dev) {
        return Ok(Check::Available(ver));
    }
    if !digest.starts_with("sha256:") {
        return Err("the release has no checksum — not installing it".into());
    }
    let _ = std::fs::create_dir_all(dest);
    let file = dest.join(format!("nexus-{ver}-{want}"));
    download_verified(&url, &digest, &file)?;
    Ok(Check::Ready(ver, file))
}

/// Downloads `url` to `file`, failing (and deleting it) unless it matches `sha256:<hex>`.
pub fn download_verified(url: &str, digest: &str, file: &Path) -> Result<(), String> {
    let mut out = std::fs::File::create(file).map_err(|e| e.to_string())?;
    let mut hasher = HashWriter { inner: &mut out, sha: Sha256::new() };
    let res = net::download(url, &mut hasher, 120, |done, _| done <= MAX_SIZE);
    let sum = format!("{:x}", hasher.sha.finalize());
    drop(out);
    if let Err(e) = res {
        let _ = std::fs::remove_file(file);
        return Err(if e == "cancelled" { "update is suspiciously big".into() } else { e });
    }
    if sum != digest.trim_start_matches("sha256:").to_lowercase() {
        let _ = std::fs::remove_file(file);
        return Err("the download doesn't match GitHub's checksum — not installing it".into());
    }
    Ok(())
}

struct HashWriter<'a, W: std::io::Write> {
    inner: &'a mut W,
    sha: Sha256,
}

impl<W: std::io::Write> std::io::Write for HashWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.sha.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Puts a downloaded update in place. Linux: swaps the binary (the running one keeps working until
/// restart). Windows: starts the installer, which closes and reopens Nexus. Returns true when the
/// caller should restart Nexus itself.
pub fn apply(file: &Path) -> Result<bool, String> {
    match install_kind() {
        Install::WindowsInstaller => {
            std::process::Command::new(file).args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]).spawn().map_err(|e| e.to_string())?;
            Ok(false)
        }
        Install::Binary(exe) => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let name = (0..z.len())
                .filter_map(|i| z.by_index(i).ok().and_then(|f| f.name().ok().map(|n| n.to_string())))
                .find(|n| n.rsplit('/').next() == Some("nexus"))
                .ok_or("the update doesn't contain the nexus program")?;
            let mut data = Vec::new();
            z.by_name(&name).map_err(|e| e.to_string())?.read_to_end(&mut data).map_err(|e| e.to_string())?;
            let tmp = exe.with_file_name(".nexus.new");
            std::fs::write(&tmp, &data).map_err(|e| format!("can't write next to the program: {e}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&tmp, &exe).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(file);
            Ok(true)
        }
        Install::Nix => Err("installed with Nix — update it through your flake or configuration".into()),
        Install::Dev => Err("this is a development build".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("2.0.1", "2.0.0-dev"));
        assert!(!newer("2.0.0", "2.0.0-dev"));
        assert!(!newer("1.19.1", "2.0.0-dev"));
        assert!(newer("2.1", "2.0.9"));
        assert_eq!(vtuple("v2.10.3-beta4"), vec![2, 10, 3, 4]);
    }

    #[test]
    fn picks_newest_nexus_2_release() {
        let list = serde_json::json!([
            {"tag_name": "v1.20.0", "draft": false, "assets": [{"name": "nexus.zip", "browser_download_url": "u1", "digest": "sha256:a"}]},
            {"tag_name": "v2.0.2", "draft": true, "assets": [{"name": "nexus2-setup-x64.exe", "browser_download_url": "d", "digest": "sha256:d"}]},
            {"tag_name": "v2.0.1", "draft": false, "prerelease": true, "assets": [{"name": "nexus2-setup-x64.exe", "browser_download_url": "u3", "digest": "sha256:c"}]},
            {"tag_name": "v2.0.0", "draft": false, "assets": [{"name": "nexus2-setup-x64.exe", "browser_download_url": "u2", "digest": "sha256:b"}]},
            {"tag_name": "v2.0.5", "draft": false, "assets": [{"name": "other.zip"}]}
        ]);
        assert_eq!(pick_release(&list, "nexus2-setup-x64.exe", "2.0.0"), ("2.0.1".into(), "u3".into(), "sha256:c".into()));
        assert_eq!(pick_release(&list, "nexus2-linux-x86_64.zip", "2.0.0"), Default::default());
    }
}
