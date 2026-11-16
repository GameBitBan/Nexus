//! Jam — listen together. One Nexus hosts and streams whatever it plays; up to four friends join with a
//! six-character code and hear it about a second behind, with chat, a shared queue and downloads.
//!
//! Wire-compatible with Nexus 1 (Python), so a jam can mix both versions:
//!  - the code → scrypt → an ntfy topic (signalling) and an AES-GCM key; every relay message is sealed,
//!    so the relay only ever sees noise
//!  - WebRTC (STUN, no media server): one Opus audio track (48 kHz, 20 ms, 320 kbps) plus a data channel
//!    "jam" carrying JSON messages and, for downloads, 32 KB binary chunks

pub mod audio;

use crate::util::safe_name;
use base64::Engine as _;
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use webrtc::api::API;
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_remote::TrackRemote;

pub use audio::GuestCtl;

/// No 0/O, 1/I/L, 2/Z, 5/S, 8/B mix-ups.
pub const ALPHABET: &str = "ACDEFGHJKMNPQRTUVWXY34679";
pub const MAX_LISTENERS: usize = 4;
const CHUNK: usize = 32 * 1024;
/// One download request can't be bigger than this.
const RX_LIMIT: u64 = 8_000_000_000;

pub fn relay() -> String {
    std::env::var("NEXUS_JAM_RELAY").unwrap_or_else(|_| "https://ntfy.sh".into()).trim_end_matches('/').to_string()
}

pub fn stun() -> Vec<String> {
    std::env::var("NEXUS_JAM_STUN")
        .unwrap_or_else(|_| "stun:stun.l.google.com:19302,stun:stun.cloudflare.com:3478".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the system random source");
    b
}

pub fn new_code() -> String {
    let a: Vec<char> = ALPHABET.chars().collect();
    let mut raw = String::new();
    while raw.len() < 6 {
        // rejection sampling: no bias towards the first letters
        let b = random_bytes::<1>()[0];
        if (b as usize) < 250 {
            raw.push(a[b as usize % a.len()]);
        }
    }
    format!("{}-{}", &raw[..3], &raw[3..])
}

/// " k7q m2x " → "K7Q-M2X"; anything that isn't six letters/numbers → None.
pub fn norm_code(code: &str) -> Option<String> {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    (c.len() == 6).then(|| format!("{}-{}", &c[..3], &c[3..]))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn short_id() -> String {
    hex(&random_bytes::<5>())
}

/// What the code turns into: the relay topic and the key that seals every relay message.
pub struct Crypto {
    pub topic: String,
    key: [u8; 32],
}

impl Crypto {
    pub fn new(code: &str) -> Option<Self> {
        // scrypt, so a code can't be brute-forced from its relay topic in seconds (6 characters is only ~30 bits)
        let c = norm_code(code)?;
        let params = scrypt::Params::new(14, 8, 1).ok()?;
        let mut k = [0u8; 64];
        scrypt::scrypt(c.as_bytes(), b"nexus-jam-v2", &params, &mut k).ok()?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&k[..32]);
        Some(Crypto { topic: format!("nexusjam2{}", hex(&k[32..46])), key })
    }

    pub fn seal(&self, v: &Value) -> String {
        use aes_gcm::aead::{Aead, KeyInit};
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
        let _ = z.write_all(v.to_string().as_bytes());
        let packed = z.finish().unwrap_or_default();
        let nonce = random_bytes::<12>();
        let cipher = aes_gcm::Aes256Gcm::new(&self.key.into());
        let ct = cipher.encrypt(&nonce.into(), packed.as_slice()).unwrap_or_default();
        let mut raw = nonce.to_vec();
        raw.extend_from_slice(&ct);
        base64::engine::general_purpose::URL_SAFE.encode(raw)
    }

    pub fn open(&self, text: &str) -> Option<Value> {
        use aes_gcm::aead::{Aead, KeyInit};
        let raw = base64::engine::general_purpose::URL_SAFE.decode(text.trim().as_bytes()).ok()?;
        if raw.len() < 12 + 16 {
            return None;
        }
        let cipher = aes_gcm::Aes256Gcm::new(&self.key.into());
        let nonce: [u8; 12] = raw[..12].try_into().ok()?;
        let packed = cipher.decrypt(&nonce.into(), &raw[12..]).ok()?;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(packed.as_slice()).take(1_000_001).read_to_end(&mut out).ok()?;
        if out.len() > 1_000_000 {
            return None;
        }
        serde_json::from_slice(&out).ok()
    }
}

/// Messages come from another computer: keep every field a sane size (same limits as Nexus 1).
pub fn clip(mut d: Map<String, Value>) -> Map<String, Value> {
    fn cut(v: &Value, n: usize) -> Value {
        let s = match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        Value::String(s.chars().take(n).collect())
    }
    for (k, n) in [("text", 500), ("from_name", 24), ("name", 24), ("title", 300), ("artist", 300), ("album", 300), ("fmt", 40), ("why", 120)] {
        if let Some(v) = d.get(k) {
            let c = cut(v, n);
            d.insert(k.into(), c);
        }
    }
    if d.get("cover").and_then(Value::as_str).is_some_and(|c| c.len() > 1_500_000) || d.get("cover").is_some_and(|c| !c.is_string()) {
        d.remove("cover");
    }
    if let Some(Value::Array(q)) = d.get("queue") {
        let q: Vec<Value> = q
            .iter()
            .take(20)
            .filter_map(|x| x.as_array().filter(|a| a.len() >= 2).map(|a| json!([cut(&a[0], 200), cut(&a[1], 200)])))
            .collect();
        d.insert("queue".into(), Value::Array(q));
    }
    if let Some(Value::Array(items)) = d.get("items") {
        let items: Vec<Value> = items.iter().take(250).filter(|x| x.as_array().is_some_and(|a| a.len() >= 5)).cloned().collect();
        d.insert("items".into(), Value::Array(items));
    }
    if let Some(ly) = d.get("lyrics") {
        if !ly.is_null() && (!ly.is_object() || ly.to_string().len() > 200_000) {
            d.insert("lyrics".into(), Value::Null);
        }
    }
    d
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Off,
    Host,
    Guest,
}

/// What the jam tells the UI.
#[derive(Clone, Debug)]
pub enum Event {
    /// `state` is "hosting" (detail: the code, or "code|problem"), "joining" (the code), "guest" (the host's
    /// name), "error" (what went wrong) or "off" (why, may be empty)
    State(String, String),
    /// A message from the other side (`t` says what it is), with `peer` and `name` added on the host.
    /// Also, from the jam itself: joined / left (host), fsent (host: upload progress), fprog / fdone (guest).
    Msg(Map<String, Value>),
}

#[derive(Clone)]
struct Peer {
    pc: Arc<RTCPeerConnection>,
    dc: Option<Arc<RTCDataChannel>>,
    name: String,
}

#[derive(Default)]
struct RxJob {
    got: u64,
    total: u64,
    files: Vec<String>,
    file: Option<std::fs::File>,
    path: PathBuf,
    left: i64,
    n: u64,
    i: u64,
    dead: bool,
    last: Option<Instant>,
}

#[derive(Default)]
struct Rx {
    jobs: HashMap<String, RxJob>,
    cur: Option<String>,
}

type Outgoing = (String, Option<String>, Option<String>);

struct Session {
    me: String,
    name: String,
    code: String,
    crypto: Crypto,
    relay: String,
    stop: Arc<AtomicBool>,
    tx: Sender<Event>,
    api: API,
    peers: Mutex<HashMap<String, Peer>>,
    host_name: Mutex<String>,
    track: Option<Arc<TrackLocalStaticSample>>,
    sending: Mutex<HashMap<String, Arc<AtomicBool>>>,
    rx: Mutex<Rx>,
    expect: Mutex<HashSet<String>>,
    connected: AtomicBool,
    ctl: Arc<GuestCtl>,
    device: String,
}

impl Session {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn emit(&self, e: Event) {
        if !self.stopped() {
            let _ = self.tx.send(e);
        }
    }

    fn state(&self, st: &str, detail: impl Into<String>) {
        self.emit(Event::State(st.into(), detail.into()));
    }

    fn msg(&self, v: Value) {
        if let Value::Object(m) = v {
            self.emit(Event::Msg(m));
        }
    }

    fn ice() -> RTCConfiguration {
        let urls = stun();
        RTCConfiguration {
            ice_servers: if urls.is_empty() { vec![] } else { vec![RTCIceServer { urls, ..Default::default() }] },
            ..Default::default()
        }
    }

    /// Seal and post one message to the relay (blocking; run off the async threads).
    fn publish_blocking(relay: &str, crypto: &Crypto, v: &Value) -> Result<(), String> {
        let body = crypto.seal(v);
        let (code, _) = crate::net::post(&format!("{relay}/{}", crypto.topic), &[("Firebase", "no")], body.as_bytes(), 15)?;
        if (200..300).contains(&code) { Ok(()) } else { Err(format!("the relay said {code}")) }
    }

    async fn publish(self: &Arc<Self>, v: Value) -> Result<(), String> {
        let s = self.clone();
        tokio::task::spawn_blocking(move || Session::publish_blocking(&s.relay, &s.crypto, &v)).await.map_err(|e| e.to_string())?
    }

    /// Streams the relay topic on its own thread; messages meant for us come out of the returned channel.
    fn listen(self: &Arc<Self>) -> tokio::sync::mpsc::UnboundedReceiver<Map<String, Value>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let s = self.clone();
        std::thread::Builder::new()
            .name("nexus-jam-relay".into())
            .spawn(move || {
                let home = relay_host(&s.relay);
                let mut seen: VecDeque<String> = VecDeque::new();
                while !s.stopped() {
                    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).saturating_sub(5);
                    let url = format!("{}/{}/json?since={since}", s.relay, s.crypto.topic);
                    if crate::net::check(&url).is_err() {
                        std::thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                    let resp = crate::net::agent()
                        .get(&url)
                        .config()
                        .timeout_global(None)
                        .timeout_connect(Some(Duration::from_secs(15)))
                        .timeout_per_read(Some(Duration::from_secs(70)))
                        .build()
                        .call();
                    let resp = match resp {
                        Ok(r) if r.status().as_u16() == 200 => r,
                        _ => {
                            std::thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                    };
                    let mut rd = std::io::BufReader::new(resp.into_body().into_with_config().limit(u64::MAX).reader());
                    let mut line = Vec::new();
                    loop {
                        line.clear();
                        match (&mut rd).take(300_000).read_until(b'\n', &mut line) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                        if s.stopped() {
                            return;
                        }
                        if line.last() != Some(&b'\n') && line.len() >= 300_000 {
                            break; // a line that big isn't ntfy: reconnect
                        }
                        let Ok(ev) = serde_json::from_slice::<Value>(&line) else { continue };
                        if ev.get("event").and_then(Value::as_str) != Some("message") {
                            continue;
                        }
                        if let Some(id) = ev.get("id").and_then(Value::as_str) {
                            if seen.iter().any(|x| x == id) {
                                continue; // sent again after a reconnect
                            }
                            seen.push_back(id.to_string());
                            if seen.len() > 200 {
                                seen.pop_front();
                            }
                        }
                        let mut text = ev.get("message").and_then(Value::as_str).unwrap_or("").to_string();
                        if let Some(att) = ev.get("attachment").and_then(|a| a.get("url")).and_then(Value::as_str) {
                            // only the relay's own attachments
                            if relay_host(att) != home || !(att.starts_with("https://") || att.starts_with("http://")) {
                                continue;
                            }
                            match crate::net::get_bytes(att, 15, 200_000) {
                                Ok(b) => text = String::from_utf8_lossy(&b).into_owned(),
                                Err(_) => continue,
                            }
                        }
                        if text.is_empty() || text.len() > 200_000 {
                            continue;
                        }
                        let Some(Value::Object(m)) = s.crypto.open(&text) else { continue }; // someone else's noise
                        let from = m.get("from").and_then(Value::as_str).unwrap_or("");
                        let to = m.get("to").and_then(Value::as_str);
                        if from != s.me && to.is_none_or(|t| t == s.me) && tx.send(m).is_err() {
                            return;
                        }
                    }
                    if !s.stopped() {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
            })
            .ok();
        rx
    }

    /// The next relay message, or None once the jam stops (or after `wait`).
    async fn next(self: &Arc<Self>, inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Map<String, Value>>, wait: Duration) -> Option<Map<String, Value>> {
        let end = Instant::now() + wait;
        while !self.stopped() {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            match tokio::time::timeout(left.min(Duration::from_millis(400)), inbox.recv()).await {
                Ok(Some(m)) => return Some(m),
                Ok(None) => return None,
                Err(_) => {}
            }
        }
        None
    }

    async fn drop_peer(self: &Arc<Self>, peer: &str) -> Option<Peer> {
        let p = self.peers.lock().remove(peer);
        if let Some(p) = &p {
            let _ = p.pc.close().await;
        }
        p
    }

    /// Sends queued JSON in order (one at a time, so a listener gets "lib" before "perm" before "np").
    async fn writer(self: Arc<Self>, mut q: tokio::sync::mpsc::UnboundedReceiver<Outgoing>) {
        while let Some((text, peer, exclude)) = q.recv().await {
            if self.stopped() {
                break;
            }
            let chans: Vec<Arc<RTCDataChannel>> = self
                .peers
                .lock()
                .iter()
                .filter(|(p, _)| peer.as_deref().is_none_or(|x| x == p.as_str()) && exclude.as_deref() != Some(p.as_str()))
                .filter_map(|(_, x)| x.dc.clone())
                .collect();
            for ch in chans {
                if ch.ready_state() == RTCDataChannelState::Open {
                    let _ = ch.send_text(text.clone()).await;
                }
            }
        }
    }

    // ---------------------------------------------------------------- hosting

    async fn host_main(self: Arc<Self>) {
        let mut inbox = self.listen();
        while !self.stopped() {
            let Some(msg) = self.next(&mut inbox, Duration::from_secs(3600)).await else { continue };
            let k = msg.get("k").and_then(Value::as_str).unwrap_or("");
            let from = msg.get("from").and_then(Value::as_str).unwrap_or("").to_string();
            if from.is_empty() || from.len() > 64 {
                continue;
            }
            let res = match k {
                "hello" => self.host_offer(&from, &msg).await,
                "answer" => {
                    let pc = self.peers.lock().get(&from).map(|p| p.pc.clone());
                    match (pc, msg.get("sdp").and_then(Value::as_str)) {
                        (Some(pc), Some(sdp)) => match RTCSessionDescription::answer(sdp.to_string()) {
                            Ok(d) => pc.set_remote_description(d).await.map_err(|e| e.to_string()),
                            Err(e) => Err(e.to_string()),
                        },
                        _ => Ok(()),
                    }
                }
                "bye" => {
                    if let Some(p) = self.drop_peer(&from).await {
                        self.msg(json!({"t": "left", "peer": from, "name": p.name, "failed": false}));
                    }
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = res {
                self.state("hosting", format!("{}|{e}", self.code));
            }
        }
    }

    async fn host_offer(self: &Arc<Self>, peer: &str, msg: &Map<String, Value>) -> Result<(), String> {
        let full = {
            let peers = self.peers.lock();
            peers.len() >= MAX_LISTENERS && !peers.contains_key(peer)
        };
        if full {
            return self.publish(json!({"k": "full", "from": self.me, "to": peer})).await;
        }
        self.drop_peer(peer).await;
        let name: String = msg.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("friend").chars().take(24).collect();
        let pc = Arc::new(self.api.new_peer_connection(Session::ice()).await.map_err(|e| e.to_string())?);
        if let Some(track) = &self.track {
            let sender = pc.add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>).await.map_err(|e| e.to_string())?;
            // RTCP has to be read for the sender's reports to work
            tokio::spawn(async move {
                let mut b = vec![0u8; 1500];
                while sender.read(&mut b).await.is_ok() {}
            });
        }
        let dc = pc.create_data_channel("jam", None).await.map_err(|e| e.to_string())?;
        self.peers.lock().insert(peer.to_string(), Peer { pc: pc.clone(), dc: Some(dc.clone()), name: name.clone() });
        {
            let (s, p, n) = (self.clone(), peer.to_string(), name.clone());
            dc.on_open(Box::new(move || {
                s.msg(json!({"t": "joined", "peer": p, "name": n}));
                Box::pin(async {})
            }));
        }
        {
            let (s, p) = (self.clone(), peer.to_string());
            dc.on_message(Box::new(move |m: DataChannelMessage| {
                if m.is_string {
                    if let Ok(Value::Object(mut d)) = serde_json::from_slice::<Value>(&m.data) {
                        let name = s.peers.lock().get(&p).map(|x| x.name.clone()).unwrap_or_else(|| "friend".into());
                        d.insert("peer".into(), json!(p));
                        d.insert("name".into(), json!(name));
                        if d.get("t").and_then(Value::as_str).is_some_and(|t| !matches!(t, "joined" | "left" | "fsent" | "fprog" | "fdone")) {
                            s.emit(Event::Msg(clip(d)));
                        }
                    }
                }
                Box::pin(async {})
            }));
        }
        {
            let (s, p, weak) = (self.clone(), peer.to_string(), Arc::downgrade(&pc));
            pc.on_peer_connection_state_change(Box::new(move |st: RTCPeerConnectionState| {
                let (s, p, weak) = (s.clone(), p.clone(), weak.clone());
                Box::pin(async move {
                    if matches!(st, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
                        let current = s.peers.lock().get(&p).map(|x| x.pc.clone());
                        if let (Some(cur), Some(me)) = (current, weak.upgrade()) {
                            if Arc::ptr_eq(&cur, &me) {
                                if let Some(gone) = s.drop_peer(&p).await {
                                    s.msg(json!({"t": "left", "peer": p, "name": gone.name, "failed": st == RTCPeerConnectionState::Failed}));
                                }
                            }
                        }
                    }
                })
            }));
        }
        let offer = pc.create_offer(None).await.map_err(|e| e.to_string())?;
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.map_err(|e| e.to_string())?;
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;
        let sdp = pc.local_description().await.map(|d| d.sdp).unwrap_or_default();
        self.publish(json!({"k": "offer", "from": self.me, "to": peer, "sdp": sdp, "host": self.name})).await
    }

    // ---------------------------------------------------------------- joining

    async fn guest_main(self: Arc<Self>) {
        let mut inbox = self.listen();
        tokio::time::sleep(Duration::from_millis(800)).await; // let the subscription settle first
        let mut tries = 0;
        let offer = loop {
            if self.stopped() {
                return;
            }
            if let Err(e) = self.publish(json!({"k": "hello", "from": self.me, "name": self.name})).await {
                tries += 1;
                if tries >= 3 {
                    self.state("error", format!("couldn't reach the jam relay ({e})"));
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            let end = Instant::now() + Duration::from_secs(12);
            let mut got = None;
            while let Some(m) = self.next(&mut inbox, end.saturating_duration_since(Instant::now())).await {
                if matches!(m.get("k").and_then(Value::as_str), Some("offer" | "full")) {
                    got = Some(m);
                    break;
                }
            }
            match got {
                Some(m) if m.get("k").and_then(Value::as_str) == Some("full") => {
                    self.state("error", "that jam is full (4 listeners max)");
                    return;
                }
                Some(m) => break m,
                None => {
                    tries += 1;
                    if tries >= 3 {
                        self.state("error", "no one is hosting with that code (or they closed it)");
                        return;
                    }
                }
            }
        };
        if let Err(e) = self.guest_connect(&offer).await {
            self.state("error", format!("couldn't connect: {e}"));
            return;
        }
        // keep listening: the host says bye when they end the jam
        let host_id = offer.get("from").and_then(Value::as_str).unwrap_or("").to_string();
        while !self.stopped() {
            if let Some(m) = self.next(&mut inbox, Duration::from_secs(3600)).await {
                if m.get("k").and_then(Value::as_str) == Some("bye") && m.get("from").and_then(Value::as_str) == Some(host_id.as_str()) {
                    self.state("ended", "the jam ended");
                    return;
                }
            }
        }
    }

    async fn guest_connect(self: &Arc<Self>, msg: &Map<String, Value>) -> Result<(), String> {
        let host: String = msg.get("host").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("host").chars().take(24).collect();
        *self.host_name.lock() = host.clone();
        let sdp = msg.get("sdp").and_then(Value::as_str).ok_or("the host's offer was empty")?.to_string();
        let pc = Arc::new(self.api.new_peer_connection(Session::ice()).await.map_err(|e| e.to_string())?);
        self.peers.lock().insert("host".into(), Peer { pc: pc.clone(), dc: None, name: host.clone() });
        {
            let s = self.clone();
            pc.on_track(Box::new(move |track: Arc<TrackRemote>, _, _| {
                let s = s.clone();
                tokio::spawn(async move { s.consume(track).await });
                Box::pin(async {})
            }));
        }
        {
            let s = self.clone();
            pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
                let s = s.clone();
                Box::pin(async move {
                    if dc.label() != "jam" {
                        return;
                    }
                    if let Some(p) = s.peers.lock().get_mut("host") {
                        p.dc = Some(dc.clone());
                    }
                    // the host's channel arrives open: we're in (before any of its messages come through)
                    s.connected.store(true, Ordering::Relaxed);
                    let h = s.host_name.lock().clone();
                    s.state("guest", h);
                    {
                        let s = s.clone();
                        dc.on_message(Box::new(move |m: DataChannelMessage| {
                            if m.is_string {
                                if let Ok(Value::Object(d)) = serde_json::from_slice::<Value>(&m.data) {
                                    match d.get("t").and_then(Value::as_str) {
                                        Some("fstart" | "fend" | "fdone") => s.rx_ctl(&d),
                                        Some("joined" | "left" | "fsent" | "fprog") => {}
                                        Some(_) => s.emit(Event::Msg(clip(d))),
                                        None => {}
                                    }
                                }
                            } else {
                                s.rx_chunk(&m.data);
                            }
                            Box::pin(async {})
                        }));
                    }
                    {
                        let s = s.clone();
                        dc.on_close(Box::new(move || {
                            if s.connected.load(Ordering::Relaxed) {
                                s.state("ended", "the jam ended");
                            }
                            Box::pin(async {})
                        }));
                    }
                })
            }));
        }
        {
            let s = self.clone();
            pc.on_peer_connection_state_change(Box::new(move |st: RTCPeerConnectionState| {
                let s = s.clone();
                Box::pin(async move {
                    if st == RTCPeerConnectionState::Failed {
                        if s.connected.load(Ordering::Relaxed) {
                            s.state("ended", "lost the connection to the host");
                        } else {
                            s.state("error", "couldn't connect to the host directly — both of your routers block it. try again, or one of you switch network (e.g. a phone hotspot)");
                        }
                    }
                })
            }));
        }
        let offer = RTCSessionDescription::offer(sdp).map_err(|e| e.to_string())?;
        pc.set_remote_description(offer).await.map_err(|e| e.to_string())?;
        let answer = pc.create_answer(None).await.map_err(|e| e.to_string())?;
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(answer).await.map_err(|e| e.to_string())?;
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;
        let sdp = pc.local_description().await.map(|d| d.sdp).unwrap_or_default();
        let to = msg.get("from").and_then(Value::as_str).unwrap_or("");
        self.publish(json!({"k": "answer", "from": self.me, "to": to, "sdp": sdp})).await
    }

    /// Guest: the host's audio → decoder thread → our own output.
    async fn consume(self: Arc<Self>, track: Arc<TrackRemote>) {
        if !track.codec().capability.mime_type.eq_ignore_ascii_case("audio/opus") {
            return;
        }
        let (ptx, prx) = crossbeam_channel::bounded::<Bytes>(500);
        {
            let s = self.clone();
            std::thread::Builder::new()
                .name("nexus-jam-play".into())
                .spawn(move || {
                    let mut out = match audio::GuestOutput::open(&s.device, s.ctl.clone()) {
                        Ok(o) => o,
                        Err(e) => {
                            s.state("error", format!("couldn't open the sound output: {e}"));
                            return;
                        }
                    };
                    let mut dec = match audio::OpusDecoder::new(out.rate) {
                        Ok(d) => d,
                        Err(e) => {
                            s.state("error", e);
                            return;
                        }
                    };
                    let mut buf = Vec::with_capacity(8192);
                    while let Ok(p) = prx.recv() {
                        if s.stopped() {
                            break;
                        }
                        buf.clear();
                        dec.decode(&p, &mut buf);
                        out.push(&buf);
                    }
                })
                .ok();
        }
        while !self.stopped() {
            match tokio::time::timeout(Duration::from_secs(2), track.read_rtp()).await {
                Ok(Ok((pkt, _))) => {
                    let _ = ptx.try_send(pkt.payload);
                }
                Ok(Err(_)) => break,
                Err(_) => {}
            }
        }
    }

    // ---------------------------------------------------------------- downloads (host → guest)

    async fn send_files(self: Arc<Self>, peer: String, rid: String, files: Vec<(PathBuf, String, u64)>, cancel: Arc<AtomicBool>) {
        let total = files.iter().map(|f| f.2).sum::<u64>().max(1);
        let n = files.len();
        let mut sent = 0u64;
        let mut last = Instant::now() - Duration::from_secs(1);
        let mut ok = true;
        let ch = self.peers.lock().get(&peer).and_then(|p| p.dc.clone());
        let gone = |ch: &Option<Arc<RTCDataChannel>>| {
            ch.as_ref().is_none_or(|c| c.ready_state() != RTCDataChannelState::Open) || cancel.load(Ordering::Relaxed) || self.stopped()
        };
        'files: for (i, (path, name, size)) in files.iter().enumerate() {
            if gone(&ch) {
                ok = false;
                break;
            }
            let c = ch.as_ref().expect("checked");
            let start = json!({"t": "fstart", "rid": rid, "i": i, "n": n, "name": name, "size": size, "total": total});
            if c.send_text(start.to_string()).await.is_err() {
                ok = false;
                break;
            }
            let mut f = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!("jam send {}: {e}", path.display());
                    ok = false;
                    break;
                }
            };
            let mut buf = vec![0u8; CHUNK];
            loop {
                while !gone(&ch) && c.buffered_amount().await > (1 << 20) {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                if gone(&ch) {
                    ok = false;
                    break 'files;
                }
                let k = match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(k) => k,
                    Err(_) => {
                        ok = false;
                        break 'files;
                    }
                };
                if c.send(&Bytes::copy_from_slice(&buf[..k])).await.is_err() {
                    ok = false;
                    break 'files;
                }
                sent += k as u64;
                if last.elapsed() > Duration::from_millis(400) {
                    last = Instant::now();
                    self.msg(json!({"t": "fsent", "rid": rid, "peer": peer, "sent": sent, "total": total, "i": i, "n": n}));
                }
            }
            if c.send_text(json!({"t": "fend", "rid": rid, "i": i}).to_string()).await.is_err() {
                ok = false;
                break;
            }
        }
        if let Some(c) = &ch {
            if c.ready_state() == RTCDataChannelState::Open {
                let _ = c.send_text(json!({"t": "fdone", "rid": rid, "ok": ok}).to_string()).await;
            }
        }
        self.sending.lock().remove(&rid);
        self.msg(json!({"t": "fsent", "rid": rid, "peer": peer, "sent": sent, "total": total, "done": true, "ok": ok, "n": n}));
    }

    fn incoming_dir(rid: &str) -> PathBuf {
        crate::paths::data_dir().join("jam-incoming").join(rid)
    }

    /// Guest: a file starts / ends / everything's done.
    fn rx_ctl(&self, d: &Map<String, Value>) {
        let rid = match d.get("rid") {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        };
        if !self.expect.lock().contains(&rid) {
            return; // we never asked for this
        }
        let int = |k: &str| d.get(k).and_then(Value::as_u64).unwrap_or(0);
        let mut rx = self.rx.lock();
        let job = rx.jobs.entry(rid.clone()).or_insert_with(|| RxJob { total: int("total").max(1), n: int("n").max(1), ..Default::default() });
        match d.get("t").and_then(Value::as_str) {
            Some("fstart") => {
                job.file = None;
                if job.files.len() >= 500 || int("total") > RX_LIMIT {
                    job.dead = true;
                    return;
                }
                job.i = int("i");
                job.n = int("n").max(1);
                job.total = int("total").max(1);
                job.left = int("size") as i64;
                let name = d.get("name").and_then(Value::as_str).unwrap_or("file").replace('\\', "/");
                let ext = std::path::Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let wanted = crate::library::AUDIO_EXT.contains(&ext.as_str()) || crate::library::IMG_EXT.contains(&ext.as_str());
                if wanted && !job.dead {
                    // the last three parts of the name, each made safe: "Artist/Album/01 song.flac"
                    let parts: Vec<String> = name.split('/').filter(|x| !matches!(*x, "" | "." | "..")).map(|x| safe_name(x, "file")).collect();
                    let parts = if parts.is_empty() { vec!["file".to_string()] } else { parts[parts.len().saturating_sub(3)..].to_vec() };
                    let mut dest = Session::incoming_dir(&rid);
                    for p in &parts {
                        dest.push(p);
                    }
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let part = PathBuf::from(format!("{}.part", dest.display()));
                    match std::fs::File::create(&part) {
                        Ok(f) => {
                            job.file = Some(f);
                            job.path = dest;
                        }
                        Err(e) => log::warn!("jam receive: {e}"),
                    }
                }
                rx.cur = Some(rid);
            }
            Some("fend") => {
                if job.file.take().is_some() {
                    let part = PathBuf::from(format!("{}.part", job.path.display()));
                    if std::fs::rename(&part, &job.path).is_ok() {
                        job.files.push(job.path.to_string_lossy().into_owned());
                    }
                }
            }
            Some("fdone") => {
                job.file = None;
                let files = std::mem::take(&mut job.files);
                rx.jobs.remove(&rid);
                rx.cur = None;
                drop(rx);
                self.expect.lock().remove(&rid);
                let ok = d.get("ok").and_then(Value::as_bool).unwrap_or(false);
                self.msg(json!({"t": "fdone", "rid": rid, "ok": ok, "paths": files}));
            }
            _ => {}
        }
    }

    fn rx_chunk(&self, b: &[u8]) {
        let mut rx = self.rx.lock();
        let Some(cur) = rx.cur.clone() else { return };
        let Some(job) = rx.jobs.get_mut(&cur) else { return };
        job.got += b.len() as u64;
        if job.file.is_some() {
            job.left -= b.len() as i64;
            if job.left < 0 || job.got > RX_LIMIT.min(job.total + 1_000_000) {
                // more than they said: drop the file
                job.file = None;
                let _ = std::fs::remove_file(format!("{}.part", job.path.display()));
                return;
            }
            if let Some(f) = job.file.as_mut() {
                let _ = f.write_all(b);
            }
        }
        if job.last.is_none_or(|t| t.elapsed() > Duration::from_millis(300)) {
            job.last = Some(Instant::now());
            let m = json!({"t": "fprog", "rid": cur, "got": job.got, "total": job.total, "i": job.i, "n": job.n});
            drop(rx);
            self.msg(m);
        }
    }
}

fn relay_host(url: &str) -> String {
    url.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase()
}

fn build_api() -> Result<API, String> {
    use webrtc::api::APIBuilder;
    use webrtc::api::interceptor_registry::register_default_interceptors;
    use webrtc::api::media_engine::MediaEngine;
    use webrtc::interceptor::registry::Registry;
    let mut m = MediaEngine::default();
    m.register_default_codecs().map_err(|e| e.to_string())?;
    let registry = register_default_interceptors(Registry::new(), &mut m).map_err(|e| e.to_string())?;
    Ok(APIBuilder::new().with_media_engine(m).with_interceptor_registry(registry).build())
}

/// The jam, as the UI sees it. All methods return straight away; what happens comes back from `poll`.
pub struct Jam {
    rt: Option<tokio::runtime::Runtime>,
    sess: Option<Arc<Session>>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    me: String,
    role: Role,
    code: String,
    /// The listener's own volume / pause (valid while a guest).
    pub ctl: Arc<GuestCtl>,
    encoder: Option<Arc<AtomicBool>>,
    closing: Option<tokio::task::JoinHandle<()>>,
    out: Option<tokio::sync::mpsc::UnboundedSender<Outgoing>>,
}

impl Default for Jam {
    fn default() -> Self {
        Jam::new()
    }
}

impl Jam {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Jam { rt: None, sess: None, tx, rx, me: short_id(), role: Role::Off, code: String::new(), ctl: GuestCtl::new(1.0), encoder: None, closing: None, out: None }
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn host_name(&self) -> String {
        self.sess.as_ref().map(|s| s.host_name.lock().clone()).unwrap_or_default()
    }

    /// Host: how many people are in (connected or connecting).
    pub fn listeners(&self) -> usize {
        self.sess.as_ref().map(|s| s.peers.lock().len()).unwrap_or(0)
    }

    fn runtime(&mut self) -> Result<tokio::runtime::Handle, String> {
        if self.rt.is_none() {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("nexus-jam")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            self.rt = Some(rt);
        }
        Ok(self.rt.as_ref().expect("just made").handle().clone())
    }

    fn session(&self, role: Role, code: &str, name: &str, device: &str, ctl: Arc<GuestCtl>, track: Option<Arc<TrackLocalStaticSample>>) -> Result<Arc<Session>, String> {
        let crypto = Crypto::new(code).ok_or("that code doesn't look right")?;
        let relay = relay();
        // the relay still works in offline mode: the user turned the jam on
        crate::net::allow_host(&relay_host(&relay));
        let _ = role;
        Ok(Arc::new(Session {
            me: self.me.clone(),
            name: name.chars().take(24).collect(),
            code: code.to_string(),
            crypto,
            relay,
            stop: Arc::new(AtomicBool::new(false)),
            tx: self.tx.clone(),
            api: build_api()?,
            peers: Mutex::new(HashMap::new()),
            host_name: Mutex::new(String::new()),
            track,
            sending: Mutex::new(HashMap::new()),
            rx: Mutex::new(Rx::default()),
            expect: Mutex::new(HashSet::new()),
            connected: AtomicBool::new(false),
            ctl,
            device: device.to_string(),
        }))
    }

    /// Start hosting. Give the returned tap to the audio engine (`Engine::set_tap`) — and take it away again
    /// when the jam ends.
    pub fn host(&mut self, name: &str) -> Result<crate::audio::Tap, String> {
        self.leave("");
        let handle = self.runtime()?;
        let code = new_code();
        let track = Arc::new(TrackLocalStaticSample::new(
            webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability {
                mime_type: webrtc::api::media_engine::MIME_TYPE_OPUS.to_owned(),
                clock_rate: audio::RATE,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".into(),
                rtcp_feedback: vec![],
            },
            "audio".into(),
            "nexus".into(),
        ));
        let s = self.session(Role::Host, &code, name, "", self.ctl.clone(), Some(track.clone()))?;
        let (tap, feed) = audio::tap();
        // the encoder thread
        let stop = s.stop.clone();
        let (s2, h2) = (s.clone(), handle.clone());
        std::thread::Builder::new()
            .name("nexus-jam-enc".into())
            .spawn(move || {
                let idle = || s2.peers.lock().is_empty();
                audio::run_encoder(feed, stop, idle, |p| {
                    let sample = webrtc::media::Sample { data: Bytes::from(p), duration: Duration::from_millis(20), ..Default::default() };
                    let _ = h2.block_on(track.write_sample(&sample));
                });
            })
            .map_err(|e| e.to_string())?;
        self.encoder = Some(s.stop.clone());
        handle.spawn(s.clone().host_main());
        let (q, qr) = tokio::sync::mpsc::unbounded_channel();
        handle.spawn(s.clone().writer(qr));
        self.out = Some(q);
        self.role = Role::Host;
        self.code = code.clone();
        self.sess = Some(s);
        let _ = self.tx.send(Event::State("hosting".into(), code));
        Ok(tap)
    }

    /// Join a friend's jam. `device` is the output device (as in settings), `gain` the starting volume.
    pub fn join(&mut self, code: &str, name: &str, device: &str, gain: f32) -> Result<(), String> {
        let code = norm_code(code).ok_or("that code doesn't look right — it's 6 letters/numbers like K7Q-M2X")?;
        self.leave("");
        let handle = self.runtime()?;
        self.ctl = GuestCtl::new(gain);
        let s = self.session(Role::Guest, &code, name, device, self.ctl.clone(), None)?;
        handle.spawn(s.clone().guest_main());
        let (q, qr) = tokio::sync::mpsc::unbounded_channel();
        handle.spawn(s.clone().writer(qr));
        self.out = Some(q);
        self.role = Role::Guest;
        self.code = code.clone();
        self.sess = Some(s);
        let _ = self.tx.send(Event::State("joining".into(), code));
        Ok(())
    }

    /// End the jam (host) or leave it (guest). `detail` is shown as the reason.
    pub fn leave(&mut self, detail: &str) {
        if self.role == Role::Off {
            return;
        }
        self.role = Role::Off;
        if let Some(s) = self.sess.take() {
            s.stop.store(true, Ordering::Relaxed);
            for c in s.sending.lock().values() {
                c.store(true, Ordering::Relaxed);
            }
            {
                let mut rx = s.rx.lock();
                for job in rx.jobs.values_mut() {
                    job.file = None;
                }
                rx.jobs.clear();
            }
            if let Some(rt) = &self.rt {
                let s2 = s.clone();
                self.closing = Some(rt.spawn(async move {
                    let bye = json!({"k": "bye", "from": s2.me});
                    let (relay, s3) = (s2.relay.clone(), s2.clone());
                    let post = tokio::task::spawn_blocking(move || Session::publish_blocking(&relay, &s3.crypto, &bye));
                    let peers: Vec<Peer> = s2.peers.lock().drain().map(|(_, p)| p).collect();
                    for p in peers {
                        let _ = tokio::time::timeout(Duration::from_secs(3), p.pc.close()).await;
                    }
                    let _ = tokio::time::timeout(Duration::from_secs(8), post).await;
                }));
            }
        }
        self.encoder = None;
        self.out = None;
        self.code.clear();
        let _ = self.tx.send(Event::State("off".into(), detail.to_string()));
    }

    /// Leave and give the goodbye a moment to get out (app exit).
    pub fn shutdown(&mut self) {
        self.leave("");
        if let Some(rt) = self.rt.take() {
            if let Some(h) = self.closing.take() {
                let _ = rt.block_on(async { tokio::time::timeout(Duration::from_secs(4), h).await });
            }
            rt.shutdown_timeout(Duration::from_secs(1));
        }
    }

    fn handle(&mut self, e: Event, out: &mut Vec<Event>) {
        match &e {
            Event::State(st, detail) if (st == "ended" || st == "error") && self.role == Role::Guest => {
                let d = detail.clone();
                if st == "error" {
                    out.push(e);
                }
                self.leave(&d);
            }
            _ => out.push(e),
        }
    }

    /// What happened since last time. A guest whose jam ended or failed is tidied up here (its "off" follows).
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = self.rx.try_recv() {
            self.handle(e, &mut out);
        }
        out
    }

    /// Wait (up to `wait`) for the next event — for tests and headless checks.
    pub fn wait_event(&mut self, wait: Duration) -> Option<Event> {
        let end = Instant::now() + wait;
        loop {
            let e = self.rx.recv_timeout(end.saturating_duration_since(Instant::now())).ok()?;
            let mut out = Vec::new();
            self.handle(e, &mut out);
            if let Some(first) = out.into_iter().next() {
                return Some(first);
            }
        }
    }

    /// JSON to one listener / everyone but `exclude` (host), or to the host (guest).
    pub fn send(&self, v: &Value, peer: Option<&str>, exclude: Option<&str>) {
        let (Some(s), Some(rt)) = (&self.sess, &self.rt) else { return };
        let _ = (s, rt);
        if let Some(q) = &self.out {
            let _ = q.send((v.to_string(), peer.map(str::to_string), exclude.map(str::to_string)));
        }
    }

    /// Host: remove someone.
    pub fn kick(&self, peer: &str) {
        let (Some(s), Some(rt)) = (&self.sess, &self.rt) else { return };
        let ch = s.peers.lock().get(peer).and_then(|p| p.dc.clone());
        let (s, p) = (s.clone(), peer.to_string());
        rt.spawn(async move {
            if let Some(ch) = ch {
                let _ = ch.send_text(json!({"t": "kicked"}).to_string()).await;
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            if let Some(gone) = s.drop_peer(&p).await {
                s.msg(json!({"t": "left", "peer": p, "name": gone.name, "failed": false}));
            }
        });
    }

    /// Host: send files to a listener. `files` = (path, name for the listener like "Artist/Album/x.flac", size).
    pub fn send_files(&self, peer: &str, rid: &str, files: Vec<(PathBuf, String, u64)>) {
        let (Some(s), Some(rt)) = (&self.sess, &self.rt) else { return };
        let cancel = Arc::new(AtomicBool::new(false));
        s.sending.lock().insert(rid.to_string(), cancel.clone());
        rt.spawn(s.clone().send_files(peer.to_string(), rid.to_string(), files, cancel));
    }

    pub fn cancel_send(&self, rid: &str) {
        if let Some(s) = &self.sess {
            if let Some(c) = s.sending.lock().get(rid) {
                c.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Guest: a download we asked for (files for any other request id are ignored).
    pub fn expect_files(&self, rid: &str) {
        if let Some(s) = &self.sess {
            s.expect.lock().insert(rid.to_string());
        }
    }

    /// Guest: where a finished download's files were put.
    pub fn incoming_dir(rid: &str) -> PathBuf {
        Session::incoming_dir(rid)
    }
}

impl Drop for Jam {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        for _ in 0..50 {
            let c = new_code();
            assert_eq!(c.len(), 7);
            assert!(c.chars().all(|x| x == '-' || ALPHABET.contains(x)));
            assert_eq!(norm_code(&c.to_lowercase()).as_deref(), Some(c.as_str()));
        }
        assert_eq!(norm_code(" k7q m2x ").as_deref(), Some("K7Q-M2X"));
        assert_eq!(norm_code("k7q-m2"), None);
    }

    #[test]
    fn crypto_matches_nexus_1() {
        // topic for "K7Q-M2X" as Nexus 1 (Python hashlib.scrypt) computes it
        let c = Crypto::new("k7q m2x").unwrap();
        assert_eq!(c.topic, "nexusjam2268af19da1868c982ae27c56214d");
        // sealed by Nexus 1
        let py = "AAAAAAAAAAAAAAAATnzVRZTPkOuFt0VYKtHkFFgFRj_Lt7qday78kce4XjhOvpC_JIc0guZRgnyBG2n7OSGGCQ1S1Z72t75cj7A=";
        assert_eq!(c.open(py), Some(json!({"k": "hello", "from": "py", "name": "Asher"})));
        let v = json!({"k": "hello", "from": "abc", "name": "Asher"});
        let sealed = c.seal(&v);
        assert_eq!(c.open(&sealed), Some(v));
        let other = Crypto::new("AAA-AAA").unwrap();
        assert_eq!(other.open(&sealed), None);
    }

    #[test]
    fn clipping() {
        let mut m = Map::new();
        m.insert("text".into(), json!("x".repeat(2000)));
        m.insert("queue".into(), json!([["a", "b"], "bad", ["c"], ["d", "e", "f"]]));
        m.insert("lyrics".into(), json!("not an object"));
        let c = clip(m);
        assert_eq!(c["text"].as_str().unwrap().len(), 500);
        assert_eq!(c["queue"], json!([["a", "b"], ["d", "e"]]));
        assert_eq!(c["lyrics"], Value::Null);
    }
}
