use std::sync::atomic::{AtomicBool, AtomicU32};
fn main() {
    for f in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        match nexus_core::inspect::analyze(&f, 1400, &AtomicU32::new(0), &AtomicBool::new(false)) {
            Ok(a) => println!("{:<22} {:?} cutoff={:.0} drop={:.1} bits={} {} | {} {:?} ({:.1}s)", std::path::Path::new(&f).file_name().unwrap().to_string_lossy(),
                a.verdict.kind, a.verdict.cutoff, a.verdict.drop, a.bits, a.cols, a.verdict.text, a.verdict.notes, t.elapsed().as_secs_f32()),
            Err(e) => println!("{f}: {e}"),
        }
    }
}
