//! bo2mp: camo packs, the player's own camos.
//!
//! A pack is how a camo is written down for every gun of every game at once:
//! a picture (or several, or a sheet of frames), and how it moves. Nothing in
//! it names a gun: the picture is laid over each gun's own texture
//! coordinates, as the other games' stock camos are, so one `scale` number
//! fits them all and no gun needs tuning.
//!
//! The `camos` folder next to the game (or `CODLEGACY_CAMOS`) holds:
//!
//! * a PNG: a still camo. A last word in its name makes it move (`lava_scroll`,
//!   `fire_pulse2`, `gem_sweep`, `ion_plasma`, `shine_gold`; a digit 0-3
//!   after the word is its speed).
//! * a folder: a pack. Several PNGs in it are the frames of an animation
//!   (played in name order, numbers by value: `2` before `10`); one PNG is a
//!   still; an optional `camo.txt` says more (see `docs/CAMOS.md`).
//!
//! The pictures are read by the asset lane (`assets::lane::t6_m2`); this
//! module is only the folder and the words, and the pixel work that does not
//! need a decoder.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::weapon_camos::{
    STYLE_FRAMES, STYLE_GOLD, STYLE_PLASMA, STYLE_PULSE, STYLE_RADIX, STYLE_REVEAL, STYLE_SCROLL_DIR, STYLE_SWEEP, USER_FIRST, USER_MAX,
    user_camo_from_stem,
};

/// The most frames one animation has (the filmstrip stays within 4096 texels).
pub const MAX_FRAMES: usize = 64;
/// The tallest filmstrip.
const MAX_STRIP: u32 = 4096;
/// The widest a filmstrip's frame is.
const MAX_FRAME_EDGE: u32 = 512;
/// How often a pack's picture repeats across a gun when it says nothing.
pub const DEFAULT_SCALE: f32 = 2.0;

/// Where a camo's picture comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// One picture.
    Image(PathBuf),
    /// One picture per frame, in play order.
    Frames(Vec<PathBuf>),
    /// One picture cut into `cols` x `rows` cells (left to right, top to
    /// bottom), the first `count` of them the frames.
    Sheet { path: PathBuf, cols: u32, rows: u32, count: u32 },
    /// A PNG built into the game (`builtin`).
    Embedded(&'static [u8]),
}

/// The camos that come with the game, in the CUSTOM tab before the player's
/// own: numbered from here, after the player's room (`USER_FIRST`, `USER_MAX`),
/// so adding a camo file never moves a built-in's number (classes save them).
pub const BUILTIN_FIRST: u32 = USER_FIRST + USER_MAX;
const _: () = assert!(BUILTIN_FIRST + 8 <= crate::weapon_camos::JUP_FIRST);

/// The built-in camos: Modern Warfare's Dark Matter (the nebula as the picture,
/// its reveal map as the alpha mask the `reveal` motion sweeps through).
/// Only ever append rows: the row index is the camo's number from `BUILTIN_FIRST`.
#[must_use]
pub fn builtin() -> Vec<UserCamo> {
    const DARK_MATTER: &[u8] = include_bytes!("../../../assets/camos/bo6_dark_matter.png");
    vec![UserCamo {
        number: BUILTIN_FIRST,
        name: "bo6 dark matter".to_owned(),
        source: Source::Embedded(DARK_MATTER),
        style: STYLE_REVEAL + STYLE_RADIX,
        scale: DEFAULT_SCALE,
        notes: Vec::new(),
    }]
}

/// One of the player's own camos.
#[derive(Clone, Debug, PartialEq)]
pub struct UserCamo {
    pub number: u32,
    /// The name the picker shows.
    pub name: String,
    pub source: Source,
    /// `PatternCamo::style` (0: still): a `STYLE_*` plus `STYLE_RADIX` times
    /// the speed step.
    pub style: u16,
    /// How many times the picture repeats across a gun.
    pub scale: f32,
    /// What was wrong in its `camo.txt` (the match load reports them).
    pub notes: Vec<String>,
}

/// What a `camo.txt` says. (Every key is optional.)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub name: Option<String>,
    /// still, scroll, pulse, sweep, plasma, gold, reveal or frames.
    pub motion: Option<String>,
    /// 0-3 (see `fps`).
    pub speed: Option<u16>,
    /// A compass step 0-7, counter-clockwise from the right.
    pub direction: Option<u16>,
    pub scale: Option<f32>,
    /// The picture's file name in the folder.
    pub image: Option<String>,
    /// A sheet's columns, rows and (when not all cells) frame count.
    pub grid: Option<(u32, u32, Option<u32>)>,
    pub notes: Vec<String>,
}

/// The speed step nearest a frame rate: the steps play 6, 12, 24 and 48
/// frames a second.
#[must_use]
pub fn fps_step(fps: f32) -> u16 {
    let off = |step: u16| (fps / (6.0 * f32::from(1u16 << step))).ln().abs();
    (0..=3u16).min_by(|a, b| off(*a).total_cmp(&off(*b))).unwrap_or(1)
}

/// A direction as a compass step: a word (`right`, `up-left`, `south`) or an
/// angle in degrees (0 right, 90 up, counter-clockwise).
#[must_use]
pub fn parse_direction(text: &str) -> Option<u16> {
    if let Ok(degrees) = text.trim().trim_end_matches('°').parse::<f32>() {
        if !degrees.is_finite() {
            return None;
        }
        return Some(((degrees / 45.0).round() as i32).rem_euclid(8) as u16);
    }
    let word: String = text.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_lowercase()).collect();
    Some(match word.as_str() {
        "right" | "east" => 0,
        "upright" | "rightup" | "northeast" => 1,
        "up" | "north" => 2,
        "upleft" | "leftup" | "northwest" => 3,
        "left" | "west" => 4,
        "downleft" | "leftdown" | "southwest" => 5,
        "down" | "south" => 6,
        "downright" | "rightdown" | "southeast" => 7,
        _ => return None,
    })
}

/// The style code of a motion word at a speed step (and, for a scroll, a
/// direction); None for a word that is not a motion.
#[must_use]
pub fn style_code(motion: &str, direction: u16, speed: u16) -> Option<u16> {
    let style = match motion {
        "" | "still" | "none" | "static" => return Some(0),
        "scroll" | "flow" | "slide" => STYLE_SCROLL_DIR + direction.min(7),
        "pulse" | "breathe" => STYLE_PULSE,
        "sweep" | "shimmer" | "glint" => STYLE_SWEEP,
        "plasma" => STYLE_PLASMA,
        "gold" => STYLE_GOLD,
        "reveal" | "dissolve" => STYLE_REVEAL,
        "frames" | "animation" | "animated" => STYLE_FRAMES,
        _ => return None,
    };
    Some(style + STYLE_RADIX * speed.min(3))
}

/// Reads a `camo.txt`: `key = value` lines (`key: value` too), `#` or `;`
/// comments, unknown or bad lines noted and skipped.
#[must_use]
pub fn parse_settings(text: &str) -> Settings {
    let mut s = Settings::default();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let at = n + 1;
        let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':')) else {
            s.notes.push(format!("camo.txt line {at}: expected `key = value`"));
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "name" => s.name = Some(value.to_owned()).filter(|v| !v.is_empty()),
            "move" | "motion" | "style" => s.motion = Some(value.to_ascii_lowercase()),
            "speed" => match value.parse::<u16>() {
                Ok(v) if v <= 3 => s.speed = Some(v),
                _ => s.notes.push(format!("camo.txt line {at}: speed is 0, 1, 2 or 3")),
            },
            "fps" => match value.parse::<f32>() {
                Ok(v) if v.is_finite() && v > 0.0 => s.speed = Some(fps_step(v)),
                _ => s.notes.push(format!("camo.txt line {at}: fps is a number above 0")),
            },
            "direction" | "dir" => match parse_direction(value) {
                Some(d) => s.direction = Some(d),
                None => s.notes.push(format!("camo.txt line {at}: direction is right, left, up, down, up-right ... or degrees")),
            },
            "scale" | "repeat" | "tiles" => match value.parse::<f32>() {
                Ok(v) if v.is_finite() && v > 0.0 => s.scale = Some(v.clamp(0.25, 64.0)),
                _ => s.notes.push(format!("camo.txt line {at}: scale is a number above 0")),
            },
            "image" | "file" | "picture" => s.image = Some(value.to_owned()).filter(|v| !v.is_empty()),
            "grid" | "sheet" | "cells" => {
                let nums: Vec<u32> = value.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).collect();
                match nums.as_slice() {
                    &[c, r] if c >= 1 && r >= 1 && c.saturating_mul(r) as usize <= MAX_FRAMES => s.grid = Some((c, r, None)),
                    &[c, r, n] if c >= 1 && r >= 1 && n >= 1 && n <= c.saturating_mul(r) && n as usize <= MAX_FRAMES => {
                        s.grid = Some((c, r, Some(n)));
                    }
                    _ => s.notes.push(format!("camo.txt line {at}: grid is columns x rows (4x4), up to {MAX_FRAMES} cells; a third number is the frame count")),
                }
            }
            other => s.notes.push(format!("camo.txt line {at}: unknown setting `{other}`")),
        }
    }
    s
}

/// A name for the picker from a file or folder name: an ordering prefix
/// (`01_`) dropped, underscores as spaces.
#[must_use]
pub fn display_name(stem: &str) -> String {
    let rest = stem.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = match rest.chars().next() {
        Some('_' | '-' | ' ') if rest.len() < stem.len() && rest.len() > 1 => &rest[1..],
        _ => stem,
    };
    rest.replace('_', " ").trim().to_owned()
}

/// File names in the order a person means: numbers by value.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u64 {
        let mut n = 0u64;
        while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
            n = n.saturating_mul(10).saturating_add(u64::from(d));
            it.next();
        }
        n
    }
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => match number(&mut a).cmp(&number(&mut b)) {
                Ordering::Equal => {}
                other => return other,
            },
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => {
                    a.next();
                    b.next();
                }
                other => return other,
            },
        }
    }
}

/// A pack from its folder's pictures (`pngs`, in play order) and settings.
/// None when it has no picture at all.
#[must_use]
pub fn build_pack(folder: &str, dir: &Path, pngs: &[PathBuf], settings: Settings, number: u32) -> Option<UserCamo> {
    let mut notes = settings.notes.clone();
    let named = settings.image.as_ref().map(|f| dir.join(f));
    let mut source = if let (Some((cols, rows, count)), Some(path)) = (settings.grid, named.clone().or_else(|| pngs.first().cloned())) {
        let count = count.unwrap_or(cols * rows);
        if count > 1 { Source::Sheet { path, cols, rows, count } } else { Source::Image(path) }
    } else if let Some(path) = named {
        Source::Image(path)
    } else if pngs.len() > 1 {
        if pngs.len() > MAX_FRAMES {
            notes.push(format!("{} pictures: only the first {MAX_FRAMES} are frames", pngs.len()));
        }
        Source::Frames(pngs.iter().take(MAX_FRAMES).cloned().collect())
    } else {
        Source::Image(pngs.first()?.clone())
    };
    let moving_source = !matches!(source, Source::Image(_));
    let motion = settings.motion.as_deref().unwrap_or(if moving_source { "frames" } else { "still" });
    let mut style = style_code(motion, settings.direction.unwrap_or(0), settings.speed.unwrap_or(1)).unwrap_or_else(|| {
        notes.push(format!("unknown move `{motion}` (still, scroll, pulse, sweep, plasma, gold, reveal, frames): drawn still"));
        0
    });
    let frames = style % STYLE_RADIX == STYLE_FRAMES;
    if frames && !moving_source {
        notes.push("`move = frames` needs several pictures or a grid: drawn still".to_owned());
        style = 0;
    } else if !frames && moving_source {
        // (Only an animation plays several pictures: any other motion
        // moves the first one.)
        source = match source {
            Source::Frames(list) => Source::Image(list.into_iter().next()?),
            Source::Sheet { path, .. } => Source::Image(path),
            other => other,
        };
        notes.push("several pictures play only with `move = frames`: the first is drawn".to_owned());
    }
    let name = settings.name.clone().unwrap_or_else(|| display_name(folder));
    Some(UserCamo { number, name, source, style, scale: settings.scale.unwrap_or(DEFAULT_SCALE), notes })
}

fn is_png(path: &Path) -> bool {
    path.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A folder's PNGs in play order.
fn folder_pngs(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut pngs: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file() && is_png(p)).collect();
    pngs.sort_by(|a, b| natural_cmp(&file_name(a).to_lowercase(), &file_name(b).to_lowercase()));
    pngs
}

/// The camos of a `camos` folder: PNGs and pack folders in name order, the
/// first `USER_MAX`, numbered from `USER_FIRST`.
#[must_use]
pub fn discover(dir: &Path) -> Vec<UserCamo> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<PathBuf> = read.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort_by(|a, b| natural_cmp(&file_name(a).to_lowercase(), &file_name(b).to_lowercase()));
    let mut out: Vec<UserCamo> = Vec::new();
    for path in entries {
        if out.len() >= USER_MAX as usize {
            break;
        }
        let number = USER_FIRST + out.len() as u32;
        if path.is_dir() {
            let pngs = folder_pngs(&path);
            let settings = std::fs::read_to_string(path.join("camo.txt")).map(|t| parse_settings(&t)).unwrap_or_default();
            if let Some(pack) = build_pack(&file_name(&path), &path, &pngs, settings, number) {
                out.push(pack);
            }
        } else if path.is_file() && is_png(&path) {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let (name, style) = user_camo_from_stem(&stem);
            out.push(UserCamo { number, name: display_name(&name), source: Source::Image(path), style, scale: DEFAULT_SCALE, notes: Vec::new() });
        }
    }
    out
}

// ---- pixels (RGBA8) ----

/// A picture resampled (bilinear, edges clamped) to `tw` x `th`.
#[must_use]
pub fn resample(src: &[u8], sw: u32, sh: u32, tw: u32, th: u32) -> Vec<u8> {
    if sw == tw && sh == th {
        return src.to_vec();
    }
    let mut out = vec![0u8; (tw * th * 4) as usize];
    let (fx, fy) = (sw as f32 / tw as f32, sh as f32 / th as f32);
    for y in 0..th {
        let sy = ((y as f32 + 0.5) * fy - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (sy.floor() as u32, sy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..tw {
            let sx = ((x as f32 + 0.5) * fx - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (sx.floor() as u32, sx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..4u32 {
                let at = |xx: u32, yy: u32| f32::from(src[((yy * sw + xx) * 4 + c) as usize]);
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                out[((y * tw + x) * 4 + c) as usize] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    out
}

/// A sheet cut into its frames (`cols` x `rows` cells, left to right, top to
/// bottom, the first `count`); None when the sheet is smaller than its grid.
#[must_use]
pub fn slice_sheet(w: u32, h: u32, rgba: &[u8], cols: u32, rows: u32, count: u32) -> Option<Vec<(u32, u32, Vec<u8>)>> {
    if cols == 0 || rows == 0 || w < cols || h < rows || rgba.len() != (w * h * 4) as usize {
        return None;
    }
    let (cw, ch) = (w / cols, h / rows);
    let mut out = Vec::new();
    for i in 0..count.min(cols * rows) {
        let (cx, cy) = (i % cols, i / cols);
        let mut cell = Vec::with_capacity((cw * ch * 4) as usize);
        for y in 0..ch {
            let start = (((cy * ch + y) * w + cx * cw) * 4) as usize;
            cell.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
        }
        out.push((cw, ch, cell));
    }
    Some(out)
}

/// Frames as one picture, the shape the shader's `STYLE_FRAMES` reads: square
/// frames (each stretched to one common side, a power of two up to 512 that
/// keeps the strip within 4096 tall) stacked top to bottom, so the frame
/// count is the picture's height over its width. A lone frame is returned as
/// it is.
#[must_use]
pub fn filmstrip(frames: &[(u32, u32, Vec<u8>)]) -> Option<(u32, u32, Vec<u8>)> {
    let n = frames.len();
    if n == 0 || n > MAX_FRAMES || frames.iter().any(|(w, h, px)| *w == 0 || *h == 0 || px.len() != (*w * *h * 4) as usize) {
        return None;
    }
    if n == 1 {
        return frames.first().cloned();
    }
    let biggest = frames.iter().map(|f| f.0.max(f.1)).max()?;
    let limit = (MAX_STRIP / n as u32).min(MAX_FRAME_EDGE).min(biggest.next_power_of_two()).max(16);
    let side = 1u32 << (31 - limit.leading_zeros());
    let mut out = Vec::with_capacity((side * side * 4) as usize * n);
    for (w, h, px) in frames {
        out.extend(resample(px, *w, *h, side, side));
    }
    Some((side, side * n as u32, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_camos_are_png_and_clear_of_the_players_numbers() {
        let list = builtin();
        assert!(!list.is_empty() && list.len() <= 8);
        for (i, c) in list.iter().enumerate() {
            assert_eq!(c.number, BUILTIN_FIRST + i as u32);
            assert!(c.number >= USER_FIRST + USER_MAX);
            assert!(matches!(c.source, Source::Embedded(b) if b.starts_with(b"\x89PNG")));
        }
    }

    #[test]
    fn settings_read_words_and_note_mistakes() {
        let s = parse_settings("# lava\nName = Hot Lava\nmove: scroll\ndirection = up-left\nspeed = 2\nscale = 3.5\nbogus = 1\nspeed = 9\nno equals");
        assert_eq!(s.name.as_deref(), Some("Hot Lava"));
        assert_eq!(s.motion.as_deref(), Some("scroll"));
        assert_eq!((s.direction, s.speed, s.scale), (Some(3), Some(2), Some(3.5)));
        assert_eq!(s.notes.len(), 3, "{:?}", s.notes);
    }

    #[test]
    fn directions_and_frame_rates_land_on_steps() {
        assert_eq!(parse_direction("Right"), Some(0));
        assert_eq!(parse_direction("down_left"), Some(5));
        assert_eq!(parse_direction("90"), Some(2));
        assert_eq!(parse_direction("-90"), Some(6));
        assert_eq!(parse_direction("sideways"), None);
        assert_eq!([6.0, 12.0, 24.0, 48.0, 30.0, 1.0].map(fps_step), [0, 1, 2, 3, 2, 0]);
    }

    #[test]
    fn style_codes_decode_as_the_shader_does() {
        assert_eq!(style_code("still", 0, 1), Some(0));
        for (motion, direction, speed) in [("scroll", 7, 3), ("scroll", 0, 0), ("pulse", 0, 2), ("frames", 0, 1), ("plasma", 0, 3), ("reveal", 0, 3)] {
            let code = style_code(motion, direction, speed).unwrap();
            assert!(code % STYLE_RADIX >= 1 && code / STYLE_RADIX == speed, "{motion}");
        }
        assert_eq!(style_code("scroll", 2, 1), Some(STYLE_SCROLL_DIR + 2 + STYLE_RADIX));
        assert_ne!(crate::weapon_camos::STYLE_SCROLL, STYLE_FRAMES);
        assert_eq!(style_code("wobble", 0, 1), None);
    }

    #[test]
    fn names_drop_their_ordering_prefix() {
        assert_eq!(display_name("01_Lava_Flow"), "Lava Flow");
        assert_eq!(display_name("2024"), "2024");
        assert_eq!(display_name("7 Seas"), "Seas");
        assert_eq!(display_name("red_tiger"), "red tiger");
    }

    #[test]
    fn frames_play_in_number_order() {
        let mut names = vec!["f10.png", "f2.png", "f1.png", "f20.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["f1.png", "f2.png", "f10.png", "f20.png"]);
    }

    fn pngs(n: usize) -> Vec<PathBuf> {
        (1..=n).map(|i| PathBuf::from(format!("pack/f{i}.png"))).collect()
    }

    #[test]
    fn a_folder_of_pictures_is_an_animation() {
        let pack = build_pack("Fire", Path::new("pack"), &pngs(8), Settings::default(), 220).unwrap();
        assert!(matches!(&pack.source, Source::Frames(f) if f.len() == 8));
        assert_eq!(pack.style, STYLE_FRAMES + STYLE_RADIX);
        assert_eq!(pack.scale, DEFAULT_SCALE);
        let one = build_pack("Still", Path::new("pack"), &pngs(1), Settings::default(), 221).unwrap();
        assert!(matches!(one.source, Source::Image(_)) && one.style == 0);
        assert!(build_pack("Empty", Path::new("pack"), &[], Settings::default(), 222).is_none());
    }

    #[test]
    fn a_sheet_and_a_mismatched_motion_are_sorted_out() {
        let sheet = parse_settings("grid = 4x2\nfps = 24");
        let pack = build_pack("Sheet", Path::new("pack"), &pngs(1), sheet, 220).unwrap();
        assert!(matches!(&pack.source, Source::Sheet { cols: 4, rows: 2, count: 8, .. }));
        assert_eq!(pack.style, STYLE_FRAMES + 2 * STYLE_RADIX);
        // Frames from one picture, and a scroll over many pictures.
        let bad = build_pack("Bad", Path::new("pack"), &pngs(1), parse_settings("move = frames"), 220).unwrap();
        assert_eq!(bad.style, 0);
        assert_eq!(bad.notes.len(), 1);
        let many = build_pack("Flow", Path::new("pack"), &pngs(3), parse_settings("move = scroll\ndirection = down"), 220).unwrap();
        assert!(matches!(many.source, Source::Image(_)));
        assert_eq!(many.style, STYLE_SCROLL_DIR + 6 + STYLE_RADIX);
    }

    fn solid(w: u32, h: u32, v: u8) -> (u32, u32, Vec<u8>) {
        (w, h, vec![v; (w * h * 4) as usize])
    }

    #[test]
    fn a_filmstrip_stacks_square_frames() {
        let (w, h, px) = filmstrip(&[solid(100, 60, 10), solid(64, 64, 20), solid(8, 8, 30)]).unwrap();
        assert_eq!((w, h), (128, 384));
        assert_eq!(px.len(), (w * h * 4) as usize);
        let frame = (w * w * 4) as usize;
        assert_eq!([px[0], px[frame], px[2 * frame]], [10, 20, 30]);
        // Many frames still fit the texture limit.
        let (w, h, _) = filmstrip(&vec![solid(256, 256, 0); 64]).unwrap();
        assert_eq!((w, h), (64, 4096));
        assert!(filmstrip(&vec![solid(4, 4, 0); 65]).is_none());
        assert_eq!(filmstrip(&[solid(30, 20, 5)]).unwrap().0, 30);
    }

    #[test]
    fn a_sheet_slices_left_to_right_then_down() {
        // 4 x 2 cells of 2 x 2 texels, cell i filled with i.
        let (w, h) = (8u32, 4u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let cell = (y / 2) * 4 + x / 2;
                let at = ((y * w + x) * 4) as usize;
                px[at..at + 4].fill(cell as u8);
            }
        }
        let cells = slice_sheet(w, h, &px, 4, 2, 6).unwrap();
        assert_eq!(cells.len(), 6);
        assert!(cells.iter().enumerate().all(|(i, (cw, ch, c))| (*cw, *ch) == (2, 2) && c.iter().all(|&b| b == i as u8)));
        assert!(slice_sheet(w, h, &px, 9, 2, 6).is_none());
    }
}
