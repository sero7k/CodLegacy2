//! How a gun's parts fit: `jup_gunfit [--save <dir>] <receiver> [<part>[=<tag>]...]`,
//! each a part zone (`.ff`, which needs the install's Oodle DLL beside its
//! `zone` folder or `JUP_OODLE_DLL`, or a decompressed `.zone`). Prints
//! each part's bones by name (hash where unknown), its surfaces' bounds,
//! rigid-range bones and materials, then the gun put together as the guest
//! guns are (`gun::assemble`): where each part landed and the grip tag.
//! `--save` writes each zone decompressed (`<name>.zone`) to `<dir>`.
use std::path::Path;

use fastfile_jup::gun::{self, PartModel};

const NAMES: &[&str] = &[
    "j_gun", "j_gun1", "tag_weapon", "tag_weapon_right", "tag_weapon_left", "tag_view", "tag_origin",
    "tag_flash", "tag_flash_silenced", "tag_brass", "tag_barrel_attach", "tag_mag_attach",
    "tag_mag2_attach", "tag_pistolgrip_attach", "tag_stock_attach", "tag_guard_attach",
    "tag_bolt_attach", "tag_scope_attach", "tag_scope", "tag_sight_on", "tag_sight_off",
    "tag_reticle", "tag_reticle_on", "tag_lens", "tag_eye", "tag_ads", "tag_ironsight",
    "tag_muzzle_attach", "tag_underbarrel_attach", "tag_grip_attach", "tag_laser_attach",
    "tag_rail_attach", "tag_trigger_attach", "tag_receiver_attach", "tag_optic_attach",
    "tag_scope_mount", "tag_ammo_attach", "tag_bolt", "tag_magnifier", "tag_stock",
    "j_bolt", "j_mag1", "j_mag2", "j_trigger", "j_hammer", "j_slide", "j_safety", "j_charge",
    "j_chargehandle", "j_bolt_handle", "j_bipod", "j_cover", "j_scope", "j_lens",
    "tag_knife_attach", "tag_clip", "tag_shell", "tag_reflector_arm_le", "tag_ik_hand_le",
    "tag_ik_hand_ri", "tag_ik_loc_le", "tag_ik_loc_ri", "tag_cambone",
];

fn name(h: u32) -> String {
    NAMES
        .iter()
        .find(|n| gun::bone_hash(n) == h)
        .map_or_else(|| format!("#{h:08x}"), |n| (*n).to_owned())
}

fn r(v: [f32; 3]) -> String {
    format!("({:.2}, {:.2}, {:.2})", v[0], v[1], v[2])
}

fn read(path: &Path, save: Option<&Path>) -> Vec<u8> {
    let bytes = if path.extension().is_some_and(|e| e == "zone") {
        std::fs::read(path).expect("zone")
    } else {
        let install = path.parent().and_then(Path::parent).unwrap_or(Path::new("."));
        let oodle = fastfile_jup::oodle::load(install).expect("oodle");
        fastfile_jup::read_zone(path, oodle.as_ref()).expect("ff").1
    };
    if let (Some(dir), Some(stem)) = (save, path.file_stem()) {
        let out = dir.join(stem).with_extension("zone");
        std::fs::write(&out, &bytes).expect("save");
        println!("saved {}", out.display());
    }
    bytes
}

fn show(label: &str, m: &PartModel, rec: &fastfile_jup::surfs::SurfsRec) {
    println!("{label}: {} ({} bones, {} surfaces)", m.name, m.bones.len(), m.surfaces.len());
    for (i, b) in m.bones.iter().enumerate() {
        println!("  bone {i:2} {:<24} at {} rot {:?}", name(b.name), r(b.pos), b.rot.map(|q| (q * 1000.0).round() / 1000.0));
    }
    for (i, (s, sr)) in m.surfaces.iter().zip(&rec.surfaces).enumerate() {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &s.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let ranges: Vec<String> = sr
            .ranges
            .iter()
            .map(|g| {
                let bone = m.bones.get(usize::from(g.bone)).map_or_else(|| format!("bone {}", g.bone), |b| name(b.name));
                format!("{bone} x{}", g.vert_count)
            })
            .collect();
        println!(
            "  surface {i:2} {:5} verts flags {:04x} {} .. {} material {:?} ranges [{}] skin {:?}",
            s.positions.len(),
            sr.flags,
            r(lo),
            r(hi),
            s.material.map(|h| format!("{h:016x}")),
            ranges.join(", "),
            sr.blend_counts
        );
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let save = match args.iter().position(|a| a == "--save") {
        Some(i) if i + 1 < args.len() => {
            let d = args.remove(i + 1);
            args.remove(i);
            std::fs::create_dir_all(&d).expect("save dir");
            Some(std::path::PathBuf::from(d))
        }
        _ => None,
    };
    let Some((first, rest)) = args.split_first() else {
        eprintln!("jup_gunfit [--save <dir>] <receiver .ff|.zone> [<part>[=<tag>]...]");
        std::process::exit(2);
    };
    let (receiver, rrec) = gun::read_model(&read(Path::new(first), save.as_deref())).expect("receiver");
    show("receiver", &receiver, &rrec);
    let mut parts = Vec::new();
    for a in rest {
        let (path, tag) = a.split_once('=').unwrap_or((a.as_str(), ""));
        let (m, rec) = gun::read_model(&read(Path::new(path), save.as_deref())).expect("part");
        show(&format!("part on {}", if tag.is_empty() { "?" } else { tag }), &m, &rec);
        parts.push((m, tag.to_owned()));
    }
    let refs: Vec<(&PartModel, &str)> = parts.iter().map(|(m, t)| (m, t.as_str())).collect();
    let g = gun::assemble(&receiver, &refs);
    println!("assembled: {} surfaces, missing {:?}", g.surfaces.len(), g.missing);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for (i, s) in g.surfaces.iter().enumerate() {
        let (mut slo, mut shi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &s.positions {
            for k in 0..3 {
                slo[k] = slo[k].min(p[k]);
                shi[k] = shi[k].max(p[k]);
            }
        }
        for k in 0..3 {
            lo[k] = lo[k].min(slo[k]);
            hi[k] = hi[k].max(shi[k]);
        }
        println!("  surface {i:2} {} .. {}", r(slo), r(shi));
    }
    println!("  gun {} .. {}", r(lo), r(hi));
    for t in ["tag_pistolgrip_attach", "tag_stock_attach", "tag_scope", "tag_flash", "j_gun", "tag_weapon"] {
        if let Some(b) = g.bone(t) {
            println!("  {t:<24} at {}", r(b.pos));
        }
    }
}
