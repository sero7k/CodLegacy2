use std::sync::atomic::{AtomicU64, Ordering};

use d3d9_sm3::{PassLoweringAbi, PassWgsl};

use super::material_runtime::RuntimeShaderPair;

// Bump when emitted WGSL changes as well as when the binary format changes.
pub const WGSL_CACHE_FORMAT: u32 = 5;

const MAGIC: &[u8; 8] = b"IWLWGSL\n";

static HIT: AtomicU64 = AtomicU64::new(0);
static MISS: AtomicU64 = AtomicU64::new(0);
static IO_NS: AtomicU64 = AtomicU64::new(0);

/// The source of the code a cached module comes out of: the bytecode
/// decoder, the program ABI, the lowering and this file. Any change to it
/// makes new keys, so the entries can live in the folder every build shares
/// (`wgsl_shared`) instead of one per install, where each new build
/// translated every program of a map again (MW3 Terminal 26.8 s).
const CODE: &[&str] = &[
    include_str!("wgsl_disk_cache.rs"),
    include_str!("sm3_wgsl.rs"),
    include_str!("../../../../d3d9_sm3/src/abi.rs"),
    include_str!("../../../../d3d9_sm3/src/bytecode.rs"),
    include_str!("../../../../d3d9_sm3/src/ir.rs"),
    include_str!("../../../../d3d9_sm3/src/lib.rs"),
    include_str!("../../../../d3d9_sm3/src/wgsl.rs"),
    include_str!("../../../../d3d9_decl/src/decl_type.rs"),
    include_str!("../../../../d3d9_decl/src/lib.rs"),
    include_str!("../../../../d3d9_decl/src/usage.rs"),
    include_str!("../../../../d3d9_state/src/address.rs"),
    include_str!("../../../../d3d9_state/src/alpha_test.rs"),
    include_str!("../../../../d3d9_state/src/blend.rs"),
    include_str!("../../../../d3d9_state/src/cmp.rs"),
    include_str!("../../../../d3d9_state/src/cull.rs"),
    include_str!("../../../../d3d9_state/src/filter.rs"),
    include_str!("../../../../d3d9_state/src/lib.rs"),
    include_str!("../../../../d3d9_state/src/render_state.rs"),
    include_str!("../../../../d3d9_state/src/sampler.rs"),
    include_str!("../../../../render_material/src/sm3.rs"),
    include_str!("../../../../render_material/src/sm3_abi.rs"),
    include_str!("../../../../render_material/src/vertex_decl.rs"),
    include_str!("../../../../render_material/src/vertex_layout.rs"),
];

/// Shared across builds (`wgsl_shared`, keys carry `CODE`'s hash), or
/// `CODLEGACY_WGSL_SHARED=0`: one cache per install, the old way.
fn shared() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !codlegacy_env::var("CODLEGACY_WGSL_SHARED").is_ok_and(|v| v.trim() == "0"))
}

fn kind() -> &'static str {
    if shared() { "wgsl_shared" } else { "wgsl" }
}

fn code_hash() -> u64 {
    static HASH: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *HASH.get_or_init(|| {
        let mut h = assets::fnv1a64(b"wgsl translator");
        for text in CODE {
            h = assets::fnv1a64_more(h, text.as_bytes());
        }
        h
    })
}

pub fn stats() -> (u64, u64, f64) {
    (
        HIT.load(Ordering::Relaxed),
        MISS.load(Ordering::Relaxed),
        IO_NS.load(Ordering::Relaxed) as f64 / 1.0e6,
    )
}

pub fn load(pair: RuntimeShaderPair, abi: &PassLoweringAbi) -> Option<PassWgsl> {
    let key = cache_key(pair, abi);
    let started = std::time::Instant::now();
    // (A guest map's baked programs first: one file read start to end,
    // `asset_material::map_pak`; `CODLEGACY_BAKE_PROGRAMS=0`: not.)
    let programs = &asset_material::map_pak::PROGRAMS;
    if let Some(module) = programs.take(&key).as_deref().and_then(decode) {
        IO_NS.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        HIT.fetch_add(1, Ordering::Relaxed);
        return Some(module);
    }
    let bytes = assets::cache_get(kind(), &key)?;
    IO_NS.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    let module = decode(&bytes)?;
    HIT.fetch_add(1, Ordering::Relaxed);
    programs.record(&key, bytes);
    Some(module)
}

pub fn store(pair: RuntimeShaderPair, abi: &PassLoweringAbi, module: &PassWgsl) {
    MISS.fetch_add(1, Ordering::Relaxed);
    let key = cache_key(pair, abi);
    let bytes = encode(module);
    let started = std::time::Instant::now();
    if let Err(error) = assets::cache_put(kind(), &key, &bytes) {
        diag::warn!(World, "wgsl cache store {key}: {error}");
    }
    asset_material::map_pak::PROGRAMS.record(&key, bytes);
    IO_NS.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

fn cache_key(pair: RuntimeShaderPair, abi: &PassLoweringAbi) -> String {
    let mut hash = assets::fnv1a64(&WGSL_CACHE_FORMAT.to_le_bytes());
    hash = assets::fnv1a64_more(hash, &pair.vertex.program_hash.to_le_bytes());
    hash = assets::fnv1a64_more(hash, &pair.pixel.program_hash.to_le_bytes());
    hash = assets::fnv1a64_more(hash, format!("{abi:?}").as_bytes());
    if shared() {
        hash = assets::fnv1a64_more(hash, &code_hash().to_le_bytes());
    }
    format!("{hash:016x}")
}

fn encode(module: &PassWgsl) -> Vec<u8> {
    let source = module.source.as_bytes();
    let mut out = Vec::with_capacity(8 + 24 + source.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&WGSL_CACHE_FORMAT.to_le_bytes());
    out.extend_from_slice(&(module.attribute_count as u32).to_le_bytes());
    out.extend_from_slice(&(module.varying_count as u32).to_le_bytes());
    out.extend_from_slice(&(module.vertex_constant_len as u32).to_le_bytes());
    out.extend_from_slice(&(module.pixel_constant_len as u32).to_le_bytes());
    out.extend_from_slice(&(module.sampler_count as u32).to_le_bytes());
    out.extend_from_slice(&(source.len() as u32).to_le_bytes());
    out.extend_from_slice(source);
    out
}

fn decode(bytes: &[u8]) -> Option<PassWgsl> {
    if bytes.len() < 8 + 24 {
        return None;
    }
    if &bytes[..8] != MAGIC {
        return None;
    }
    let format = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    if format != WGSL_CACHE_FORMAT {
        return None;
    }
    let attribute_count = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    let varying_count = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    let vertex_constant_len = u32::from_le_bytes(bytes[20..24].try_into().ok()?) as usize;
    let pixel_constant_len = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as usize;
    let sampler_count = u32::from_le_bytes(bytes[28..32].try_into().ok()?) as usize;
    let source_len = u32::from_le_bytes(bytes[32..36].try_into().ok()?) as usize;
    let source = bytes.get(36..36 + source_len)?;
    let source = std::str::from_utf8(source).ok()?.to_owned();
    Some(PassWgsl {
        source,
        attribute_count,
        varying_count,
        vertex_constant_len,
        pixel_constant_len,
        sampler_count,
    })
}
