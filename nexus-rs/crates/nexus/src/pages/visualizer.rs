//! Full-window visualizer over a slowly moving blur of the cover; double-click for immersive (full screen).
//! Styles: dots, bars, mirror, wave, ring.

use crate::acts::Act;
use crate::analyzer::{Analyzer, resample};
use crate::app::Nexus;
use crate::covers::full_uv;
use crate::layout::{gradient, region};
use crate::theme::{W, alpha};
use crate::widgets::{self, Pill, galley};
use egui::epaint::RectShape;
use egui::{Align, Color32, Layout, Painter, Rect, Sense, Shape, Stroke, TextureId, Ui, pos2, vec2};
use nexus_core::util::fmt_time;
use std::f32::consts::PI;

pub const STYLES: [&str; 5] = ["dots", "bars", "mirror", "wave", "ring"];

/// Feeds the analyzer (call once per frame from whatever shows a visualizer).
pub fn tick(app: &mut Nexus, ui: &Ui) {
    app.vis_wanted = true;
    let playing = app.playing();
    let samples = if playing { Some((app.engine.vis_samples(2048), app.engine.sample_rate())) } else { None };
    let now = ui.input(|i| i.time);
    let sens = (app.cfg.f("vis_sensitivity") as f32) / 100.0;
    app.analyzer.tick(now, samples, sens);
}

/// The colour to draw with: the theme's text colour, or one picked from the cover.
pub fn vis_color(app: &mut Nexus) -> Color32 {
    if app.cfg.s("vis_color") == "cover" {
        if let Some(k) = app.current_track().map(|t| t.cover.clone()) {
            if let Some(c) = app.covers.color(&k) {
                return c;
            }
        }
    }
    app.t.c.text
}

#[allow(clippy::too_many_arguments)]
pub fn paint(p: &Painter, r: Rect, an: &Analyzer, style: &str, fg: Color32, dim: Color32, pitch: f32, cover: Option<(TextureId, f32)>) {
    let (w, h) = (r.width(), r.height());
    match style {
        "bars" => {
            let n = ((w / pitch.max(6.0)) as usize).max(8);
            let v = resample(&an.bands, n);
            let bw = w / n as f32;
            for (i, x) in v.iter().enumerate() {
                let hh = (x * h).max(2.0);
                p.rect_filled(Rect::from_min_size(pos2(r.left() + i as f32 * bw + bw * 0.18, r.bottom() - hh), vec2(bw * 0.64, hh)), bw * 0.3, fg);
            }
        }
        "mirror" => {
            let n = ((w / pitch.max(6.0)) as usize).max(8);
            let half = resample(&an.bands, n.div_ceil(2));
            let v: Vec<f32> = half.iter().rev().chain(half.iter()).copied().collect();
            let bw = w / v.len() as f32;
            let cy = r.center().y;
            for (i, x) in v.iter().enumerate() {
                let hh = (x * h * 0.92).max(2.0);
                p.rect_filled(Rect::from_min_size(pos2(r.left() + i as f32 * bw + bw * 0.2, cy - hh / 2.0), vec2(bw * 0.6, hh)), bw * 0.3, fg);
            }
        }
        "wave" => {
            let n = an.wave.len();
            if n < 2 {
                return;
            }
            let cy = r.center().y;
            let pts: Vec<egui::Pos2> = an.wave.iter().enumerate().map(|(i, s)| pos2(r.left() + i as f32 / (n - 1) as f32 * w, cy - s * h * 0.45)).collect();
            p.add(Shape::line(pts.clone(), Stroke::new(7.0, alpha(fg, 0.18))));
            p.add(Shape::line(pts, Stroke::new(2.0, fg)));
        }
        "ring" => {
            let c = r.center();
            let rr = w.min(h) * 0.27;
            let n = 72;
            let half = resample(&an.bands, n / 2);
            let v: Vec<f32> = half.iter().chain(half.iter().rev()).copied().collect();
            if let Some((id, _)) = cover {
                let rad = rr * 0.86 * (1.0 + (an.level * 0.05).min(0.06));
                p.add(RectShape::filled(Rect::from_center_size(c, vec2(rad * 2.0, rad * 2.0)), rad, Color32::WHITE).with_texture(id, full_uv()));
            }
            let ext = w.min(h) / 2.0 - rr - 6.0;
            let step = 9.0;
            let rows = (ext / step).max(0.0) as usize;
            for (i, x) in v.iter().enumerate() {
                let a = -PI / 2.0 + i as f32 / n as f32 * 2.0 * PI;
                let k = ((x * ext / step) as usize + 1).max(1);
                for j in 0..rows {
                    let r2 = rr + j as f32 * step;
                    let on = j < k;
                    p.circle_filled(pos2(c.x + r2 * a.cos(), c.y + r2 * a.sin()), if on { 2.2 } else { 1.6 }, if on { fg } else { dim });
                }
            }
        }
        _ => {
            let cols = ((w / pitch) as usize).max(4);
            let rows = ((h / pitch) as usize).max(3);
            let v = resample(&an.bands, cols);
            let pk = resample(&an.peaks, cols);
            let x0 = r.left() + (w - cols as f32 * pitch) / 2.0 + pitch / 2.0;
            let y0 = r.bottom() - (h - rows as f32 * pitch) / 2.0 - pitch / 2.0;
            let d = pitch * 0.46;
            let peak_c = alpha(fg, 0.55);
            for i in 0..cols {
                let lit = (v[i] * rows as f32).round() as usize;
                let peak = ((pk[i] * rows as f32).round() as usize).min(rows - 1);
                let x = x0 + i as f32 * pitch;
                for j in 0..rows {
                    let y = y0 - j as f32 * pitch;
                    let col = if j < lit {
                        fg
                    } else if j == peak && peak > 0 {
                        peak_c
                    } else {
                        dim
                    };
                    p.circle_filled(pos2(x, y), d / 2.0, col);
                }
            }
        }
    }
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    tick(app, ui);
    let t = app.t.clone();
    let c = t.c;
    let p = ui.painter_at(rect);
    // moving blur of the cover
    let now = ui.input(|i| i.time) as f32;
    let cover = app.current_track().map(|t| t.cover.clone()).unwrap_or_default();
    if let Some(id) = app.covers.blurred(&cover) {
        let lvl = app.analyzer.level.min(1.0);
        let side = rect.width().max(rect.height()) * (1.25 + 0.05 * (now / 7.0).sin() + 0.04 * lvl);
        let cx = rect.center().x + (now / 11.0).sin() * rect.width() * 0.06;
        let cy = rect.center().y + (now / 13.0).cos() * rect.height() * 0.06;
        let angle = ((now / 17.0).sin() * 12.0).to_radians();
        let op = 0.55 + 0.15 * lvl;
        p.add(RectShape::filled(Rect::from_center_size(pos2(cx, cy), vec2(side, side)), 0.0, Color32::from_white_alpha((op * 255.0) as u8)).with_texture(id, full_uv()).with_angle(angle));
        gradient(&p, rect, alpha(c.panel, 0.15), alpha(c.panel, 0.75));
    }
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 24.0), rect.max - vec2(28.0, 22.0));
    let immersive = app.immersive;
    let top_h = 40.0;
    {
        let style = app.cfg.s("vis_style");
        let cov = app.cfg.s("vis_color") == "cover";
        region(ui, Rect::from_min_size(inner.min, vec2(inner.width(), top_h)), Layout::left_to_right(Align::Center), |ui| {
            widgets::eyebrow(ui, &t, "VISUALIZER");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if widgets::pill(ui, &t, "done", Pill::Primary).clicked() {
                    app.acts.push(Act::ToggleVisualizer);
                }
                let imm = if immersive { "exit immersive" } else { "immersive" };
                if widgets::pill(ui, &t, imm, Pill::Normal).clicked() {
                    let on = !app.immersive;
                    app.set_immersive(on);
                }
                ui.add_space(10.0);
                if widgets::chip(ui, &t, if cov { "cover colour" } else { "mono" }, cov, true).clicked() {
                    app.set_cfg("vis_color", if cov { "mono" } else { "cover" });
                }
                ui.add_space(10.0);
                for s in STYLES.iter().rev() {
                    if widgets::chip(ui, &t, s, style == *s, true).clicked() {
                        app.set_cfg("vis_style", *s);
                    }
                }
            });
        });
    }
    let bottom_h = 80.0;
    let vis_r = Rect::from_min_max(pos2(inner.left(), inner.top() + top_h + 10.0), pos2(inner.right(), inner.bottom() - bottom_h - 10.0));
    let resp = ui.interact(vis_r, egui::Id::unique("vis-area"), Sense::click());
    if resp.double_clicked() {
        let on = !app.immersive;
        app.set_immersive(on);
    }
    let fg = vis_color(app);
    let dim = alpha(c.dim, 0.13);
    let style = app.cfg.s("vis_style");
    let ppp = ui.ctx().pixels_per_point();
    let ring_cover = if style == "ring" {
        let rr = vis_r.width().min(vis_r.height()) * 0.27;
        app.covers.get(&cover, rr * 1.7, ppp).map(|id| (id, rr))
    } else {
        None
    };
    paint(&p, vis_r, &app.analyzer, &style, fg, dim, 14.0, ring_cover);
    // the song
    let b = Rect::from_min_max(pos2(inner.left(), inner.bottom() - bottom_h + 8.0), inner.max);
    let cr = Rect::from_min_size(b.min, vec2(72.0, 72.0));
    let cur = app.current_track().cloned();
    if cur.is_some() {
        app.covers.paint(&p, &t, &cover, cr);
    }
    let big = t.font(W::Display, 29.0);
    let tg = galley(ui, &fmt_time(app.pos), big, c.text);
    p.galley(pos2(b.right() - tg.size().x, b.bottom() - tg.size().y), tg.clone(), c.text);
    region(ui, Rect::from_min_max(pos2(cr.right() + 16.0, b.top()), pos2(b.right() - tg.size().x - 16.0, b.bottom())), Layout::top_down(Align::Min), |ui| {
        ui.add_space(14.0);
        match &cur {
            Some(tr) => {
                widgets::label(ui, &tr.title, t.font(W::Bold, 6.0), c.text, Sense::hover());
                widgets::dim(ui, &t, &format!("{} — {}", tr.artist, tr.album));
            }
            None => {
                widgets::label(ui, &t.l("nothing playing"), t.font(W::Bold, 6.0), c.text, Sense::hover());
            }
        }
    });
}
