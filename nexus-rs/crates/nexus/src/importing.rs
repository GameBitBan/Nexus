//! New music arriving: dropped album zips and the watch folder (both sorted into Artist/Album).

use crate::app::Nexus;
use crossbeam_channel::{Receiver, Sender, unbounded};
use nexus_core::importer::{self, Imported, Watch};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub enum Msg {
    Marked(String),
    Imported(String, Imported),
}

#[derive(Default)]
struct Conf {
    folder: Option<PathBuf>,
    root: PathBuf,
}

pub struct Importing {
    conf: Arc<Mutex<Conf>>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    watching: bool,
    rescan_due: Option<f64>,
}

impl Importing {
    pub fn new(ctx: &egui::Context, marked: String) -> Self {
        let (tx, rx) = unbounded();
        let conf = Arc::new(Mutex::new(Conf::default()));
        let (c2, tx2, ctx) = (conf.clone(), tx.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("nexus-watch".into())
            .spawn(move || {
                let mut w = Watch::new(&marked);
                let mut last_marked = marked;
                loop {
                    let (folder, root) = {
                        let c = c2.lock();
                        (c.folder.clone(), c.root.clone())
                    };
                    let mut again = false;
                    if let Some(f) = folder.filter(|f| f.is_dir()) {
                        let m = w.mark(&f);
                        if m != last_marked {
                            last_marked = m.clone();
                            let _ = tx2.send(Msg::Marked(m));
                        }
                        let (got, a) = w.process(&f, &root);
                        again = a;
                        if !got.files.is_empty() {
                            let _ = tx2.send(Msg::Imported(String::new(), got));
                            ctx.request_repaint();
                        }
                    }
                    std::thread::sleep(Duration::from_secs(if again { 2 } else { 4 }));
                }
            })
            .expect("spawn watcher");
        Importing { conf, tx, rx, watching: false, rescan_due: None }
    }
}

impl Importing {
    /// Rescan a moment from now (more files may be landing).
    pub fn request_rescan(&mut self, now: f64) {
        self.rescan_due = Some(now + 2.5);
    }
}

impl Nexus {
    fn import_root(&mut self) -> PathBuf {
        let folders = self.cfg.strings("library_folders");
        let root = importer::root(&self.cfg.s("import_root"), &folders);
        // make sure imports show up in the library
        let rs = root.to_string_lossy().to_string();
        if !folders.iter().any(|f| rs.starts_with(f.as_str())) {
            let mut f = folders;
            f.push(rs);
            self.cfg.set("library_folders", serde_json::json!(f));
            self.cfg.save();
        }
        root
    }

    /// Settings changed / startup: what to watch and where to put things.
    pub fn watch_apply(&mut self) {
        let folder = self.cfg.s("watch_folder");
        let on = self.cfg.b("watch_enabled") && !folder.trim().is_empty();
        let root = self.import_root();
        let Some(im) = &mut self.importing else { return };
        let f = on.then(|| nexus_core::paths::expand_user(folder.trim())).filter(|f| f.is_dir());
        im.watching = f.is_some();
        let mut c = im.conf.lock();
        c.folder = f;
        c.root = root;
    }

    /// An album zip (Bandcamp etc.) → into the library.
    pub fn import_album_zip(&mut self, zip: PathBuf) {
        let root = self.import_root();
        let Some(im) = &self.importing else { return };
        let tx = im.tx.clone();
        let ctx = self.ctx.clone();
        let name = zip.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        self.toast(format!("importing {name}…"));
        std::thread::spawn(move || {
            let got = importer::import_zip(&zip, &root);
            let _ = tx.send(Msg::Imported(name, got));
            ctx.request_repaint();
        });
    }

    pub fn importing_tick(&mut self, now: f64) {
        let Some(im) = &mut self.importing else { return };
        let msgs: Vec<Msg> = im.rx.try_iter().collect();
        let due = im.rescan_due;
        for m in msgs {
            match m {
                Msg::Marked(f) => {
                    self.cfg.set("_watch_marked", serde_json::json!(f));
                    self.cfg.save();
                }
                Msg::Imported(name, got) => {
                    for l in &got.log {
                        log::info!("import: {l}");
                    }
                    let n = got.files.len();
                    let s = if n == 1 { "" } else { "s" };
                    if !name.is_empty() {
                        self.toast(if n > 0 { format!("imported {n} song{s} from {name}") } else { format!("no music found in {name}") });
                    } else if n > 0 {
                        self.toast(format!("imported {n} new song{s}"));
                    }
                    if n > 0 {
                        if let Some(im) = &mut self.importing {
                            im.rescan_due = Some(now + 2.5);
                        }
                        self.ctx.request_repaint_after(Duration::from_millis(2600));
                    }
                }
            }
        }
        if let Some(d) = due {
            if now >= d && self.scan.is_none() {
                if let Some(im) = &mut self.importing {
                    im.rescan_due = None;
                }
                self.rescan(false);
            }
        }
    }
}
