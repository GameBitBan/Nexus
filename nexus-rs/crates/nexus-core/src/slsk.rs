//! Nexus's own Soulseek client (soulseek-rs underneath): sign in, search, download, and share your
//! library back. Runs on its own threads; the Download page talks to it through this small interface.

use crate::util::safe_name;
use parking_lot::Mutex;
use soulseek_rs::{Client, ClientSettings, DownloadStatus, PeerAddress, SessionLoss};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub user: String,
    pub password: String,
    pub server: String,
    pub server_port: u16,
    pub listen_port: u16,
    pub upnp: bool,
    pub shares: Vec<String>,
    pub upload_slots: usize,
    pub download_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Off,
    Connecting,
    Online,
    Error(String),
}

/// One file in a search answer (attribute names as in Python Nexus).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RFile {
    pub filename: String,
    pub size: u64,
    pub bit_rate: Option<u32>,
    pub length: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

/// One user's answer to a search.
#[derive(Clone, Debug, Default)]
pub struct Response {
    pub username: String,
    pub free_slot: bool,
    pub speed: u64,
    pub queue: u32,
    pub files: Vec<RFile>,
}

/// A download as the page shows it.
#[derive(Clone, Debug)]
pub struct Transfer {
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub done: u64,
    pub speed: f64,
    pub state: TState,
    pub place: Option<u32>,
    pub local_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TState {
    Queued,
    Downloading,
    Paused,
    Done,
    Cancelled,
    Failed(String),
}

impl TState {
    pub fn finished(&self) -> bool {
        matches!(self, TState::Done | TState::Cancelled | TState::Failed(_))
    }
    pub fn failed(&self) -> bool {
        matches!(self, TState::Cancelled | TState::Failed(_))
    }
}

#[derive(Clone, Debug)]
pub struct Upload {
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub sent: u64,
    pub state: String,
}

struct Inner {
    cfg: Option<Config>,
    client: Option<Arc<Client>>,
    state: State,
    kicked: bool,
    generation: u64,
    upnp: Option<crate::netcheck::Upnp>,
}

pub struct Slsk {
    inner: Arc<Mutex<Inner>>,
    last_search: Mutex<Option<Instant>>,
    note: Mutex<String>,
}

/// Send the library's log lines to `soulseek.log` (call first thing in `main`, before any threads).
pub fn init_logging(path: &Path) {
    if let Ok(m) = std::fs::metadata(path) {
        if m.len() > 1_500_000 {
            let _ = std::fs::rename(path, path.with_extension("log.1"));
        }
    }
    // SAFETY: called at the start of main, before other threads exist.
    unsafe {
        if std::env::var_os("LOG_FILE").is_none() {
            std::env::set_var("LOG_FILE", path);
        }
        if std::env::var_os("LOG_LEVEL").is_none() {
            std::env::set_var("LOG_LEVEL", "INFO");
        }
    }
}

/// Remote paths are `Music\Artist\Album\01 x.flac`: → (`Music\Artist\Album`, `01 x.flac`).
pub fn remote_split(path: &str) -> (String, String) {
    let p = path.replace('\\', "/");
    match p.rsplit_once('/') {
        Some((d, b)) => (d.to_string(), b.to_string()),
        None => (String::new(), p),
    }
}

impl Default for Slsk {
    fn default() -> Self {
        Self::new()
    }
}

impl Slsk {
    pub fn new() -> Self {
        Slsk {
            inner: Arc::new(Mutex::new(Inner { cfg: None, client: None, state: State::Off, kicked: false, generation: 0, upnp: None })),
            last_search: Mutex::new(None),
            note: Mutex::new(String::new()),
        }
    }

    pub fn state(&self) -> State {
        self.inner.lock().state.clone()
    }

    pub fn ready(cfg: &Config) -> bool {
        !cfg.user.trim().is_empty() && !cfg.password.is_empty()
    }

    /// A line about something that happened by itself (e.g. signed back in), once.
    pub fn take_note(&self) -> String {
        std::mem::take(&mut *self.note.lock())
    }

    pub fn label(&self) -> String {
        let i = self.inner.lock();
        let user = i.cfg.as_ref().map(|c| c.user.clone()).unwrap_or_default();
        match &i.state {
            State::Online if i.kicked => format!("soulseek · {user} · signed out (name used elsewhere)"),
            State::Online => {
                let lost = i.client.as_ref().and_then(|c| c.session_loss()).is_some();
                if lost { format!("soulseek · {user} · reconnecting…") } else { format!("soulseek · {user} · online") }
            }
            State::Connecting => "soulseek · connecting…".into(),
            State::Error(e) => format!("soulseek · {e}").chars().take(90).collect(),
            State::Off => format!("soulseek · {}", if user.is_empty() { "no login set".into() } else { user }),
        }
    }

    /// New settings: reconnect if they matter to the connection.
    pub fn configure(&self, cfg: Config) {
        let mut i = self.inner.lock();
        let changed = i.cfg.as_ref() != Some(&cfg);
        let shares_only = i.cfg.as_ref().is_some_and(|old| Config { shares: cfg.shares.clone(), upload_slots: cfg.upload_slots, ..old.clone() } == cfg);
        i.cfg = Some(cfg.clone());
        if !changed {
            return;
        }
        if shares_only {
            if let Some(c) = i.client.clone() {
                c.set_upload_slots(cfg.upload_slots.max(1));
                let shares = cfg.shares.clone();
                std::thread::spawn(move || {
                    let _ = c.set_shared_directories(shares);
                });
            }
            return;
        }
        if i.client.is_some() || matches!(i.state, State::Connecting | State::Error(_)) {
            drop(i);
            self.restart();
        }
    }

    fn connect_thread(inner: Arc<Mutex<Inner>>, generation: u64) {
        let cfg = {
            let i = inner.lock();
            match &i.cfg {
                Some(c) => c.clone(),
                None => return,
            }
        };
        let mut client = Client::with_settings(ClientSettings {
            server_address: PeerAddress::new(cfg.server.clone(), cfg.server_port),
            enable_listen: true,
            listen_port: cfg.listen_port,
            shared_directories: cfg.shares.clone(),
            ..ClientSettings::new(cfg.user.clone(), cfg.password.clone())
        });
        let res = client.connect().map_err(|e| format!("can't connect: {e}")).and_then(|()| match client.login() {
            Ok(true) => Ok(()),
            Ok(false) => Err("login refused — wrong password, or the name is taken".to_string()),
            Err(e) => Err(format!("can't sign in: {e}")),
        });
        let mut i = inner.lock();
        if i.generation != generation {
            return; // stopped or restarted meanwhile
        }
        match res {
            Ok(()) => {
                client.set_upload_slots(cfg.upload_slots.max(1));
                i.client = Some(Arc::new(client));
                i.state = State::Online;
                i.kicked = false;
                if cfg.upnp {
                    i.upnp = Some(crate::netcheck::Upnp::start(cfg.listen_port));
                }
            }
            Err(e) => i.state = State::Error(e),
        }
    }

    /// Starts signing in (no-op when online or already trying).
    pub fn start(&self) {
        let mut i = self.inner.lock();
        if matches!(i.state, State::Online | State::Connecting) {
            return;
        }
        let Some(cfg) = &i.cfg else { return };
        if !Self::ready(cfg) {
            i.state = State::Error("set a soulseek username + password in settings → downloads".into());
            return;
        }
        if let Some(dir) = Some(cfg.download_dir.clone()) {
            let _ = std::fs::create_dir_all(dir);
        }
        i.state = State::Connecting;
        i.generation += 1;
        let (g, inner) = (i.generation, self.inner.clone());
        std::thread::Builder::new().name("nexus-slsk-connect".into()).spawn(move || Self::connect_thread(inner, g)).ok();
    }

    pub fn stop(&self) {
        let (client, upnp) = {
            let mut i = self.inner.lock();
            i.generation += 1;
            i.state = State::Off;
            (i.client.take(), i.upnp.take())
        };
        drop(upnp);
        // dropping the last handle disconnects (and frees the port) — off the caller's thread
        if let Some(c) = client {
            std::thread::spawn(move || drop(c));
        }
    }

    pub fn restart(&self) {
        self.stop();
        std::thread::sleep(Duration::from_millis(300));
        self.start();
    }

    /// The signed-in client, waiting for a sign-in in progress; signs in again after a dropped
    /// session or being displaced by another login.
    pub fn client(&self, wait: Duration) -> Result<Arc<Client>, String> {
        self.start();
        let t0 = Instant::now();
        loop {
            let (state, client) = {
                let i = self.inner.lock();
                (i.state.clone(), i.client.clone())
            };
            match state {
                State::Online => {
                    let c = client.ok_or("soulseek isn't running")?;
                    match c.session_loss() {
                        None => return Ok(c),
                        Some(loss) => {
                            let why = match loss {
                                SessionLoss::Displaced => "the same username was signed in somewhere else (another Nexus, Nicotine+, or someone using your login)",
                                _ => "the connection to Soulseek dropped",
                            };
                            *self.note.lock() = format!("signed back in to Soulseek — {why}");
                            self.inner.lock().kicked = matches!(loss, SessionLoss::Displaced);
                            self.restart();
                        }
                    }
                }
                State::Error(e) => return Err(e),
                State::Off => self.start(),
                State::Connecting => {}
            }
            if t0.elapsed() > wait {
                return Err("couldn't connect to soulseek".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn online_client(&self) -> Option<Arc<Client>> {
        let i = self.inner.lock();
        if i.state == State::Online { i.client.clone() } else { None }
    }

    /// Searches for `secs` seconds, reporting (users, files) as answers arrive.
    pub fn search(&self, text: &str, secs: u64, progress: &dyn Fn(usize, usize), cancel: &AtomicBool) -> Result<Vec<Response>, String> {
        let c = self.client(Duration::from_secs(25))?;
        {
            // back-to-back searches get ignored by the server
            let mut last = self.last_search.lock();
            if let Some(t) = *last {
                let gap = t.elapsed();
                if gap < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_secs(3) - gap);
                }
            }
            *last = Some(Instant::now());
        }
        let flag = Arc::new(AtomicBool::new(false));
        let (c2, q2, f2) = (c.clone(), text.to_string(), flag.clone());
        let worker = std::thread::spawn(move || c2.search_with_cancel(&q2, Duration::from_secs(secs), Some(f2)));
        while !worker.is_finished() {
            std::thread::sleep(Duration::from_millis(500));
            if cancel.load(Ordering::Relaxed) {
                flag.store(true, Ordering::Relaxed);
            }
            if let Some(rs) = c.try_get_search_results(text) {
                progress(rs.len(), rs.iter().map(|r| r.files.len()).sum());
            }
        }
        let res = worker.join().map_err(|_| "search crashed".to_string())?.map_err(|e| e.to_string())?;
        let _ = c.forget_search(text);
        log::info!("search {text:?}: {} users answered", res.len());
        Ok(res
            .into_iter()
            .map(|r| Response {
                username: r.username.clone(),
                free_slot: r.slots > 0,
                speed: u64::from(r.speed),
                queue: 0,
                files: r
                    .files
                    .iter()
                    .map(|f| RFile {
                        filename: f.name.clone(),
                        size: f.size,
                        bit_rate: f.attribs.get(&0).copied(),
                        length: f.attribs.get(&1).copied(),
                        sample_rate: f.attribs.get(&4).copied(),
                        bit_depth: f.attribs.get(&5).copied(),
                    })
                    .collect(),
            })
            .collect())
    }

    /// Where a file from `user`'s folder goes while downloading.
    pub fn folder_dir(&self, user: &str, remote: &str) -> PathBuf {
        let root = self.inner.lock().cfg.as_ref().map(|c| c.download_dir.clone()).unwrap_or_else(|| crate::paths::data_dir().join("soulseek").join("downloads"));
        let (dir, _) = remote_split(remote);
        let folder = dir.rsplit('/').next().unwrap_or("").to_string();
        root.join(safe_name(user, "user")).join(safe_name(&folder, "files"))
    }

    pub fn enqueue(&self, user: &str, files: &[(String, u64)]) -> Result<(), String> {
        let c = self.client(Duration::from_secs(25))?;
        for (name, size) in files {
            let dir = self.folder_dir(user, name);
            c.download(name.clone(), user.to_string(), *size, dir.to_string_lossy().to_string()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn downloads(&self) -> Vec<Transfer> {
        let Some(c) = self.online_client() else { return vec![] };
        c.get_all_downloads()
            .into_iter()
            .map(|d| {
                let (state, done, speed) = match &d.status {
                    DownloadStatus::Queued => (TState::Queued, 0, 0.0),
                    DownloadStatus::InProgress { bytes_downloaded, speed_bytes_per_sec, .. } => (TState::Downloading, *bytes_downloaded, *speed_bytes_per_sec),
                    DownloadStatus::Paused { bytes_downloaded, .. } => (TState::Paused, *bytes_downloaded, 0.0),
                    DownloadStatus::Completed => (TState::Done, d.size, 0.0),
                    DownloadStatus::Cancelled => (TState::Cancelled, 0, 0.0),
                    DownloadStatus::Failed(r) => (TState::Failed(r.clone().unwrap_or_default()), 0, 0.0),
                    DownloadStatus::TimedOut => (TState::Failed("timed out".into()), 0, 0.0),
                };
                let local = (state == TState::Done).then(|| PathBuf::from(&d.download_directory).join(remote_split(&d.filename).1));
                Transfer { username: d.username.clone(), filename: d.filename.clone(), size: d.size, done, speed, state, place: d.queue_position, local_path: local }
            })
            .collect()
    }

    /// Stops and forgets a download.
    pub fn cancel(&self, user: &str, filename: &str) {
        if let Some(c) = self.online_client() {
            let _ = c.cancel_download(user, filename);
            let _ = c.remove_download(user, filename);
        }
    }

    pub fn uploads(&self) -> Vec<Upload> {
        let Some(c) = self.online_client() else { return vec![] };
        c.uploads()
            .into_iter()
            .map(|u| Upload {
                username: u.username,
                filename: u.filename,
                size: u.size,
                sent: u.bytes_sent,
                state: match u.status {
                    soulseek_rs::UploadStatus::Queued(n) => format!("queued #{n}"),
                    soulseek_rs::UploadStatus::InProgress => "sending".into(),
                    soulseek_rs::UploadStatus::Completed => "sent".into(),
                    soulseek_rs::UploadStatus::Cancelled => "cancelled".into(),
                    soulseek_rs::UploadStatus::Failed(e) => format!("failed ({e})"),
                },
            })
            .collect()
    }

    pub fn shared_counts(&self) -> (u32, u32) {
        self.online_client().map(|c| c.shared_counts()).unwrap_or((0, 0))
    }

    pub fn excluded_phrases(&self) -> Vec<String> {
        self.online_client().map(|c| c.excluded_search_phrases()).unwrap_or_default()
    }

    pub fn upnp_status(&self) -> Option<String> {
        self.inner.lock().upnp.as_ref().map(|u| u.status())
    }

    pub fn listen_port(&self) -> u16 {
        self.inner.lock().cfg.as_ref().map(|c| c.listen_port).unwrap_or(2234)
    }
}

impl Drop for Slsk {
    fn drop(&mut self) {
        self.stop();
    }
}
