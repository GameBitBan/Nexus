//! Album covers as GPU textures: loaded and scaled on worker threads, kept in a bounded cache,
//! drawn with rounded corners. Missing covers get the dot-matrix placeholder.

use crate::theme::{Theme, alpha};
use crossbeam_channel::{Receiver, Sender, unbounded};
use egui::epaint::RectShape;
use egui::{Align2, Color32, ColorImage, Painter, Rect, TextureHandle, TextureId, TextureOptions, pos2};
use std::collections::HashMap;
use std::path::PathBuf;

const LIMIT: usize = 600;

#[derive(Clone, PartialEq, Eq, Hash)]
enum Kind {
    Square(u32),
    Blur,
    /// any image file, longest side this many px (skin card previews)
    File(std::path::PathBuf, u32),
}

struct Job {
    key: String,
    kind: Kind,
    generation: u64,
}

struct Done {
    key: String,
    kind: Kind,
    generation: u64,
    tex: Option<TextureHandle>,
    color: Option<Option<Color32>>,
}

enum Slot {
    Loading,
    Ready(TextureHandle),
    Missing,
}

pub struct Covers {
    dir: PathBuf,
    map: HashMap<(String, Kind), (Slot, u64)>,
    colors: HashMap<String, Option<Color32>>,
    tick: u64,
    generation: u64,
    jobs: Sender<Job>,
    done: Receiver<Done>,
}

impl Covers {
    pub fn new(ctx: &egui::Context, dir: PathBuf) -> Self {
        let (jtx, jrx) = unbounded::<Job>();
        let (dtx, drx) = unbounded::<Done>();
        for i in 0..2 {
            let (jrx, dtx, ctx, dir) = (jrx.clone(), dtx.clone(), ctx.clone(), dir.clone());
            std::thread::Builder::new()
                .name(format!("nexus-covers-{i}"))
                .spawn(move || {
                    for job in jrx {
                        let path = dir.join(format!("{}.jpg", job.key));
                        let (tex, color) = match &job.kind {
                            Kind::Square(px) => (load_square(&ctx, &path, &job.key, *px), None),
                            Kind::File(f, px) => (load_file(&ctx, f, *px), None),
                            Kind::Blur => {
                                let (t, c) = load_blur(&ctx, &path, &job.key);
                                (t, Some(c))
                            }
                        };
                        let _ = dtx.send(Done { key: job.key, kind: job.kind, generation: job.generation, tex, color });
                        ctx.request_repaint();
                    }
                })
                .expect("spawn cover thread");
        }
        Covers { dir, map: HashMap::new(), colors: HashMap::new(), tick: 0, generation: 0, jobs: jtx, done: drx }
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    /// Forget everything (after a rescan or tag edit changed covers on disk).
    pub fn clear(&mut self) {
        self.generation += 1;
        self.map.clear();
        self.colors.clear();
    }

    /// Forget one album's cover.
    pub fn forget(&mut self, key: &str) {
        self.map.retain(|(k, _), _| k != key);
        self.colors.remove(key);
    }

    /// Once per frame: take finished loads, evict the least recently used.
    pub fn pump(&mut self) {
        self.tick += 1;
        for d in self.done.try_iter() {
            if d.generation != self.generation {
                continue;
            }
            if let Some(c) = d.color {
                self.colors.insert(d.key.clone(), c);
            }
            let slot = match d.tex {
                Some(t) => Slot::Ready(t),
                None => Slot::Missing,
            };
            self.map.insert((d.key, d.kind), (slot, self.tick));
        }
        if self.map.len() > LIMIT {
            let mut ages: Vec<(u64, (String, Kind))> = self.map.iter().map(|(k, (_, t))| (*t, k.clone())).collect();
            ages.sort_by_key(|(t, _)| *t);
            for (_, k) in ages.into_iter().take(self.map.len() - LIMIT * 3 / 4) {
                self.map.remove(&k);
            }
        }
    }

    fn lookup(&mut self, key: &str, kind: Kind) -> Option<TextureId> {
        if key.is_empty() {
            return None;
        }
        let tick = self.tick;
        let k = (key.to_string(), kind.clone());
        match self.map.get_mut(&k) {
            Some((slot, t)) => {
                *t = tick;
                match slot {
                    Slot::Ready(h) => Some(h.id()),
                    _ => None,
                }
            }
            None => {
                self.map.insert(k, (Slot::Loading, tick));
                let _ = self.jobs.send(Job { key: key.to_string(), kind, generation: self.generation });
                None
            }
        }
    }

    /// The cover drawn `size` points wide (scaled for the screen). None while loading or when there's none.
    pub fn get(&mut self, key: &str, size: f32, ppp: f32) -> Option<TextureId> {
        let px = ((size * ppp).ceil() as u32).max(8);
        let bucket = px.div_ceil(16) * 16;
        self.lookup(key, Kind::Square(bucket))
    }

    /// Any picture file (cached under `key`), scaled to fit `px`.
    pub fn file_texture(&mut self, key: &str, path: &std::path::Path, px: u32) -> Option<TextureId> {
        if !path.exists() {
            return None;
        }
        self.lookup(key, Kind::File(path.to_path_buf(), px))
    }

    /// The soft colour wash made from a cover (ambient background, visualizer page).
    pub fn blurred(&mut self, key: &str) -> Option<TextureId> {
        self.lookup(key, Kind::Blur)
    }

    /// A lively colour from the cover (visualizer "cover colour"), once the blur has loaded.
    pub fn color(&mut self, key: &str) -> Option<Color32> {
        if let Some(c) = self.colors.get(key) {
            return *c;
        }
        self.blurred(key);
        None
    }

    pub fn exists(&self, key: &str) -> bool {
        !key.is_empty() && self.dir.join(format!("{key}.jpg")).exists()
    }

    /// Draws a cover (or the placeholder) into `rect` with the theme's rounding.
    pub fn paint(&mut self, painter: &Painter, t: &Theme, key: &str, rect: Rect) {
        let ppp = painter.ctx().pixels_per_point();
        let rad = t.cover_radius(rect.width());
        match self.get(key, rect.width(), ppp) {
            Some(id) => paint_texture(painter, id, rect, rad, Color32::WHITE),
            None => paint_placeholder(painter, t, rect),
        }
    }
}

pub fn full_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

pub fn paint_texture(painter: &Painter, id: TextureId, rect: Rect, radius: f32, tint: Color32) {
    painter.add(RectShape::filled(rect, radius, tint).with_texture(id, full_uv()));
}

/// The empty-cover look: panel colour, a faint dot grid, a border and the placeholder glyph.
pub fn paint_placeholder(painter: &Painter, t: &Theme, rect: Rect) {
    let c = t.c;
    let size = rect.width();
    let rad = t.cover_radius(size);
    let r = rect.shrink(0.5);
    painter.rect_filled(r, rad, c.panel);
    if size >= 48.0 {
        let dot = alpha(c.dim, 0.18);
        let step = (size / 16.0).max(6.0);
        let rr = (step / 9.0).max(0.7);
        let inner = r.shrink(rad * 0.3);
        let mut y = r.top() + step / 2.0;
        while y < r.bottom() {
            let mut x = r.left() + step / 2.0;
            while x < r.right() {
                if inner.contains(pos2(x, y)) || rad < 1.0 {
                    painter.circle_filled(pos2(x, y), rr, dot);
                }
                x += step;
            }
            y += step;
        }
    }
    painter.rect_stroke(r, rad, egui::Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
    let glyph = if t.placeholder.is_empty() { None } else { Some(t.placeholder.clone()) };
    if let Some(g) = glyph {
        let f = egui::FontId::proportional((size * 0.3).max(6.0));
        painter.text(rect.center(), Align2::CENTER_CENTER, g, f, c.dim);
    }
}

fn open(path: &std::path::Path) -> Option<image::DynamicImage> {
    image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()
}

fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

fn load_square(ctx: &egui::Context, path: &std::path::Path, key: &str, px: u32) -> Option<TextureHandle> {
    let img = open(path)?;
    let sq = img.resize_to_fill(px, px, image::imageops::FilterType::CatmullRom).to_rgba8();
    Some(ctx.load_texture(format!("cover:{key}:{px}"), to_color_image(&sq), TextureOptions::LINEAR))
}

fn load_file(ctx: &egui::Context, path: &std::path::Path, px: u32) -> Option<TextureHandle> {
    let img = open(path)?;
    let img = img.resize(px, px, image::imageops::FilterType::Triangle).to_rgba8();
    Some(ctx.load_texture(format!("file:{}", path.display()), to_color_image(&img), TextureOptions::LINEAR))
}

/// 14 px → 56 → 224 with smooth scaling: a cheap, soft blur (same as the Python version).
fn load_blur(ctx: &egui::Context, path: &std::path::Path, key: &str) -> (Option<TextureHandle>, Option<Color32>) {
    use image::imageops::FilterType::Triangle;
    let Some(img) = open(path) else { return (None, None) };
    let tiny = img.resize_exact(14, 14, Triangle).to_rgba8();
    let color = cover_color(&img.resize_exact(16, 16, Triangle).to_rgba8());
    let mut cur = image::DynamicImage::ImageRgba8(tiny);
    for s in [56u32, 224] {
        cur = cur.resize_exact(s, s, Triangle);
    }
    let tex = ctx.load_texture(format!("blur:{key}"), to_color_image(&cur.to_rgba8()), TextureOptions::LINEAR);
    (Some(tex), color)
}

/// The most saturated bright pixel, made a bit livelier; None for grey covers.
fn cover_color(img: &image::RgbaImage) -> Option<Color32> {
    let mut best = -1.0f32;
    let mut col: Option<(f32, f32, f32)> = None;
    for p in img.pixels() {
        let (h, s, v) = rgb_to_hsv(p[0], p[1], p[2]);
        let score = s * 0.7 + v * 0.3;
        if v > 0.25 && score > best {
            best = score;
            col = Some((h, s, v));
        }
    }
    let (h, s, v) = col?;
    if s < 0.18 {
        return None;
    }
    let (r, g, b) = hsv_to_rgb(h, (s * 1.1).min(1.0), v.max(0.75));
    Some(Color32::from_rgb(r, g, b))
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if mx == 0.0 { 0.0 } else { d / mx };
    (h, s, mx)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
