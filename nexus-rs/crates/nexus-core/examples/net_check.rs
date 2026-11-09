fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    println!("{:?}", nexus_core::tags::online_album_lookup(&a[0], &a[1]));
    println!("{:?}", nexus_core::net::get_bytes("https://api.github.com/repos/GameBitBan/Nexus/releases/latest", 10, 1 << 20).map(|b| b.len()));
    println!("{:?}", nexus_core::net::get_bytes("file:///etc/passwd", 10, 1 << 20).map(|b| b.len()));
}
