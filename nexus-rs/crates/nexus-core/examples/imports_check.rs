fn main() {
    for f in std::env::args().skip(1) {
        let data = std::fs::read(&f).unwrap();
        let r = nexus_core::imports::parse_import(&f, &data).unwrap();
        let name = std::path::Path::new(&f).file_name().unwrap().to_string_lossy().to_string();
        let pl: Vec<(String, usize)> = r.playlists.iter().map(|(n, i)| (n.clone(), i.len())).collect();
        let al: Vec<&str> = r.albums.iter().map(|a| a.album.as_str()).collect();
        println!("{name} {} {:?} {} {:?} {:?}", r.source, pl, r.liked.len(), r.artists, al);
    }
}
