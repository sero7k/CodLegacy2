//! An opened install: build config -> `vfs-root` -> TVFS paths, and files read as ranged reads
//! of `Data/data/data.NNN` (never more than the one entry is read).

use crate::idx::{Index, Location};
use crate::tvfs::File;
use crate::{Error, Result, blte, config, idx, tvfs};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct Storage {
    pub install: PathBuf,
    pub build: HashMap<String, String>,
    pub index: Index,
    pub files: Vec<File>,
    by_path: HashMap<String, usize>,
}

impl Storage {
    /// Open read-only.
    pub fn open(install: &Path) -> Result<Self> {
        let info = config::build_info(&std::fs::read_to_string(install.join(".build.info"))?)?;
        let key = info.get("Build Key").ok_or_else(|| Error::Format("no Build Key".into()))?;
        let build = config::key_values(&std::fs::read_to_string(config::config_path(install, key))?);
        let index = idx::load_dir(&install.join("Data/data"))?;
        let vfs = config::ekey_of(build.get("vfs-root").ok_or_else(|| Error::Format("no vfs-root (not TVFS)".into()))?)?;
        let mut s = Self { install: install.to_path_buf(), build, index, files: Vec::new(), by_path: HashMap::new() };
        let root = s.read_ekey(&vfs)?;
        s.files = tvfs::parse(&root)?;
        s.by_path = s.files.iter().enumerate().map(|(i, f)| (f.path.clone(), i)).collect();
        Ok(s)
    }

    pub fn locate(&self, ekey: &[u8]) -> Option<Location> {
        let k: [u8; 9] = ekey.get(..9)?.try_into().ok()?;
        self.index.get(&k).copied()
    }

    /// The raw `data.NNN` entry (30-byte header + BLTE).
    pub fn read_entry(&self, loc: Location) -> Result<Vec<u8>> {
        let mut f = std::fs::File::open(self.install.join(format!("Data/data/data.{:03}", loc.archive)))?;
        f.seek(SeekFrom::Start(loc.offset))?;
        let mut b = vec![0u8; loc.size as usize];
        f.read_exact(&mut b)?;
        Ok(b)
    }

    /// One encoded file, decoded.
    pub fn read_ekey(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        let loc = self.locate(ekey).ok_or_else(|| Error::NotFound(crate::hex(ekey)))?;
        if !loc.resident() {
            return Err(Error::NotFound(format!("{} (not resident)", crate::hex(ekey))));
        }
        blte::decode_entry(&self.read_entry(loc)?, &mut blte::Modes::default())
    }

    pub fn file(&self, path: &str) -> Option<&File> {
        self.by_path.get(&path.to_ascii_lowercase()).map(|&i| &self.files[i])
    }

    /// Whether every span of `f` has data in this install.
    pub fn resident(&self, f: &File) -> bool {
        f.spans.iter().all(|s| self.locate(&s.ekey).is_some_and(|l| l.resident()))
    }

    /// A whole file by its TVFS path.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let f = self.file(path).ok_or_else(|| Error::NotFound(path.into()))?;
        let mut out = Vec::with_capacity(f.size() as usize);
        for s in &f.spans {
            out.extend(self.read_ekey(&s.ekey)?);
        }
        Ok(out)
    }
}
