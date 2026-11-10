//! Which of a material's pictures are its layers.
//!
//! The family 12 programs read 45 picture indices from the material record
//! (`records`), filled very probably in the material's slot-pair order
//! (`docs/JUP.md`). By groups of four, slot `i` of layer `l`: 0..3 colour,
//! 4..7 packed normal, 8..11 a 3-channel map times a colour, 12..15 a
//! 1-channel map compared with 128/255 (the layer's blend mask), 16..19 a
//! 1-channel map, and so on. This reads those slots by the picture's usage
//! byte (0x36/0x32 colour, 0x37 normal, 0x33 mask) so a slot the data does not
//! bear out is left empty, not guessed: [`classify`] reports how many of the
//! expected slots held the expected usage.

use fastfile_jup::map::materials::{MapImage, MapMaterial};

/// The first record slot of the colour pictures.
pub const COLOUR_AT: usize = 0;
/// The first record slot of the packed normal maps.
pub const NORMAL_AT: usize = 4;
/// The first record slot of the blend masks.
pub const MASK_AT: usize = 12;

/// Usage bytes.
pub const USAGE_COLOUR: u8 = 0x36;
pub const USAGE_MASK_COLOUR: u8 = 0x32;
pub const USAGE_NORMAL: u8 = 0x37;
pub const USAGE_MASK: u8 = 0x33;

/// A material's layers as indices into the map's images.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layers {
    pub colour: [Option<usize>; 4],
    pub normal: [Option<usize>; 4],
    /// The blend masks of layers 1 to 3.
    pub mask: [Option<usize>; 3],
    /// Layers drawn (1 to 4).
    pub count: u8,
    /// Slots read as expected / slots that held a picture of another usage.
    pub as_expected: u8,
    pub other_usage: u8,
}

/// A placeholder picture the engine fills unused slots with (1x1 or 4x4).
fn placeholder(img: &MapImage) -> bool {
    u32::from(img.width) * u32::from(img.height) <= 16
}

/// The material's pictures as layers by run: the pictures of one layer (its
/// colour, packed normal and own 0x33 mask) were written to the zone one after
/// another, so they hold neighbouring image indices (Highrise material 543:
/// 365..367 concrete, normal, grunge; 643..645 normal, mask, colour; 152/154
/// colour, normal). The run holding the material's first entry is the base;
/// the others are layers 1..3 in the order their first picture appears, each
/// blended in by the vertex's painted weight for it (byte k of the world
/// vertices' extra word), revealed through its own mask where it has one.
/// `None` when the pictures make fewer than two runs with a 0x36 colour each
/// (the slot classification then decides).
pub fn classify_runs(m: &MapMaterial, usage: &[u8], images: &[MapImage]) -> Option<Layers> {
    let mut seen: Vec<usize> = Vec::new();
    for i in m.entries.iter().flatten().copied() {
        let Some(img) = images.get(i) else { continue };
        if placeholder(img) || seen.contains(&i) {
            continue;
        }
        seen.push(i);
    }
    // Runs: pictures whose indices lie within two of each other, in order of first appearance.
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for &i in &seen {
        match runs.iter_mut().find(|r| r.iter().any(|&j| i.abs_diff(j) <= 2)) {
            Some(r) => r.push(i),
            None => runs.push(vec![i]),
        }
    }
    let of = |r: &[usize], u: u8| r.iter().copied().filter(|&i| usage.get(i).copied() == Some(u)).collect::<Vec<_>>();
    // (Only a 0x36 picture makes a run a layer: a run of 0x32 pictures alone is a
    // tint or mask set; taken as the base it drew Highrise's vista towers, material
    // 1137, black.)
    let colour_of = |r: &[usize]| of(r, USAGE_COLOUR).first().copied();
    if runs.len() < 2 || runs.iter().take(4).any(|r| colour_of(r).is_none()) {
        return None;
    }
    let dims = |i: usize| images.get(i).map(|im| (im.width, im.height));
    let mut out = Layers { count: 0, ..Default::default() };
    for (l, r) in runs.iter().take(4).enumerate() {
        let c = colour_of(r)?;
        out.colour[l] = Some(c);
        let normals = of(r, USAGE_NORMAL);
        out.normal[l] = normals.iter().copied().find(|&n| dims(n) == dims(c)).or_else(|| normals.first().copied());
        if l > 0 {
            out.mask[l - 1] = of(r, USAGE_MASK).first().copied();
        }
        out.count += 1;
    }
    out.as_expected = (out.colour.iter().chain(out.normal.iter()).chain(out.mask.iter()).flatten().count()) as u8;
    Some(out)
}

pub fn classify(m: &MapMaterial, usage: &[u8], images: &[MapImage]) -> Layers {
    if std::env::var("BO2MP_JUP_LAYER_RUNS").map_or(true, |v| v != "0")
        && let Some(l) = classify_runs(m, usage, images)
    {
        return l;
    }
    let mut out = Layers { count: 1, ..Default::default() };
    // The material's pictures in slot order, by usage. The record's fixed slot
    // numbers (0..3 colour, 4..7 normal, ...) are not what the zones hold: a
    // material fills the slots it needs, so a colour can sit in slot 4 or a mask
    // in slot 0. The k-th colour, normal and mask in order are layer k's.
    let mut colours: Vec<usize> = Vec::new();
    let mut tints: Vec<usize> = Vec::new();
    let mut normals: Vec<usize> = Vec::new();
    let mut masks: Vec<usize> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for e in &m.entries {
        let Some(i) = *e else { continue };
        let Some(img) = images.get(i) else { continue };
        if placeholder(img) || seen.contains(&i) {
            continue;
        }
        seen.push(i);
        match usage.get(i).copied().unwrap_or(0) {
            USAGE_COLOUR => colours.push(i),
            USAGE_MASK_COLOUR => tints.push(i),
            USAGE_NORMAL => normals.push(i),
            USAGE_MASK => masks.push(i),
            _ => out.other_usage += 1,
        }
    }
    // (A material with no 0x36 picture of its own falls back to its 0x32 ones.)
    if colours.is_empty() {
        colours = tints.clone();
    }
    // (Two blend masks and one colour: the layers' other colours are pictures
    // the loader could not find, and the one present is an upper layer. Alone it
    // would cover the surface with a picture meant for patches, so it is kept as
    // layer 1 over the material's stand-in albedo, by its blend mask; the shader
    // draws layer 0 from the stand-in where it has no picture.)
    let lone_overlay = masks.len() >= 2 && colours.len() == 1 && m.colour.is_some_and(|c| colours[0] == c);
    let dims = |i: usize| images.get(i).map(|im| (im.width, im.height));
    let mut free = normals.clone();
    let mut layer = 0usize;
    if lone_overlay {
        if !free.is_empty() {
            out.normal[0] = Some(free.remove(0));
        }
        layer = 1;
    }
    for (k, &c) in colours.iter().enumerate().take(4) {
        // (Layers above the first need their blend mask.)
        let mask = if layer == 0 { None } else { masks.get(layer - 1).copied() };
        if layer > 0 && mask.is_none() {
            break;
        }
        out.colour[layer] = Some(c);
        // The normal map of the same size, else the k-th.
        let at = free.iter().position(|&n| dims(n) == dims(c)).or_else(|| if free.is_empty() { None } else { Some(0) });
        if let Some(p) = at {
            out.normal[layer] = Some(free.remove(p));
        }
        if layer > 0 {
            out.mask[layer - 1] = mask;
        }
        let _ = k;
        layer += 1;
    }
    out.as_expected = (out.colour.iter().chain(out.normal.iter()).chain(out.mask.iter()).flatten().count()) as u8;
    // (No colour at all: the material's colour picture and its first packed normal map.)
    if out.colour[0].is_none() && !lone_overlay {
        out.colour[0] = m.colour;
    }
    if out.normal[0].is_none() {
        out.normal[0] = m.images.iter().copied().find(|&i| usage.get(i).copied() == Some(USAGE_NORMAL) && images.get(i).is_some_and(|im| !placeholder(im)));
    }
    out.count = 1 + (1..4).filter(|&l| out.colour[l].is_some() && out.mask[l - 1].is_some()).count() as u8;
    out
}
