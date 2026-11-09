use nexus_core::audio::*;
use std::time::{Duration, Instant};
fn wait_until(e: &Engine, secs: f64, mut f: impl FnMut(&Engine, &[Event]) -> bool) -> Vec<Event> {
    let t0 = Instant::now(); let mut all = vec![];
    while t0.elapsed().as_secs_f64() < secs { let ev = e.poll(); all.extend(ev.clone()); if f(e, &ev) { break; } std::thread::sleep(Duration::from_millis(10)); }
    all
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = &args[0];
    let e = Engine::new(PlaybackOptions { device: format!("file:{out}"), ..Default::default() });
    e.set_volume(1.0);
    match args[1].as_str() {
        "seek" => {
            e.load(&args[2], 1.0, true);                                   // start at 1.0 s
            wait_until(&e, 3.0, |e, _| e.position() > 1.3);
            println!("pos after start@1.0: {:.3}", e.position());
            e.seek(0.25);                                                  // jump back
            wait_until(&e, 3.0, |e, _| e.position() < 0.5 && e.position() > 0.3);
            println!("pos after seek 0.25: {:.3}", e.position());
            e.pause(); std::thread::sleep(Duration::from_millis(400)); let p1 = e.position();
            std::thread::sleep(Duration::from_millis(300)); println!("paused, pos steady: {}", (e.position() - p1).abs() < 0.001);
            e.play();
            let ev = wait_until(&e, 5.0, |_, ev| ev.contains(&Event::Ended));
            println!("ended: {}", ev.contains(&Event::Ended));
        }
        "eq" => {
            e.load(&args[2], 0.0, true);
            wait_until(&e, 2.0, |e, _| e.position() > 0.5);
            e.set_eq(EqSettings { on: true, bands: [0.0, 0.0, 0.0, 0.0, 6.0, 6.0, 6.0, 0.0, 0.0, 0.0], preamp: 0.0 });
            wait_until(&e, 2.0, |e, _| e.position() > 1.4);
            e.set_eq(EqSettings::default());
            let ev = wait_until(&e, 5.0, |_, ev| ev.contains(&Event::Ended));
            println!("eq run ended: {}", ev.contains(&Event::Ended));
        }
        "formats" => {
            for f in &args[2..] {
                e.load(f, 0.0, true);
                let ev = wait_until(&e, 4.0, |e, ev| ev.iter().any(|x| matches!(x, Event::Error(_))) || e.position() > 0.4);
                let err = ev.iter().find_map(|x| if let Event::Error(m) = x { Some(m.clone()) } else { None });
                println!("{:<28} dur={:>6.2}  pos={:.2}  rate={}  {}", std::path::Path::new(f).file_name().unwrap().to_string_lossy(),
                         e.duration(), e.position(), e.sample_rate(), err.unwrap_or_else(|| "ok".into()));
                e.stop(); std::thread::sleep(Duration::from_millis(100));
            }
        }
        _ => {}
    }
}
