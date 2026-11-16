//! The Jam page: start (host / join), hosting (code, who's listening, download requests, chat) and
//! listening along (the host's song, your own volume and pause, their queue and library, chat).

use crate::acts::Act;
use crate::app::Nexus;
use crate::layout::region;
use crate::theme::{Theme, W, alpha};
use crate::widgets::{self, Pill, elided};
use egui::{Align, CursorIcon, Key, Layout, Rect, Sense, Stroke, Ui, pos2, vec2};
use nexus_core::util::fmt_time;

type Later = Option<Box<dyn FnOnce(&mut Nexus)>>;

pub fn draw(app: &mut Nexus, ui: &mut Ui, rect: Rect) {
    let t = app.t.clone();
    let c = t.c;
    let inner = Rect::from_min_max(rect.min + vec2(32.0, 28.0), rect.max - vec2(28.0, 12.0));
    let screen = app.jam.screen.clone();
    let mut later: Later = None;
    region(ui, inner, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                widgets::eyebrow(ui, &t, &t.l("LISTEN TOGETHER"));
                widgets::label(ui, &t.l("Jam"), t.font(W::Display, 27.0), c.text, Sense::hover());
            });
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                let label = match screen.as_str() {
                    "hosting" => Some("end jam"),
                    "guest" | "joining" => Some("leave"),
                    _ => None,
                };
                if let Some(l) = label {
                    if widgets::pill(ui, &t, l, Pill::Normal).clicked() {
                        later = Some(Box::new(|a: &mut Nexus| a.jam_leave()));
                    }
                }
            });
        });
        let status = if app.jam.status.is_empty() { "listen together with a friend — one of you hosts, the other joins".to_string() } else { app.jam.status.clone() };
        wrapped(ui, &t.l(&status), t.body(), c.dim);
        let body = ui.available_rect_before_wrap();
        let later2 = match screen.as_str() {
            "hosting" => hosting(app, ui, &t, body),
            "guest" => guest(app, ui, &t, body),
            _ => start(app, ui, &t, body, screen == "joining"),
        };
        if later2.is_some() {
            later = later2;
        }
    });
    if let Some(f) = later {
        f(app);
    }
}

fn wrapped(ui: &mut Ui, text: &str, font: egui::FontId, color: egui::Color32) {
    let w = ui.available_width();
    let g = ui.fonts_mut(|f| f.layout(text.to_string(), font, color, w));
    let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    ui.painter().galley(r.min, g, color);
}

fn panel(ui: &mut Ui, t: &Theme, f: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(t.c.panel)
        .stroke(Stroke::new(1.0, t.c.border))
        .corner_radius(t.radius.max(8.0))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui);
        });
}

/// A column of fixed width and the given height.
fn column<R>(ui: &mut Ui, w: f32, h: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(w, h), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        ui.set_max_height(h);
        f(ui)
    })
    .inner
}

fn text_field(ui: &mut Ui, t: &Theme, text: &mut String, hint: &str, width: f32, id: &str) -> (egui::Response, bool) {
    let id = egui::Id::unique(("jam-field", id));
    let resp = widgets::search_box(ui, t, text, hint, width, id);
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    (resp, enter)
}

// ---------------------------------------------------------------- start

fn start(app: &mut Nexus, ui: &mut Ui, t: &Theme, body: Rect, joining: bool) -> Later {
    let gap = 16.0;
    let mut later: Later = None;
    egui::ScrollArea::vertical().id_salt("jam-start").auto_shrink([false, false]).max_height(body.height()).show(ui, |ui| {
        let w = ui.available_width() - 14.0;
        let two = w > 720.0;
        let card_w = if two { (w - gap) / 2.0 } else { w };
        if two {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let a = column(ui, card_w, body.height(), |ui| host_card(app, ui, t));
                let b = column(ui, card_w, body.height(), |ui| join_card(app, ui, t, joining));
                later = a.or(b);
            });
        } else {
            ui.set_width(w);
            let a = host_card(app, ui, t);
            ui.add_space(gap);
            let b = join_card(app, ui, t, joining);
            later = a.or(b);
        }
    });
    later
}

fn host_card(app: &mut Nexus, ui: &mut Ui, t: &Theme) -> Later {
    let c = t.c;
    let mut later: Later = None;
    panel(ui, t, |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        widgets::label(ui, &t.l("Host"), t.font(W::Display, 13.0), c.text, Sense::hover());
        wrapped(ui, &t.l("Your friends hear whatever you play, in real time (about a second behind). They can see what's on, chat, and add songs from your library to the queue."), t.body(), c.dim);
        perm_rows(app, ui, t, true);
        ui.add_space(4.0);
        if widgets::pill(ui, t, "start a jam", Pill::Primary).clicked() {
            later = Some(Box::new(|a: &mut Nexus| a.jam_start_host()));
        }
    });
    later
}

fn join_card(app: &mut Nexus, ui: &mut Ui, t: &Theme, joining: bool) -> Later {
    let c = t.c;
    let mut later: Later = None;
    panel(ui, t, |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        widgets::label(ui, &t.l("Join"), t.font(W::Display, 13.0), c.text, Sense::hover());
        wrapped(ui, &t.l("Got a code from a friend? Type it here to hear their Nexus. Your own music pauses."), t.body(), c.dim);
        let fw = 240.0f32.min(ui.available_width());
        let (_, enter) = text_field(ui, t, &mut app.jam.code_in, "code, like K7Q-M2X", fw, "code");
        ui.add_space(4.0);
        let label = if joining { "joining…" } else { "join" };
        let clicked = ui.add_enabled_ui(!joining, |ui| widgets::pill(ui, t, label, Pill::Primary)).inner.clicked();
        if (clicked || enter) && !joining {
            later = Some(Box::new(|a: &mut Nexus| a.jam_join()));
        }
    });
    later
}

/// The host's settings: name (before starting), what friends may do.
fn perm_rows(app: &mut Nexus, ui: &mut Ui, t: &Theme, with_name: bool) {
    let c = t.c;
    let avail = ui.available_width();
    let label_w = (avail - 122.0).clamp(100.0, 170.0);
    let field_w = (avail - label_w - 12.0).clamp(80.0, 200.0);
    let row = |ui: &mut Ui, text: &str| {
        let g = elided(ui, &t.l(text), t.body(), c.text, label_w);
        let (r, _) = ui.allocate_exact_size(vec2(label_w, 24.0), Sense::hover());
        ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, c.text);
    };
    let mut changed = false;
    if with_name {
        ui.horizontal(|ui| {
            row(ui, "your name");
            let hint = app.jam_name();
            let (resp, _) = text_field(ui, t, &mut app.jam.name_in, &hint, field_w, "name");
            if resp.lost_focus() {
                let n = app.jam.name_in.trim().to_string();
                app.cfg.set("jam_name", n);
                app.cfg.save();
            }
        });
    }
    for (key, text) in [("jam_allow_add", "friends can add songs"), ("jam_allow_skip", "friends can skip")] {
        ui.horizontal(|ui| {
            row(ui, text);
            let mut on = app.cfg.b(key);
            if widgets::switch(ui, t, &mut on).changed() {
                app.cfg.set(key, on);
                app.cfg.save();
                changed = true;
            }
        });
    }
    ui.horizontal(|ui| {
        row(ui, "friends can download");
        let cur = {
            let m = app.cfg.s("jam_allow_download");
            if m.is_empty() { "ask".to_string() } else { m }
        };
        let opts = [("ask", "ask me"), ("yes", "always"), ("no", "never")];
        let shown = opts.iter().find(|(k, _)| *k == cur).map(|(_, l)| t.l(l)).unwrap_or_else(|| t.l("ask me"));
        egui::ComboBox::from_id_salt("jam-dl").selected_text(shown).width(field_w.min(140.0)).show_ui(ui, |ui| {
            for (k, l) in opts {
                if ui.selectable_label(cur == k, t.l(l)).clicked() {
                    app.cfg.set("jam_allow_download", k);
                    app.cfg.save();
                    changed = true;
                }
            }
        });
    });
    if changed {
        app.jam_send_perm(None);
    }
}

// ---------------------------------------------------------------- hosting

fn hosting(app: &mut Nexus, ui: &mut Ui, t: &Theme, body: Rect) -> Later {
    let c = t.c;
    let mut later: Later = None;
    let gap = 18.0;
    let left_w = (body.width() * 0.38).clamp(280.0, 360.0);
    let right_w = body.width() - left_w - gap;
    let h = body.height();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        column(ui, left_w, h, |ui| {
            egui::ScrollArea::vertical().id_salt("jam-host-left").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                widgets::eyebrow(ui, t, &t.l("YOUR CODE"));
                let code = app.jam.jam.code().to_string();
                widgets::label(ui, &code, t.font(W::Display, 30.0), c.text, Sense::hover());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::pill(ui, t, "copy code", Pill::Normal).clicked() {
                        app.acts.push(Act::CopyText(code.clone()));
                        later = Some(Box::new(|a: &mut Nexus| a.toast("code copied — send it to your friend")));
                    }
                    if widgets::pill(ui, t, "copy invite", Pill::Normal).clicked() {
                        app.acts.push(Act::CopyText(app.jam_invite_text()));
                        later = Some(Box::new(|a: &mut Nexus| a.toast("invite copied — paste it in Discord")));
                    }
                });
                ui.add_space(6.0);
                widgets::eyebrow(ui, t, &t.l("LISTENING"));
                if app.jam.people.is_empty() {
                    widgets::small_dim(ui, t, &t.l("nobody yet…"));
                }
                for (peer, name) in app.jam.people.clone() {
                    let w = ui.available_width() - 8.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, alpha(c.select, 0.7));
                    }
                    let g = elided(ui, &name, t.body(), c.text, w - 90.0);
                    ui.painter().galley(pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, c.text);
                    let br = Rect::from_min_size(pos2(r.right() - 72.0, r.top() + 3.0), vec2(68.0, 22.0));
                    let b = ui.interact(br, egui::Id::unique(("jam-kick", &peer)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text(t.l("remove from the jam"));
                    if resp.hovered() || b.hovered() {
                        let col = if b.hovered() { c.text } else { c.dim };
                        ui.painter().text(br.center(), egui::Align2::CENTER_CENTER, t.l("remove"), t.font(W::Regular, -2.0), col);
                    }
                    if b.clicked() {
                        let p = peer.clone();
                        later = Some(Box::new(move |a: &mut Nexus| a.jam_kick(&p)));
                    }
                }
                if !app.jam.requests.is_empty() {
                    ui.add_space(6.0);
                    widgets::eyebrow(ui, t, &t.l("DOWNLOAD REQUESTS"));
                    for r in &app.jam.requests {
                        panel(ui, t, |ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            wrapped(ui, &r.text, t.body(), c.text);
                            widgets::small_dim(ui, t, &t.l(&r.line));
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if !r.started && widgets::pill(ui, t, "send", Pill::Primary).clicked() {
                                    let rid = r.rid.clone();
                                    later = Some(Box::new(move |a: &mut Nexus| a.jam_allow_request(&rid)));
                                }
                                if r.finished.is_none() {
                                    let l = if r.started { "stop" } else { "decline" };
                                    if widgets::pill(ui, t, l, Pill::Normal).clicked() {
                                        let rid = r.rid.clone();
                                        later = Some(Box::new(move |a: &mut Nexus| a.jam_decline_request(&rid)));
                                    }
                                }
                            });
                        });
                    }
                }
                ui.add_space(6.0);
                widgets::eyebrow(ui, t, &t.l("FRIENDS CAN"));
                perm_rows(app, ui, t, false);
            });
        });
        column(ui, right_w, h, |ui| chat(app, ui, t, h));
    });
    later
}

fn chat(app: &mut Nexus, ui: &mut Ui, t: &Theme, h: f32) {
    let c = t.c;
    widgets::eyebrow(ui, t, &t.l("CHAT"));
    let list_h = (h - 80.0).max(80.0);
    egui::Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)).corner_radius(t.radius.max(8.0)).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_height(list_h - 20.0);
        egui::ScrollArea::vertical().id_salt("jam-chat").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if app.jam.chat.is_empty() {
                widgets::small_dim(ui, t, &t.l("say hi — everyone in the jam sees it"));
            }
            for (who, text) in &app.jam.chat {
                let w = ui.available_width();
                let mut job = egui::text::LayoutJob::default();
                job.wrap.max_width = w;
                job.append(&format!("{who}  "), 0.0, egui::TextFormat { font_id: t.font(W::Semi, 0.0), color: c.text, ..Default::default() });
                job.append(text, 0.0, egui::TextFormat { font_id: t.body(), color: c.dim, ..Default::default() });
                let g = ui.fonts_mut(|f| f.layout_job(job));
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, c.text);
            }
        });
    });
    app.jam.chat_new = false;
    let w = ui.available_width();
    let (_, enter) = text_field(ui, t, &mut app.jam.chat_in, "say something", w, "chat");
    if enter {
        app.jam_say();
        ui.memory_mut(|m| m.request_focus(egui::Id::unique(("jam-field", "chat"))));
    }
}

// ---------------------------------------------------------------- listening along

fn guest(app: &mut Nexus, ui: &mut Ui, t: &Theme, body: Rect) -> Later {
    let c = t.c;
    let mut later: Later = None;
    let gap = 18.0;
    let h = body.height();
    let w = body.width();
    let col1 = 240.0f32.min(w * 0.3);
    let rest = w - col1 - gap * 2.0;
    let col2 = rest * 0.5;
    let col3 = rest - col2;
    let host = app.jam_host_label();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        column(ui, col1, h, |ui| {
            egui::ScrollArea::vertical().id_salt("jam-guest-np").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let size = col1.min(220.0);
                let (cr, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
                let key = app.jam.np.as_ref().map(|n| n.cover_key.clone()).unwrap_or_default();
                let p = ui.painter().clone();
                app.covers.paint(&p, t, &key, cr);
                ui.add_space(4.0);
                widgets::eyebrow(ui, t, &format!("{} {}", t.l("LISTENING WITH"), host.to_uppercase()));
                let (title, sub, fmt) = match &app.jam.np {
                    Some(n) => (
                        if n.title.is_empty() { t.l("nothing playing") } else { n.title.clone() },
                        [n.artist.as_str(), n.album.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" — "),
                        n.fmt.clone(),
                    ),
                    None => (t.l("waiting for the music…"), String::new(), String::new()),
                };
                let g = ui.fonts_mut(|f| f.layout(title, t.font(W::Display, 6.0), c.text, col1));
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, c.text);
                if !sub.is_empty() {
                    wrapped(ui, &sub, t.body(), c.dim);
                }
                if !fmt.is_empty() {
                    widgets::small_dim(ui, t, &fmt);
                }
                let (pos, dur) = app.jam_pos();
                let v = if dur > 0.0 { (pos / dur) as f32 } else { 0.0 };
                let fh = (ui.fonts_mut(|f| f.row_height(&t.body())) + 6.0).max(18.0);
                let _ = widgets::text_bar(ui, t, egui::Id::unique("jam-progress"), v, &t.progress_style, vec2(size, fh), false).resp.on_hover_text(t.l("only the host can seek"));
                widgets::small_dim(ui, t, &format!("{} / {}", fmt_time(pos / 1000.0), fmt_time(dur / 1000.0)));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    widgets::small_dim(ui, t, &t.l("volume"));
                    let vol = app.cfg.f("volume").min(1.0);
                    let out = widgets::text_bar(ui, t, egui::Id::unique("jam-vol"), vol as f32, &t.volume_style, vec2((size - 70.0).max(60.0), fh), true);
                    if let Some(v) = out.changed {
                        app.acts.push(Act::SetVolume(f64::from(v)));
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let label = if app.jam_paused() { "resume" } else { "pause for me" };
                    if widgets::pill(ui, t, label, Pill::Normal).on_hover_text(t.l("pause just for you — the others keep listening (space)")).clicked() {
                        app.acts.push(Act::TogglePlay);
                    }
                    if app.jam.perm_skip && widgets::pill(ui, t, "skip", Pill::Normal).clicked() {
                        app.acts.push(Act::Next);
                    }
                });
                if app.jam.dl_mode != "no" {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let id = app.jam.np.as_ref().and_then(|n| n.id);
                        if widgets::pill(ui, t, "⤓ song", Pill::Normal).on_hover_text(t.l("ask to download this song into your library")).clicked() {
                            later = Some(Box::new(move |a: &mut Nexus| a.jam_request_download(id, "song")));
                        }
                        if widgets::pill(ui, t, "⤓ album", Pill::Normal).on_hover_text(t.l("ask to download the whole album into your library")).clicked() {
                            later = Some(Box::new(move |a: &mut Nexus| a.jam_request_download(id, "album")));
                        }
                    });
                }
                if !app.jam.dl_line.is_empty() {
                    wrapped(ui, &t.l(&app.jam.dl_line), t.font(W::Regular, -1.5), c.dim);
                }
            });
        });
        column(ui, col2, h, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            widgets::eyebrow(ui, t, &t.l("UP NEXT"));
            let qh = 150.0f32.min(h * 0.3);
            panel(ui, t, |ui| {
                egui::ScrollArea::vertical().id_salt("jam-queue").max_height(qh).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    if app.jam.queue.is_empty() {
                        widgets::small_dim(ui, t, &t.l("nothing queued"));
                    }
                    for (title, artist) in &app.jam.queue {
                        let w = ui.available_width();
                        let g = elided(ui, &format!("{title}  —  {artist}"), t.body(), c.text, w);
                        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 2.0), Sense::hover());
                        ui.painter().galley(r.min, g, c.text);
                    }
                });
            });
            let show_lib = app.jam.perm_add || app.jam.dl_mode != "no";
            if show_lib {
                widgets::eyebrow(ui, t, &t.l(if app.jam.perm_add { "ADD FROM THEIR LIBRARY" } else { "THEIR LIBRARY" }));
                let w = ui.available_width();
                let _ = text_field(ui, t, &mut app.jam.lib_search, "search their songs", w, "lib");
                let tip = [
                    if app.jam.perm_add { "click to add to their queue" } else { "" },
                    if app.jam.dl_mode != "no" { "right-click to download" } else { "" },
                ]
                .iter()
                .filter(|x| !x.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
                let view: Vec<usize> = app.jam.lib_filtered().to_vec();
                let can_add = app.jam.perm_add;
                let can_dl = app.jam.dl_mode != "no";
                egui::ScrollArea::vertical().id_salt("jam-lib").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    if view.is_empty() {
                        widgets::small_dim(ui, t, &t.l(if app.jam.lib.is_empty() { "their library is on its way…" } else { "no matches" }));
                    }
                    for i in view {
                        let Some(x) = app.jam.lib.get(i) else { continue };
                        let w = ui.available_width() - 8.0;
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 4.0, alpha(c.select, 0.7));
                        }
                        let g = elided(ui, &format!("{}  —  {}", x.title, x.artist), t.body(), c.text, w - 12.0);
                        ui.painter().galley(pos2(r.left() + 6.0, r.center().y - g.size().y / 2.0), g, c.text);
                        let resp = if tip.is_empty() { resp } else { resp.on_hover_text(t.l(&tip)) };
                        let resp = if can_add { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp };
                        if can_add && resp.clicked() {
                            later = Some(Box::new(move |a: &mut Nexus| a.jam_add_song(i)));
                        }
                        if can_dl {
                            let id = x.id;
                            resp.context_menu(|ui| {
                                if ui.button(t.l("download this song")).clicked() {
                                    later = Some(Box::new(move |a: &mut Nexus| a.jam_request_download(Some(id), "song")));
                                    ui.close();
                                }
                                if ui.button(t.l("download the whole album")).clicked() {
                                    later = Some(Box::new(move |a: &mut Nexus| a.jam_request_download(Some(id), "album")));
                                    ui.close();
                                }
                            });
                        }
                    }
                });
            }
        });
        column(ui, col3, h, |ui| chat(app, ui, t, h));
    });
    later
}
