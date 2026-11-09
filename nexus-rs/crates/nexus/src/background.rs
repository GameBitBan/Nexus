//! What's behind the window: your own background or the skin's — a still image, or a silent looping
//! video decoded on its own thread (paused while the window is minimised).

use crossbeam_channel::{Receiver, Sender, bounded};
use egui::{ColorImage, TextureHandle, TextureOptions};
use ffmpeg_next as ffmpeg;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_SIDE: u32 = 1600;

pub struct Background {
    key: (PathBuf, bool),
    pub tex: Option<TextureHandle>,
    pub size: [usize; 2],
    video: Option<Video>,
    image_rx: Option<Receiver<Option<ColorImage>>>,
}

struct Video {
    frames: Receiver<ColorImage>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

impl Drop for Video {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Default for Background {
    fn default() -> Self {
        Background { key: (PathBuf::new(), false), tex: None, size: [0, 0], video: None, image_rx: None }
    }
}

impl Background {
    /// Switches to `path` (None = no background). Cheap when nothing changed.
    pub fn set(&mut self, ctx: &egui::Context, path: Option<PathBuf>, blur: bool) {
        let key = (path.clone().unwrap_or_default(), blur);
        if key == self.key {
            return;
        }
        self.key = key;
        self.tex = None;
        self.video = None;
        self.image_rx = None;
        let Some(p) = path else { return };
        if nexus_core::skins::is_video(&p) {
            self.video = Some(start_video(ctx.clone(), p, blur));
        } else {
            let (tx, rx) = bounded(1);
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(load_image(&p, blur));
                ctx2.request_repaint();
            });
            self.image_rx = Some(rx);
        }
    }

    /// Once per frame: take a decoded image or the newest video frame.
    pub fn pump(&mut self, ctx: &egui::Context, hidden: bool) {
        if let Some(rx) = &self.image_rx {
            if let Ok(img) = rx.try_recv() {
                self.image_rx = None;
                if let Some(img) = img {
                    self.size = img.size;
                    self.tex = Some(ctx.load_texture("background", img, TextureOptions::LINEAR));
                }
            }
        }
        if let Some(v) = &self.video {
            v.paused.store(hidden, Ordering::Relaxed);
            if let Some(img) = v.frames.try_iter().last() {
                self.size = img.size;
                match &mut self.tex {
                    Some(t) if t.size() == img.size => t.set(img, TextureOptions::LINEAR),
                    _ => self.tex = Some(ctx.load_texture("background-video", img, TextureOptions::LINEAR)),
                }
            }
        }
    }

    pub fn is_video(&self) -> bool {
        self.video.is_some()
    }
}

fn to_color(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

/// Soft blur the cheap way: shrink a lot, grow back smoothly.
fn blur_steps(img: image::DynamicImage, start: u32, steps: &[u32]) -> image::DynamicImage {
    use image::imageops::FilterType::Triangle;
    let (w, h) = (img.width().max(1), img.height().max(1));
    let fit = |s: u32| {
        let k = f64::from(s) / f64::from(w.min(h));
        ((f64::from(w) * k).round().max(1.0) as u32, (f64::from(h) * k).round().max(1.0) as u32)
    };
    let (a, b) = fit(start);
    let mut cur = img.resize_exact(a, b, Triangle);
    for &s in steps {
        let (a, b) = fit(s);
        cur = cur.resize_exact(a, b, Triangle);
    }
    cur
}

fn load_image(p: &Path, blur: bool) -> Option<ColorImage> {
    let img = image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if blur {
        blur_steps(img, 48, &[192, 768])
    } else if img.width().max(img.height()) > 2560 {
        img.resize(2560, 2560, image::imageops::FilterType::CatmullRom)
    } else {
        img
    };
    Some(to_color(&img.to_rgba8()))
}

fn start_video(ctx: egui::Context, path: PathBuf, blur: bool) -> Video {
    let (tx, rx) = bounded::<ColorImage>(2);
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let (s2, p2) = (stop.clone(), paused.clone());
    std::thread::Builder::new()
        .name("nexus-bg-video".into())
        .spawn(move || {
            if let Err(e) = play_video(&ctx, &path, blur, &tx, &s2, &p2) {
                crate::log_problem(&format!("background video: {e}"));
            }
        })
        .ok();
    Video { frames: rx, stop, paused }
}

fn play_video(ctx: &egui::Context, path: &Path, blur: bool, tx: &Sender<ColorImage>, stop: &AtomicBool, paused: &AtomicBool) -> Result<(), String> {
    ffmpeg::init().map_err(|e| e.to_string())?;
    let frame_time = Duration::from_secs_f64(1.0 / 30.0);
    loop {
        let mut ictx = ffmpeg::format::input(&path).map_err(|e| e.to_string())?;
        let st = ictx.streams().best(ffmpeg::media::Type::Video).ok_or("no video stream")?;
        let idx = st.index();
        let tb = f64::from(st.time_base());
        let mut dec = ffmpeg::codec::context::Context::from_parameters(st.parameters()).map_err(|e| e.to_string())?.decoder().video().map_err(|e| e.to_string())?;
        let (w, h) = (dec.width().max(1), dec.height().max(1));
        let k = (f64::from(MAX_SIDE) / f64::from(w.max(h))).min(1.0);
        let (ow, oh) = (((f64::from(w) * k) as u32).max(2) & !1, ((f64::from(h) * k) as u32).max(2) & !1);
        let mut scaler = ffmpeg::software::scaling::Context::get(dec.format(), w, h, ffmpeg::format::Pixel::RGBA, ow, oh, ffmpeg::software::scaling::Flags::BILINEAR)
            .map_err(|e| e.to_string())?;
        let mut decoded = ffmpeg::frame::Video::empty();
        let mut rgba = ffmpeg::frame::Video::empty();
        let t0 = Instant::now();
        let mut last_sent = Instant::now() - frame_time;
        let mut paused_for = Duration::ZERO;
        let mut send = |decoded: &ffmpeg::frame::Video, rgba: &mut ffmpeg::frame::Video, last_sent: &mut Instant, paused_for: &mut Duration| -> Result<bool, String> {
            while paused.load(Ordering::Relaxed) {
                if stop.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(200));
                *paused_for += Duration::from_millis(200);
            }
            // keep real time: wait for this frame's moment
            let pts = decoded.pts().map(|p| p as f64 * tb).unwrap_or(0.0).max(0.0);
            let due = Duration::from_secs_f64(pts) + *paused_for;
            let now = t0.elapsed();
            if due > now {
                std::thread::sleep((due - now).min(Duration::from_millis(500)));
            }
            if last_sent.elapsed() < frame_time {
                return Ok(true); // 30 fps is plenty for a backdrop
            }
            scaler.run(decoded, rgba).map_err(|e| e.to_string())?;
            let stride = rgba.stride(0);
            let data = rgba.data(0);
            let (w, h) = (rgba.width() as usize, rgba.height() as usize);
            let mut px = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                px.extend_from_slice(&data[y * stride..y * stride + w * 4]);
            }
            let img = if blur {
                let im = image::RgbaImage::from_raw(w as u32, h as u32, px).ok_or("bad frame")?;
                to_color(&blur_steps(image::DynamicImage::ImageRgba8(im), 64, &[256]).to_rgba8())
            } else {
                ColorImage::from_rgba_unmultiplied([w, h], &px)
            };
            *last_sent = Instant::now();
            if tx.send(img).is_err() {
                return Ok(false);
            }
            ctx.request_repaint();
            Ok(true)
        };
        for (s, packet) in ictx.packets() {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            if s.index() != idx || dec.send_packet(&packet).is_err() {
                continue;
            }
            while dec.receive_frame(&mut decoded).is_ok() {
                if !send(&decoded, &mut rgba, &mut last_sent, &mut paused_for)? {
                    return Ok(());
                }
            }
        }
        // loop forever (silent, like a GIF)
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
    }
}
