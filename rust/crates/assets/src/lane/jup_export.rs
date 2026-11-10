//! bo2mp: the Modern Warfare III (2023) guest guns exported to a folder of
//! our own converted files, and read back from it when the game is not
//! installed (`CodLegacy export-jup <folder>`; `docs/JUP.md`, "The exported
//! guns").
//!
//! This is a child of `jup_gun`: what the export holds is what
//! `prefetch_jup_guest_guns` reads from the install (`JupRead`: each gun's
//! assembled view and world models, the gun materials' decoded pictures
//! and the camos' pictures), so the models are still built by the lane's
//! own build path (`build_jup_guest_guns`) from them, whether they came
//! from the install or from this folder. A change to the build (placement,
//! materials) therefore applies to an export without exporting again; a
//! change to what is read (the parts put together, the pictures decoded)
//! needs the export made again.
//!
//! Files (little-endian; `asset_transport::jup_export` has the folder
//! rules and the pack files):
//!
//! - `guns/<gun>.jgun`: `BO2JGUN1` and an LZ4 block (size first) of the
//!   gun's view and world models (bones, then per surface the vertex
//!   arrays, indices and its material's hash) and its view receiver's top;
//! - `materials_NNN.pack`/`materials.idx` in `materials/`: one blob per
//!   (material hash, picture size cap), `<hash>_<cap>`: `BO2JMAT1`, the
//!   gun flag, the tint, and the five pictures (colour, occlusion, normal,
//!   mask, gloss) each a length and a PNG (RGBA8, as decoded);
//! - `camos/camos.tsv` (`number<TAB>reference<TAB>name<TAB>game`) and the
//!   camos' pictures, PNG, in `camos_NNN.pack`.

use std::collections::HashMap;
use std::path::Path;

use asset_transport::jup_export as ex;
use fastfile_jup::gun::{Bone, Gun, PartSurface};

use super::{FIRST_PERSON_MAX, GUNS, JupGun, JupRead, MaterialPixels, NamedPicture, WORLD_MAX};

const GUN_MAGIC: &[u8; 8] = b"BO2JGUN1";
const MAT_MAGIC: &[u8; 8] = b"BO2JMAT1";

/// Folders of an export.
const MATERIALS: &str = "materials";
const CAMOS: &str = "camos";

// --- A tiny binary writer and reader. ---

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }
    fn text(&mut self, s: &str) {
        self.len(s.len());
        self.0.extend_from_slice(s.as_bytes());
    }
    fn floats<const N: usize>(&mut self, v: &[[f32; N]]) {
        self.len(v.len());
        for a in v {
            for f in a {
                self.f32(*f);
            }
        }
    }
}

struct In<'a> {
    b: &'a [u8],
    at: usize,
}

impl In<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    /// A count that fits what is left (each item at least `item` bytes).
    fn count(&mut self, item: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        (n.checked_mul(item.max(1))? <= self.b.len() - self.at).then_some(n)
    }
    fn text(&mut self) -> Option<String> {
        let n = self.count(1)?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn floats<const N: usize>(&mut self) -> Option<Vec<[f32; N]>> {
        let n = self.count(4 * N)?;
        (0..n)
            .map(|_| {
                let mut a = [0f32; N];
                for f in &mut a {
                    *f = self.f32()?;
                }
                Some(a)
            })
            .collect()
    }
}

// --- Guns. ---

fn put_gun(o: &mut Out, g: &Gun) {
    o.len(g.receiver_surfaces);
    o.len(g.bones.len());
    for b in &g.bones {
        o.u32(b.name);
        for f in b.rot {
            o.f32(f);
        }
        for f in b.pos {
            o.f32(f);
        }
    }
    o.len(g.missing.len());
    for m in &g.missing {
        o.text(m);
    }
    o.len(g.surfaces.len());
    for s in &g.surfaces {
        match s.material {
            Some(m) => {
                o.u8(1);
                o.u64(m);
            }
            None => o.u8(0),
        }
        o.floats(&s.positions);
        o.floats(&s.normals);
        o.floats(&s.tangents);
        o.floats(&s.uvs);
        o.len(s.colours.len());
        for c in &s.colours {
            o.0.extend_from_slice(c);
        }
        o.len(s.indices.len());
        for i in &s.indices {
            o.u32(*i);
        }
    }
}

fn get_gun(r: &mut In) -> Option<Gun> {
    let receiver_surfaces = r.u32()? as usize;
    let n = r.count(4 + 28)?;
    let mut bones = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.u32()?;
        let rot = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let pos = [r.f32()?, r.f32()?, r.f32()?];
        bones.push(Bone { name, rot, pos });
    }
    let n = r.count(4)?;
    let missing = (0..n).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
    let n = r.count(1)?;
    let mut surfaces = Vec::with_capacity(n);
    for _ in 0..n {
        let material = match r.u8()? {
            0 => None,
            _ => Some(r.u64()?),
        };
        let positions = r.floats::<3>()?;
        let normals = r.floats::<3>()?;
        let tangents = r.floats::<3>()?;
        let uvs = r.floats::<2>()?;
        let n = r.count(4)?;
        let colours = (0..n).map(|_| Some(r.take(4)?.try_into().ok()?)).collect::<Option<Vec<[u8; 4]>>>()?;
        let n = r.count(4)?;
        let indices = (0..n).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
        surfaces.push(PartSurface { positions, normals, tangents, uvs, colours, indices, material });
    }
    Some(Gun { surfaces, bones, missing, receiver_surfaces })
}

/// A gun file's bytes.
fn encode_gun(view: &Gun, world: &Gun, top: f32) -> Vec<u8> {
    let mut o = Out::default();
    o.f32(top);
    put_gun(&mut o, view);
    put_gun(&mut o, world);
    let mut file = GUN_MAGIC.to_vec();
    file.extend_from_slice(&lz4_flex::compress_prepend_size(&o.0));
    file
}

/// (view, world, the view receiver's top) from a gun file's bytes.
fn decode_gun(bytes: &[u8]) -> Option<(Gun, Gun, f32)> {
    let body = bytes.strip_prefix(GUN_MAGIC)?;
    let raw = lz4_flex::decompress_size_prepended(body).ok()?;
    let mut r = In { b: &raw, at: 0 };
    let top = r.f32()?;
    let view = get_gun(&mut r)?;
    let world = get_gun(&mut r)?;
    (r.at == raw.len()).then_some((view, world, top))
}

// --- Pictures and materials. ---

pub(crate) fn png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    wr.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// (width, height, RGBA8) from a PNG written by [`png`].
pub(crate) fn unpng(bytes: &[u8]) -> Option<NamedPicture> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0u8; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width, info.height, buf))
}

fn put_pic(o: &mut Out, p: &Option<NamedPicture>) -> Result<(), String> {
    match p {
        Some((w, h, px)) => {
            let bytes = png(*w, *h, px)?;
            o.len(bytes.len());
            o.0.extend_from_slice(&bytes);
        }
        None => o.u32(0),
    }
    Ok(())
}

fn encode_material(m: &MaterialPixels) -> Result<Vec<u8>, String> {
    let mut o = Out::default();
    o.0.extend_from_slice(MAT_MAGIC);
    o.u8(u8::from(m.gun));
    o.0.extend_from_slice(&m.tint);
    for p in [&m.colour, &m.occlusion, &m.normal, &m.mask, &m.gloss] {
        put_pic(&mut o, p)?;
    }
    Ok(o.0)
}

fn decode_material(bytes: &[u8]) -> Option<MaterialPixels> {
    let body = bytes.strip_prefix(MAT_MAGIC)?;
    let mut r = In { b: body, at: 0 };
    let gun = r.u8()? != 0;
    let tint: [u8; 3] = r.take(3)?.try_into().ok()?;
    let mut pics = Vec::with_capacity(5);
    for _ in 0..5 {
        let n = r.u32()? as usize;
        pics.push(if n == 0 { None } else { Some(unpng(r.take(n)?)?) });
    }
    let [colour, occlusion, normal, mask, gloss]: [Option<NamedPicture>; 5] = pics.try_into().ok()?;
    Some(MaterialPixels { tint, colour, occlusion, normal, mask, gloss, gun, ..MaterialPixels::default() })
}

fn material_name(hash: u64, cap: u32) -> String {
    format!("{hash:016x}_{cap}")
}

/// The materials the guns of a read use, as the lane asks for them: each
/// gun's view surfaces at the first-person size, its world ones at the
/// world size.
fn wanted_materials<'a>(guns: impl Iterator<Item = (&'a Gun, &'a Gun)>) -> Vec<(u64, u32)> {
    let mut out: Vec<(u64, u32)> = Vec::new();
    for (view, world) in guns {
        for (gun, max) in [(view, FIRST_PERSON_MAX), (world, WORLD_MAX)] {
            for s in &gun.surfaces {
                if let Some(m) = s.material
                    && !out.contains(&(m, max))
                {
                    out.push((m, max));
                }
            }
        }
    }
    out
}

// --- Writing. ---

/// What the export of the guns made (for the manifest and the report).
pub struct JupGunExport {
    pub guns: Vec<String>,
    pub gun_bytes: u64,
    pub materials: usize,
    pub material_bytes: u64,
    /// (number, reference, name, game) of each camo with a picture.
    pub camos: Vec<(u32, String, String, &'static str)>,
    pub camo_bytes: u64,
}

/// Writes what a read holds into `dest`: the Modern Warfare III (2023)
/// guns' files, their materials and the camos. `say` hears how far it is.
pub(crate) fn write(read: &JupRead, dest: &Path, say: &mut dyn FnMut(&str)) -> Result<JupGunExport, String> {
    let guns_dir = dest.join(ex::GUNS_DIR);
    std::fs::create_dir_all(&guns_dir).map_err(|e| format!("{}: {e}", guns_dir.display()))?;
    let ours: Vec<&(&'static JupGun, Gun, Gun, f32)> = read.guns.iter().collect();
    let mut w = JupGunExport { guns: Vec::new(), gun_bytes: 0, materials: 0, material_bytes: 0, camos: Vec::new(), camo_bytes: 0 };
    for (g, view, world, top) in ours.iter().map(|t| (t.0, &t.1, &t.2, t.3)) {
        let bytes = encode_gun(view, world, top);
        let path = guns_dir.join(format!("{}.{}", g.gun, ex::GUN_EXT));
        std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        w.gun_bytes += bytes.len() as u64;
        w.guns.push(g.gun.to_owned());
    }
    say(&format!("exported {} gun models ({:.1} MB)", w.guns.len(), w.gun_bytes as f64 / 1e6));

    // The materials the exported guns use (the read may hold more).
    let wanted = wanted_materials(ours.iter().map(|t| (&t.1, &t.2)));
    let mut pack = ex::PackWriter::new(&dest.join(MATERIALS), "materials")?;
    for (k, key) in wanted.iter().enumerate() {
        let Some(m) = read.materials.get(key) else { continue };
        pack.add(&material_name(key.0, key.1), &encode_material(m)?)?;
        if k % 200 == 199 {
            say(&format!("materials: {} of {}", k + 1, wanted.len()));
        }
    }
    (w.materials, w.material_bytes) = (pack.count, pack.bytes);
    pack.finish()?;
    say(&format!("exported {} materials with their pictures ({:.1} MB)", w.materials, w.material_bytes as f64 / 1e6));

    // The camos: Modern Warfare III's and II's, with their names.
    let mut tsv = String::new();
    let mut pack = ex::PackWriter::new(&dest.join(CAMOS), "camos")?;
    for (first, game, refs, names) in [
        (asset_core::weapon_camos::JUP_FIRST, "mw3", &asset_game::weapon_camo_models::JUP_CAMOS[..], &asset_game::weapon_camo_models::JUP_CAMO_NAMES[..]),
        (asset_core::weapon_camos::IW9_FIRST, "mw2", &asset_game::weapon_camo_models::IW9_CAMOS[..], &asset_game::weapon_camo_models::IW9_CAMO_NAMES[..]),
    ] {
        for (n, reference) in refs.iter().enumerate() {
            let Some((pw, ph, px)) = read.camos.get(*reference) else { continue };
            pack.add(reference, &png(*pw, *ph, px)?)?;
            let number = first + n as u32;
            let name = names.get(n).copied().unwrap_or(reference);
            tsv.push_str(&format!("{number}\t{reference}\t{name}\t{game}\n"));
            w.camos.push((number, (*reference).to_owned(), name.to_owned(), game));
        }
    }
    w.camo_bytes = pack.bytes;
    pack.finish()?;
    let camos_tsv = dest.join(CAMOS).join("camos.tsv");
    std::fs::write(&camos_tsv, tsv).map_err(|e| format!("{}: {e}", camos_tsv.display()))?;
    say(&format!("exported {} camos with their names ({:.1} MB)", w.camos.len(), w.camo_bytes as f64 / 1e6));
    Ok(w)
}

// --- Reading back. ---

/// An export read back whole: (guns, materials, camo pictures, problems).
pub(crate) fn check(dir: &Path) -> (usize, usize, usize, Vec<String>) {
    let (mut guns, mut materials, mut camos, mut report) = (Vec::new(), HashMap::new(), HashMap::new(), Vec::new());
    read_into(dir, &mut guns, &mut materials, &mut camos, &mut report);
    let problems = report.into_iter().filter(|l| !l.contains(" guns, ")).collect();
    (guns.len(), materials.len(), camos.len(), problems)
}

/// Adds the export's guns, materials and camos to what a read holds (the
/// install's own win: a gun or picture already there is kept). `report`
/// hears what was found and what was not.
pub(crate) fn read_into(
    dir: &Path,
    guns: &mut Vec<(&'static JupGun, Gun, Gun, f32)>,
    materials: &mut HashMap<(u64, u32), MaterialPixels>,
    camos: &mut HashMap<String, NamedPicture>,
    report: &mut Vec<String>,
) {
    let started = std::time::Instant::now();
    let mut added = Vec::new();
    for name in ex::guns(dir) {
        let Some(g) = GUNS.iter().find(|g| g.gun == name) else {
            report.push(format!("jup export: {name}: not a gun this build plays (export made by another build?)"));
            continue;
        };
        if guns.iter().any(|(have, ..)| have.gun == g.gun) {
            continue;
        }
        let path = dir.join(ex::GUNS_DIR).join(format!("{name}.{}", ex::GUN_EXT));
        match std::fs::read(&path).ok().and_then(|b| decode_gun(&b)) {
            Some((view, world, top)) => {
                added.push(g.gun);
                guns.push((g, view, world, top));
            }
            None => report.push(format!("jup export: {name}: {} does not read back (made by another format?)", path.display())),
        }
    }
    let wanted = wanted_materials(guns.iter().filter(|g| added.contains(&g.0.gun)).map(|g| (&g.1, &g.2)));
    let mut found = 0usize;
    if let Some(index) = ex::PackIndex::open(&dir.join(MATERIALS), "materials") {
        for key in wanted {
            if materials.contains_key(&key) {
                continue;
            }
            match index.get(&material_name(key.0, key.1)).and_then(|b| decode_material(&b)) {
                Some(m) => {
                    found += 1;
                    materials.insert(key, m);
                }
                None => report.push(format!("jup export: material {:016x} ({}) not in the export", key.0, key.1)),
            }
        }
    }
    let mut pictures = 0usize;
    if let Some(index) = ex::PackIndex::open(&dir.join(CAMOS), "camos") {
        for name in index.names() {
            if camos.contains_key(name) {
                continue;
            }
            if let Some(p) = index.get(name).and_then(|b| unpng(&b)) {
                camos.insert(name.clone(), p);
                pictures += 1;
            }
        }
    }
    report.push(format!(
        "jup export: {}: {} guns, {found} materials, {pictures} camo pictures in {:.1} s",
        dir.display(),
        added.len(),
        started.elapsed().as_secs_f64()
    ));
}
