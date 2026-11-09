//! Settings: a list of sections on the left, the section on the right (same sections and options as
//! the Python version), plus the skins gallery and skin maker.

use crate::acts::Act;
use crate::app::{Nexus, Page};
use crate::layout::region;
use crate::theme::{W, alpha, hex};
use crate::widgets::{self, Pill, elided, galley};
use egui::{Align, Color32, CursorIcon, Layout, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use nexus_core::skins;
use serde_json::{Value, json};
use std::path::PathBuf;

pub const TABS: [&str; 10] = ["look", "skins", "player", "library", "sound", "downloads", "discord", "import", "update", "advanced"];

const LAYOUTS: [(&str, &str); 5] = [
    ("studio", "icon rail, library, record panel"),
    ("classic", "text sidebar with playlists"),
    ("tabs", "navigation across the top, full-width library"),
    ("focus", "big record on the left, library on the right"),
    ("mini", "small player window (ctrl+m)"),
];

#[derive(Default)]
pub struct SkinForm {
    pub loaded: bool,
    pub name: String,
    pub author: String,
    pub description: String,
    pub qss: String,
    pub background: Option<skins::Background>,
    pub bg_label: String,
    pub msg: String,
    pub show_ref: bool,
}

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect, tab: Option<String>) {
    if let Some(tab) = tab {
        if TABS.contains(&tab.as_str()) {
            app.settings_tab = tab;
        }
        app.page = Page::Settings(None);
    }
    let t = app.t.clone();
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 16.0));
    let head_h = 56.0;
    region(ui, Rect::from_min_size(inner.min, vec2(inner.width(), head_h)), Layout::left_to_right(Align::Center), |ui| {
        let g = galley(ui, "Settings", t.font(W::Display, 27.0), t.h1_color.unwrap_or(t.c.text));
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        ui.painter().galley(r.min, g, t.c.text);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::pill(ui, &t, "done", Pill::Primary).clicked() {
                app.acts.push(Act::Back);
            }
        });
    });
    let body = Rect::from_min_max(pos2(inner.left(), inner.top() + head_h + 16.0), inner.max);
    // section list
    let nav = Rect::from_min_size(body.min, vec2(170.0, body.height()));
    region(ui, nav, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for tab in TABS {
            let on = app.settings_tab == tab;
            let font = t.font(W::Semi, 0.0);
            let g = elided(ui, &t.l(tab), font, t.c.text, 140.0);
            let (r, resp) = ui.allocate_exact_size(vec2(170.0, g.size().y + 18.0), Sense::click());
            let col = if on || resp.hovered() { t.c.text } else { t.c.dim };
            if on {
                ui.painter().rect_filled(r, t.radius.min(10.0), t.c.select);
            }
            ui.painter().galley(pos2(r.left() + 14.0, r.center().y - g.size().y / 2.0), g, col);
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                app.settings_tab = tab.to_string();
            }
        }
    });
    let page = Rect::from_min_max(pos2(nav.right() + 28.0, body.top()), body.max);
    let tab = app.settings_tab.clone();
    region(ui, page, Layout::top_down(Align::Min), |ui| {
        let title = tab[..1].to_uppercase() + &tab[1..];
        widgets::label(ui, &title, t.font(W::Display, 11.0), t.c.text, Sense::hover());
        ui.add_space(14.0);
        egui::ScrollArea::vertical().id_salt(("settings", tab.as_str())).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width() - 12.0);
            ui.spacing_mut().item_spacing.y = 12.0;
            match tab.as_str() {
                "look" => look(app, ui),
                "skins" => skins_tab(app, ui),
                "player" => player(app, ui),
                "library" => library(app, ui),
                "sound" => sound(app, ui),
                "downloads" => downloads(app, ui),
                "discord" => discord(app, ui),
                "import" => import(app, ui),
                "update" => update(app, ui),
                _ => advanced(app, ui),
            }
            ui.add_space(30.0);
        });
    });
}

// ------------------------------------------------------------------ form pieces

fn row(app: &Nexus, ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui)) {
    let t = &app.t;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        let g = ui.fonts_mut(|f| f.layout(t.l(label), t.body(), t.c.text, 200.0));
        let h = g.size().y.max(26.0);
        let (r, _) = ui.allocate_exact_size(vec2(200.0, h), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, t.c.text);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            field(ui);
        });
    });
}

fn section(app: &Nexus, ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    widgets::eyebrow(ui, &app.t, &app.t.l(text).to_uppercase());
}

fn note(app: &Nexus, ui: &mut Ui, text: &str) {
    let t = &app.t;
    let w = ui.available_width().min(760.0);
    let g = ui.fonts_mut(|f| f.layout(t.l(text), t.body(), t.c.dim, w));
    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(r.min, g, t.c.dim);
}

fn small(app: &Nexus, ui: &mut Ui, text: &str) {
    let t = &app.t;
    let w = ui.available_width().min(760.0);
    let g = ui.fonts_mut(|f| f.layout(text.to_string(), t.font(W::Regular, -1.5), t.c.dim, w));
    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(r.min, g, t.c.dim);
}

fn toggle(app: &mut Nexus, ui: &mut Ui, label: &str, key: &str) {
    let mut on = app.cfg.b(key);
    let t = app.t.clone();
    let mut changed = false;
    row(app, ui, label, |ui| {
        changed = widgets::switch(ui, &t, &mut on).changed();
    });
    if changed {
        app.set_cfg(key, on);
    }
}

fn spin(app: &mut Nexus, ui: &mut Ui, label: &str, key: &str, lo: i64, hi: i64, step: i64) {
    let mut v = app.cfg.i(key);
    let before = v;
    row(app, ui, label, |ui| {
        ui.add_sized([110.0, 26.0], egui::DragValue::new(&mut v).range(lo..=hi).speed(step as f64 * 0.25).max_decimals(0));
        ui.add_space(-20.0);
    });
    let v = (v / step.max(1)) * step.max(1);
    if v != before {
        app.set_cfg(key, v.clamp(lo, hi));
    }
}

fn combo(app: &mut Nexus, ui: &mut Ui, label: &str, key: &str, options: &[(&str, &str)]) {
    let cur = app.cfg.s(key);
    let mut pick: Option<String> = None;
    let t = app.t.clone();
    row(app, ui, label, |ui| {
        let shown = options.iter().find(|(v, _)| *v == cur).map(|(_, l)| t.l(l)).unwrap_or_else(|| cur.clone());
        egui::ComboBox::from_id_salt(("combo", key)).selected_text(shown).width(260.0).show_ui(ui, |ui| {
            for (v, l) in options {
                if ui.selectable_label(*v == cur, t.l(l)).clicked() {
                    pick = Some(v.to_string());
                }
            }
        });
    });
    if let Some(v) = pick {
        app.set_cfg(key, v);
    }
}

/// Text field that saves when you press enter or click away.
fn line_field(app: &mut Nexus, ui: &mut Ui, key: &str, hint: &str, width: f32, password: bool) -> Option<String> {
    let cur = if password { app.cfg.secret(key) } else { app.cfg.s(key) };
    let buf = app.settings_edit.entry(key.to_string()).or_insert_with(|| cur.clone());
    let resp = ui.add(egui::TextEdit::singleline(buf).desired_width(width).hint_text(hint).password(password).margin(egui::Margin::symmetric(8, 5)));
    if resp.lost_focus() {
        let v = buf.clone();
        app.settings_edit.remove(key);
        if v != cur {
            return Some(v);
        }
    } else if !resp.has_focus() && *buf != cur {
        *buf = cur;
    }
    None
}

fn line(app: &mut Nexus, ui: &mut Ui, label: &str, key: &str, hint: &str) {
    let mut out = None;
    let t = app.t.clone();
    let _ = t;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        label_cell(app, ui, label);
        out = line_field(app, ui, key, hint, 360.0, false);
    });
    if let Some(v) = out {
        app.set_cfg(key, v);
    }
}

fn label_cell(app: &Nexus, ui: &mut Ui, label: &str) {
    let t = &app.t;
    let g = ui.fonts_mut(|f| f.layout(t.l(label), t.body(), t.c.text, 200.0));
    let (r, _) = ui.allocate_exact_size(vec2(200.0, g.size().y.max(26.0)), Sense::hover());
    ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, t.c.text);
}

fn folder_row(app: &mut Nexus, ui: &mut Ui, label: &str, key: &str, hint: &str) {
    let mut out = None;
    let mut pick = false;
    let t = app.t.clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        label_cell(app, ui, label);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            out = line_field(app, ui, key, hint, 300.0, false);
            pick = widgets::pill(ui, &t, "…", Pill::Normal).clicked();
        });
    });
    if pick {
        let start = Some(app.cfg.s(key)).filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(nexus_core::paths::home_dir);
        if let Some(d) = rfd::FileDialog::new().set_directory(start).pick_folder() {
            out = Some(d.to_string_lossy().to_string());
        }
    }
    if let Some(v) = out {
        app.set_cfg(key, v);
    }
}

fn button(app: &Nexus, ui: &mut Ui, text: &str) -> bool {
    widgets::pill(ui, &app.t, text, Pill::Normal).clicked()
}

// ------------------------------------------------------------------ look

fn look(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    let sk = skins::active(&app.cfg);
    let skin_name = match &sk {
        Some(s) => format!("{}{}", s.name, if s.author.is_empty() { String::new() } else { format!("  ·  by {}", s.author) }),
        None => {
            let p = app.cfg.s("preset");
            let p = if p.is_empty() { "custom".to_string() } else { p };
            format!("{}{} (built in)", p[..1].to_uppercase(), &p[1..])
        }
    };
    let mut to_skins = false;
    row(app, ui, "skin", |ui| {
        widgets::label(ui, &skin_name, t.body(), t.c.text, Sense::hover());
        to_skins = widgets::pill(ui, &t, "change skin", Pill::Normal).clicked();
    });
    if to_skins {
        app.settings_tab = "skins".into();
    }
    let cur_layout = app.layout_name();
    let mut new_layout = None;
    row(app, ui, "layout", |ui| {
        egui::ComboBox::from_id_salt("layout").selected_text(t.l(&cur_layout)).width(160.0).show_ui(ui, |ui| {
            for (k, d) in LAYOUTS {
                if ui.selectable_label(cur_layout == k, t.l(k)).on_hover_text(d).clicked() {
                    new_layout = Some(k);
                }
            }
        });
        let d = LAYOUTS.iter().find(|(k, _)| *k == cur_layout).map(|(_, d)| *d).unwrap_or("");
        widgets::small_dim(ui, &t, d);
    });
    if let Some(l) = new_layout {
        app.cfg.set("layout_pref", true);
        app.acts.push(Act::SwitchLayout(l.to_string()));
    }
    toggle(app, ui, "now playing panel", "now_panel");
    toggle(app, ui, "spinning record", "dial_spin");
    spin(app, ui, "record spin (sec/turn)", "dial_spin_seconds", 2, 60, 1);
    spin(app, ui, "record size", "dial_size", 200, 420, 10);
    let themes = crate::theme::theme_names();
    let opts: Vec<(&str, &str)> = themes.iter().map(|s| (s.as_str(), s.as_str())).collect();
    combo(app, ui, "theme", "theme", &opts);

    // colours
    let mut overrides = app.cfg.map("color_overrides");
    let mut changed = false;
    let mut reset = false;
    let c = t.c;
    let cur: [(&str, Color32); 7] =
        [("bg", c.bg), ("panel", c.panel), ("text", c.text), ("dim", c.dim), ("accent", c.accent), ("select", c.select), ("border", c.border)];
    row(app, ui, "colors", |ui| {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(14.0, 8.0);
                for (k, col) in cur {
                    ui.allocate_ui_with_layout(vec2(110.0, 48.0), Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        widgets::small_dim(ui, &t, k);
                        let mut cc = col;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if egui::widgets::color_picker::color_edit_button_srgba(ui, &mut cc, egui::widgets::color_picker::Alpha::Opaque).changed() {
                                overrides.insert(k.to_string(), json!(format!("#{:02x}{:02x}{:02x}", cc.r(), cc.g(), cc.b())));
                                changed = true;
                            }
                            widgets::small_dim(ui, &t, &format!("#{:02x}{:02x}{:02x}", col.r(), col.g(), col.b()));
                        });
                    });
                }
            });
            reset = widgets::text_button(ui, &t, "reset colors to theme").clicked();
        });
    });
    if changed {
        app.set_cfg("color_overrides", Value::Object(overrides));
    }
    if reset {
        app.set_cfg("color_overrides", json!({}));
    }

    // background
    let own = app.cfg.s("custom_background");
    let name = if own.trim().is_empty() {
        t.l("none (the skin's, if it has one)")
    } else {
        std::path::Path::new(own.trim()).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    };
    let (mut choose, mut remove) = (false, false);
    row(app, ui, "background image / video", |ui| {
        choose = widgets::pill(ui, &t, "choose…", Pill::Normal).clicked();
        remove = widgets::pill(ui, &t, "remove", Pill::Normal).clicked();
        widgets::small_dim(ui, &t, &name);
    });
    if choose {
        if let Some(f) = pick_background() {
            let video = skins::is_video(&f);
            app.set_cfg("custom_background", f.to_string_lossy().to_string());
            if video {
                app.toast("video background on — it's silent, loops, and pauses while Nexus is minimised");
            }
        }
    }
    if remove {
        app.set_cfg("custom_background", "");
    }
    spin(app, ui, "background opacity %", "skin_bg_opacity", 5, 100, 5);
    toggle(app, ui, "blur the background", "skin_bg_blur");

    // fonts
    font_row(app, ui, "font", "font_family", true);
    font_row(app, ui, "heading font", "display_font", false);
    spin(app, ui, "font size", "font_size", 7, 32, 1);
    combo(app, ui, "column header case", "header_case", &[("upper", "upper"), ("title", "title"), ("lower", "lower")]);
    combo(app, ui, "ui text case", "ui_case", &[("sentence", "sentence"), ("lower", "lower")]);
    toggle(app, ui, "pill buttons", "pill_buttons");
    spin(app, ui, "window opacity % (restart)", "background_alpha", 20, 100, 5);
    toggle(app, ui, "uppercase ui", "uppercase");
    let br = app.cfg.s("brackets");
    let mut pick: Option<String> = None;
    row(app, ui, "button brackets", |ui| {
        egui::ComboBox::from_id_salt("brackets").selected_text(if br.is_empty() { "none".to_string() } else { br.clone() }).width(120.0).show_ui(ui, |ui| {
            for b in ["none", "[]", "()", "<>", "{}", "||", "«»"] {
                let v = if b == "none" { "" } else { b };
                if ui.selectable_label(br == v, b).clicked() {
                    pick = Some(v.to_string());
                }
            }
        });
    });
    if let Some(v) = pick {
        app.set_cfg("brackets", v);
    }
    toggle(app, ui, "bracket padding", "bracket_padding");
    line(app, ui, "nav marker", "nav_marker", "");
    line(app, ui, "header text", "header_text", "nexus");
    spin(app, ui, "corner radius", "corner_radius", 0, 40, 1);
    spin(app, ui, "album cover size", "cover_size", 64, 400, 8);
    spin(app, ui, "list thumb size (0=off)", "list_cover_size", 0, 96, 2);
    line(app, ui, "no-art placeholder", "placeholder", "");
    spin(app, ui, "sidebar width", "sidebar_width", 120, 500, 10);
    section(app, ui, "background & visualizer");
    toggle(app, ui, "album-art glow behind the app", "ambient_background");
    spin(app, ui, "glow strength %", "ambient_strength", 10, 90, 5);
    toggle(app, ui, "record ring dances to the music", "dial_visualizer");
    combo(app, ui, "visualizer style", "vis_style", &[("dots", "dots"), ("bars", "bars"), ("mirror", "mirror"), ("wave", "wave"), ("ring", "ring")]);
    combo(app, ui, "visualizer colour", "vis_color", &[("mono", "mono (theme colour)"), ("cover", "colour from the album cover")]);
    spin(app, ui, "visualizer sensitivity %", "vis_sensitivity", 40, 250, 10);
}

pub fn pick_background() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("background image or video")
        .set_directory(nexus_core::paths::home_dir())
        .add_filter("Images and videos", &["jpg", "jpeg", "png", "webp", "gif", "mp4", "webm", "mov", "m4v", "mkv"])
        .add_filter("all files", &["*"])
        .pick_file()
}

fn font_row(app: &mut Nexus, ui: &mut Ui, label: &str, key: &str, body: bool) {
    let t = app.t.clone();
    let cur = app.cfg.s(key);
    let shown = if cur.is_empty() { if body { "Inter (default)".to_string() } else { "Doto (default)".to_string() } } else { cur.clone() };
    let fams: Vec<String> = app.sysfonts.as_ref().map(|s| s.families(body && app.mono_only)).unwrap_or_default();
    let mut pick: Option<String> = None;
    let mut mono = app.mono_only;
    let filter = &mut app.font_filter;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        let g = ui.fonts_mut(|f| f.layout(t.l(label), t.body(), t.c.text, 200.0));
        let (r, _) = ui.allocate_exact_size(vec2(200.0, g.size().y.max(26.0)), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, t.c.text);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            egui::ComboBox::from_id_salt(("font", key)).selected_text(shown).width(300.0).height(360.0).show_ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(filter).hint_text("filter…").desired_width(260.0));
                if ui.selectable_label(cur.is_empty(), "default").clicked() {
                    pick = Some(String::new());
                }
                let f = filter.to_lowercase();
                for fam in fams.iter().filter(|x| f.is_empty() || x.to_lowercase().contains(&f)).take(400) {
                    if ui.selectable_label(*fam == cur, fam).clicked() {
                        pick = Some(fam.clone());
                    }
                }
                if fams.is_empty() {
                    ui.label(t.l("looking for installed fonts…"));
                }
            });
            if body {
                let resp = widgets::switch(ui, &t, &mut mono);
                let _ = resp;
                widgets::small_dim(ui, &t, &t.l("mono only"));
            }
        });
    });
    app.mono_only = mono;
    if let Some(v) = pick {
        app.font_filter.clear();
        app.set_cfg(key, v);
    }
}

// ------------------------------------------------------------------ skins

fn skin_card(app: &mut Nexus, ui: &mut Ui, name: &str, sub: &str, colors: &serde_json::Map<String, Value>, bg: Option<PathBuf>, active: bool) -> egui::Response {
    let t = app.t.clone();
    let (rect, resp) = ui.allocate_exact_size(vec2(196.0, 168.0), Sense::click());
    let p = ui.painter().clone();
    let col = |k: &str, d: &str| colors.get(k).and_then(Value::as_str).and_then(hex).unwrap_or_else(|| hex(d).unwrap());
    let prev = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(rect.width() - 8.0, 112.0));
    p.rect_filled(prev, 10.0, col("bg", "#000"));
    if let Some(bgp) = bg {
        let still = if skins::is_video(&bgp) { bgp.with_file_name("poster.jpg") } else { bgp };
        let key = format!("@{}", still.to_string_lossy());
        if let Some(id) = app.covers.file_texture(&key, &still, 200) {
            crate::covers::paint_texture(&p, id, prev, 10.0, Color32::from_white_alpha(178));
        }
    }
    let rr = t.radius.min(8.0) / 2.0;
    let panel = col("panel", "#111");
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(prev.min + vec2(x - 4.0, y - 4.0), vec2(w, h));
    p.rect_filled(at(10.0, 10.0, 18.0, 92.0), rr, panel);
    p.rect_filled(at(32.0, 10.0, 104.0, 92.0), rr, panel);
    p.rect_filled(at(140.0, 10.0, prev.width() - 140.0, 92.0), rr, panel);
    for i in 0..3 {
        p.rect_filled(at(38.0 + i as f32 * 32.0, 18.0, 26.0, 26.0), rr, col("select", "#222"));
    }
    p.rect_filled(at(38.0, 52.0, 60.0, 5.0), 2.5, col("text", "#fff"));
    p.rect_filled(at(38.0, 62.0, 42.0, 4.0), 2.0, col("dim", "#888"));
    p.rect_filled(at(38.0, 72.0, 80.0, 4.0), 2.0, col("dim", "#888"));
    let cx = prev.left() - 4.0 + 140.0 + (prev.width() - 140.0) / 2.0 + 2.0;
    p.circle_filled(pos2(cx, prev.top() - 4.0 + 42.0), 17.0, col("accent", "#fff"));
    p.circle_filled(pos2(cx, prev.top() - 4.0 + 42.0), 4.0, panel);
    p.circle_filled(pos2(cx, prev.top() - 4.0 + 86.0), 6.0, col("text", "#fff"));
    if active {
        p.rect_stroke(prev.expand(2.0), 12.0, Stroke::new(2.0, t.c.text), StrokeKind::Middle);
    } else if resp.hovered() {
        p.rect_stroke(prev.expand(2.0), 12.0, Stroke::new(1.0, alpha(t.c.text, 0.35)), StrokeKind::Middle);
    }
    let ng = elided(ui, name, t.font(W::Bold, 0.0), t.c.text, rect.width() - 8.0);
    p.galley(pos2(rect.left() + 4.0, rect.top() + 122.0), ng, t.c.text);
    let sg = elided(ui, sub, t.body(), t.c.dim, rect.width() - 8.0);
    p.galley(pos2(rect.left() + 4.0, rect.top() + 144.0), sg, t.c.dim);
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text("click to apply")
}

fn skins_tab(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    note(app, ui, "Skins change Nexus's whole look: colours, fonts, shapes, layout, glyphs, a background image or video and \
                   a little stylesheet. Share them as .nexusskin files — drop one on the window to install it. \
                   Skins can't contain code, so they're safe to import.");
    let (mut import, mut open) = (false, false);
    ui.horizontal(|ui| {
        import = widgets::pill(ui, &t, "import skin file", Pill::Normal).clicked();
        open = widgets::pill(ui, &t, "open skins folder", Pill::Normal).clicked();
        if !app.skin_form.msg.is_empty() {
            widgets::small_dim(ui, &t, &app.skin_form.msg.clone());
        }
    });
    if import {
        if let Some(f) = rfd::FileDialog::new().add_filter("Nexus skins", &["nexusskin", "zip"]).set_directory(nexus_core::paths::home_dir().join("Downloads")).pick_file() {
            app.import_skin(&f);
        }
    }
    if open {
        app.acts.push(Act::OpenFolder(nexus_core::paths::skins_dir().to_string_lossy().to_string()));
    }
    widgets::eyebrow(ui, &t, "YOUR SKINS");
    let cur = app.cfg.s("skin");
    let preset = app.cfg.s("preset");
    let list = skins::list();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
        for name in skins::preset_names() {
            let p = skins::preset(&name).unwrap_or_default();
            let theme = p.get("theme").and_then(Value::as_str).unwrap_or("nothing").to_string();
            let colors = theme_colors_map(&theme);
            ui.allocate_ui_with_layout(vec2(196.0, 200.0), Layout::top_down(Align::Min), |ui| {
                let title = name[..1].to_uppercase() + &name[1..];
                if skin_card(app, ui, &title, "built in", &colors, None, cur.is_empty() && preset == name).clicked() {
                    skins::apply_preset(&mut app.cfg, &name);
                    app.after_look_change();
                }
            });
        }
        for m in &list {
            let theme = m.settings.get("theme").and_then(Value::as_str).unwrap_or("nothing").to_string();
            let mut colors = theme_colors_map(&theme);
            colors.extend(m.colors.clone());
            let sub = if !m.author.is_empty() { format!("by {}", m.author) } else if !m.description.is_empty() { m.description.clone() } else { "skin".into() };
            ui.allocate_ui_with_layout(vec2(196.0, 200.0), Layout::top_down(Align::Min), |ui| {
                if skin_card(app, ui, &m.name, &sub, &colors, m.background_path(), m.slug == cur).clicked() {
                    match skins::apply(&mut app.cfg, &m.slug) {
                        Ok(()) => {
                            app.skin_form.loaded = false;
                            app.after_look_change();
                        }
                        Err(e) => app.skin_form.msg = e,
                    }
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if widgets::chip(ui, &t, "export", false, true).clicked() {
                        export_skin(app, m);
                    }
                    if widgets::chip(ui, &t, "delete", false, true).clicked() {
                        app.delete_skin(m);
                    }
                });
            });
        }
    });

    // maker
    ui.add_space(10.0);
    widgets::eyebrow(ui, &t, "MAKE A SKIN");
    note(app, ui, "Set things up how you like in Look (colours, fonts, corner radius, layout, visualizer…), add a background \
                   image or video and a little stylesheet here if you want, then save it as a skin.");
    if !app.skin_form.loaded {
        let sk = skins::active(&app.cfg);
        app.skin_form = SkinForm {
            loaded: true,
            name: sk.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            author: sk.as_ref().map(|s| s.author.clone()).unwrap_or_default(),
            description: sk.as_ref().map(|s| s.description.clone()).unwrap_or_default(),
            qss: sk.as_ref().map(|s| s.qss.clone()).unwrap_or_default(),
            background: None,
            bg_label: sk.as_ref().map(|s| s.background.clone()).filter(|b| !b.is_empty()).unwrap_or_else(|| "none".into()),
            msg: String::new(),
            show_ref: false,
        };
    }
    for (label, hint, which) in [("name", "My skin", 0), ("made by", "your name", 1), ("description", "", 2)] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 28.0;
            label_cell(app, ui, label);
            let f = &mut app.skin_form;
            let s = match which {
                0 => &mut f.name,
                1 => &mut f.author,
                _ => &mut f.description,
            };
            ui.add(egui::TextEdit::singleline(s).hint_text(hint).desired_width(360.0).margin(egui::Margin::symmetric(8, 5)));
        });
    }
    let (mut choose, mut remove) = (false, false);
    let bl = app.skin_form.bg_label.clone();
    row(app, ui, "background", |ui| {
        choose = widgets::pill(ui, &t, "choose image / video", Pill::Normal).clicked();
        remove = widgets::pill(ui, &t, "remove", Pill::Normal).clicked();
        widgets::small_dim(ui, &t, &bl);
    });
    if choose {
        if let Some(f) = pick_background() {
            let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if skins::is_video(&f) {
                if std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0) > 145_000_000 {
                    app.skin_form.msg = "that video is too big for a skin (145 MB max)".into();
                } else {
                    app.skin_form.background = Some(skins::Background::File(f.clone()));
                    app.set_cfg("custom_background", f.to_string_lossy().to_string()); // preview it right away
                    app.skin_form.bg_label = format!("{name} (saved with the skin)");
                }
            } else {
                match std::fs::read(&f).ok().and_then(|b| crate::pages::settings::prepare_image(&b, 2560)) {
                    Some(jpg) => {
                        app.skin_form.background = Some(skins::Background::Bytes(jpg, "jpg"));
                        app.skin_form.bg_label = format!("{name} (saved with the skin)");
                    }
                    None => app.skin_form.msg = "couldn't read that image".into(),
                }
            }
        }
    }
    if remove {
        app.skin_form.background = Some(skins::Background::Remove);
        if !app.cfg.s("custom_background").is_empty() {
            app.set_cfg("custom_background", "");
        }
        app.skin_form.bg_label = "none (saved with the skin)".into();
    }
    spin(app, ui, "background opacity %", "skin_bg_opacity", 5, 100, 5);
    toggle(app, ui, "blur the background", "skin_bg_blur");
    let mut show_ref = app.skin_form.show_ref;
    ui.horizontal(|ui| {
        widgets::label(ui, &t.l("stylesheet"), t.body(), t.c.text, Sense::hover());
        if widgets::pill(ui, &t, "reference", Pill::Normal).clicked() {
            show_ref = !show_ref;
        }
    });
    app.skin_form.show_ref = show_ref;
    if show_ref {
        small(app, ui, "Nexus 2 reads these parts of a skin's Qt stylesheet (the rest is kept for Nexus 1):\n\
                        QWidget#card, QWidget#nowpanel, QWidget#rail { border: 1px solid @accent; }   — a border round the panels\n\
                        QPushButton#pillPrimary { background: @accent; color: @bg; border-color: @accent; }   — the main buttons\n\
                        QLabel#h1 { color: @accent; }   — big headings\n\
                        colour tokens: @bg @panel @text @dim @accent @select @border, or #rrggbb / rgba(r, g, b, a)");
    }
    ui.add(egui::TextEdit::multiline(&mut app.skin_form.qss).desired_rows(6).desired_width(ui.available_width().min(760.0)).code_editor().hint_text("/* optional — e.g.  QWidget#rail { border: 1px solid @accent; } */"));
    let (mut preview, mut save, mut save_export) = (false, false, false);
    ui.horizontal(|ui| {
        preview = widgets::pill(ui, &t, "preview stylesheet", Pill::Normal).clicked();
        save = widgets::pill(ui, &t, "save as skin", Pill::Primary).clicked();
        save_export = widgets::pill(ui, &t, "save + export file", Pill::Normal).clicked();
    });
    if preview {
        let qss = app.skin_form.qss.clone();
        app.cfg.set("custom_qss", qss);
        app.restyle(false);
        app.skin_form.msg = "previewing — save the skin to keep it".into();
    }
    if save || save_export {
        let f = &app.skin_form;
        let name = f.name.trim().to_string();
        if name.is_empty() {
            app.skin_form.msg = "give the skin a name first".into();
            return;
        }
        let (author, desc, qss) = (f.author.trim().to_string(), f.description.trim().to_string(), f.qss.clone());
        let bg = app.skin_form.background.take().unwrap_or(skins::Background::Keep);
        let colors = current_colors(app);
        if app.cfg.s("custom_qss") == qss {
            app.cfg.set("custom_qss", "");
        }
        match skins::save_current(&mut app.cfg, colors, &name, &author, &desc, &qss, bg) {
            Ok(slug) => {
                app.toast(format!("saved skin “{name}”"));
                app.after_look_change();
                if save_export {
                    if let Some(m) = skins::read_dir(&nexus_core::paths::skins_dir().join(slug)) {
                        export_skin(app, &m);
                    }
                }
            }
            Err(e) => app.skin_form.msg = format!("couldn't save: {e}"),
        }
    }
}

fn export_skin(app: &mut Nexus, m: &skins::Skin) {
    let fname = format!("{}.{}", skins::slug(&m.name), skins::SKIN_EXT);
    if let Some(mut f) = rfd::FileDialog::new().set_file_name(&fname).add_filter("Nexus skin", &[skins::SKIN_EXT]).save_file() {
        if f.extension().is_none_or(|e| e != skins::SKIN_EXT) {
            f.set_extension(skins::SKIN_EXT);
        }
        match skins::export(m, &f) {
            Ok(()) => {
                let dir = f.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                app.toast_action(format!("exported “{}”", m.name), "show", move |a| a.acts.push(Act::OpenFolder(dir)), 6.0);
            }
            Err(e) => app.skin_form.msg = format!("export failed: {e}"),
        }
    }
}

/// A picture made into a JPEG no bigger than `max` px (skin backgrounds).
pub fn prepare_image(bytes: &[u8], max: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width().max(img.height()) > max { img.resize(max, max, image::imageops::FilterType::CatmullRom) } else { img };
    let mut out = std::io::Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    img.to_rgb8().write_with_encoder(enc).ok()?;
    Some(out.into_inner())
}

fn theme_colors_map(theme: &str) -> serde_json::Map<String, Value> {
    let mut cfg = nexus_core::config::Config::in_memory();
    cfg.set("theme", theme);
    let c = crate::theme::Colors::from_config(&cfg);
    color_map(&c)
}

fn color_map(c: &crate::theme::Colors) -> serde_json::Map<String, Value> {
    let h = |x: Color32| json!(format!("#{:02x}{:02x}{:02x}", x.r(), x.g(), x.b()));
    [("bg", c.bg), ("panel", c.panel), ("text", c.text), ("dim", c.dim), ("accent", c.accent), ("select", c.select), ("border", c.border)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), h(v)))
        .collect()
}

fn current_colors(app: &Nexus) -> serde_json::Map<String, Value> {
    color_map(&app.t.c)
}

// ------------------------------------------------------------------ player

fn player(app: &mut Nexus, ui: &mut Ui) {
    let styles = [("dots", "dots"), ("line", "line"), ("text", "text")];
    combo(app, ui, "progress bar style", "progress_style", &styles);
    combo(app, ui, "volume bar style", "volume_style", &styles);
    combo(app, ui, "player controls", "control_style", &[("icons", "icons"), ("text", "text")]);
    line(app, ui, "progress chars (text)", "progress_chars", "━─");
    small(app, ui, "filled, empty, optional head — e.g.  ━─   █░   =-   ■□   ━─●");
    for g in ["play", "pause", "prev", "next", "shuffle", "repeat", "repeat_one", "now"] {
        let key = format!("glyph:{g}");
        let cur = app.cfg.map("glyphs").get(g).and_then(Value::as_str).unwrap_or("").to_string();
        let mut out = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 28.0;
            label_cell(app, ui, &format!("text glyph: {g}"));
            let buf = app.settings_edit.entry(key.clone()).or_insert_with(|| cur.clone());
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(120.0).margin(egui::Margin::symmetric(8, 5)));
            if r.lost_focus() && *buf != cur {
                out = Some(buf.clone());
            }
        });
        if let Some(v) = out {
            app.settings_edit.remove(&key);
            let mut gl = app.cfg.map("glyphs");
            gl.insert(g.into(), json!(v));
            app.set_cfg("glyphs", Value::Object(gl));
        }
    }
    toggle(app, ui, "show remaining time", "show_remaining");
    toggle(app, ui, "remember queue on exit", "remember_session");
    let mut out = None;
    let mut browse = false;
    let t = app.t.clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        label_cell(app, ui, "now playing file");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            out = line_field(app, ui, "now_playing_file", "off — e.g. ~/nowplaying.txt (for OBS text sources)", 360.0, false);
            browse = widgets::pill(ui, &t, "…", Pill::Normal).clicked();
        });
    });
    if browse {
        if let Some(p) = rfd::FileDialog::new().set_file_name("nowplaying.txt").set_directory(nexus_core::paths::home_dir()).save_file() {
            out = Some(p.to_string_lossy().to_string());
        }
    }
    if let Some(v) = out {
        app.set_cfg("now_playing_file", v.trim().to_string());
    }
    line(app, ui, "now playing format", "now_playing_format", "{artist} - {title}");
    small(app, ui, "keys: {title} {artist} {album} {year} {quality} {genre}");
}

// ------------------------------------------------------------------ library

fn library(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    widgets::label(ui, &t.l("music folders"), t.body(), t.c.text, Sense::hover());
    let folders = app.cfg.strings("library_folders");
    let sel_id = egui::Id::unique("lib-folder-sel");
    let mut sel: Option<usize> = ui.data(|d| d.get_temp(sel_id));
    egui::Frame::new().fill(t.c.panel).stroke(Stroke::new(1.0, t.c.border)).corner_radius(t.radius.min(9.0) as u8).inner_margin(6.0).show(ui, |ui| {
        ui.set_min_height(120.0);
        ui.set_width(ui.available_width().min(760.0));
        if folders.is_empty() {
            widgets::small_dim(ui, &t, &t.l("no music folders yet"));
        }
        for (i, f) in folders.iter().enumerate() {
            if crate::layout::list_item(ui, &t, f, sel == Some(i)).clicked() {
                sel = Some(i);
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(sel_id, sel));
    let (mut add, mut remove, mut rescan, mut full) = (false, false, false, false);
    ui.horizontal(|ui| {
        add = button(app, ui, "add folder");
        remove = button(app, ui, "remove");
        rescan = button(app, ui, "rescan");
        full = button(app, ui, "full rescan");
    });
    if add {
        app.acts.push(Act::AddFolder);
    }
    if remove {
        if let Some(i) = sel.filter(|&i| i < folders.len()) {
            let mut f = folders.clone();
            f.remove(i);
            app.set_cfg("library_folders", json!(f));
            ui.data_mut(|d| d.remove::<Option<usize>>(sel_id));
        }
    }
    if rescan {
        app.acts.push(Act::Rescan(false));
    }
    if full {
        app.acts.push(Act::Rescan(true));
    }
    toggle(app, ui, "scan on startup", "scan_on_startup");
    toggle(app, ui, "single click plays songs", "single_click_play");
    combo(app, ui, "album sort", "album_sort", &[("artist", "artist"), ("title", "title"), ("year", "year"), ("recent", "recent")]);
    combo(app, ui, "start view", "start_view", &[("albums", "albums"), ("browse", "browse"), ("artists", "artists"), ("songs", "songs"), ("queue", "queue")]);
    let mut cols = app.cfg.strings("song_columns");
    let mut changed = false;
    row(app, ui, "song columns", |ui| {
        for c in ["#", "title", "artist", "album", "year", "genre", "duration", "format"] {
            let mut on = cols.iter().any(|x| x == c);
            if widgets::switch(ui, &t, &mut on).changed() {
                changed = true;
                if on {
                    cols.push(c.to_string());
                } else {
                    cols.retain(|x| x != c);
                }
            }
            widgets::small_dim(ui, &t, c);
            ui.add_space(6.0);
        }
    });
    if changed {
        let order = ["#", "title", "artist", "album", "year", "genre", "duration", "format"];
        cols.sort_by_key(|c| order.iter().position(|o| o == c).unwrap_or(99));
        app.set_cfg("song_columns", json!(cols));
    }
}

// ------------------------------------------------------------------ sound

fn sound(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    let info = app.engine.output_info();
    let status = if info.rate > 0 {
        format!(
            "{} · {} Hz · {} ch · {}{}{}",
            info.device,
            info.rate,
            info.channels,
            info.format,
            if info.exclusive { " · exclusive" } else { "" },
            if info.native_rate { " · bit-perfect rate" } else { " · resampled by the system" }
        )
    } else {
        t.l("Nexus engine ready — starts with the first song")
    };
    row(app, ui, "status", |ui| {
        widgets::dim(ui, &t, &status);
    });
    if app.devices.is_none() {
        app.devices = Some(nexus_core::audio::list_devices());
    }
    let devs = app.devices.clone().unwrap_or_default();
    let cur = app.cfg.s("audio_device");
    let mut pick: Option<String> = None;
    let mut refresh = false;
    row(app, ui, "output device", |ui| {
        egui::ComboBox::from_id_salt("device").selected_text(if cur.is_empty() { t.l("system default") } else { cur.clone() }).width(320.0).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_empty(), t.l("system default")).clicked() {
                pick = Some(String::new());
            }
            for d in &devs {
                if ui.selectable_label(*d == cur, d).clicked() {
                    pick = Some(d.clone());
                }
            }
        });
        refresh = widgets::text_button(ui, &t, "refresh").clicked();
    });
    if refresh {
        app.devices = None;
    }
    if let Some(d) = pick {
        app.set_cfg("audio_device", d);
    }
    if cfg!(windows) {
        toggle(app, ui, "exclusive mode (bit-perfect)", "exclusive_mode");
        small(app, ui, "takes over the device: no windows mixer / resampling. other apps go silent while it plays.");
    }
    let vm = app.cfg.i("volume_max");
    let mut pick_vm = None;
    row(app, ui, "max volume", |ui| {
        egui::ComboBox::from_id_salt("volmax").selected_text(format!("{vm}%")).width(120.0).show_ui(ui, |ui| {
            for p in [100, 150, 200, 300, 400] {
                if ui.selectable_label(vm == p, format!("{p}%")).clicked() {
                    pick_vm = Some(p);
                }
            }
        });
    });
    if let Some(p) = pick_vm {
        app.set_cfg("volume_max", p);
    }
    combo(app, ui, "ReplayGain (even loudness)", "replaygain", &[("off", "off"), ("track", "track"), ("album", "album")]);
    spin(app, ui, "ReplayGain preamp dB", "replaygain_preamp", -12, 12, 1);
    toggle(app, ui, "gapless playback", "gapless");
    let mut eq = false;
    row(app, ui, "equalizer", |ui| {
        eq = widgets::pill(ui, &t, "open equalizer  (ctrl+e)", Pill::Normal).clicked();
    });
    if eq {
        app.acts.push(Act::OpenEq);
    }
}

// ------------------------------------------------------------------ downloads / discord / import / update / advanced

fn later(app: &Nexus, ui: &mut Ui, what: &str) {
    small(app, ui, &format!("{what} arrive{} in a later Nexus 2 build — the settings here are already shared with Nexus 1.", if what.ends_with('s') { "" } else { "s" }));
}

fn downloads(app: &mut Nexus, ui: &mut Ui) {
    widgets::eyebrow(ui, &app.t, "SOULSEEK");
    note(app, ui, "Any username + password works — the first login creates the account. Keep the password: it's how Soulseek knows the name is yours.");
    line(app, ui, "username", "slsk_user", "");
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        label_cell(app, ui, "password");
        out = line_field(app, ui, "slsk_password", "", 360.0, true);
    });
    if let Some(v) = out {
        app.set_cfg("slsk_password", v);
    }
    toggle(app, ui, "share my music folders", "slsk_share_library");
    line(app, ui, "extra shared folders", "slsk_share_extra", "more folders to share, separated by ;");
    let mut shares: Vec<String> = if app.cfg.b("slsk_share_library") { app.cfg.strings("library_folders") } else { Vec::new() };
    shares.extend(app.cfg.s("slsk_share_extra").split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string));
    let txt = if shares.is_empty() { "sharing nothing — many peers refuse to upload to people who don't share".to_string() } else { format!("sharing: {}", shares.join(", ")) };
    small(app, ui, &txt);
    spin(app, ui, "upload slots", "slsk_upload_slots", 1, 20, 1);
    spin(app, ui, "listening port", "slsk_port", 1024, 65000, 1);
    toggle(app, ui, "open port with UPnP", "slsk_upnp");
    later(app, ui, "Soulseek downloads");
    ui.add_space(10.0);
    widgets::eyebrow(ui, &app.t, "IMPORTING");
    folder_row(app, ui, "import into", "import_root", "default: your first music folder");
    toggle(app, ui, "move (not copy) on import", "import_move");
    spin(app, ui, "search time (sec)", "dl_search_seconds", 5, 60, 1);
    toggle(app, ui, "switch to another user if a download hasn't started", "dl_auto_switch");
    spin(app, ui, "…after (minutes)", "dl_stall_minutes", 1, 60, 1);
    ui.add_space(10.0);
    widgets::eyebrow(ui, &app.t, "WATCH FOLDER");
    note(app, ui, "New music files or zips that land here (e.g. your Downloads folder) get sorted into Artist/Album in your library automatically. Files already there are left alone.");
    let dl = nexus_core::paths::home_dir().join("Downloads").to_string_lossy().to_string();
    folder_row(app, ui, "folder", "watch_folder", &dl);
    toggle(app, ui, "watch it", "watch_enabled");
    ui.add_space(10.0);
    widgets::eyebrow(ui, &app.t, "FREE & LEGAL SOURCES");
    note(app, ui, "The Download page can also search the Internet Archive (public-domain recordings, netlabels and the Live Music Archive's concerts in FLAC) and Jamendo (Creative Commons music). No account needed for either.");
    line(app, ui, "Jamendo client id", "jamendo_client_id", "free from devportal.jamendo.com");
}

fn discord(app: &mut Nexus, ui: &mut Ui) {
    note(app, ui, "Show friends what you're playing (“Listening to Nexus”). Needs the Discord desktop app running on this computer. \
                   Not showing on your profile? Discord → User Settings → Activity Privacy → turn on “Share my activity”. \
                   Nothing is sent anywhere except to Discord — and, if the cover is on: either the album cover image to catbox.moe \
                   (once per album), or the artist + album to iTunes/Deezer to find a matching cover.");
    toggle(app, ui, "show on discord", "discord_enabled");
    line(app, ui, "own app id (optional)", "discord_client_id", "empty = Nexus's built-in app");
    toggle(app, ui, "song title in status", "discord_song_in_status");
    toggle(app, ui, "hide when paused", "discord_hide_paused");
    toggle(app, ui, "show album cover", "discord_cover_lookup");
    combo(app, ui, "cover from", "discord_cover_source", &[("upload", "your own cover (exact, uploaded once to catbox.moe)"), ("online", "match it online (itunes / deezer, nothing uploaded)")]);
    line(app, ui, "fallback image asset", "discord_large_image", "nexus");
    later(app, ui, "Discord status");
}

fn import(app: &mut Nexus, ui: &mut Ui) {
    note(app, ui, "bring your playlists, liked songs and followed artists over from Spotify, Apple Music, YouTube Music or another \
                   player. Nexus matches them to the songs you have and can get the missing albums from Soulseek.");
    if widgets::pill(ui, &app.t.clone(), "open import", Pill::Primary).clicked() {
        app.acts.push(Act::Go(Page::Import));
    }
}

fn update(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    widgets::label(ui, &format!("Nexus {}", nexus_core::VERSION), t.font(W::Display, 11.0), t.c.text, Sense::hover());
    note(app, ui, "Nexus 2 updates itself from GitHub releases (checksum-checked). On NixOS, update through nix instead.");
    toggle(app, ui, "check for updates on launch", "update_check");
    toggle(app, ui, "offline mode", "offline_mode");
    small(app, ui, "offline: no cover/lyrics/discover lookups, no update checks, no discord art. Soulseek and Jam still connect when you use them.");
    line(app, ui, "GitHub releases from", "update_repo", "GameBitBan/Nexus");
    line(app, ui, "custom feed (optional)", "update_url", "a version.json url");
    later(app, ui, "Updates");
}

fn advanced(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    note(app, ui, "custom stylesheet — Nexus 2 reads the same parts as in skins (panel borders, the main buttons, heading colour). \
                   See Skins → reference.");
    let key = "custom_qss".to_string();
    let cur = app.cfg.s("custom_qss");
    let buf = app.settings_edit.entry(key.clone()).or_insert(cur);
    ui.add(egui::TextEdit::multiline(buf).desired_rows(8).desired_width(ui.available_width().min(760.0)).code_editor().hint_text("QWidget#card { border: 1px solid @accent; }"));
    let (mut apply, mut open) = (false, false);
    ui.horizontal(|ui| {
        apply = widgets::pill(ui, &t, "apply", Pill::Primary).clicked();
        open = widgets::pill(ui, &t, "open config folder", Pill::Normal).clicked();
    });
    if apply {
        let v = app.settings_edit.remove(&key).unwrap_or_default();
        app.set_cfg("custom_qss", v);
    }
    if open {
        app.acts.push(Act::OpenFolder(nexus_core::paths::data_dir().to_string_lossy().to_string()));
    }
    small(app, ui, &format!("config: {}", app.cfg.path.display()));
}
