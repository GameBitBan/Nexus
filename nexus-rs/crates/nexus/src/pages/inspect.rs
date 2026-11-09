//! Inspect: Spek-style spectrogram and a verdict — is this file really lossless?

use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::W;
use crate::widgets::{self, Pill, galley};
use crossbeam_channel::{Receiver, bounded};
use egui::{Align, Align2, Color32, ColorImage, Layout, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, pos2, vec2};
use nexus_core::inspect::{Analysis, Kind, palette, spectrum_rgb};
use nexus_core::util::{fmt_time, human_size};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Default)]
pub struct InspectState {
    pub path: Option<String>,
    info: Option<Arc<Analysis>>,
    tex: Option<(TextureHandle, String)>,
    cache: VecDeque<(String, Arc<Analysis>)>,
    job: Option<Job>,
    error: String,
    meta: (String, String),
}

struct Job {
    path: String,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Result<Analysis, String>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl InspectState {
    fn load(&mut self, app_lib: &nexus_core::library::Library, path: &str, ctx: &egui::Context) {
        if self.path.as_deref() == Some(path) && (self.info.is_some() || self.job.is_some()) {
            return;
        }
        self.path = Some(path.to_string());
        self.error.clear();
        self.tex = None;
        self.info = None;
        self.job = None;
        let t = app_lib.track(path).map(|t| (**t).clone()).or_else(|| nexus_core::scanner::read_track(std::path::Path::new(path)));
        let fname = std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        self.meta = match &t {
            Some(t) => {
                let name = if t.title.is_empty() { fname.clone() } else { t.title.clone() };
                let title = if t.artist.is_empty() { name } else { format!("{name}  —  {}", t.artist) };
                let parts = [
                    if t.fmt.is_empty() { std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default() } else { t.fmt.clone() },
                    if t.sr > 0 { format!("{} kHz", trim_num(f64::from(t.sr) / 1000.0)) } else { String::new() },
                    if t.bits > 0 { format!("{}-bit", t.bits) } else { String::new() },
                    if t.channels > 0 { format!("{} ch", t.channels) } else { String::new() },
                    if size > 0 && t.duration > 0.0 { format!("{:.0} kbps avg", size as f64 * 8.0 / t.duration / 1000.0) } else { String::new() },
                    if t.duration > 0.0 { fmt_time(t.duration) } else { String::new() },
                    if size > 0 { human_size(size) } else { String::new() },
                ];
                (title, format!("{}\n{path}", parts.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("  ·  ")))
            }
            None => (fname, path.to_string()),
        };
        if let Some((_, a)) = self.cache.iter().find(|(p, _)| p == path) {
            self.info = Some(a.clone());
            return;
        }
        let (tx, rx) = bounded(1);
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p2, c2, path2, ctx2) = (progress.clone(), cancel.clone(), path.to_string(), ctx.clone());
        std::thread::Builder::new()
            .name("nexus-inspect".into())
            .spawn(move || {
                let _ = tx.send(nexus_core::inspect::analyze(&path2, 1400, &p2, &c2));
                ctx2.request_repaint();
            })
            .ok();
        self.job = Some(Job { path: path.to_string(), progress, cancel, rx });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        match job.rx.try_recv() {
            Ok(res) => {
                let path = job.path.clone();
                self.job = None;
                match res {
                    Ok(a) => {
                        let a = Arc::new(a);
                        self.cache.push_back((path, a.clone()));
                        if self.cache.len() > 12 {
                            self.cache.pop_front();
                        }
                        self.info = Some(a);
                    }
                    Err(e) => self.error = e,
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(120)),
            Err(_) => self.job = None,
        }
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect, path: Option<String>) {
    let ctx = ui.ctx().clone();
    if let Some(p) = path {
        let lib = app.lib.clone();
        app.inspect.load(&lib, &p, &ctx);
        app.page = crate::app::Page::Inspect(None);
    }
    app.inspect.poll(&ctx);
    let t = app.t.clone();
    let c = t.c;
    let pal = { let p = app.cfg.s("inspect_palette"); if p.is_empty() { "spek".to_string() } else { p } };
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 18.0));
    let mut pick = false;
    region(ui, Rect::from_min_size(inner.min, vec2(inner.width(), 56.0)), Layout::left_to_right(Align::Max), |ui| {
        let g = galley(ui, "Inspect", t.font(W::Display, 27.0), t.h1_color.unwrap_or(c.text));
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        ui.painter().galley(r.min, g, c.text);
        ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if widgets::pill(ui, &t, "done", Pill::Primary).clicked() {
                app.acts.push(Act::Back);
            }
            pick = widgets::pill(ui, &t, "open file…", Pill::Normal).clicked();
            if widgets::pill(ui, &t, if pal == "mono" { "color" } else { "mono" }, Pill::Normal).clicked() {
                app.cfg.set("inspect_palette", if pal == "mono" { "spek" } else { "mono" });
                app.cfg.save();
                app.inspect.tex = None;
            }
        });
    });
    if pick {
        let exts: Vec<&str> = nexus_core::library::AUDIO_EXT.iter().map(|e| e.trim_start_matches('.')).collect();
        if let Some(f) = rfd::FileDialog::new().set_directory(nexus_core::paths::music_dir()).add_filter("audio", &exts).pick_file() {
            let lib = app.lib.clone();
            app.inspect.load(&lib, &f.to_string_lossy(), &ctx);
        }
    }
    let st = &app.inspect;
    let (title, meta) = if st.path.is_some() { st.meta.clone() } else { (t.l("nothing inspected yet"), String::new()) };
    let verdict = if let Some(job) = &st.job {
        (t.l(&format!("analysing… {}%", job.progress.load(Ordering::Relaxed) / 10)), Color32::TRANSPARENT)
    } else if !st.error.is_empty() {
        (t.l(&format!("couldn't analyse this file: {}", st.error)), Color32::TRANSPARENT)
    } else if let Some(a) = &st.info {
        let (mark, col) = match a.verdict.kind {
            Kind::Good => ("●  ", Color32::from_rgb(0x4c, 0xc3, 0x7a)),
            Kind::Warn => ("◐  ", Color32::from_rgb(0xe0, 0xa0, 0x40)),
            Kind::Bad => ("○  ", Color32::from_rgb(0xe0, 0x5a, 0x5a)),
        };
        (format!("{mark}{}", t.l(&a.verdict.text)), col)
    } else {
        (String::new(), Color32::TRANSPARENT)
    };
    let notes = st.info.as_ref().map(|a| a.verdict.notes.join("   ·   ")).unwrap_or_default();
    let mut y = inner.top() + 56.0 + 10.0;
    let w = inner.width();
    let mut text = |s: &str, font: egui::FontId, col: Color32, ui: &mut Ui| {
        if s.is_empty() {
            return;
        }
        let g = ui.fonts_mut(|f| f.layout(s.to_string(), font, col, w));
        let h = g.size().y;
        ui.painter().galley(pos2(inner.left(), y), g, col);
        y += h + 10.0;
    };
    text(&title, t.font(W::Bold, 6.0), c.text, ui);
    text(&meta, t.body(), c.dim, ui);
    text(&verdict.0, t.font(W::Bold, 3.0), c.text, ui);
    text(&notes, t.font(W::Regular, -1.5), c.dim, ui);
    let hint = t.l("how to read it: real lossless fills up to the top (~22 kHz for CD, ~48 kHz for 96k). \
                    MP3/AAC leave a flat ceiling — the dashed line — at 16–20 kHz. \
                    it's a strong hint, not proof: some encoders keep full bandwidth.");
    let hg = ui.fonts_mut(|f| f.layout(hint, t.font(W::Regular, -1.5), c.dim, w));
    let hint_h = hg.size().y;
    ui.painter().galley(pos2(inner.left(), inner.bottom() - hint_h), hg, c.dim);
    let view = Rect::from_min_max(pos2(inner.left(), y), pos2(inner.right(), inner.bottom() - hint_h - 10.0));
    if view.height() > 60.0 {
        spectrum_view(app, ui, view, &pal);
    }
}

fn spectrum_view(app: &mut Nexus, ui: &mut Ui, area: Rect, pal: &str) {
    let t = app.t.clone();
    let c = t.c;
    let font = t.body();
    let fh = ui.fonts_mut(|f| f.row_height(&font));
    let left = widgets::text_width(ui, "00 kHz", &font) + 14.0;
    let r = Rect::from_min_max(pos2(area.left() + left, area.top() + 8.0), pos2(area.right() - 70.0, area.bottom() - fh - 16.0));
    let p = ui.painter().clone();
    p.rect_filled(r, 0.0, Color32::BLACK);
    let Some(info) = app.inspect.info.clone() else {
        p.text(r.center(), Align2::CENTER_CENTER, t.l("pick a song: right-click → inspect, ctrl+i, or drop a file here"), font, c.dim);
        return;
    };
    let key = format!("{}:{pal}", app.inspect.path.clone().unwrap_or_default());
    if app.inspect.tex.as_ref().is_none_or(|(_, k)| *k != key) {
        let (w, h, rgb) = spectrum_rgb(&info, pal, 560);
        let img = ColorImage::from_rgb([w, h], &rgb);
        app.inspect.tex = Some((ui.ctx().load_texture("spectrum", img, TextureOptions::LINEAR), key));
    }
    if let Some((tex, _)) = &app.inspect.tex {
        p.image(tex.id(), r, crate::covers::full_uv(), Color32::WHITE);
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, c.border), StrokeKind::Inside);
    let nyq = info.sr as f32 / 2.0;
    // frequency axis
    let step = if nyq <= 24000.0 { 2000.0 } else if nyq <= 50000.0 { 5000.0 } else { 10000.0 };
    let mut f = 0.0;
    while f <= nyq {
        let y = r.bottom() - f / nyq * r.height();
        p.line_segment([pos2(r.left() - 4.0, y), pos2(r.left(), y)], Stroke::new(1.0, c.dim));
        let lab = if f == 0.0 { "0".to_string() } else { format!("{} kHz", trim_num(f64::from(f) / 1000.0)) };
        p.text(pos2(r.left() - 8.0, y), Align2::RIGHT_CENTER, lab, font.clone(), c.dim);
        f += step;
    }
    // cutoff marker
    let cut = info.verdict.cutoff;
    if info.verdict.drop > 0.0 && cut < nyq - 200.0 {
        let y = r.bottom() - cut / nyq * r.height();
        let mut x = r.left();
        while x < r.right() {
            p.line_segment([pos2(x, y), pos2((x + 5.0).min(r.right()), y)], Stroke::new(1.0, c.text));
            x += 9.0;
        }
        p.text(pos2(r.right() - 6.0, y - 5.0), Align2::RIGHT_BOTTOM, format!("cutoff {:.1} kHz", f64::from(cut) / 1000.0), font.clone(), c.text);
    }
    // time axis
    let dur = info.duration.max(1.0);
    let tstep = [5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0].into_iter().find(|s| dur / s <= 12.0).unwrap_or(600.0);
    let mut tt = 0.0;
    while tt <= dur {
        let x = r.left() + (tt / dur) as f32 * r.width();
        p.line_segment([pos2(x, r.bottom()), pos2(x, r.bottom() + 4.0)], Stroke::new(1.0, c.dim));
        p.text(pos2(x, r.bottom() + 6.0), Align2::CENTER_TOP, fmt_time(tt), font.clone(), c.dim);
        tt += tstep;
    }
    // dB legend
    let legend = Rect::from_min_size(pos2(r.right() + 14.0, r.top()), vec2(14.0, r.height()));
    let palv = palette(pal);
    let mut mesh = egui::Mesh::default();
    let n = 32;
    for i in 0..=n {
        let v = 1.0 - i as f32 / n as f32;
        let [r8, g8, b8] = palv[(v * 255.0) as usize];
        let y = legend.top() + i as f32 / n as f32 * legend.height();
        let col = Color32::from_rgb(r8, g8, b8);
        mesh.colored_vertex(pos2(legend.left(), y), col);
        mesh.colored_vertex(pos2(legend.right(), y), col);
        if i > 0 {
            let k = (i * 2) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    p.add(egui::Shape::mesh(mesh));
    for d in [0, -20, -40, -60, -80, -100, -120] {
        let y = legend.top() + (-d as f32 / 120.0) * legend.height();
        p.text(pos2(legend.right() + 4.0, y), Align2::LEFT_CENTER, d.to_string(), font.clone(), c.dim);
    }
    // hover readout
    let resp = ui.interact(r, egui::Id::unique("spectrum"), Sense::hover());
    if let Some(h) = resp.hover_pos() {
        let fr = (r.bottom() - h.y) / r.height() * nyq;
        let tm = f64::from((h.x - r.left()) / r.width()) * dur;
        let text = format!("{}  ·  {:.1} kHz", fmt_time(tm), fr / 1000.0);
        let g = galley(ui, &text, font.clone(), Color32::WHITE);
        let bw = g.size().x + 16.0;
        let bx = Rect::from_min_size(pos2((h.x + 12.0).min(r.right() - bw), (h.y - 28.0).max(r.top())), vec2(bw, fh + 8.0));
        p.rect_filled(bx, 0.0, Color32::from_black_alpha(200));
        p.galley(pos2(bx.left() + 8.0, bx.top() + 4.0), g, Color32::WHITE);
    }
}
