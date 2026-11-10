//! Why don't downloads start? Port forwarding (UPnP), reachability, VPNs and the Windows Firewall.

use parking_lot::Mutex;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Asks the router (UPnP) to forward the Soulseek ports here, and keeps the mapping fresh.
pub struct Upnp {
    status: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
}

impl Upnp {
    pub fn start(port: u16) -> Self {
        let status = Arc::new(Mutex::new("asking the router…".to_string()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (status.clone(), stop.clone());
        std::thread::Builder::new()
            .name("nexus-upnp".into())
            .spawn(move || {
                let mut opts = igd_next::SearchOptions::default();
                opts.timeout = Some(Duration::from_secs(6));
                let gw = match igd_next::search_gateway(opts) {
                    Ok(g) => g,
                    Err(e) => {
                        log::info!("UPnP: no router answered ({e})");
                        *st.lock() = "none".into();
                        return;
                    }
                };
                let Some(ip) = local_ip_addr() else {
                    *st.lock() = "failed".into();
                    return;
                };
                loop {
                    let mut ok = true;
                    for p in [port, port.saturating_add(1)] {
                        if let Err(e) = gw.add_port(igd_next::PortMappingProtocol::TCP, p, SocketAddr::new(ip, p), 3600, "Nexus Soulseek") {
                            log::info!("UPnP: failed to map port {p}: {e}");
                            ok = false;
                        }
                    }
                    *st.lock() = if ok { "mapped".into() } else { "failed".into() };
                    if ok {
                        log::info!("UPnP: mapped port {port} to {ip}");
                    }
                    for _ in 0..(50 * 60) {
                        if sp.load(Ordering::Relaxed) {
                            for p in [port, port.saturating_add(1)] {
                                let _ = gw.remove_port(igd_next::PortMappingProtocol::TCP, p);
                            }
                            return;
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .ok();
        Upnp { status, stop }
    }

    /// "mapped" / "failed" / "none" (no UPnP router) / still asking.
    pub fn status(&self) -> String {
        self.status.lock().clone()
    }
}

impl Drop for Upnp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn local_ip_addr() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("10.255.255.255:1").ok()?;
    Some(s.local_addr().ok()?.ip())
}

pub fn local_ip() -> String {
    local_ip_addr().map(|i| i.to_string()).unwrap_or_else(|| "this computer".into())
}

/// Asks the Soulseek website whether other users can reach this computer on `port`.
pub fn port_check(port: u16) -> Option<&'static str> {
    let b = crate::net::get_bytes(&format!("http://www.slsknet.org/porttest.php?port={port}"), 12, 20_000).ok()?;
    let t = String::from_utf8_lossy(&b).to_lowercase();
    let i = t.find(&format!("{port}/tcp"))?;
    let rest = &t[i..t.len().min(i + 40)];
    if rest.contains("open") {
        Some("open")
    } else if rest.contains("closed") {
        Some("closed")
    } else {
        None
    }
}

const VPN_WORDS: [(&str, &str); 21] = [
    ("nordlynx", "NordVPN"),
    ("nordvpn", "NordVPN"),
    ("proton", "Proton VPN"),
    ("mullvad", "Mullvad"),
    ("surfshark", "Surfshark"),
    ("expressvpn", "ExpressVPN"),
    ("windscribe", "Windscribe"),
    ("cyberghost", "CyberGhost"),
    ("private internet access", "PIA"),
    ("pia", "PIA"),
    ("tunnelbear", "TunnelBear"),
    ("hotspot shield", "Hotspot Shield"),
    ("ipvanish", "IPVanish"),
    ("cloudflare warp", "Cloudflare WARP"),
    ("warp", "Cloudflare WARP"),
    ("airvpn", "AirVPN"),
    ("wireguard", "a WireGuard VPN"),
    ("wintun", "a VPN"),
    ("tap-windows", "an OpenVPN VPN"),
    ("openvpn", "an OpenVPN VPN"),
    ("tap-", "a VPN"),
];

/// Name of a VPN this computer is connected through right now, or "".
pub fn vpn_active() -> String {
    let names: Vec<String> = if cfg!(windows) {
        hidden_command("powershell", &["-NoProfile", "-Command", "Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object { $_.Name + '|' + $_.InterfaceDescription }"])
            .map(|o| o.lines().filter(|l| !l.trim().is_empty()).map(str::to_lowercase).collect())
            .unwrap_or_default()
    } else {
        let re = regex::Regex::new(r"^(wg|tun|proton|nordlynx|mullvad|pvpn|ipv6leakintrf|cloudflare)").expect("regex");
        std::fs::read_dir("/sys/class/net")
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| {
                        let st = std::fs::read_to_string(format!("/sys/class/net/{n}/operstate")).unwrap_or_default();
                        matches!(st.trim(), "up" | "unknown") && re.is_match(n)
                    })
                    .map(|n| n.to_lowercase())
                    .collect()
            })
            .unwrap_or_default()
    };
    for n in &names {
        for (w, nice) in VPN_WORDS {
            if n.contains(w) {
                return nice.to_string();
            }
        }
    }
    if !cfg!(windows) && !names.is_empty() {
        return "a VPN".into();
    }
    String::new()
}

pub fn vpn_advice(name: &str) -> String {
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "nexus".into());
    format!(
        "you're connected through {name}. VPNs stop other Soulseek users from reaching you, so a lot of downloads never start. \
         fix: turn the VPN off while downloading, or exclude Nexus from it (split tunneling → add {exe}), or use a VPN server \
         with port forwarding and put that port in settings → downloads. Internet Archive and Jamendo downloads work fine through a VPN"
    )
}

fn hidden_command(prog: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(prog);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    let out = c.output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub const FIREWALL_RULE: &str = "Nexus-Soulseek";

/// Windows: is our inbound allow rule there? (None = can't tell / not Windows)
pub fn firewall_ok() -> Option<bool> {
    if !cfg!(windows) {
        return None;
    }
    let out = hidden_command("netsh", &["advfirewall", "firewall", "show", "rule", &format!("name={FIREWALL_RULE}")])?;
    Some(out.contains(FIREWALL_RULE))
}

/// Windows: one admin prompt that (re)creates the inbound rule for Nexus on just the Soulseek ports.
pub fn firewall_fix(port: u16) -> Result<bool, String> {
    if !cfg!(windows) {
        return Err("only needed on Windows".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let cmds = format!(
        "netsh advfirewall firewall delete rule name={FIREWALL_RULE} & netsh advfirewall firewall add rule name={FIREWALL_RULE} dir=in action=allow program=\"{}\" protocol=TCP localport={port},{} enable=yes profile=any",
        exe.display(),
        port + 1
    );
    let ps = format!("Start-Process cmd.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '/c {}'", cmds.replace('\'', "''"));
    hidden_command("powershell", &["-NoProfile", "-Command", &ps]).ok_or("couldn't ask for admin rights")?;
    Ok(firewall_ok() == Some(true))
}

/// Why might a search come back empty? → one human sentence.
pub fn diagnose(query: &str, port: u16, excluded: &[String], upnp: Option<String>) -> String {
    let q = query.to_lowercase();
    if let Some(hit) = excluded.iter().find(|p| !p.is_empty() && q.contains(&p.to_lowercase())) {
        return format!("the Soulseek server lists “{hit}” as an excluded search phrase, so most clients won't answer searches containing it — try the album or song name without it");
    }
    let reach = port_check(port);
    let fw = firewall_ok();
    let vpn = vpn_active();
    if !vpn.is_empty() && reach != Some("open") {
        return vpn_advice(&vpn);
    }
    if reach == Some("closed") {
        if fw == Some(false) {
            return format!("port {port} is closed and the Windows Firewall rule is missing — use “fix firewall” on this page");
        }
        let fwd = format!("forward TCP ports {port}-{} to {} in your router", port + 1, local_ip());
        let router = match upnp.as_deref() {
            Some("none") => format!("your router isn't answering UPnP — turn UPnP on in its settings, or {fwd}"),
            Some("failed") => format!("your router refused to open the port automatically — {fwd}"),
            Some("mapped") => "your router says the port is open but it's still closed from outside — usually your internet provider shares one public IP between customers (CGNAT) or there are two routers".into(),
            _ => format!("turn on UPnP in your router, or {fwd}"),
        };
        return format!("the firewall on this computer is fine — your router is the block (port {port} closed from the internet): {router}. downloads from people who can be reached still work; this mostly costs you search results");
    }
    if fw == Some(false) {
        return "Windows Firewall isn't set up for Nexus yet — use “fix firewall” on this page, then search again".into();
    }
    format!("port {port} is open, so it's the search: nobody shares files whose names contain every word of “{query}”. try fewer words (artist + album), or untick lossless only")
}
