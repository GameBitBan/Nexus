//! Outside the window: Discord Rich Presence (and the desktop media controls).

use crate::app::Nexus;
use crossbeam_channel::bounded;
use nexus_core::discord::{self, Update};
use serde_json::json;

impl Nexus {
    /// Tell Discord soon (several changes in a row become one update).
    pub fn push_presence(&mut self) {
        if self.cfg.b("discord_enabled") {
            self.presence_due = Some(self.ctx.input(|i| i.time) + 0.4);
            self.ctx.request_repaint_after(std::time::Duration::from_millis(450));
        }
    }

    fn presence_update(&self) -> Update {
        let cfg = &self.cfg;
        let client_id = cfg.s("discord_client_id");
        let playing = self.playing();
        let Some(t) = self.current_track().filter(|_| playing || !cfg.b("discord_hide_paused")) else {
            return Update { client_id, ..Default::default() };
        };
        let clip = |s: &str| -> String {
            let s = if s.chars().count() >= 2 { s.to_string() } else { format!("{s}  ") };
            s.chars().take(128).collect()
        };
        let large_text: Vec<String> = [t.album.clone(), t.quality()].into_iter().filter(|s| !s.is_empty()).collect();
        let large = cfg.s("discord_large_image");
        let mut act = json!({
            "type": 2,
            "details": clip(&t.title),
            "state": clip(&format!("by {}", t.artist)),
            "assets": {"large_image": if large.is_empty() { "nexus".to_string() } else { large.clone() }, "large_text": clip(&large_text.join(" · "))},
            "status_display_type": if cfg.b("discord_song_in_status") { 2 } else { 0 },
        });
        if playing && self.dur > 0.0 {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
            let start = now - self.pos;
            act["timestamps"] = json!({"start": start as i64, "end": (start + self.dur) as i64});
        } else if !playing {
            act["state"] = json!(clip(&format!("by {} · paused", t.artist)));
        }
        let cover_file = if t.cover.is_empty() { String::new() } else { nexus_core::paths::covers_dir().join(format!("{}.jpg", t.cover)).to_string_lossy().to_string() };
        let src = cfg.s("discord_cover_source");
        Update {
            activity: Some(act),
            artist: if t.albumartist.is_empty() { t.artist.clone() } else { t.albumartist.clone() },
            album: t.album.clone(),
            title: t.title.clone(),
            art_key: t.album_key.clone(),
            cover_file,
            art_source: if src.is_empty() { "upload".into() } else { src },
            art_lookup: cfg.b("discord_cover_lookup"),
            client_id,
            large_image: large,
            small_image: cfg.s("discord_small_image"),
        }
    }

    /// Every frame: notice play/pause/track changes, send due updates, the one-time offer.
    pub fn presence_tick(&mut self, now: f64) {
        let seen = (self.current_path.clone(), self.playing());
        if seen != self.presence_seen {
            self.presence_seen = seen;
            self.push_presence();
        }
        if let Some(due) = self.presence_due {
            if now >= due {
                self.presence_due = None;
                if self.cfg.b("discord_enabled") {
                    let u = self.presence_update();
                    self.discord.send(u);
                }
            }
        }
        if let Some(rx) = &self.discord_offer {
            if let Ok(running) = rx.try_recv() {
                self.discord_offer = None;
                if running {
                    self.cfg.set("discord_offered", true);
                    self.cfg.save();
                    self.toast_action(
                        "show what you're listening to on your Discord profile?",
                        "turn on",
                        |app| {
                            app.cfg.set("discord_enabled", true);
                            app.cfg.save();
                            app.push_presence();
                            app.toast("showing what you play on discord — turn it off in settings → discord");
                        },
                        15.0,
                    );
                }
            }
        }
    }

    /// Once: if Discord is open, offer to show what you're playing.
    pub fn maybe_offer_discord(&mut self) {
        if self.cfg.b("discord_enabled") || self.cfg.b("discord_offered") {
            return;
        }
        let (tx, rx) = bounded(1);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(4));
            let _ = tx.send(discord::discord_running());
            ctx.request_repaint();
        });
        self.discord_offer = Some(rx);
    }

    pub fn discord_setting_changed(&mut self, key: &str) {
        match key {
            "discord_client_id" => self.discord.reset(),
            "discord_enabled" if !self.cfg.b("discord_enabled") => {
                let cid = self.cfg.s("discord_client_id");
                self.discord.clear(&cid);
            }
            _ => {}
        }
        self.push_presence();
    }
}
