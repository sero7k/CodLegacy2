//! `pictures.pak` written and read back (files under the build's own
//! temporary folder).

use map_bake::Key;
use map_bake::pictures::{FILE, Picture, Reader, Writer};
use std::path::{Path, PathBuf};

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("map_bake").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn pic(i: u32) -> Picture {
    Picture { name: format!("~pic_{i}"), format: 71 + i, width: 4 << i, height: 2 << i, levels: i + 1, part: i.is_multiple_of(2).then_some(u64::from(i)), data: vec![u8::try_from(i).unwrap_or(u8::MAX); 100 * (i as usize + 1)] }
}

fn write(d: &Path, key: Key, keep: bool, pics: &[Picture]) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    let w = Writer::start(d, key, move |l| {
        let _ = tx.send(l);
    })
    .expect("writer");
    for p in pics {
        w.push(p.clone());
    }
    w.finish(keep, "{}".into());
    rx.recv().expect("end line")
}

fn key(v: u64) -> Key {
    let mut k = Key::new();
    k.u64(v);
    k
}

#[test]
fn round_trip_in_order() {
    let d = dir("round_trip");
    let pics: Vec<Picture> = (0..5).map(pic).collect();
    let line = write(&d, key(7), true, &pics);
    assert!(line.contains("5 pictures"), "{line}");
    assert!(d.join("manifest.json").is_file());
    let mut r = Reader::open(&d.join(FILE), key(7)).expect("whole pak");
    assert_eq!(r.count, 5);
    let mut back = Vec::new();
    while let Some(p) = r.next_picture().expect("reads") {
        back.push(p);
    }
    assert_eq!(back, pics);
    // (Packed: these pictures are runs of one byte, far smaller on disk.)
    let raw: u64 = pics.iter().map(|p| p.data.len() as u64).sum();
    assert!(std::fs::metadata(d.join(FILE)).expect("pak").len() < raw, "the file is packed");
    // (A different key is a miss.)
    assert!(Reader::open(&d.join(FILE), key(8)).is_none());
}

#[test]
fn cut_short_is_a_miss() {
    let d = dir("cut_short");
    write(&d, key(1), true, &(0..3).map(pic).collect::<Vec<_>>());
    let f = d.join(FILE);
    let b = std::fs::read(&f).expect("pak");
    std::fs::write(&f, &b[..b.len() - 10]).expect("cut");
    assert!(Reader::open(&f, key(1)).is_none());
}

#[test]
fn not_kept_leaves_nothing() {
    let d = dir("not_kept");
    let line = write(&d, key(1), false, &[pic(0)]);
    assert!(line.contains("not kept"), "{line}");
    assert!(!d.join(FILE).exists());
    assert!(std::fs::read_dir(&d).expect("dir").next().is_none(), "no half-written file left");
}

#[test]
fn read_ahead_reads_the_whole_file() {
    let d = dir("read_ahead");
    write(&d, key(3), true, &(0..4).map(pic).collect::<Vec<_>>());
    let (tx, rx) = std::sync::mpsc::channel();
    map_bake::read_ahead(d.join(FILE), move |l| {
        let _ = tx.send(l);
    });
    let line = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("end line");
    assert!(line.contains("read ahead ("), "{line}");
    // (A missing file: nothing.)
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    map_bake::read_ahead(d.join("missing.pak"), move |l| {
        let _ = tx.send(l);
    });
    assert!(rx.recv_timeout(std::time::Duration::from_millis(200)).is_err());
}
