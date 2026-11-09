//! Small custom widgets: icon buttons, pills, chips, the dot/line/text progress bar, labels that elide.

use crate::icons::{self, Icon};
use crate::theme::{Theme, W, alpha};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, CursorIcon, FontId, Galley, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use std::sync::Arc;

/// One line of text cut with "…" to fit `width`.
pub fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts_mut(|f| f.layout_job(job))
}

pub fn galley(ui: &Ui, text: &str, font: FontId, color: Color32) -> Arc<Galley> {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, color))
}

pub fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    galley(ui, text, font.clone(), Color32::WHITE).size().x
}

/// A label that cuts itself to the available width; returns its response (clickable when asked).
pub fn label(ui: &mut Ui, text: &str, font: FontId, color: Color32, sense: Sense) -> Response {
    let w = ui.available_width();
    let g = elided(ui, text, font, color, w);
    let (rect, resp) = ui.allocate_exact_size(g.size(), sense);
    ui.painter().galley(rect.min, g.clone(), color);
    if sense.senses_click() {
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        if g.elided {
            return resp.on_hover_text(text);
        }
        return resp;
    }
    if g.elided { resp.on_hover_text(text) } else { resp }
}

/// Small grey caps label ("NOW PLAYING", "ALBUM").
pub fn eyebrow(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    label(ui, text, t.font(W::Semi, -1.5), t.c.dim, Sense::hover())
}

pub fn dim(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    label(ui, text, t.body(), t.c.dim, Sense::hover())
}

pub fn small_dim(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    label(ui, text, t.font(W::Regular, -1.5), t.c.dim, Sense::hover())
}

pub fn h1(ui: &mut Ui, t: &Theme, text: &str, sense: Sense) -> Response {
    label(ui, text, t.font(W::Display, 27.0), t.c.text, sense)
}

pub fn h2(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    label(ui, text, t.font(W::Display, 11.0), t.c.text, Sense::hover())
}

// ---------------------------------------------------------------- icon buttons

#[derive(Clone, Copy, Default)]
pub struct IconOpts {
    pub active: bool,
    /// rounded-square tile, filled when active (the nav rail)
    pub rail: bool,
    /// circular chip (back / home / settings)
    pub round: bool,
    pub disabled: bool,
    pub volume: f32,
}

pub fn icon_button(ui: &mut Ui, t: &Theme, icon: Icon, size: f32, o: IconOpts) -> Response {
    let c = t.c;
    if t.text_controls {
        let label = if icon.uses_glyph() { t.b(&t.glyph(icon.text_key())) } else { t.b(icon.text_key()) };
        let font = t.body();
        let g = galley(ui, &label, font.clone(), c.text);
        let sz = g.size() + vec2(10.0, 10.0);
        let (rect, resp) = ui.allocate_exact_size(sz, if o.disabled { Sense::hover() } else { Sense::click() });
        let hover = resp.hovered() && !o.disabled;
        let col = if o.disabled {
            c.border
        } else if hover || (icon.is_toggle() && o.active) || (o.rail && o.active) {
            c.accent
        } else if icon.is_toggle() && !o.active {
            c.dim
        } else {
            c.text
        };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, col);
        return if o.disabled { resp } else { resp.on_hover_cursor(CursorIcon::PointingHand) };
    }
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if o.disabled { Sense::hover() } else { Sense::click() });
    paint_icon_button(ui, t, icon, rect, &resp, o);
    if o.disabled { resp } else { resp.on_hover_cursor(CursorIcon::PointingHand) }
}

pub fn paint_icon_button(ui: &Ui, t: &Theme, icon: Icon, rect: Rect, resp: &Response, o: IconOpts) {
    let c = t.c;
    let p = ui.painter();
    let mut hover = resp.hovered();
    let mut fg = c.text;
    if o.disabled {
        fg = c.border;
        hover = false;
    }
    let toggle = icon.is_toggle();
    let mut col = if !toggle || o.active { fg } else { c.dim };
    if hover && toggle && !o.active {
        col = fg;
    }
    let mut s = rect.width().min(rect.height());
    if o.rail || o.round {
        if o.rail {
            let rad = t.radius.min(12.0);
            if o.active {
                p.rect_filled(rect, rad, fg);
                col = c.bg;
            } else {
                if hover {
                    p.rect_filled(rect, rad, c.select);
                }
                col = if hover || o.active { fg } else { c.dim };
            }
        } else {
            p.circle_filled(rect.center(), rect.width() / 2.0 - 0.5, if hover { c.select } else { c.panel });
            col = fg;
        }
        s *= 0.8;
    } else if icon.is_extra() {
        col = if hover || o.active { fg } else { c.dim };
    }
    if matches!(icon, Icon::Prev | Icon::Next | Icon::Back) && !hover && !o.disabled {
        col = alpha(fg, 0.85);
    }
    let k = icons::Ctx {
        fg,
        bg: c.bg,
        panel: c.panel,
        hover,
        down: resp.is_pointer_button_down_on(),
        active: o.active,
        rail: o.rail,
        volume: o.volume,
    };
    icons::paint(p, icon, rect, col, s, &k);
    let dot = (toggle && o.active && icon != Icon::Mute)
        || (o.active && !o.rail && !o.round && matches!(icon, Icon::Queue | Icon::Panel | Icon::Eq | Icon::Lyrics | Icon::Vis));
    if dot {
        p.circle_filled(pos2(rect.center().x, rect.bottom() - 2.9), 1.6, col);
    }
}

// ---------------------------------------------------------------- pills and chips

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    Normal,
    Primary,
}

pub fn pill(ui: &mut Ui, t: &Theme, text: &str, kind: Pill) -> Response {
    pill_pad(ui, t, text, kind, 18.0)
}

/// Width a row of pills needs (to pick a tighter padding when space is short).
pub fn pills_width(ui: &Ui, t: &Theme, labels: &[&str], pad: f32, gap: f32) -> f32 {
    let font = if t.pills { t.font(W::Semi, 0.0) } else { t.body() };
    labels.iter().map(|l| text_width(ui, &t.b(l), &font) + if t.pills { pad * 2.0 } else { 8.0 }).sum::<f32>() + gap * (labels.len().max(1) - 1) as f32
}

pub fn pill_pad(ui: &mut Ui, t: &Theme, text: &str, kind: Pill, pad: f32) -> Response {
    let c = t.c;
    let label = t.b(text);
    if !t.pills {
        // flat text button; the primary one in the accent colour
        let font = t.body();
        let g = galley(ui, &label, font.clone(), c.text);
        let (rect, resp) = ui.allocate_exact_size(g.size() + vec2(8.0, 6.0), Sense::click());
        let col = if resp.hovered() || kind == Pill::Primary { c.accent } else { c.text };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, col);
        return resp.on_hover_cursor(CursorIcon::PointingHand);
    }
    let font = t.font(W::Semi, 0.0);
    let g = galley(ui, &label, font.clone(), c.text);
    let size = vec2(g.size().x + pad * 2.0, g.size().y + 14.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let rad = size.y / 2.0;
    let hover = resp.hovered();
    let (fill, stroke, col) = match (kind, t.primary) {
        (Pill::Primary, Some((bg, fg, br))) => (if hover { crate::theme::shade(bg, 0.85) } else { bg }, br, fg),
        (Pill::Primary, None) if hover => (c.dim, c.dim, c.bg),
        (Pill::Primary, None) => (c.text, c.text, c.bg),
        (Pill::Normal, _) => (c.panel, if hover { c.dim } else { c.border }, c.text),
    };
    p.rect(rect, rad, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, col);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Toggle chip (visualizer styles, filters): filled when on.
pub fn chip(ui: &mut Ui, t: &Theme, text: &str, on: bool, enabled: bool) -> Response {
    let c = t.c;
    let label = t.l(text);
    let font = t.font(W::Semi, 0.0);
    let g = galley(ui, &label, font.clone(), c.text);
    let size = vec2(g.size().x + 28.0, g.size().y + 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let rad = if t.pills { 15.0f32.min(size.y / 2.0) } else { t.radius.min(8.0) };
    let hover = resp.hovered() && enabled;
    let (fill, stroke, col) = if on {
        (c.text, c.text, c.bg)
    } else if !enabled {
        (Color32::TRANSPARENT, c.border, c.border)
    } else if hover {
        (Color32::TRANSPARENT, c.dim, c.text)
    } else {
        (Color32::TRANSPARENT, c.border, c.dim)
    };
    let p = ui.painter();
    p.rect(rect, rad, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, col);
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

/// Plain text button ("tb" in the Python styles): dim, brightens on hover.
pub fn text_button(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    let label = t.b(text);
    let font = t.body();
    let g = galley(ui, &label, font.clone(), t.c.dim);
    let (rect, resp) = ui.allocate_exact_size(g.size() + vec2(8.0, 6.0), Sense::click());
    let col = if resp.hovered() { t.c.text } else { t.c.dim };
    ui.painter().text(rect.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER, label, font, col);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ---------------------------------------------------------------- progress / volume bar

pub struct BarOut {
    pub resp: Response,
    /// the value while being dragged / clicked
    pub changed: Option<f32>,
    /// the value when the mouse let go (or after a wheel step)
    pub released: Option<f32>,
}

/// Dot-matrix, apple-style line with knob, or characters — `style` is `dots` | `line` | `text`.
pub fn text_bar(ui: &mut Ui, t: &Theme, id: egui::Id, value: f32, style: &str, size: Vec2, wheel: bool) -> BarOut {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let drag_key = id.with("drag");
    let mut dragging: Option<f32> = ui.data(|d| d.get_temp(drag_key));
    let mut changed = None;
    let mut released = None;
    let to_v = |x: f32| ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    if (resp.drag_started() || resp.is_pointer_button_down_on()) && ui.input(|i| i.pointer.primary_down()) {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            let v = to_v(p.x);
            if dragging != Some(v) {
                changed = Some(v);
            }
            dragging = Some(v);
        }
    }
    if dragging.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        released = dragging;
        dragging = None;
    }
    if wheel && resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy.abs() > 0.5 {
            let v = (value + dy / 50.0 * 0.05).clamp(0.0, 1.0);
            changed = Some(v);
            released = Some(v);
        }
    }
    ui.data_mut(|d| match dragging {
        Some(v) => {
            d.insert_temp(drag_key, v);
        }
        None => d.remove::<f32>(drag_key),
    });
    let v = dragging.unwrap_or(value).clamp(0.0, 1.0);
    let active = resp.hovered() || dragging.is_some();
    paint_bar(ui, t, rect, v, style, active);
    BarOut { resp, changed, released }
}

pub fn paint_bar(ui: &Ui, t: &Theme, rect: Rect, v: f32, style: &str, active: bool) {
    let c = t.c;
    let p = ui.painter();
    let fg = c.accent;
    let (w, h) = (rect.width(), rect.height());
    match style {
        "line" => {
            let th = if active { 4.0 } else { 3.0 };
            let knob = if active { 6.0 } else { 0.0 };
            let (x0, x1) = (rect.left() + knob, rect.right() - knob);
            let y = rect.center().y;
            let span = (x1 - x0).max(1.0);
            let bg = alpha(c.dim, 0.35);
            p.rect_filled(Rect::from_min_size(pos2(x0, y - th / 2.0), vec2(span, th)), th / 2.0, bg);
            p.rect_filled(Rect::from_min_size(pos2(x0, y - th / 2.0), vec2(span * v, th)), th / 2.0, fg);
            if knob > 0.0 {
                p.circle_filled(pos2(x0 + span * v, y), knob, fg);
            }
        }
        "dots" => {
            let d = if active { 3.8 } else { 3.2 };
            let gap = 3.4;
            let n = (((w + gap) / (d + gap)) as usize).max(1);
            let filled = (n as f32 * v).round() as usize;
            let x = rect.left() + (w - (n as f32 * d + (n as f32 - 1.0) * gap)) / 2.0;
            let y = rect.center().y;
            let bg = alpha(c.dim, 0.35);
            for i in 0..n {
                p.circle_filled(pos2(x + i as f32 * (d + gap) + d / 2.0, y), d / 2.0, if i < filled { fg } else { bg });
            }
        }
        _ => {
            let font = t.body();
            let chars = &t.progress_chars;
            let fc = chars.first().copied().unwrap_or('━');
            let ec = chars.get(1).copied().unwrap_or(' ');
            let hc = chars.get(2).copied();
            let cw = text_width(ui, &fc.to_string(), &font).max(text_width(ui, &ec.to_string(), &font)).max(1.0);
            let n = ((w / cw) as usize).max(1);
            let filled = (n as f32 * v).round() as usize;
            let mut s1: String = std::iter::repeat_n(fc, filled).collect();
            if let (Some(hc), true) = (hc, filled > 0) {
                s1 = std::iter::repeat_n(fc, filled - 1).collect::<String>();
                s1.push(hc);
            }
            let s2: String = std::iter::repeat_n(ec, n - filled).collect();
            let g1 = galley(ui, &s1, font.clone(), fg);
            let y = rect.top() + (h - g1.size().y) / 2.0;
            let x2 = rect.left() + g1.size().x;
            p.galley(pos2(rect.left(), y), g1, fg);
            p.galley(pos2(x2, y), galley(ui, &s2, font, c.dim), c.dim);
        }
    }
}

// ---------------------------------------------------------------- misc

pub fn hline(ui: &mut Ui, t: &Theme) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, t.c.border);
}

pub fn vline_at(ui: &Ui, t: &Theme, x: f32, top: f32, bottom: f32) {
    ui.painter().rect_filled(Rect::from_min_max(pos2(x, top), pos2(x + 1.0, bottom)), 0.0, t.c.border);
}

/// Toggle switch (settings).
pub fn switch(ui: &mut Ui, t: &Theme, on: &mut bool) -> Response {
    let size = vec2(38.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let k = ui.ctx().animate_bool_responsive(resp.id, *on);
    let c = t.c;
    let p = ui.painter();
    let track = if *on { c.text } else { c.select };
    p.rect(rect, rect.height() / 2.0, track, Stroke::new(1.0, if *on { c.text } else { c.border }), StrokeKind::Inside);
    let r = rect.height() / 2.0 - 3.0;
    let x = egui::lerp(rect.left() + 3.0 + r..=rect.right() - 3.0 - r, k);
    p.circle_filled(pos2(x, rect.center().y), r, if *on { c.bg } else { c.dim });
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Rounded search box (returns the TextEdit response).
pub fn search_box(ui: &mut Ui, t: &Theme, text: &mut String, hint: &str, width: f32, id: egui::Id) -> Response {
    let c = t.c;
    let h = t.px(t.fs) + 20.0;
    let rad = if t.pills { 17.0f32.min(h / 2.0) } else { t.radius };
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(rect, rad, c.panel, Stroke::new(1.0, if focused { c.dim } else { c.border }), StrokeKind::Inside);
    let inner = rect.shrink2(vec2(14.0, 0.0));
    let clear_w = if text.is_empty() { 0.0 } else { 18.0 };
    let edit_rect = Rect::from_min_max(inner.min, pos2(inner.max.x - clear_w, inner.max.y));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(egui::Frame::NONE)
            .hint_text(egui::RichText::new(hint).color(c.dim))
            .font(t.body())
            .text_color(c.text)
            .desired_width(edit_rect.width())
            .margin(egui::Margin::ZERO),
    );
    if !text.is_empty() {
        let xr = Rect::from_center_size(pos2(inner.max.x - 7.0, rect.center().y), vec2(16.0, 16.0));
        let xresp = ui.interact(xr, id.with("clear"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let col = if xresp.hovered() { c.text } else { c.dim };
        let p = ui.painter();
        let d = 4.0;
        p.line_segment([xr.center() + vec2(-d, -d), xr.center() + vec2(d, d)], Stroke::new(1.5, col));
        p.line_segment([xr.center() + vec2(-d, d), xr.center() + vec2(d, -d)], Stroke::new(1.5, col));
        if xresp.clicked() {
            text.clear();
        }
    }
    resp
}

/// Rect helper: centre a size inside a rect.
pub fn centered(rect: Rect, size: Vec2) -> Rect {
    Rect::from_center_size(rect.center(), size)
}

pub fn point_in_circle(p: Pos2, c: Pos2, r: f32) -> bool {
    p.distance(c) <= r
}
