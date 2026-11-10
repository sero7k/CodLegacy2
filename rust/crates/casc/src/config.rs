//! `.build.info` (a `|`-separated table with typed headers) and the build config
//! (`Data/config/xx/yy/<key>`, `key = value` lines).

use crate::{Error, Result};
use std::collections::HashMap;
use std::path::Path;

/// The active row of `.build.info`, by column name (without the `!TYPE:n` suffix).
pub fn build_info(text: &str) -> Result<HashMap<String, String>> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let head: Vec<String> = lines
        .next()
        .ok_or_else(|| Error::Format("empty .build.info".into()))?
        .split('|')
        .map(|c| c.split('!').next().unwrap_or("").to_string())
        .collect();
    let rows: Vec<HashMap<String, String>> = lines
        .map(|l| head.iter().cloned().zip(l.split('|').map(str::to_string)).collect())
        .collect();
    rows.iter()
        .find(|r| r.get("Active").map(String::as_str) == Some("1"))
        .or(rows.first())
        .cloned()
        .ok_or_else(|| Error::Format(".build.info has no rows".into()))
}

/// `key = value` lines (comments start with `#`).
pub fn key_values(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = ").map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect()
}

/// `Data/config/ab/cd/abcd...`.
pub fn config_path(install: &Path, key: &str) -> std::path::PathBuf {
    install.join("Data/config").join(&key[0..2]).join(&key[2..4]).join(key)
}

/// The second hash of a `ckey ekey` pair (the encoded key), as bytes.
pub fn ekey_of(pair: &str) -> Result<Vec<u8>> {
    let e = pair.split_whitespace().nth(1).ok_or_else(|| Error::Format(format!("no ekey in {pair:?}")))?;
    crate::unhex(e).ok_or_else(|| Error::Format(format!("bad hex {e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cold_war_build_info() {
        let t = "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Version!STRING:0\n\
                 eu|1|3107edb8e994e5adc88144e792c303bd|1.35.0.27007542\n";
        let r = build_info(t).unwrap();
        assert_eq!(r["Build Key"], "3107edb8e994e5adc88144e792c303bd");
        assert_eq!(r["Version"], "1.35.0.27007542");
        let kv = key_values("# Build Configuration\n\nvfs-root = 7eeb e7c5\nbuild-product = zeus\n");
        assert_eq!(ekey_of(&kv["vfs-root"]).unwrap(), vec![0xe7, 0xc5]);
    }
}
