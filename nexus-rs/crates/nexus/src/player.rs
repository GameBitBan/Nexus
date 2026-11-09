//! Queue and playback: play lists, shuffle/repeat, gapless hand-over, volume, the now-playing file.

use crate::acts::Tracks;
use crate::app::{Nexus, format_track};
use nexus_core::audio::State;
use nexus_core::library::Track;
use std::sync::Arc;

impl Nexus {
    pub fn current_track(&self) -> Option<&Arc<Track>> {
        self.qi.and_then(|i| self.queue.get(i))
    }

    pub fn playing(&self) -> bool {
        self.engine.state() == State::Playing
    }

    pub fn play_list(&mut self, tracks: Tracks, start: usize) {
        let mut tracks = tracks;
        if tracks.is_empty() {
            return;
        }
        let mut start = start.min(tracks.len() - 1);
        self.unshuffled = tracks.clone();
        if self.cfg.b("shuffle") {
            let first = tracks.remove(start);
            fastrand::shuffle(&mut tracks);
            tracks.insert(0, first);
            start = 0;
        }
        self.queue = tracks;
        self.qi = Some(start);
        self.queue_ver += 1;
        self.play_current_at(0.0, true);
    }

    pub fn play_shuffled(&mut self, tracks: Tracks) {
        if tracks.is_empty() {
            return;
        }
        if !self.cfg.b("shuffle") {
            self.cfg.set("shuffle", true);
        }
        let i = fastrand::usize(..tracks.len());
        self.play_list(tracks, i);
    }

    pub fn play_queue_index(&mut self, i: usize) {
        if i < self.queue.len() {
            self.qi = Some(i);
            self.play_current_at(0.0, true);
        }
    }

    pub fn play_current_at(&mut self, start: f64, autoplay: bool) {
        let Some(t) = self.current_track().cloned() else {
            self.engine.stop();
            return;
        };
        self.current_path = Some(t.path.clone());
        self.counted = false;
        self.dur = t.duration;
        self.engine.load(&t.path, start, autoplay);
        self.track_changed(&t);
    }

    /// Everything that follows the current song: up-next for gapless, the OBS file, the background.
    fn track_changed(&mut self, t: &Arc<Track>) {
        self.update_gapless_next();
        self.write_now_playing(t);
        self.set_ambient(&t.cover.clone());
        self.queue_tbl.scroll_to = self.qi;
    }

    pub fn set_ambient(&mut self, key: &str) {
        if key != self.ambient_key {
            self.ambient_old = std::mem::replace(&mut self.ambient_key, key.to_string());
            self.ambient_t0 = self.ctx.input(|i| i.time);
        }
    }

    fn write_now_playing(&mut self, t: &Track) {
        let path = self.cfg.s("now_playing_file");
        let path = path.trim();
        if path.is_empty() {
            return;
        }
        let fmt = self.cfg.s("now_playing_format");
        let text = format_track(if fmt.is_empty() { "{artist} - {title}" } else { &fmt }, t);
        let path = if let Some(rest) = path.strip_prefix("~/") { nexus_core::paths::home_dir().join(rest) } else { path.into() };
        if let Err(e) = std::fs::write(&path, text) {
            log::warn!("now playing file: {e}");
        }
    }

    pub fn toggle_play(&mut self) {
        match self.engine.state() {
            State::Playing => self.engine.pause(),
            State::Paused if self.current_track().is_some() => self.engine.play(),
            _ => {
                if self.current_track().is_some() {
                    self.play_current_at(0.0, true);
                } else if !self.lib.tracks.is_empty() {
                    let rows = self.songs_view();
                    let rows = if rows.is_empty() { self.lib.tracks.clone() } else { self.songs_tbl.view(&rows) };
                    let rows = if rows.is_empty() { self.lib.tracks.clone() } else { rows };
                    self.play_list(rows, 0);
                }
            }
        }
    }

    pub fn seek(&mut self, secs: f64) {
        self.engine.seek(secs.clamp(0.0, self.dur.max(0.0)));
    }

    pub fn next(&mut self, auto: bool) {
        if self.queue.is_empty() {
            return;
        }
        let repeat = self.cfg.s("repeat");
        if auto && repeat == "one" {
            self.play_current_at(0.0, true);
            return;
        }
        let qi = self.qi.unwrap_or(0);
        if qi + 1 < self.queue.len() {
            self.qi = Some(qi + 1);
        } else if repeat == "all" {
            self.qi = Some(0);
        } else {
            if auto {
                self.engine.stop();
                self.qi = Some(self.queue.len() - 1);
                self.queue_ver += 1;
            }
            return;
        }
        self.play_current_at(0.0, true);
    }

    pub fn prev(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        if self.engine.position() > 3.0 || self.qi.unwrap_or(0) == 0 {
            self.engine.seek(0.0);
            return;
        }
        self.qi = Some(self.qi.unwrap_or(1) - 1);
        self.play_current_at(0.0, true);
    }

    pub fn toggle_shuffle(&mut self) {
        let on = !self.cfg.b("shuffle");
        self.cfg.set("shuffle", on);
        if let Some(qi) = self.qi.filter(|&i| i < self.queue.len()) {
            let cur = self.queue[qi].clone();
            if on {
                self.unshuffled = self.queue.clone();
                let mut rest = self.queue.split_off(qi + 1);
                fastrand::shuffle(&mut rest);
                self.queue.extend(rest);
            } else if !self.unshuffled.is_empty() {
                self.queue = self.unshuffled.clone();
                self.qi = Some(self.queue.iter().position(|t| Arc::ptr_eq(t, &cur)).unwrap_or(0));
            }
            self.queue_ver += 1;
        }
        self.update_gapless_next();
    }

    pub fn cycle_repeat(&mut self) {
        let next = match self.cfg.s("repeat").as_str() {
            "off" => "all",
            "all" => "one",
            _ => "off",
        };
        self.cfg.set("repeat", next);
        self.update_gapless_next();
    }

    pub fn queue_next(&mut self, tracks: Tracks) {
        let Some(qi) = self.qi.filter(|_| !self.queue.is_empty()) else {
            return self.play_list(tracks, 0);
        };
        let at = (qi + 1).min(self.queue.len());
        self.queue.splice(at..at, tracks.iter().cloned());
        if !self.unshuffled.is_empty() {
            let cur = self.queue[qi].clone();
            let j = self.unshuffled.iter().position(|t| Arc::ptr_eq(t, &cur)).unwrap_or(self.unshuffled.len() - 1);
            self.unshuffled.splice(j + 1..j + 1, tracks);
        }
        self.queue_ver += 1;
        self.update_gapless_next();
    }

    pub fn queue_add(&mut self, tracks: Tracks) {
        if self.queue.is_empty() {
            return self.play_list(tracks, 0);
        }
        self.queue.extend(tracks.iter().cloned());
        self.unshuffled.extend(tracks);
        self.queue_ver += 1;
        self.update_gapless_next();
    }

    pub fn queue_remove(&mut self, rows: &[usize]) {
        let qi = self.qi.unwrap_or(usize::MAX);
        let removed: Vec<Arc<Track>> = rows.iter().filter_map(|&r| self.queue.get(r).cloned()).collect();
        let before = rows.iter().filter(|&&r| r < qi).count();
        let cur_gone = rows.contains(&qi);
        let mut i = 0;
        self.queue.retain(|_| {
            let keep = !rows.contains(&i);
            i += 1;
            keep
        });
        self.unshuffled.retain(|t| !removed.iter().any(|r| Arc::ptr_eq(r, t)));
        if let Some(q) = self.qi {
            let shift = before + usize::from(cur_gone);
            self.qi = q.checked_sub(shift);
        }
        self.queue_ver += 1;
        self.update_gapless_next();
    }

    pub fn queue_clear(&mut self) {
        match self.current_track().cloned() {
            Some(t) => {
                self.queue = vec![t];
                self.qi = Some(0);
            }
            None => {
                self.queue.clear();
                self.qi = None;
            }
        }
        self.unshuffled = self.queue.clone();
        self.queue_ver += 1;
        self.update_gapless_next();
    }

    pub fn vol_max(&self) -> f64 {
        (self.cfg.f("volume_max") / 100.0).max(1.0)
    }

    pub fn set_volume(&mut self, v: f64) {
        let v = v.clamp(0.0, self.vol_max());
        self.cfg.set("volume", v);
        let gain = if v <= 1.0 { v * v } else { v };
        self.engine.set_volume(gain as f32);
    }

    fn peek_next(&self) -> Option<Arc<Track>> {
        let qi = self.qi?;
        if qi >= self.queue.len() {
            return None;
        }
        match self.cfg.s("repeat").as_str() {
            "one" => Some(self.queue[qi].clone()),
            r => {
                if qi + 1 < self.queue.len() {
                    Some(self.queue[qi + 1].clone())
                } else if r == "all" {
                    self.queue.first().cloned()
                } else {
                    None
                }
            }
        }
    }

    pub fn update_gapless_next(&mut self) {
        self.engine.set_next(if self.cfg.b("gapless") { self.peek_next().map(|t| t.path.clone()) } else { None });
    }

    /// The engine already rolled into the next file without a gap: move the queue along.
    pub fn on_gapless(&mut self, path: &str) {
        let nxt = self.peek_next();
        match nxt {
            Some(n) if n.path == path => {
                if self.cfg.s("repeat") != "one" {
                    let qi = self.qi.unwrap_or(0);
                    self.qi = Some(if qi + 1 < self.queue.len() { qi + 1 } else { 0 });
                }
            }
            _ => {
                if let Some(i) = self.queue.iter().position(|t| t.path == path) {
                    self.qi = Some(i);
                }
            }
        }
        if let Some(t) = self.current_track().cloned() {
            self.current_path = Some(t.path.clone());
            self.counted = false;
            self.dur = t.duration;
            self.track_changed(&t);
        }
        self.queue_ver += 1;
    }

    pub fn on_error(&mut self, msg: &str) {
        self.errors += 1;
        let cur = self.current_track().map(|t| t.path.clone()).unwrap_or_default();
        log::warn!("can't play {cur}: {msg} [output: {:?}]", self.engine.output_info());
        let short: String = format!("can't play: {msg}").chars().take(140).collect();
        self.toast(short);
        if self.errors < 5 && !msg.contains("audio") {
            self.next(true);
        }
    }
}
