//! cargo run --example slsk_sharer -- <server port> <user> <shared dir> <seconds>
use soulseek_rs::{Client, ClientSettings, PeerAddress};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut c = Client::with_settings(ClientSettings {
        server_address: PeerAddress::new("127.0.0.1".into(), a[0].parse().unwrap()),
        listen_port: 22300 + (a[1].len() as u16),
        shared_directories: vec![a[2].clone()],
        ..ClientSettings::new(a[1].clone(), "pw")
    });
    c.connect().unwrap();
    println!("{} login {:?}", a[1], c.login());
    let secs: u64 = a[3].parse().unwrap();
    for _ in 0..secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        for u in c.take_upload_events() { println!("upload {:?}", u); }
    }
}
