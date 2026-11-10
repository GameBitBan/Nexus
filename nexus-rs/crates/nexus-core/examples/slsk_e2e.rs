use soulseek_rs::{Client, ClientSettings, PeerAddress};
use std::time::Duration;
fn main() {
    let mut a = Client::with_settings(ClientSettings { server_address: PeerAddress::new("127.0.0.1".into(), 22416), listen_port: 22234, shared_directories: vec!["/tmp/claude-0/-home-claude/2b76f504-92e2-51fa-82bc-1e097d9fb83a/scratchpad/music".into()], ..ClientSettings::new("sharer", "pw") });
    a.connect().unwrap();
    println!("sharer login {:?}", a.login());
    let mut b = Client::with_settings(ClientSettings { server_address: PeerAddress::new("127.0.0.1".into(), 22416), listen_port: 22236, ..ClientSettings::new("getter", "pw") });
    b.connect().unwrap();
    println!("getter login {:?}", b.login());
    std::thread::sleep(Duration::from_secs(1));
    let res = b.search("aphex", Duration::from_secs(4)).unwrap();
    for r in &res { println!("{} slots={} speed={} files={}", r.username, r.slots, r.speed, r.files.len()); for f in r.files.iter().take(3) { println!("   {} {} {:?}", f.name, f.size, f.attribs); } }
    let f = &res[0].files[0];
    let (_d, rx) = b.download(f.name.clone(), f.username.clone(), f.size, "/tmp/claude-0/-home-claude/2b76f504-92e2-51fa-82bc-1e097d9fb83a/scratchpad/slskdl".into()).unwrap();
    for s in rx.iter() { println!("status {s:?}"); if s.is_terminal() { break; } }
    println!("downloads {:?}", b.get_all_downloads().iter().map(|d| (&d.filename, &d.status)).collect::<Vec<_>>());
    println!("uploads {:?}", a.uploads());
}
