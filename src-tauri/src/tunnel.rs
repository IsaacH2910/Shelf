use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

const KEYRING_SERVICE: &str = "com.isaach.shelf";
const KEYRING_ACCOUNT: &str = "cloudflare-tunnel-token";

#[derive(Clone)]
enum TunnelCmd {
    Named { token: String },
}

pub struct TunnelManager {
    child: Mutex<Option<Child>>,
    status: Arc<Mutex<String>>,
    last_log: Arc<Mutex<String>>,
    command: Mutex<Option<TunnelCmd>>,
    stop_flag: AtomicBool,
    restart_count: AtomicU32,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            status: Arc::new(Mutex::new("stopped".into())),
            last_log: Arc::new(Mutex::new(String::new())),
            command: Mutex::new(None),
            stop_flag: AtomicBool::new(true),
            restart_count: AtomicU32::new(0),
        }
    }

    pub fn running(&self) -> bool {
        let mut child = self.child.lock();
        if let Some(c) = child.as_mut() {
            match c.try_wait() {
                Ok(Some(_)) => {
                    *child = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    pub fn status(&self) -> String {
        self.status.lock().clone()
    }

    pub fn restart_count(&self) -> u32 {
        self.restart_count.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        *self.command.lock() = None;
        kill_child(&self.child);
        *self.status.lock() = "stopped".into();
    }

    pub fn mark_error(&self, error: &str) {
        *self.status.lock() = error.to_string();
    }

    pub fn start_named(&self, token: &str) -> Result<(), String> {
        let token = token.trim();
        if token.is_empty() {
            return Err("Cloud tunnel token is not configured".into());
        }
        self.launch(TunnelCmd::Named {
            token: token.to_string(),
        })
    }

    fn launch(&self, cmd: TunnelCmd) -> Result<(), String> {
        self.stop_flag.store(false, Ordering::SeqCst);
        self.restart_count.store(0, Ordering::Relaxed);
        *self.command.lock() = Some(cmd.clone());
        self.spawn_once(&cmd)?;
        Ok(())
    }

    fn spawn_once(&self, cmd: &TunnelCmd) -> Result<(), String> {
        kill_child(&self.child);
        *self.last_log.lock() = String::new();
        let TunnelCmd::Named { token } = cmd;
        let bin = cloudflared_bin().ok_or_else(|| {
            "Install cloudflared: brew install cloudflare/cloudflare/cloudflared".to_string()
        })?;
        let mut command = Command::new(&bin);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args(["tunnel", "--no-autoupdate", "run", "--token", token]);
        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to start the Cloudflare tunnel: {e}"))?;
        let status_slot = Arc::clone(&self.status);
        let log_slot = Arc::clone(&self.last_log);
        if let Some(stdout) = child.stdout.take() {
            spawn_log_reader(stdout, Arc::clone(&status_slot), Arc::clone(&log_slot));
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_log_reader(stderr, status_slot, log_slot);
        }
        *self.child.lock() = Some(child);
        *self.status.lock() = "connecting".into();
        info!("Cloudflare named tunnel started");
        Ok(())
    }

    /// If the process died while remote access is still enabled, restart it with backoff.
    pub fn supervise_tick(&self) {
        if self.stop_flag.load(Ordering::SeqCst) {
            return;
        }
        if self.running() {
            return;
        }
        let Some(cmd) = self.command.lock().clone() else {
            return;
        };
        let detail = self.last_log.lock().clone();
        let n = self.restart_count.fetch_add(1, Ordering::Relaxed) + 1;
        let delay = Duration::from_secs(1u64.saturating_mul(2u64.saturating_pow(n.min(4))).min(15));
        *self.status.lock() = if detail.is_empty() {
            format!("restarting in {}s", delay.as_secs())
        } else {
            format!("{detail} · retry in {}s", delay.as_secs())
        };
        warn!("public tunnel exited; restart #{n} in {:?}", delay);
        thread::sleep(delay);
        if self.stop_flag.load(Ordering::SeqCst) {
            return;
        }
        if let Err(error) = self.spawn_once(&cmd) {
            *self.status.lock() = error;
        }
    }
}

fn spawn_log_reader(
    stream: impl Read + Send + 'static,
    status_slot: Arc<Mutex<String>>,
    log_slot: Arc<Mutex<String>>,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines().flatten() {
            let line = strip_ansi(&line);
            if looks_like_tunnel_ready(&line) {
                *status_slot.lock() = "running".into();
            } else if looks_like_tunnel_error(&line) {
                *log_slot.lock() = summarize_tunnel_error(&line);
            }
            info!("tunnel: {line}");
        }
    });
}

fn looks_like_tunnel_ready(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("registered tunnel connection") || lower.contains("connindex")
}

fn looks_like_tunnel_error(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("denied")
        || lower.contains("failed")
        || lower.contains("unauthorized")
        || lower.contains("could not")
        || lower.contains("refused")
}

fn summarize_tunnel_error(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() > 120 {
        format!("{}…", trimmed.chars().take(117).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn kill_child(child: &Mutex<Option<Child>>) {
    if let Some(mut child) = child.lock().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn cloudflared_available() -> bool {
    cloudflared_bin().is_some()
}

pub fn cloudflared_bin() -> Option<String> {
    if let Ok(path) = which("cloudflared") {
        return Some(path);
    }
    for candidate in ["/opt/homebrew/bin/cloudflared", "/usr/local/bin/cloudflared"] {
        if std::path::Path::new(candidate).is_file() {
            return Some(candidate.into());
        }
    }
    None
}

fn which(name: &str) -> Result<String, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }
    Err(())
}

pub fn normalize_hostname(raw: &str) -> Result<String, String> {
    let host = raw
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || host.contains(' ') || !host.contains('.') {
        return Err("Enter a hostname like shelf.example.com".into());
    }
    Ok(host)
}

pub fn public_url_for_hostname(host: &str) -> Option<String> {
    let host = host.trim();
    if host.is_empty() {
        None
    } else {
        Some(format!("https://{host}"))
    }
}

pub fn host_matches_configured(request_host: &str, configured: &str) -> bool {
    let host = request_host
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let configured = configured
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || configured.is_empty() {
        return false;
    }
    host == configured || host == format!("www.{configured}")
}

fn keychain_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("Could not open the keychain: {e}"))
}

/// Hostname from local settings, then `SHELF_TUNNEL_HOSTNAME`. No compiled fallback.
pub fn resolve_hostname(stored: Option<&str>) -> Option<String> {
    if let Some(host) = stored.and_then(|value| normalize_hostname(value).ok()) {
        return Some(host);
    }
    std::env::var("SHELF_TUNNEL_HOSTNAME")
        .ok()
        .and_then(|value| normalize_hostname(&value).ok())
}

pub fn load_token() -> Result<Option<String>, String> {
    if let Ok(token) = std::env::var("SHELF_TUNNEL_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }
    match keychain_entry()?.get_password() {
        Ok(token) => {
            let token = token.trim().to_string();
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Could not read the tunnel token: {e}")),
    }
}

pub fn save_token(token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Cloud tunnel token is empty".into());
    }
    keychain_entry()?
        .set_password(token)
        .map_err(|e| format!("Could not save the tunnel token: {e}"))
}

pub fn has_token() -> bool {
    load_token().ok().flatten().is_some()
}

pub fn start_supervisor(manager: Arc<TunnelManager>) {
    thread::Builder::new()
        .name("shelf-tunnel".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(3));
            manager.supervise_tick();
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::{
        host_matches_configured, looks_like_tunnel_ready, normalize_hostname, resolve_hostname,
        strip_ansi,
    };

    #[test]
    fn normalizes_hostname() {
        assert_eq!(
            normalize_hostname("https://Shelf.example.com/").as_deref(),
            Ok("shelf.example.com")
        );
        assert!(normalize_hostname("localhost").is_err());
        assert!(normalize_hostname("").is_err());
    }

    #[test]
    fn matches_configured_and_www() {
        assert!(host_matches_configured("shelf.example.com", "shelf.example.com"));
        assert!(host_matches_configured(
            "www.shelf.example.com",
            "shelf.example.com"
        ));
        assert!(!host_matches_configured(
            "trycloudflare.com",
            "shelf.example.com"
        ));
        assert!(!host_matches_configured(
            "abcd.run.pinggy-free.link",
            "shelf.example.com"
        ));
    }

    #[test]
    fn detects_ready_log() {
        assert!(looks_like_tunnel_ready(
            "INF Registered tunnel connection connIndex=0"
        ));
        assert!(!looks_like_tunnel_ready("starting"));
    }

    #[test]
    fn strips_ansi() {
        let line = "\u{1b}[32mRegistered tunnel connection\u{1b}[0m";
        assert!(looks_like_tunnel_ready(&strip_ansi(line)));
    }

    #[test]
    fn resolve_hostname_uses_stored_value() {
        assert_eq!(
            resolve_hostname(Some("https://shelf.example.com")).as_deref(),
            Some("shelf.example.com")
        );
        let previous = std::env::var("SHELF_TUNNEL_HOSTNAME").ok();
        std::env::remove_var("SHELF_TUNNEL_HOSTNAME");
        assert_eq!(resolve_hostname(Some("")).as_deref(), None);
        assert_eq!(resolve_hostname(None).as_deref(), None);
        match previous {
            Some(value) => std::env::set_var("SHELF_TUNNEL_HOSTNAME", value),
            None => {}
        }
    }
}
