//! bo2mp: Ghosts' bullet penetration depth table, the raw file
//! `info/bullet_penetration_mp`.
//!
//! The game reads it once as an info string (`BULLET_PEN_TABLE`, then
//! `\key\value` pairs) and fills three rows of 31 floats: `small_<surface>`,
//! `medium_<surface>`, `large_<surface>` for each of its 31 surface types, in
//! the order of `SURFACE_NAMES` (row 0, no penetration, stays zero). A
//! weapon's depth on a surface is `row[penetrate type][surface type]` times
//! the weapon's multiplier (see `docs/exe_notes/IW6_COLLISION.md`, 4.2).
//!
//! The table lives in a zone the map does not need (a code or common zone of
//! the install), so `find_in_image` looks for the raw file by name in an
//! inflated zone image without walking the zone.

use std::io::Read;

/// Ghosts' surface types, in its order (bits 20..24 of a surface's flags).
pub const SURFACE_NAMES: [&str; 31] = [
    "default",
    "bark",
    "brick",
    "carpet",
    "cloth",
    "concrete",
    "dirt",
    "flesh",
    "foliage",
    "glass",
    "grass",
    "gravel",
    "ice",
    "metal",
    "metal_grate",
    "mud",
    "paper",
    "plaster",
    "rock",
    "sand",
    "snow",
    "water",
    "wood",
    "asphalt",
    "ceramic",
    "plastic",
    "rubber",
    "fruit",
    "paintedmetal",
    "riotshield",
    "slush",
];

/// The raw file's name.
pub const RAW_FILE: &str = "info/bullet_penetration_mp";

/// The info string's leading word.
pub const MAGIC: &str = "BULLET_PEN_TABLE";

/// Depths by penetrate type (0 none, 1 small, 2 medium, 3 large) and
/// Ghosts surface type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenTable {
    pub depths: [[f32; 31]; 4],
    /// Keys the string set (of the 93 the game asks for).
    pub keys_set: usize,
}

/// Reads the info string. `None` when it does not start with the magic
/// word. Keys the game does not ask for are ignored (the game's parser
/// only looks for its own); a value that is not a number reads as 0.
pub fn parse(text: &str) -> Option<PenTable> {
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    let rest = text.strip_prefix(MAGIC)?;
    let mut out = PenTable {
        depths: [[0.0; 31]; 4],
        keys_set: 0,
    };
    let mut parts = rest.split('\\');
    // (The string starts with a separator: `BULLET_PEN_TABLE\small_bark\..`.)
    if rest.starts_with('\\') {
        parts.next();
    }
    while let (Some(key), Some(value)) = (parts.next(), parts.next()) {
        let Some((class, surface)) = key.trim().split_once('_') else {
            continue;
        };
        let row = match class {
            "small" => 1,
            "medium" => 2,
            "large" => 3,
            _ => continue,
        };
        let Some(col) = SURFACE_NAMES.iter().position(|n| *n == surface) else {
            continue;
        };
        out.depths[row][col] = value.trim().parse().unwrap_or(0.0);
        out.keys_set += 1;
    }
    Some(out)
}

/// A pointer the zone loads inline (`FDFDFDFF_FFFFFFFF`).
const FOLLOW: u64 = 0xFDFD_FDFF_FFFF_FFFF;

/// Finds the raw file in an inflated zone image by its name (the raw file's
/// 24-byte header, name and data are written in that order) and returns
/// its text. The header is checked (both pointers inline, sizes in range).
pub fn find_in_image(image: &[u8]) -> Option<Vec<u8>> {
    let mut needle = RAW_FILE.as_bytes().to_vec();
    needle.push(0);
    let mut from = 0usize;
    while let Some(p) = image[from..]
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
    {
        let at = from + p;
        from = at + 1;
        let Some(h) = at.checked_sub(24) else {
            continue;
        };
        let u64_at = |o: usize| u64::from_le_bytes(image[o..o + 8].try_into().unwrap_or([0; 8]));
        let i32_at = |o: usize| i32::from_le_bytes(image[o..o + 4].try_into().unwrap_or([0; 4]));
        if u64_at(h) != FOLLOW || u64_at(h + 16) != FOLLOW {
            continue;
        }
        let (compressed, len) = (i32_at(h + 8), i32_at(h + 12));
        if !(0..=1 << 20).contains(&compressed) || !(1..=1 << 20).contains(&len) {
            continue;
        }
        let data_at = at + needle.len();
        if compressed > 0 {
            let src = image.get(data_at..data_at + compressed as usize)?;
            let mut out = Vec::with_capacity(len as usize);
            if flate2::read::ZlibDecoder::new(src)
                .read_to_end(&mut out)
                .is_ok()
            {
                return Some(out);
            }
        } else if let Some(src) = image.get(data_at..data_at + len as usize) {
            return Some(src.to_vec());
        }
    }
    None
}

/// Zones of an install that may hold the table, in the order they are
/// tried (the code zones first: small).
pub const ZONES: [&str; 6] = [
    "code_pre_gfx_mp",
    "patch_code_post_gfx_mp",
    "code_post_gfx_mp",
    "patch_common_mp",
    "common_mp",
    "common_core_mp",
];

/// Looks for the table in `zone_dir`'s zones (`ZONES`): the table and the
/// zone it came from, or what was looked at.
pub fn find_in_zones(zone_dir: &std::path::Path) -> Result<(PenTable, String), String> {
    let mut tried = Vec::new();
    for z in ZONES {
        let path = zone_dir.join(format!("{z}.ff"));
        if !path.is_file() {
            continue;
        }
        let Ok(zone) = crate::Zone::open(&path) else {
            tried.push(format!("{z} (does not open)"));
            continue;
        };
        tried.push(z.to_owned());
        if let Some(bytes) = find_in_image(&zone.image)
            && let Some(t) = parse(&String::from_utf8_lossy(&bytes))
        {
            return Ok((t, z.to_owned()));
        }
    }
    Err(if tried.is_empty() {
        format!("no zone of {:?} in {}", ZONES, zone_dir.display())
    } else {
        format!("not in {}", tried.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_three_rows_by_ghosts_surface_names() {
        let t = parse("BULLET_PEN_TABLE\\small_wood\\4\\medium_metal_grate\\12.5\\large_slush\\30\\small_default\\9\\bogus_wood\\1\\small_cushion\\2").unwrap();
        assert_eq!(t.depths[1][22], 4.0);
        assert_eq!(t.depths[2][14], 12.5);
        assert_eq!(t.depths[3][30], 30.0);
        // Row 0 is never set; a default-surface key is read as the game does.
        assert_eq!(t.depths[0], [0.0; 31]);
        assert_eq!(t.depths[1][0], 9.0);
        // Not Ghosts names: left out.
        assert_eq!(t.keys_set, 4);
        assert!(parse("SOMETHING_ELSE\\small_wood\\1").is_none());
    }

    #[test]
    fn finds_the_raw_file_behind_its_header() {
        let text = b"BULLET_PEN_TABLE\\small_wood\\4";
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut enc, text).unwrap();
        let packed = enc.finish().unwrap();
        let mut image = vec![0xAAu8; 40];
        image.extend(FOLLOW.to_le_bytes());
        image.extend((packed.len() as i32).to_le_bytes());
        image.extend((text.len() as i32).to_le_bytes());
        image.extend(FOLLOW.to_le_bytes());
        image.extend(RAW_FILE.as_bytes());
        image.push(0);
        image.extend(&packed);
        image.extend([0u8; 16]);
        // A mention of the name elsewhere (a string table) is not the file.
        let mut decoy = RAW_FILE.as_bytes().to_vec();
        decoy.push(0);
        let mut all = decoy.clone();
        all.extend(&image);
        assert_eq!(find_in_image(&all).as_deref(), Some(&text[..]));
        let t = parse(&String::from_utf8_lossy(&find_in_image(&all).unwrap())).unwrap();
        assert_eq!(t.depths[1][22], 4.0);
        assert!(find_in_image(&decoy).is_none());
    }
}
