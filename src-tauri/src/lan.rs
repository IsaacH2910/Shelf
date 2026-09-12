use chrono::{DateTime, Duration, Utc};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use parking_lot::Mutex;
use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;
use tracing::{info, warn};

use crate::db::Database;

pub const SERVICE_TYPE: &str = "_shelf._tcp.local.";
pub const DEFAULT_PORT: u16 = 7834;
pub const DURATION_UNTIL_OFF: &str = "until_off";
pub const DURATION_15M: &str = "15m";
pub const DURATION_60M: &str = "60m";
const TXT_PATH: &str = "/";
const TXT_PROTO: &str = "http";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanIdentity {
    pub bonjour_host: String,
    pub instance_name: String,
    pub ipv4: Vec<Ipv4Addr>,
    pub port: u16,
    pub url: String,
}

impl LanIdentity {
    pub fn primary_ip(&self) -> Option<Ipv4Addr> {
        self.ipv4.first().copied()
    }

    pub fn matches_host(&self, host: &str) -> bool {
        lan_host_matches(host, self)
    }
}

pub fn enabled(db: &Database) -> bool {
    let flag = db
        .get_setting("lan_enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    let expires = db.get_setting("lan_expires_at").ok().flatten();
    is_session_active(flag, expires.as_deref(), Utc::now())
}

pub fn stored_duration(db: &Database) -> String {
    normalize_duration(
        db.get_setting("lan_duration")
            .ok()
            .flatten()
            .as_deref(),
    )
    .to_string()
}

pub fn stored_expires_at(db: &Database) -> Option<String> {
    db.get_setting("lan_expires_at")
        .ok()
        .flatten()
        .filter(|v| !v.is_empty())
}

pub fn normalize_duration(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or(DURATION_UNTIL_OFF).trim() {
        "15" | "15m" | "15min" | "minutes15" => DURATION_15M,
        "60" | "60m" | "1h" | "hour" | "60min" => DURATION_60M,
        _ => DURATION_UNTIL_OFF,
    }
}

pub fn expires_at_for(duration: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match normalize_duration(Some(duration)) {
        DURATION_15M => Some(now + Duration::minutes(15)),
        DURATION_60M => Some(now + Duration::minutes(60)),
        _ => None,
    }
}

pub fn is_session_active(flag: bool, expires_at: Option<&str>, now: DateTime<Utc>) -> bool {
    if !flag {
        return false;
    }
    let Some(raw) = expires_at.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    DateTime::parse_from_rfc3339(raw)
        .map(|deadline| now <= deadline.with_timezone(&Utc))
        .unwrap_or(false)
}

pub fn reconcile_expired(db: &Database) {
    let flag = db
        .get_setting("lan_enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    if !flag {
        return;
    }
    if enabled(db) {
        return;
    }
    let _ = db.set_setting("lan_enabled", "false");
    let _ = db.set_setting("lan_expires_at", "");
    let _ = db.set_setting("lan_url", "");
}

pub fn discover_identity(port: u16) -> LanIdentity {
    let bonjour_host = local_bonjour_host();
    let ipv4 = lan_ipv4s();
    let url = preferred_local_url(&bonjour_host, ipv4.first().copied(), port);
    let instance_name = instance_name_for(&bonjour_host);
    LanIdentity {
        bonjour_host,
        instance_name,
        ipv4,
        port,
        url,
    }
}

pub fn preferred_local_url(bonjour_host: &str, ip: Option<Ipv4Addr>, port: u16) -> String {
    if is_usable_bonjour_label(bonjour_host) {
        return format!("http://{bonjour_host}.local:{port}");
    }
    if let Some(ip) = ip {
        return format!("http://{ip}:{port}");
    }
    format!("http://127.0.0.1:{port}")
}

pub fn is_loopback_host(host: &str) -> bool {
    matches!(
        host,
        "localhost" | "127.0.0.1" | "::1" | "[::1]"
    )
}

pub fn is_lan_host(host: &str, identity: Option<&LanIdentity>) -> bool {
    let host = normalize_host(host);
    if host.is_empty() {
        return false;
    }
    if host.ends_with(".local") {
        return true;
    }
    if parse_private_ipv4(&host).is_some() {
        return true;
    }
    identity.is_some_and(|id| id.matches_host(&host))
}

pub fn lan_host_matches(host: &str, identity: &LanIdentity) -> bool {
    let host = normalize_host(host);
    if host.is_empty() {
        return false;
    }
    if is_loopback_host(&host) {
        return true;
    }
    let bonjour = identity.bonjour_host.to_ascii_lowercase();
    if !bonjour.is_empty() {
        if host == bonjour || host == format!("{bonjour}.local") {
            return true;
        }
    }
    identity.ipv4.iter().any(|ip| host == ip.to_string())
}

pub fn host_allowed(host: &str, configured: Option<&str>, lan: Option<&LanIdentity>) -> bool {
    let host = normalize_host(host);
    if host.is_empty() {
        return false;
    }
    if is_loopback_host(&host) {
        return true;
    }
    if configured.is_some_and(|expected| crate::tunnel::host_matches_configured(&host, expected)) {
        return true;
    }
    if let Some(identity) = lan {
        return identity.matches_host(&host);
    }
    false
}

fn normalize_host(host: &str) -> String {
    host.split(':')
        .next()
        .unwrap_or(host)
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

pub fn parse_private_ipv4(host: &str) -> Option<Ipv4Addr> {
    let ip: Ipv4Addr = host.parse().ok()?;
    if is_private_ipv4(ip) {
        Some(ip)
    } else {
        None
    }
}

pub fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    matches!(o[0], 10)
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
}

fn is_usable_bonjour_label(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.len() > 63 {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

fn instance_name_for(bonjour_host: &str) -> String {
    let host = bonjour_host.trim();
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return "Shelf".into();
    }
    let name = format!("Shelf ({host})");
    if name.len() > 63 {
        "Shelf".into()
    } else {
        name
    }
}

fn local_bonjour_host() -> String {
    if let Some(name) = command_stdout("scutil", &["--get", "LocalHostName"]) {
        let cleaned = sanitize_bonjour_label(&name);
        if is_usable_bonjour_label(&cleaned) {
            return cleaned;
        }
    }
    if let Some(name) = command_stdout("hostname", &["-s"]) {
        let cleaned = sanitize_bonjour_label(&name);
        if is_usable_bonjour_label(&cleaned) {
            return cleaned;
        }
    }
    hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .map(|s| sanitize_bonjour_label(s.split('.').next().unwrap_or(&s)))
        .filter(|s| is_usable_bonjour_label(s))
        .unwrap_or_default()
}

fn sanitize_bonjour_label(raw: &str) -> String {
    raw.trim()
        .trim_end_matches(".local")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else if c.is_whitespace() || c == '_' {
                '-'
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn command_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(bin).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|iface| !iface.is_loopback())
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) if is_private_ipv4(ip) => Some(ip),
            _ => None,
        })
        .collect();
    ips.sort();
    ips.dedup();
    if ips.is_empty() {
        if let Some(ip) = udp_hint_ipv4() {
            if is_private_ipv4(ip) {
                ips.push(ip);
            }
        }
    }
    ips
}

fn udp_hint_ipv4() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

struct AdvertiserInner {
    daemon: ServiceDaemon,
    fullname: String,
}

pub struct LanService {
    inner: Mutex<Option<AdvertiserInner>>,
    identity: Mutex<LanIdentity>,
}

impl LanService {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            identity: Mutex::new(discover_identity(DEFAULT_PORT)),
        }
    }

    pub fn identity(&self) -> LanIdentity {
        self.identity.lock().clone()
    }

    pub fn advertising(&self) -> bool {
        self.inner.lock().is_some()
    }

    pub fn refresh(&self, port: u16) -> LanIdentity {
        let identity = discover_identity(port);
        *self.identity.lock() = identity.clone();
        identity
    }

    pub fn start(&self, port: u16) -> Result<LanIdentity, String> {
        self.stop();
        let identity = self.refresh(port);
        let Some(ip) = identity.primary_ip() else {
            return Err("Could not find a LAN address to advertise".into());
        };
        if !is_usable_bonjour_label(&identity.bonjour_host) {
            return Err("Could not determine a Bonjour hostname".into());
        }
        let daemon = ServiceDaemon::new().map_err(|e| format!("Could not start Bonjour: {e}"))?;
        let host_name = format!("{}.local.", identity.bonjour_host);
        let properties = [
            ("path", TXT_PATH),
            ("version", env!("CARGO_PKG_VERSION")),
            ("proto", TXT_PROTO),
            ("https", "0"),
            ("url", identity.url.as_str()),
        ];
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &identity.instance_name,
            &host_name,
            IpAddr::V4(ip),
            identity.port,
            &properties[..],
        )
        .map_err(|e| format!("Could not build Bonjour service: {e}"))?;
        let fullname = service.get_fullname().to_string();
        daemon
            .register(service)
            .map_err(|e| format!("Could not advertise on the local network: {e}"))?;
        info!(
            url = %identity.url,
            service = %fullname,
            "Advertising Shelf over Bonjour"
        );
        *self.inner.lock() = Some(AdvertiserInner { daemon, fullname });
        Ok(identity)
    }

    pub fn stop(&self) {
        let Some(inner) = self.inner.lock().take() else {
            return;
        };
        if let Err(error) = inner.daemon.unregister(&inner.fullname) {
            warn!("Could not unregister Bonjour service: {error}");
        }
        if let Err(error) = inner.daemon.shutdown() {
            warn!("Could not stop Bonjour daemon: {error}");
        }
    }
}

impl Drop for LanService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(host: &str, ip: &str) -> LanIdentity {
        let ipv4 = ip.parse().unwrap();
        LanIdentity {
            bonjour_host: host.into(),
            instance_name: instance_name_for(host),
            ipv4: vec![ipv4],
            port: 7834,
            url: preferred_local_url(host, Some(ipv4), 7834),
        }
    }

    #[test]
    fn prefers_dot_local_url() {
        assert_eq!(
            preferred_local_url("Isaacs-MacBook", Some(Ipv4Addr::new(192, 168, 1, 20)), 7834),
            "http://Isaacs-MacBook.local:7834"
        );
        assert_eq!(
            preferred_local_url("", Some(Ipv4Addr::new(10, 0, 0, 5)), 7834),
            "http://10.0.0.5:7834"
        );
    }

    #[test]
    fn allow_list_accepts_loopback_and_cloud() {
        assert!(host_allowed("127.0.0.1:7834", None, None));
        assert!(host_allowed("localhost", None, None));
        assert!(host_allowed(
            "shelf.example.com",
            Some("shelf.example.com"),
            None
        ));
        assert!(host_allowed(
            "www.shelf.example.com",
            Some("shelf.example.com"),
            None
        ));
        assert!(!host_allowed("evil.example", Some("shelf.example.com"), None));
        assert!(!host_allowed("192.168.1.20", Some("shelf.example.com"), None));
    }

    #[test]
    fn allow_list_accepts_lan_when_enabled() {
        let lan = identity("Isaacs-MacBook", "192.168.1.20");
        assert!(host_allowed("isaacs-macbook.local", None, Some(&lan)));
        assert!(host_allowed("Isaacs-MacBook.local:7834", None, Some(&lan)));
        assert!(host_allowed("192.168.1.20", None, Some(&lan)));
        assert!(host_allowed("isaacs-macbook", None, Some(&lan)));
        assert!(!host_allowed("10.0.0.9", None, Some(&lan)));
        assert!(!host_allowed("other.local", None, Some(&lan)));
    }

    #[test]
    fn lan_disabled_rejects_lan_hosts() {
        assert!(!host_allowed("macbook.local", None, None));
        assert!(!host_allowed("192.168.1.20", None, None));
    }

    #[test]
    fn private_ipv4_detection() {
        assert!(is_private_ipv4("10.1.2.3".parse().unwrap()));
        assert!(is_private_ipv4("192.168.0.1".parse().unwrap()));
        assert!(is_private_ipv4("172.16.0.1".parse().unwrap()));
        assert!(!is_private_ipv4("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ipv4("1.1.1.1".parse().unwrap()));
        assert!(parse_private_ipv4("192.168.1.8").is_some());
        assert!(parse_private_ipv4("8.8.8.8").is_none());
    }

    #[test]
    fn instance_name_stays_short() {
        assert_eq!(instance_name_for("Studio"), "Shelf (Studio)");
        assert_eq!(instance_name_for(""), "Shelf");
    }

    #[test]
    fn nearby_session_honors_duration_and_expiry() {
        assert_eq!(normalize_duration(Some("15m")), DURATION_15M);
        assert_eq!(normalize_duration(Some("1h")), DURATION_60M);
        assert_eq!(normalize_duration(Some("until_off")), DURATION_UNTIL_OFF);
        let now = DateTime::parse_from_rfc3339("2026-09-12T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(expires_at_for(DURATION_UNTIL_OFF, now).is_none());
        assert_eq!(
            expires_at_for(DURATION_15M, now).map(|t| t.to_rfc3339()),
            Some("2026-09-12T10:15:00+00:00".into())
        );
        assert!(is_session_active(true, None, now));
        assert!(is_session_active(
            true,
            Some("2026-09-12T10:15:00+00:00"),
            now
        ));
        assert!(!is_session_active(
            true,
            Some("2026-09-12T09:59:00+00:00"),
            now
        ));
        assert!(!is_session_active(false, None, now));
    }
}
