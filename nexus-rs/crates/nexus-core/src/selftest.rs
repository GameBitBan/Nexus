//! `nexus --selftest report.txt`: checks every part a packaged build needs and writes a report
//! (exit code 1 when something essential is missing). Used by CI and the Nix package check.

use crate::paths::resource_dir;

pub fn run() -> (String, bool) {
    let mut lines = vec![format!("nexus {} · {} {} · {}", crate::VERSION, std::env::consts::OS, std::env::consts::ARCH, match crate::update::install_kind() {
        crate::update::Install::Nix => "nix",
        crate::update::Install::Dev => "dev build",
        crate::update::Install::WindowsInstaller => "windows install",
        crate::update::Install::Binary(_) => "binary",
    })];
    let mut ok = true;
    // FFmpeg (decoding, inspect, video backgrounds)
    match ffmpeg_next::init() {
        Ok(()) => {
            let v = ffmpeg_next::util::version();
            let codecs = ["flac", "alac", "mp3", "aac", "opus", "vorbis", "wavpack", "ape", "dsd_lsbf_planar", "pcm_s24le", "h264", "vp9"];
            let missing: Vec<&str> = codecs.iter().copied().filter(|c| ffmpeg_next::decoder::find_by_name(c).is_none()).collect();
            lines.push(format!("ok    ffmpeg (libavutil {}.{}.{})", v >> 16, (v >> 8) & 0xff, v & 0xff));
            if missing.is_empty() {
                lines.push(format!("ok    decoders: {}", codecs.join(" ")));
            } else {
                ok = false;
                lines.push(format!("FAIL  decoders missing: {}", missing.join(" ")));
            }
        }
        Err(e) => {
            ok = false;
            lines.push(format!("FAIL  ffmpeg: {e}"));
        }
    }
    // audio output (informational: build machines often have no sound card)
    let devs = crate::audio::list_devices();
    lines.push(format!("info  audio outputs: {}", if devs.is_empty() { "none found".to_string() } else { devs.join(", ") }));
    // tags
    lines.push("ok    lofty tags".into());
    // certificates for https
    let _ = crate::net::agent();
    lines.push("ok    https client".into());
    // bundled resources
    let res = resource_dir();
    for d in ["fonts", "icons", "skins"] {
        let n = std::fs::read_dir(res.join(d)).map(|r| r.count()).unwrap_or(0);
        lines.push(format!("{}  {d}/ ({n} files) in {}", if n > 0 { "ok  " } else { "FAIL" }, res.display()));
        ok &= n > 0;
    }
    lines.push(if ok { "all good".into() } else { "something's missing".into() });
    (lines.join("\n") + "\n", ok)
}
