//! Headless jam for testing (also against Nexus 1).
//!   jam_cli host <name> <seconds> [file-to-offer]   — streams a 440 Hz tone (44.1 kHz source), answers chat
//!   jam_cli join <code> <name> <out.f32> <seconds>   — listens into a raw 48 kHz-ish f32 file, chats, asks
//!                                                     for a download of id 0
//! Set NEXUS_JAM_RELAY / NEXUS_JAM_STUN to use a local relay.

use nexus_core::jam::{Event, Jam};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut jam = Jam::new();
    match a.get(1).map(String::as_str) {
        Some("host") => host(&mut jam, &a[2], a[3].parse().unwrap_or(30), a.get(4).cloned()),
        Some("join") => join(&mut jam, &a[2], &a[3], &a[4], a[5].parse().unwrap_or(20)),
        _ => eprintln!("usage: jam_cli host <name> <secs> [file] | join <code> <name> <out> <secs>"),
    }
    jam.shutdown();
}

fn print(e: &Event) {
    match e {
        Event::State(s, d) => println!("STATE {s} {d}"),
        Event::Msg(m) => {
            let mut m = m.clone();
            if let Some(Value::String(c)) = m.get("cover") {
                let n = c.len();
                m.insert("cover".into(), json!(format!("<{n} bytes>")));
            }
            if let Some(Value::Array(items)) = m.get("items") {
                let n = items.len();
                m.insert("items".into(), json!(format!("<{n} items>")));
            }
            println!("MSG {}", Value::Object(m));
        }
    }
}

fn host(jam: &mut Jam, name: &str, secs: u64, file: Option<String>) {
    let mut tap = jam.host(name).expect("host");
    println!("CODE {}", jam.code());
    // a stand-in for the engine: 44.1 kHz tone in 10 ms blocks, in real time
    std::thread::spawn(move || {
        let mut t = 0u64;
        let mut next = Instant::now();
        loop {
            let block: Vec<f32> = (0..441)
                .flat_map(|i| {
                    let v = ((t + i) as f32 / 44100.0 * 440.0 * std::f32::consts::TAU).sin() * 0.4;
                    [v, v]
                })
                .collect();
            t += 441;
            tap(&block, 44100);
            next += Duration::from_millis(10);
            if let Some(d) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
        }
    });
    use base64::Engine as _;
    let cover = std::env::var("NEXUS_TEST_COVER").ok().and_then(|p| std::fs::read(p).ok()).map(|b| base64::engine::general_purpose::STANDARD.encode(b)).unwrap_or_default();
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        let Some(e) = jam.wait_event(Duration::from_millis(200)) else { continue };
        print(&e);
        if let Event::Msg(m) = &e {
            let peer = m.get("peer").and_then(Value::as_str).unwrap_or("").to_string();
            match m.get("t").and_then(Value::as_str) {
                Some("joined") => {
                    jam.send(&json!({"t": "lib", "items": [[0, "Tone", "Nexus", "Test Album", 60], [1, "Other", "Nexus", "Test Album", 61]]}), Some(&peer), None);
                    jam.send(&json!({"t": "perm", "add": true, "skip": true, "dl": "yes"}), Some(&peer), None);
                    jam.send(&json!({"t": "np", "title": "Tone", "artist": "Nexus", "album": "Test Album", "fmt": "FLAC 16/44.1", "id": 0, "pos": 1000, "dur": 60000, "playing": true, "queue": [["Other", "Nexus"]], "cover": cover.clone(), "lyrics": {"synced": [[0, "la la"], [5000, "second line"]], "plain": null, "source": "host"}}), Some(&peer), None);
                    jam.send(&json!({"t": "chat", "text": "welcome", "from_name": name}), Some(&peer), None);
                }
                Some("chat") => {
                    let text = m.get("text").and_then(Value::as_str).unwrap_or("");
                    jam.send(&json!({"t": "chat", "text": format!("echo: {text}"), "from_name": name}), Some(&peer), None);
                }
                Some("dlreq") => {
                    let rid = m.get("rid").and_then(Value::as_str).unwrap_or("").to_string();
                    match &file {
                        Some(f) => {
                            let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
                            let base = std::path::Path::new(f).file_name().unwrap().to_string_lossy().to_string();
                            jam.send_files(&peer, &rid, vec![(f.into(), format!("Nexus/Test Album/{base}"), size)]);
                        }
                        None => jam.send(&json!({"t": "dlno", "rid": rid, "why": "no file"}), Some(&peer), None),
                    }
                }
                _ => {}
            }
        }
    }
    jam.leave("");
    while let Some(e) = jam.wait_event(Duration::from_millis(300)) {
        print(&e);
    }
}

fn join(jam: &mut Jam, code: &str, name: &str, out: &str, secs: u64) {
    jam.join(code, name, &format!("file:{out}"), 1.0).expect("join");
    let end = Instant::now() + Duration::from_secs(secs);
    let mut asked = false;
    while Instant::now() < end {
        let Some(e) = jam.wait_event(Duration::from_millis(200)) else { continue };
        print(&e);
        match &e {
            Event::State(s, _) if s == "guest" => {
                jam.send(&json!({"t": "hello", "name": name}), None, None);
                jam.send(&json!({"t": "chat", "text": "hi from rust", "from_name": name}), None, None);
            }
            Event::State(s, _) if s == "off" || s == "error" => break,
            Event::Msg(m) if m.get("t").and_then(Value::as_str) == Some("perm") && !asked => {
                asked = true;
                jam.expect_files("r1");
                jam.send(&json!({"t": "dlreq", "rid": "r1", "id": 0, "what": "song"}), None, None);
            }
            _ => {}
        }
    }
    jam.leave("");
}
