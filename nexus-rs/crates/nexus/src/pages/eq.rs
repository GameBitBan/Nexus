//! Equalizer: preset chips, the curve over a live spectrum (drag the points), and a dot slider per band.

use crate::acts::Act;
use crate::analyzer::resample;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::{W, alpha};
use crate::widgets::{self, Pill, galley};
use egui::epaint::{CubicBezierShape, PathStroke};
use egui::{Align, Align2, Color32, CursorIcon, Layout, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use nexus_core::audio::EqSettings;
use nexus_core::audio::eq::{BANDS, PRESETS};
use serde_json::{Value, json};

struct State {
    on: bool,
    preset: String,
    preamp: f32,
    bands: [f32; 10],
}

fn load(app: &Nexus) -> State {
    let eq = app.cfg.get("eq").cloned().unwrap_or(Value::Null);
    let s = EqSettings::from_json(&eq);
    State {
        on: s.on,
        preset: eq.get("preset").and_then(Value::as_str).unwrap_or("flat").to_string(),
        preamp: s.preamp,
        bands: s.bands,
    }
}

fn store(app: &mut Nexus, st: &State) {
    app.cfg.set("eq", json!({"on": st.on, "preset": st.preset, "preamp": st.preamp, "bands": st.bands}));
    app.engine.set_eq(EqSettings { on: st.on, bands: st.bands, preamp: st.preamp });
    app.eq_save_due = Some(app.ctx.input(|i| i.time) + 0.4);
}

fn snap(v: f32) -> f32 {
    (v.clamp(-12.0, 12.0) * 2.0).round() / 2.0
}

fn preset_of(bands: &[f32; 10]) -> String {
    PRESETS.iter().find(|(_, g)| g == bands).map(|(n, _)| n.to_string()).unwrap_or_else(|| "custom".into())
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    crate::pages::visualizer::tick(app, ui);
    let t = app.t.clone();
    let c = t.c;
    let mut st = load(app);
    let mut changed = false;
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 22.0));
    // header
    region(ui, Rect::from_min_size(inner.min, vec2(inner.width(), 56.0)), Layout::left_to_right(Align::Center), |ui| {
        let g = galley(ui, "Equalizer", t.font(W::Display, 27.0), t.h1_color.unwrap_or(c.text));
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        ui.painter().galley(r.min, g, c.text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if widgets::pill(ui, &t, "done", Pill::Primary).clicked() {
                app.acts.push(Act::Back);
            }
            if widgets::pill(ui, &t, "reset", Pill::Normal).clicked() {
                st.bands = [0.0; 10];
                st.preamp = 0.0;
                st.preset = "flat".into();
                changed = true;
            }
            ui.add_space(8.0);
            widgets::label(ui, &t.l("on"), t.body(), c.text, Sense::hover());
            if widgets::switch(ui, &t, &mut st.on).changed() {
                changed = true;
            }
        });
    });
    // preset chips
    let chips = Rect::from_min_size(pos2(inner.left(), inner.top() + 56.0 + 16.0), vec2(inner.width(), 40.0));
    region(ui, chips, Layout::left_to_right(Align::Center), |ui| {
        egui::ScrollArea::horizontal().id_salt("eq-chips").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (name, gains) in PRESETS {
                    if widgets::chip(ui, &t, name, st.preset == *name, true).clicked() {
                        st.bands = *gains;
                        st.preset = name.to_string();
                        if *name != "flat" {
                            st.on = true;
                        }
                        changed = true;
                    }
                }
                widgets::chip(ui, &t, "custom", st.preset == "custom", false);
            });
        });
    });
    let rest = Rect::from_min_max(pos2(inner.left(), chips.bottom() + 10.0), inner.max);
    let hint_h = 36.0;
    let avail = rest.height() - hint_h - 14.0 - 10.0;
    let graph = Rect::from_min_size(rest.min, vec2(rest.width(), (avail * 5.0 / 9.0).max(170.0)));
    let sliders = Rect::from_min_max(pos2(rest.left(), graph.bottom() + 14.0), pos2(rest.right(), rest.bottom() - hint_h - 10.0));
    if eq_graph(app, ui, graph, &mut st) {
        changed = true;
    }
    // sliders: preamp | 10 bands
    let n = 11;
    let col_w = (sliders.width() - 17.0) / n as f32;
    for i in 0..n {
        let x = sliders.left() + i as f32 * col_w + if i > 0 { 17.0 } else { 0.0 };
        let col = Rect::from_min_size(pos2(x, sliders.top()), vec2(col_w, sliders.height()));
        let label = if i == 0 {
            "preamp".to_string()
        } else {
            let b = BANDS[i - 1] as u32;
            if b >= 1000 { format!("{}k", b / 1000) } else { b.to_string() }
        };
        let v = if i == 0 { st.preamp } else { st.bands[i - 1] };
        let vf = t.font(W::Display, 4.0);
        let vg = galley(ui, &if v == 0.0 { "0".to_string() } else { format!("{v:+}") }, vf, c.text);
        let p = ui.painter();
        p.galley(pos2(col.center().x - vg.size().x / 2.0, col.top()), vg.clone(), c.text);
        let lg = galley(ui, &label.to_uppercase(), t.font(W::Semi, -1.5), c.dim);
        p.galley(pos2(col.center().x - lg.size().x / 2.0, col.bottom() - lg.size().y), lg.clone(), c.dim);
        let sr = Rect::from_min_max(pos2(col.left(), col.top() + vg.size().y + 6.0), pos2(col.right(), col.bottom() - lg.size().y - 6.0));
        if let Some(nv) = dot_slider(ui, &t, egui::Id::unique(("eq-slider", i)), sr, v) {
            if i == 0 {
                st.preamp = nv;
            } else {
                st.bands[i - 1] = nv;
                st.preset = preset_of(&st.bands);
                if st.bands.iter().any(|b| *b != 0.0) {
                    st.on = true;
                }
            }
            changed = true;
        }
        if i == 0 {
            widgets::vline_at(ui, &t, col.right() + 8.0, sliders.top(), sliders.bottom());
        }
    }
    // hint
    let hint = t.l("drag the dots or the curve · scroll on a slider for fine steps · double-click resets a band. \
                    Boosting can clip — the limiter catches it, but a negative preamp sounds cleaner.");
    let g = ui.fonts_mut(|f| f.layout(hint, t.font(W::Regular, -1.5), c.dim, inner.width()));
    ui.painter().galley(pos2(inner.left(), inner.bottom() - g.size().y), g, c.dim);
    if changed {
        store(app, &st);
    }
}

fn eq_graph(app: &mut Nexus, ui: &mut Ui, r: Rect, st: &mut State) -> bool {
    let t = app.t.clone();
    let c = t.c;
    let m = 26.0;
    let (top, bot) = (r.top() + 18.0, r.bottom() - 26.0);
    let (lo, hi) = (20f32.log10(), 20000f32.log10());
    let fx = |f: f32| r.left() + m + (f.log10() - lo) / (hi - lo) * (r.width() - 2.0 * m);
    let dy = |db: f32| top + (12.0 - db) / 24.0 * (bot - top);
    let p = ui.painter().clone();
    // live spectrum behind
    let pitch = 9.0;
    let (x_lo, x_hi) = (fx(32.0), fx(16000.0));
    let cols = (((x_hi - x_lo) / pitch) as usize).max(8);
    let v = resample(&app.analyzer.bands, cols);
    let rows = ((bot - top) / pitch) as usize;
    let q = alpha(c.text, 0.16);
    for (i, x) in v.iter().enumerate() {
        let xx = x_lo + i as f32 * pitch + pitch / 2.0;
        for j in 0..(x * rows as f32) as usize {
            p.circle_filled(pos2(xx, bot - j as f32 * pitch - pitch / 2.0), 1.7, q);
        }
    }
    // grid
    let small = t.font(W::Regular, -2.0);
    for db in [12, 6, 0, -6, -12] {
        let y = dy(db as f32);
        if db == 0 {
            p.line_segment([pos2(r.left() + m, y), pos2(r.right() - m, y)], Stroke::new(1.0, alpha(c.dim, 0.5)));
        } else {
            dotted(&p, pos2(r.left() + m, y), pos2(r.right() - m, y), c.border);
        }
        let lab = if db == 0 { "0".to_string() } else { format!("{db:+}") };
        p.text(pos2(r.left() + m - 4.0, y), Align2::RIGHT_CENTER, lab, small.clone(), c.dim);
    }
    for (f, lab) in [(50.0, "50"), (100.0, "100"), (200.0, "200"), (500.0, "500"), (1000.0, "1k"), (2000.0, "2k"), (5000.0, "5k"), (10000.0, "10k")] {
        let x = fx(f);
        dotted(&p, pos2(x, top), pos2(x, bot), c.border);
        p.text(pos2(x, r.bottom() - 13.0), Align2::CENTER_CENTER, lab, small.clone(), c.dim);
    }
    // curve
    let fg = if st.on { c.text } else { c.dim };
    let pts: Vec<Pos2> = BANDS.iter().zip(st.bands.iter()).map(|(f, g)| pos2(fx(*f), dy((g + st.preamp).clamp(-12.0, 12.0)))).collect();
    let mut fill = vec![pos2(r.left() + m, dy(0.0)), pos2(r.left() + m, pts[0].y), pts[0]];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mx = (a.x + b.x) / 2.0;
        let bez = CubicBezierShape::from_points_stroke([a, pos2(mx, a.y), pos2(mx, b.y), b], false, Color32::TRANSPARENT, PathStroke::new(2.2, fg));
        fill.extend(bez.flatten(Some(0.5)).into_iter().skip(1));
        p.add(bez);
    }
    fill.push(pos2(r.right() - m, pts[9].y));
    fill.push(pos2(r.right() - m, dy(0.0)));
    // the fill isn't convex: draw it as thin vertical strips
    let fc = alpha(fg, 0.08);
    let z = dy(0.0);
    let mut mesh = egui::Mesh::default();
    for w in fill.windows(2).skip(1) {
        let (a, b) = (w[0], w[1]);
        if (b.x - a.x).abs() < 0.01 {
            continue;
        }
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, fc);
        mesh.colored_vertex(b, fc);
        mesh.colored_vertex(pos2(b.x, z), fc);
        mesh.colored_vertex(pos2(a.x, z), fc);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    p.add(Shape::mesh(mesh));
    p.line_segment([pos2(r.left() + m, pts[0].y), pts[0]], Stroke::new(2.2, fg));
    p.line_segment([pts[9], pos2(r.right() - m, pts[9].y)], Stroke::new(2.2, fg));
    for (i, pt) in pts.iter().enumerate() {
        let big = app.eq_drag == Some(i);
        let rr = if big { 7.0 } else { 5.0 };
        p.circle_filled(*pt, rr, fg);
        p.circle_filled(*pt, rr - 2.5, c.panel);
    }
    // dragging a point
    let resp = ui.interact(r, egui::Id::unique("eq-graph"), Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("drag a point to change that band");
    let band_at = |x: f32| -> Option<usize> {
        let (i, d) = BANDS.iter().enumerate().map(|(i, f)| (i, (fx(*f) - x).abs())).min_by(|a, b| a.1.total_cmp(&b.1))?;
        (d < 40.0).then_some(i)
    };
    let db_at = |y: f32| 12.0 - (y - top) / (bot - top).max(1.0) * 24.0 - st.preamp;
    let mut changed = false;
    if resp.double_clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(|p| band_at(p.x)) {
            st.bands[i] = 0.0;
            changed = true;
        }
    } else if resp.is_pointer_button_down_on() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if app.eq_drag.is_none() {
                app.eq_drag = band_at(pos.x);
            }
            if let Some(i) = app.eq_drag {
                let v = snap(db_at(pos.y));
                if v != st.bands[i] {
                    st.bands[i] = v;
                    changed = true;
                }
            }
        }
    } else {
        app.eq_drag = None;
    }
    if changed {
        st.preset = preset_of(&st.bands);
        if st.bands.iter().any(|b| *b != 0.0) {
            st.on = true;
        }
    }
    changed
}

fn dotted(p: &egui::Painter, a: Pos2, b: Pos2, col: Color32) {
    let len = a.distance(b);
    let n = (len / 4.0) as usize;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let t1 = (i as f32 + 0.5) / n as f32;
        p.line_segment([a.lerp(b, t0), a.lerp(b, t1)], Stroke::new(1.0, col));
    }
}

/// Vertical dot-column slider, −12…+12 dB in half-dB steps. Returns the new value when it changed.
fn dot_slider(ui: &mut Ui, t: &crate::theme::Theme, id: egui::Id, r: Rect, v: f32) -> Option<f32> {
    let (lo, hi) = (-12.0f32, 12.0f32);
    let c = t.c;
    let m = 10.0;
    let y_of = |v: f32| r.top() + m + (hi - v) / (hi - lo) * (r.height() - 2.0 * m);
    let resp = ui.interact(r, id, Sense::click_and_drag()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("drag · scroll · double-click resets");
    let mut out = None;
    if resp.double_clicked() {
        out = Some(0.0);
    } else if resp.is_pointer_button_down_on() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let frac = (pos.y - r.top() - m) / (r.height() - 2.0 * m).max(1.0);
            out = Some(snap(hi - frac * (hi - lo)));
        }
    }
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy.abs() > 0.5 {
            out = Some(snap(v + if dy > 0.0 { 0.5 } else { -0.5 }));
        }
    }
    let v_show = out.unwrap_or(v);
    let p = ui.painter();
    let cx = r.center().x;
    let steps = if r.height() < 330.0 { 24 } else { 48 };
    let dimc = alpha(c.dim, 0.3);
    for i in 0..=steps {
        let val = hi - i as f32 * (hi - lo) / steps as f32;
        let y = y_of(val);
        let on = (0.0 <= val && val <= v_show) || (v_show <= val && val <= 0.0);
        let zero = val.abs() < 1e-6;
        let d = if zero { 6.0 } else { 4.2 };
        let col = if on && v_show != 0.0 { c.text } else if zero { c.dim } else { dimc };
        p.circle_filled(pos2(cx, y), d / 2.0, col);
    }
    let yv = y_of(v_show);
    p.circle_filled(pos2(cx, yv), 8.0, c.text);
    p.circle_filled(pos2(cx, yv), 3.0, c.panel);
    out.filter(|nv| *nv != v)
}
