//! What the launcher remembers: the launcher key, the account-service address and the folders.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Fault};

pub const DEFAULT_API: &str = "https://codlegacy.example.com";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub api_url: String,
    pub key: String,
    /// The player's own Black Ops II folder. Empty means "scan for it".
    pub game_dir: String,
    /// Where CodLegacy's own build lives (the launcher keeps it up to date).
    pub install_dir: String,
    /// The "Call of Claude" folder the launcher built from the player's own games (empty: none yet).
    pub pack_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { api_url: DEFAULT_API.into(), key: String::new(), game_dir: String::new(), install_dir: String::new(), pack_dir: String::new() }
    }
}

/// A launcher key is 32 lower-case hex characters.
pub fn key_is_well_formed(key: &str) -> bool {
    key.len() == 32 && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Cleans up what a player pasted (spaces, capitals) before it is checked.
pub fn clean_key(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase()
}

/// `https` only, except for a loopback address while testing. Returns the address without a trailing slash.
pub fn normalise_api(raw: &str) -> Result<String, Error> {
    let bad = || Error::other("The service address must be https://… (http only for localhost).");
    let u = url::Url::parse(raw.trim()).map_err(|_| bad())?;
    let local = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(u.scheme() == "https" || (u.scheme() == "http" && local)) || u.host_str().is_none() {
        return Err(bad());
    }
    Ok(u.as_str().trim_end_matches('/').to_string())
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("CodLegacy")
}

/// Where the trimmed copy of the player's games goes unless they pick another place.
pub fn default_pack_dir() -> PathBuf {
    default_install_dir().with_file_name(asset_transport::call_of_claude::FOLDER_NAME)
}

pub fn default_install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("CodLegacy").join("game")
}

impl Settings {
    pub fn path() -> PathBuf {
        config_dir().join("launcher.json")
    }

    pub fn install_path(&self) -> PathBuf {
        if self.install_dir.trim().is_empty() { default_install_dir() } else { PathBuf::from(self.install_dir.trim()) }
    }

    pub fn load_from(path: &Path) -> Settings {
        let parsed: Option<Settings> = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok());
        if let Some(s) = parsed {
            return s;
        }
        Settings::default()
    }

    pub fn load() -> Settings {
        Self::load_from(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), Error> {
        let io = |e: std::io::Error| Error::other(format!("Could not save settings: {e}"));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::other(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(io)?;
        // The key is a credential: keep the file private to this user where the OS allows it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path).map_err(io)
    }

    pub fn save(&self) -> Result<(), Error> {
        self.save_to(&Self::path())
    }

    /// Checks the key shape before anything goes on the wire.
    pub fn check_key(&self) -> Result<(), Error> {
        if key_is_well_formed(&self.key) {
            Ok(())
        } else {
            Err(Error::new(Fault::KeyMissing, "Paste your 32-character launcher key from your account page."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_32_lowercase_hex() {
        assert!(key_is_well_formed("0123456789abcdef0123456789abcdef"));
        assert!(!key_is_well_formed("0123456789ABCDEF0123456789abcdef"));
        assert!(!key_is_well_formed("0123456789abcdef"));
        assert!(!key_is_well_formed("g123456789abcdef0123456789abcdef"));
        assert_eq!(clean_key("  0123456789ABCDEF 0123456789abcdef\n"), "0123456789abcdef0123456789abcdef");
    }

    #[test]
    fn service_address_must_be_https_or_loopback() {
        assert_eq!(normalise_api("https://codlegacy.gg/").unwrap(), "https://codlegacy.gg");
        assert_eq!(normalise_api("http://localhost:8080").unwrap(), "http://localhost:8080");
        assert!(normalise_api("http://codlegacy.gg").is_err());
        assert!(normalise_api("ftp://codlegacy.gg").is_err());
        assert!(normalise_api("nonsense").is_err());
    }

    #[test]
    fn settings_round_trip_and_survive_junk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/launcher.json");
        let s = Settings { key: "0123456789abcdef0123456789abcdef".into(), game_dir: "C:/BO2".into(), ..Settings::default() };
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default());
    }
}
