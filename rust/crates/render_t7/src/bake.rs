//! bo2mp: Black Ops III maps' pictures from the map bake (`map_bake`).
//!
//! The warm's pictures came from about 2000 scattered places in the game's
//! packages, each decoded: 28.5 s of Skyjacked's 43.7 s load on his PC (a
//! busy hard drive). The first load of a map now also writes each picture
//! as the card took it to `<bake>\t7\<map>\pictures.pak`, in the order they
//! finished; the next load reads that file start to end on one thread while
//! the others upload. A picture the file does not hold, or one the card
//! has no memory for, loads the old way. `CODLEGACY_BAKE=0`: the old way
//! only.

use std::collections::HashMap;
use std::sync::Mutex;

use fastfile_t7::map::T7Map;
use fastfile_t7::texture::ImageLevels;
use map_bake::Key;
use map_bake::pictures::{Picture, Reader, Writer};
use render_h1::gpu::Gpu;

use crate::gpu::Tex;

/// This file's layout of the key (raised when what the key covers changes).
/// (2: the warm's list of pictures is no longer in it, so a map baked
/// ahead, from every picture its materials name, is found by the load; a
/// picture the file lacks loads the old way.)
const KEY_FORMAT: u64 = 2;

/// The code a picture's bytes come from: a change to any of it makes a new
/// key, never a stale picture.
const CODE: &[&str] = &[
    include_str!("bake.rs"),
    include_str!("../../fastfile_t7/src/texture.rs"),
    include_str!("../../fastfile_t7/src/xpak.rs"),
    include_str!("../../fastfile_t7/src/map.rs"),
];

/// A map's baked pictures: where they go and what they must have been made from.
pub(crate) struct Pictures {
    dir: std::path::PathBuf,
    key: Key,
    /// The key's parts, for saying why a copy is made again.
    parts: map_bake::why::Parts,
    map: String,
    cap: u32,
}

impl Pictures {
    /// The bake of `map`'s pictures at size cap `cap`; `None` with the bake
    /// off or nowhere to put it.
    pub(crate) fn new(map: &T7Map, cap: u32) -> Option<Self> {
        let dir = map_bake::map_dir("t7", &map.name)?;
        let (zone, pak_dir, own_dir) = map.sources();
        use map_bake::why;
        // (The same key as one run of it; each part's own key says what
        // changed when it misses.)
        let mut key = Key::new();
        let mut parts = why::Parts::default();
        parts.add(why::FORMAT, &mut key, |k| {
            k.str("t7 pictures").u64(KEY_FORMAT).u64(u64::from(map_bake::pictures::FORMAT));
        });
        parts.add(why::SETTINGS, &mut key, |k| {
            k.u64(u64::from(cap));
        });
        parts.add(why::GAME_FILES, &mut key, |k| {
            k.file(zone).dir_files(pak_dir, &["ff", "xpak"]);
            if let Some(own) = own_dir {
                k.dir_files(own, &["ff", "xpak"]);
            }
        });
        parts.add(why::CODE, &mut key, |k| {
            for c in CODE {
                k.str(c);
            }
        });
        Some(Self { dir, key, parts, map: map.name.clone(), cap })
    }

    /// The pak, if one was made with this key and is whole.
    pub(crate) fn open(&self) -> Option<Reader> {
        Reader::open(&self.dir.join(map_bake::pictures::FILE), self.key)
    }

    /// Why there is no usable pak ("first load", "game updated").
    pub(crate) fn why(&self) -> String {
        map_bake::why::reason(&self.dir, map_bake::pictures::FILE, &self.parts)
    }

    /// A new pak for this key; its end is noted in the load's log.
    pub(crate) fn writer(&self) -> Option<Writer> {
        Writer::start(&self.dir, self.key, |line| crate::prewarm::note(&line))
    }

    /// The manifest written beside the pak.
    pub(crate) fn manifest(&self, count: usize) -> String {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        format!(
            "{{\n  \"game\": \"t7\",\n  \"map\": {},\n  \"key\": \"{:016x}\",\n  \"format\": {},\n  \"picture_cap\": {},\n  \"pictures\": {count},\n  \"written_unix\": {at},\n  {}\n}}\n",
            map_bake::pictures::json_str(&self.map),
            self.key.value(),
            map_bake::pictures::FORMAT,
            self.cap,
            self.parts.to_json()
        )
    }

    /// The pak's folder (for the log).
    pub(crate) fn dir(&self) -> &std::path::Path {
        &self.dir
    }
}

/// Every picture `map`'s materials name (the warm loads the ones its draws
/// bind, fewer), and the weather's stand-ins, once each.
fn ahead_names(map: &T7Map) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for m in &map.materials {
        names.extend(m.textures.iter().map(|(_, n)| n.clone()));
    }
    if let Some((_, cap)) = map.zone_data() {
        for m in &cap.materials {
            names.extend(m.textures.iter().map(|(_, n)| n.clone()));
        }
    }
    names.extend(crate::weather::DEFAULT_IMAGES.iter().map(|n| (*n).to_owned()));
    names.retain(|n| !n.starts_with('<'));
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));
    names
}

/// The picture size cap a load would bake at now (the quality's, or
/// `T7_IMAGE_MAX_DIM`).
pub fn ahead_cap() -> u32 {
    crate::gbuffer::image_max_dim()
}

/// bo2mp (bake all): bakes the map in `zone` now, without the card or a
/// load, into the same file the first load would write: every picture its
/// materials name, at `cap` (see [`ahead_cap`]). A map whose file is
/// already good is left alone. Shown as the bake's job
/// (`map_bake::status`); cancelling the job throws the half-written file
/// away. One line on how it went.
pub fn bake_ahead(zone: &std::path::Path, cap: u32) -> Result<String, String> {
    let map = T7Map::load(&fastfile_t7::map::MapSources::beside(zone))?;
    bake_ahead_of(&map, cap)
}

/// As [`bake_ahead`], for a map already read (PREPARE ALL MAPS reads each
/// zone once for its pictures and its sun map).
pub fn bake_ahead_of(map: &T7Map, cap: u32) -> Result<String, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let b = Pictures::new(map, cap).ok_or("the bake is off or has nowhere to go")?;
    if b.open().is_some() {
        return Ok(format!("bake: {} already baked", map.name));
    }
    let why = b.why();
    let names = ahead_names(&map);
    let (tx, rx) = std::sync::mpsc::channel();
    let w = Writer::start(&b.dir, b.key, move |line| {
        let _ = tx.send(line);
    })
    .ok_or_else(|| format!("bake: could not start a pictures file in {}", b.dir.display()))?;
    let job = map_bake::status::begin("t7", &map.name, "pictures", names.len() as u64);
    job.set_why(&why);
    let next = AtomicUsize::new(0);
    let kept = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..crate::par::workers() {
            s.spawn(|| {
                while !job.is_canceled() {
                    let Some(n) = names.get(next.fetch_add(1, Ordering::Relaxed)) else { break };
                    // (A picture that does not load is left out: the load
                    // tries it the old way.)
                    if let Ok(l) = map.image(n, cap) {
                        w.push(Picture { name: n.clone(), format: l.format, width: l.width, height: l.height, levels: l.levels, part: l.part.map(|p| p as u64), data: l.data });
                        kept.fetch_add(1, Ordering::Relaxed);
                    }
                    job.add(1);
                }
            });
        }
    });
    let done = !job.is_canceled();
    let k = kept.into_inner();
    w.finish(done, if done { b.manifest(k) } else { "cancelled".into() });
    let line = rx.recv().unwrap_or_else(|_| "bake: the pictures writer stopped".into());
    job.end(done);
    Ok(format!("{line} ({why}; {k} of {} pictures)", names.len()))
}

/// Starts reading the map's `pictures.pak` (if it has one) into the file
/// cache as the load begins, beside the zone read, so the warm finds it in
/// memory (`CODLEGACY_BAKE_READ_AHEAD=0`: off). `zone`: the map's zone file.
pub fn read_ahead(zone: &std::path::Path) {
    let Some(dir) = zone.file_stem().and_then(|s| s.to_str()).and_then(|m| map_bake::map_dir("t7", m)) else { return };
    map_bake::read_ahead(dir.join(map_bake::pictures::FILE), |line| crate::prewarm::note(&line));
}

/// A picture as the card takes it, for the pak.
pub(crate) fn picture(name: &str, l: &ImageLevels) -> Picture {
    Picture { name: name.to_owned(), format: l.format, width: l.width, height: l.height, levels: l.levels, part: l.part.map(|p| p as u64), data: l.data.clone() }
}

/// One picture from the pak: its texture, bytes, and seconds reading and
/// uploading.
pub(crate) type Loaded = (Tex, u64, f32, f32);

/// Every picture of `reader` that `index` (name -> place in the warm's list)
/// wants, read in file order on one thread (packed, so that thread is the
/// disk's) and unpacked and uploaded on the others. The pictures uploaded by
/// place, and the read error if the file stopped reading back (the pictures
/// after it load the old way).
pub(crate) fn upload_all(gpu: &Gpu, mut reader: Reader, index: &HashMap<&str, usize>, names: &[String]) -> (Vec<(usize, Loaded)>, Option<String>) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<(usize, map_bake::pictures::Packed, f32)>(32);
    let rx = Mutex::new(rx);
    // (The card ran out of memory: the rest load the old way, which steps
    // the size down.)
    let full = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        let read = s.spawn(move || -> Option<String> {
            loop {
                let t = std::time::Instant::now();
                match reader.next_packed() {
                    Ok(Some(p)) => {
                        let Some(&i) = index.get(p.name.as_str()) else { continue };
                        if tx.send((i, p, t.elapsed().as_secs_f32())).is_err() {
                            return None;
                        }
                    }
                    Ok(None) => return None,
                    Err(e) => return Some(e.to_string()),
                }
            }
        });
        let uploads: Vec<_> = (0..crate::par::workers())
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let next = match rx.lock() {
                            Ok(r) => r.recv(),
                            Err(_) => break,
                        };
                        let Ok((i, p, read)) = next else { break };
                        if full.load(std::sync::atomic::Ordering::Relaxed) {
                            continue;
                        }
                        let t = std::time::Instant::now();
                        // (A picture that does not unpack loads the old way.)
                        let Ok(p) = p.unpack() else { continue };
                        let l = ImageLevels { format: p.format, width: p.width, height: p.height, levels: p.levels, data: p.data, part: p.part.map(|v| v as usize) };
                        match crate::gbuffer::upload_or_oom(gpu, &names[i], &l) {
                            Ok(tex) => out.push((i, (tex, l.data.len() as u64, read, t.elapsed().as_secs_f32()))),
                            Err(crate::gbuffer::Upload::OutOfMemory(_)) => full.store(true, std::sync::atomic::Ordering::Relaxed),
                            Err(crate::gbuffer::Upload::Failed(_)) => {}
                        }
                    }
                    out
                })
            })
            .collect();
        let err = read.join().ok().flatten();
        let got = uploads.into_iter().filter_map(|h| h.join().ok()).flatten().collect();
        (got, err)
    })
}
