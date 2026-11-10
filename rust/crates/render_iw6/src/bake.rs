//! bo2mp: Ghosts maps' pictures from the map bake (`map_bake`).
//!
//! The renderer's warm read each picture its lists bind from the zones and
//! the `imagefile<N>.pak`s beside them, one after the other, scattered over
//! the game's packages (Ghosts' mp_ca_rumble: 3.8 GB of pictures in a 92 s
//! "lists made" on the owner's PC, 3 Nov 2026). The first load of a map now
//! also writes each picture as the card took it to
//! `<bake>\iw6\<map>\pictures.pak` (lz4 packed, `map_bake::pictures`); the
//! next load reads that file start to end on one thread while the others
//! unpack and upload, before the warm starts, and the warm finds them made.
//! A picture the file does not hold loads the old way.
//! `BO2MP_IW6_BAKE=0` (or `CODLEGACY_BAKE=0`): the old way only.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use map_bake::Key;
use map_bake::pictures::{Picture, Reader, Writer};
use render_h1::gpu::{self, Gpu, Tex};

/// Whether the Ghosts bake is used (`BO2MP_IW6_BAKE=0`: no).
pub fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_BAKE");
    S.get() && map_bake::enabled()
}

/// This file's layout of the key (raised when what the key covers changes).
const KEY_FORMAT: u64 = 1;

/// The code a picture's bytes come from: a change to any of it makes a new
/// key, never a stale picture.
const CODE: &[&str] = &[
    include_str!("bake.rs"),
    include_str!("scene.rs"),
    include_str!("sky.rs"),
    include_str!("../../fastfile_iw6/src/image.rs"),
    include_str!("../../fastfile_iw6/src/pak.rs"),
    include_str!("../../fastfile_iw6/src/shared.rs"),
    include_str!("../../fastfile_iw6/src/walk/mod.rs"),
];

/// The map's name from its zone list (`zones[0]` is the map).
pub fn map_name(zones: &[PathBuf]) -> Option<String> {
    zones.first()?.file_stem().map(|s| s.to_string_lossy().into_owned())
}

/// A map's baked pictures: where they go and what they must have been made from.
pub struct Pictures {
    dir: PathBuf,
    key: Key,
    parts: map_bake::why::Parts,
    map: String,
    cap: u32,
}

impl Pictures {
    /// The bake of the pictures of the map in `zones` at size cap `cap`;
    /// `None` with the bake off or nowhere to put it.
    pub fn new(zones: &[PathBuf], cap: u32) -> Option<Self> {
        if !on() {
            return None;
        }
        let map = map_name(zones)?;
        let dir = map_bake::map_dir("iw6", &map)?;
        use map_bake::why;
        let mut key = Key::new();
        let mut parts = why::Parts::default();
        parts.add(why::FORMAT, &mut key, |k| {
            k.str("iw6 pictures").u64(KEY_FORMAT).u64(u64::from(map_bake::pictures::FORMAT));
        });
        parts.add(why::SETTINGS, &mut key, |k| {
            k.u64(u64::from(cap));
        });
        parts.add(why::GAME_FILES, &mut key, |k| {
            // The zones themselves, and every picture package beside them.
            let mut dirs: Vec<&Path> = Vec::new();
            for z in zones {
                k.file(z);
                let d = z.parent().unwrap_or(Path::new("."));
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
            for d in dirs {
                k.dir_files(d, &["pak"]);
            }
        });
        parts.add(why::CODE, &mut key, |k| {
            for c in CODE {
                k.str(c);
            }
        });
        Some(Self { dir, key, parts, map, cap })
    }

    /// The pak, if one was made with this key and is whole.
    pub fn open(&self) -> Option<Reader> {
        Reader::open(&self.dir.join(map_bake::pictures::FILE), self.key)
    }

    /// Why there is no usable pak ("first load", "game updated").
    pub fn why(&self) -> String {
        map_bake::why::reason(&self.dir, map_bake::pictures::FILE, &self.parts)
    }

    /// A new pak for this key; its end is noted in the load's log.
    pub fn writer(&self) -> Option<Writer> {
        Writer::start(&self.dir, self.key, |line| eprintln!("note: iw6 {line}"))
    }

    /// Throws the pak away (it stopped reading back).
    pub fn remove(&self) {
        let _ = std::fs::remove_file(self.dir.join(map_bake::pictures::FILE));
    }

    /// The manifest written beside the pak.
    pub fn manifest(&self, count: usize) -> String {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        format!(
            "{{\n  \"game\": \"iw6\",\n  \"map\": {},\n  \"key\": \"{:016x}\",\n  \"format\": {},\n  \"picture_cap\": {},\n  \"pictures\": {count},\n  \"written_unix\": {at},\n  {}\n}}\n",
            map_bake::pictures::json_str(&self.map),
            self.key.value(),
            map_bake::pictures::FORMAT,
            self.cap,
            self.parts.to_json()
        )
    }

    /// The pak's folder (for the log).
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Starts reading the map's `pictures.pak` (if it has one) into the file
/// cache as the load begins, beside the zone reads, so the warm finds it in
/// memory (`CODLEGACY_BAKE_READ_AHEAD=0`: off).
pub fn read_ahead(zones: &[PathBuf]) {
    if !on() {
        return;
    }
    let Some(dir) = map_name(zones).and_then(|m| map_bake::map_dir("iw6", &m)) else { return };
    map_bake::read_ahead(dir.join(map_bake::pictures::FILE), |line| eprintln!("note: iw6 {line}"));
}

/// A picture as the card takes it, for the pak (`part`: its faces, 1 or 6).
pub fn picture(name: &str, px: &fastfile_h1::image::ImagePixels, faces: u32) -> Picture {
    Picture {
        name: name.to_owned(),
        format: px.dxgi_format,
        width: px.width,
        height: px.height,
        levels: px.levels,
        part: Some(u64::from(faces)),
        data: px.data.clone(),
    }
}

/// Every picture of `reader`, read in file order on one thread and unpacked
/// and uploaded on the others: (name, texture) for each that uploaded, the
/// bytes uploaded, and the read error if the file stopped reading back (the
/// pictures after it load the old way).
#[allow(clippy::type_complexity)]
pub fn upload_all(g: &Gpu, mut reader: Reader) -> (Vec<(String, Tex)>, u64, Option<String>) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<map_bake::pictures::Packed>(32);
    let rx = Mutex::new(rx);
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 6);
    let bytes = std::sync::atomic::AtomicU64::new(0);
    let bytes = &bytes;
    let (got, err) = std::thread::scope(|s| {
        let read = s.spawn(move || -> Option<String> {
            loop {
                match reader.next_packed() {
                    Ok(Some(p)) => {
                        if tx.send(p).is_err() {
                            return None;
                        }
                    }
                    Ok(None) => return None,
                    Err(e) => return Some(e.to_string()),
                }
            }
        });
        let uploads: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let next = match rx.lock() {
                            Ok(r) => r.recv(),
                            Err(_) => break,
                        };
                        let Ok(p) = next else { break };
                        // (A picture that does not unpack or upload loads the old way.)
                        let Ok(p) = p.unpack() else { continue };
                        let faces = p.part.map_or(1, |f| f as u32).max(1);
                        let px = fastfile_h1::image::ImagePixels { width: p.width, height: p.height, levels: p.levels, dxgi_format: p.format, data: p.data };
                        if let Ok(t) = gpu::upload(g, &p.name, &px, faces) {
                            bytes.fetch_add(px.data.len() as u64, std::sync::atomic::Ordering::Relaxed);
                            out.push((p.name, t));
                        }
                    }
                    out
                })
            })
            .collect();
        let err = read.join().ok().flatten();
        let got = uploads.into_iter().filter_map(|h| h.join().ok()).flatten().collect();
        (got, err)
    });
    (got, bytes.load(std::sync::atomic::Ordering::Relaxed), err)
}

/// The bake as the renderer's load goes: not begun, writing the first
/// load's pictures, or over.
#[derive(Default)]
pub enum State {
    #[default]
    NotBegun,
    Writing(Pictures, Writer, usize),
    Over,
}
