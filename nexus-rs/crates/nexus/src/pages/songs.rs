//! Every song, filtered by the search box.

use crate::app::Nexus;
use crate::layout::region;
use crate::table::{self, Mode, Spec};
use egui::{Align, Layout, Rect, Ui, vec2};
use std::hash::{Hash, Hasher};

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let rows = app.songs_view();
    let t = app.t.clone();
    let cols = table::columns(&app.cfg.strings("song_columns"), &[]);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (app.lib_ver, &app.search).hash(&mut h);
    let stamp = h.finish();
    let names = app.playlists.names();
    let current = app.current_path.clone();
    let single = app.cfg.get("single_click_play").and_then(|v| v.as_bool()).unwrap_or(true);
    region(ui, Rect::from_min_max(rect.min + vec2(28.0, 14.0), rect.max - vec2(28.0, 0.0)), Layout::top_down(Align::Min), |ui| {
        let mut env = table::Env {
            t: &t,
            covers: &mut app.covers,
            current_path: current.as_deref(),
            qi: app.qi,
            playlists: &names,
            single_click: single,
            acts: &mut app.acts,
            playlist: None,
        };
        table::show(ui, &mut env, &mut app.songs_tbl, Spec { id: "songs", tracks: &rows, stamp, mode: Mode::List, cols, thumbs: true, plain: false });
    });
}
