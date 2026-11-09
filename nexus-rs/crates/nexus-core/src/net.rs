//! Every web request Nexus makes goes through here: http(s) only, a size cap, the user's proxy
//! (HTTPS_PROXY etc.) and certificate store, and offline mode, which blocks all hosts except the
//! ones a feature the user turned on needs (the jam relay).

use parking_lot::RwLock;
use std::collections::HashSet;
use std::io::Read;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use ureq::Agent;
use ureq::tls::{PemItem, RootCerts, TlsConfig};

static OFFLINE: AtomicBool = AtomicBool::new(false);
static ALLOW: RwLock<Option<HashSet<String>>> = RwLock::new(None);

pub const JSON_LIMIT: u64 = 16 << 20;

pub fn user_agent() -> String {
    format!("Nexus/{} (+https://github.com/GameBitBan/Nexus)", crate::VERSION)
}

pub fn set_offline(on: bool) {
    OFFLINE.store(on, Ordering::Relaxed);
}

pub fn offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

/// A host that may still be reached in offline mode.
pub fn allow_host(host: &str) {
    ALLOW.write().get_or_insert_with(HashSet::new).insert(host.to_lowercase());
}

fn roots() -> RootCerts {
    // an explicit bundle wins (corporate proxies, NixOS sandboxes); otherwise the OS store
    for var in ["SSL_CERT_FILE", "NIX_SSL_CERT_FILE"] {
        if let Some(path) = std::env::var_os(var) {
            if let Ok(pem) = std::fs::read(&path) {
                let certs: Vec<_> = ureq::tls::parse_pem(&pem)
                    .filter_map(|i| match i {
                        Ok(PemItem::Certificate(c)) => Some(c),
                        _ => None,
                    })
                    .collect();
                if !certs.is_empty() {
                    return RootCerts::new_with_certs(&certs);
                }
            }
        }
    }
    RootCerts::PlatformVerifier
}

pub fn agent() -> &'static Agent {
    static A: OnceLock<Agent> = OnceLock::new();
    A.get_or_init(|| {
        Agent::config_builder()
            .user_agent(user_agent())
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .max_redirects(8)
            .tls_config(TlsConfig::builder().root_certs(roots()).build())
            .build()
            .into()
    })
}

fn host_of(url: &str) -> Result<String, String> {
    let low = url.trim_start().to_ascii_lowercase();
    if !low.starts_with("http://") && !low.starts_with("https://") {
        return Err(format!("blocked url scheme: {}", low.split(':').next().unwrap_or("")));
    }
    let uri: ureq::http::Uri = url.parse().map_err(|_| format!("bad url: {url}"))?;
    match uri.scheme_str() {
        Some("http") | Some("https") => {}
        other => return Err(format!("blocked url scheme: {}", other.unwrap_or("none"))),
    }
    Ok(uri.host().unwrap_or("").trim_matches(['[', ']']).to_lowercase())
}

/// Errors if the url isn't http(s) or offline mode blocks it.
pub fn check(url: &str) -> Result<(), String> {
    let host = host_of(url)?;
    if offline() && !matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") && !ALLOW.read().as_ref().is_some_and(|a| a.contains(&host)) {
        return Err("offline mode is on (settings → advanced)".into());
    }
    Ok(())
}

fn status_err(code: u16) -> String {
    match code {
        404 => "not found (404)".into(),
        403 => "refused (403)".into(),
        429 => "too many requests — try again in a minute (429)".into(),
        c if c >= 500 => format!("the server had a problem ({c})"),
        c => format!("http {c}"),
    }
}

fn tidy(e: ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "timed out".into(),
        ureq::Error::HostNotFound => "couldn't find the server — are you online?".into(),
        ureq::Error::Io(e) => e.to_string(),
        ureq::Error::BodyExceedsLimit(n) => format!("response too big (over {} MB)", n >> 20),
        other => other.to_string(),
    }
}

/// GET with extra headers; returns the body (capped at `limit` bytes).
pub fn get_with(url: &str, headers: &[(&str, &str)], timeout: u64, limit: u64) -> Result<Vec<u8>, String> {
    check(url)?;
    let mut req = agent().get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.config().timeout_global(Some(Duration::from_secs(timeout))).build().call().map_err(tidy)?;
    let code = resp.status().as_u16();
    if !(200..300).contains(&code) {
        return Err(status_err(code));
    }
    resp.body_mut().with_config().limit(limit).read_to_vec().map_err(tidy)
}

pub fn get_bytes(url: &str, timeout: u64, limit: u64) -> Result<Vec<u8>, String> {
    get_with(url, &[], timeout, limit)
}

pub fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let b = get_with(url, &[("Accept", "application/json")], 12, JSON_LIMIT)?;
    serde_json::from_slice(&b).map_err(|e| format!("bad reply: {e}"))
}

pub fn get_json_with(url: &str, headers: &[(&str, &str)], timeout: u64) -> Result<serde_json::Value, String> {
    let b = get_with(url, headers, timeout, JSON_LIMIT)?;
    serde_json::from_slice(&b).map_err(|e| format!("bad reply: {e}"))
}

/// POST a body; returns (status, body).
pub fn post(url: &str, headers: &[(&str, &str)], body: &[u8], timeout: u64) -> Result<(u16, Vec<u8>), String> {
    check(url)?;
    let mut req = agent().post(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.config().timeout_global(Some(Duration::from_secs(timeout))).build().send(body).map_err(tidy)?;
    let code = resp.status().as_u16();
    let b = resp.body_mut().with_config().limit(JSON_LIMIT).read_to_vec().map_err(tidy)?;
    Ok((code, b))
}

/// Streams a download to `out`, calling `progress(done, total)`; `cancel` returning true stops it.
pub fn download(url: &str, out: &mut impl std::io::Write, timeout: u64, mut progress: impl FnMut(u64, u64) -> bool) -> Result<u64, String> {
    check(url)?;
    let resp = agent().get(url).config().timeout_global(Some(Duration::from_secs(timeout))).build().call().map_err(tidy)?;
    let code = resp.status().as_u16();
    if !(200..300).contains(&code) {
        return Err(status_err(code));
    }
    let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut rd = resp.into_body().into_with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = rd.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if !progress(done, total) {
            return Err("cancelled".into());
        }
    }
    Ok(done)
}

/// `a=1&b=two%20words`
pub fn query(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

pub fn enc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_and_offline() {
        assert!(check("file:///etc/passwd").is_err());
        assert!(check("ftp://x/y").is_err());
        assert!(check("https://api.deezer.com/chart").is_ok());
        set_offline(true);
        assert!(check("https://api.deezer.com/chart").is_err());
        assert!(check("http://127.0.0.1:9/x").is_ok());
        allow_host("relay.example");
        assert!(check("https://relay.example/x").is_ok());
        set_offline(false);
        assert_eq!(query(&[("q", "a b&c"), ("n", "é")]), "q=a%20b%26c&n=%C3%A9");
    }
}
