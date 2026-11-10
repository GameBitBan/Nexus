//! Download: search Soulseek (or the Internet Archive / Jamendo), queue albums, watch them land in
//! the library. Unfinished downloads carry on after a restart.

use crate::app::{Nexus, Page};
use crate::layout::region;
use crate::theme::{Theme, W, alpha};
use crate::widgets::{self, Pill, elided};
use crossbeam_channel::{Receiver, Sender, unbounded};
use egui::{Align, CursorIcon, Layout, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use nexus_core::dl::{self, Job, Pending, Row};
use nexus_core::slsk::{self, Response, Slsk, TState, Transfer};
use nexus_core::util::{fmt_time, human_size};
use nexus_core::webmusic::{self, WebDownloads, WebFile, WebRow};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub enum Msg {
    SearchDone { text: String, original: String, fallbacks: Vec<String>, res: Result<Vec<Response>, String> },
    Diagnosed(String),
    WebSearchDone(Result<Vec<WebRow>, String>),
    WebFiles { idx: usize, res: Result<(Vec<WebFile>, String, WebRow), String>, download: Option<Option<String>> },
    Enqueued { row: Box<Row>, files: Vec<(String, u64)>, res: Result<(), String> },
    AutoFound { artist: String, album: String, cover: String, quiet: bool, res: Result<Vec<Row>, String>, status: Option<Arc<Mutex<String>>> },
    Switch { key: String, auto: bool, res: Result<Vec<Row>, String> },
    Imported { n: usize, covers: Vec<(String, Vec<PathBuf>)> },
    WebImported { id: u64, n: usize, total: usize, failed: usize, name: String },
    Resumed,
    Vpn(String),
    Firewall(Option<bool>),
    FirewallFixed(Result<bool, String>),
}

pub struct DownloadState {
    pub slsk: Arc<Slsk>,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub pending: Pending,
    pub transfers: Vec<Transfer>,
    pub webdl: WebDownloads,
    query: String,
    status: String,
    busy: bool,
    progress: Arc<Mutex<(usize, usize)>>,
    raw: Option<Vec<Response>>,
    rows: Vec<Row>,
    web_rows: Vec<WebRow>,
    open: HashSet<usize>,
    next_poll: f64,
    resumed: bool,
    importing: bool,
    switching: bool,
    vpn: String,
    vpn_hidden: bool,
    firewall_bad: bool,
    checked_net: bool,
}

impl DownloadState {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = unbounded();
        let c2 = ctx.clone();
        DownloadState {
            slsk: Arc::new(Slsk::new()),
            tx,
            rx,
            pending: Pending::load(),
            transfers: vec![],
            webdl: WebDownloads::new(move || c2.request_repaint()),
            query: String::new(),
            status: String::new(),
            busy: false,
            progress: Arc::default(),
            raw: None,
            rows: vec![],
            web_rows: vec![],
            open: HashSet::new(),
            next_poll: 0.0,
            resumed: false,
            importing: false,
            switching: false,
            vpn: String::new(),
            vpn_hidden: false,
            firewall_bad: false,
            checked_net: false,
        }
    }
}

fn folder_of(t: &Transfer) -> String {
    slsk::remote_split(&t.filename).0.replace('/', "\\")
}

impl Nexus {
    /// The Soulseek settings as the client needs them.
    pub fn slsk_config(&self) -> slsk::Config {
        let cfg = &self.cfg;
        let mut shares: Vec<String> = if cfg.b("slsk_share_library") { cfg.strings("library_folders") } else { vec![] };
        shares.extend(cfg.s("slsk_share_extra").split(';').map(str::trim).filter(|s| !s.is_empty()).map(|s| nexus_core::paths::expand_user(s).to_string_lossy().to_string()));
        shares.retain(|s| std::path::Path::new(s).is_dir());
        shares.dedup();
        let dl = cfg.s("slsk_download_dir");
        slsk::Config {
            user: cfg.s("slsk_user").trim().to_string(),
            password: cfg.secret("slsk_password"),
            server: Some(cfg.s("slsk_server")).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "vps.slsknet.org".into()),
            server_port: cfg.f("slsk_server_port") as u16,
            listen_port: cfg.f("slsk_port").clamp(1024.0, 65000.0) as u16,
            upnp: cfg.b("slsk_upnp"),
            shares,
            upload_slots: cfg.f("slsk_upload_slots").max(1.0) as usize,
            download_dir: if dl.trim().is_empty() { nexus_core::paths::data_dir().join("soulseek").join("downloads") } else { nexus_core::paths::expand_user(dl.trim()) },
        }
    }

    fn slsk_ready(&self) -> bool {
        Slsk::ready(&self.slsk_config())
    }

    /// Startup / settings change: hand the settings over; after a restart carry on with unfinished downloads.
    pub fn downloads_apply(&mut self) {
        let cfg = self.slsk_config();
        let st = &mut self.download;
        st.slsk.configure(cfg.clone());
        if st.resumed || !Slsk::ready(&cfg) {
            return;
        }
        if st.pending.pending.is_empty() {
            st.resumed = true;
            return;
        }
        // re-queue what was still coming when Nexus closed (partial files continue where they stopped)
        let files: Vec<(String, String, u64)> = st.pending.pending.iter().map(|((u, f), (s, _))| (u.clone(), f.clone(), *s)).collect();
        let (s, tx, ctx) = (st.slsk.clone(), st.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut by_user: HashMap<String, Vec<(String, u64)>> = HashMap::new();
            for (u, f, size) in files {
                by_user.entry(u).or_default().push((f, size));
            }
            for (u, fs) in by_user {
                if let Err(e) = s.enqueue(&u, &fs) {
                    log::warn!("resume downloads from {u}: {e}");
                }
            }
            let _ = tx.send(Msg::Resumed);
            ctx.request_repaint();
        });
    }

    fn dl_source(&self) -> String {
        let s = self.cfg.s("dl_source");
        if s.is_empty() { "soulseek".into() } else { s }
    }

    pub fn dl_search(&mut self, text: Option<String>) {
        let src = self.dl_source();
        let st = &mut self.download;
        if st.busy {
            return;
        }
        if src != "soulseek" {
            let text = text.unwrap_or_else(|| st.query.clone()).trim().to_string();
            if text.is_empty() {
                return;
            }
            let lossless = self.cfg.b("dl_lossless_only");
            let jid = self.cfg.s("jamendo_client_id");
            if src == "jamendo" && jid.trim().is_empty() {
                st.status = "Jamendo needs a free app id: devportal.jamendo.com → create an app → paste its client id in settings → downloads".into();
                return;
            }
            st.busy = true;
            st.status = format!("searching {}…", if src == "archive" { "the Internet Archive" } else { "Jamendo" });
            let (tx, ctx) = (st.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let res = webmusic::search(&src, &text, lossless, jid.trim());
                let _ = tx.send(Msg::WebSearchDone(res));
                ctx.request_repaint();
            });
            return;
        }
        let text = dl::clean_query(&text.unwrap_or_else(|| st.query.clone()));
        if text.is_empty() {
            return;
        }
        let fallbacks = dl::broader_queries(&text);
        self.dl_search_slsk(text.clone(), text, fallbacks);
    }

    fn dl_search_slsk(&mut self, text: String, original: String, fallbacks: Vec<String>) {
        if !self.slsk_ready() {
            self.download.status = "set up your soulseek login first: settings → downloads".into();
            return;
        }
        let secs = self.cfg.f("dl_search_seconds").clamp(5.0, 60.0) as u64;
        let st = &mut self.download;
        st.busy = true;
        st.rows.clear();
        st.web_rows.clear();
        st.open.clear();
        st.status = format!("searching for “{text}”…");
        *st.progress.lock() = (0, 0);
        let (s, tx, ctx, prog) = (st.slsk.clone(), st.tx.clone(), self.ctx.clone(), st.progress.clone());
        std::thread::spawn(move || {
            let c2 = ctx.clone();
            let res = s.search(
                &text,
                secs,
                &|u, f| {
                    *prog.lock() = (u, f);
                    c2.request_repaint();
                },
                &AtomicBool::new(false),
            );
            let _ = tx.send(Msg::SearchDone { text, original, fallbacks, res });
            ctx.request_repaint();
        });
    }

    /// Queue a row (all its audio + images, or just `files`).
    pub fn dl_download(&mut self, row: Row, files: Option<Vec<slsk::RFile>>, job: Option<Job>) {
        let files = files.unwrap_or_else(|| row.audio.iter().chain(row.images.iter()).cloned().collect());
        let key = dl::job_key(&row.user, &row.dir);
        let st = &mut self.download;
        let mut job = job.unwrap_or_else(|| Job { name: row.name.clone(), alts: alternatives(&row, &st.rows), ..Default::default() });
        job.user = row.user.clone();
        job.dir = row.dir.clone();
        job.started = dl::now();
        job.auto = files.len() > 1;
        if !job.tried.contains(&row.user) {
            job.tried.push(row.user.clone());
        }
        let tried = job.tried.clone();
        job.alts.retain(|a| !tried.contains(&a.user));
        st.jobs_insert(key, job);
        st.status = format!("queueing {} files from {}…", files.len(), row.user);
        let list: Vec<(String, u64)> = files.iter().map(|f| (f.filename.clone(), f.size)).collect();
        let (s, tx, ctx) = (st.slsk.clone(), st.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let res = s.enqueue(&row.user, &list);
            let _ = tx.send(Msg::Enqueued { row: Box::new(row), files: list, res });
            ctx.request_repaint();
        });
    }

    /// Find "artist album" on Soulseek, pick the best copy, queue it (with backups). Used by
    /// Discover and Import too. `status` gets short progress lines when given.
    pub fn auto_download(&mut self, artist: &str, album: &str, cover_url: &str, quiet: bool, status: Option<Arc<Mutex<String>>>) {
        if !self.slsk_ready() {
            if let Some(s) = &status {
                *s.lock() = "set up Soulseek first (settings → downloads)".into();
            }
            self.toast_action("set a Soulseek login in settings → downloads first", "open", |a| a.go(Page::Settings(Some("downloads".into())), true), 8.0);
            return;
        }
        let lossless = self.cfg.b("dl_lossless_only");
        let secs = self.cfg.f("dl_search_seconds").clamp(5.0, 60.0) as u64;
        let mut queries = vec![dl::clean_query(&format!("{artist} {album}"))];
        queries.extend(dl::broader_queries(&format!("{artist} {album}")));
        let want: Vec<String> = dl::tokens(album).into_iter().filter(|t| !dl::NOISE_WORDS.contains(&t.as_str())).collect();
        if want.join(" ").len() >= 6 {
            queries.push(dl::clean_query(album));
        }
        let mut seen = HashSet::new();
        queries.retain(|q| !q.is_empty() && seen.insert(q.clone()));
        queries.truncate(3);
        if let Some(s) = &status {
            *s.lock() = "searching Soulseek…".into();
        }
        let st = &self.download;
        let (s, tx, ctx) = (st.slsk.clone(), st.tx.clone(), self.ctx.clone());
        let (artist, album, cover) = (artist.to_string(), album.to_string(), cover_url.to_string());
        std::thread::spawn(move || {
            let mut out: Result<Vec<Row>, String> = Ok(vec![]);
            for q in &queries {
                match s.search(q, secs, &|_, _| {}, &AtomicBool::new(false)) {
                    Ok(res) => {
                        let rows = dl::group_results(&res, lossless);
                        let mut good: Vec<(bool, Row)> = rows
                            .into_iter()
                            .filter_map(|r| {
                                let (ok, has_artist) = dl::fits(&r, &artist, &album);
                                ok.then_some((has_artist, r))
                            })
                            .collect();
                        if !good.is_empty() {
                            good.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal)));
                            out = Ok(good.into_iter().map(|(_, r)| r).collect());
                            break;
                        }
                    }
                    Err(e) => {
                        out = Err(e);
                        break;
                    }
                }
            }
            let _ = tx.send(Msg::AutoFound { artist, album, cover, quiet, res: out, status });
            ctx.request_repaint();
        });
    }

    fn dl_switch(&mut self, key: &str, auto: bool) {
        let st = &mut self.download;
        if st.switching {
            return;
        }
        let (user, folder) = dl::split_key(key);
        let job = st.pending.jobs.entry(key.to_string()).or_insert_with(|| Job {
            name: { let n = slsk::remote_split(&folder).1; if n.is_empty() { folder.clone() } else { n } },
            tried: vec![user.clone()],
            user: user.clone(),
            dir: folder.clone(),
            ..Default::default()
        });
        let job = job.clone();
        st.switching = true;
        st.status = format!("looking for another user with “{}”…", job.name);
        let fs = folder_files(&st.transfers, key);
        let n_audio = fs.iter().filter(|t| !dl::is_image_name(&t.filename)).count().max(1);
        let lossless = self.cfg.b("dl_lossless_only");
        let secs = self.cfg.f("dl_search_seconds").clamp(5.0, 60.0) as u64;
        let (s, tx, ctx, key) = (st.slsk.clone(), st.tx.clone(), self.ctx.clone(), key.to_string());
        std::thread::spawn(move || {
            let alts: Vec<Row> = job.alts.iter().filter(|a| !job.tried.contains(&a.user)).cloned().collect();
            let res = if alts.is_empty() {
                // nobody left from the old search: search again
                let mut words: Vec<String> = dl::tokens(&job.name).into_iter().collect();
                let low = job.name.to_lowercase();
                words.sort_by_key(|w| low.find(w.as_str()).unwrap_or(usize::MAX));
                let q: String = words.join(" ").chars().take(80).collect();
                let q = if q.is_empty() { job.name.clone() } else { q };
                s.search(&q, secs, &|_, _| {}, &AtomicBool::new(false))
                    .map(|res| dl::group_results(&res, lossless).into_iter().filter(|r| !job.tried.contains(&r.user) && dl::similar(&job.name, n_audio, r)).collect())
            } else {
                Ok(alts)
            };
            let _ = tx.send(Msg::Switch { key, auto, res });
            ctx.request_repaint();
        });
    }

    fn dl_retry_failed(&mut self, key: &str) {
        let (user, _) = dl::split_key(key);
        let st = &mut self.download;
        let failed: Vec<(String, u64)> = folder_files(&st.transfers, key).iter().filter(|t| t.state.failed()).map(|t| (t.filename.clone(), t.size)).collect();
        if failed.is_empty() {
            self.toast("nothing failed in that one");
            return;
        }
        if let Some(j) = st.pending.jobs.get_mut(key) {
            j.started = dl::now();
        }
        st.status = format!("retrying {} files from {user}", failed.len());
        let s = st.slsk.clone();
        std::thread::spawn(move || {
            for (f, _) in &failed {
                s.cancel(&user, f);
            }
            let _ = s.enqueue(&user, &failed);
        });
    }

    fn dl_cancel(&mut self, key: &str) {
        let (user, _) = dl::split_key(key);
        let st = &mut self.download;
        let fs: Vec<String> = folder_files(&st.transfers, key).iter().filter(|t| t.state != TState::Done).map(|t| t.filename.clone()).collect();
        for f in &fs {
            st.pending.pending.remove(&(user.clone(), f.clone()));
        }
        st.pending.jobs.remove(key);
        st.pending.save();
        let s = st.slsk.clone();
        std::thread::spawn(move || {
            for f in fs {
                s.cancel(&user, &f);
            }
        });
        st.next_poll = 0.0;
    }

    fn dl_clear_finished(&mut self) {
        let st = &mut self.download;
        st.webdl.clear_finished();
        let done: Vec<(String, String)> = st.transfers.iter().filter(|t| t.state.finished()).map(|t| (t.username.clone(), t.filename.clone())).collect();
        let s = st.slsk.clone();
        std::thread::spawn(move || {
            for (u, f) in done {
                s.cancel(&u, &f);
            }
        });
        st.next_poll = 0.0;
    }

    /// Every frame (cheap): handle finished background work; every 2 s look at the transfers.
    pub fn downloads_tick(&mut self, now: f64) {
        let msgs: Vec<Msg> = self.download.rx.try_iter().collect();
        for m in msgs {
            self.dl_handle(m);
        }
        for job in self.download.webdl.take_finished() {
            self.dl_import_web(job);
        }
        let st = &self.download;
        let active = !st.pending.pending.is_empty() || st.transfers.iter().any(|t| !t.state.finished()) || st.webdl.active() || self.page == Page::Download;
        if active && now >= st.next_poll && st.slsk.online_client().is_some() {
            self.download.next_poll = now + 2.0;
            self.dl_poll();
        }
        if active {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        }
    }

    fn dl_poll(&mut self) {
        let st = &mut self.download;
        st.transfers = st.slsk.downloads();
        let online = st.slsk.online_client().is_some();
        // forget pending files that failed, were cancelled or vanished (only once resumed)
        if online && st.resumed {
            let live: HashMap<(String, String), TState> = st.transfers.iter().map(|t| ((t.username.clone(), t.filename.clone()), t.state.clone())).collect();
            let gone: Vec<(String, String)> = st.pending.pending.keys().filter(|k| live.get(*k).is_none_or(TState::failed)).cloned().collect();
            if !gone.is_empty() {
                for k in gone {
                    st.pending.pending.remove(&k);
                }
                st.pending.save();
            }
            let done_jobs: Vec<String> = st.pending.jobs.keys().filter(|k| {
                let fs = folder_files(&st.transfers, k);
                !fs.is_empty() && fs.iter().all(|t| t.state == TState::Done)
            }).cloned().collect();
            for k in done_jobs {
                st.pending.jobs.remove(&k);
            }
        }
        self.dl_import_finished();
        self.dl_check_stalls();
    }

    fn dl_import_finished(&mut self) {
        let st = &mut self.download;
        if st.importing {
            return;
        }
        let own = self.slsk_config().download_dir;
        let st = &mut self.download;
        let todo: Vec<(String, String, PathBuf)> = st
            .transfers
            .iter()
            .filter(|t| t.state == TState::Done)
            .filter_map(|t| {
                let lp = t.local_path.clone()?;
                let k = (t.username.clone(), t.filename.clone());
                (lp.exists() && !st.pending.imported.contains(&k) && (st.pending.pending.contains_key(&k) || lp.starts_with(&own))).then_some((t.username.clone(), t.filename.clone(), lp))
            })
            .collect();
        if todo.is_empty() {
            return;
        }
        for (u, f, _) in &todo {
            st.pending.imported.insert((u.clone(), f.clone()));
            st.pending.pending.remove(&(u.clone(), f.clone()));
        }
        st.pending.save();
        let groups = dl::finished_groups(&todo);
        let covers: HashMap<String, String> = groups.keys().filter_map(|k| st.pending.jobs.get(k).map(|j| (k.clone(), j.cover_url.clone()))).filter(|(_, c)| !c.is_empty()).collect();
        let root = nexus_core::importer::root(&self.cfg.s("import_root"), &self.cfg.strings("library_folders"));
        let st = &mut self.download;
        st.importing = true;
        let (tx, ctx) = (st.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut n = 0;
            let mut cov = Vec::new();
            for (key, audio) in groups {
                let got = nexus_core::importer::import_files(&audio, &root, true);
                n += got.files.len();
                if let Some(c) = covers.get(&key) {
                    cov.push((c.clone(), got.files.iter().map(PathBuf::from).collect()));
                }
            }
            let _ = tx.send(Msg::Imported { n, covers: cov });
            ctx.request_repaint();
        });
    }

    fn dl_import_web(&mut self, job: webmusic::WebJob) {
        let root = nexus_core::importer::root(&self.cfg.s("import_root"), &self.cfg.strings("library_folders"));
        let (tx, ctx) = (self.download.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            // cover.jpg next to the files comes along on import
            let got = nexus_core::importer::import_files(&job.paths, &root, true);
            if let Some(folder) = job.paths.first().and_then(|p| p.parent()) {
                let _ = std::fs::remove_dir_all(folder);
            }
            let _ = tx.send(Msg::WebImported { id: job.id, n: got.files.len(), total: job.files.len(), failed: job.failed, name: job.row.name.clone() });
            ctx.request_repaint();
        });
    }

    fn dl_check_stalls(&mut self) {
        if !self.cfg.b("dl_auto_switch") || self.download.switching {
            return;
        }
        let mins = self.cfg.f("dl_stall_minutes").max(1.0);
        let st = &self.download;
        let now = dl::now();
        let stuck = st.pending.jobs.iter().find(|(k, job)| {
            let fs = folder_files(&st.transfers, k);
            if fs.is_empty() || !job.auto {
                return false;
            }
            let got: u64 = fs.iter().map(|t| t.done).sum();
            let all_failed = fs.iter().all(|t| t.state.failed());
            let stalled = got == 0 && fs.iter().all(|t| matches!(t.state, TState::Queued) || t.state.failed());
            all_failed || (stalled && now - job.started > mins * 60.0)
        });
        if let Some((k, _)) = stuck {
            let k = k.clone();
            self.dl_switch(&k, true);
        }
    }

    fn dl_handle(&mut self, m: Msg) {
        match m {
            Msg::SearchDone { text, original, fallbacks, res } => {
                let st = &mut self.download;
                st.busy = false;
                let note = st.slsk.take_note();
                if !note.is_empty() {
                    self.toast(note);
                }
                let st = &mut self.download;
                match res {
                    Err(e) => st.status = e,
                    Ok(res) if res.is_empty() && !fallbacks.is_empty() => {
                        let next = fallbacks[0].clone();
                        st.status = format!("nothing for “{text}” — trying “{next}”…");
                        self.dl_search_slsk(next, original, fallbacks[1..].to_vec());
                    }
                    Ok(res) => {
                        let lossless = self.cfg.b("dl_lossless_only");
                        let st = &mut self.download;
                        st.rows = dl::group_results(&res, lossless);
                        st.rows.truncate(120);
                        let n = st.rows.len();
                        st.status = format!("{n} folders found{}{}", if lossless { " (lossless)" } else { "" }, if n == 0 { " — try fewer words, or untick lossless only" } else { "" });
                        if !res.is_empty() && text != original {
                            st.status += &format!(" · nothing matched “{original}”, these are for “{text}”");
                        }
                        if res.is_empty() {
                            st.status = format!("0 results for “{original}” — checking why…");
                            let (s, tx, ctx) = (st.slsk.clone(), st.tx.clone(), self.ctx.clone());
                            std::thread::spawn(move || {
                                let why = nexus_core::netcheck::diagnose(&original, s.listen_port(), &s.excluded_phrases(), s.upnp_status());
                                let _ = tx.send(Msg::Diagnosed(why));
                                ctx.request_repaint();
                            });
                        } else if st.rows.is_empty() {
                            st.status = format!("{} users answered, but none had lossless folders — untick lossless only", res.len());
                        }
                        st.raw = Some(res);
                    }
                }
            }
            Msg::Diagnosed(why) => self.download.status = format!("0 results — {why}"),
            Msg::WebSearchDone(res) => {
                let lossless = self.cfg.b("dl_lossless_only");
                let st = &mut self.download;
                st.busy = false;
                match res {
                    Err(e) => st.status = format!("couldn't search: {e}").chars().take(200).collect(),
                    Ok(rows) => {
                        st.web_rows = rows.into_iter().take(80).collect();
                        st.rows.clear();
                        st.open.clear();
                        let n = st.web_rows.len();
                        st.status = format!("{n} found{}{}", if lossless { " (lossless)" } else { "" }, if n == 0 { " — try fewer words, or untick lossless only" } else { "" });
                    }
                }
            }
            Msg::WebFiles { idx, res, download } => match res {
                Err(e) => self.download.status = format!("couldn't open it: {e}").chars().take(200).collect(),
                Ok((files, fmt, row)) => {
                    let st = &mut self.download;
                    if let Some(r) = st.web_rows.get_mut(idx) {
                        *r = row;
                        if !fmt.is_empty() {
                            r.fmt = fmt;
                        }
                        r.files = Some(files);
                    }
                    if let Some(only) = download {
                        self.dl_web_download(idx, only);
                    }
                }
            },
            Msg::Enqueued { row, files, res } => {
                let st = &mut self.download;
                match res {
                    Err(e) => st.status = e,
                    Ok(()) => {
                        for (f, size) in files {
                            st.pending.pending.insert((row.user.clone(), f), (size, row.name.clone()));
                        }
                        st.pending.save();
                        st.status = format!("queued “{}” from {}", row.name, row.user);
                        st.next_poll = 0.0;
                    }
                }
            }
            Msg::AutoFound { artist, album, cover, quiet, res, status } => {
                let lossless = self.cfg.b("dl_lossless_only");
                let set = |s: &str| {
                    if let Some(st) = &status {
                        *st.lock() = s.to_string();
                    }
                };
                match res {
                    Err(e) => set(&format!("couldn't search: {e}")),
                    Ok(c) if c.is_empty() => {
                        let msg = if lossless { "no lossless copy on Soulseek right now" } else { "not on Soulseek right now" };
                        set(msg);
                        if !quiet {
                            let q = format!("{artist} {album}");
                            self.toast_action(
                                format!("“{album}”: {msg}"),
                                "search myself",
                                move |a| {
                                    a.go(Page::Download, true);
                                    a.download.query = q.clone();
                                    a.dl_search(Some(q));
                                },
                                8.0,
                            );
                        }
                    }
                    Ok(mut c) => {
                        let best = c.remove(0);
                        set(&format!("downloading from {} · {}", best.user, best.fmt));
                        let fmt = best.fmt.clone();
                        let job = Job { name: format!("{artist} - {album}"), alts: c.into_iter().take(6).collect(), cover_url: cover, ..Default::default() };
                        self.dl_download(best, None, Some(job));
                        if !quiet {
                            self.toast_action(format!("downloading “{album}” ({fmt}) — it lands in your library when done"), "show", |a| a.go(Page::Download, true), 6.0);
                        }
                    }
                }
            }
            Msg::Switch { key, auto, res } => {
                self.download.switching = false;
                let alts = match res {
                    Ok(a) if !a.is_empty() => a,
                    other => {
                        let name = self.download.pending.jobs.get(&key).map(|j| j.name.clone()).unwrap_or_default();
                        if let Some(j) = self.download.pending.jobs.get_mut(&key) {
                            j.auto = false; // don't keep hammering
                        }
                        let msg = match other {
                            Err(e) => e,
                            _ => format!("no one else has “{name}” right now"),
                        };
                        self.download.status = msg.clone();
                        if !auto {
                            self.toast(msg);
                        }
                        return;
                    }
                };
                let st = &mut self.download;
                let Some(mut job) = st.pending.jobs.remove(&key) else { return };
                let fs = folder_files(&st.transfers, &key);
                let done_stems: HashSet<String> = fs.iter().filter(|t| t.state == TState::Done).map(|t| dl::stem(&t.filename)).collect();
                // drop the stuck ones from the old user
                let old = job.user.clone();
                let leftover: Vec<String> = fs.iter().filter(|t| t.state != TState::Done).map(|t| t.filename.clone()).collect();
                for f in &leftover {
                    st.pending.pending.remove(&(old.clone(), f.clone()));
                }
                let s = st.slsk.clone();
                let o2 = old.clone();
                std::thread::spawn(move || {
                    for f in leftover {
                        s.cancel(&o2, &f);
                    }
                });
                let new = alts[0].clone();
                job.alts = alts[1..].to_vec();
                let want = dl::still_needed(&new, &done_stems);
                let name = job.name.clone();
                let nu = new.user.clone();
                self.dl_download(new, if want.is_empty() { None } else { Some(want) }, Some(job));
                self.toast(if auto { format!("“{name}” wasn't starting — switched from {old} to {nu}") } else { format!("switched “{name}” from {old} to {nu}") });
            }
            Msg::Imported { n, covers } => {
                self.download.importing = false;
                if n > 0 {
                    self.download.status = format!("imported {n} files into your library");
                    for (url, files) in covers {
                        add_cover(url, files);
                    }
                    self.schedule_rescan();
                }
            }
            Msg::WebImported { id, n, total, failed, name } => {
                let state = if n > 0 && failed == 0 {
                    "in your library".to_string()
                } else if n > 0 {
                    format!("some failed — {n}/{total} in your library")
                } else {
                    "failed (couldn't import)".into()
                };
                self.download.webdl.set_state(id, state);
                if n > 0 {
                    self.toast(format!("“{name}” is in your library ({n} songs)"));
                    self.schedule_rescan();
                }
            }
            Msg::Resumed => {
                self.download.resumed = true;
                self.download.next_poll = 0.0;
            }
            Msg::Vpn(v) => self.download.vpn = v,
            Msg::Firewall(ok) => self.download.firewall_bad = ok == Some(false),
            Msg::FirewallFixed(res) => match res {
                Ok(true) => {
                    self.download.firewall_bad = false;
                    self.toast("Windows Firewall is set up — reconnecting to Soulseek");
                    self.download.slsk.restart();
                }
                other => {
                    self.download.firewall_bad = true;
                    let e = match other {
                        Err(e) => format!(": {e}"),
                        _ => String::new(),
                    };
                    self.toast_ms(format!("the firewall rule didn't get added (prompt cancelled, or no admin rights on this account){e}"), 15.0);
                }
            },
        }
    }

    fn dl_web_files(&mut self, idx: usize, download: Option<Option<String>>) {
        let st = &mut self.download;
        let Some(row) = st.web_rows.get(idx).cloned() else { return };
        let lossless = self.cfg.b("dl_lossless_only");
        let jid = self.cfg.s("jamendo_client_id");
        let (tx, ctx) = (st.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut row = row;
            let res = webmusic::files(&mut row, lossless, jid.trim()).map(|(f, fmt)| (f, fmt, row));
            let _ = tx.send(Msg::WebFiles { idx, res, download });
            ctx.request_repaint();
        });
    }

    /// `only`: one file's url, or the whole album.
    fn dl_web_download(&mut self, idx: usize, only: Option<String>) {
        let st = &mut self.download;
        let Some(r) = st.web_rows.get(idx).cloned() else { return };
        let Some(files) = &r.files else {
            self.dl_web_files(idx, Some(only));
            return;
        };
        let files: Vec<WebFile> = files.iter().filter(|f| only.as_ref().is_none_or(|u| &f.url == u)).cloned().collect();
        if files.is_empty() {
            st.status = format!("“{}” has no downloadable audio", r.name);
            return;
        }
        let name = match &only {
            Some(_) => format!("{} — {}", r.name, if files[0].title.is_empty() { &files[0].name } else { &files[0].title }),
            None => r.name.clone(),
        };
        let src = if r.source == "archive" { "the Internet Archive" } else { "Jamendo" };
        st.webdl.add(WebRow { name: name.clone(), ..r }, files);
        self.toast(format!("downloading “{name}” from {src} — it lands in your library when done"));
    }

    fn schedule_rescan(&mut self) {
        if let Some(im) = &mut self.importing {
            im.request_rescan(self.ctx.input(|i| i.time));
        }
    }
}

/// Albums from Discover: if the download came without a cover, save the one we showed you.
fn add_cover(url: String, files: Vec<PathBuf>) {
    let dirs: HashSet<PathBuf> = files.iter().filter_map(|f| f.parent().map(PathBuf::from)).collect();
    std::thread::spawn(move || {
        let need: Vec<&PathBuf> = dirs.iter().filter(|d| nexus_core::scanner::folder_cover(d).is_none()).collect();
        if need.is_empty() {
            return;
        }
        if let Ok(data) = nexus_core::net::get_bytes(&url, 20, 15_000_000) {
            for d in need {
                let _ = std::fs::write(d.join("cover.jpg"), &data);
            }
        }
    });
}

fn alternatives(row: &Row, rows: &[Row]) -> Vec<Row> {
    rows.iter().filter(|r| r.user != row.user && dl::similar(&row.name, row.audio.len(), r)).take(6).cloned().collect()
}

fn folder_files<'a>(transfers: &'a [Transfer], key: &str) -> Vec<&'a Transfer> {
    let (user, folder) = dl::split_key(key);
    transfers.iter().filter(|t| t.username == user && folder_of(t) == folder).collect()
}

impl DownloadState {
    fn jobs_insert(&mut self, key: String, job: Job) {
        self.pending.jobs.insert(key, job);
    }

    fn stall_left(&self, job: Option<&Job>, auto_switch: bool, mins: f64) -> Option<i64> {
        let job = job?;
        if !auto_switch || !job.auto {
            return None;
        }
        Some(((job.started + mins * 60.0 - dl::now()) / 60.0).ceil().max(0.0) as i64)
    }
}

// ------------------------------------------------------------------ drawing

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let c = t.c;
    let src = app.dl_source();
    if !app.download.checked_net {
        app.download.checked_net = true;
        app.download.slsk.configure(app.slsk_config());
        if app.slsk_ready() && src == "soulseek" {
            app.download.slsk.start();
        }
        let (tx, ctx) = (app.download.tx.clone(), app.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Vpn(nexus_core::netcheck::vpn_active()));
            let _ = tx.send(Msg::Firewall(nexus_core::netcheck::firewall_ok()));
            ctx.request_repaint();
        });
    }
    if app.download.busy && src == "soulseek" {
        let (u, f) = *app.download.progress.lock();
        let q = app.download.status.split('“').nth(1).and_then(|s| s.split('”').next()).unwrap_or("").to_string();
        if !q.is_empty() && app.download.status.starts_with("searching") {
            app.download.status = format!("searching for “{q}”… {u} users, {f} files");
        }
    }
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 12.0));
    let mut act: Option<Box<dyn FnOnce(&mut Nexus)>> = None;
    region(ui, inner, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        // header
        ui.horizontal(|ui| {
            widgets::label(ui, &t.l("Download"), t.font(W::Display, 27.0), c.text, Sense::hover());
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                if widgets::pill(ui, &t, "connection", Pill::Normal).clicked() {
                    act = Some(Box::new(|a: &mut Nexus| a.go(Page::Settings(Some("downloads".into())), true)));
                }
                if src == "soulseek" {
                    let label = if app.slsk_ready() { app.download.slsk.label() } else { t.l("set a soulseek login in settings → downloads") };
                    widgets::small_dim(ui, &t, &label);
                }
            });
        });
        // search row
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let names = [("soulseek", "Soulseek"), ("archive", "Internet Archive"), ("jamendo", "Jamendo")];
            let cur = names.iter().find(|(k, _)| *k == src).map(|(_, n)| *n).unwrap_or("Soulseek");
            let mut pick = None;
            egui::ComboBox::from_id_salt("dl-source").selected_text(cur).width(150.0).show_ui(ui, |ui| {
                for (k, n) in names {
                    if ui.selectable_label(k == src, n).clicked() {
                        pick = Some(k.to_string());
                    }
                }
            });
            if let Some(k) = pick {
                act = Some(Box::new(move |a: &mut Nexus| {
                    a.cfg.set("dl_source", serde_json::json!(k));
                    a.cfg.save();
                    let st = &mut a.download;
                    st.rows.clear();
                    st.web_rows.clear();
                    st.open.clear();
                    st.status = match k.as_str() {
                        "archive" => "free and legal: public-domain recordings, netlabels and the Live Music Archive. click a result to see its tracks.".into(),
                        "jamendo" => "free and legal Creative Commons music from independent artists. check each album's licence before using it in videos.".into(),
                        _ => String::new(),
                    };
                }));
            }
            let hint = match src.as_str() {
                "archive" => "Search the Internet Archive — artist, album, or a band + year for live shows",
                "jamendo" => "Search Jamendo — artist, album, genre",
                _ => "Search Soulseek — artist, album or song",
            };
            let w = (ui.available_width() - 260.0).clamp(160.0, 620.0);
            let resp = widgets::search_box(ui, &t, &mut app.download.query, &t.l(hint), w, egui::Id::unique("dl-query"));
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                act = Some(Box::new(|a: &mut Nexus| a.dl_search(None)));
            }
            let mut lossless = app.cfg.b("dl_lossless_only");
            if widgets::switch(ui, &t, &mut lossless).changed() {
                act = Some(Box::new(move |a: &mut Nexus| {
                    a.cfg.set("dl_lossless_only", serde_json::json!(lossless));
                    a.cfg.save();
                    if a.dl_source() != "soulseek" {
                        if !a.download.query.trim().is_empty() {
                            a.dl_search(None);
                        }
                    } else if let Some(raw) = &a.download.raw {
                        a.download.rows = dl::group_results(raw, lossless);
                        a.download.open.clear();
                    }
                }));
            }
            widgets::label(ui, &t.l("lossless only"), t.body(), c.dim, Sense::hover());
            if ui.add_enabled_ui(!app.download.busy, |ui| widgets::pill(ui, &t, "search", Pill::Primary)).inner.clicked() {
                act = Some(Box::new(|a: &mut Nexus| a.dl_search(None)));
            }
        });
        if !app.download.status.is_empty() {
            let g = ui.fonts_mut(|f| f.layout(t.l(&app.download.status), t.body(), c.dim, inner.width()));
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, c.dim);
        }
        // results get what's left above the downloads list
        let dl_h = 250.0f32.min(ui.available_height() * 0.45);
        let res_h = (ui.available_height() - dl_h - 20.0).max(80.0);
        let (res_rect, _) = ui.allocate_exact_size(vec2(inner.width(), res_h), Sense::hover());
        region(ui, res_rect, Layout::top_down(Align::Min), |ui| {
            if let Some(a) = if src == "soulseek" { results(app, ui, &t) } else { web_results(app, ui, &t, &src) } {
                act = Some(a);
            }
        });
        if let Some(a) = downloads_list(app, ui, &t) {
            act = Some(a);
        }
    });
    if let Some(f) = act {
        f(app);
    }
}

type ActFn = Box<dyn FnOnce(&mut Nexus)>;

const COLW: [f32; 6] = [130.0, 64.0, 84.0, 140.0, 90.0, 110.0];

fn header_row(ui: &mut Ui, t: &Theme, labels: [&str; 7]) {
    let c = t.c;
    let w = ui.available_width() - 12.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(Rect::from_min_size(pos2(r.left(), r.bottom() - 1.0), vec2(w, 1.0)), 0.0, c.border);
    let font = t.font(W::Semi, -1.5);
    let name_w = w - 26.0 - COLW.iter().sum::<f32>();
    let mut x = r.left() + 26.0;
    for (i, l) in labels.iter().enumerate() {
        let cw = if i == 0 { name_w } else { COLW[i - 1] };
        p.text(pos2(x + 4.0, r.center().y), egui::Align2::LEFT_CENTER, t.header(&t.l(l)), font.clone(), c.dim);
        x += cw;
    }
}

/// One results line: a dot, columns, a download button. Returns (row response, download clicked).
fn result_line(ui: &mut Ui, t: &Theme, dot: &str, cols: [String; 7], open: bool, id: egui::Id, bold: bool) -> (egui::Response, bool) {
    let c = t.c;
    let w = ui.available_width() - 12.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    let p = ui.painter().clone();
    if resp.hovered() || open {
        p.rect_filled(r, 0.0, alpha(c.select, if open { 1.0 } else { 0.7 }));
    }
    p.text(pos2(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, dot, t.font(W::Regular, -2.0), c.dim);
    let name_w = w - 26.0 - COLW.iter().sum::<f32>();
    let mut x = r.left() + 26.0;
    for (i, txt) in cols.iter().enumerate().take(6) {
        let cw = if i == 0 { name_w } else { COLW[i - 1] };
        let col = if i == 0 { c.text } else { c.dim };
        let font = if i == 0 && bold { t.font(W::Semi, 0.0) } else { t.body() };
        let g = elided(ui, txt, font, col, cw - 10.0);
        p.galley(pos2(x + 4.0, r.center().y - g.size().y / 2.0), g, col);
        x += cw;
    }
    let mut clicked = false;
    if !cols[6].is_empty() {
        let br = Rect::from_min_size(pos2(r.right() - COLW[5] + 6.0, r.top() + 3.0), vec2(COLW[5] - 10.0, 26.0));
        let b = ui.interact(br, id.with("btn"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let hover = b.hovered();
        p.rect(br, 13.0, if hover { c.text } else { c.panel }, Stroke::new(1.0, if hover { c.text } else { c.border }), StrokeKind::Inside);
        p.text(br.center(), egui::Align2::CENTER_CENTER, t.b(&cols[6]), t.font(W::Semi, -1.0), if hover { c.bg } else { c.text });
        clicked = b.clicked();
    }
    (resp.on_hover_cursor(CursorIcon::PointingHand), clicked)
}

fn results(app: &mut Nexus, ui: &mut Ui, t: &Theme) -> Option<ActFn> {
    let mut act: Option<ActFn> = None;
    if app.download.rows.is_empty() {
        return None;
    }
    header_row(ui, t, ["album / folder", "format", "tracks", "size", "user", "speed", ""]);
    egui::ScrollArea::vertical().id_salt("dl-results").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let rows = app.download.rows.clone();
        for (i, r) in rows.iter().enumerate() {
            let open = app.download.open.contains(&i);
            let cols = [
                r.name.clone(),
                r.fmt.clone(),
                r.audio.len().to_string(),
                human_size(r.size),
                r.user.clone(),
                if r.speed > 0 { format!("{:.1} MB/s", r.speed as f64 / 1_000_000.0) } else { "—".into() },
                "download".into(),
            ];
            let (resp, dl_click) = result_line(ui, t, if r.free { "●" } else { "○" }, cols, open, egui::Id::unique(("dlrow", i)), true);
            let resp = resp.on_hover_text(format!("{}\n{}", r.dir, if r.free { "free upload slot".to_string() } else { format!("queued: {} ahead", r.queue) }));
            if dl_click {
                let row = r.clone();
                act = Some(Box::new(move |a: &mut Nexus| a.dl_download(row, None, None)));
            } else if resp.double_clicked() {
                let row = r.clone();
                act = Some(Box::new(move |a: &mut Nexus| a.dl_download(row, None, None)));
            } else if resp.clicked() {
                act = Some(Box::new(move |a: &mut Nexus| {
                    if !a.download.open.remove(&i) {
                        a.download.open.insert(i);
                    }
                }));
            }
            if open {
                let mut files: Vec<&slsk::RFile> = r.audio.iter().collect();
                files.sort_by(|a, b| a.filename.cmp(&b.filename));
                for f in files {
                    let base = slsk::remote_split(&f.filename).1;
                    let extra = match (f.bit_depth, f.sample_rate, f.bit_rate) {
                        (Some(bd), Some(sr), _) if bd > 0 => format!("{bd}/{}", f64::from(sr) / 1000.0),
                        (_, _, Some(br)) if br > 0 => format!("{br}k"),
                        _ => String::new(),
                    };
                    let cols = [format!("    {base}"), extra, fmt_time(f64::from(f.length.unwrap_or(0))), human_size(f.size), String::new(), String::new(), String::new()];
                    let (resp, _) = result_line(ui, t, "", cols, false, egui::Id::unique(("dlfile", i, &f.filename)), false);
                    if resp.on_hover_text(t.l("double-click to download just this file")).double_clicked() {
                        let (row, file) = (r.clone(), f.clone());
                        act = Some(Box::new(move |a: &mut Nexus| a.dl_download(row, Some(vec![file]), None)));
                    }
                }
            }
        }
    });
    act
}

fn web_results(app: &mut Nexus, ui: &mut Ui, t: &Theme, _src: &str) -> Option<ActFn> {
    let mut act: Option<ActFn> = None;
    if app.download.web_rows.is_empty() {
        return None;
    }
    header_row(ui, t, ["album", "format", "tracks", "size", "where", "year", ""]);
    egui::ScrollArea::vertical().id_salt("dl-web").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let rows = app.download.web_rows.clone();
        for (i, r) in rows.iter().enumerate() {
            let open = app.download.open.contains(&i);
            let tracks = r.files.as_ref().map(|f| f.len().to_string()).unwrap_or_else(|| "…".into());
            let size = r.files.as_ref().map(|f| f.iter().map(|x| x.size).sum::<u64>()).filter(|s| *s > 0).unwrap_or(r.size);
            let where_ = if r.artist.is_empty() { r.info.clone() } else { format!("{} · {}", r.artist, r.info) };
            let cols = [r.name.clone(), r.fmt.clone(), tracks, if size > 0 { human_size(size) } else { "—".into() }, where_, r.year.clone(), "download".into()];
            let (resp, dl_click) = result_line(ui, t, if open { "▾" } else { "▸" }, cols, open, egui::Id::unique(("webrow", i)), true);
            let tip = if r.license.is_empty() { r.page.clone() } else { format!("{}\nlicence: {}", r.page, r.license) };
            let resp = resp.on_hover_text(tip);
            if dl_click {
                act = Some(Box::new(move |a: &mut Nexus| a.dl_web_download(i, None)));
            } else if resp.clicked() {
                let need = r.files.is_none();
                act = Some(Box::new(move |a: &mut Nexus| {
                    if !a.download.open.remove(&i) {
                        a.download.open.insert(i);
                        if need {
                            a.dl_web_files(i, None);
                        }
                    }
                }));
            }
            if open {
                match &r.files {
                    None => {
                        result_line(ui, t, "", [format!("    {}", t.l("loading…")), String::new(), String::new(), String::new(), String::new(), String::new(), String::new()], false, egui::Id::unique(("webload", i)), false);
                    }
                    Some(files) => {
                        for f in files {
                            let title = if f.title.is_empty() { f.name.clone() } else { f.title.clone() };
                            let cols = [format!("    {title}"), String::new(), f.track.clone(), if f.size > 0 { human_size(f.size) } else { String::new() }, String::new(), String::new(), String::new()];
                            let (resp, _) = result_line(ui, t, "", cols, false, egui::Id::unique(("webfile", i, &f.url)), false);
                            if resp.on_hover_text(t.l("double-click to download just this file")).double_clicked() {
                                let url = f.url.clone();
                                act = Some(Box::new(move |a: &mut Nexus| a.dl_web_download(i, Some(url))));
                            }
                        }
                    }
                }
            }
        }
    });
    act
}

fn dots(frac: f64) -> String {
    let n = 16;
    let filled = ((frac.clamp(0.0, 1.0)) * n as f64) as usize;
    "●".repeat(filled) + &"·".repeat(n - filled)
}

fn downloads_list(app: &mut Nexus, ui: &mut Ui, t: &Theme) -> Option<ActFn> {
    let c = t.c;
    let mut act: Option<ActFn> = None;
    ui.horizontal(|ui| {
        widgets::eyebrow(ui, t, &t.l("DOWNLOADS"));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::pill_pad(ui, t, "clear finished", Pill::Normal, 12.0).clicked() {
                act = Some(Box::new(|a: &mut Nexus| a.dl_clear_finished()));
            }
        });
    });
    let banner = |ui: &mut Ui, text: &str, buttons: &[&str]| -> Option<usize> {
        let mut hit = None;
        egui::Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)).corner_radius(t.radius.max(8.0)).inner_margin(egui::Margin { left: 14, right: 8, top: 6, bottom: 6 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let bw = 240.0;
                let g = ui.fonts_mut(|f| f.layout(t.l(text), t.font(W::Regular, -1.0), c.dim, (ui.available_width() - bw).max(120.0)));
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, c.dim);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for (i, b) in buttons.iter().enumerate().rev() {
                        if widgets::pill_pad(ui, t, b, if i == 0 { Pill::Primary } else { Pill::Normal }, 12.0).clicked() {
                            hit = Some(i);
                        }
                    }
                });
            });
        });
        hit
    };
    if app.download.firewall_bad {
        if banner(ui, "Windows Firewall isn't letting other Soulseek users connect to you, so some downloads never start. One admin prompt fixes it (only Nexus's download ports are opened).", &["fix firewall"]) == Some(0) {
            act = Some(Box::new(|a: &mut Nexus| {
                let port = a.slsk_config().listen_port;
                a.toast_ms("click Yes on the Windows prompt…", 15.0);
                let (tx, ctx) = (a.download.tx.clone(), a.ctx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::FirewallFixed(nexus_core::netcheck::firewall_fix(port)));
                    ctx.request_repaint();
                });
            }));
        }
    }
    if !app.download.vpn.is_empty() && !app.download.vpn_hidden && app.dl_source() == "soulseek" {
        match banner(ui, &nexus_core::netcheck::vpn_advice(&app.download.vpn), &["use Internet Archive", "hide"]) {
            Some(0) => {
                act = Some(Box::new(|a: &mut Nexus| {
                    a.cfg.set("dl_source", serde_json::json!("archive"));
                    a.cfg.save();
                }))
            }
            Some(1) => app.download.vpn_hidden = true,
            _ => {}
        }
    }
    // one line per folder being fetched
    struct Line {
        key: String,
        text: String,
        frac: f64,
        web: Option<u64>,
    }
    let st = &app.download;
    let auto_switch = app.cfg.b("dl_auto_switch");
    let mins = app.cfg.f("dl_stall_minutes").max(1.0);
    let mut groups: Vec<(String, Vec<&Transfer>)> = Vec::new();
    for tr in &st.transfers {
        let key = dl::job_key(&tr.username, &folder_of(tr));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(tr),
            None => groups.push((key, vec![tr])),
        }
    }
    let mut lines: Vec<Line> = Vec::new();
    for (key, fs) in &groups {
        let total: u64 = fs.iter().map(|f| f.size).sum::<u64>().max(1);
        let got: u64 = fs.iter().map(|f| f.done).sum();
        let ok = fs.iter().filter(|f| f.state == TState::Done).count();
        let bad = fs.iter().filter(|f| f.state.failed()).count();
        let queued = fs.iter().filter(|f| f.state == TState::Queued).count();
        let imp = fs.iter().filter(|f| st.pending.imported.contains(&(f.username.clone(), f.filename.clone())) || (f.state == TState::Done && !f.local_path.as_ref().is_some_and(|p| p.exists()))).count();
        let pct = got * 100 / total;
        let job = st.pending.jobs.get(key);
        let spots: Vec<u32> = fs.iter().filter_map(|f| f.place).filter(|p| *p > 0).collect();
        let state = if imp > 0 && imp >= ok {
            if imp == fs.len() { format!("in your library {imp}/{}", fs.len()) } else { format!("imported {imp}/{}", fs.len()) }
        } else if ok == fs.len() {
            "done".into()
        } else if bad > 0 && ok + bad == fs.len() {
            let reason = fs.iter().find_map(|f| if let TState::Failed(r) = &f.state { Some(r.clone()).filter(|r| !r.is_empty()) } else { None });
            format!("{bad} failed{} — right-click → try another user", reason.map(|r| format!(" ({r})")).unwrap_or_default())
        } else if queued > 0 && got == 0 {
            let mut s = if let Some(m) = spots.iter().min() { format!("waiting in their queue (#{m})") } else { format!("queued ({queued})") };
            if let Some(left) = st.stall_left(job, auto_switch, mins) {
                s += &if left > 0 { format!(" · tries another user in {left} min") } else { " · trying another user…".into() };
            }
            s
        } else {
            format!("{pct}%{}", if bad > 0 { format!(" · {bad} failed") } else { String::new() })
        };
        let (user, folder) = dl::split_key(key);
        let name = { let n = slsk::remote_split(&folder).1; if n.is_empty() { folder.clone() } else { n } };
        lines.push(Line { key: key.clone(), text: format!("{name}  ·  {user}  ·  {ok}/{}  ·  {state}", fs.len()), frac: got as f64 / total as f64, web: None });
    }
    for j in st.webdl.jobs() {
        let n = j.files.len().max(1);
        let mut frac = if j.total > 0 { j.got as f64 / j.total as f64 } else { j.done as f64 / n as f64 };
        if j.state == "in your library" || j.state == "importing" {
            frac = 1.0;
        }
        let src = if j.row.source == "archive" { "internet archive" } else { "jamendo" };
        let state = if j.state == "downloading" { format!("{}%", (frac * 100.0) as u32) } else { j.state.clone() };
        lines.push(Line { key: format!("web:{}", j.id), text: format!("{}  ·  {src}  ·  {}/{n}  ·  {state}", j.row.name, j.done), frac, web: Some(j.id) });
    }
    let h = ui.available_height().max(60.0);
    egui::ScrollArea::vertical().id_salt("dl-list").max_height(h).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if lines.is_empty() {
            widgets::small_dim(ui, t, &t.l("nothing downloading"));
        }
        for l in &lines {
            let w = ui.available_width() - 12.0;
            let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, alpha(c.select, 0.7));
            }
            let bar = dots(l.frac);
            let bar_g = elided(ui, &bar, t.font(W::Regular, -3.0), c.text, 150.0);
            ui.painter().galley(pos2(r.left() + 6.0, r.center().y - bar_g.size().y / 2.0), bar_g, c.text);
            let g = elided(ui, &l.text, t.body(), c.text, w - 170.0);
            ui.painter().galley(pos2(r.left() + 164.0, r.center().y - g.size().y / 2.0), g, c.text);
            let key = l.key.clone();
            let web = l.web;
            resp.context_menu(|ui| {
                if let Some(id) = web {
                    let page = app.download.webdl.jobs().into_iter().find(|j| j.id == id).map(|j| (j.row.page.clone(), j.error.clone())).unwrap_or_default();
                    if ui.button(t.l("cancel")).clicked() {
                        app.download.webdl.cancel(id);
                        ui.close();
                    }
                    if ui.button(t.l("retry")).clicked() {
                        app.download.webdl.retry(id);
                        ui.close();
                    }
                    if !page.0.is_empty() && ui.button(t.l("open its page")).clicked() {
                        let _ = open::that(&page.0);
                        ui.close();
                    }
                    if !page.1.is_empty() {
                        ui.add_enabled(false, egui::Button::new(format!("last error: {}", page.1).chars().take(90).collect::<String>()));
                    }
                    return;
                }
                let k1 = key.clone();
                if ui.button(t.l("try another user")).clicked() {
                    act = Some(Box::new(move |a: &mut Nexus| a.dl_switch(&k1, false)));
                    ui.close();
                }
                let k2 = key.clone();
                if ui.button(t.l("retry failed files")).clicked() {
                    act = Some(Box::new(move |a: &mut Nexus| a.dl_retry_failed(&k2)));
                    ui.close();
                }
                let k3 = key.clone();
                if ui.button(t.l("cancel")).clicked() {
                    act = Some(Box::new(move |a: &mut Nexus| a.dl_cancel(&k3)));
                    ui.close();
                }
                ui.separator();
                let k4 = key.clone();
                if ui.button(t.l("search for it again")).clicked() {
                    act = Some(Box::new(move |a: &mut Nexus| {
                        let name = a.download.pending.jobs.get(&k4).map(|j| j.name.clone()).unwrap_or_else(|| slsk::remote_split(&dl::split_key(&k4).1).1);
                        a.download.query = name.clone();
                        a.dl_search(Some(name));
                    }));
                    ui.close();
                }
                if ui.button(t.l("open soulseek log")).clicked() {
                    let _ = open::that(nexus_core::paths::data_dir().join("soulseek.log"));
                    ui.close();
                }
            });
        }
    });
    act
}
