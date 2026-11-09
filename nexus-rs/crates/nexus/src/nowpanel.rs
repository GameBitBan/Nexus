//! The now-playing panel: the spinning record wrapped in a dot ring (progress, dancing to the music),
//! song info, the big clock and what's up next.

use crate::acts::Act;
use crate::analyzer::resample;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::{W, alpha};
use crate::widgets::{self, elided, galley};
use egui::epaint::RectShape;
use egui::{Align, CursorIcon, Layout, Rect, Sense, Stroke, Ui, pos2, vec2};
use nexus_core::util::fmt_time;
use std::f32::consts::PI;

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let c = t.c;
    crate::layout::card(app, ui, rect);
    let inner = Rect::from_min_max(rect.min + vec2(22.0, 18.0), rect.max - vec2(22.0, 16.0));
    let cur = app.current_track().cloned();
    region(ui, inner, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        widgets::eyebrow(ui, &t, "NOW PLAYING");
        ui.add_space(12.0);
        let s = app.dial_size();
        ui.horizontal(|ui| {
            ui.add_space(((inner.width() - s) / 2.0).max(0.0));
            dial(app, ui, s);
        });
        ui.add_space(16.0);
        match &cur {
            None => {
                widgets::eyebrow(ui, &t, "IDLE");
                ui.add_space(2.0);
                widgets::label(ui, &t.l("nothing playing"), t.font(W::Bold, 6.0), c.text, Sense::hover());
                widgets::dim(ui, &t, &t.l("pick something on the left"));
            }
            Some(tr) => {
                let n = app.lib.album(&tr.album_key).map(|a| a.tracks.len()).unwrap_or(0);
                let mut bits = Vec::new();
                if tr.track > 0 {
                    bits.push(if n > 0 { format!("TRACK {} / {n}", tr.track) } else { format!("TRACK {}", tr.track) });
                }
                let q = tr.quality();
                if !q.is_empty() {
                    bits.push(q);
                }
                if tr.is_lossy() {
                    bits.push("LOSSY".into());
                }
                widgets::eyebrow(ui, &t, &bits.join("  ·  "));
                ui.add_space(2.0);
                if widgets::label(ui, &tr.title, t.font(W::Bold, 6.0), c.text, Sense::click()).clicked() {
                    app.acts.push(Act::GotoPlayingAlbum);
                }
                if widgets::label(ui, &format!("{} — {}", tr.artist, tr.album), t.body(), c.dim, Sense::click()).clicked() {
                    app.acts.push(Act::GotoPlayingArtist);
                }
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let big = t.font(W::Display, 29.0);
            let tg = galley(ui, &fmt_time(app.pos), big, c.text);
            let total = if app.dur > 0.0 && cur.is_some() { format!("/ {}", fmt_time(app.dur)) } else { String::new() };
            let sg = galley(ui, &total, t.font(W::Display, 9.0), c.dim);
            let h = tg.size().y;
            let (r1, _) = ui.allocate_exact_size(tg.size(), Sense::hover());
            ui.painter().galley(r1.min, tg, c.text);
            let (r2, _) = ui.allocate_exact_size(vec2(sg.size().x, h), Sense::hover());
            ui.painter().galley(pos2(r2.left(), r2.bottom() - sg.size().y - h * 0.08), sg, c.dim);
        });
        ui.add_space(14.0);
        // up next
        let start = app.qi.map(|i| i + 1).unwrap_or(0);
        let next: Vec<(usize, std::sync::Arc<nexus_core::library::Track>)> =
            app.queue.iter().enumerate().skip(start).take(4).map(|(i, t)| (i, t.clone())).collect();
        if !next.is_empty() && cur.is_some() {
            widgets::eyebrow(ui, &t, "UP NEXT");
            ui.add_space(2.0);
            for (k, (qi, tr)) in next.iter().enumerate() {
                let w = ui.available_width();
                let body = t.body();
                let fh = ui.fonts_mut(|f| f.row_height(&body));
                let (r, resp) = ui.allocate_exact_size(vec2(w, fh + 14.0), Sense::click());
                let p = ui.painter();
                let numf = t.font(W::Display, 2.0);
                let num = galley(ui, &format!("{:02}", k + 1), numf, c.dim);
                p.galley(pos2(r.left() + 4.0, r.center().y - num.size().y / 2.0), num, c.dim);
                let time = galley(ui, &fmt_time(tr.duration), t.font(W::Regular, -1.5), c.dim);
                let tw = time.size().x;
                p.galley(pos2(r.right() - 4.0 - tw, r.center().y - time.size().y / 2.0), time, c.dim);
                let col = if resp.hovered() { c.accent } else { c.text };
                let title = elided(ui, &tr.title, body, col, (r.width() - 4.0 - 26.0 - 12.0 - tw - 16.0).max(40.0));
                p.galley(pos2(r.left() + 4.0 + 26.0 + 12.0, r.center().y - title.size().y / 2.0), title, col);
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    app.acts.push(Act::PlayQueueIndex(*qi));
                }
            }
        }
    });
}

/// The record: cover in a circle, rotating while it plays, with the dot ring.
fn dial(app: &mut Nexus, ui: &mut Ui, s: f32) {
    let t = app.t.clone();
    let c = t.c;
    let (rect, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let vis_on = app.cfg.b("dial_visualizer");
    let ctr = rect.center();
    let rr = s / 2.0 - if vis_on { 21.0 } else { 7.0 };
    let rc = rr - 20.0;
    let v = if app.dur > 0.0 && app.current_track().is_some() { (app.pos / app.dur) as f32 } else { 0.0 };
    let n = ((2.0 * PI * rr / 9.5) as usize).max(40);
    let filled = (n as f32 * v).round() as usize;
    let fg = c.text;
    let dim = alpha(c.dim, 0.3);
    let p = ui.painter().clone();

    let playing = app.playing();
    let dance = vis_on && (playing || app.analyzer.active());
    let mut lv = Vec::new();
    if dance {
        app.vis_wanted = true;
        let samples = if playing { Some((app.engine.vis_samples(2048), app.engine.sample_rate())) } else { None };
        let now = ui.input(|i| i.time);
        app.analyzer.tick(now, samples, (app.cfg.f("vis_sensitivity") as f32) / 100.0);
        let half = resample(&app.analyzer.bands, n / 2 + 1);
        lv = (0..n).map(|i| half[i.min(n - i)]).collect();
    }
    for i in 0..n {
        let a = -PI / 2.0 + i as f32 / n as f32 * 2.0 * PI;
        let head = filled > 0 && i == filled - 1;
        let d = if head { 9.0 } else { 4.2 };
        let (ca, sa) = (a.cos(), a.sin());
        p.circle_filled(pos2(ctr.x + rr * ca, ctr.y + rr * sa), d / 2.0, if i < filled { fg } else { dim });
        if dance {
            let k = (lv[i] * 3.2) as usize;
            for j in 1..=k.min(3) {
                let r2 = rr + j as f32 * 6.5;
                let q = alpha(fg, 0.85 - j as f32 * 0.2);
                let dd = 3.6 - j as f32 * 0.4;
                p.circle_filled(pos2(ctr.x + r2 * ca, ctr.y + r2 * sa), dd / 2.0, q);
            }
        }
    }
    let disc = Rect::from_center_size(ctr, vec2(rc * 2.0, rc * 2.0));
    let key = app.current_track().map(|t| t.cover.clone()).unwrap_or_default();
    let ppp = ui.ctx().pixels_per_point();
    match app.covers.get(&key, rc * 2.0, ppp) {
        Some(id) => {
            p.add(RectShape::filled(disc, rc, egui::Color32::WHITE).with_texture(id, crate::covers::full_uv()).with_angle(app.dial_angle.to_radians()));
        }
        None => {
            // the empty record: panel colour with the faint dot grid
            p.circle_filled(ctr, rc, c.panel);
            let step = (rc * 2.0 / 16.0).max(6.0);
            let dot = alpha(c.dim, 0.18);
            let rr2 = (step / 9.0).max(0.7);
            let mut y = ctr.y - rc + step / 2.0;
            while y < ctr.y + rc {
                let mut x = ctr.x - rc + step / 2.0;
                while x < ctr.x + rc {
                    if (x - ctr.x).hypot(y - ctr.y) < rc - step / 2.0 {
                        p.circle_filled(pos2(x, y), rr2, dot);
                    }
                    x += step;
                }
                y += step;
            }
        }
    }
    let hole = rc * 0.16;
    p.circle_filled(ctr, hole, c.panel);
    p.circle_stroke(ctr, rc, Stroke::new(1.0, alpha(c.text, 0.12)));
    p.circle_filled(ctr, rc * 0.05, dim);

    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let (x, y) = (pos.x - ctr.x, pos.y - ctr.y);
            let dist = (x * x + y * y).sqrt();
            if dist >= rr - 16.0 {
                let frac = ((y.atan2(x) + PI / 2.0) / (2.0 * PI)).rem_euclid(1.0);
                app.acts.push(Act::SeekFrac(frac));
            } else if dist <= rc {
                app.acts.push(Act::TogglePlay);
            }
        }
    }
}
