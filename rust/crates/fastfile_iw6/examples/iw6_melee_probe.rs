//! Lists the notetracks of Ghosts' knife melee clips, and the melee sound
//! fields and notetrack sound maps of weapons whose name has a word:
//! `iw6_melee_probe <zone.ff> [weapon word]` (default `knife`).
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let word = a.next().unwrap_or_else(|| "knife".into()).to_ascii_lowercase();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    println!("walked {}/{} {:?}", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error));
    let s = |i: u32| zone.assets.script_strings.get(i as usize).cloned().flatten().unwrap_or_default();
    for x in cap.xanims.iter().filter(|x| x.name.to_ascii_lowercase().contains("melee")) {
        let notes: Vec<String> = x.notify.iter().map(|&(i, t)| format!("{}@{t:.3}", s(i))).collect();
        println!("xanim {} frames {} rate {} notes {notes:?}", x.name, x.numframes, x.framerate);
    }
    for w in cap.weapons.iter().filter(|w| w.name.to_ascii_lowercase().contains(&word)) {
        println!("weapon {}", w.name);
        for (off, name) in &w.sounds {
            println!("   sound @{off}: {name}");
        }
        let keys: Vec<String> = w.notetrack_sound_keys.iter().map(|&i| s(i)).collect();
        let vals: Vec<String> = w.notetrack_sound_values.iter().map(|&i| s(i)).collect();
        println!("   notetrack sounds: {:?}", keys.iter().zip(vals.iter()).collect::<Vec<_>>());
    }
}
