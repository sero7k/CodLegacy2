//! The launcher updating itself. A signed `launcher.json` manifest names the newest launcher
//! program. A newer one is downloaded, checked (signature, then size and SHA-256), and swapped
//! in by renaming: a running program can be renamed but not overwritten on Windows.

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::manifest::{Manifest, sha256_hex};
use crate::update::Fetch;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const LAUNCHER_FILE: &str = if cfg!(windows) { "codlegacy-launcher.exe" } else { "codlegacy-launcher" };

fn parts(v: &str) -> Option<Vec<u64>> {
    v.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// Whether `candidate` is a strictly newer dotted version than `current`. Anything that is not
/// plain `1.2.3` numbers is never "newer", so an odd manifest cannot trigger an install loop.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parts(current), parts(candidate)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            b > a
        }
        _ => false,
    }
}

/// The manifest's launcher entry, if the manifest is newer than the running launcher.
pub fn available<'a>(m: &'a Manifest, current: &str) -> Option<&'a crate::manifest::FileEntry> {
    if !is_newer(current, &m.version) {
        return None;
    }
    m.files.iter().find(|f| f.path == LAUNCHER_FILE)
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map_or_else(std::ffi::OsString::new, |n| n.to_os_string());
    name.push(suffix);
    path.with_file_name(name)
}

/// Puts `new_bytes` at `target`, keeping the old program as `<target>.old` until the next start.
pub fn replace_exe(target: &Path, new_bytes: &[u8]) -> Result<(), Error> {
    let io = |e: std::io::Error| Error::other(format!("Could not update the launcher: {e}"));
    let (new, old) = (sibling(target, ".new"), sibling(target, ".old"));
    std::fs::write(&new, new_bytes).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old).map_err(|e| {
        let _ = std::fs::remove_file(&new);
        io(e)
    })?;
    if let Err(e) = std::fs::rename(&new, target) {
        // Put the working program back rather than leave the player with none.
        let _ = std::fs::rename(&old, target);
        let _ = std::fs::remove_file(&new);
        return Err(io(e));
    }
    Ok(())
}

/// Removes the previous program after a successful restart.
pub fn cleanup_old(target: &Path) {
    let _ = std::fs::remove_file(sibling(target, ".old"));
    let _ = std::fs::remove_file(sibling(target, ".new"));
}

/// Downloads and installs the launcher from an already-verified manifest.
pub fn install(m: &Manifest, current: &str, target: &Path, fetch: Fetch) -> Result<bool, Error> {
    let Some(entry) = available(m, current) else { return Ok(false) };
    let bytes = fetch(&m.file_url(&entry.path), entry.size)?;
    if bytes.len() as u64 != entry.size || sha256_hex(&bytes) != entry.sha256 {
        return Err(Error::new(crate::Fault::BadChecksum, "The launcher update does not match its checksum, so it was not installed."));
    }
    replace_exe(target, &bytes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileEntry;

    fn manifest(version: &str, bytes: &[u8]) -> Manifest {
        Manifest {
            version: version.into(),
            base_url: "https://dl.example.test".into(),
            files: vec![FileEntry { path: LAUNCHER_FILE.into(), size: bytes.len() as u64, sha256: sha256_hex(bytes) }],
        }
    }

    #[test]
    fn version_order() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.9.9", "0.10.0"));
        assert!(is_newer("1.0", "1.0.1"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.0", "banana"));
        assert!(!is_newer("0.1.0", "0.2.0-beta"));
    }

    #[test]
    fn swaps_the_program_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(LAUNCHER_FILE);
        std::fs::write(&exe, b"old").unwrap();
        let fetch = |_: &str, _: u64| Ok(b"new program".to_vec());
        assert!(install(&manifest("0.2.0", b"new program"), "0.1.0", &exe, &fetch).unwrap());
        assert_eq!(std::fs::read(&exe).unwrap(), b"new program");
        assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"old");
        cleanup_old(&exe);
        assert!(!sibling(&exe, ".old").exists());
    }

    #[test]
    fn same_or_older_version_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(LAUNCHER_FILE);
        std::fs::write(&exe, b"old").unwrap();
        let fetch = |_: &str, _: u64| -> Result<Vec<u8>, Error> { panic!("must not download") };
        assert!(!install(&manifest("0.1.0", b"x"), "0.1.0", &exe, &fetch).unwrap());
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    }

    #[test]
    fn a_wrong_download_leaves_the_program_alone() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(LAUNCHER_FILE);
        std::fs::write(&exe, b"old").unwrap();
        let fetch = |_: &str, _: u64| Ok(b"evil bytes".to_vec());
        let err = install(&manifest("0.2.0", b"good bytes"), "0.1.0", &exe, &fetch).unwrap_err();
        assert_eq!(err.fault, crate::Fault::BadChecksum);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!sibling(&exe, ".old").exists());
    }

    #[test]
    fn a_missing_program_file_cannot_be_swapped() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(LAUNCHER_FILE);
        // No existing program to rename: the swap fails and leaves no stray files.
        assert!(replace_exe(&exe, b"x").is_err());
        assert!(!sibling(&exe, ".new").exists());
    }
}
