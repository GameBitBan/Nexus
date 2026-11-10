//! The pages shown in the card. Each draws itself into the rect it's given.

mod albums;
mod browse;
mod collection;
pub mod discover;
pub mod download;
mod eq;
pub mod import;
pub mod inspect;
pub mod lyrics;
mod queue;
pub mod settings;
mod songs;
pub mod tags;
pub mod visualizer;

use crate::app::{Nexus, Page};
use crate::layout::region;
use crate::theme::W;
use crate::widgets;
use egui::{Align, Layout, Rect, Sense, Ui, vec2};

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let page = app.page.clone();
    match page {
        Page::Albums => albums::albums_page(app, ui, rect),
        Page::Artists(_) => albums::artists_page(app, ui, rect),
        Page::Browse(_) => browse::draw(app, ui, rect),
        Page::Songs => songs::draw(app, ui, rect),
        Page::Queue => queue::draw(app, ui, rect),
        Page::Album(k) => collection::album(app, ui, rect, &k),
        Page::Playlist(n) => collection::playlist(app, ui, rect, &n),
        Page::Settings(tab) => settings::draw(app, ui, rect, tab),
        Page::Eq => eq::draw(app, ui, rect),
        Page::Visualizer => visualizer::draw(app, ui, rect),
        Page::Lyrics => lyrics::draw(app, ui, rect),
        Page::Inspect(p) => inspect::draw(app, ui, rect, p),
        Page::Tags(p) => tags::draw(app, ui, rect, &p),
        Page::Import => import::draw(app, ui, rect),
        Page::Discover => discover::draw(app, ui, rect),
        Page::Download => download::draw(app, ui, rect),
        other => coming(app, ui, rect, &other),
    }
}

/// Pages that aren't in the Rust version yet.
fn coming(app: &mut Nexus, ui: &mut Ui, rect: Rect, page: &Page) {
    let t = app.t.clone();
    let name = match page {
        Page::Jam => "jam",
        _ => "page",
    };
    region(ui, rect.shrink2(vec2(32.0, 28.0)), Layout::top_down(Align::Min), |ui| {
        widgets::eyebrow(ui, &t, &name.to_uppercase());
        ui.add_space(4.0);
        widgets::label(ui, &t.l(name), t.font(W::Display, 27.0), t.c.text, Sense::hover());
        ui.add_space(10.0);
        widgets::dim(ui, &t, "coming in the next build of Nexus 2");
        ui.add_space(14.0);
        if widgets::pill(ui, &t, "back", widgets::Pill::Primary).clicked() {
            app.acts.push(crate::acts::Act::Back);
        }
    });
}
