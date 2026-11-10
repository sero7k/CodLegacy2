//! The game's programs: `zone/code_post_gfx.ff` holds every vertex, pixel and
//! compute program as DXIL in plain `DXBC` containers. They are found
//! (`fastfile_jup::shader::scan`), read (`dxil_sm6::read_part`), lowered to
//! WGSL (`dxil_sm6::wgsl::translate`) and, for a vertex and pixel program
//! that draw together, linked by semantic and given one non-clashing set of
//! bindings ([`link`]).
//!
//! Bindings. The lowering numbers a resource `group` = its register space and
//! `binding` = its lower bound plus 0 (constant buffer), 256 (shader
//! resource), 512 (sampler) or 768 (unordered access). The game's vertex and
//! pixel programs share one root layout, so the same register can be a raw
//! buffer to one stage and a texture to the other (space 0, `t0`); a WGSL bind
//! group layout entry has one type, so [`link`] moves the pixel stage's clashing
//! entries up by [`CLASH_SHIFT`].

use std::path::Path;

use dxil_sm6::wgsl::Translation;
use fastfile_jup::oodle::Decompressor;
use fastfile_jup::shader::{self, Program, Stage};

/// Where the game keeps its programs.
pub const ZONE: &str = "code_post_gfx";

/// The binding offset given to a pixel-stage entry whose register the vertex
/// stage uses as another type.
pub const CLASH_SHIFT: u32 = 1024;

/// The decoded `code_post_gfx` zone and the programs found in it.
pub struct Library {
    pub bytes: Vec<u8>,
    pub programs: Vec<Program>,
}

impl Library {
    /// Reads `<install>/zone/code_post_gfx.ff`.
    pub fn load(install: &Path, oodle: &dyn Decompressor) -> Result<Self, String> {
        let path = install.join("zone").join(format!("{ZONE}.ff"));
        let (_, bytes) = fastfile_jup::container::read_zone(&path, oodle).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self::from_bytes(bytes))
    }

    /// From a zone already decoded (`jup_bench::Ooz`, or a saved decode).
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let programs = shader::scan(&bytes);
        Self { bytes, programs }
    }

    /// The program's stage as the lowering names it.
    pub fn is_graphics(&self, i: usize) -> bool {
        self.programs.get(i).is_some_and(|p| matches!(p.stage, Stage::Pixel | Stage::Vertex))
    }

    /// Program `i` read and lowered.
    pub fn lower(&self, i: usize) -> Result<Translation, String> {
        let p = self.programs.get(i).ok_or_else(|| format!("no program {i}"))?;
        let m = dxil_sm6::read_part(&self.bytes[p.bitcode.clone()]).map_err(|e| format!("program {i}: read: {e}"))?;
        dxil_sm6::wgsl::translate(&m).map_err(|e| format!("program {i}: {e}"))
    }
}

/// What a declared global is, read back from its WGSL declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclKind {
    /// `var<uniform> x: array<vec4<u32>, N>`: N rows of 16 bytes.
    Uniform { rows: u32 },
    /// `var<storage, read> x: array<u32>`: a raw buffer.
    RawBuffer,
    /// `texture_2d<f32>`, `texture_3d<u32>`, ...: dimension and sample type
    /// as written (`2d`, `2d_array`, `3d`, `cube`, `depth_2d`; `f32`/`u32`/`i32`).
    Texture { dim: String, sample: String },
    /// `texture_storage_*`: kept as written.
    StorageTexture(String),
    Sampler { comparison: bool },
    /// A `binding_array` of one of the above, with its count.
    Array { of: Box<DeclKind>, count: u32 },
}

/// One global of a lowered module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    pub group: u32,
    pub binding: u32,
    pub name: String,
    pub kind: DeclKind,
}

fn parse_kind(ty: &str, storage: bool, uniform: bool) -> Option<DeclKind> {
    let ty = ty.trim();
    if let Some(inner) = ty.strip_prefix("binding_array<").and_then(|r| r.strip_suffix('>')) {
        let (of, count) = inner.rsplit_once(',')?;
        return Some(DeclKind::Array { of: Box::new(parse_kind(of, false, false)?), count: count.trim().parse().ok()? });
    }
    if uniform {
        let count = ty.strip_prefix("array<vec4<u32>,")?.strip_suffix('>')?.trim().parse().ok()?;
        return Some(DeclKind::Uniform { rows: count });
    }
    if storage {
        return (ty == "array<u32>").then_some(DeclKind::RawBuffer);
    }
    if ty == "sampler" {
        return Some(DeclKind::Sampler { comparison: false });
    }
    if ty == "sampler_comparison" {
        return Some(DeclKind::Sampler { comparison: true });
    }
    if ty.starts_with("texture_storage") {
        return Some(DeclKind::StorageTexture(ty.to_owned()));
    }
    if let Some(rest) = ty.strip_prefix("texture_depth_") {
        return Some(DeclKind::Texture { dim: format!("depth_{rest}"), sample: "depth".into() });
    }
    let rest = ty.strip_prefix("texture_")?;
    let (dim, sample) = rest.split_once('<')?;
    Some(DeclKind::Texture { dim: dim.to_owned(), sample: sample.strip_suffix('>')?.to_owned() })
}

/// The globals a lowered module declares (its `@group(g) @binding(b) var ...;` lines).
pub fn decls(wgsl: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for line in wgsl.lines() {
        let Some(rest) = line.strip_prefix("@group(") else { continue };
        let Some((group, rest)) = rest.split_once(") @binding(") else { continue };
        let Some((binding, rest)) = rest.split_once(") var") else { continue };
        let (storage, uniform) = (rest.starts_with("<storage"), rest.starts_with("<uniform"));
        // (`<storage, read> name: ...` or `<uniform> name: ...` or ` name: ...`.)
        let rest = if rest.starts_with('<') { &rest[rest.find('>').map_or(0, |p| p + 1)..] } else { rest };
        let Some((name, ty)) = rest.trim().trim_end_matches(';').split_once(':') else { continue };
        let (Ok(group), Ok(binding)) = (group.parse(), binding.parse()) else { continue };
        if let Some(kind) = parse_kind(ty, storage, uniform) {
            out.push(Decl { group, binding, name: name.trim().to_owned(), kind });
        }
    }
    out
}

/// Two programs that draw together, ready for pipelines: the pixel stage's
/// inputs moved onto the vertex stage's output locations, its clashing
/// bindings moved, and the declarations of both.
#[derive(Clone, Debug)]
pub struct Linked {
    pub vertex_wgsl: String,
    pub pixel_wgsl: String,
    pub vertex_decls: Vec<Decl>,
    pub pixel_decls: Vec<Decl>,
    /// Pixel inputs the vertex stage does not write (`NAME index`).
    pub unlinked: Vec<String>,
    /// Pixel bindings moved by [`CLASH_SHIFT`].
    pub shifted: usize,
}

fn element_key(name: &str, index: u32) -> (String, u32) {
    (name.to_ascii_uppercase(), index)
}

/// Links a vertex and a pixel program.
pub fn link(vs: &Translation, ps: &Translation) -> Linked {
    let mut pixel = ps.wgsl.clone();
    // Locations: by (semantic, index).
    let mut unlinked = Vec::new();
    let mut moves: Vec<(u32, u32, u32)> = Vec::new(); // (element id, row, new location)
    for (id, old) in &ps.input_locations {
        let Some(e) = ps.inputs.iter().find(|e| e.id == *id) else { continue };
        let first_old = ps.input_locations.iter().filter(|(i, _)| i == id).map(|(_, l)| *l).min().unwrap_or(*old);
        let want = element_key(&e.name, e.semantic_index);
        let at = vs.outputs.iter().find(|o| element_key(&o.name, o.semantic_index) == want).and_then(|o| vs.output_locations.iter().find(|(i, _)| *i == o.id)).map(|(_, l)| *l);
        match at {
            Some(l) => moves.push((*id, *old - first_old, l + (*old - first_old))),
            None => {
                let tag = format!("{} {}", e.name, e.semantic_index);
                if !unlinked.contains(&tag) {
                    unlinked.push(tag);
                }
            }
        }
    }
    if let Some(start) = pixel.find("struct EntryIn {") {
        let end = pixel[start..].find("\n}").map_or(pixel.len(), |e| start + e);
        let block = pixel[start..end].to_owned();
        let mut rebuilt = String::with_capacity(block.len());
        for line in block.lines() {
            let t = line.trim_start();
            let mut done = false;
            if let Some(r) = t.strip_prefix("@location(")
                && let Some((_, after)) = r.split_once(')')
                && let Some(field) = after.split_whitespace().find(|w| w.starts_with('i') && w.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
            {
                // `i<id>_<row>:`
                let f = field.trim_start_matches('i').trim_end_matches(':').trim_end_matches(',');
                let (id, row) = f.split_once('_').unwrap_or((f, "0"));
                if let (Ok(id), Ok(row)) = (id.parse::<u32>(), row.parse::<u32>())
                    && let Some((_, _, l)) = moves.iter().find(|(i, rr, _)| *i == id && *rr == row)
                {
                    rebuilt.push_str(&format!("  @location({l}){after}\n"));
                    done = true;
                }
            }
            if !done {
                rebuilt.push_str(line);
                rebuilt.push('\n');
            }
        }
        pixel.replace_range(start..end, rebuilt.trim_end_matches('\n'));
    }
    // Bindings: a pixel entry on a register the vertex stage declares as another type moves.
    let vertex_decls = decls(&vs.wgsl);
    let mut pixel_decls = decls(&pixel);
    let mut shifted = 0;
    for d in &mut pixel_decls {
        if vertex_decls.iter().any(|v| v.group == d.group && v.binding == d.binding && v.kind != d.kind) {
            let from = format!("@group({}) @binding({}) var", d.group, d.binding);
            let to = format!("@group({}) @binding({}) var", d.group, d.binding + CLASH_SHIFT);
            pixel = pixel.replacen(&from, &to, 1);
            d.binding += CLASH_SHIFT;
            shifted += 1;
        }
    }
    Linked { vertex_wgsl: vs.wgsl.clone(), pixel_wgsl: pixel, vertex_decls, pixel_decls, unlinked, shifted }
}

/// The pairs of a library that draw together: each pixel program that takes a
/// `GPU_PIPELINE_ID` (a material's record offset), with the nearest earlier
/// vertex program whose outputs cover its inputs by semantic. The game's
/// ubershaders sit in the library right after their vertex program.
pub fn material_pairs(lib: &Library) -> Vec<(usize, usize)> {
    let has = |p: &Program, name: &str| p.inputs.iter().any(|e| e.semantic.eq_ignore_ascii_case(name));
    let mut out = Vec::new();
    for (i, p) in lib.programs.iter().enumerate() {
        if p.stage != Stage::Pixel || !has(p, "GPU_PIPELINE_ID") {
            continue;
        }
        let want: Vec<(String, u32)> = p.inputs.iter().filter(|e| !e.semantic.to_ascii_uppercase().starts_with("SV_")).map(|e| (e.semantic.to_ascii_uppercase(), e.index)).collect();
        let found = (i.saturating_sub(64)..i).rev().find(|&v| {
            let vp = &lib.programs[v];
            vp.stage == Stage::Vertex && want.iter().all(|(n, k)| vp.outputs.iter().any(|o| o.semantic.to_ascii_uppercase() == *n && o.index == *k))
        });
        if let Some(v) = found {
            out.push((v, i));
        }
    }
    out
}

/// A vertex and pixel pair for a full-screen or UI-style draw: the pixel
/// program `ps` and the nearest vertex program before it whose outputs cover
/// its inputs by semantic.
pub fn vertex_for(lib: &Library, ps: usize) -> Option<usize> {
    let p = lib.programs.get(ps)?;
    let want: Vec<(String, u32)> = p.inputs.iter().filter(|e| !e.semantic.to_ascii_uppercase().starts_with("SV_")).map(|e| (e.semantic.to_ascii_uppercase(), e.index)).collect();
    (0..ps).rev().find(|&v| {
        let vp = &lib.programs[v];
        vp.stage == Stage::Vertex && want.iter().all(|(n, k)| vp.outputs.iter().any(|o| o.semantic.to_ascii_uppercase() == *n && o.index == *k))
    })
}
