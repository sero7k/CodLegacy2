//! The game's environment settings are named `CODLEGACY_*`. They were named
//! `IW4L_*` before the engine took its current name, so every read of a
//! `CODLEGACY_*` name that is not set falls back to its old spelling: old
//! `.env` files, scripts and shortcuts keep working.

use std::borrow::Cow;
use std::env::VarError;
use std::ffi::{OsStr, OsString};

const PREFIX: &str = "CODLEGACY_";
const OLD_PREFIX: &str = "IW4L_";

/// `std::env::var`, falling back to the old spelling of a `CODLEGACY_*` name.
pub fn var<K: AsRef<OsStr>>(key: K) -> Result<String, VarError> {
    let key = key.as_ref();
    match std::env::var(key) {
        Err(VarError::NotPresent) => old_name(key).map_or(Err(VarError::NotPresent), std::env::var),
        found => found,
    }
}

/// `std::env::var_os`, falling back to the old spelling of a `CODLEGACY_*` name.
pub fn var_os<K: AsRef<OsStr>>(key: K) -> Option<OsString> {
    let key = key.as_ref();
    std::env::var_os(key).or_else(|| old_name(key).and_then(std::env::var_os))
}

/// Whether `key` is one of the game's settings, in either spelling.
pub fn is_ours(key: &str) -> bool {
    key.starts_with(PREFIX) || key.starts_with(OLD_PREFIX)
}

/// `key` in the current spelling (an `IW4L_*` name becomes `CODLEGACY_*`).
pub fn new_name(key: &str) -> Cow<'_, str> {
    match key.strip_prefix(OLD_PREFIX) {
        Some(rest) => Cow::Owned(format!("{PREFIX}{rest}")),
        None => Cow::Borrowed(key),
    }
}

/// The old spelling of a `CODLEGACY_*` name.
pub fn old_name(key: &OsStr) -> Option<String> {
    key.to_str()?
        .strip_prefix(PREFIX)
        .map(|rest| format!("{OLD_PREFIX}{rest}"))
}

/// Removes a setting from a child's environment in both spellings, so the
/// child cannot fall back to an inherited old one.
pub fn env_remove<'c>(
    command: &'c mut std::process::Command,
    key: &str,
) -> &'c mut std::process::Command {
    if let Some(old) = old_name(OsStr::new(key)) {
        command.env_remove(old);
    }
    command.env_remove(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(new_name("IW4L_GAMES"), "CODLEGACY_GAMES");
        assert_eq!(new_name("CODLEGACY_GAMES"), "CODLEGACY_GAMES");
        assert_eq!(new_name("BO2MP_X"), "BO2MP_X");
        assert_eq!(
            old_name(OsStr::new("CODLEGACY_PERF")).as_deref(),
            Some("IW4L_PERF")
        );
        assert_eq!(old_name(OsStr::new("BO2MP_X")), None);
        assert!(is_ours("IW4L_GAMES") && is_ours("CODLEGACY_GAMES") && !is_ours("PATH"));
    }

    #[test]
    fn old_spelling_is_read_when_the_new_one_is_unset() {
        // A name no other test or setting uses.
        unsafe { std::env::set_var("IW4L_CODLEGACY_ENV_TEST", "old") };
        assert_eq!(var("CODLEGACY_CODLEGACY_ENV_TEST").as_deref(), Ok("old"));
        assert_eq!(
            var_os("CODLEGACY_CODLEGACY_ENV_TEST"),
            Some(OsString::from("old"))
        );
        unsafe { std::env::set_var("CODLEGACY_CODLEGACY_ENV_TEST", "new") };
        assert_eq!(var("CODLEGACY_CODLEGACY_ENV_TEST").as_deref(), Ok("new"));
        assert!(var("CODLEGACY_CODLEGACY_ENV_UNSET").is_err());
        assert!(var_os("NOT_OURS_CODLEGACY_ENV_TEST").is_none());
    }
}
