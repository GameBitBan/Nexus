//! One Nexus at a time: a second copy would sign in to Soulseek with the same name and fight over the
//! audio device. A second launch asks the running one to come to the front and exits. Same socket name
//! and protocol as the Python version ("show\n" → "ok"), so the two recognise each other.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static CTX: OnceLock<egui::Context> = OnceLock::new();
pub static SHOW: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

fn user() -> String {
    ["LOGNAME", "USER", "LNAME", "USERNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_else(|| "user".into())
}

pub fn name() -> String {
    format!("nexus-{}", user())
}

pub fn set_context(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// Call each frame: true once after another launch asked us to show ourselves.
pub fn take_show() -> bool {
    SHOW.swap(false, Ordering::SeqCst)
}

pub fn release() {
    RUNNING.store(false, Ordering::SeqCst);
    #[cfg(unix)]
    let _ = std::fs::remove_file(unix::path());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    pub fn path() -> PathBuf {
        std::env::temp_dir().join(name())
    }

    /// Is a responsive Nexus already running? It has to answer, so one that's busy closing doesn't count.
    pub fn ping(wait: Duration) -> bool {
        let Ok(mut s) = UnixStream::connect(path()) else { return false };
        let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
        if s.write_all(b"show\n").is_err() {
            return false;
        }
        let t0 = Instant::now();
        let mut got = Vec::new();
        let mut buf = [0u8; 64];
        while t0.elapsed() < wait {
            match s.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(2).any(|w| w == b"ok") {
                        return true;
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
        }
        false
    }

    pub fn serve() -> bool {
        let p = path();
        let _ = std::fs::remove_file(&p);
        let Ok(l) = UnixListener::bind(&p) else { return false };
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
        }
        RUNNING.store(true, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("nexus-instance".into())
            .spawn(move || {
                for s in l.incoming() {
                    if !RUNNING.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(mut s) = s else { continue };
                    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut buf = [0u8; 64];
                    let n = s.read(&mut buf).unwrap_or(0);
                    if buf[..n].starts_with(b"show") {
                        SHOW.store(true, Ordering::SeqCst);
                        if let Some(c) = CTX.get() {
                            c.request_repaint();
                        }
                    }
                    let _ = s.write_all(b"ok\n");
                }
            })
            .is_ok()
    }
}

/// Returns false when another Nexus is running (it has been asked to come to the front).
/// After an update restart, waits for the old copy to finish closing instead.
pub fn claim(restarted: bool) -> bool {
    #[cfg(unix)]
    {
        use std::time::{Duration, Instant};
        if restarted {
            let t0 = Instant::now();
            while unix::ping(Duration::from_millis(600)) && t0.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(300));
            }
        } else if unix::ping(Duration::from_millis(1500)) {
            return false;
        }
        unix::serve();
    }
    let _ = restarted;
    true
}
