//! Colours, fonts and the text rules (case, brackets) — everything the look settings control.

use egui::{Color32, FontFamily, FontId};
use nexus_core::config::Config;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

const THEMES_JSON: &str = include_str!("../../../assets/themes.json");

pub const INTER_REGULAR: &[u8] = include_bytes!("../../../../fonts/Inter-Regular.ttf");
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../../../fonts/Inter-Medium.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../../fonts/Inter-SemiBold.ttf");
pub const INTER_BOLD: &[u8] = include_bytes!("../../../../fonts/Inter-Bold.ttf");
pub const DOTO_BOLD: &[u8] = include_bytes!("../../../../fonts/Doto-Bold.ttf");

pub const UI_FONTS: &[&str] = &[
    "SF Pro Text", "SF Pro Display", "Inter", "Helvetica Neue", "Segoe UI Variable Text", "Segoe UI", "Cantarell", "Noto Sans",
    "DejaVu Sans",
];
pub const DISPLAY_FONTS: &[&str] = &["Ndot 57", "Ndot 55", "NDot 57", "NType 82", "Doto"];
/// Installed fonts used for glyphs the main fonts lack (box drawing ━─, ◐, ♪, arrows…).
pub const FALLBACK_FONTS: &[&str] = &["DejaVu Sans", "Noto Sans Symbols 2", "Noto Sans Symbols", "Segoe UI Symbol", "Symbola", "Noto Sans", "Arial Unicode MS"];

pub const MONO_FONTS: &[&str] = &[
    "JetBrainsMono Nerd Font", "JetBrains Mono", "Iosevka", "Iosevka Term", "Fira Code", "Cascadia Mono", "Cascadia Code", "Hack",
    "Source Code Pro", "DejaVu Sans Mono", "Consolas", "Menlo", "Monaco",
];

pub fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        6 => Some(Color32::from_rgb(v(0)?, v(2)?, v(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(v(0)?, v(2)?, v(4)?, v(6)?)),
        3 => {
            let d = |i: usize| u8::from_str_radix(s.get(i..i + 1)?, 16).ok().map(|x| x * 17);
            Some(Color32::from_rgb(d(0)?, d(1)?, d(2)?))
        }
        _ => None,
    }
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn lightness(c: Color32) -> f32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    let (mx, mn) = (r.max(g).max(b) as f32, r.min(g).min(b) as f32);
    (mx + mn) / 2.0 / 255.0
}

/// QColor.darker(110) / lighter(140) for the play button's hover.
pub fn shade(c: Color32, factor: f32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let f = |x: u8| ((x as f32) * factor).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(r), f(g), f(b), a)
}

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub bg: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub select: Color32,
    pub border: Color32,
}

pub fn theme_names() -> Vec<String> {
    let v: serde_json::Map<String, Value> = serde_json::from_str(THEMES_JSON).unwrap_or_default();
    v.keys().cloned().collect()
}

impl Colors {
    pub fn from_config(cfg: &Config) -> Self {
        let themes: serde_json::Map<String, Value> = serde_json::from_str(THEMES_JSON).unwrap_or_default();
        let name = cfg.s("theme");
        let t = themes.get(&name).or_else(|| themes.get("midnight")).cloned().unwrap_or(Value::Null);
        let over = cfg.map("color_overrides");
        let pick = |k: &str, dflt: &str| {
            over.get(k)
                .and_then(Value::as_str)
                .and_then(hex)
                .or_else(|| t.get(k).and_then(Value::as_str).and_then(hex))
                .unwrap_or_else(|| hex(dflt).unwrap())
        };
        Colors {
            bg: pick("bg", "#0e0f13"),
            panel: pick("panel", "#14161c"),
            text: pick("text", "#d7dae0"),
            dim: pick("dim", "#6b7080"),
            accent: pick("accent", "#7aa2f7"),
            select: pick("select", "#1d2130"),
            border: pick("border", "#23262f"),
        }
    }

    pub fn is_light(&self) -> bool {
        lightness(self.bg) > 0.5
    }
}

/// Font weights we keep loaded (egui has no weights, so each is its own family).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum W {
    Regular,
    Medium,
    Semi,
    Bold,
    /// headings, the big clock, the brand: the display font (Ndot / Doto), bold
    Display,
}

impl W {
    pub fn family(self) -> FontFamily {
        match self {
            W::Regular => FontFamily::Proportional,
            W::Medium => FontFamily::Name("medium".into()),
            W::Semi => FontFamily::Name("semibold".into()),
            W::Bold => FontFamily::Name("bold".into()),
            W::Display => FontFamily::Name("display".into()),
        }
    }
}

/// Everything about the look that the UI reads every frame, resolved from the config once.
#[derive(Clone)]
pub struct Theme {
    pub c: Colors,
    /// body font size in points (Qt-style pt; 1 pt = 4/3 px)
    pub fs: f32,
    pub radius: f32,
    pub pills: bool,
    pub uppercase: bool,
    pub lower: bool,
    pub brackets: (String, String),
    pub text_controls: bool,
    pub progress_style: String,
    pub volume_style: String,
    pub progress_chars: Vec<char>,
    pub glyphs: HashMap<String, String>,
    pub header_case: String,
    pub placeholder: String,
    pub cover_size: f32,
    pub list_cover_size: f32,
    pub nav_marker: String,
    pub lossy: Color32,
    pub header_text: String,
    /// window opacity 0..1 (background_alpha)
    pub alpha: f32,
    /// from the skin's stylesheet (and the custom one): border around the panels
    pub panel_border: Option<(f32, Color32)>,
    /// primary button colours: (fill, text, border)
    pub primary: Option<(Color32, Color32, Color32)>,
    pub h1_color: Option<Color32>,
}

fn bracket_pair(br: &str, pad: bool) -> (String, String) {
    if br.is_empty() {
        return (String::new(), String::new());
    }
    let parts: Vec<&str> = br.split_whitespace().collect();
    let (l, r) = if br.trim().contains(' ') && parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        let chars: Vec<char> = br.chars().collect();
        if chars.len() >= 2 {
            let h = chars.len() / 2;
            (chars[..h].iter().collect(), chars[h..].iter().collect())
        } else {
            (br.to_string(), br.to_string())
        }
    };
    if pad { (format!("{l} "), format!(" {r}")) } else { (l, r) }
}

impl Theme {
    pub fn from_config(cfg: &Config) -> Self {
        let mut glyphs: HashMap<String, String> =
            [("play", ">"), ("pause", "||"), ("prev", "<<"), ("next", ">>"), ("shuffle", "shuf"), ("repeat", "rep"), ("repeat_one", "rep1"), ("now", "●")]
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();
        for (k, v) in cfg.map("glyphs") {
            if let Some(s) = v.as_str() {
                glyphs.insert(k, s.to_string());
            }
        }
        let chars: Vec<char> = {
            let s = cfg.s("progress_chars");
            if s.is_empty() { "━─".chars().collect() } else { s.chars().collect() }
        };
        Theme {
            c: Colors::from_config(cfg),
            fs: (cfg.f("font_size") as f32).clamp(6.0, 40.0),
            radius: (cfg.f("corner_radius") as f32).clamp(0.0, 40.0),
            pills: cfg.b("pill_buttons"),
            uppercase: cfg.b("uppercase"),
            lower: cfg.s("ui_case") == "lower",
            brackets: bracket_pair(&cfg.s("brackets"), cfg.get("bracket_padding").map(|v| v.as_bool() != Some(false)).unwrap_or(true)),
            text_controls: cfg.s("control_style") == "text",
            progress_style: cfg.s("progress_style"),
            volume_style: cfg.s("volume_style"),
            progress_chars: chars,
            glyphs,
            header_case: cfg.s("header_case"),
            placeholder: cfg.s("placeholder"),
            cover_size: (cfg.f("cover_size") as f32).clamp(60.0, 600.0),
            list_cover_size: (cfg.f("list_cover_size") as f32).clamp(0.0, 200.0),
            nav_marker: cfg.s("nav_marker"),
            lossy: hex(&cfg.s("lossy_color")).unwrap_or(Color32::from_rgb(0xe0, 0xa0, 0x40)),
            header_text: {
                let h = cfg.s("header_text");
                if h.is_empty() { "nexus".into() } else { h }
            },
            alpha: ((cfg.f("background_alpha") as f32) / 100.0).clamp(0.2, 1.0),
            panel_border: None,
            primary: None,
            h1_color: None,
        }
        .with_skin_style(cfg)
    }

    /// The parts of the skin's (and the custom) stylesheet Nexus 2 understands.
    fn with_skin_style(mut self, cfg: &Config) -> Self {
        let c = self.c;
        let hexs = |x: Color32| format!("#{:02x}{:02x}{:02x}", x.r(), x.g(), x.b());
        let colors: serde_json::Map<String, Value> = [
            ("bg", c.bg),
            ("panel", c.panel),
            ("text", c.text),
            ("dim", c.dim),
            ("accent", c.accent),
            ("select", c.select),
            ("border", c.border),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), Value::String(hexs(v))))
        .collect();
        let mut qss = nexus_core::skins::active(cfg).map(|s| s.qss).unwrap_or_default();
        qss.push('\n');
        qss.push_str(&cfg.s("custom_qss"));
        let st = nexus_core::skins::style(&qss, &colors);
        if let Some((w, col, a)) = st.panel_border {
            self.panel_border = hex(&col).map(|x| (w, alpha(x, a)));
        }
        if st.primary_bg.is_some() || st.primary_fg.is_some() {
            let bg = st.primary_bg.as_deref().and_then(hex).unwrap_or(c.text);
            let fg = st.primary_fg.as_deref().and_then(hex).unwrap_or(c.bg);
            let br = st.primary_border.as_deref().and_then(hex).unwrap_or(bg);
            self.primary = Some((bg, fg, br));
        }
        self.h1_color = st.h1_color.as_deref().and_then(hex);
        self
    }

    /// pt → egui points (logical px)
    pub fn px(&self, pt: f32) -> f32 {
        pt * 4.0 / 3.0
    }

    /// Font for `fs + delta` pt.
    pub fn font(&self, w: W, delta: f32) -> FontId {
        FontId::new(self.px((self.fs + delta).max(5.0)), w.family())
    }

    pub fn body(&self) -> FontId {
        self.font(W::Regular, 0.0)
    }

    /// Label text: sentence case / lower / UPPER, like the Python version's `L()`.
    pub fn l(&self, s: &str) -> String {
        if self.uppercase {
            return s.to_uppercase();
        }
        if self.lower {
            return s.to_string();
        }
        for (i, ch) in s.char_indices() {
            if ch.is_alphabetic() {
                let mut out = String::with_capacity(s.len());
                out.push_str(&s[..i]);
                out.extend(ch.to_uppercase());
                out.push_str(&s[i + ch.len_utf8()..]);
                return out;
            }
            if ch.is_alphanumeric() || "“\"'".contains(ch) {
                return s.to_string();
            }
        }
        s.to_string()
    }

    /// Button text: `l()` plus the configured brackets (`[ play ]`).
    pub fn b(&self, s: &str) -> String {
        let s = self.l(s);
        if self.brackets.0.is_empty() && self.brackets.1.is_empty() {
            return s;
        }
        format!("{}{}{}", self.brackets.0, s, self.brackets.1)
    }

    /// Table header text.
    pub fn header(&self, s: &str) -> String {
        if self.uppercase || self.header_case == "upper" {
            return s.to_uppercase();
        }
        if self.header_case == "title" {
            let mut c = s.chars();
            return match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            };
        }
        s.to_string()
    }

    pub fn glyph(&self, k: &str) -> String {
        self.glyphs.get(k).cloned().unwrap_or_else(|| k.to_string())
    }

    /// Covers: corner radius for a cover drawn `size` points wide.
    pub fn cover_radius(&self, size: f32) -> f32 {
        self.radius.min(size / 5.0)
    }
}

// ---------------------------------------------------------------- fonts

/// Finds installed fonts by family name (for the font settings).
pub struct SystemFonts {
    db: fontdb::Database,
}

impl SystemFonts {
    pub fn load() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        SystemFonts { db }
    }

    pub fn families(&self, mono_only: bool) -> Vec<String> {
        let mut v: Vec<String> = self.db.faces().filter(|f| !mono_only || f.monospaced).flat_map(|f| f.families.iter().map(|(n, _)| n.clone())).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup();
        v
    }

    pub fn has(&self, family: &str) -> bool {
        self.db.faces().any(|f| f.families.iter().any(|(n, _)| n == family))
    }

    /// The face of `family` closest to `weight`, as (bytes, index).
    pub fn face(&self, family: &str, weight: u16) -> Option<(Vec<u8>, u32)> {
        let q = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight(weight),
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let id = self.db.query(&q)?;
        self.db.with_face_data(id, |data, idx| (data.to_vec(), idx))
    }
}

fn data(bytes: Vec<u8>, index: u32) -> Arc<egui::FontData> {
    Arc::new(egui::FontData::from_blob(Arc::new(bytes), index))
}

/// Builds egui's fonts: the body font in four weights plus the display font, with egui's own
/// fonts (emoji, symbols) behind them for any glyph they lack.
pub fn font_definitions(cfg: &Config, sys: Option<&SystemFonts>) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let fallbacks: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mono: Vec<String> = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    // body font: the configured family if installed, else the first of UI_FONTS we have (Inter is bundled)
    let wanted = cfg.s("font_family");
    let family = sys
        .and_then(|s| {
            if !wanted.is_empty() && s.has(&wanted) {
                Some(wanted.clone())
            } else {
                UI_FONTS.iter().take_while(|f| **f != "Inter").find(|f| s.has(f)).map(|f| f.to_string())
            }
        })
        .unwrap_or_default();
    let bundled = [(W::Regular, INTER_REGULAR), (W::Medium, INTER_MEDIUM), (W::Semi, INTER_SEMIBOLD), (W::Bold, INTER_BOLD)];
    for (w, inter) in bundled {
        let weight = match w {
            W::Regular => 400,
            W::Medium => 500,
            W::Semi => 600,
            _ => 700,
        };
        let name = format!("body-{weight}");
        let face = if family.is_empty() { None } else { sys.and_then(|s| s.face(&family, weight)) };
        defs.font_data.insert(name.clone(), match face {
            Some((b, i)) => data(b, i),
            None => Arc::new(egui::FontData::from_static(inter)),
        });
        let mut list = vec![name];
        list.extend(fallbacks.iter().cloned());
        defs.families.insert(w.family(), list);
    }

    // display font: configured, else Ndot if installed, else the bundled Doto
    let wanted = cfg.s("display_font");
    let disp = sys.and_then(|s| {
        let pick = if !wanted.is_empty() && s.has(&wanted) {
            Some(wanted.clone())
        } else {
            DISPLAY_FONTS.iter().take_while(|f| **f != "Doto").find(|f| s.has(f)).map(|f| f.to_string())
        };
        pick.and_then(|f| s.face(&f, 700))
    });
    defs.font_data.insert("display".into(), match disp {
        Some((b, i)) => data(b, i),
        None => Arc::new(egui::FontData::from_static(DOTO_BOLD)),
    });
    let mut list = vec!["display".to_string(), "body-700".to_string()];
    list.extend(fallbacks.iter().cloned());
    defs.families.insert(W::Display.family(), list);

    // installed fallbacks go last in every family
    let mut extra = Vec::new();
    if let Some(s) = sys {
        for (i, name) in FALLBACK_FONTS.iter().enumerate() {
            if let Some((b, idx)) = s.has(name).then(|| s.face(name, 400)).flatten() {
                let key = format!("fallback-{i}");
                defs.font_data.insert(key.clone(), data(b, idx));
                extra.push(key);
            }
        }
    }
    for list in defs.families.values_mut() {
        list.extend(extra.iter().cloned());
    }

    // monospace: a real mono font if there is one (terminal look), egui's otherwise
    if let Some((b, i)) = sys.and_then(|s| MONO_FONTS.iter().find(|f| s.has(f)).and_then(|f| s.face(f, 400))) {
        defs.font_data.insert("mono".into(), data(b, i));
        let mut list = vec!["mono".to_string()];
        list.extend(mono);
        list.extend(extra);
        defs.families.insert(FontFamily::Monospace, list);
    }
    defs
}

// ---------------------------------------------------------------- egui style

pub fn apply_style(ctx: &egui::Context, t: &Theme) {
    let c = t.c;
    let r = t.radius;
    ctx.all_styles_mut(|s| {
        let v = &mut s.visuals;
        *v = if c.is_light() { egui::Visuals::light() } else { egui::Visuals::dark() };
        v.override_text_color = Some(c.text);
        v.panel_fill = c.bg;
        v.window_fill = c.panel;
        v.window_stroke = egui::Stroke::new(1.0, c.border);
        v.window_corner_radius = egui::CornerRadius::same(r.min(10.0) as u8);
        v.menu_corner_radius = egui::CornerRadius::same(r.min(10.0) as u8);
        v.window_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) };
        v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(80) };
        v.extreme_bg_color = c.panel;
        v.faint_bg_color = c.select;
        v.code_bg_color = c.select;
        v.hyperlink_color = c.accent;
        v.selection.bg_fill = alpha(c.accent, 0.35);
        v.selection.stroke = egui::Stroke::new(1.0, c.text);
        v.text_cursor.stroke = egui::Stroke::new(1.5, c.text);
        let wr = egui::CornerRadius::same(r.min(9.0) as u8);
        for (w, fill, stroke) in [
            (&mut v.widgets.noninteractive, c.panel, c.border),
            (&mut v.widgets.inactive, c.panel, c.border),
            (&mut v.widgets.hovered, c.select, c.dim),
            (&mut v.widgets.active, c.select, c.text),
            (&mut v.widgets.open, c.select, c.dim),
        ] {
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = egui::Stroke::new(1.0, stroke);
            w.corner_radius = wr;
            w.fg_stroke = egui::Stroke::new(1.0, c.text);
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, c.dim);
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, c.border);

        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.window_margin = egui::Margin::same(10);
        let mut sc = egui::style::ScrollStyle::thin();
        sc.bar_width = 8.0;
        sc.floating = true;
        sc.handle_min_length = 30.0;
        sc.bar_inner_margin = 2.0;
        sc.bar_outer_margin = 2.0;
        sc.foreground_color = true;
        sc.dormant_handle_opacity = 0.25;
        sc.active_handle_opacity = 0.45;
        sc.interact_handle_opacity = 0.6;
        sc.dormant_background_opacity = 0.0;
        sc.active_background_opacity = 0.0;
        sc.interact_background_opacity = 0.0;
        s.spacing.scroll = sc;
        s.interaction.selectable_labels = false;
        s.text_styles = [
            (egui::TextStyle::Body, t.body()),
            (egui::TextStyle::Button, t.body()),
            (egui::TextStyle::Small, t.font(W::Regular, -1.5)),
            (egui::TextStyle::Heading, t.font(W::Display, 11.0)),
            (egui::TextStyle::Monospace, FontId::new(t.px(t.fs), FontFamily::Monospace)),
        ]
        .into();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_split_like_python() {
        assert_eq!(bracket_pair("[]", true), ("[ ".into(), " ]".into()));
        assert_eq!(bracket_pair("<< >>", false), ("<<".into(), ">>".into()));
        assert_eq!(bracket_pair("|", false), ("|".into(), "|".into()));
    }

    #[test]
    fn hex_colours() {
        assert_eq!(hex("#ff8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(hex("fff"), Some(Color32::WHITE));
        assert_eq!(hex("nope"), None);
    }
}
