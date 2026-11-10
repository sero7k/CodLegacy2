//! Ghosts effects as the zone stores them, one text dump per effect (the offline
//! effect harness' data half): `iw6_fx_audit <zone dir> <map> [--effect <part>]...
//! [--paths <file>] [--census] [--quiet]`.
//!
//! The effects dumped are the map's createfx placements, the paths in `--paths`
//! (one `fx/...` or `vfx/...` path a line: the map scripts' literals, from
//! `gsc_iw6_host`'s `iw6_script_census`) and everything those run (runner,
//! impact, death and emitted effects), looked up in the map zone, its patch and
//! the shared zones. Per element: type, flags, lit type, count, delay, life,
//! range, fades, gravity, sort, HDR scalars, atlas, velocity and visual
//! samples, and per material the state bits' blend, the technique set and
//! whether its colour picture reads from the pictures staged (or falls back).
//! `--census` ends with the element types used and which have a drawer.
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use fastfile_iw6::fxview::{OwnedElem, OwnedFx, OwnedVisual};
use fastfile_iw6::image::{ImagePaks, image_pixels};
use fastfile_iw6::shared::{SharedImages, image_key};

fn f(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4).map_or(0.0, |s| f32::from_le_bytes(s.try_into().unwrap()))
}
fn i(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4).map_or(0, |s| i32::from_le_bytes(s.try_into().unwrap()))
}

fn type_name(t: u8) -> &'static str {
    match t {
        0 => "billboard",
        1 => "oriented",
        2 => "tail",
        3 => "trail",
        4 => "flare",
        5 => "particle-sim",
        6 => "type6",
        7 => "type7",
        8 => "cloud",
        9 => "spark-cloud",
        10 => "spark-fountain",
        11 => "model",
        12 => "omni-light",
        13 => "spot-light",
        14 => "sound",
        15 => "decal",
        16 => "runner",
        17 => "vector-field",
        _ => "?",
    }
}

fn blend_name(b: &[u8; 28]) -> String {
    let w2 = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
    let w1 = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    let (s, d, op) = (w2 & 0xf, (w2 >> 4) & 0xf, (w2 >> 8) & 7);
    let kind = match (op, s, d) {
        (0, _, _) => "opaque".to_owned(),
        (_, 2, 2) => "add".to_owned(),
        (_, 5, 6) => "alpha".to_owned(),
        (_, 2, 6) => "premult".to_owned(),
        (_, 2, 1) => "opaque".to_owned(),
        (_, 9, 1) | (_, 1, 3) => "multiply".to_owned(),
        (_, 5, 2) => "alpha-add".to_owned(),
        (op, s, d) => format!("blend op{op} {s}/{d}"),
    };
    let depth = if w1 & 1 != 0 { "zw" } else { "nozw" };
    let test = if w1 & 2 != 0 { "notest".to_owned() } else { format!("test{}", (w1 >> 2) & 3) };
    format!("{kind} {depth} {test} cull{} [w0 {:#x} w1 {:#x} w2 {:#x}]", (u32::from_le_bytes([b[0], b[1], b[2], b[3]]) >> 3) & 3, u32::from_le_bytes([b[0], b[1], b[2], b[3]]), w1, w2)
}

struct Lookup {
    map_fx: HashMap<String, OwnedFx>,
    map_base: HashMap<String, String>,
    shared: SharedImages,
}
impl Lookup {
    fn effect(&self, name: &str) -> Option<OwnedFx> {
        let key = name.trim_start_matches(',').to_ascii_lowercase();
        if let Some(f) = self.map_fx.get(&key) {
            return Some(f.clone());
        }
        let base = key.rsplit('/').next().unwrap_or(&key);
        if let Some(f) = self.map_base.get(base).and_then(|k| self.map_fx.get(k)) {
            return Some(f.clone());
        }
        self.shared.effect(name).cloned()
    }
}

struct Mat {
    techset: String,
    state: Vec<[u8; 28]>,
    sort_key: u8,
    colour: Option<String>,
    constants: Vec<(String, [f32; 4])>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(&args[0]);
    let map = args[1].clone();
    let mut want: Vec<String> = Vec::new();
    let mut paths_file: Option<PathBuf> = None;
    let (mut census, mut quiet) = (false, false);
    let mut k = 2;
    while k < args.len() {
        match args[k].as_str() {
            "--effect" => {
                want.push(args[k + 1].clone());
                k += 1;
            }
            "--paths" => {
                paths_file = Some(args[k + 1].clone().into());
                k += 1;
            }
            "--census" => census = true,
            "--quiet" => quiet = true,
            _ => {}
        }
        k += 1;
    }
    let zone_path = dir.join(format!("{map}.ff"));
    let zone = fastfile_iw6::Zone::open(&zone_path).expect("zone");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let cfx = fastfile_iw6::createfx::read(&cap, &zone.image);

    let mut shared = SharedImages::default();
    for z in fastfile_iw6::shared::lane_zones() {
        let p = dir.join(format!("{z}.ff"));
        if p.is_file() {
            let _ = shared.add_zone(&p);
        }
    }
    // The map's patch zone first (its effects replace the map's).
    let mut map_fx: HashMap<String, OwnedFx> = HashMap::new();
    let mut map_base: HashMap<String, String> = HashMap::new();
    let patch_path = dir.join(format!("patch_{map}.ff"));
    let mut patch_cap = None;
    if patch_path.is_file()
        && let Ok(pz) = fastfile_iw6::Zone::open(&patch_path)
    {
        let (pc, _) = fastfile_iw6::walk::walk_zone(&pz);
        let n = pc.fx.iter().filter(|f| !f.name.starts_with(',') && !f.elems.is_empty()).count();
        eprintln!("patch_{map}: {} effects with elements", n);
        for fr in &pc.fx {
            if fr.name.starts_with(',') || fr.elems.is_empty() {
                continue;
            }
            let key = fr.name.to_ascii_lowercase();
            map_base.entry(key.rsplit('/').next().unwrap_or(&key).to_owned()).or_insert_with(|| key.clone());
            map_fx.insert(key, fastfile_iw6::fxview::own(fr, &pc, &pz.image));
        }
        patch_cap = Some((pc, pz));
    }
    for fr in &cap.fx {
        if fr.name.starts_with(',') || fr.elems.is_empty() {
            continue;
        }
        let key = fr.name.to_ascii_lowercase();
        map_base.entry(key.rsplit('/').next().unwrap_or(&key).to_owned()).or_insert_with(|| key.clone());
        map_fx.entry(key).or_insert_with(|| fastfile_iw6::fxview::own(fr, &cap, &zone.image));
    }
    let lk = Lookup { map_fx, map_base, shared };

    // Materials: the map's (and patch's) by name, else the shared zones'.
    let mut mats: HashMap<String, Mat> = HashMap::new();
    let mut add_mats = |c: &fastfile_iw6::walk::Capture| {
        for m in &c.materials {
            let Some(ts) = m.techset else { continue };
            let key = image_key(&m.name);
            let colour = m
                .textures
                .iter()
                .find(|t| t.semantic == 2 && t.image.is_some())
                .and_then(|t| c.images.get(t.image?.index as usize))
                .map(|im| im.name.clone());
            mats.entry(key).or_insert_with(|| Mat {
                techset: c.name_of(ts).trim_start_matches(',').to_owned(),
                state: m.state_bits.clone(),
                sort_key: m.sort_key,
                colour,
                constants: m.constants.iter().map(|c| (c.name.clone(), c.literal)).collect(),
            });
        }
    };
    if let Some((pc, _)) = &patch_cap {
        add_mats(pc);
    }
    add_mats(&cap);
    let mut paks = ImagePaks::new(&dir);
    let mut picture_cache: HashMap<String, String> = HashMap::new();
    let mut picture = |name: &str, lk: &Lookup, paks: &mut ImagePaks| -> String {
        let key = image_key(name);
        if let Some(r) = picture_cache.get(&key) {
            return r.clone();
        }
        let from_map = cap.images.iter().find(|im| image_key(&im.name) == key && !fastfile_iw6::shared::is_reference(im));
        let r = if let Some(rec) = from_map {
            match image_pixels(rec, &zone.image, paks, 512) {
                Ok(p) => format!("loaded {}x{} fmt{} {}lv (map zone)", p.width, p.height, p.dxgi_format, p.levels),
                Err(e) => format!("FALLBACK ({e})"),
            }
        } else if lk.shared.get(&key).is_some() {
            match lk.shared.pixels(&key, paks, 512) {
                Ok(p) => format!("loaded {}x{} fmt{} {}lv (shared)", p.width, p.height, p.dxgi_format, p.levels),
                Err(e) => format!("FALLBACK ({e})"),
            }
        } else if fastfile_iw6::shared::builtin_pixels(&key).is_some() {
            "builtin".to_owned()
        } else {
            "FALLBACK (in no zone read)".to_owned()
        };
        picture_cache.insert(key, r.clone());
        r
    };

    // The effect set.
    let mut queue: VecDeque<(String, String)> = VecDeque::new(); // (name, why)
    let mut why: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut placed_n: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(c) = &cfx {
        for p in &c.placed {
            let path = c.paths.get(&p.fxid).cloned().unwrap_or_else(|| p.fxid.clone());
            *placed_n.entry(path.clone()).or_default() += 1;
        }
        for (path, n) in &placed_n {
            queue.push_back((path.clone(), format!("createfx x{n}")));
        }
    }
    if let Some(p) = &paths_file {
        for l in std::fs::read_to_string(p).expect("paths").lines() {
            let l = l.trim();
            if l.starts_with("fx/") || l.starts_with("vfx/") {
                queue.push_back((l.to_owned(), "script".to_owned()));
            }
        }
    }
    for w in &want {
        queue.push_back((w.clone(), "asked".to_owned()));
    }
    let mut found: BTreeMap<String, OwnedFx> = BTreeMap::new();
    let mut missing: BTreeMap<String, String> = BTreeMap::new();
    while let Some((name, reason)) = queue.pop_front() {
        let Some(fx) = lk.effect(&name) else {
            missing.entry(name.clone()).or_insert(reason);
            continue;
        };
        let key = fx.name.to_ascii_lowercase();
        why.entry(key.clone()).or_default().push(reason);
        if found.contains_key(&key) {
            continue;
        }
        for el in &fx.elems {
            for n in el
                .visuals
                .iter()
                .filter_map(|v| if let OwnedVisual::Effect(n) = v { Some(n) } else { None })
                .chain([&el.effect_on_impact, &el.effect_on_death, &el.effect_emitted].into_iter().flatten())
            {
                queue.push_back((n.clone(), format!("child of {}", fx.name)));
            }
        }
        found.insert(key, fx);
    }

    let mut types_used: BTreeMap<u8, BTreeSet<String>> = BTreeMap::new();
    let mut fallback_pics: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut techsets: BTreeMap<String, usize> = BTreeMap::new();
    for (key, fx) in &found {
        if !want.is_empty() && !want.iter().any(|w| key.contains(&w.to_ascii_lowercase())) && !quiet {
            continue;
        }
        let h = &fx.header;
        if !quiet {
            println!(
                "== {} [{}] looping elems {} one-shot {} emission {}, loop life {} ms, seed {}",
                fx.name,
                why[key].iter().take(3).cloned().collect::<Vec<_>>().join("; "),
                i(h, 20),
                i(h, 24),
                i(h, 28),
                i(h, 16),
                h.get(0x26).copied().unwrap_or(0) as u32 | (h.get(0x27).copied().unwrap_or(0) as u32) << 8
            );
        }
        for (n, el) in fx.elems.iter().enumerate() {
            let r = fx_iw4::iw6::Iw6ElemRules::read(&el.record);
            types_used.entry(r.elem_type).or_default().insert(fx.name.clone());
            describe(n, el, &r, &mut mats, &mut picture, &lk, &mut paks, &mut fallback_pics, &mut techsets, quiet);
        }
    }
    if !missing.is_empty() {
        println!("-- named but in no zone read ({}):", missing.len());
        for (n, why) in &missing {
            println!("   {n}  ({why})");
        }
    }
    if census {
        println!("-- {} effects in the set", found.len());
        println!("-- element types used (count of effects):");
        for (t, set) in &types_used {
            let draw = if fx_iw4::iw6::drawn_type(*t) { "drawn by the exe" } else { "never drawn by the exe" };
            println!("   type {t:2} {:15} {:3} effects  ({draw})", type_name(*t), set.len());
        }
        println!("-- pictures that fall back ({}):", fallback_pics.len());
        for (p, users) in &fallback_pics {
            println!("   {p}  <- {}", users.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
        }
        println!("-- technique sets of the effects' materials:");
        for (t, n) in &techsets {
            println!("   {n:3} {t}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn describe(
    n: usize,
    el: &OwnedElem,
    r: &fx_iw4::iw6::Iw6ElemRules,
    mats: &mut HashMap<String, Mat>,
    picture: &mut dyn FnMut(&str, &Lookup, &mut ImagePaks) -> String,
    lk: &Lookup,
    paks: &mut ImagePaks,
    fallback_pics: &mut BTreeMap<String, BTreeSet<String>>,
    techsets: &mut BTreeMap<String, usize>,
    quiet: bool,
) {
    let rec = &el.record;
    let frame = fx_iw4::iw6::run_frame(r.flags);
    let off = fx_iw4::iw6::spawn_offset(r.flags);
    let atlas = (rec[172], rec[173], rec[174], rec[175], rec[176], rec[177], rec[178]);
    let mut lines = Vec::new();
    lines.push(format!(
        "  [{n}] {} (type {}) flags {:#010x} flags2 {:#x} run {frame:?} offset {off:?} lit {} sort {} lightingFrac {}",
        type_name(r.elem_type),
        r.elem_type,
        r.flags,
        r.flags2,
        r.lit_type,
        r.sort_order,
        r.lighting_frac
    ));
    lines.push(format!(
        "      spawn a {} b {} (as one-shot count {}+rand{}; as loop interval {} ms count {}) delay {:?} life {:?} range {:?}/{:?}",
        r.spawn_a, r.spawn_b, r.spawn_a, r.spawn_b, r.spawn_a, r.spawn_b, r.delay, r.life, f(rec, 16), f(rec, 20)
    ));
    lines.push(format!(
        "      fadeIn(far) {:?} fadeOut(near) {:?} gravity {:?} reflect {:?} spawnOffset r{:?} h{:?} initRot {:?} cull-radius {}",
        r.fade_in, r.fade_out, r.gravity, [f(rec, 164), f(rec, 168)], [f(rec, 84), f(rec, 88)], [f(rec, 92), f(rec, 96)], r.initial_rotation, f(rec, 40)
    ));
    lines.push(format!(
        "      hdr: unlitScalar {} litScalar {} blend {} maxColourChange/s {}; atlas beh{} idx{} fps{} loops{} colbits{} rowbits{} entries{}",
        r.unlit_hdr_scalar, r.lit_hdr_scalar, r.lit_unlit_blend, r.lit_max_color_change_per_sec, atlas.0, atlas.1, atlas.2, atlas.3, atlas.4, atlas.5, atlas.6
    ));
    // Velocity samples: 96 bytes each (local 48, world 48).
    for s in 0..=(usize::from(r.vel_intervals)) {
        let b = s * 96;
        if el.vel_samples.len() < b + 96 {
            break;
        }
        let v = |o: usize| [f(&el.vel_samples, b + o), f(&el.vel_samples, b + o + 4), f(&el.vel_samples, b + o + 8)];
        if s == 0 || s == usize::from(r.vel_intervals) || r.vel_intervals <= 2 {
            lines.push(format!(
                "      vel[{s}] local v {:.4?}+{:.4?} d {:.4?}+{:.4?} | world v {:.4?}+{:.4?} d {:.4?}+{:.4?}",
                v(0), v(12), v(24), v(36), v(48), v(60), v(72), v(84)
            ));
        }
    }
    for s in 0..=(usize::from(r.vis_intervals)) {
        let b = s * 80;
        if el.vis_samples.len() < b + 80 {
            break;
        }
        if s == 0 || s == usize::from(r.vis_intervals) || r.vis_intervals <= 3 {
            let v = &el.vis_samples[b..b + 80];
            lines.push(format!(
                "      vis[{s}] rgba {:.3?}~{:.3?} unlitHDR {:.3}~{:.3} rotDelta {:.5} rotTotal {:.5} size0 {:.2}~{:.2} size1 {:.2}~{:.2} scale {:.2}~{:.2}",
                [f(v, 0), f(v, 4), f(v, 8), f(v, 12)],
                [f(v, 40), f(v, 44), f(v, 48), f(v, 52)],
                f(v, 16),
                f(v, 56),
                f(v, 20),
                f(v, 24),
                f(v, 28),
                f(v, 68),
                f(v, 32),
                f(v, 72),
                f(v, 36),
                f(v, 76),
            ));
        }
    }
    for v in &el.visuals {
        match v {
            OwnedVisual::Material(name) => {
                let key = image_key(name);
                let mut s = format!("      material {name}");
                if let Some(m) = mats.get(&key) {
                    *techsets.entry(m.techset.clone()).or_default() += 1;
                    let blends: BTreeSet<String> = m.state.iter().map(blend_name).collect();
                    s += &format!(" techset {} sortKey {} state [{}]", m.techset, m.sort_key, blends.into_iter().take(3).collect::<Vec<_>>().join(" | "));
                    if !m.constants.is_empty() && !quiet {
                        let c: Vec<String> = m.constants.iter().take(4).map(|(n, v)| format!("{n}={v:.2?}")).collect();
                        s += &format!(" consts {}", c.join(" "));
                    }
                    match &m.colour {
                        Some(pic) => {
                            let st = picture(pic, lk, paks);
                            if st.starts_with("FALLBACK") {
                                fallback_pics.entry(pic.trim_start_matches(',').to_owned()).or_default().insert(name.trim_start_matches(',').to_owned());
                            }
                            s += &format!("; picture {} -> {st}", pic.trim_start_matches(','));
                        }
                        None => s += "; no colour picture",
                    }
                } else if let Some(m) = lk.shared.material(name) {
                    *techsets.entry(m.techset.clone()).or_default() += 1;
                    s += &format!(" techset {} sortKey {} (shared)", m.techset, m.sort_key);
                    if let Some(t) = m.textures.iter().find(|t| t.2 == 2) {
                        let st = picture(&t.3, lk, paks);
                        if st.starts_with("FALLBACK") {
                            fallback_pics.entry(t.3.trim_start_matches(',').to_owned()).or_default().insert(name.trim_start_matches(',').to_owned());
                        }
                        s += &format!("; picture {} -> {st}", t.3.trim_start_matches(','));
                    }
                } else {
                    s += " (material in no zone read)";
                }
                lines.push(s);
            }
            OwnedVisual::Effect(e) => lines.push(format!("      runs effect {e}")),
            OwnedVisual::Model(m) => lines.push(format!("      model {m}")),
            OwnedVisual::Sound(s) => lines.push(format!("      sound {s}")),
            OwnedVisual::Mark(p) => lines.push(format!("      decal materials {p:?}")),
            _ => {}
        }
    }
    for (what, e) in [("on impact", &el.effect_on_impact), ("on death", &el.effect_on_death), ("emitted", &el.effect_emitted)] {
        if let Some(e) = e {
            lines.push(format!("      {what}: {e}"));
        }
    }
    if let Some((tr, v, ix)) = &el.trail {
        lines.push(format!("      trail record {} B, {} verts, {} indices", tr.len(), v.len() / 24, ix.len() / 2));
    }
    if !el.extended.is_empty() {
        lines.push(format!("      extended record {} B: {:?}", el.extended.len(), &el.extended[..el.extended.len().min(64)].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap_or([0; 4]))).collect::<Vec<_>>()));
    }
    if !quiet {
        for l in lines {
            println!("{l}");
        }
    } else {
        // (still resolves pictures and tallies)
        let _ = lines;
    }
    let _ = Path::new("");
}
