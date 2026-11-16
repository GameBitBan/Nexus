//! The pages shown in the card. Each draws itself into the rect it's given.

mod albums;
mod browse;
mod collection;
pub mod discover;
pub mod download;
mod eq;
pub mod import;
pub mod inspect;
pub mod jam;
pub mod lyrics;
mod queue;
pub mod settings;
mod songs;
pub mod tags;
pub mod visualizer;

use crate::app::{Nexus, Page};
use egui::{Rect, Ui};

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
        Page::Jam => jam::draw(app, ui, rect),
    }
}
