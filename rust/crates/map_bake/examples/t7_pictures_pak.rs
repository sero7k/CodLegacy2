//! bo2mp (map bake timing): `t7_pictures_pak <map zone.ff> <out folder>`
//! reads every picture a Black Ops III map's materials name the old way
//! (the packages, decoded, on 12 threads: the warm's work without the
//! card), writes them to `<out folder>\pictures.pak` as the game's bake
//! does, reads the pak back start to end, and reads it once more past
//! the file cache (what a cold load from the hard drive costs). Prints
//! each time and checks the pak gives back the same bytes.
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use map_bake::Key;
use map_bake::pictures::{FILE, Picture, Reader, Writer};

fn main() {
    let mut args = std::env::args().skip(1);
    let zone = std::path::PathBuf::from(args.next().expect("usage: t7_pictures_pak <map zone.ff> <out folder>"));
    let out = std::path::PathBuf::from(args.next().expect("usage: t7_pictures_pak <map zone.ff> <out folder>"));
    let t = std::time::Instant::now();
    let m = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&zone)).expect("map");
    println!("map loaded in {:.1}s", t.elapsed().as_secs_f32());
    let mut names: Vec<String> = Vec::new();
    for mm in &m.materials {
        names.extend(mm.textures.iter().map(|(_, n)| n.clone()));
    }
    if let Some((_, cap)) = m.zone_data() {
        for mm in &cap.materials {
            names.extend(mm.textures.iter().map(|(_, n)| n.clone()));
        }
    }
    names.retain(|n| !n.starts_with('<'));
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));
    println!("{} pictures", names.len());

    // The old way, writing the pak as each finishes.
    let mut key = Key::new();
    key.str("t7_pictures_pak example").file(&zone);
    let (tx, rx) = std::sync::mpsc::channel();
    let w = Writer::start(&out, key, move |l| {
        let _ = tx.send(l);
    })
    .expect("writer");
    let next = AtomicUsize::new(0);
    let sums = Mutex::new(Vec::new());
    let t1 = std::time::Instant::now();
    std::thread::scope(|s| {
        for _ in 0..12 {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(n) = names.get(i) else { break };
                    if let Ok(l) = m.image(n, 1024) {
                        if let Ok(mut v) = sums.lock() {
                            v.push((n.clone(), fnv(&l.data)));
                        }
                        w.push(Picture { name: n.clone(), format: l.format, width: l.width, height: l.height, levels: l.levels, part: l.part.map(|p| p as u64), data: l.data });
                    }
                }
            });
        }
    });
    let old = t1.elapsed().as_secs_f32();
    let mut sums = sums.into_inner().unwrap_or_else(|p| p.into_inner());
    println!("old way: {} pictures read and decoded in {old:.1}s on 12 threads", sums.len());
    w.finish(true, "{}".into());
    println!("{}", rx.recv().expect("writer end"));

    // Read back start to end (file cache warm: just written).
    let t2 = std::time::Instant::now();
    let mut r = Reader::open(&out.join(FILE), key).expect("whole pak");
    let mut back = Vec::new();
    while let Some(p) = r.next_picture().expect("reads back") {
        back.push((p.name, fnv(&p.data)));
    }
    println!("pak read back start to end in {:.2}s ({} pictures, {:.0} MB)", t2.elapsed().as_secs_f32(), back.len(), r.bytes as f64 / 1048576.0);
    sums.sort();
    back.sort();
    println!("same bytes as the old way: {}", if sums == back { "yes" } else { "NO" });
    cold_read(&out.join(FILE));
}

/// The whole file read past the file cache in 8 MiB pieces (Windows), as a
/// cold load from the disk would.
#[cfg(windows)]
fn cold_read(path: &std::path::Path) {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    const NO_BUFFERING: u32 = 0x2000_0000;
    const PIECE: usize = 8 << 20;
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(NO_BUFFERING).open(path) else {
        println!("cold read: could not open past the cache");
        return;
    };
    // (Reads past the cache need a buffer aligned to the disk's sectors.)
    let mut buf = vec![0u8; PIECE + 4096];
    let at = buf.as_ptr().align_offset(4096);
    let piece = &mut buf[at..at + PIECE];
    let t = std::time::Instant::now();
    let mut total = 0u64;
    loop {
        match f.read(piece) {
            Ok(0) => break,
            Ok(n) => total += n as u64,
            Err(e) => {
                println!("cold read: {e}");
                return;
            }
        }
    }
    let s = t.elapsed().as_secs_f32();
    println!("pak read past the file cache in {s:.2}s ({:.0} MB, {:.0} MB/s)", total as f64 / 1048576.0, total as f64 / 1048576.0 / f64::from(s.max(1e-3)));
}

#[cfg(not(windows))]
fn cold_read(_: &std::path::Path) {}

fn fnv(b: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for x in b {
        h ^= u64::from(*x);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}
