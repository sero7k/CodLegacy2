//! Service configuration, read from environment variables.

use std::env;

use url::{Host, Url};

use crate::crypto::sha256_hex;

#[derive(Clone, Debug)]
pub struct Config {
    /// Address to listen on, e.g. `127.0.0.1:8080`.
    pub bind: String,
    /// SQLite file. `:memory:` keeps everything in RAM (tests).
    pub db_path: String,
    /// Public base URL of this service, no trailing slash. Steam sends the
    /// browser back here, so it must be reachable from the player's browser.
    pub public_url: String,
    /// Origins (scheme + host + port) that may call the API from a browser and
    /// that sign-in may return to. The service's own origin is always included.
    pub allowed_origins: Vec<String>,
    /// Header that carries the client address when a proxy sits in front
    /// (e.g. `CF-Connecting-IP`). Only set it if the proxy overwrites it.
    pub trust_ip_header: Option<String>,
    /// Game servers allowed to call `/verify` and `/results`: (name, sha256 of key).
    pub server_keys: Vec<(String, String)>,
    /// sha256 of the admin key (`X-Admin-Key`): answers tickets, bans, lists requests.
    pub admin_key: Option<String>,
    /// Lifetime of a website session.
    pub session_days: i64,
    /// Cheap password hashing, for tests only.
    pub cheap_hashing: bool,
    /// Folder with the website (`web/`) to serve at `/`, for local testing and
    /// single-host setups.
    pub static_dir: Option<String>,
}

fn is_loopback_url(u: &Url) -> bool {
    match u.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|k| env::var(k).ok())
    }

    /// Like `from_env`, reading settings through `get` (tests pass a map).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let var = |k: &str| get(k).filter(|v| !v.trim().is_empty());
        let public_url = var("ACCOUNTS_PUBLIC_URL").unwrap_or_else(|| "http://localhost:8080".into());
        let public_url = public_url.trim().trim_end_matches('/').to_string();
        let parsed = Url::parse(&public_url).map_err(|e| format!("ACCOUNTS_PUBLIC_URL: {e}"))?;
        if parsed.scheme() != "https" && !(parsed.scheme() == "http" && is_loopback_url(&parsed)) {
            return Err("ACCOUNTS_PUBLIC_URL must be https (http only for localhost)".into());
        }
        let mut allowed_origins: Vec<String> = var("ACCOUNTS_ALLOWED_ORIGINS")
            .map(|v| v.split(',').map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_else(Vec::new);
        let own = parsed.origin().ascii_serialization();
        if !allowed_origins.contains(&own) {
            allowed_origins.push(own);
        }
        let mut server_keys = Vec::new();
        if let Some(v) = var("ACCOUNTS_SERVER_KEYS") {
            for item in v.split(',') {
                let (name, key) = item.split_once(':').ok_or("ACCOUNTS_SERVER_KEYS items are name:key")?;
                if key.trim().len() < 24 {
                    return Err(format!("server key '{}' is too short (24+ characters)", name.trim()));
                }
                server_keys.push((name.trim().to_string(), sha256_hex(key.trim())));
            }
        }
        let admin_key = match var("ACCOUNTS_ADMIN_KEY") {
            Some(k) if k.trim().len() < 24 => return Err("ACCOUNTS_ADMIN_KEY is too short (24+ characters)".into()),
            Some(k) => Some(sha256_hex(k.trim())),
            None => None,
        };
        Ok(Self {
            bind: var("ACCOUNTS_BIND").unwrap_or_else(|| "127.0.0.1:8080".into()),
            db_path: var("ACCOUNTS_DB").unwrap_or_else(|| "accounts.sqlite".into()),
            public_url,
            allowed_origins,
            trust_ip_header: var("ACCOUNTS_TRUST_IP_HEADER"),
            server_keys,
            admin_key,
            session_days: var("ACCOUNTS_SESSION_DAYS").and_then(|v| v.parse().ok()).unwrap_or(30),
            cheap_hashing: false,
            static_dir: var("ACCOUNTS_STATIC_DIR"),
        })
    }

    /// A configuration for tests: in-memory database, cheap hashing, one
    /// allowed web origin, one game server key (`test-server-key-0123456789`).
    pub fn for_tests() -> Self {
        Self {
            bind: "127.0.0.1:0".into(),
            db_path: ":memory:".into(),
            public_url: "https://api.example.test".into(),
            allowed_origins: vec!["https://api.example.test".into(), "https://site.example.test".into()],
            trust_ip_header: None,
            server_keys: vec![("test".into(), sha256_hex("test-server-key-0123456789"))],
            admin_key: Some(sha256_hex("test-admin-key-0123456789abcdef")),
            session_days: 30,
            cheap_hashing: true,
            static_dir: None,
        }
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == origin)
    }

    /// Whether sign-in may send the browser back to `target`. Allowed: pages on
    /// an allowed origin, and the launcher's short-lived loopback listener.
    /// Anything else would make the service an open redirect.
    pub fn return_allowed(&self, target: &str) -> bool {
        let Ok(u) = Url::parse(target) else { return false };
        if !u.username().is_empty() || u.password().is_some() {
            return false;
        }
        match u.scheme() {
            "https" | "http" => {}
            _ => return false,
        }
        if self.origin_allowed(&u.origin().ascii_serialization()) {
            return true;
        }
        u.scheme() == "http" && is_loopback_url(&u)
    }
}
