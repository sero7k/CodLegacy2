//! Random tokens, hashing of stored secrets, and password hashing (argon2id).

use argon2::{
    Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version, password_hash::phc::PasswordHash,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("operating system random source failed");
    b
}

/// A session token, join ticket, exchange code or sign-in state: 256 random bits.
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

/// A launcher key: 128 random bits as 32 lowercase hex characters.
pub fn random_launcher_key() -> String {
    hex(&random_bytes::<16>())
}

pub fn is_launcher_key(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn random_below(n: u32) -> u32 {
    u32::from_le_bytes(random_bytes::<4>()) % n.max(1)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Secrets are stored as the SHA-256 of the secret, so a copy of the database
/// does not contain anything that can be used to sign in.
pub fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

pub struct Hasher {
    argon: Argon2<'static>,
    dummy: String,
}

impl Hasher {
    pub fn new(cheap: bool) -> Self {
        let argon = if cheap {
            let params = Params::new(8, 1, 1, None).expect("valid test parameters");
            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        } else {
            Argon2::default()
        };
        let dummy = PasswordHasher::<PasswordHash>::hash_password(&argon, b"dummy password for timing")
            .expect("hashing works")
            .to_string();
        Self { argon, dummy }
    }

    pub fn hash(&self, password: &str) -> Option<String> {
        PasswordHasher::<PasswordHash>::hash_password(&self.argon, password.as_bytes()).ok().map(|h| h.to_string())
    }

    /// Verify a password. `stored = None` (no such account, or no password set)
    /// still does the same work against a dummy hash and returns false, so the
    /// response time does not reveal which usernames exist.
    pub fn verify(&self, stored: Option<&str>, password: &str) -> bool {
        match stored {
            Some(h) => self.argon.verify_password(password.as_bytes(), h).is_ok(),
            None => {
                let _ = self.argon.verify_password(password.as_bytes(), self.dummy.as_str());
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let h = Hasher::new(true);
        let stored = h.hash("correct horse").unwrap();
        assert!(h.verify(Some(&stored), "correct horse"));
        assert!(!h.verify(Some(&stored), "wrong"));
        assert!(!h.verify(None, "correct horse"));
    }

    #[test]
    fn tokens_are_distinct_and_keys_are_hex() {
        assert_ne!(random_token(), random_token());
        let k = random_launcher_key();
        assert!(is_launcher_key(&k), "{k}");
        assert!(!is_launcher_key("zz"));
        assert_eq!(sha256_hex("a").len(), 64);
    }
}
