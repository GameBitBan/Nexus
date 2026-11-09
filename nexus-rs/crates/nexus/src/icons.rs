//! The vector icons (same drawings as the Python version's IconButton), painted with egui shapes.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke, QuadraticBezierShape};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Shuffle,
    Repeat,
    RepeatOne,
    Volume,
    Mute,
    Back,
    Columns,
    Grid,
    List,
    Queue,
    Plus,
    Home,
    Search,
    Gear,
    Refresh,
    Panel,
    Eq,
    Lyrics,
    Download,
    Vis,
    Discover,
    Jam,
}

impl Icon {
    /// Icons drawn as outlines (the "extra" set): dim until hovered/active.
    pub fn is_extra(self) -> bool {
        use Icon::*;
        matches!(self, Columns | Grid | List | Queue | Plus | Home | Search | Gear | Refresh | Panel | Eq | Lyrics | Download | Vis | Discover | Jam)
    }

    pub fn is_toggle(self) -> bool {
        matches!(self, Icon::Shuffle | Icon::Repeat | Icon::RepeatOne | Icon::Mute)
    }

    /// Text-mode label (control_style = text).
    pub fn text_key(self) -> &'static str {
        use Icon::*;
        match self {
            Play => "play",
            Pause => "pause",
            Prev => "prev",
            Next => "next",
            Shuffle => "shuffle",
            Repeat => "repeat",
            RepeatOne => "repeat_one",
            Volume => "vol",
            Mute => "mute",
            Back => "<",
            Columns => "browse",
            Grid => "albums",
            List => "songs",
            Queue => "queue",
            Plus => "+",
            Home => "home",
            Search => "/",
            Gear => "settings",
            Refresh => "rescan",
            Panel => "panel",
            Eq => "eq",
            Lyrics => "lyrics",
            Download => "download",
            Vis => "vis",
            Discover => "discover",
            Jam => "jam",
        }
    }

    /// True for the transport glyphs that come from the `glyphs` setting in text mode.
    pub fn uses_glyph(self) -> bool {
        use Icon::*;
        matches!(self, Play | Pause | Prev | Next | Shuffle | Repeat | RepeatOne)
    }
}

/// A line with round caps (egui strokes are butt-ended).
pub fn line(p: &Painter, a: Pos2, b: Pos2, w: f32, col: Color32) {
    p.line_segment([a, b], Stroke::new(w, col));
    p.circle_filled(a, w / 2.0, col);
    p.circle_filled(b, w / 2.0, col);
}

fn polyline(p: &Painter, pts: Vec<Pos2>, w: f32, col: Color32) {
    if let (Some(&a), Some(&b)) = (pts.first(), pts.last()) {
        p.circle_filled(a, w / 2.0, col);
        p.circle_filled(b, w / 2.0, col);
    }
    p.add(Shape::line(pts, Stroke::new(w, col)));
}

fn filled(p: &Painter, pts: Vec<Pos2>, col: Color32) {
    p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
}

/// Points along a circle arc (angles in degrees, Qt convention: 0 = 3 o'clock, counter-clockwise positive).
pub fn arc_points(c: Pos2, r: f32, start_deg: f32, span_deg: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = (start_deg + span_deg * i as f32 / n as f32).to_radians();
            pos2(c.x + r * a.cos(), c.y - r * a.sin())
        })
        .collect()
}

pub fn rounded_rect_outline(p: &Painter, r: Rect, rad: f32, w: f32, col: Color32) {
    p.rect_stroke(r, rad, Stroke::new(w, col), egui::StrokeKind::Middle);
}

pub struct Ctx {
    pub fg: Color32,
    pub bg: Color32,
    pub panel: Color32,
    pub hover: bool,
    pub down: bool,
    pub active: bool,
    pub rail: bool,
    pub volume: f32,
}

/// Draws `icon` centred in `rect` in colour `col` (the caller has already picked it).
pub fn paint(p: &Painter, icon: Icon, rect: Rect, col: Color32, s: f32, k: &Ctx) {
    use Icon::*;
    let (cx, cy) = (rect.center().x, rect.center().y);
    let c = |dx: f32, dy: f32| pos2(cx + s * dx, cy + s * dy);
    let pen = (s / 17.0).max(1.6);
    match icon {
        Columns => {
            let (cw, ch, gap) = (s * 0.13, s * 0.42, s * 0.055);
            let x0 = cx - (cw * 3.0 + gap * 2.0) / 2.0;
            for i in 0..3 {
                let r = Rect::from_min_size(pos2(x0 + i as f32 * (cw + gap), cy - ch / 2.0), vec2(cw, ch));
                rounded_rect_outline(p, r, 1.5, pen, col);
            }
        }
        Grid => {
            let (q, gap) = (s * 0.16, s * 0.07);
            for dx in [-1.0f32, 1.0] {
                for dy in [-1.0f32, 1.0] {
                    let x = cx + if dx > 0.0 { gap / 2.0 } else { -gap / 2.0 - q };
                    let y = cy + if dy > 0.0 { gap / 2.0 } else { -gap / 2.0 - q };
                    rounded_rect_outline(p, Rect::from_min_size(pos2(x, y), vec2(q, q)), 1.8, pen, col);
                }
            }
        }
        List | Queue => {
            for (i, dy) in [-0.15f32, 0.0, 0.15].into_iter().enumerate() {
                let y = cy + s * dy;
                let x1 = cx + s * if icon == List || i < 2 { 0.24 } else { 0.02 };
                line(p, pos2(cx - s * 0.12, y), pos2(x1, y), pen, col);
                if icon == List {
                    p.circle_filled(pos2(cx - s * 0.22, y), pen * 0.75, col);
                }
            }
            if icon == Queue {
                for dy in [-0.15f32, 0.0, 0.15] {
                    line(p, c(-0.24, dy), c(-0.2, dy), pen, col);
                }
                filled(p, vec![c(0.1, 0.07), c(0.1, 0.23), c(0.24, 0.15)], col);
            }
        }
        Plus => {
            line(p, c(-0.17, 0.0), c(0.17, 0.0), pen, col);
            line(p, c(0.0, -0.17), c(0.0, 0.17), pen, col);
        }
        Home => {
            polyline(p, vec![c(-0.21, -0.01), c(0.0, -0.21), c(0.21, -0.01)], pen, col);
            polyline(p, vec![c(-0.15, -0.07), c(-0.15, 0.2), c(0.15, 0.2), c(0.15, -0.07)], pen, col);
        }
        Search => {
            p.circle_stroke(c(-0.04, -0.04), s * 0.14, Stroke::new(pen, col));
            line(p, c(0.07, 0.07), c(0.2, 0.2), pen, col);
        }
        Gear => {
            p.circle_stroke(pos2(cx, cy), s * 0.13, Stroke::new(pen, col));
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                line(p, pos2(cx + a.cos() * s * 0.17, cy + a.sin() * s * 0.17), pos2(cx + a.cos() * s * 0.23, cy + a.sin() * s * 0.23), pen, col);
            }
            p.circle_stroke(pos2(cx, cy), s * 0.035, Stroke::new(pen * 0.8, col));
        }
        Refresh => {
            let r = s * 0.18;
            polyline(p, arc_points(pos2(cx, cy), r, 60.0, 290.0, 28), pen, col);
            let (ax, ay) = (cx + r * 60f32.to_radians().cos(), cy - r * 60f32.to_radians().sin());
            let ah = s * 0.08;
            line(p, pos2(ax, ay), pos2(ax - ah, ay - ah * 0.2), pen, col);
            line(p, pos2(ax, ay), pos2(ax + ah * 0.1, ay - ah), pen, col);
        }
        Download => {
            line(p, c(0.0, -0.2), c(0.0, 0.07), pen, col);
            line(p, c(-0.1, -0.03), c(0.0, 0.07), pen, col);
            line(p, c(0.1, -0.03), c(0.0, 0.07), pen, col);
            polyline(p, vec![c(-0.2, 0.05), c(-0.2, 0.18), c(0.2, 0.18), c(0.2, 0.05)], pen, col);
        }
        Lyrics => {
            let r = Rect::from_min_size(c(-0.22, -0.19), vec2(s * 0.44, s * 0.32));
            rounded_rect_outline(p, r, s * 0.08, pen, col);
            polyline(p, vec![c(-0.1, 0.13), c(-0.16, 0.23), c(0.0, 0.13)], pen, col);
            line(p, c(-0.12, -0.07), c(0.12, -0.07), pen, col);
            line(p, c(-0.12, 0.02), c(0.05, 0.02), pen, col);
        }
        Eq => {
            for (dx, ky) in [(-0.15f32, 0.06f32), (0.0, -0.09), (0.15, 0.03)] {
                let x = cx + s * dx;
                line(p, pos2(x, cy - s * 0.2), pos2(x, cy + s * 0.2), pen, col);
                let knob = pos2(x, cy + s * ky);
                if !k.rail {
                    p.circle_filled(knob, s * 0.05, k.panel);
                }
                p.circle_stroke(knob, s * 0.05, Stroke::new(pen, col));
            }
        }
        Jam => {
            for dx in [-0.1f32, 0.1] {
                let x = cx + s * dx;
                p.circle_stroke(pos2(x, cy - s * 0.08), s * 0.075, Stroke::new(pen, col));
                let st = PathStroke::new(pen, col);
                p.add(QuadraticBezierShape::from_points_stroke(
                    [pos2(x - s * 0.13, cy + s * 0.2), pos2(x - s * 0.13, cy + s * 0.04), pos2(x, cy + s * 0.04)],
                    false,
                    Color32::TRANSPARENT,
                    st.clone(),
                ));
                p.add(QuadraticBezierShape::from_points_stroke(
                    [pos2(x, cy + s * 0.04), pos2(x + s * 0.13, cy + s * 0.04), pos2(x + s * 0.13, cy + s * 0.2)],
                    false,
                    Color32::TRANSPARENT,
                    st,
                ));
            }
        }
        Discover => {
            p.circle_stroke(pos2(cx, cy), s * 0.22, Stroke::new(pen, col));
            p.add(Shape::Path(PathShape::convex_polygon(
                vec![c(0.1, -0.1), c(0.035, 0.035), c(-0.1, 0.1), c(-0.035, -0.035)],
                col,
                Stroke::NONE,
            )));
        }
        Vis => {
            for (dx, hh) in [(-0.18f32, 0.16f32), (-0.06, 0.34), (0.06, 0.24), (0.18, 0.4)] {
                let x = cx + s * dx;
                line(p, pos2(x, cy + s * 0.2), pos2(x, cy + s * 0.2 - s * hh), pen, col);
            }
        }
        Panel => {
            let (rw, rh) = (s * 0.46, s * 0.36);
            let r = Rect::from_center_size(pos2(cx, cy), vec2(rw, rh));
            rounded_rect_outline(p, r, 2.5, pen, col);
            let x = r.right() - rw * 0.34;
            p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(pen, col));
        }
        Play | Pause => {
            let scale = if k.down { 0.94 } else { 1.0 };
            let r = s / 2.0 * scale - 0.5;
            let fill = if !k.hover {
                k.fg
            } else if crate::theme::lightness(k.fg) > 0.5 {
                crate::theme::shade(k.fg, 1.0 / 1.1)
            } else {
                crate::theme::shade(k.fg, 1.4)
            };
            p.circle_filled(pos2(cx, cy), r, fill);
            if icon == Play {
                filled(p, vec![c(-0.11, -0.17), c(-0.11, 0.17), c(0.19, 0.0)], k.bg);
            } else {
                let (bw, bh) = (s * 0.085, s * 0.32);
                for dx in [-s * 0.085, s * 0.085] {
                    p.rect_filled(Rect::from_center_size(pos2(cx + dx, cy), vec2(bw, bh)), 1.5, k.bg);
                }
            }
        }
        Prev | Next => {
            let d = if icon == Prev { -1.0 } else { 1.0 };
            let (th, tw) = (s * 0.30, s * 0.20);
            for off in [-tw / 2.0 - 1.0, tw / 2.0 + 1.0] {
                let bx = cx + off * d;
                filled(p, vec![pos2(bx - tw / 2.0 * d, cy - th / 2.0), pos2(bx - tw / 2.0 * d, cy + th / 2.0), pos2(bx + tw / 2.0 * d, cy)], col);
            }
        }
        Back => {
            let w = (s / 16.0).max(1.6);
            polyline(p, vec![c(0.06, -0.16), c(-0.08, 0.0), c(0.06, 0.16)], w, col);
        }
        Shuffle => {
            let w = (s / 16.0).max(1.6);
            let (l, r, t, b) = (cx - s * 0.24, cx + s * 0.24, cy - s * 0.15, cy + s * 0.15);
            let st = PathStroke::new(w, col);
            p.add(CubicBezierShape::from_points_stroke([pos2(l, b), pos2(cx, b), pos2(cx, t), pos2(r, t)], false, Color32::TRANSPARENT, st.clone()));
            p.add(CubicBezierShape::from_points_stroke([pos2(l, t), pos2(cx, t), pos2(cx, b), pos2(r, b)], false, Color32::TRANSPARENT, st));
            let ah = s * 0.07;
            for yy in [t, b] {
                line(p, pos2(r, yy), pos2(r - ah, yy - ah), w, col);
                line(p, pos2(r, yy), pos2(r - ah, yy + ah), w, col);
            }
        }
        Repeat | RepeatOne => {
            let w = (s / 16.0).max(1.6);
            let (rw, rh) = (s * 0.44, s * 0.28);
            let rect = Rect::from_center_size(pos2(cx, cy), vec2(rw, rh));
            let mut pts = vec![pos2(rect.left() + rw * 0.35, rect.bottom()), pos2(rect.left() + rh / 2.0, rect.bottom())];
            // left cap: from the bottom round to the top (Qt arcTo 270° sweeping -180° = clockwise through 180°)
            pts.extend(arc_points(pos2(rect.left() + rh / 2.0, rect.center().y), rh / 2.0, 270.0, -180.0, 14));
            pts.push(pos2(rect.right() - rh / 2.0, rect.top()));
            pts.extend(arc_points(pos2(rect.right() - rh / 2.0, rect.center().y), rh / 2.0, 90.0, -180.0, 14));
            pts.push(pos2(rect.right() - rw * 0.35, rect.bottom()));
            p.add(Shape::line(pts, Stroke::new(w, col)));
            let ah = s * 0.065;
            let (ax, ay) = (rect.left() + rw * 0.35, rect.bottom());
            line(p, pos2(ax, ay), pos2(ax - ah, ay - ah), w, col);
            line(p, pos2(ax, ay), pos2(ax - ah, ay + ah), w, col);
            let (bx, by) = (rect.right() - rw * 0.35, rect.top());
            line(p, pos2(bx, by), pos2(bx + ah, by - ah), w, col);
            line(p, pos2(bx, by), pos2(bx + ah, by + ah), w, col);
            if icon == RepeatOne {
                let f = egui::FontId::new((s * 0.24).max(7.0), crate::theme::W::Bold.family());
                p.text(pos2(cx, cy), egui::Align2::CENTER_CENTER, "1", f, col);
            }
        }
        Volume | Mute => {
            let w = (s / 16.0).max(1.6);
            let x0 = cx - s * 0.22;
            filled(
                p,
                vec![
                    pos2(x0, cy - s * 0.07),
                    pos2(x0 + s * 0.09, cy - s * 0.07),
                    pos2(x0 + s * 0.2, cy - s * 0.17),
                    pos2(x0 + s * 0.2, cy + s * 0.17),
                    pos2(x0 + s * 0.09, cy + s * 0.07),
                    pos2(x0, cy + s * 0.07),
                ],
                col,
            );
            if icon == Mute {
                let xm = cx + s * 0.12;
                line(p, pos2(xm - s * 0.07, cy - s * 0.07), pos2(xm + s * 0.07, cy + s * 0.07), w, col);
                line(p, pos2(xm - s * 0.07, cy + s * 0.07), pos2(xm + s * 0.07, cy - s * 0.07), w, col);
            } else {
                for (i, rr) in [0.11f32, 0.19].into_iter().enumerate() {
                    if k.volume > if i == 0 { 0.0 } else { 0.5 } {
                        polyline(p, arc_points(pos2(cx - s * 0.02, cy), rr * s, -45.0, 90.0, 12), w, col);
                    }
                }
            }
        }
    }
}
