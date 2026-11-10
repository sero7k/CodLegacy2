//! Modern Warfare III's shader programs: DXIL (shader model 6) in `DXBC`
//! containers inside the zone image, found by scanning for the container
//! magic (the assets around them are not walked).
//!
//! A container is `DXBC`, 16 hash bytes, u32 1, u32 total size, u32 part
//! count, u32 part offsets; each part is a fourcc and a u32 length. The game's
//! are stripped: `SFI0`, `ISG1`, `OSG1`, (`PSG1`), `PSV0`, `HASH`, `DXIL`
//! (the bitcode wrapper), no `RDAT`/`RDEF`. What is readable without the
//! bitcode: the stage and every resource binding (from `PSV0`) and the
//! input/output signatures (semantic names). See `docs/JUP.md`.

/// Pipeline stage (`PSV0` runtime info, byte 24).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Pixel,
    Vertex,
    Geometry,
    Hull,
    Domain,
    Compute,
    Library,
    Mesh,
    Amplification,
    Other(u8),
}

impl Stage {
    fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Pixel,
            1 => Self::Vertex,
            2 => Self::Geometry,
            3 => Self::Hull,
            4 => Self::Domain,
            5 => Self::Compute,
            6 => Self::Library,
            7 => Self::Mesh,
            8 => Self::Amplification,
            o => Self::Other(o),
        }
    }
}

/// `PSV0` resource class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResClass {
    Sampler,
    Cbv,
    SrvTyped,
    SrvRaw,
    SrvStructured,
    UavTyped,
    UavRaw,
    UavStructured,
    UavCounter,
    Other(u32),
}

impl ResClass {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Sampler,
            2 => Self::Cbv,
            3 => Self::SrvTyped,
            4 => Self::SrvRaw,
            5 => Self::SrvStructured,
            6 => Self::UavTyped,
            7 => Self::UavRaw,
            8 => Self::UavStructured,
            9 => Self::UavCounter,
            o => Self::Other(o),
        }
    }
}

/// One resource range a program declares (`t3`/`b2`/`s7..s10`, in a space).
/// `kind` is `DXIL::ResourceKind` (2 Texture2D, 4 Texture3D, 5 TextureCube,
/// 7 Texture2DArray, 11 RawBuffer, 12 StructuredBuffer, 13 CBuffer, 14 Sampler).
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub class: ResClass,
    pub space: u32,
    pub lower: u32,
    /// `u32::MAX`-ish upper bounds mean unbounded; the game's bindless ranges
    /// end at 270335.
    pub upper: u32,
    pub kind: u32,
}

/// One input or output signature element.
#[derive(Clone, Debug)]
pub struct SigElem {
    pub semantic: String,
    pub index: u32,
    /// `D3D_REGISTER_COMPONENT_TYPE`: 1 uint, 2 int, 3 float.
    pub comp_type: u32,
    pub register: u32,
    pub mask: u8,
}

/// A program found in a zone image.
#[derive(Clone, Debug)]
pub struct Program {
    /// Offset of the `DXBC` magic in the zone image.
    pub at: usize,
    pub size: usize,
    pub stage: Stage,
    /// The `hlsl` source path and entry when the record names one in the
    /// clear just before the container (compute and post-process programs);
    /// the material pixel/vertex programs carry none.
    pub name: String,
    pub bindings: Vec<Binding>,
    pub inputs: Vec<SigElem>,
    pub outputs: Vec<SigElem>,
    /// The bitcode wrapper's range (`DXIL` part payload) in the image.
    pub bitcode: std::ops::Range<usize>,
    /// The program record's own key fields, from the 64 bytes before the
    /// container (see [`ProgramKey`]).
    pub key: ProgramKey,
}

/// The fields of a program record that tell what it is for. The record
/// before a container is eight little-endian u64: `[size flags, pass mask,
/// family, feature mask, 0, 0, -4, container size]`.
///
/// Measured on `code_post_gfx`: of the 567 pixel programs that read material
/// pictures (bindless), `family` is 12 for 341 (they read the material
/// record in the raw buffer `t39` of space 1; 266 do), 44 for 199 and 45 for 13
/// (they read it in `t47`, a second kind of record), and 0, 2, 8, 9, 13 for
/// 14 others; `pass_mask` has one of nine bits (`0x1` to `0x100`) set on all
/// but a few that set two (`0xc`, `0x60`, `0x110`, `0x180`, ...);
/// `feature_mask` is `2^k - 1` (k up to 6 on family 12) or, on family 44,
/// `2^k - 1` for k 6 to 12 and `0x100 | (2^k - 1)`. The pass bits come in
/// pairs with identical statistics (`0x2` and `0x10`, `0x4` and `0x20`,
/// `0x8` and `0x40`). What a bit means is not stored; the engine's own
/// tables (not in the zone) say which pass a bit is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProgramKey {
    pub pass_mask: u64,
    pub family: u64,
    pub feature_mask: u64,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

type Part = ([u8; 4], usize, usize);

fn parts(z: &[u8], at: usize) -> Option<(usize, Vec<Part>)> {
    let size = u32_at(z, at + 24)? as usize;
    let n = u32_at(z, at + 28)? as usize;
    if n == 0 || n > 16 || size < 32 + 4 * n || at + size > z.len() {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let p = u32_at(z, at + 32 + 4 * k)? as usize;
        let len = u32_at(z, at + p + 4)? as usize;
        if p + 8 + len > size {
            return None;
        }
        let tag: [u8; 4] = z.get(at + p..at + p + 4)?.try_into().ok()?;
        out.push((tag, at + p + 8, len));
    }
    Some((size, out))
}

fn signature(z: &[u8], at: usize, len: usize) -> Vec<SigElem> {
    let mut out = Vec::new();
    let Some(count) = u32_at(z, at) else { return out };
    for i in 0..count as usize {
        let e = at + 8 + 32 * i;
        if e + 32 > at + len {
            break;
        }
        let g = |k: usize| u32_at(z, e + 4 * k).map_or(0, |v| v);
        let name_at = at + g(1) as usize;
        let name = z
            .get(name_at..(at + len).min(name_at + 64))
            .and_then(|s| s.iter().position(|&b| b == 0).map(|n| &s[..n]))
            .map_or_else(String::new, |s| String::from_utf8_lossy(s).into_owned());
        out.push(SigElem {
            semantic: name,
            index: g(2),
            comp_type: g(4),
            register: g(5),
            mask: z[e + 24],
        });
    }
    out
}

fn name_before(z: &[u8], at: usize) -> String {
    let from = at.saturating_sub(400);
    let seg = &z[from..at];
    let mut best: &[u8] = &[];
    let mut start = None;
    for (i, &b) in seg.iter().enumerate() {
        match (start, b) {
            (None, 0x20..=0x7e) => start = Some(i),
            (Some(s), 0) => {
                if i - s >= 6 {
                    best = &seg[s..i];
                }
                start = None;
            }
            (Some(_), 0x20..=0x7e) => {}
            (Some(_), _) => start = None,
            _ => {}
        }
    }
    String::from_utf8_lossy(best).into_owned()
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

/// The key fields of the record that precedes the container at `at`
/// (zero where the zone starts too soon or the record is not of this form:
/// its sixth and seventh words are `-4` and the container's size).
fn record_key(z: &[u8], at: usize) -> ProgramKey {
    let Some(from) = at.checked_sub(64) else { return ProgramKey::default() };
    let w = |k: usize| u64_at(z, from + 8 * k).unwrap_or(0);
    if w(6) != 0xffff_ffff_ffff_fffc {
        return ProgramKey::default();
    }
    ProgramKey { pass_mask: w(1), family: w(2), feature_mask: w(3) }
}

/// Every well-formed container of the zone image, in order.
#[must_use]
pub fn scan(z: &[u8]) -> Vec<Program> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(p) = z[at..].windows(4).position(|w| w == b"DXBC") {
        let o = at + p;
        at = o + 4;
        let Some((size, ps)) = parts(z, o) else { continue };
        let mut prog = Program {
            at: o,
            size,
            stage: Stage::Other(0xff),
            name: name_before(z, o),
            bindings: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            bitcode: 0..0,
            key: record_key(z, o),
        };
        for &(tag, pa, len) in &ps {
            match &tag {
                b"DXIL" => prog.bitcode = pa..pa + len,
                b"ISG1" => prog.inputs = signature(z, pa, len),
                b"OSG1" => prog.outputs = signature(z, pa, len),
                b"PSV0" => {
                    let info = u32_at(z, pa).unwrap_or(0) as usize;
                    if let Some(&s) = z.get(pa + 4 + 24) {
                        prog.stage = Stage::from_byte(s);
                    }
                    let q = pa + 4 + info;
                    let count = u32_at(z, q).unwrap_or(0) as usize;
                    let stride = u32_at(z, q + 4).unwrap_or(0) as usize;
                    if count > 0 && stride >= 20 && q + 8 + count * stride <= pa + len {
                        for i in 0..count {
                            let b = q + 8 + i * stride;
                            let g = |k: usize| u32_at(z, b + 4 * k).unwrap_or(0);
                            prog.bindings.push(Binding {
                                class: ResClass::from_u32(g(0)),
                                space: g(1),
                                lower: g(2),
                                upper: g(3),
                                kind: g(4),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        if !prog.name.is_empty() {
            // A program named in the clear has its name where the key words
            // would be.
            prog.key = ProgramKey::default();
        }
        at = o + size;
        out.push(prog);
    }
    out
}

// --- The program library --------------------------------------------------

/// The unbounded picture ranges' upper bound (`0..=270335` descriptors).
pub const BINDLESS_UPPER: u32 = 270_335;

impl Program {
    /// Whether the program reads material pictures through the bindless
    /// ranges (unbounded Texture2D / Texture2DArray in spaces 3 to 7):
    /// the material pixel programs and nothing else.
    #[must_use]
    pub fn is_bindless(&self) -> bool {
        self.bindings.iter().any(|b| {
            matches!(b.class, ResClass::SrvTyped) && b.upper == BINDLESS_UPPER && b.space >= 3
        })
    }

    /// The vertex programs of surfaces: they output `GPU_PIPELINE_ID`, the
    /// byte offset of the drawn material's record, for the pixel program.
    #[must_use]
    pub fn has_pipeline_id(&self) -> bool {
        self.outputs.iter().any(|o| o.semantic == "GPU_PIPELINE_ID")
    }

    /// The colour targets a pixel program writes (`SV_Target0..`).
    #[must_use]
    pub fn targets(&self) -> usize {
        self.outputs.iter().filter(|o| o.semantic == "SV_Target").count()
    }

    #[must_use]
    pub fn writes(&self, semantic: &str) -> bool {
        self.outputs.iter().any(|o| o.semantic == semantic)
    }

    /// What a pixel program writes, as its signature says (the pass it
    /// belongs to is not stored; the engine picks programs by its own state).
    #[must_use]
    pub fn pixel_outputs(&self) -> PixelOutputs {
        PixelOutputs {
            targets: self.targets() as u8,
            depth: self.writes("SV_Depth"),
            coverage: self.writes("SV_Coverage"),
            material: self.is_bindless(),
        }
    }
}

/// A pixel program's outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixelOutputs {
    pub targets: u8,
    pub depth: bool,
    pub coverage: bool,
    /// Reads material pictures (bindless).
    pub material: bool,
}

/// A vertex program and the pixel programs after it in the zone before the
/// next vertex program (0 to 45 of them; 132 of the 300 groups have one).
/// The zone holds no reference from a program to another or from a material
/// to a program (see `docs/JUP.md`), so this grouping is the only pairing
/// the data gives.
#[derive(Clone, Debug)]
pub struct Group {
    /// Index of the vertex program in the scanned list.
    pub vertex: usize,
    /// Indices of the pixel programs after it.
    pub pixels: Vec<usize>,
}

/// The vertex-plus-pixel groups of a scanned program list, in zone order.
#[must_use]
pub fn groups(programs: &[Program]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (i, p) in programs.iter().enumerate() {
        match p.stage {
            Stage::Vertex => out.push(Group { vertex: i, pixels: Vec::new() }),
            Stage::Pixel => {
                if let Some(g) = out.last_mut() {
                    g.pixels.push(i);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(stage: Stage) -> Program {
        Program {
            at: 0,
            size: 0,
            stage,
            name: String::new(),
            bindings: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            bitcode: 0..0,
            key: ProgramKey::default(),
        }
    }

    #[test]
    fn groups_pair_pixels_with_the_vertex_program_before_them() {
        let list = [
            prog(Stage::Pixel), // before any vertex program: belongs to none
            prog(Stage::Vertex),
            prog(Stage::Pixel),
            prog(Stage::Pixel),
            prog(Stage::Compute),
            prog(Stage::Vertex),
        ];
        let g = groups(&list);
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].vertex, g[0].pixels.clone()), (1, vec![2, 3]));
        assert_eq!((g[1].vertex, g[1].pixels.clone()), (5, vec![]));
    }

    #[test]
    fn record_key_reads_the_words_before_the_container() {
        let mut z = vec![0u8; 64];
        let words: [u64; 8] = [0x8000_0000_0000_2080, 0x10, 12, 3, 0, 0, 0xffff_ffff_ffff_fffc, 0x2060];
        for (i, w) in words.iter().enumerate() {
            z[8 * i..8 * i + 8].copy_from_slice(&w.to_le_bytes());
        }
        z.extend_from_slice(b"DXBC");
        assert_eq!(record_key(&z, 64), ProgramKey { pass_mask: 0x10, family: 12, feature_mask: 3 });
        // Not of the form (no -4): no key.
        z[48] = 0;
        assert_eq!(record_key(&z, 64), ProgramKey::default());
        // Too close to the start of the zone.
        assert_eq!(record_key(&z, 10), ProgramKey::default());
    }
}
