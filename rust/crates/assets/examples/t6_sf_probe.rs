//! Lists Black Ops II's Select Fire pairs in a zone: each `au_<gun>_sf`
//! unique's alternate weapon and that weapon's fire type, fire and burst
//! times, alternate raise / drop: `t6_sf_probe <common_mp.ff>`.
use fastfile_t6::layout::WeaponDef as d;
use fastfile_t6::layout::WeaponVariantDef as v;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let c = asset_t6::capture_zone(std::path::Path::new(&args[1])).unwrap();
    for u in c.attachment_uniques.iter().filter(|u| u.name.ends_with("_sf")) {
        let w = c.weapons.iter().find(|w| w.name == u.alt_weapon_name);
        match w {
            Some(w) => println!(
                "{} -> {} fireType {} fire {} burstDelay {} altRaise {} altDrop {}",
                u.name,
                w.name,
                w.def_i32(d::fireType),
                w.def_i32(d::iFireTime),
                w.def_i32(d::iBurstDelayTime),
                w.var_i32(v::iAltRaiseTime),
                w.def_i32(d::iAltDropTime)
            ),
            None => println!("{} -> {} (not in this zone)", u.name, u.alt_weapon_name),
        }
        // The raise / drop slots (0x17 raise .. 0x1D quick drop; 0x1A/0x1B
        // the alternate's) of the unique, the alternate and the gun.
        let gun = u.name.strip_prefix("au_").and_then(|s| s.strip_suffix("_sf")).map(|b| format!("{b}_mp"));
        let gun = gun.and_then(|g| c.weapons.iter().find(|w| w.name == g));
        println!(
            "  unique altRaise {} altDrop {}; gun altRaise {} altDrop {}",
            u.i32_at(fastfile_t6::layout::WeaponAttachmentUnique::iAltRaiseTime),
            u.i32_at(fastfile_t6::layout::WeaponAttachmentUnique::iAltDropTime),
            gun.map_or(-1, |g| g.var_i32(v::iAltRaiseTime)),
            gun.map_or(-1, |g| g.def_i32(d::iAltDropTime))
        );
        for (who, x) in [("unique", Some(&u.xanims)), ("alt", w.map(|w| &w.xanims)), ("gun", gun.map(|w| &w.xanims))] {
            if let Some(x) = x {
                let slots: Vec<String> = (0x17..=0x1D).map(|i| format!("{i:#x}={}", x.get(i).map_or("", |s| s.as_str()))).collect();
                println!("  {who}: {}", slots.join(" "));
                for i in [0x1A, 0x1B] {
                    let name = x.get(i).map_or("", |s| s.as_str());
                    if let Some(a) = c.xanims.iter().find(|a| !name.is_empty() && a.name.eq_ignore_ascii_case(name)) {
                        println!("    {name}: {} frames at {} fps", a.numframes, a.framerate);
                    }
                }
            }
        }
    }
}
