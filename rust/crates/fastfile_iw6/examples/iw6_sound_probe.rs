//! bo2mp: a Ghosts zone's sound-side records: string tables (the sound
//! tables), curves with their knots, Doppler presets, occlusion shapes and
//! the alias values past the basics; with a word, the aliases whose list
//! name holds it: `iw6_sound_probe <zone.ff> [word]`.
use fastfile_iw6::asset_type::AssetType;
use std::collections::BTreeMap;
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let word = a.next().map(|w| w.to_ascii_lowercase());
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    println!("walked {}/{} {:?}", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error));
    for t in &cap.string_tables {
        println!("stringtable {} {}x{} first row {:?}", t.name, t.rows, t.columns, (0..t.columns).map(|c| t.cell(0, c)).collect::<Vec<_>>());
    }
    for ty in [AssetType::ReverbPreset, AssetType::DopplerPreset] {
        println!("{ty:?}: {:?}", cap.names(ty));
    }
    for (i, c) in &cap.snd_curves {
        println!("curve #{i} {:?} {} {:?}", cap.others[*i as usize].ty, c.name, c.knots);
    }
    for (i, d) in &cap.doppler_presets {
        println!("doppler #{i} {d:?}");
    }
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let (mut n, mut flags, mut vel, mut wet, mut dop, mut curves, mut spk) = (0, BTreeMap::new(), 0, 0, 0, 0, BTreeMap::new());
    for l in &cap.sounds {
        for al in &l.aliases {
            n += 1;
            *flags.entry(al.flags & 0x7f).or_insert(0) += 1;
            let p = &al.params;
            vel += usize::from(p.velocity_min != 0.0);
            wet += usize::from(p.wet_mix_override != 0.0);
            dop += usize::from(p.allow_doppler);
            curves += usize::from(p.volume_falloff.is_some());
            if let Some(o) = p.occlusion {
                *shapes.entry(format!("{o:?}")).or_insert(0) += 1;
            }
            if let Some(s) = &p.speaker_map {
                *spk.entry(s.clone()).or_insert(0) += 1;
            }
            if let Some(w) = &word
                && l.name.display().to_ascii_lowercase().contains(w.as_str())
            {
                println!(
                    "{} vol {}..{} pitch {}..{} dist {}..{} prob {} flags {:#x} ch {} {p:?}",
                    l.name.display(), al.vol_min, al.vol_max, al.pitch_min, al.pitch_max, al.dist_min, al.dist_max, al.probability, al.flags, (al.flags >> 9) & 0x7f
                );
            }
        }
    }
    println!("aliases {n}: low flag bits {flags:?}; velocityMin set {vel}, wetMixOverride set {wet}, allowDoppler {dop}, volume curve {curves}");
    println!("occlusion shapes {shapes:?}");
    println!("speaker maps {spk:?}");
}
