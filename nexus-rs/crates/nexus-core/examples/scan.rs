use nexus_core::scanner;
use std::path::PathBuf;
fn main() {
    let folders: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let covers = std::env::temp_dir().join("nexus-scan-covers");
    std::fs::create_dir_all(&covers).unwrap();
    let p = scanner::ScanProgress::default();
    let t0 = std::time::Instant::now();
    let tracks = scanner::scan(&folders, &[], &covers, true, &p);
    eprintln!("{} tracks in {:?}", tracks.len(), t0.elapsed());
    println!("{}", serde_json::to_string(&tracks).unwrap());
}
