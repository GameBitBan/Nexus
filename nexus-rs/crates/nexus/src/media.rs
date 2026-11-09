//! Desktop media controls: MPRIS on Linux (media keys, GNOME/KDE/waybar widgets, playerctl) and the
//! system media overlay on Windows.

use crate::app::Nexus;
use crossbeam_channel::{Receiver, unbounded};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};
use std::time::Duration;

pub struct Media {
    controls: MediaControls,
    rx: Receiver<MediaControlEvent>,
    meta: String,
    state: (u8, i64),
    volume: f64,
    next_tick: f64,
}

impl Media {
    /// `hwnd` is the window on Windows (unused elsewhere).
    pub fn new(ctx: &egui::Context, hwnd: Option<*mut std::ffi::c_void>) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            // no session bus (a bare tty, some containers): nothing to talk to
            use std::os::unix::fs::MetadataExt;
            let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
            let uid_bus = std::path::Path::new("/run/user").join(uid.to_string()).join("bus");
            if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() && !uid_bus.exists() {
                return None;
            }
        }
        let cfg = PlatformConfig { display_name: "Nexus", dbus_name: "nexus", hwnd };
        let mut controls = MediaControls::new(cfg).ok()?;
        let (tx, rx) = unbounded();
        let ctx = ctx.clone();
        controls
            .attach(move |ev| {
                let _ = tx.send(ev);
                ctx.request_repaint();
            })
            .ok()?;
        Some(Media { controls, rx, meta: String::new(), state: (255, 0), volume: -1.0, next_tick: 0.0 })
    }
}

impl Nexus {
    pub fn media_tick(&mut self, now: f64) {
        let Some(mut m) = self.media.take() else { return };
        let events: Vec<MediaControlEvent> = m.rx.try_iter().collect();
        // what's playing
        let cur = self.current_track().cloned();
        let key = cur.as_ref().map(|t| format!("{}\u{1}{}\u{1}{}\u{1}{}", t.path, t.title, t.cover, t.duration)).unwrap_or_default();
        if key != m.meta {
            m.meta = key;
            let art = cur
                .as_ref()
                .filter(|t| !t.cover.is_empty())
                .map(|t| nexus_core::paths::covers_dir().join(format!("{}.jpg", t.cover)))
                .filter(|p| p.exists())
                .map(|p| format!("file://{}", p.to_string_lossy().replace('\\', "/")));
            let _ = match &cur {
                Some(t) => m.controls.set_metadata(MediaMetadata {
                    title: Some(&t.title),
                    album: Some(&t.album),
                    artist: Some(&t.artist),
                    cover_url: art.as_deref(),
                    duration: (t.duration > 0.0).then(|| Duration::from_secs_f64(t.duration)),
                }),
                None => m.controls.set_metadata(MediaMetadata::default()),
            };
        }
        // play state + position (MPRIS readers poll the position, so keep it fresh while playing)
        let st = match (cur.is_some(), self.engine.state()) {
            (false, _) | (_, nexus_core::audio::State::Stopped) => 0u8,
            (_, nexus_core::audio::State::Paused) => 1,
            (_, nexus_core::audio::State::Playing) => 2,
        };
        let pos_s = self.pos.max(0.0) as i64;
        let jumped = st == 2 && (pos_s - m.state.1).abs() > 2;
        if st != m.state.0 || jumped || (st == 2 && now >= m.next_tick) || (st == 1 && pos_s != m.state.1) {
            m.state = (st, pos_s);
            m.next_tick = now + 1.0;
            let progress = Some(MediaPosition(Duration::from_secs_f64(self.pos.max(0.0))));
            let _ = m.controls.set_playback(match st {
                0 => MediaPlayback::Stopped,
                1 => MediaPlayback::Paused { progress },
                _ => MediaPlayback::Playing { progress },
            });
        }
        if st == 2 {
            self.ctx.request_repaint_after(Duration::from_millis(1000));
        }
        let vol = self.cfg.f("volume");
        if (vol - m.volume).abs() > 0.001 {
            m.volume = vol;
            let _ = m.controls.set_volume(vol);
        }
        self.media = Some(m);
        for ev in events {
            match ev {
                MediaControlEvent::Play => {
                    if !self.playing() {
                        self.toggle_play();
                    }
                }
                MediaControlEvent::Pause | MediaControlEvent::Stop => {
                    if self.playing() {
                        self.toggle_play();
                    }
                }
                MediaControlEvent::Toggle => self.toggle_play(),
                MediaControlEvent::Next => self.next(false),
                MediaControlEvent::Previous => self.prev(),
                MediaControlEvent::Seek(d) => self.seek(self.pos + if d == SeekDirection::Forward { 10.0 } else { -10.0 }),
                MediaControlEvent::SeekBy(d, by) => self.seek(self.pos + if d == SeekDirection::Forward { by.as_secs_f64() } else { -by.as_secs_f64() }),
                MediaControlEvent::SetPosition(MediaPosition(p)) => self.seek(p.as_secs_f64()),
                MediaControlEvent::SetVolume(v) => self.set_volume(v.clamp(0.0, 1.0)),
                MediaControlEvent::SetShuffle(on) => {
                    if on != self.cfg.b("shuffle") {
                        self.toggle_shuffle();
                    }
                }
                MediaControlEvent::OpenUri(u) => {
                    let p = u.strip_prefix("file://").unwrap_or(&u).to_string();
                    self.open_files(vec![std::path::PathBuf::from(p)], true);
                }
                MediaControlEvent::Raise => {
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                MediaControlEvent::Quit => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
    }
}

/// The native window handle the Windows media overlay needs.
pub fn hwnd_of(w: &impl raw_window_handle::HasWindowHandle) -> Option<*mut std::ffi::c_void> {
    match w.window_handle().ok()?.as_raw() {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
}
