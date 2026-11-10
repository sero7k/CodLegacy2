mod admitted;
mod backend;
mod colour_submit;
mod depth_range;
mod draw;
mod exact_pipeline;
mod exact_pipecache; // bo2mp: the exact pipelines' driver cache on disk
mod floatz;
mod bo2_afterlife; // bo2zm
mod bo2_bloom; // bo2zm
mod bo2_dof; // bo2mp
mod motion; // bo2mp
mod upscale; // bo2mp
mod upscale_caps; // bo2mp: what the adapter can run, checked before anything is made
pub mod vram; // bo2mp: the card's memory, for the quality the renderers apply
mod temporal; // bo2mp: FSR 3.1 and DLSS (upscale.rs falls back to FSR 1)
#[cfg(feature = "dlss")]
mod dlss; // bo2mp: NVIDIA DLSS (cargo feature `dlss`)
mod geometry_diagnostic;
mod h1_native; // bo2mp
mod iw6_native; // bo2mp
mod t7_native; // bo2mp
#[cfg(feature = "jup")]
mod jup_native; // bo2mp
#[cfg(feature = "iw7")]
mod iw7_native; // bo2mp
mod shared_look; // bo2mp: BO2MP_BO2_SHARED_LOOK's post passes

/// bo2mp: whether a game's own renderer drew this frame's world, props and
/// sky (MWR's, `h1_native`; Ghosts', `iw6_native`; Black Ops III's,
/// `t7_native`), so the converted world, Black Ops II's grade and bloom
/// stay out.
pub(crate) fn native_world() -> bool {
    h1_native::active() || iw6_native::active() || t7_native::active() || jup_active() || iw7_active()
}
/// bo2mp: whether a game's own renderer finished this frame's world (the
/// screenshot gate takes it as a submitted world).
pub fn native_world_drawn() -> bool {
    native_world()
}
mod gpu_prepare;
mod gpu_resources;
mod hud_blood;
mod iw_tess;
mod model_lighting_tiles;
mod postfx;
mod postfx_dof;
mod products;
mod resolved_scene;
pub(crate) mod scene_depth;
mod script_grade;
mod shadowmap_spot_gpu;
mod shadowmap_sun_gpu;
mod sm3_wgsl;
mod smodel_cache_gpu;
mod smodel_cached;
mod state;
mod sun_effects;
mod texture_table;

pub use admitted::AdmittedExactPort;
pub use products::ExtractedRenderFrameProducts;
pub use render_backend::overlay::{
    CODE_BASE_LIGHTING_COORDS, CODE_SHADOWMAP_POLYGON_OFFSET, CODE_TEXTURE_OUTDOOR,
    CODE_TRANSPOSE_WORLD_VIEW_PROJECTION0, code_transpose_matrix_row4, code_transpose_matrix_rows,
};
pub use render_frame::{
    RetainedDrawItem, RetainedDrawKind, SUN_SHADOW_CASTER_TECH, SUN_SHADOW_PARTITION_COUNT,
    SurfaceSamplerInputs,
};
pub use render_material::{PackedCodeSamplerLane, RuntimeTextureBinding, TechType};
pub use render_material::{SamplerSource, SamplerTextureDimension};
pub mod gpu_contract {
    pub use render_frame::{
        TEXTURE_TABLE_2D_CAPACITY, TEXTURE_TABLE_3D_CAPACITY, TEXTURE_TABLE_CUBE_CAPACITY,
        TEXTURE_TABLE_SAMPLER_CAPACITY, WgpuBindLayoutEntry, WgpuBindingKind, WgpuLayoutRefusal,
        WgpuPassLayout, WgpuShaderVisibility, WgpuVertexAttribute, WgpuVertexBufferLayout,
        WgpuVertexFormat, derive_wgpu_pass_layout, texture_table_bind_entries,
    };
}
pub use gpu_contract::*;
pub use gpu_prepare::{
    ConstantPackRefusal, PassConstantBuffers, overlay_packed_code_on_banks, split_bind_layout,
};

pub use colour_submit::{
    CODE_TEXTURE_FLOATZ, CODE_TEXTURE_RESOLVED_POST_SUN, CODE_TEXTURE_SHADOWMAP_SPOT,
    CODE_TEXTURE_SHADOWMAP_SUN, ExtractedStaticGeometry, FocusedOwnerSubmitState,
    InstalledRenderWorld, PublishedRenderFrame, RenderFrameData, RenderWorldData,
    bind_group_layout_from_entries, cached_lighting_port_variant, colour_ports_static,
    colour_world_smodel_static, dump_shader_program_names, dump_sorted_material_names,
    emit_focused_owner_submit, vertex_layouts_from_contract,
};
pub(crate) use draw::register_drawsurf_render;
pub use geometry_diagnostic::{
    ExtractedDiagnosticGeometry, ExtractedT6Dynamic, T6DeferredFill, T6DynamicDraw, T6SkinShape,
    T6Skinned, T6StaticDraw, geometry_diagnostic_enabled,
};
pub use gpu_resources::*;
pub use model_lighting_tiles::{ModelLightingTileUpload, ModelLightingTileUploads};
pub use postfx::{ExtractedBlood, ExtractedFilm, ExtractedPostFx};
pub use postfx_dof::{DepthOfField, DofFrame, GLOW_APPLY_MATERIAL, GLOW_SETUP_MATERIAL, GlowFrame};
pub use shadowmap_spot_gpu::*;
pub use shadowmap_sun_gpu::*;
pub use sm3_wgsl::{
    PASS_FRAGMENT_ENTRY, PASS_VERTEX_ENTRY, ValidatedPassWgsl, alpha_test_fragment_entry,
};
pub use smodel_cache_gpu::*;
pub use state::{ChangeState0Host, ChangeState1Host, GfxPassState};

/// bo2mp: whether Modern Warfare III's (2023) own renderer draws the frame
/// (never when the game is built without it, the `jup` feature).
fn jup_active() -> bool {
    #[cfg(feature = "jup")]
    return jup_native::active();
    #[cfg(not(feature = "jup"))]
    false
}

/// bo2mp: whether Infinite Warfare's own renderer draws the frame (never
/// without the `iw7` feature).
fn iw7_active() -> bool {
    #[cfg(feature = "iw7")]
    return iw7_native::active();
    #[cfg(not(feature = "iw7"))]
    false
}
