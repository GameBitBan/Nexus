fn main() {
    let d = std::path::Path::new("/tmp/claude-0/-home-claude/2b76f504-92e2-51fa-82bc-1e097d9fb83a/scratchpad/upd");
    std::fs::create_dir_all(d).unwrap();
    println!("{:?}", nexus_core::update::download_verified("http://127.0.0.1:8765/preview.mp3", "sha256:c60219602943b09285811b0a6d461452612e74463714c489abdae8f5c1c41b86", &d.join("ok.bin")));
    println!("{:?}", nexus_core::update::download_verified("http://127.0.0.1:8765/preview.mp3", "sha256:00", &d.join("bad.bin")));
    println!("{:?}", std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>());
    println!("{:?}", nexus_core::update::install_kind());
}
