//! The build manifest: which files make up a CodLegacy build, how big, and their SHA-256.
//! It lists CodLegacy's own program files only. Black Ops II's game files are the player's
//! own and are never listed, hosted or downloaded.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Error;

pub const MAX_FILES: usize = 5_000;
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    /// Forward-slash path inside the build, e.g. `CodLegacy.exe`.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    /// Where the files are served from; each file is at `base_url/<path>`.
    pub base_url: String,
    pub files: Vec<FileEntry>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// A path that stays inside the install folder: relative, no `..`, no drive letters or backslashes.
pub fn safe_rel_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 260
        && !p.starts_with('/')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains(['\\', ':', '\0']))
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, Error> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| Error::other(format!("The build manifest is not valid ({e}).")))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), Error> {
        let bad = |why: &str| Err(Error::other(format!("The build manifest is not valid ({why}).")));
        if self.version.is_empty() || self.version.len() > 40 || !self.version.chars().all(|c| c.is_ascii_alphanumeric() || ".-+_".contains(c)) {
            return bad("version");
        }
        if !(self.base_url.starts_with("https://") || self.base_url.starts_with("http://localhost") || self.base_url.starts_with("http://127.0.0.1")) {
            return bad("base_url must be https");
        }
        if self.files.is_empty() || self.files.len() > MAX_FILES {
            return bad("file count");
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.files {
            if !safe_rel_path(&f.path) || f.size > MAX_FILE_BYTES || f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return bad(&format!("entry {}", f.path));
            }
            if !seen.insert(f.path.to_ascii_lowercase()) {
                return bad("duplicate path");
            }
        }
        Ok(())
    }

    pub fn file_url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }
}

/// Makes a manifest for a folder of build output (the publishing side, `codlegacy-launcher manifest`).
pub fn build(dir: &Path, version: &str, base_url: &str) -> Result<Manifest, Error> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = std::fs::read_dir(&d).map_err(|e| Error::other(format!("{}: {e}", d.display())))?;
        for entry in rd.flatten() {
            let path = entry.path();
            let ft = entry.file_type().map_err(|e| Error::other(e.to_string()))?;
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                let rel = path.strip_prefix(dir).map_err(|e| Error::other(e.to_string()))?.to_string_lossy().replace('\\', "/");
                let bytes = std::fs::read(&path).map_err(|e| Error::other(format!("{}: {e}", path.display())))?;
                files.push(FileEntry { path: rel, size: bytes.len() as u64, sha256: sha256_hex(&bytes) });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let m = Manifest { version: version.into(), base_url: base_url.into(), files };
    m.validate()?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_must_stay_inside() {
        for ok in ["CodLegacy.exe", "bin/x.dll", "a/b/c.txt"] {
            assert!(safe_rel_path(ok), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../x", "a//b", "C:/x", "a\\b", "./x", "a/"] {
            assert!(!safe_rel_path(bad), "{bad}");
        }
    }

    #[test]
    fn build_then_parse_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("CodLegacy.exe"), b"exe").unwrap();
        std::fs::write(dir.path().join("bin/a.dll"), b"dll").unwrap();
        let m = build(dir.path(), "1.0.0", "https://dl.example.test/build").unwrap();
        assert_eq!(m.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["CodLegacy.exe", "bin/a.dll"]);
        assert_eq!(m.files[0].sha256, sha256_hex(b"exe"));
        let again = Manifest::parse(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(again, m);
        assert_eq!(m.file_url("bin/a.dll"), "https://dl.example.test/build/bin/a.dll");
    }

    #[test]
    fn hostile_manifests_are_refused() {
        let h = "a".repeat(64);
        let mk = |path: &str, base: &str| format!(r#"{{"version":"1","base_url":"{base}","files":[{{"path":"{path}","size":1,"sha256":"{h}"}}]}}"#);
        assert!(Manifest::parse(&mk("x.exe", "https://a.test")).is_ok());
        assert!(Manifest::parse(&mk("../x.exe", "https://a.test")).is_err());
        assert!(Manifest::parse(&mk("x.exe", "http://evil.test")).is_err());
        assert!(Manifest::parse(&mk("x.exe", "ftp://a.test")).is_err());
        assert!(Manifest::parse("{}").is_err());
    }
}
