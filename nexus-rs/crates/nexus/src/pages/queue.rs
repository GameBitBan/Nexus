//! The play queue.

use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::table::{self, Mode, Spec};
use crate::widgets::{self, Pill};
use egui::{Align, Layout, Rect, Sense, Ui, vec2};
use nexus_core::util::fmt_time;

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let rows = app.queue.clone();
    let left: f64 = rows.iter().skip(app.qi.unwrap_or(0)).map(|t| t.duration).sum();
    let cols = table::columns(&app.cfg.strings("song_columns"), &[]);
    let names = app.playlists.names();
    let current = app.current_path.clone();
    let single = app.cfg.get("single_click_play").and_then(|v| v.as_bool()).unwrap_or(true);
    let stamp = app.queue_ver ^ (app.lib_ver << 32);
    region(ui, Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 0.0)), Layout::top_down(Align::Min), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let g = widgets::galley(ui, &t.l("queue"), t.font(crate::theme::W::Display, 27.0), t.c.text);
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, t.c.text);
            ui.with_layout(Layout::left_to_right(Align::Max), |ui| {
                widgets::dim(ui, &t, &format!("{} tracks  ·  {} left", rows.len(), fmt_time(left)));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::pill(ui, &t, "clear", Pill::Normal).clicked() {
                    app.acts.push(Act::QueueClear);
                }
            });
        });
        ui.add_space(16.0);
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
        table::show(ui, &mut env, &mut app.queue_tbl, Spec { id: "queue", tracks: &rows, stamp, mode: Mode::Queue, cols, thumbs: true, plain: false });
    });
}
