//! Repeated placements drawn as one instanced call (`BO2MP_H1_INSTANCING`).
//!
//! A pass's per-primitive block (`b0`: the world matrix, the lighting
//! coordinates) is the only thing that differs between two placements of
//! one model drawn with the same pipeline, groups and index range, and a
//! plan keeps every object's block side by side at a fixed stride
//! (`world::CbPlan`). So a run of such draws whose blocks follow one another
//! goes as one call with an instance per draw: the vertex program reads its
//! block from a storage array bound at the first draw's block, indexed by
//! the instance (`patch_vertex`), instead of a uniform block at a dynamic
//! offset. Only pipelines whose pixel program does not read `b0` (the
//! pixel stage would need the instance passed down) and whose vertex
//! program does not read the instance itself are instanced.

/// The most draws one instanced call takes (each plan's object buffer is
/// padded by this many blocks so the storage binding never runs past it).
pub const MAX_INSTANCES: u32 = 64;

/// `BO2MP_H1_INSTANCING=0`: every draw its own call (as before).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_H1_INSTANCING").is_ok_and(|v| v.trim() == "0"))
}

/// A translated vertex program reading constant buffer `slot` as a storage
/// array of `stride`-byte blocks indexed by the instance (copied into a
/// private block of the old name at the top of `main`, so the body is
/// unchanged). None when the program does not declare the block as
/// expected, reads the instance itself, or its block is larger than the
/// stride.
pub fn patch_vertex(wgsl: &str, slot: u32, stride: u64) -> Option<String> {
    if wgsl.contains("sv_iid") || stride % 16 != 0 {
        return None;
    }
    let name = format!("cb{slot}");
    let ty = format!("CB{slot}");
    let decl = format!("var<uniform> {name}: {ty};");
    let decl_line = wgsl.lines().find(|l| l.trim_end().ends_with(&decl))?;
    let prefix = decl_line.trim_end().strip_suffix(&decl)?;
    let struct_head = format!("struct {ty} {{ d: array<vec4<u32>, ");
    let regs: u64 = wgsl
        .lines()
        .find_map(|l| l.trim().strip_prefix(&struct_head))
        .and_then(|r| r.split('>').next())
        .and_then(|n| n.trim().parse().ok())?;
    let per = stride / 16;
    if regs > per {
        return None;
    }
    let main = wgsl.lines().find(|l| l.starts_with("@vertex fn main("))?;
    let (new_main, iid) = if main.contains("fn main()") {
        (main.replacen("fn main()", "fn main(@builtin(instance_index) h1_iid: u32)", 1), "h1_iid")
    } else {
        (main.replacen("fn main(", "fn main(@builtin(instance_index) h1_iid: u32, ", 1), "h1_iid")
    };
    let storage = format!(
        "struct {ty}I {{ d: array<vec4<u32>, {per}> }}\n{prefix}var<storage, read> {name}_i: array<{ty}I>;\nvar<private> {name}: {ty};"
    );
    let copy = format!(
        "{new_main}\n    for (var h1_k = 0u; h1_k < {regs}u; h1_k++) {{ {name}.d[h1_k] = {name}_i[{iid}].d[h1_k]; }}"
    );
    let mut out = String::with_capacity(wgsl.len() + 256);
    let mut done_decl = false;
    let mut done_main = false;
    for l in wgsl.lines() {
        if !done_decl && l == decl_line {
            out.push_str(&storage);
            done_decl = true;
        } else if !done_main && l == main {
            out.push_str(&copy);
            done_main = true;
        } else {
            out.push_str(l);
        }
        out.push('\n');
    }
    (done_decl && done_main).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VS: &str = "diagnostic(off, derivative_uniformity);
struct CB0 { d: array<vec4<u32>, 5> }
@group(0) @binding(0) var<uniform> cb0: CB0;
struct CB1 { d: array<vec4<u32>, 8> }
@group(0) @binding(1) var<uniform> cb1: CB1;
fn body() { r0 = cb0.d[2]; }
struct In {
    @location(0) v0: vec4<f32>,
}
struct Out {
    @builtin(position) pos: vec4<f32>,
}
@vertex fn main(input: In) -> Out {
    v0 = bitcast<vec4<u32>>(input.v0);
    body();
}
";

    #[test]
    fn the_object_block_becomes_an_instanced_array() {
        let p = patch_vertex(VS, 0, 256).expect("patched");
        assert!(p.contains("struct CB0I { d: array<vec4<u32>, 16> }"));
        assert!(p.contains("@group(0) @binding(0) var<storage, read> cb0_i: array<CB0I>;"));
        assert!(p.contains("var<private> cb0: CB0;"));
        assert!(p.contains("@vertex fn main(@builtin(instance_index) h1_iid: u32, input: In) -> Out {"));
        assert!(p.contains("for (var h1_k = 0u; h1_k < 5u; h1_k++) { cb0.d[h1_k] = cb0_i[h1_iid].d[h1_k]; }"));
        // The other blocks and the body are untouched.
        assert!(p.contains("@group(0) @binding(1) var<uniform> cb1: CB1;"));
        assert!(p.contains("fn body() { r0 = cb0.d[2]; }"));
        assert!(!p.contains("var<uniform> cb0"));
    }

    #[test]
    fn programs_that_cannot_be_instanced() {
        // Larger than the stride, reading the instance, no such block.
        assert!(patch_vertex(VS, 0, 64).is_none());
        assert!(patch_vertex(&VS.replace("v0: vec4<f32>,", "v0: vec4<f32>, @builtin(instance_index) sv_iid: u32,"), 0, 256).is_none());
        assert!(patch_vertex(VS, 3, 256).is_none());
    }
}
