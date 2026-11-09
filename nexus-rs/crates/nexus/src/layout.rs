//! The window shells: studio, classic, tabs, focus and mini — top bar, nav rail / sidebar, the card
//! the pages live in, the now-playing panel and the player bar. Plus the ambient background and toasts.

use crate::acts::Act;
use crate::app::{Nexus, Page};
use crate::covers;
use crate::icons::Icon;
use crate::theme::{W, alpha};
use crate::widgets::{self, IconOpts, Pill, elided, galley, icon_button, text_width};
use egui::{Align, Align2, Color32, CursorIcon, Layout, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use nexus_core::util::fmt_time;

pub const NAV_ITEMS: [(&str, Icon, &str); 7] = [
    ("albums", Icon::Grid, "albums  (ctrl+1)"),
    ("browse", Icon::Columns, "browse  (ctrl+2)"),
    ("songs", Icon::List, "songs  (ctrl+3)"),
    ("queue", Icon::Queue, "queue  (ctrl+4)"),
    ("discover", Icon::Discover, "discover  (ctrl+5)"),
    ("jam", Icon::Jam, "listen together  (ctrl+j)"),
    ("download", Icon::Download, "download  (ctrl+d)"),
];

pub fn page_for(key: &str) -> Page {
    Page::from_name(key)
}

/// A child Ui confined to `rect`.
pub fn region<R>(ui: &mut Ui, rect: Rect, layout: Layout, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    f(&mut child)
}

pub fn draw(app: &mut Nexus, ui: &mut Ui) {
    let full = ui.max_rect();
    paint_background(app, ui, full);
    if app.immersive {
        crate::pages::draw(app, ui, full);
        draw_toast(app, ui, full, 0.0);
        return;
    }
    let lay = app.layout_name();
    let classic = lay == "classic";
    let bar_h = 88.0;
    let outer = if classic { full } else { Rect::from_min_max(full.min + vec2(8.0, 8.0), full.max - vec2(8.0, 0.0)) };
    let bar = Rect::from_min_max(pos2(full.left(), full.bottom() - bar_h), full.max);
    let body = Rect::from_min_max(outer.min, pos2(outer.right(), bar.top() - if classic { 0.0 } else { 0.0 }));
    match lay.as_str() {
        "classic" => classic_shell(app, ui, body),
        "tabs" => tabs_shell(app, ui, body),
        "mini" => mini_shell(app, ui, body),
        _ => studio_shell(app, ui, body, lay == "focus"),
    }
    npbar(app, ui, bar, classic, lay == "mini");
    draw_toast(app, ui, full, bar_h);
}

// ------------------------------------------------------------------ background

fn paint_background(app: &mut Nexus, ui: &Ui, rect: Rect) {
    let c = app.t.c;
    let p = ui.painter();
    p.rect_filled(rect, 0.0, alpha(c.bg, app.t.alpha));
    if let Some(tex) = &app.bg.tex {
        // cover the window, keep the picture's shape
        let [w, h] = app.bg.size;
        let k = (rect.width() / w.max(1) as f32).max(rect.height() / h.max(1) as f32);
        let r = Rect::from_center_size(rect.center(), vec2(w as f32 * k, h as f32 * k));
        let op = (app.cfg.f("skin_bg_opacity") as f32).clamp(5.0, 100.0) / 100.0;
        p.add(egui::epaint::RectShape::filled(r, 0.0, Color32::from_white_alpha((op * 255.0) as u8)).with_texture(tex.id(), covers::full_uv()));
    }
    if !app.cfg.b("ambient_background") {
        return;
    }
    let strength = (app.cfg.f("ambient_strength") as f32).clamp(0.0, 100.0) / 100.0;
    let now = ui.input(|i| i.time);
    let fade = (((now - app.ambient_t0) / 0.9) as f32).clamp(0.0, 1.0);
    let side = rect.width().max(rect.height());
    let sq = Rect::from_center_size(rect.center(), vec2(side, side));
    let mut draw = |key: String, a: f32| {
        if a <= 0.001 || key.is_empty() {
            return;
        }
        if let Some(id) = app.covers.blurred(&key) {
            let tint = Color32::from_white_alpha((a * 255.0) as u8);
            p.add(egui::epaint::RectShape::filled(sq, 0.0, tint).with_texture(id, covers::full_uv()));
        }
    };
    if fade < 1.0 {
        draw(app.ambient_old.clone(), strength * (1.0 - fade));
    }
    draw(app.ambient_key.clone(), strength * fade);
    // keep the top bar and player bar readable over bright covers
    let light = c.is_light();
    let h = rect.height();
    gradient(p, Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 70.0)), alpha(c.bg, if light { 0.55 } else { 0.35 }), alpha(c.bg, 0.0));
    gradient(p, Rect::from_min_max(pos2(rect.left(), rect.top() + h - 130.0), rect.max), alpha(c.bg, 0.0), alpha(c.bg, if light { 0.75 } else { 0.5 }));
}

/// Vertical gradient from `top` to `bottom` colour.
pub fn gradient(p: &egui::Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(r.left_top(), top);
    mesh.colored_vertex(r.right_top(), top);
    mesh.colored_vertex(r.left_bottom(), bottom);
    mesh.colored_vertex(r.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    p.add(egui::Shape::mesh(mesh));
}

/// Panel colour: see-through when the ambient glow or a background is on, or the window itself is.
pub fn panel_fill(app: &Nexus) -> Color32 {
    let c = app.t.c;
    let mut a = app.t.alpha;
    if app.cfg.b("ambient_background") {
        let glow = (app.cfg.f("ambient_strength") as f32).clamp(0.0, 100.0) / 100.0;
        a = a.min(1.0 - glow * 0.5);
    }
    if app.bg.tex.is_some() {
        a = a.min(0.72);
    }
    alpha(c.panel, a)
}

pub fn card(app: &Nexus, ui: &Ui, r: Rect) {
    let rad = app.t.radius.max(6.0);
    ui.painter().rect_filled(r, rad, panel_fill(app));
    if let Some((w, col)) = app.t.panel_border {
        ui.painter().rect_stroke(r, rad, Stroke::new(w, col), StrokeKind::Inside);
    }
}

// ------------------------------------------------------------------ studio / focus

fn studio_shell(app: &mut Nexus, ui: &mut Ui, body: Rect, focus: bool) {
    let top_h = 44.0;
    let top = Rect::from_min_size(body.min, vec2(body.width(), top_h));
    studio_topbar(app, ui, top);
    let rest = Rect::from_min_max(pos2(body.left(), top.bottom() + 8.0), body.max);
    let rail = Rect::from_min_size(rest.min, vec2(68.0, rest.height()));
    nav_rail(app, ui, rail);
    let mut inner = Rect::from_min_max(pos2(rail.right() + 8.0, rest.top()), rest.max);
    if app.now_panel_visible() {
        let w = app.dial_size() + 44.0;
        let np = if focus {
            let r = Rect::from_min_size(inner.min, vec2(w, inner.height()));
            inner.min.x = r.right() + 8.0;
            r
        } else {
            let r = Rect::from_min_max(pos2(inner.right() - w, inner.top()), inner.max);
            inner.max.x = r.left() - 8.0;
            r
        };
        crate::nowpanel::draw(app, ui, np);
    }
    card(app, ui, inner);
    crate::pages::draw(app, ui, inner);
}

fn brand(app: &Nexus, ui: &mut Ui, width: Option<f32>) {
    let t = &app.t;
    let font = t.font(W::Display, 10.0);
    let text = t.header_text.clone();
    let w = width.unwrap_or_else(|| text_width(ui, &text, &font) + 4.0);
    let g = elided(ui, &text, font, t.c.text, w);
    let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, t.c.text);
}

fn time_label(app: &Nexus, ui: &mut Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    let font = app.t.font(W::Regular, -1.5);
    let g = galley(ui, text, font, app.t.c.dim);
    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(r.min, g, app.t.c.dim);
}

fn count_text(app: &Nexus) -> String {
    format!("{} tracks · {} albums", thousands(app.lib.tracks.len()), thousands(app.lib.albums.len()))
}

pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn search(app: &mut Nexus, ui: &mut Ui, width: f32) {
    let id = egui::Id::unique("search");
    if app.search_focus {
        ui.memory_mut(|m| m.request_focus(id));
        app.search_focus = false;
    }
    let before = app.search.clone();
    let hint = app.t.l("Search songs, artists, albums, formats");
    let t = app.t.clone();
    widgets::search_box(ui, &t, &mut app.search, &hint, width, id);
    if app.search != before && !app.search.is_empty() && app.page != Page::Songs {
        app.go(Page::Songs, true);
    }
}

fn studio_topbar(app: &mut Nexus, ui: &mut Ui, r: Rect) {
    let r = Rect::from_min_max(r.min + vec2(10.0, 0.0), r.max - vec2(6.0, 0.0));
    region(ui, r, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let t = app.t.clone();
        brand(app, ui, Some(190.0));
        let b = icon_button(ui, &t, Icon::Back, 34.0, IconOpts { round: true, disabled: app.history.is_empty(), ..Default::default() });
        if b.clicked() {
            app.acts.push(Act::Back);
        }
        if icon_button(ui, &t, Icon::Home, 44.0, IconOpts { round: true, ..Default::default() }).on_hover_text("home").clicked() {
            app.acts.push(Act::Go(Page::Albums));
        }
        let small = t.font(W::Regular, -1.5);
        let count = count_text(app);
        let right = 40.0 + 6.0 + text_width(ui, &count, &small) + 10.0 + if app.scan_label.is_empty() { 0.0 } else { text_width(ui, &app.scan_label, &small) + 10.0 } + 10.0;
        let sw = (ui.available_width() - right).clamp(120.0, 520.0);
        search(app, ui, sw);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if icon_button(ui, &t, Icon::Gear, 40.0, IconOpts { round: true, ..Default::default() }).on_hover_text("settings  (ctrl+,)").clicked() {
                app.acts.push(Act::OpenSettings);
            }
            ui.add_space(-4.0);
            time_label(app, ui, &count);
            let sl = app.scan_label.clone();
            time_label(app, ui, &sl);
        });
    });
}

fn nav_rail(app: &mut Nexus, ui: &mut Ui, r: Rect) {
    card(app, ui, r);
    let inner = r.shrink(12.0);
    let t = app.t.clone();
    let cur = app.page.nav_key();
    region(ui, inner, Layout::top_down(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        for (key, icon, tip) in NAV_ITEMS {
            let resp = icon_button(ui, &t, icon, 44.0, IconOpts { rail: true, active: cur == key, ..Default::default() });
            if resp.on_hover_text(tip).clicked() {
                app.acts.push(Act::Go(page_for(key)));
            }
        }
        ui.add_space(4.0);
        widgets::hline(ui, &t);
        ui.add_space(4.0);
        if icon_button(ui, &t, Icon::Plus, 44.0, IconOpts { rail: true, ..Default::default() }).on_hover_text("new playlist").clicked() {
            app.acts.push(Act::PlaylistNew(Vec::new()));
        }
        let avail = ui.available_height() - 52.0;
        let (list_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), avail.max(0.0)), Sense::hover());
        region(ui, list_rect, Layout::top_down(Align::Center), |ui| {
            egui::ScrollArea::vertical().id_salt("rail-playlists").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.set_width(list_rect.width());
                ui.spacing_mut().item_spacing.y = 8.0;
                playlist_covers(app, ui);
            });
        });
        ui.add_space(8.0);
        if icon_button(ui, &t, Icon::Refresh, 44.0, IconOpts { rail: true, ..Default::default() }).on_hover_text("rescan library").clicked() {
            app.acts.push(Act::Rescan(false));
        }
    });
}

/// The playlists as cover tiles (rail / tabs strip).
fn playlist_covers(app: &mut Nexus, ui: &mut Ui) {
    let t = app.t.clone();
    for name in app.playlists.names() {
        let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
        let active = app.page == Page::Playlist(name.clone());
        let p = ui.painter().clone();
        let rad = t.radius.min(10.0);
        let cover = app.playlist_cover(&name);
        let ppp = ui.ctx().pixels_per_point();
        match app.covers.get(&cover, 44.0, ppp) {
            Some(id) => covers::paint_texture(&p, id, rect, rad, Color32::WHITE),
            None => {
                p.rect_filled(rect, rad, t.c.select);
                let letter = name.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_else(|| "?".into());
                p.text(rect.center(), Align2::CENTER_CENTER, letter, egui::FontId::new(44.0 * 0.46, W::Display.family()), t.c.text);
            }
        }
        if active || resp.hovered() {
            let col = if active { t.c.text } else { alpha(t.c.text, 0.35) };
            p.rect_stroke(rect.shrink(1.0), rad, Stroke::new(2.0, col), StrokeKind::Middle);
        }
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(&name);
        if resp.clicked() {
            app.acts.push(Act::Go(Page::Playlist(name.clone())));
        }
        playlist_menu(app, &resp, &name);
    }
}

fn playlist_menu(app: &mut Nexus, resp: &egui::Response, name: &str) {
    let t = app.t.clone();
    let lib = app.lib.clone();
    let paths = app.playlists.get(name).cloned().unwrap_or_default();
    let acts = &mut app.acts;
    resp.context_menu(|ui| {
        if ui.button(t.l("play")).clicked() {
            let tracks = paths.iter().filter_map(|p| lib.track(p).cloned()).collect();
            acts.push(Act::PlayList(tracks, 0));
        }
        if ui.button(t.l("rename")).clicked() {
            acts.push(Act::PlaylistRename(name.to_string()));
        }
        if ui.button(t.l("delete")).clicked() {
            acts.push(Act::PlaylistDelete(name.to_string()));
        }
    });
}

// ------------------------------------------------------------------ tabs

fn tabs_shell(app: &mut Nexus, ui: &mut Ui, body: Rect) {
    let top = Rect::from_min_size(body.min, vec2(body.width(), 44.0));
    let t = app.t.clone();
    let cur = app.page.nav_key();
    region(ui, Rect::from_min_max(top.min + vec2(10.0, 0.0), top.max - vec2(6.0, 0.0)), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        brand(app, ui, None);
        ui.add_space(10.0);
        if icon_button(ui, &t, Icon::Back, 34.0, IconOpts { round: true, disabled: app.history.is_empty(), ..Default::default() }).clicked() {
            app.acts.push(Act::Back);
        }
        ui.add_space(6.0);
        for (key, icon, tip) in NAV_ITEMS {
            if icon_button(ui, &t, icon, 40.0, IconOpts { rail: true, active: cur == key, ..Default::default() }).on_hover_text(tip).clicked() {
                app.acts.push(Act::Go(page_for(key)));
            }
        }
        ui.add_space(10.0);
        let small = t.font(W::Regular, -1.5);
        let count = count_text(app);
        let right = 40.0 + 36.0 + 12.0 + text_width(ui, &count, &small) + 12.0 + if app.scan_label.is_empty() { 0.0 } else { text_width(ui, &app.scan_label, &small) + 6.0 };
        let sw = (ui.available_width() - right).max(120.0);
        search(app, ui, sw);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if icon_button(ui, &t, Icon::Gear, 40.0, IconOpts { round: true, ..Default::default() }).on_hover_text("settings  (ctrl+,)").clicked() {
                app.acts.push(Act::OpenSettings);
            }
            if icon_button(ui, &t, Icon::Refresh, 36.0, IconOpts { round: true, ..Default::default() }).on_hover_text("rescan library").clicked() {
                app.acts.push(Act::Rescan(false));
            }
            time_label(app, ui, &count);
            let sl = app.scan_label.clone();
            time_label(app, ui, &sl);
        });
    });
    let strip = Rect::from_min_size(pos2(body.left(), top.bottom() + 8.0), vec2(body.width(), 60.0));
    card(app, ui, strip);
    region(ui, strip.shrink2(vec2(12.0, 8.0)), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if icon_button(ui, &t, Icon::Plus, 44.0, IconOpts { rail: true, ..Default::default() }).on_hover_text("new playlist").clicked() {
            app.acts.push(Act::PlaylistNew(Vec::new()));
        }
        egui::ScrollArea::horizontal().id_salt("strip-playlists").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                playlist_covers(app, ui);
            });
        });
    });
    let mut inner = Rect::from_min_max(pos2(body.left(), strip.bottom() + 8.0), body.max);
    if app.now_panel_visible() {
        let w = app.dial_size() + 44.0;
        let np = Rect::from_min_max(pos2(inner.right() - w, inner.top()), inner.max);
        inner.max.x = np.left() - 8.0;
        crate::nowpanel::draw(app, ui, np);
    }
    card(app, ui, inner);
    crate::pages::draw(app, ui, inner);
}

// ------------------------------------------------------------------ mini

fn mini_shell(app: &mut Nexus, ui: &mut Ui, body: Rect) {
    let t = app.t.clone();
    let top = Rect::from_min_size(body.min, vec2(body.width(), 40.0));
    region(ui, Rect::from_min_max(top.min + vec2(10.0, 4.0), top.max - vec2(6.0, 4.0)), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        brand(app, ui, None);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if widgets::pill(ui, &t, "full", Pill::Primary).clicked() {
                app.acts.push(Act::ToggleMini);
            }
            let pin = if app.pinned { "unpin" } else { "pin on top" };
            if widgets::pill(ui, &t, pin, Pill::Normal).on_hover_text("keep the mini player above other windows").clicked() {
                app.acts.push(Act::TogglePin);
            }
            let sl = app.scan_label.clone();
            time_label(app, ui, &sl);
        });
    });
    let w = app.dial_size() + 44.0;
    let rest = Rect::from_min_max(pos2(body.left(), top.bottom() + 6.0), body.max);
    let np = Rect::from_center_size(pos2(rest.center().x, rest.center().y), vec2(w.min(rest.width()), rest.height()));
    crate::nowpanel::draw(app, ui, np);
}

// ------------------------------------------------------------------ classic

fn classic_shell(app: &mut Nexus, ui: &mut Ui, body: Rect) {
    let t = app.t.clone();
    let c = t.c;
    let sw = (app.cfg.f("sidebar_width") as f32).clamp(140.0, 420.0);
    let side = Rect::from_min_size(body.min, vec2(sw, body.height()));
    ui.painter().rect_filled(side, 0.0, panel_fill(app));
    ui.painter().rect_filled(Rect::from_min_max(pos2(side.right() - 1.0, side.top()), side.max), 0.0, alpha(c.border, 0.8));
    region(ui, Rect::from_min_max(side.min + vec2(14.0, 22.0), side.max - vec2(14.0, 14.0)), Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        brand(app, ui, None);
        ui.add_space(22.0);
        let cur = app.page.nav_key();
        let mk = t.nav_marker.clone();
        for key in ["albums", "browse", "artists", "songs", "queue", "discover", "jam", "download"] {
            let on = cur == key;
            let prefix = if mk.is_empty() { String::new() } else if on { format!("{mk} ") } else { format!("{} ", " ".repeat(mk.chars().count())) };
            if nav_item(ui, &t, &format!("{prefix}{}", t.l(key)), on).clicked() {
                app.acts.push(Act::Go(page_for(key)));
            }
        }
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            widgets::eyebrow(ui, &t, "PLAYLISTS");
        });
        ui.add_space(4.0);
        let list_h = (ui.available_height() - 110.0).max(40.0);
        let (lr, _) = ui.allocate_exact_size(vec2(ui.available_width(), list_h), Sense::hover());
        region(ui, lr, Layout::top_down(Align::Min), |ui| {
            egui::ScrollArea::vertical().id_salt("classic-playlists").show(ui, |ui| {
                ui.set_width(lr.width());
                for name in app.playlists.names() {
                    let on = app.page == Page::Playlist(name.clone());
                    let prefix = if mk.is_empty() { String::new() } else if on { format!("{mk} ") } else { format!("{} ", " ".repeat(mk.chars().count())) };
                    let resp = list_item(ui, &t, &format!("{prefix}{name}"), on);
                    if resp.clicked() {
                        app.acts.push(Act::Go(Page::Playlist(name.clone())));
                    }
                    playlist_menu(app, &resp, &name);
                }
            });
        });
        if widgets::text_button(ui, &t, "+ new playlist").clicked() {
            app.acts.push(Act::PlaylistNew(Vec::new()));
        }
        ui.add_space(8.0);
        if !app.scan_label.is_empty() {
            let sl = app.scan_label.clone();
            widgets::dim(ui, &t, &sl);
        }
        if widgets::text_button(ui, &t, "rescan").clicked() {
            app.acts.push(Act::Rescan(false));
        }
        if widgets::text_button(ui, &t, "settings").clicked() {
            app.acts.push(Act::OpenSettings);
        }
    });

    let mut main = Rect::from_min_max(pos2(side.right(), body.top()), body.max);
    if app.now_panel_visible() {
        let w = app.dial_size() + 44.0;
        let np = Rect::from_min_max(pos2(main.right() - w, main.top()), main.max);
        main.max.x = np.left();
        crate::nowpanel::draw(app, ui, np);
    }
    let top = Rect::from_min_size(main.min, vec2(main.width(), 56.0));
    region(ui, Rect::from_min_max(top.min + vec2(24.0, 14.0), top.max - vec2(28.0, 6.0)), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if icon_button(ui, &t, Icon::Back, 30.0, IconOpts { disabled: app.history.is_empty(), ..Default::default() }).clicked() {
            app.acts.push(Act::Back);
        }
        let small = t.font(W::Regular, -1.5);
        let count = count_text(app);
        let sw = (ui.available_width() - text_width(ui, &count, &small) - 24.0).clamp(120.0, 520.0);
        search(app, ui, sw);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| time_label(app, ui, &count));
    });
    let page = Rect::from_min_max(pos2(main.left(), top.bottom()), main.max);
    crate::pages::draw(app, ui, page);
}

fn nav_item(ui: &mut Ui, t: &crate::theme::Theme, text: &str, on: bool) -> egui::Response {
    let font = t.font(W::Medium, 0.0);
    let w = ui.available_width();
    let g = elided(ui, text, font, t.c.text, w - 16.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y + 14.0), Sense::click());
    let col = if on || resp.hovered() { t.c.text } else { t.c.dim };
    if on {
        ui.painter().rect_filled(r, t.radius.min(9.0), t.c.select);
    }
    ui.painter().galley(pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, col);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn list_item(ui: &mut Ui, t: &crate::theme::Theme, text: &str, on: bool) -> egui::Response {
    let font = t.body();
    let w = ui.available_width();
    let g = elided(ui, text, font, t.c.text, w - 12.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y + 12.0), Sense::click());
    let col = if on || resp.hovered() { t.c.text } else { t.c.dim };
    if on {
        ui.painter().rect_filled(r, t.radius.min(8.0), t.c.select);
    }
    ui.painter().galley(pos2(r.left() + 6.0, r.center().y - g.size().y / 2.0), g, col);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ------------------------------------------------------------------ the player bar

fn npbar(app: &mut Nexus, ui: &mut Ui, bar: Rect, classic: bool, mini: bool) {
    let t = app.t.clone();
    let c = t.c;
    if classic {
        ui.painter().rect_filled(bar, 0.0, panel_fill(app));
        ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width(), 1.0)), 0.0, alpha(c.border, 0.8));
    }
    let inner = Rect::from_min_max(bar.min + vec2(16.0, 12.0), bar.max - vec2(20.0, 12.0));
    let right_w = if mini { 176.0 } else { 420.0 };
    let left_w = if mini { 0.0 } else { 56.0 + 14.0 + 270.0 + 14.0 };
    let cur = app.current_track().cloned();

    if !mini {
        let cr = Rect::from_min_size(pos2(inner.left(), inner.center().y - 28.0), vec2(56.0, 56.0));
        let key = cur.as_ref().map(|t| t.cover.clone()).unwrap_or_default();
        let p = ui.painter().clone();
        app.covers.paint(&p, &t, &key, cr);
        let resp = ui.interact(cr, egui::Id::unique("np-cover"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() {
            app.acts.push(Act::GotoPlayingAlbum);
        }
        let lr = Rect::from_min_size(pos2(cr.right() + 14.0, inner.top()), vec2(270.0, inner.height()));
        region(ui, lr, Layout::top_down(Align::Min), |ui| {
            let (title, sub) = match &cur {
                Some(tr) => (tr.title.clone(), format!("{} — {}", tr.artist, tr.album)),
                None => (String::new(), String::new()),
            };
            let tf = t.font(W::Semi, 0.0);
            let fh = ui.fonts_mut(|f| f.row_height(&tf));
            ui.add_space(((lr.height() - fh * 2.0 - 3.0) / 2.0).max(0.0));
            ui.spacing_mut().item_spacing.y = 3.0;
            if widgets::label(ui, &title, tf, c.text, Sense::click()).clicked() {
                app.acts.push(Act::GotoPlayingAlbum);
            }
            if widgets::label(ui, &sub, t.body(), c.dim, Sense::click()).clicked() {
                app.acts.push(Act::GotoPlayingArtist);
            }
        });
    }

    // centre: transport + progress
    let center = Rect::from_min_max(pos2(inner.left() + left_w, inner.top()), pos2(inner.right() - right_w - 14.0, inner.bottom()));
    let playing = app.playing();
    let ctrl_h = 40.0;
    let ctrl = Rect::from_min_size(center.min, vec2(center.width(), ctrl_h));
    let shuffle = app.cfg.b("shuffle");
    let repeat = app.cfg.s("repeat");
    region(ui, ctrl, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let total: f32 = if t.text_controls { 5.0 * 46.0 } else { 30.0 + 32.0 + 40.0 + 32.0 + 30.0 } + 4.0 * 14.0;
        ui.add_space(((ctrl.width() - total) / 2.0).max(0.0));
        if icon_button(ui, &t, Icon::Shuffle, 30.0, IconOpts { active: shuffle, ..Default::default() }).on_hover_text("shuffle  (ctrl+s)").clicked() {
            app.acts.push(Act::ToggleShuffle);
        }
        if icon_button(ui, &t, Icon::Prev, 32.0, IconOpts::default()).on_hover_text("previous").clicked() {
            app.acts.push(Act::Prev);
        }
        let pi = if playing { Icon::Pause } else { Icon::Play };
        if icon_button(ui, &t, pi, 40.0, IconOpts::default()).on_hover_text("play / pause  (space)").clicked() {
            app.acts.push(Act::TogglePlay);
        }
        if icon_button(ui, &t, Icon::Next, 32.0, IconOpts::default()).on_hover_text("next").clicked() {
            app.acts.push(Act::Next);
        }
        let ri = if repeat == "one" { Icon::RepeatOne } else { Icon::Repeat };
        if icon_button(ui, &t, ri, 30.0, IconOpts { active: repeat != "off", ..Default::default() }).on_hover_text("repeat  (ctrl+r)").clicked() {
            app.acts.push(Act::CycleRepeat);
        }
    });
    let prog = Rect::from_min_max(pos2(center.left(), ctrl.bottom() + 4.0), center.max);
    region(ui, prog, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let seek_w = (prog.width() - 2.0 * 52.0 - 24.0).clamp(40.0, 620.0);
        let total = seek_w + 2.0 * 52.0 + 24.0;
        ui.add_space(((prog.width() - total) / 2.0).max(0.0));
        let small = t.font(W::Regular, -1.5);
        let id = egui::Id::unique("seekbar");
        let dragging: Option<f32> = ui.data(|d| d.get_temp(id.with("drag")));
        let dur = app.dur;
        let pos = match dragging {
            Some(v) => f64::from(v) * dur,
            None => app.pos,
        };
        let (pr, _) = ui.allocate_exact_size(vec2(52.0, 18.0), Sense::hover());
        ui.painter().text(pr.right_center(), Align2::RIGHT_CENTER, fmt_time(pos), small.clone(), c.dim);
        let v = if dur > 0.0 { (pos / dur) as f32 } else { 0.0 };
        let fh = (ui.fonts_mut(|f| f.row_height(&t.body())) + 6.0).max(18.0);
        let out = widgets::text_bar(ui, &t, id, v, &t.progress_style, vec2(seek_w, fh), false);
        if let Some(r) = out.released {
            app.acts.push(Act::SeekFrac(r));
        }
        let remaining = app.cfg.b("show_remaining");
        let dtext = if remaining { format!("-{}", fmt_time((dur - pos).max(0.0))) } else { fmt_time(dur) };
        let (dr, dresp) = ui.allocate_exact_size(vec2(52.0, 18.0), Sense::click());
        ui.painter().text(dr.left_center(), Align2::LEFT_CENTER, dtext, small, c.dim);
        if dresp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            app.acts.push(Act::ToggleRemaining);
        }
    });

    // right: badge, tool buttons, volume
    let right = Rect::from_min_max(pos2(inner.right() - right_w, inner.top()), inner.max);
    let vol = app.cfg.f("volume");
    let vmax = app.vol_max();
    let muted = app.engine.muted();
    let page = app.page.clone();
    let eq_on = app.cfg.get("eq").and_then(|e| e.get("on")).and_then(|v| v.as_bool()).unwrap_or(false);
    let panel_on = app.cfg.b("now_panel");
    let has_lyrics = page == Page::Lyrics || cur.as_ref().is_some_and(|tr| app.lyrics.get(&tr.path).is_some());
    region(ui, right, Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let small = t.font(W::Regular, -1.5);
        let pct = format!("{}%", (vol * 100.0).round() as i64);
        let (lr, _) = ui.allocate_exact_size(vec2(38.0, 18.0), Sense::hover());
        ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, &pct, small.clone(), c.dim);
        let fh = (ui.fonts_mut(|f| f.row_height(&t.body())) + 6.0).max(18.0);
        let out = widgets::text_bar(ui, &t, egui::Id::unique("volbar"), (vol / vmax) as f32, &t.volume_style, vec2(96.0, fh), true);
        let out_resp = out.resp.on_hover_text(format!("{pct}{}", if vol > 1.0 { "  (boost)" } else { "" }));
        let _ = out_resp;
        if let Some(v) = out.changed {
            app.acts.push(Act::SetVolume(f64::from(v) * vmax));
        }
        let vi = if muted || vol <= 0.0 { Icon::Mute } else { Icon::Volume };
        if icon_button(ui, &t, vi, 26.0, IconOpts { volume: vol as f32, active: muted, ..Default::default() }).on_hover_text("mute").clicked() {
            app.acts.push(Act::ToggleMute);
        }
        if mini {
            return;
        }
        ui.add_space(2.0);
        if icon_button(ui, &t, Icon::Panel, 30.0, IconOpts { active: panel_on, ..Default::default() }).on_hover_text("now playing panel").clicked() {
            app.acts.push(Act::TogglePanel);
        }
        if icon_button(ui, &t, Icon::Queue, 30.0, IconOpts { active: page == Page::Queue, ..Default::default() }).on_hover_text("queue").clicked() {
            app.acts.push(Act::Go(Page::Queue));
        }
        if icon_button(ui, &t, Icon::Eq, 30.0, IconOpts { active: eq_on || page == Page::Eq, ..Default::default() }).on_hover_text("equalizer  (ctrl+e)").clicked() {
            app.acts.push(Act::OpenEq);
        }
        if icon_button(ui, &t, Icon::Vis, 30.0, IconOpts { active: page == Page::Visualizer, ..Default::default() }).on_hover_text("visualizer  (ctrl+b)").clicked() {
            app.acts.push(Act::ToggleVisualizer);
        }
        let tip = if has_lyrics { "lyrics  (ctrl+l)" } else { "no lyrics for this song" };
        if icon_button(ui, &t, Icon::Lyrics, 30.0, IconOpts { active: page == Page::Lyrics, disabled: !has_lyrics, ..Default::default() }).on_hover_text(tip).clicked() {
            app.acts.push(Act::ToggleLyrics);
        }
        if let Some(tr) = &cur {
            let q = tr.quality();
            if !q.is_empty() && !panel_on {
                ui.add_space(4.0);
                let text = if tr.is_lossy() { format!("{q}  ·  lossy") } else { q };
                badge(ui, &t, &text);
            }
        }
    });
}

pub fn badge(ui: &mut Ui, t: &crate::theme::Theme, text: &str) {
    let font = t.font(W::Semi, -2.5);
    let g = galley(ui, text, font, t.c.dim);
    let (r, _) = ui.allocate_exact_size(g.size() + vec2(16.0, 4.0), Sense::hover());
    ui.painter().rect_stroke(r, 9.0, Stroke::new(1.0, t.c.border), StrokeKind::Inside);
    ui.painter().galley(r.min + vec2(8.0, 2.0), g, t.c.dim);
}

// ------------------------------------------------------------------ toast

fn draw_toast(app: &mut Nexus, ui: &mut Ui, full: Rect, bar_h: f32) {
    let Some(toast) = &app.toast else { return };
    let t = app.t.clone();
    let c = t.c;
    let font = t.font(W::Semi, 0.0);
    let g = galley(ui, &toast.text, font.clone(), c.bg);
    let action = toast.action.as_ref().map(|(l, _)| t.l(l));
    let ag = action.as_ref().map(|a| galley(ui, a, t.font(W::Bold, 0.0), c.bg));
    let aw = ag.as_ref().map(|g| g.size().x + 20.0 + 12.0).unwrap_or(0.0);
    let w = (g.size().x + 18.0 + 10.0 + aw).min(full.width() - 16.0);
    let h = g.size().y.max(ag.as_ref().map(|g| g.size().y + 8.0).unwrap_or(0.0)) + 16.0;
    let bottom = full.bottom() - bar_h - 18.0;
    let r = Rect::from_min_size(pos2(full.center().x - w / 2.0, (bottom - h).max(full.top() + 8.0)), vec2(w, h));
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("toast"));
    let p = ui.ctx().layer_painter(layer);
    let rad = if t.pills { 18.0f32.min(h / 2.0) } else { t.radius.min(10.0) };
    p.rect_filled(r, rad, c.text);
    let tg = elided(ui, &toast.text, font, c.bg, w - 28.0 - aw);
    p.galley(pos2(r.left() + 18.0, r.center().y - tg.size().y / 2.0), tg, c.bg);
    let mut clicked = false;
    let mut hover_any = false;
    if let Some(ag) = ag {
        let br = Rect::from_min_size(pos2(r.right() - 10.0 - ag.size().x - 20.0, r.center().y - (ag.size().y + 8.0) / 2.0), vec2(ag.size().x + 20.0, ag.size().y + 8.0));
        let resp = ui.interact(br, egui::Id::unique("toast-btn"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
        let hov = resp.hovered();
        hover_any = hov;
        p.rect(br, 12.0, if hov { c.bg } else { Color32::TRANSPARENT }, Stroke::new(1.0, c.bg), StrokeKind::Inside);
        p.galley(br.min + vec2(10.0, 4.0), ag, if hov { c.text } else { c.bg });
        clicked = resp.clicked();
    }
    let body = ui.interact(r, egui::Id::unique("toast-body"), Sense::hover());
    if body.hovered() || hover_any {
        // keep it while the mouse is on it
        let now = ui.input(|i| i.time);
        if let Some(tt) = &mut app.toast {
            tt.until = tt.until.max(now + 1.5);
        }
    }
    if clicked {
        if let Some(tt) = app.toast.take() {
            if let Some((_, f)) = tt.action {
                f(app);
            }
        }
    }
}
