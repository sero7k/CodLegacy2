//! Finished-gun cache: the weapon rows a lane finished with, as JSON.
//!
//! Edges into the mesh and animation catalogs are left unset here and filled
//! again by [`import_weapons`] from the restored catalogs. Effect, tracer,
//! reticle and HUD edges are kept: they name rows of the effect catalogs
//! restored in the same order.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SlotDto {
    view_flash: Option<(u8, u32)>,
    world_flash: Option<(u8, u32)>,
    view_shell_eject: Option<(u8, u32)>,
    world_shell_eject: Option<(u8, u32)>,
    view_last_shot_eject: Option<(u8, u32)>,
    world_last_shot_eject: Option<(u8, u32)>,
    explosion: Option<(u8, u32)>,
    tracer: Option<(u8, u32)>,
}

fn slot_out(s: CombatFxSlots) -> SlotDto {
    let p = |x: Option<Ptr>| x.map(|p| (p.block, p.offset));
    SlotDto {
        view_flash: p(s.view_flash),
        world_flash: p(s.world_flash),
        view_shell_eject: p(s.view_shell_eject),
        world_shell_eject: p(s.world_shell_eject),
        view_last_shot_eject: p(s.view_last_shot_eject),
        world_last_shot_eject: p(s.world_last_shot_eject),
        explosion: p(s.explosion),
        tracer: p(s.tracer),
    }
}

fn slot_in(s: SlotDto) -> CombatFxSlots {
    let p = |x: Option<(u8, u32)>| x.map(|(block, offset)| Ptr { block, offset });
    CombatFxSlots {
        view_flash: p(s.view_flash),
        world_flash: p(s.world_flash),
        view_shell_eject: p(s.view_shell_eject),
        world_shell_eject: p(s.world_shell_eject),
        view_last_shot_eject: p(s.view_last_shot_eject),
        world_last_shot_eject: p(s.world_last_shot_eject),
        explosion: p(s.explosion),
        tracer: p(s.tracer),
    }
}

#[derive(Serialize, Deserialize)]
struct RowDto {
    name: String,
    donor_namespace: Option<crate::AssetNamespace>,
    alternate_weapon: Option<String>,
    alternate_index: u32,
    namespace: crate::AssetNamespace,
    facts: WeaponBodyFacts,
    gun_xmodel: Option<String>,
    hand_xmodel: Option<String>,
    ads_view_models: Vec<String>,
    ads_view_offsets: Vec<Option<asset_model::FpvMountOffset>>,
    world_model: Option<String>,
    attachment_world_mounts: Vec<Option<String>>,
    projectile_model: Option<String>,
    rocket_model: Option<String>,
    knife_xmodel: Option<String>,
    sz_xanims: Vec<Option<String>>,
    sz_xanims_right: Vec<Option<String>>,
    sz_xanims_left: Vec<Option<String>>,
    notetrack_actions: Vec<(String, LinkedNotetrackAction)>,
    ghosts_melee_sounds: bool,
    hide_tags: Vec<String>,
    attachment_view_models: Vec<String>,
    attachment_view_offsets: Vec<Option<asset_model::FpvMountOffset>>,
    attachment_world_models: Vec<String>,
    world_parts: Vec<(String, asset_model::FpvMountOffset)>,
    dual_wield_weapon: Option<String>,
    spawned_grenade: Option<String>,
    iw5_configuration: Option<(u32, Iw5AttachmentSelection)>,
    prepared_attachments: Vec<String>,
    iw5_attachment_slots: Vec<Option<String>>,
    iw5_reload_overrides: Vec<fastfile_iw5::ReloadOverride>,
    iw5_anim_overrides: Vec<LeftoverAnimOverride>,
    iw5_fx_overrides: Vec<Iw5FxOverride>,
    iw5_notetrack_overrides: Vec<Iw5NotetrackOverride>,
    sounds: WeaponSoundAliases,
    combat_fx: WeaponCombatFx,
    reticle: WeaponReticleAssets,
    hud_material_edges: WeaponHudMaterialEdges,
    overlay_material: Option<String>,
    overlay_image: Option<String>,
    overlay_material_from_slot: bool,
    hud_icon: Option<String>,
    hud_icon_from_slot: bool,
    pickup_icon: Option<String>,
    pickup_icon_image: Option<String>,
    pickup_icon_authored: bool,
    pickup_icon_ratio: i32,
    hud_icon_ratio: i32,
    hud_icon_image: Option<String>,
    dpad_icon_image: Option<String>,
    dpad_icon_atlas: Option<[u8; 2]>,
    dpad_icon_ratio: i32,
    kill_icon: Option<String>,
    kill_icon_from_slot: bool,
    kill_icon_image: Option<String>,
    projectile_fx: WeaponProjectileFx,
    proj_trail: Option<String>,
    proj_trail_from_slot: bool,
    proj_beacon: Option<String>,
    proj_beacon_from_slot: bool,
    proj_ignition: Option<String>,
    proj_ignition_from_slot: bool,
    display_name_key: Option<String>,
    guest: guest::GuestRow,
}

fn opt_arr<const N: usize>(v: [Option<String>; N]) -> Vec<Option<String>> {
    v.into_iter().collect()
}

fn arr<const N: usize>(v: Vec<Option<String>>) -> Option<[Option<String>; N]> {
    v.try_into().ok()
}

fn row_out(row: &WeaponRow) -> RowDto {
    let row = row.clone();
    let WeaponRow {
        name,
        donor_namespace,
        alternate_weapon,
        alternate_index,
        namespace,
        facts,
        gun_xmodel,
        hand_xmodel,
        gun_xmodel_edge: _,
        hand_xmodel_edge: _,
        rocket_model_edge: _,
        attachment_view_model_edges: _,
        fpv_hands: _,
        fpv_mount_plan: _,
        ads_view_models,
        ads_view_offsets,
        fpv_mount_plan_ads: _,
        fpv_assemblies: _,
        world_model,
        world_model_edge: _,
        attachment_world_model_edges: _,
        attachment_world_mounts,
        projectile_model,
        projectile_model_edge: _,
        rocket_model,
        knife_xmodel,
        sz_xanims,
        sz_xanim_edges: _,
        sz_xanim_right_edges: _,
        sz_xanim_left_edges: _,
        notetrack_actions,
        ghosts_melee_sounds,
        sz_xanims_right,
        sz_xanims_left,
        hide_tags,
        attachment_view_models,
        attachment_view_offsets,
        attachment_world_models,
        world_parts,
        dual_wield_weapon,
        spawned_grenade,
        iw5_configuration,
        prepared_attachments,
        iw5_attachment_slots,
        iw5_reload_overrides,
        iw5_anim_overrides,
        iw5_fx_overrides,
        iw5_notetrack_overrides,
        sounds,
        combat_fx,
        reticle,
        hud_material_edges,
        overlay_material,
        overlay_image,
        overlay_material_from_slot,
        hud_icon,
        hud_icon_from_slot,
        pickup_icon,
        pickup_icon_image,
        pickup_icon_authored,
        pickup_icon_ratio,
        hud_icon_ratio,
        hud_icon_image,
        dpad_icon_image,
        dpad_icon_atlas,
        dpad_icon_ratio,
        kill_icon,
        kill_icon_from_slot,
        kill_icon_image,
        projectile_fx,
        proj_trail,
        proj_trail_from_slot,
        proj_beacon,
        proj_beacon_from_slot,
        proj_ignition,
        proj_ignition_from_slot,
        display_name_key,
        guest,
    } = row;
    RowDto {
        name: name.clone(),
        donor_namespace,
        alternate_weapon,
        alternate_index,
        namespace,
        facts,
        gun_xmodel,
        hand_xmodel,
        ads_view_models,
        ads_view_offsets,
        world_model,
        attachment_world_mounts,
        projectile_model,
        rocket_model,
        knife_xmodel,
        sz_xanims: opt_arr(sz_xanims),
        sz_xanims_right: opt_arr(sz_xanims_right),
        sz_xanims_left: opt_arr(sz_xanims_left),
        notetrack_actions: notetrack_actions.into_iter().collect(),
        ghosts_melee_sounds,
        hide_tags,
        attachment_view_models,
        attachment_view_offsets,
        attachment_world_models,
        world_parts,
        dual_wield_weapon,
        spawned_grenade,
        iw5_configuration,
        prepared_attachments,
        iw5_attachment_slots: iw5_attachment_slots.into_iter().collect(),
        iw5_reload_overrides,
        iw5_anim_overrides,
        iw5_fx_overrides,
        iw5_notetrack_overrides,
        sounds,
        combat_fx,
        reticle,
        hud_material_edges,
        overlay_material,
        overlay_image,
        overlay_material_from_slot,
        hud_icon,
        hud_icon_from_slot,
        pickup_icon,
        pickup_icon_image,
        pickup_icon_authored,
        pickup_icon_ratio,
        hud_icon_ratio,
        hud_icon_image,
        dpad_icon_image,
        dpad_icon_atlas,
        dpad_icon_ratio,
        kill_icon,
        kill_icon_from_slot,
        kill_icon_image,
        projectile_fx,
        proj_trail,
        proj_trail_from_slot,
        proj_beacon,
        proj_beacon_from_slot,
        proj_ignition,
        proj_ignition_from_slot,
        display_name_key,
        guest,
    }
}

fn row_in(row: RowDto) -> Option<WeaponRow> {
    let sz_xanims = arr(row.sz_xanims)?;
    let sz_xanims_right = arr(row.sz_xanims_right)?;
    let sz_xanims_left = arr(row.sz_xanims_left)?;
    let iw5_attachment_slots = row.iw5_attachment_slots.try_into().ok()?;
    Some(WeaponRow {
        name: row.name,
        donor_namespace: row.donor_namespace,
        alternate_weapon: row.alternate_weapon,
        alternate_index: row.alternate_index,
        namespace: row.namespace,
        facts: row.facts,
        gun_xmodel: row.gun_xmodel,
        hand_xmodel: row.hand_xmodel,
        gun_xmodel_edge: AssetEdge::Absent,
        hand_xmodel_edge: AssetEdge::Absent,
        rocket_model_edge: AssetEdge::Absent,
        attachment_view_model_edges: Vec::new(),
        fpv_hands: [None, None],
        fpv_mount_plan: None,
        ads_view_models: row.ads_view_models,
        ads_view_offsets: row.ads_view_offsets,
        fpv_mount_plan_ads: None,
        fpv_assemblies: [None, None],
        world_model: row.world_model,
        world_model_edge: AssetEdge::Absent,
        attachment_world_model_edges: Vec::new(),
        attachment_world_mounts: row.attachment_world_mounts,
        projectile_model: row.projectile_model,
        projectile_model_edge: AssetEdge::Absent,
        rocket_model: row.rocket_model,
        knife_xmodel: row.knife_xmodel,
        sz_xanims,
        sz_xanim_edges: [AssetEdge::Absent; WEAPON_ANIM_SLOTS],
        sz_xanim_right_edges: [AssetEdge::Absent; WEAPON_ANIM_SLOTS],
        sz_xanim_left_edges: [AssetEdge::Absent; WEAPON_ANIM_SLOTS],
        notetrack_actions: row.notetrack_actions.into_iter().collect(),
        ghosts_melee_sounds: row.ghosts_melee_sounds,
        sz_xanims_right,
        sz_xanims_left,
        hide_tags: row.hide_tags,
        attachment_view_models: row.attachment_view_models,
        attachment_view_offsets: row.attachment_view_offsets,
        attachment_world_models: row.attachment_world_models,
        world_parts: row.world_parts,
        dual_wield_weapon: row.dual_wield_weapon,
        spawned_grenade: row.spawned_grenade,
        iw5_configuration: row.iw5_configuration,
        prepared_attachments: row.prepared_attachments,
        iw5_attachment_slots,
        iw5_reload_overrides: row.iw5_reload_overrides,
        iw5_anim_overrides: row.iw5_anim_overrides,
        iw5_fx_overrides: row.iw5_fx_overrides,
        iw5_notetrack_overrides: row.iw5_notetrack_overrides,
        sounds: row.sounds,
        combat_fx: row.combat_fx,
        reticle: row.reticle,
        hud_material_edges: row.hud_material_edges,
        overlay_material: row.overlay_material,
        overlay_image: row.overlay_image,
        overlay_material_from_slot: row.overlay_material_from_slot,
        hud_icon: row.hud_icon,
        hud_icon_from_slot: row.hud_icon_from_slot,
        pickup_icon: row.pickup_icon,
        pickup_icon_image: row.pickup_icon_image,
        pickup_icon_authored: row.pickup_icon_authored,
        pickup_icon_ratio: row.pickup_icon_ratio,
        hud_icon_ratio: row.hud_icon_ratio,
        hud_icon_image: row.hud_icon_image,
        dpad_icon_image: row.dpad_icon_image,
        dpad_icon_atlas: row.dpad_icon_atlas,
        dpad_icon_ratio: row.dpad_icon_ratio,
        kill_icon: row.kill_icon,
        kill_icon_from_slot: row.kill_icon_from_slot,
        kill_icon_image: row.kill_icon_image,
        projectile_fx: row.projectile_fx,
        proj_trail: row.proj_trail,
        proj_trail_from_slot: row.proj_trail_from_slot,
        proj_beacon: row.proj_beacon,
        proj_beacon_from_slot: row.proj_beacon_from_slot,
        proj_ignition: row.proj_ignition,
        proj_ignition_from_slot: row.proj_ignition_from_slot,
        display_name_key: row.display_name_key,
        guest: row.guest,
    })
}

#[derive(Serialize, Deserialize)]
struct BuildDto {
    rows: Vec<RowDto>,
    slots: Vec<SlotDto>,
    iw5: Vec<(String, Iw5ScopeRow)>,
    item_groups: Vec<(String, String, String)>,
    turrets: Vec<(String, String)>,
    compass: Vec<(String, [String; 2], [i32; 2])>,
    guest_names: bool,
    guest_script: Vec<String>,
    family_tables: Vec<(crate::AssetNamespace, crate::CapturedStringTable)>,
    registry_tables: Vec<(crate::AssetNamespace, crate::CapturedStringTable)>,
    bo2_attachments: Vec<(String, Bo2AttachmentStats)>,
    bo2_sounds: Vec<(String, Vec<(usize, String)>)>,
}

/// The finished weapon catalog, without mesh or animation edges.
pub fn export_weapons(build: &WeaponBuild) -> Result<Vec<u8>, String> {
    let dto = BuildDto {
        rows: build.registry.rows.iter().map(row_out).collect(),
        slots: build.combat_slots.iter().copied().map(slot_out).collect(),
        iw5: build
            .registry
            .iw5_attachments
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        item_groups: build
            .registry
            .item_groups
            .iter()
            .map(|((ns, key), value)| (ns.as_str().to_owned(), key.clone(), value.clone()))
            .collect(),
        turrets: build
            .registry
            .vehicle_turrets
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        compass: build
            .registry
            .vehicle_compass
            .iter()
            .map(|(k, (names, nums))| (k.clone(), names.clone(), *nums))
            .collect(),
        guest_names: build.registry.guest_names,
        guest_script: build.registry.guest_script.clone(),
        family_tables: build.family_tables.clone(),
        registry_tables: build.registry.family_tables.iter().cloned().collect(),
        bo2_attachments: build
            .registry
            .bo2_attachments
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
        bo2_sounds: build
            .registry
            .bo2_attachment_sounds
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.iter()
                        .map(|(slot, alias)| (*slot as usize, alias.clone()))
                        .collect(),
                )
            })
            .collect(),
    };
    serde_json::to_vec(&dto).map_err(|e| e.to_string())
}

fn ns_of(s: &str) -> Option<crate::AssetNamespace> {
    Some(match s {
        "iw4" => crate::AssetNamespace::Iw4,
        "t5" => crate::AssetNamespace::T5,
        "iw5" => crate::AssetNamespace::Iw5,
        "t6" => crate::AssetNamespace::T6,
        "iw6" => crate::AssetNamespace::Iw6,
        "t4" => crate::AssetNamespace::T4,
        "iw3" => crate::AssetNamespace::Iw3,
        "s1" => crate::AssetNamespace::S1,
        "t7" => crate::AssetNamespace::T7,
        "jup" => crate::AssetNamespace::Jup,
        "h1" => crate::AssetNamespace::H1,
        "iw7" => crate::AssetNamespace::Iw7,
        _ => return None,
    })
}

/// Reads [`export_weapons`] and binds mesh and animation edges again.
pub fn import_weapons(
    bytes: &[u8],
    xanims: &crate::XAnimCatalog,
    fpv: &crate::FpvMeshCatalog,
    world: &crate::WorldWeaponCatalog,
) -> Option<WeaponBuild> {
    let dto: BuildDto = serde_json::from_slice(bytes).ok()?;
    if dto.rows.len() != dto.slots.len() {
        return None;
    }
    let rows: Vec<WeaponRow> = dto.rows.into_iter().map(row_in).collect::<Option<_>>()?;
    let mut item_groups = HashMap::new();
    for (ns, key, value) in dto.item_groups {
        item_groups.insert((ns_of(&ns)?, key), value);
    }
    let mut bo2_sounds = HashMap::new();
    for (name, sounds) in dto.bo2_sounds {
        let mut row = Vec::new();
        for (slot, alias) in sounds {
            let slot = WeaponSoundSlot::ALL.get(slot).copied()?;
            row.push((slot, alias));
        }
        bo2_sounds.insert(name, row);
    }
    let mut registry = WeaponRegistry {
        rows,
        world_catalog_identity: 0,
        iw5_attachments: dto.iw5.into_iter().collect(),
        configurations: HashMap::new(),
        by_name: HashMap::new(),
        by_namespaced: HashMap::new(),
        item_groups,
        families: crate::WeaponFamilies::default(),
        alternate_fpv: HashMap::new(),
        fpv_clip_tracks: Arc::default(),
        vehicle_turrets: dto.turrets.into_iter().collect(),
        vehicle_compass: dto
            .compass
            .into_iter()
            .map(|(k, names, nums)| (k, (names, nums)))
            .collect(),
        revision: 0,
        guest_names: dto.guest_names,
        guest_script: dto.guest_script,
        family_tables: Arc::new(dto.registry_tables),
        bo2_attachments: dto.bo2_attachments.into_iter().collect(),
        bo2_attachment_sounds: bo2_sounds,
    };
    registry.rebuild_name_maps();
    registry.families = crate::WeaponFamilies::build(&registry.family_tables, &registry);
    registry.revision = mint_weapon_revision();
    let mut build = WeaponBuild {
        registry,
        combat_slots: dto.slots.into_iter().map(slot_in).collect(),
        family_tables: dto.family_tables,
    };
    build.resolve_sz_xanim_edges(xanims);
    build.resolve_fpv_mesh_edges(fpv);
    build.resolve_world_model_edges(world);
    Some(build)
}
