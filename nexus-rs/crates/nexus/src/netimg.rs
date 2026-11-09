//! Pictures from the web (Discover artists and covers): fetched off the UI thread, cached on disk in
//! `discover_img/`, kept as textures while Nexus runs.

use crossbeam_channel::{Receiver, Sender, unbounded};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct NetImages {
    tex: HashMap<String, Option<TextureHandle>>,
    jobs: Sender<String>,
    done: Receiver<(String, Option<ColorImage>)>,
}

impl NetImages {
    pub fn new(ctx: &Context) -> Self {
        let dir: PathBuf = nexus_core::paths::data_dir().join("discover_img");
        let _ = std::fs::create_dir_all(&dir);
        let (jobs, job_rx) = unbounded::<String>();
        let (done_tx, done) = unbounded();
        for i in 0..4 {
            let (rx, tx, dir, ctx) = (job_rx.clone(), done_tx.clone(), dir.clone(), ctx.clone());
            std::thread::Builder::new()
                .name(format!("nexus-netimg-{i}"))
                .spawn(move || {
                    for url in rx {
                        let f = dir.join(format!("{}.jpg", nexus_core::util::md5_hex(&url)));
                        let data = std::fs::read(&f).ok().or_else(|| {
                            let d = nexus_core::net::get_bytes(&url, 15, 4_000_000).ok()?;
                            let _ = std::fs::write(&f, &d);
                            Some(d)
                        });
                        let img = data.and_then(|d| image::load_from_memory(&d).ok()).map(|img| {
                            let img = img.thumbnail(400, 400).to_rgba8();
                            ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
                        });
                        if tx.send((url, img)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                })
                .expect("spawn image fetcher");
        }
        NetImages { tex: HashMap::new(), jobs, done }
    }

    /// The picture if it's here; otherwise starts fetching it.
    pub fn get(&mut self, url: &str) -> Option<TextureHandle> {
        if url.is_empty() {
            return None;
        }
        match self.tex.get(url) {
            Some(t) => t.clone(),
            None => {
                self.tex.insert(url.to_string(), None);
                let _ = self.jobs.send(url.to_string());
                None
            }
        }
    }

    pub fn pump(&mut self, ctx: &Context) {
        while let Ok((url, img)) = self.done.try_recv() {
            let t = img.map(|i| ctx.load_texture(format!("net:{url}"), i, TextureOptions::LINEAR));
            self.tex.insert(url, t);
        }
    }
}
