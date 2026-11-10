//! The bake's "clear cache": only the files the bake makes go.
use map_bake::clear::{Cleared, clear_in};

#[test]
fn clear_takes_only_baked_files() {
    let root = std::env::temp_dir().join(format!("map_bake_clear_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let map = root.join("t7").join("mp_skyjacked");
    let other = root.join("t7").join("mp_other");
    std::fs::create_dir_all(&map).expect("folder");
    std::fs::create_dir_all(&other).expect("folder");
    std::fs::write(map.join("pictures.pak"), [0u8; 100]).expect("write");
    std::fs::write(map.join("manifest.json"), b"{}").expect("write");
    std::fs::write(map.join("pictures.pak.part1234"), [0u8; 7]).expect("write");
    std::fs::write(other.join("pictures.pak"), [0u8; 1]).expect("write");
    std::fs::write(map.join("mips.pak"), [0u8; 3]).expect("write");
    std::fs::write(map.join("mips.idx.part77"), [0u8; 4]).expect("write");
    // (Black Ops II's map pictures, and one half-written.)
    let t6 = root.join("t6").join("mp_nuketown_2020");
    std::fs::create_dir_all(&t6).expect("folder");
    std::fs::write(t6.join("t6pictures.pak"), [0u8; 5]).expect("write");
    std::fs::write(t6.join("t6pictures.pak.part4321"), [0u8; 6]).expect("write");
    // (Not the bake's: stays.)
    std::fs::write(other.join("notes.txt"), b"mine").expect("write");
    std::fs::write(other.join("pictures.pak.partx"), b"x").expect("write");
    std::fs::write(root.join("readme.txt"), b"root").expect("write");

    let mut out = Cleared::default();
    clear_in(&root, &mut out);
    assert_eq!((out.files, out.bytes), (8, 100 + 2 + 7 + 1 + 3 + 4 + 5 + 6));
    assert!(!map.exists(), "an emptied map folder goes");
    assert!(!t6.exists(), "Black Ops II's pictures go");
    assert!(other.join("notes.txt").is_file());
    assert!(other.join("pictures.pak.partx").is_file());
    assert!(root.join("readme.txt").is_file());
    assert_eq!(out.notes.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// A game with a root of its own (`CODLEGACY_BAKE_DIR_T7`) keeps its maps
/// straight under it, and CLEAR CACHE empties that root too.
#[test]
fn a_games_own_root_holds_its_maps_and_clears() {
    let own = std::env::temp_dir().join(format!("map_bake_t7_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&own);
    // (This test is the only one that sets the setting; the others leave it.)
    unsafe { std::env::set_var("CODLEGACY_BAKE_DIR_T7", &own) };
    assert_eq!(map_bake::game_root("t7"), Some(own.clone()));
    assert_eq!(map_bake::game_root("t6"), None);
    assert_eq!(map_bake::map_dir("t7", "mp_stronghold"), Some(own.join("mp_stronghold")));
    let map = own.join("mp_stronghold");
    std::fs::create_dir_all(&map).expect("folder");
    std::fs::write(map.join("pictures.pak"), [0u8; 9]).expect("write");
    std::fs::write(map.join("notes.txt"), b"mine").expect("write");
    let mut out = Cleared::default();
    // (The main root is left alone here: the test only checks the game's own.)
    unsafe { std::env::set_var("CODLEGACY_BAKE_DIR", own.join("none-such")) };
    map_bake::clear::clear(&mut out);
    unsafe { std::env::remove_var("CODLEGACY_BAKE_DIR") };
    unsafe { std::env::remove_var("CODLEGACY_BAKE_DIR_T7") };
    assert_eq!((out.files, out.bytes), (1, 9));
    assert!(map.join("notes.txt").is_file(), "not the bake's: stays");
    let _ = std::fs::remove_dir_all(&own);
}
