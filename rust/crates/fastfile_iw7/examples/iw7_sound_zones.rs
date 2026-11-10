//! A zone's sound tables: `iw7_sound_zones <zone.ff>` prints each sound
//! bank's zones (name, loop, ambient def, reverb), its ambient defs with
//! their events' aliases and ranges, and its reverbs.
fn main() {
    let path = std::env::args().nth(1).expect("usage: iw7_sound_zones <zone.ff>");
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&path)).expect("open");
    let (w, _) = fastfile_iw7::walk::walk_fitted(&zone);
    for b in &w.cap.sound_banks {
        let t = &b.table;
        println!(
            "bank {} ({}): {} aliases, {} zones, {} ambient defs, {} events, {} elements, {} reverbs",
            b.name,
            b.zone,
            b.aliases.len(),
            t.zones.len(),
            t.ambient_defs.len(),
            t.ambient_events.len(),
            t.ambient_elements.len(),
            t.reverbs.len()
        );
        for z in &t.zones {
            println!("  zone {:?} state {:?}: loop {:?} ambient {:?} (def {}) reverb {:?} (#{}) mix {:?} occlusion {:?}", z.name, z.state, z.ambient_stream, z.ambient, z.ambient_def_index, z.reverb, z.reverb_index, z.mix, z.occlusion);
        }
        for d in &t.ambient_defs {
            let evs: Vec<String> = (d.first_event as usize..(d.first_event + d.event_count) as usize)
                .filter_map(|i| t.ambient_events.get(i))
                .map(|&(el, wt)| match t.ambient_elements.get(el as usize) {
                    Some(e) => format!("{} x{wt} @{:?}", e.alias, e.range),
                    None => format!("element {el}?"),
                })
                .collect();
            println!("  ambient {:?} every {:?}s: {}", d.name, d.time, evs.join(", "));
        }
        for r in &t.reverbs {
            println!("  reverb #{:#x} {:?} dry {} wet {} fade {}", r.id, r.room_type, r.dry, r.wet, r.fade);
        }
    }
}
