use nexus_core::audio::*;
fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let dev = std::env::var("NEXUS_TEST_OUT").map(|p| format!("file:{p}")).unwrap_or_default();
    let e = Engine::new(PlaybackOptions { device: dev, ..Default::default() });
    e.set_volume(1.0);
    e.load(&files[0], 0.0, true);
    if files.len() > 1 { e.set_next(Some(files[1].clone())); }
    let t0 = std::time::Instant::now();
    loop {
        for ev in e.poll() { println!("{:>6.2}s event {ev:?}  pos={:.2}", t0.elapsed().as_secs_f64(), e.position()); }
        if e.state() == State::Stopped && t0.elapsed().as_secs() > 1 { break; }
        if t0.elapsed().as_secs() > 20 { println!("timeout"); break; }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    println!("output: {:?}", e.output_info());
}
