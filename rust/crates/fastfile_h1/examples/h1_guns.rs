//! Lists every weapon in a zone: internal and display name, type and class,
//! view, hand and world models, animations, attachments, sounds, and where
//! its models' surfaces and its images are (in the zone, the zone's own pak,
//! a retail `imagefile<N>.pak`, or only a placeholder whose data another zone
//! loads).
//!
//! `cargo run -p fastfile_h1 --release --example h1_guns -- [--strings
//! <eng_zone.ff>]... [--pixels] [-v] <zone.ff>...`
//!
//! `--strings` adds the localized strings of another zone (display names are
//! keys such as `WEAPON_AK47` in retail; Horizon MW writes the text itself).
//! `--pixels` reads every streamed image a gun names from its pak
//! (`H1_IMAGEFILES` = the retail `zone/` folder, else beside the zone).
//! `--table` adds a markdown table at the end: weapon, display name, class,
//! view model, zone, and its images by place (inline / own pak / retail
//! pak).
//! `-v` names the images and model placeholders whose data is outside the
//! zone.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use fastfile_h1::image::{ImagePaks, ZONE_PAK_INDEX, image_pixels};
use fastfile_h1::layout::WeaponDef as D;
use fastfile_h1::walk::{AssetId, Capture, walk_zone};

const WEAP_TYPES: [&str; 6] = [
    "none",
    "bullet",
    "grenade",
    "projectile",
    "riotshield",
    "energy",
];
const WEAP_CLASSES: [&str; 15] = [
    "rifle",
    "sniper",
    "mg",
    "smg",
    "spread",
    "pistol",
    "grenade",
    "rocketlauncher",
    "turret",
    "throwingknife",
    "non_player",
    "item",
    "cone",
    "beam",
    "shield",
];
const INVENTORY: [&str; 6] = [
    "primary",
    "offhand",
    "item",
    "altmode",
    "exclusive",
    "scavenger",
];
const FIRE_TYPES: [&str; 7] = [
    "auto", "single", "burst2", "burst3", "burst4", "burst5", "double",
];
const ATT_TYPES: [&str; 4] = ["scope", "underbarrel", "siderail", "other"];

fn label(names: &[&str], v: i32) -> String {
    usize::try_from(v)
        .ok()
        .and_then(|i| names.get(i))
        .map_or_else(|| v.to_string(), |s| (*s).to_owned())
}

/// A name the walk read in place of an asset whose data is in another
/// zone: retail zones write these with a leading `,`.
fn is_placeholder(name: &str) -> bool {
    name.starts_with(',')
}

/// The models drawn for `id`: itself, or its composite parts (AW and H1
/// build some guns from part models: base, sight, ...).
fn drawn_models(cap: &Capture, id: AssetId) -> Vec<AssetId> {
    let m = &cap.xmodels[id.index as usize];
    if m.numsurfs == 0 && !m.composite.is_empty() {
        m.composite.iter().flatten().copied().collect()
    } else {
        vec![id]
    }
}

/// Where a model's surfaces are (a composite's: all its parts').
fn model_state(cap: &Capture, id: AssetId) -> &'static str {
    let parts = drawn_models(cap, id);
    if parts != [id] {
        let states: BTreeSet<&str> = parts.iter().map(|&p| model_state(cap, p)).collect();
        return match states.into_iter().collect::<Vec<_>>()[..] {
            ["inline"] => "inline (composite)",
            [] => "no surfaces",
            _ => "partly (composite)",
        };
    }
    let m = &cap.xmodels[id.index as usize];
    if is_placeholder(&m.name) {
        return "elsewhere";
    }
    let mut any = false;
    for l in m.lods.iter().filter(|l| l.numsurfs > 0) {
        let Some(s) = l.surfs else { return "missing" };
        let ms = &cap.model_surfs[s.index as usize];
        if ms.surfs.is_empty()
            || ms
                .surfs
                .iter()
                .any(|s| s.verts.is_none() || s.tris.is_none())
        {
            return "missing";
        }
        any = true;
    }
    if any { "inline" } else { "no surfaces" }
}

#[derive(Default)]
struct Tally(BTreeMap<String, usize>);

impl Tally {
    fn add(&mut self, k: impl Into<String>) {
        *self.0.entry(k.into()).or_default() += 1;
    }
    fn text(&self) -> String {
        self.0
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Where an image's pixels are.
fn image_place(cap: &Capture, id: AssetId) -> String {
    let i = &cap.images[id.index as usize];
    if is_placeholder(&i.name) {
        return "elsewhere".into();
    }
    if i.streamed {
        return match i.stream_files.iter().flatten().next().map(|s| s.file_index) {
            Some(ZONE_PAK_INDEX) => "own pak".into(),
            Some(n) => format!("imagefile{n}"),
            None => "streamed, no range".into(),
        };
    }
    if i.pixels.is_some() {
        "inline".into()
    } else {
        "engine".into()
    }
}

fn main() {
    let mut pixels = false;
    let mut verbose = false;
    let mut table = false;
    let mut rows: Vec<String> = Vec::new();
    let mut string_zones = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--pixels" {
            pixels = true;
        } else if a == "--table" {
            table = true;
        } else if a == "-v" {
            verbose = true;
        } else if a == "--strings" {
            string_zones.extend(args.next().map(PathBuf::from));
        } else {
            paths.push(a.into());
        }
    }
    if paths.is_empty() {
        eprintln!("usage: h1_guns [--strings <eng_zone.ff>]... [--pixels] <zone.ff>...");
        std::process::exit(2);
    }
    let mut strings: HashMap<String, String> = HashMap::new();
    for p in &string_zones {
        match fastfile_h1::Zone::open(p) {
            Ok(z) => strings.extend(walk_zone(&z).0.localize),
            Err(e) => eprintln!("{}: {e}", p.display()),
        }
    }
    let retail = std::env::var_os("H1_IMAGEFILES").map(PathBuf::from);
    for path in paths {
        let zone = match fastfile_h1::Zone::open(&path) {
            Ok(z) => z,
            Err(e) => {
                println!("{}: {e}", path.display());
                continue;
            }
        };
        let (cap, stop) = walk_zone(&zone);
        // (The zone's own pak is found beside the real file, not a link.)
        let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let mut paks = ImagePaks::for_zone(&real, retail.as_deref());
        println!(
            "{}: {} weapons, {} attachments, {} xanims, {} models, {} images{}",
            path.display(),
            cap.weapons.len(),
            cap.attachments.len(),
            cap.xanims.len(),
            cap.xmodels.len(),
            cap.images.len(),
            stop.map_or(String::new(), |s| format!(
                " (walk stopped at asset {}: {})",
                s.asset_index, s.error
            ))
        );
        let name = |id: Option<AssetId>| id.map_or("-".to_owned(), |i| cap.name_of(i).to_owned());
        for g in &cap.weapons {
            let d = g.def.get(&zone.image);
            let int = |o: usize| i32::from_le_bytes(d[o..o + 4].try_into().unwrap());
            let display = match strings
                .get(&g.display_name)
                .or(cap.localize.get(&g.display_name))
            {
                Some(t) => format!("{t} ({})", g.display_name),
                None => g.display_name.clone(),
            };
            println!(
                "\n{} \"{}\": {} {}, {} {}, clip {}, damage {}",
                g.name,
                display,
                label(&WEAP_TYPES, int(D::weapType)),
                label(&WEAP_CLASSES, int(D::weapClass)),
                label(&INVENTORY, int(D::inventoryType)),
                label(&FIRE_TYPES, int(D::fireType)),
                int(D::clipSize),
                int(D::damage),
            );
            // Models: view, hand, world, then every attachment's.
            let mut models: Vec<(String, AssetId)> = Vec::new();
            for (k, m) in [
                ("view", g.gun_models.first()),
                ("world", g.world_models.first()),
            ] {
                if let Some(&Some(id)) = m {
                    models.push((k.to_owned(), id));
                }
            }
            if let Some(id) = g.hand_model {
                models.push(("hands".to_owned(), id));
            }
            let state = |id: AssetId| format!("{} [{}]", cap.name_of(id), model_state(&cap, id));
            println!(
                "  models: view {} | world {} | hands {}",
                g.gun_models
                    .first()
                    .copied()
                    .flatten()
                    .map_or("-".into(), state),
                g.world_models
                    .first()
                    .copied()
                    .flatten()
                    .map_or("-".into(), state),
                g.hand_model.map_or("-".into(), state),
            );
            // Animations: the normal set (right/left-handed sets are usually
            // the same assets).
            let anims: HashSet<AssetId> = g.anims.iter().flatten().copied().collect();
            let extra: HashSet<AssetId> = g
                .anims_right
                .iter()
                .chain(&g.anims_left)
                .flatten()
                .copied()
                .filter(|a| !anims.contains(a))
                .collect();
            let mut anim_place = Tally::default();
            for &a in anims.iter().chain(&extra) {
                let x = &cap.xanims[a.index as usize];
                anim_place.add(if is_placeholder(&x.name) {
                    "elsewhere"
                } else if x.numframes > 0 || x.data_byte.is_some() || x.data_short.is_some() {
                    "inline"
                } else {
                    "0 frames"
                });
            }
            println!(
                "  anims: {} ({} more right/left-handed, {} overrides): {}; idle {}",
                anims.len(),
                extra.len(),
                g.anim_overrides.len(),
                anim_place.text(),
                name(g.anims.get(1).copied().flatten()),
            );
            // Attachments.
            let atts: Vec<String> = g
                .attachments
                .iter()
                .flatten()
                .map(|&a| {
                    let r = &cap.attachments[a.index as usize];
                    for (k, m) in r
                        .view_models
                        .iter()
                        .chain(&r.world_models)
                        .flatten()
                        .enumerate()
                    {
                        models.push((format!("{}#{k}", r.name), *m));
                    }
                    format!("{} ({})", r.name, label(&ATT_TYPES, r.ty))
                })
                .collect();
            println!(
                "  attachments: {}",
                if atts.is_empty() {
                    "-".into()
                } else {
                    atts.join(", ")
                }
            );
            // Surfaces and images of every model named above.
            let mut surf = Tally::default();
            let mut img = Tally::default();
            let mut seen_img = HashSet::new();
            let (mut read_ok, mut read_bad) = (0usize, Tally::default());
            let mut outside = BTreeSet::new();
            for (_, m) in &models {
                surf.add(model_state(&cap, *m));
                let mats: Vec<AssetId> = drawn_models(&cap, *m)
                    .into_iter()
                    .flat_map(|p| {
                        cap.xmodels[p.index as usize]
                            .materials
                            .iter()
                            .flatten()
                            .copied()
                    })
                    .collect();
                for mat in &mats {
                    if is_placeholder(cap.name_of(*mat)) {
                        img.add("material elsewhere");
                        continue;
                    }
                    for t in &cap.materials[mat.index as usize].textures {
                        let Some(i) = t.image else { continue };
                        if !seen_img.insert(i) {
                            continue;
                        }
                        let place = image_place(&cap, i);
                        if place != "inline" {
                            outside.insert(format!("{} [{place}]", cap.name_of(i)));
                        }
                        if pixels && cap.images[i.index as usize].streamed {
                            match image_pixels(
                                &cap.images[i.index as usize],
                                &zone.image,
                                &mut paks,
                                1 << 14,
                            ) {
                                Ok(_) => read_ok += 1,
                                Err(_) => read_bad.add(place.clone()),
                            }
                        }
                        img.add(place);
                    }
                }
            }
            println!(
                "  model surfaces ({} models): {}",
                models.len(),
                surf.text()
            );
            println!(
                "  images ({}): {}",
                seen_img.len(),
                if img.0.is_empty() {
                    "-".into()
                } else {
                    img.text()
                }
            );
            if table {
                let zone_name = path
                    .file_name()
                    .map_or(String::new(), |n| n.to_string_lossy().into_owned());
                let (mut inl, mut own, mut retail) = (0, 0, BTreeMap::<String, usize>::new());
                for (k, n) in &img.0 {
                    match k.as_str() {
                        "inline" => inl += n,
                        "own pak" => own += n,
                        k if k.starts_with("imagefile") => {
                            *retail.entry(k.to_owned()).or_default() += n
                        }
                        _ => {}
                    }
                }
                let retail = if retail.is_empty() {
                    "-".to_owned()
                } else {
                    retail
                        .iter()
                        .map(|(k, n)| format!("{n} ({k})"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                rows.push(format!(
                    "| {} | {} | {} | {} | {} | {} / {} / {} |",
                    g.name,
                    display,
                    label(&WEAP_CLASSES, int(D::weapClass)),
                    name(g.gun_models.first().copied().flatten()),
                    zone_name,
                    inl,
                    own,
                    retail
                ));
            }
            if verbose && !outside.is_empty() {
                println!("    {}", outside.into_iter().collect::<Vec<_>>().join(", "));
            }
            if pixels {
                println!(
                    "  streamed pixels read {read_ok}, not read: {}",
                    read_bad.text()
                );
            }
            // Sounds: aliases and where their samples are.
            let mut snd = Tally::default();
            let mut lists = BTreeSet::new();
            for (_, alias) in &g.sounds {
                if !lists.insert(alias.clone()) {
                    continue;
                }
                let Some(list) = cap.sounds.iter().find(|s| &s.name == alias) else {
                    snd.add("list elsewhere");
                    continue;
                };
                for a in &list.aliases {
                    let f = a.file.and_then(|v| cap.sound_files.get(&v));
                    let l = f
                        .and_then(|f| f.loaded)
                        .map(|id| &cap.loaded[id.index as usize]);
                    snd.add(match (f, l) {
                        (_, Some(l)) if l.data.is_some() && l.stream.is_none() => {
                            "inline".to_owned()
                        }
                        (_, Some(l)) => match l.stream {
                            Some((n, _, _)) => format!("soundfile{n}"),
                            None => "loaded, no data".to_owned(),
                        },
                        (Some(f), None) => match f.stream {
                            Some((n, _, _)) => format!("soundfile{n} (streamed)"),
                            None => "no file".to_owned(),
                        },
                        (None, None) => "no file".to_owned(),
                    });
                }
            }
            println!("  sounds: {} lists: {}", lists.len(), snd.text());
        }
        if !cap.attachments.is_empty() {
            let mut by_type: BTreeMap<String, Vec<&str>> = BTreeMap::new();
            for a in &cap.attachments {
                by_type
                    .entry(label(&ATT_TYPES, a.ty))
                    .or_default()
                    .push(&a.name);
            }
            println!("\nattachments in the zone ({}):", cap.attachments.len());
            for (t, names) in by_type {
                println!("  {t}: {}", names.join(" "));
            }
        }
    }
    if table {
        println!(
            "\n| weapon | display name | class | view model | zone | images inline / own pak / retail pak |"
        );
        println!("|---|---|---|---|---|---|");
        for r in rows {
            println!("{r}");
        }
    }
}
