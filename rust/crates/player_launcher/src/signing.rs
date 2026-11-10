//! Signed manifests. The publisher signs the exact bytes of a manifest with an Ed25519 key that
//! never leaves their PC; the launcher has the matching public key built in and refuses
//! anything that does not verify. A hacked web host or CDN therefore cannot push a program
//! to players, it can only serve what was signed.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use crate::error::{Error, Fault};

/// Mixed into every signature so a manifest signature can never be reused for anything else.
const CONTEXT: &[u8] = b"codlegacy-manifest-v1\n";

/// The public key compiled into this launcher (`update_key.pub`, 64 hex characters).
pub fn embedded_public_key() -> Option<[u8; 32]> {
    parse_key(include_str!("../update_key.pub").trim())
}

fn parse_key(hex: &str) -> Option<[u8; 32]> {
    let v = from_hex(hex)?;
    <[u8; 32]>::try_from(v).ok()
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// A new key pair as (secret hex, public hex). Keep the secret off the repo and off the server.
pub fn keygen() -> Result<(String, String), Error> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| Error::other(format!("No random numbers available: {e}")))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((to_hex(&seed), to_hex(key.verifying_key().as_bytes())))
}

fn signed_bytes(manifest: &[u8]) -> Vec<u8> {
    let mut v = CONTEXT.to_vec();
    v.extend_from_slice(manifest);
    v
}

pub fn sign(secret_hex: &str, manifest: &[u8]) -> Result<String, Error> {
    let seed = parse_key(secret_hex.trim()).ok_or_else(|| Error::other("The signing key must be 64 hex characters."))?;
    Ok(to_hex(&SigningKey::from_bytes(&seed).sign(&signed_bytes(manifest)).to_bytes()))
}

pub fn verify(public: &[u8; 32], manifest: &[u8], signature_hex: &str) -> Result<(), Error> {
    let bad = || Error::new(Fault::BadChecksum, "The update is not signed by CodLegacy, so it was not installed.");
    let key = VerifyingKey::from_bytes(public).map_err(|_| bad())?;
    let sig_bytes = <[u8; 64]>::try_from(from_hex(signature_hex.trim()).ok_or_else(bad)?).map_err(|_| bad())?;
    key.verify(&signed_bytes(manifest), &Signature::from_bytes(&sig_bytes)).map_err(|_| bad())
}

/// Verifies against the key built into this launcher.
pub fn verify_embedded(manifest: &[u8], signature_hex: &str) -> Result<(), Error> {
    let key = embedded_public_key().ok_or_else(|| Error::other("Updates are switched off: this launcher was built without an update signing key."))?;
    verify(&key, manifest, signature_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_verify_only_for_the_same_bytes_and_key() {
        let (secret, public) = keygen().unwrap();
        let public = parse_key(&public).unwrap();
        let sig = sign(&secret, b"{\"version\":\"1\"}").unwrap();
        assert!(verify(&public, b"{\"version\":\"1\"}", &sig).is_ok());
        assert!(verify(&public, b"{\"version\":\"2\"}", &sig).is_err(), "changed manifest");
        let (_, other_public) = keygen().unwrap();
        assert!(verify(&parse_key(&other_public).unwrap(), b"{\"version\":\"1\"}", &sig).is_err(), "someone else's key");
        assert!(verify(&public, b"x", "nothex").is_err());
        assert!(verify(&public, b"x", &"00".repeat(64)).is_err());
        assert!(sign("short", b"x").is_err());
    }

    #[test]
    fn hex_helpers_round_trip() {
        assert_eq!(from_hex(&to_hex(&[0, 1, 254, 255])).unwrap(), [0, 1, 254, 255]);
        assert!(from_hex("abc").is_none());
        assert!(from_hex("zz").is_none());
    }

    #[test]
    fn an_unset_public_key_means_updates_are_off() {
        // The placeholder file is not a key, so nothing verifies until the owner generates one.
        if embedded_public_key().is_none() {
            assert!(verify_embedded(b"x", &"00".repeat(64)).is_err());
        }
    }
}
