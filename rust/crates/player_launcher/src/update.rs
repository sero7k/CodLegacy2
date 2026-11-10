//! Brings the installed CodLegacy build in line with the manifest.
//! Every file is downloaded to a temporary name, checked against its size and SHA-256,
//! and only then moved into place, so a cut connection never leaves a half-written program.

use std::path::{Path, PathBuf};

use crate::error::{Error, Fault};
use crate::manifest::{FileEntry, Manifest, sha256_hex};

pub const VERSION_FILE: &str = "version.txt";

/// The files that are missing or differ from the manifest.
pub fn stale<'a>(m: &'a Manifest, install: &Path) -> Vec<&'a FileEntry> {
    m.files
        .iter()
        .filter(|f| match std::fs::read(install.join(&f.path)) {
            Ok(bytes) => bytes.len() as u64 != f.size || sha256_hex(&bytes) != f.sha256,
            Err(_) => true,
        })
        .collect()
}

pub fn installed_version(install: &Path) -> Option<String> {
    std::fs::read_to_string(install.join(VERSION_FILE)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Progress as (files done, files total).
pub type Progress<'a> = &'a mut dyn FnMut(usize, usize);

/// Fetches a URL, at most the given number of bytes.
pub type Fetch<'a> = &'a dyn Fn(&str, u64) -> Result<Vec<u8>, Error>;

/// Downloads what is stale. `fetch(url, max_bytes)` returns the file's bytes.
/// Returns how many files were replaced.
pub fn apply(
    m: &Manifest,
    install: &Path,
    fetch: Fetch,
    progress: Progress,
) -> Result<usize, Error> {
    let todo = stale(m, install);
    let total = todo.len();
    progress(0, total);
    for (i, f) in todo.iter().enumerate() {
        let bytes = fetch(&m.file_url(&f.path), f.size)?;
        if bytes.len() as u64 != f.size || sha256_hex(&bytes) != f.sha256 {
            return Err(Error::new(Fault::BadChecksum, format!("{} does not match its checksum, so it was not installed.", f.path)));
        }
        let dest: PathBuf = install.join(&f.path);
        let io = |e: std::io::Error| Error::other(format!("{}: {e}", dest.display()));
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = dest.with_file_name(format!("{}.part", dest.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned())));
        std::fs::write(&tmp, &bytes).map_err(io)?;
        // Windows will not rename over a file that exists.
        if dest.exists() {
            std::fs::remove_file(&dest).map_err(io)?;
        }
        std::fs::rename(&tmp, &dest).map_err(io)?;
        progress(i + 1, total);
    }
    std::fs::create_dir_all(install).map_err(|e| Error::other(e.to_string()))?;
    std::fs::write(install.join(VERSION_FILE), &m.version).map_err(|e| Error::other(e.to_string()))?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn manifest(files: &[(&str, &[u8])]) -> Manifest {
        Manifest {
            version: "1.2.3".into(),
            base_url: "https://dl.example.test".into(),
            files: files.iter().map(|(p, b)| FileEntry { path: (*p).into(), size: b.len() as u64, sha256: sha256_hex(b) }).collect(),
        }
    }

    fn server(files: &[(&str, &[u8])]) -> impl Fn(&str, u64) -> Result<Vec<u8>, Error> {
        let map: HashMap<String, Vec<u8>> = files.iter().map(|(p, b)| (format!("https://dl.example.test/{p}"), b.to_vec())).collect();
        move |url, _| map.get(url).cloned().ok_or_else(|| Error::other("404"))
    }

    #[test]
    fn installs_missing_and_replaces_changed_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let files: [(&str, &[u8]); 2] = [("CodLegacy.exe", b"new exe"), ("bin/a.dll", b"dll")];
        let m = manifest(&files);
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("bin/a.dll"), b"dll").unwrap();
        std::fs::write(dir.path().join("CodLegacy.exe"), b"old exe").unwrap();
        assert_eq!(stale(&m, dir.path()).len(), 1);
        let mut seen = vec![];
        let n = apply(&m, dir.path(), &server(&files), &mut |d, t| seen.push((d, t))).unwrap();
        assert_eq!(n, 1);
        assert_eq!(seen, [(0, 1), (1, 1)]);
        assert_eq!(std::fs::read(dir.path().join("CodLegacy.exe")).unwrap(), b"new exe");
        assert_eq!(installed_version(dir.path()).as_deref(), Some("1.2.3"));
        assert!(stale(&m, dir.path()).is_empty());
        assert!(!dir.path().join("CodLegacy.exe.part").exists());
    }

    #[test]
    fn a_tampered_download_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let m = manifest(&[("CodLegacy.exe", b"real")]);
        let err = apply(&m, dir.path(), &server(&[("CodLegacy.exe", b"evil")]), &mut |_, _| {}).unwrap_err();
        assert_eq!(err.fault, Fault::BadChecksum);
        assert!(!dir.path().join("CodLegacy.exe").exists());
        assert_eq!(installed_version(dir.path()), None);
    }
}
