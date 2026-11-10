//! Where the Battlefield 1 install is: the `bf1` line of `game_paths.cfg`
//! (the folder that holds `Data` and `Patch`), overridden by `BF1_FILES`
//! (`BF1_GAME_DIR`, the upstream viewer's name, is accepted too).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Environment override: the folder that holds `Data` and `Patch`.
pub const ENV_FILES: &str = "BF1_FILES";
/// The upstream viewer's variable, honoured as a fallback.
pub const ENV_LEGACY: &str = "BF1_GAME_DIR";

#[derive(Debug, PartialEq, Eq)]
pub enum Bf1Error {
    /// Neither `BF1_FILES` nor a `bf1=` line is set.
    NotConfigured,
    /// The configured folder does not look like an install.
    NotAnInstall { path: PathBuf, why: String },
    /// `bf1:<id>` names a level this adapter does not know.
    UnknownMap(String),
    /// Opening or reading the install failed.
    Read(String),
}

impl fmt::Display for Bf1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => write!(
                f,
                "Battlefield 1 is not configured: set `bf1=<folder with Data and Patch>` in game_paths.cfg (Options > Game Folders) or the {ENV_FILES} environment variable"
            ),
            Self::NotAnInstall { path, why } => write!(f, "{} is not a Battlefield 1 install: {why}", path.display()),
            Self::UnknownMap(id) => write!(f, "unknown Battlefield 1 map `{id}`"),
            Self::Read(e) => write!(f, "cannot read the Battlefield 1 install: {e}"),
        }
    }
}

impl std::error::Error for Bf1Error {}

/// Whether `root` has the two folders the readers need: `Data` (with
/// `layout.toc`) and `Patch`. Returns the reason when not.
pub fn check_install(root: &Path) -> Result<(), String> {
    if !root.is_dir() {
        return Err("the folder does not exist".to_owned());
    }
    if !root.join("Data").join("layout.toc").is_file() {
        return Err("no Data\\layout.toc (pick the folder that has Data, Patch and bf1.exe)".to_owned());
    }
    if !root.join("Patch").is_dir() {
        return Err("no Patch folder (both Data and Patch are needed)".to_owned());
    }
    Ok(())
}

/// Whether the folder looks like a BF1 install (for the Game Folders panel).
pub fn looks_like_install(root: &Path) -> bool {
    check_install(root).is_ok()
}

/// The install folder: `files` (`BF1_FILES`) wins, then `legacy`
/// (`BF1_GAME_DIR`), then `configured` (the `game_paths.cfg` line). Empty
/// values count as unset. The folder is checked, not opened.
pub fn resolve(configured: Option<&Path>, files: Option<OsString>, legacy: Option<OsString>) -> Result<PathBuf, Bf1Error> {
    let from_env = [files, legacy].into_iter().flatten().find(|v| !v.is_empty()).map(PathBuf::from);
    let path = match from_env {
        Some(p) => p,
        None => match configured {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => return Err(Bf1Error::NotConfigured),
        },
    };
    check_install(&path).map_err(|why| Bf1Error::NotAnInstall { path: path.clone(), why })?;
    Ok(path)
}

/// [`resolve`] with the process environment.
pub fn resolve_from_env(configured: Option<&Path>) -> Result<PathBuf, Bf1Error> {
    resolve(configured, std::env::var_os(ENV_FILES), std::env::var_os(ENV_LEGACY))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bf1-adapter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        std::fs::create_dir_all(dir.join("Patch")).unwrap();
        std::fs::write(dir.join("Data").join("layout.toc"), b"x").unwrap();
        dir
    }

    #[test]
    fn unset_is_a_clean_error() {
        assert_eq!(resolve(None, None, None), Err(Bf1Error::NotConfigured));
        assert_eq!(resolve(Some(Path::new("")), Some(OsString::new()), None), Err(Bf1Error::NotConfigured));
        assert!(Bf1Error::NotConfigured.to_string().contains("BF1_FILES"));
    }

    #[test]
    fn a_wrong_folder_says_why() {
        let dir = std::env::temp_dir().join(format!("bf1-adapter-wrong-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let Err(Bf1Error::NotAnInstall { why, .. }) = resolve(Some(&dir), None, None) else { panic!("expected NotAnInstall") };
        assert!(why.contains("layout.toc"));
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        std::fs::write(dir.join("Data").join("layout.toc"), b"x").unwrap();
        let Err(Bf1Error::NotAnInstall { why, .. }) = resolve(Some(&dir), None, None) else { panic!("expected NotAnInstall") };
        assert!(why.contains("Patch"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!looks_like_install(&dir));
    }

    #[test]
    fn env_beats_config_and_legacy_is_a_fallback() {
        let a = install("a");
        let b = install("b");
        assert_eq!(resolve(Some(&a), Some(b.clone().into()), None).unwrap(), b);
        assert_eq!(resolve(Some(&a), None, Some(b.clone().into())).unwrap(), b);
        assert_eq!(resolve(Some(&a), Some(a.clone().into()), Some(b.clone().into())).unwrap(), a);
        assert_eq!(resolve(Some(&a), None, None).unwrap(), a);
        // An env path that is not an install is an error, not a silent fall back to the config.
        assert!(matches!(resolve(Some(&a), Some(OsString::from("/no/such/folder")), None), Err(Bf1Error::NotAnInstall { .. })));
        for d in [a, b] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
