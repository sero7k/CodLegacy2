//! bo2mp: the one "clear cache" (Options) for every cache the game keeps.
//!
//! - The prepared-artifact cache (`%LOCALAPPDATA%\iw4l\cache-<install id>`
//!   for every build folder, and `cache-shared`): each kind's folder in it.
//! - The graphics pipeline caches: Black Ops III's and the exact renderer's
//!   (`t7_pipelines.bin`, `exact_pipelines.bin` in `%LOCALAPPDATA%\iw4l\pipelines`
//!   or `CODLEGACY_CACHE_DIR`), Ghosts' (`iw6_pipelines_<map>.bin` in
//!   `BO2MP_CACHE_DIR`, else the platform's cache folder + `bo2mp`) and the
//!   Vulkan driver's (`codlegacy-artifacts\cache\vk_pipeline_cache.bin`, or
//!   the file `BO2MP_VK_PIPELINE_CACHE` names).
//! - The map bake (`map_bake::clear`).
//!
//! Nothing is deleted on a loose match: only the files and folders these
//! caches make, by their exact names. A cache in a folder the owner named
//! (`CODLEGACY_CACHE_DIR`, a harness's linked folder) keeps its artifact
//! entries and says so; its pipeline files still go (exact names). A file
//! the running game holds open stays and is noted.
//!
//! Gameplay data (stats, hitboxes, collision) is never read from any of
//! these, so clearing them only costs load time.

use std::path::{Path, PathBuf};

pub use map_bake::clear::Cleared;

/// Clears every cache (see the module notes); the result says what went.
pub fn clear_all() -> Cleared {
    let mut out = Cleared::default();
    let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(PathBuf::from);
    artifacts(local.as_deref(), &mut out);
    pipelines(local.as_deref(), &mut out);
    map_bake::clear::clear(&mut out);
    diag::info!(Zone, "clear cache: {}", out.summary());
    for n in &out.notes {
        diag::info!(Zone, "clear cache: {n}");
    }
    out
}

/// The artifact cache's folders under `<local>\iw4l`.
fn artifacts(local: Option<&Path>, out: &mut Cleared) {
    if let Some(dir) = codlegacy_env::var_os("CODLEGACY_CACHE_DIR").filter(|v| !v.is_empty()) {
        out.notes.push(format!("artifact cache in {} left as it is (a folder named by CODLEGACY_CACHE_DIR)", PathBuf::from(dir).display()));
        return;
    }
    let Some(local) = local else { return };
    let iw4l = local.join("iw4l");
    for e in std::fs::read_dir(&iw4l).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !e.file_type().is_ok_and(|t| t.is_dir()) || !is_cache_folder(&name) {
            continue;
        }
        clear_cache_folder(&e.path(), out);
    }
}

/// `cache-shared` or `cache-` and the install's 16-digit id.
pub(crate) fn is_cache_folder(name: &str) -> bool {
    name == "cache-shared" || name.strip_prefix("cache-").is_some_and(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Every kind's folder in one artifact cache folder (`<kind>\<2>\<key>`);
/// anything else in it (`install.txt`) stays.
pub(crate) fn clear_cache_folder(dir: &Path, out: &mut Cleared) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type().is_ok_and(|t| t.is_dir()) && crate::artifact_cache::kind_ok(&name) {
            remove_tree(&e.path(), out);
        }
    }
}

/// A cache's own folder, every file and folder in it (links are not followed).
fn remove_tree(dir: &Path, out: &mut Cleared) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            remove_tree(&e.path(), out);
        } else if t.is_file() {
            out.remove_file(&e.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// The pipeline cache files (exact names).
fn pipelines(local: Option<&Path>, out: &mut Cleared) {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    // (render_t7::pipecache::cache_dir's rule.)
    let t7 = codlegacy_env::var_os("CODLEGACY_CACHE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| local.map(|l| l.join("iw4l").join("pipelines")))
        .or_else(|| var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache"))).map(|d| d.join("iw4l")));
    if let Some(dir) = t7 {
        for f in ["t7_pipelines.bin", "exact_pipelines.bin"] {
            remove_if_file(&dir.join(f), out);
        }
    }
    // (render_iw6::pipecache::cache_dir's rule.)
    let iw6 = var("BO2MP_CACHE_DIR").or_else(|| var("XDG_CACHE_HOME").or_else(|| local.map(Path::to_path_buf)).or_else(|| var("HOME").map(|h| h.join(".cache"))).map(|b| b.join("bo2mp")));
    if let Some(dir) = iw6 {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_iw6_pipelines(&name) && e.file_type().is_ok_and(|t| t.is_file()) {
                out.remove_file(&e.path());
            }
        }
    }
    // (third_party/wgpu-hal auto_cache's rule.)
    let vk = match std::env::var("BO2MP_VK_PIPELINE_CACHE") {
        Ok(v) if v.trim() == "0" => None,
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v.trim())),
        _ => Some(Path::new(crate::discover::ARTIFACTS_DIR).join("cache").join("vk_pipeline_cache.bin")),
    };
    if let Some(f) = vk {
        remove_if_file(&f, out);
    }
}

/// `iw6_pipelines_<map>.bin`.
pub(crate) fn is_iw6_pipelines(name: &str) -> bool {
    name.strip_prefix("iw6_pipelines_").and_then(|m| m.strip_suffix(".bin")).is_some_and(|m| !m.is_empty() && m.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

fn remove_if_file(path: &Path, out: &mut Cleared) {
    if path.is_file() {
        out.remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_exact() {
        assert!(is_cache_folder("cache-shared"));
        assert!(is_cache_folder("cache-0123456789abcdef"));
        assert!(!is_cache_folder("cache-0123"));
        assert!(!is_cache_folder("cache-notmine-xxxxxxxx"));
        assert!(!is_cache_folder("pipelines"));
        assert!(is_iw6_pipelines("iw6_pipelines_mp_prisonbreak.bin"));
        assert!(!is_iw6_pipelines("iw6_pipelines_.bin"));
        assert!(!is_iw6_pipelines("iw6_pipelines_x.bin.bak"));
        assert!(!is_iw6_pipelines("my_iw6_pipelines_x.bin"));
    }

    #[test]
    fn cache_folder_keeps_what_it_did_not_make() {
        let dir = std::env::temp_dir().join(format!("clear_cache_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mips").join("ab")).unwrap();
        std::fs::write(dir.join("mips").join("ab").join("abcd"), [1u8; 10]).unwrap();
        std::fs::write(dir.join("install.txt"), b"x").unwrap();
        std::fs::create_dir_all(dir.join("Not A Kind")).unwrap();
        std::fs::write(dir.join("Not A Kind").join("keep"), b"x").unwrap();
        let mut out = Cleared::default();
        clear_cache_folder(&dir, &mut out);
        assert_eq!((out.files, out.bytes), (1, 10));
        assert!(!dir.join("mips").exists());
        assert!(dir.join("install.txt").is_file());
        assert!(dir.join("Not A Kind").join("keep").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
