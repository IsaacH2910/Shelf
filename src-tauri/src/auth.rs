use crate::db::Database;
use crate::models::{AuthSession, LoginRequest, SessionInfo, UserRole};
use argon2::{
    password_hash::{
        rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};
use chrono::{Duration, Utc};
use parking_lot::Mutex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "shelf_session";
const IDLE_DAYS: i64 = 14;
const ABSOLUTE_DAYS: i64 = 90;
const ROTATE_AFTER_DAYS: i64 = 7;
const LOCKOUT_AFTER: i64 = 8;
const LOCKOUT_MINUTES: i64 = 15;
const MIN_PASSWORD_LEN: usize = 8;

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hash_password(password: &str) -> Result<String, String> {
    validate_password(password)?;
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn validate_password(password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters"
        ));
    }
    Ok(())
}

pub fn validate_username(username: &str) -> Result<String, String> {
    let trimmed = username.trim().to_ascii_lowercase();
    if trimmed.len() < 2 || trimmed.len() > 32 {
        return Err("Username must be 2–32 characters".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err("Username may only contain lowercase letters, digits, and underscores".into());
    }
    Ok(trimmed)
}

pub struct LoginLimiter {
    attempts: Mutex<HashMap<String, (u32, i64)>>,
    global: Mutex<(u32, i64)>,
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            global: Mutex::new((0, Utc::now().timestamp())),
        }
    }

    pub fn allow(&self, key: &str) -> bool {
        let now = Utc::now().timestamp();
        {
            let mut global = self.global.lock();
            if now - global.1 > 300 {
                *global = (0, now);
            }
            global.0 += 1;
            if global.0 > 60 {
                return false;
            }
        }
        let mut map = self.attempts.lock();
        let entry = map.entry(key.to_string()).or_insert((0, now));
        if now - entry.1 > 300 {
            *entry = (0, now);
        }
        entry.0 += 1;
        entry.0 <= 8
    }
}

#[derive(Debug)]
pub struct IssuedSession {
    pub info: SessionInfo,
    pub token: String,
}

pub fn login(
    db: &Arc<Database>,
    req: &LoginRequest,
    limiter: &LoginLimiter,
    client_key: &str,
) -> Result<IssuedSession, String> {
    if !limiter.allow(client_key) {
        return Err("Too many sign-in attempts. Try again later.".into());
    }
    if !db.owner_password_set().map_err(|e| e.to_string())? {
        return Err("Remote sign-in is not configured yet. Set an owner password on the Mac.".into());
    }
    let username = validate_username(&req.username).unwrap_or_else(|_| req.username.trim().to_string());
    let Some(row) = db
        .get_user_by_username(&username)
        .map_err(|e| e.to_string())?
    else {
        return Err("Invalid username or password".into());
    };
    if row.disabled {
        return Err("This account is disabled".into());
    }
    if let Some(until) = &row.locked_until {
        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(until) {
            if ts > Utc::now() {
                return Err("This account is locked. Try again later.".into());
            }
        }
    }
    if row.password_hash.is_empty() || !verify_password(&req.password, &row.password_hash) {
        let _ = db.record_login_failure(row.id, LOCKOUT_AFTER, LOCKOUT_MINUTES);
        return Err("Invalid username or password".into());
    }
    let _ = db.clear_login_failures(row.id);
    issue_session(
        db,
        row.id,
        &row.username,
        &row.display_name,
        row.role,
        row.access_all,
        req.device_name.as_deref(),
    )
}

fn issue_session(
    db: &Database,
    user_id: i64,
    username: &str,
    display_name: &str,
    role: UserRole,
    access_all: bool,
    device_name: Option<&str>,
) -> Result<IssuedSession, String> {
    let token = random_token();
    let hash = sha256_hex(token.as_bytes());
    let now = Utc::now();
    let idle = (now + Duration::days(IDLE_DAYS)).to_rfc3339();
    let absolute = (now + Duration::days(ABSOLUTE_DAYS)).to_rfc3339();
    let label = device_label(device_name);
    let _session_id = db
        .create_auth_session(user_id, &hash, &label, &idle, &absolute)
        .map_err(|e| e.to_string())?;
    Ok(IssuedSession {
        token,
        info: SessionInfo {
            user_id,
            username: username.to_string(),
            display_name: display_name.to_string(),
            role,
            access_all,
            expires_at: idle,
        },
    })
}

fn device_label(name: Option<&str>) -> String {
    let trimmed = name.unwrap_or("").trim();
    if trimmed.is_empty() {
        "Unknown device".into()
    } else {
        trimmed.chars().take(64).collect()
    }
}

pub fn lookup_session(db: &Database, token: &str) -> Result<Option<AuthSession>, String> {
    if token.is_empty() {
        return Ok(None);
    }
    let hash = sha256_hex(token.as_bytes());
    db.lookup_auth_session(&hash).map_err(|e| e.to_string())
}

pub fn touch_session(db: &Database, session: &AuthSession) -> Result<Option<String>, String> {
    let now = Utc::now();
    let idle = (now + Duration::days(IDLE_DAYS)).to_rfc3339();
    let rotate = chrono::DateTime::parse_from_rfc3339(&session.rotated_at)
        .ok()
        .map(|t| now.signed_duration_since(t.with_timezone(&Utc)) > Duration::days(ROTATE_AFTER_DAYS))
        .unwrap_or(true);
    if rotate {
        let token = random_token();
        let hash = sha256_hex(token.as_bytes());
        db.rotate_auth_session(session.id, &hash, &idle)
            .map_err(|e| e.to_string())?;
        Ok(Some(token))
    } else {
        db.touch_auth_session(session.id, &idle)
            .map_err(|e| e.to_string())?;
        Ok(None)
    }
}

pub fn session_cookie(token: &str, secure: bool, domain: Option<&str>) -> String {
    let max_age = ABSOLUTE_DAYS * 86400;
    let mut cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={max_age}; SameSite=Lax; HttpOnly"
    );
    if let Some(domain) = domain.map(str::trim).filter(|d| !d.is_empty()) {
        cookie.push_str("; Domain=");
        cookie.push_str(domain);
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn clear_session_cookie(secure: bool, domain: Option<&str>) -> String {
    let mut cookie = format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly");
    if let Some(domain) = domain.map(str::trim).filter(|d| !d.is_empty()) {
        cookie.push_str("; Domain=");
        cookie.push_str(domain);
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn temp_db() -> Arc<Database> {
        let dir = std::env::temp_dir().join(format!("shelf-auth-{}", Uuid::new_v4()));
        Arc::new(Database::open(dir).unwrap())
    }

    #[test]
    fn rejects_short_passwords() {
        assert!(validate_password("1234567").is_err());
        assert!(validate_password("12345678").is_ok());
    }

    #[test]
    fn hashes_and_verifies() {
        let hash = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn login_requires_owner_password() {
        let db = temp_db();
        let limiter = LoginLimiter::new();
        let err = login(
            &db,
            &LoginRequest {
                username: "owner".into(),
                password: "correct horse".into(),
                device_name: None,
            },
            &limiter,
            "127.0.0.1",
        )
        .unwrap_err();
        assert!(err.contains("not configured"));
    }

    #[test]
    fn login_issues_session() {
        let db = temp_db();
        let hash = hash_password("correct horse").unwrap();
        db.set_user_password(1, &hash).unwrap();
        let limiter = LoginLimiter::new();
        let issued = login(
            &db,
            &LoginRequest {
                username: "owner".into(),
                password: "correct horse".into(),
                device_name: Some("iPhone".into()),
            },
            &limiter,
            "10.0.0.8",
        )
        .unwrap();
        let found = lookup_session(&db, &issued.token).unwrap().unwrap();
        assert_eq!(found.viewer.user_id, 1);
        assert_eq!(found.username, "owner");
        assert!(lookup_session(&db, "nope").unwrap().is_none());
    }

    #[test]
    fn session_cookie_sets_secure_and_domain() {
        let cookie = session_cookie("abc", true, Some("shelf.example.com"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Domain=shelf.example.com"));
        assert!(!session_cookie("abc", false, None).contains("Secure"));
    }

    #[test]
    fn limiter_trips_after_burst() {
        let limiter = LoginLimiter::new();
        for _ in 0..8 {
            assert!(limiter.allow("1.2.3.4"));
        }
        assert!(!limiter.allow("1.2.3.4"));
        assert!(limiter.allow("9.9.9.9"));
    }
}
