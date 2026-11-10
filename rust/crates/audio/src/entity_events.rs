use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use asset_game::WeaponSoundSlot;
use assets::PreparedWeapons;
use bevy::prelude::*;
use entity_iw4::{EntityEventKind, LOCAL_SOUND_ENTITY};
use net::{CEntity, LocalPresentClient, PresentedSnapshot};

use crate::{
    Footstep, LandSound, PlayAlias, SoundBank, StepGait, WeaponSound, ent_from_number,
    footstep_aliases, gear_alias, gear_rattle_alias, land_aliases,
};

/// The weapon sound slots an entity event plays: (the world's, the player's
/// own view). `None` for an event that plays no weapon-file sound (the
/// melee hit and miss pick theirs by surface, see `melee_impact_alias`).
pub fn event_sound_slots(event: EntityEventKind) -> Option<(WeaponSoundSlot, WeaponSoundSlot)> {
    Some(match event {
        EntityEventKind::ITEM_PICKUP => (WeaponSoundSlot::Pickup, WeaponSoundSlot::PickupPlayer),
        EntityEventKind::AMMO_PICKUP => (
            WeaponSoundSlot::AmmoPickup,
            WeaponSoundSlot::AmmoPickupPlayer,
        ),
        EntityEventKind::NOAMMO => (WeaponSoundSlot::EmptyFire, WeaponSoundSlot::EmptyFirePlayer),
        EntityEventKind::RELOAD => (WeaponSoundSlot::Reload, WeaponSoundSlot::ReloadPlayer),
        EntityEventKind::RELOAD_FROM_EMPTY => (
            WeaponSoundSlot::ReloadEmpty,
            WeaponSoundSlot::ReloadEmptyPlayer,
        ),
        EntityEventKind::RELOAD_START => (
            WeaponSoundSlot::ReloadStart,
            WeaponSoundSlot::ReloadStartPlayer,
        ),
        EntityEventKind::RELOAD_END => {
            (WeaponSoundSlot::ReloadEnd, WeaponSoundSlot::ReloadEndPlayer)
        }
        EntityEventKind::RAISE_WEAPON => (WeaponSoundSlot::Raise, WeaponSoundSlot::RaisePlayer),
        EntityEventKind::FIRST_RAISE_WEAPON => (
            WeaponSoundSlot::FirstRaise,
            WeaponSoundSlot::FirstRaisePlayer,
        ),
        EntityEventKind::PUTAWAY_WEAPON => {
            (WeaponSoundSlot::Putaway, WeaponSoundSlot::PutawayPlayer)
        }
        EntityEventKind::WEAPON_ALT => {
            (WeaponSoundSlot::AltSwitch, WeaponSoundSlot::AltSwitchPlayer)
        }
        EntityEventKind::PULLBACK_WEAPON => {
            (WeaponSoundSlot::Pullback, WeaponSoundSlot::PullbackPlayer)
        }
        EntityEventKind::PREP_OFFHAND => {
            (WeaponSoundSlot::Pullback, WeaponSoundSlot::PullbackPlayer)
        }
        EntityEventKind::USE_OFFHAND => (WeaponSoundSlot::Fire, WeaponSoundSlot::FirePlayer),
        // The detonator's key (the C4): `detonateSound` / `detonateSoundPlayer`.
        EntityEventKind::DETONATE => (WeaponSoundSlot::Detonate, WeaponSoundSlot::DetonatePlayer),
        EntityEventKind::RECHAMBER_WEAPON => {
            (WeaponSoundSlot::Rechamber, WeaponSoundSlot::RechamberPlayer)
        }
        EntityEventKind::MELEE_SWIPE => (
            WeaponSoundSlot::MeleeSwipe,
            WeaponSoundSlot::MeleeSwipePlayer,
        ),
        _ => return None,
    })
}

/// The weapon-file sound slots a placed-equipment event plays: (the world's,
/// the owner's own view). `projDudSound` has no player variant. `None` for
/// any other event. These events come from the item itself (a thrown
/// claymore, a Bouncing Betty, a dud round), so they are heard where the
/// item is; the owner hears the `Player` alias instead when the weapon file
/// names one.
pub fn equipment_state_slots(event: EntityEventKind) -> Option<(WeaponSoundSlot, WeaponSoundSlot)> {
    Some(match event {
        EntityEventKind::DEPLOY_EQUIPMENT => (WeaponSoundSlot::Deploy, WeaponSoundSlot::DeployPlayer),
        EntityEventKind::FINISH_DEPLOY_EQUIPMENT => (
            WeaponSoundSlot::FinishDeploy,
            WeaponSoundSlot::FinishDeployPlayer,
        ),
        EntityEventKind::BREAKDOWN_EQUIPMENT => {
            (WeaponSoundSlot::Breakdown, WeaponSoundSlot::BreakdownPlayer)
        }
        EntityEventKind::FINISH_BREAKDOWN_EQUIPMENT => (
            WeaponSoundSlot::FinishBreakdown,
            WeaponSoundSlot::FinishBreakdownPlayer,
        ),
        EntityEventKind::PROJ_DUD => (WeaponSoundSlot::ProjDud, WeaponSoundSlot::ProjDud),
        _ => return None,
    })
}

/// Which slot a placed-equipment event plays for this listener: the owner's
/// own view takes the `Player` slot when the weapon file authors one, else
/// the world slot (the item's sound, positioned).
pub fn pick_equipment_state_slot(
    slots: (WeaponSoundSlot, WeaponSoundSlot),
    owner_is_listener: bool,
    player_authored: bool,
) -> (WeaponSoundSlot, bool) {
    if owner_is_listener && player_authored && slots.1 != slots.0 {
        (slots.1, true)
    } else {
        (slots.0, false)
    }
}

fn selected_alias<'a>(
    weapons: &asset_game::WeaponRegistry,
    bank: &'a asset_audio::SoundCatalog,
    weapon: u32,
    event: EntityEventKind,
    player_view: bool,
    knife: bool,
) -> Option<(asset_core::AssetNamespace, &'a str)> {
    let weapon = if matches!(
        event,
        EntityEventKind::MELEE_SWIPE | EntityEventKind::MELEE_HIT | EntityEventKind::MELEE_MISS
    ) {
        // bo2mp: a gun whose melee plays Ghosts' clips plays Ghosts' sounds
        // (its knife swing, the knife's hit on a body, on a surface); a
        // missing alias keeps the Black Ops II sound.
        let melee = weapons.melee_weapon_of(weapon);
        if (weapons.ghosts_melee_sounds_of(weapon) || weapons.ghosts_melee_sounds_of(melee))
            && let Some(found) = ghosts_melee_alias(bank, event, player_view)
        {
            return Some(found);
        }
        melee
    } else {
        weapon
    };
    let (world, player) = match event {
        EntityEventKind::MELEE_HIT => {
            return melee_impact_alias(weapons, bank, weapon, knife, WeaponSoundSlot::MeleeHit);
        }
        EntityEventKind::MELEE_MISS => {
            return melee_impact_alias(weapons, bank, weapon, knife, WeaponSoundSlot::MeleeMiss);
        }
        other => event_sound_slots(other)?,
    };

    // bo2zm: Black Ops II's own knife swing (`wpn_melee_whoosh_plr`) is a
    // placeholder whose one variant names no sound file; the swing it has
    // is the world one (`wpn_melee_whoosh_npc`), so the player hears that.
    if player_view
        && event == EntityEventKind::MELEE_SWIPE
        && let Some((ns, name)) = weapons.weapon_sound_key(weapon, player, bank)
    {
        if !alias_is_silent(bank, ns, name) {
            return Some((ns, name));
        }
        return weapons.weapon_sound_key(weapon, world, bank);
    }
    weapons.weapon_sound_key(weapon, if player_view { player } else { world }, bank)
}

/// bo2mp: Ghosts' sound for a melee event (`asset_game::ghosts_melee`), in
/// the bank under Ghosts' namespace. `None` (Black Ops II's sound plays,
/// logged once for each event) when the bank has none of its aliases.
fn ghosts_melee_alias<'a>(
    bank: &'a asset_audio::SoundCatalog,
    event: EntityEventKind,
    player_view: bool,
) -> Option<(asset_core::AssetNamespace, &'a str)> {
    use asset_game::ghosts_melee::{MeleeSound, pick_event_alias};
    let ns = asset_core::AssetNamespace::Iw6;
    let sound = match event {
        EntityEventKind::MELEE_SWIPE => MeleeSound::Swipe,
        EntityEventKind::MELEE_HIT => MeleeSound::Hit,
        _ => MeleeSound::Miss,
    };
    let found = pick_event_alias(sound, player_view, |a| bank.index_in(ns, a).is_some());
    let Some(alias) = found else {
        static LOGGED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        let bit = 1u8 << (sound as u8);
        if LOGGED.fetch_or(bit, std::sync::atomic::Ordering::Relaxed) & bit == 0 {
            diag::warn!(
                Audio,
                "bo2mp ghosts melee sounds: no Ghosts alias for the {} ({:?}); Black Ops II's plays",
                sound.label(),
                asset_game::ghosts_melee::event_aliases(sound, player_view)
            );
        }
        return None;
    };
    let order = bank.index_in(ns, alias)?;
    Some((bank.namespace_of_alias(order), bank.name_at(order)?))
}

/// A Black Ops II alias whose every variant names no sound file.
fn alias_is_silent(
    bank: &asset_audio::SoundCatalog,
    ns: asset_core::AssetNamespace,
    name: &str,
) -> bool {
    ns == asset_core::AssetNamespace::T6
        && bank.sound_in(ns, name).is_some_and(|sound| {
            !sound.aliases.is_empty()
                && sound
                    .aliases
                    .iter()
                    .all(|a| a.streamed.is_none() && a.file_name.is_none())
        })
}

fn melee_impact_alias<'a>(
    weapons: &asset_game::WeaponRegistry,
    bank: &'a asset_audio::SoundCatalog,
    weapon: u32,
    knife: bool,
    slot: WeaponSoundSlot,
) -> Option<(asset_core::AssetNamespace, &'a str)> {
    let ns = weapons.borrowed_namespace_of(weapon).unwrap_or_default();
    // bo2zm: Black Ops II names its melee sounds as Black Ops does
    // (`wpn_melee_knife_hit_body`, `wpn_melee_hit_other`).
    let t5 = matches!(
        ns,
        asset_core::AssetNamespace::T5 | asset_core::AssetNamespace::T6
    );
    let hit = slot == WeaponSoundSlot::MeleeHit;
    let generic = |knife: bool| {
        let name = match (knife, hit) {
            (true, true) => "melee_knife_hit_body",
            (true, false) => "melee_knife_hit_other",
            (false, true) => "melee_hit",
            (false, false) => "melee_hit_other",
        };
        let name = if t5 {
            format!("wpn_{name}")
        } else {
            name.to_owned()
        };
        let order = bank.index_in(ns, &name)?;
        Some((bank.namespace_of_alias(order), bank.name_at(order)?))
    };
    let own = || weapons.weapon_sound_key(weapon, slot, bank);
    let knife_alias = || knife.then(|| generic(true)).flatten();
    if t5 {
        own().or_else(knife_alias)
    } else {
        knife_alias().or_else(own)
    }
    .or_else(|| generic(false))
}

fn entity_event_sound(
    sound: On<net::EntityEventSound>,
    identities: Query<&CEntity>,
    local: Res<LocalPresentClient>,
    weapons: Option<Res<PreparedWeapons>>,
    bank: Option<Res<SoundBank>>,
    adopted: Option<Res<net::LastAdoptedSnapshot>>,
    announcer: Option<Res<crate::match_voices::AnnouncerRoutes>>,
    mut output: MessageWriter<WeaponSound>,
    mut play: MessageWriter<crate::AliasCommand>,
    mut unvoiced: Local<HashSet<(i32, u32)>>,
) {
    let event = sound.event.event;
    if event == EntityEventKind::SOUND_ALIAS {
        play_cs_sound_alias(
            &sound.event,
            adopted.as_deref(),
            announcer.as_deref(),
            bank.as_deref().map(|bank| &*bank.0),
            &mut play,
        );
        return;
    }
    if event == EntityEventKind::SOUND_ALIAS_AS_MASTER {
        diag::warn!(Audio, "audio: EV_SOUND_ALIAS_AS_MASTER is unsupported");
        return;
    }
    let Ok(identity) = identities.get(sound.entity) else {
        diag::warn!(
            Audio,
            "audio: entity sound has no CEntity identity (typed gap)"
        );
        return;
    };
    let player_view = identity.client() == Some(local.0);
    if event_sound_slots(event).is_none()
        && !matches!(event, EntityEventKind::MELEE_HIT | EntityEventKind::MELEE_MISS)
    {
        diag::warn!(
            Audio,
            "audio: entity sound event {} has no handler (typed gap)",
            event.0
        );
        return;
    }
    let Some(weapons) = weapons.as_deref() else {
        diag::warn!(
            Audio,
            "audio: entity sound weapon is unavailable (typed gap)"
        );
        return;
    };
    if weapons.0.sounds_of(sound.event.payload.weapon).is_none() {
        diag::warn!(
            Audio,
            "audio: entity sound weapon is unavailable (typed gap)"
        );
        return;
    }
    // Offhand view sounds may be authored entirely as animation notetracks.
    // A missing player alias must not fall back to the world throw sound.
    if event == EntityEventKind::USE_OFFHAND
        && weapons
            .0
            .authored_weapon_sound(
                sound.event.payload.weapon,
                if player_view {
                    WeaponSoundSlot::FirePlayer
                } else {
                    WeaponSoundSlot::Fire
                },
            )
            .is_none()
    {
        return;
    }
    let Some((namespace, alias)) = bank.as_deref().and_then(|bank| {
        selected_alias(
            &weapons.0,
            &bank.0,
            sound.event.payload.weapon,
            event,
            player_view,
            sound.event.payload.event_parm == 1,
        )
    }) else {
        // Once for each event and gun: it fired hundreds of times a match.
        if unvoiced.insert((event.0, sound.event.payload.weapon)) {
            diag::warn!(
                Audio,
                "audio: entity sound event {} has no authored alias (weapon {})",
                event.0,
                sound.event.payload.weapon
            );
        }
        return;
    };
    output.write(WeaponSound {
        namespace,
        alias: alias.to_owned(),
        origin_inches: (!player_view
            || matches!(
                event,
                EntityEventKind::MELEE_HIT | EntityEventKind::MELEE_MISS
            ))
        .then_some(sound.event.payload.origin),
        snd_ent: Some(u32::from(identity.number())),
    });
}

fn play_cs_sound_alias(
    payload: &net::DispatchedEntityEvent,
    adopted: Option<&net::LastAdoptedSnapshot>,
    announcer: Option<&crate::match_voices::AnnouncerRoutes>,
    bank: Option<&asset_audio::SoundCatalog>,
    play: &mut MessageWriter<crate::AliasCommand>,
) {
    let index = u8::try_from(payload.payload.event_parm).unwrap_or(0);
    let Some(alias) = adopted.and_then(|snap| snap.sound_alias_name(index).map(str::to_owned))
    else {
        diag::warn!(
            Audio,
            "audio: EV_SOUND_ALIAS CS index {index} is unresolved (typed gap)"
        );
        return;
    };
    let local = payload.payload.number == LOCAL_SOUND_ENTITY;
    diag::event!(
        Audio,
        Debug,
        "cs_sound",
        "audio: cs sound `{alias}` local={local}"
    );
    let (namespace, alias) = alias
        .split_once(':')
        .and_then(|(ns, name)| {
            asset_core::AssetNamespace::parse(ns).map(|ns| (ns, name.to_owned()))
        })
        .unwrap_or((asset_core::AssetNamespace::Iw4, alias));
    let (namespace, alias) = match announcer.and_then(|routes| routes.routed(namespace, &alias)) {
        Some((namespace, routed)) => (namespace, routed.to_owned()),
        None => (namespace, alias),
    };
    let (namespace, alias) = match bank
        .filter(|_| namespace == asset_core::AssetNamespace::T6)
        .and_then(|bank| crate::ui_sounds::ui_sound(bank, &alias))
    {
        Some((ns, to)) => (ns, to.to_owned()),
        None => (namespace, alias),
    };
    let namespace = bank.map_or(namespace, |bank| crate::ui_sounds::bo3_script_alias(bank, namespace, &alias));
    play.write(crate::AliasCommand::Play(PlayAlias {
        namespace,
        alias,
        fallback: None,
        origin_inches: (!local).then_some(payload.payload.origin),
        snd_ent: if local {
            None
        } else {
            ent_from_number(payload.payload.number)
        },
    }));
}

fn movement_sound(
    sound: On<net::EntityMovementSound>,
    identities: Query<&CEntity>,
    local: Res<LocalPresentClient>,
    presented: Res<PresentedSnapshot>,
    mut footsteps: MessageWriter<Footstep>,
    mut gear: MessageWriter<WeaponSound>,
    mut play: MessageWriter<crate::AliasCommand>,
    mut land: MessageWriter<LandSound>,
    bank: Option<Res<SoundBank>>,
) {
    let Ok(identity) = identities.get(sound.entity) else {
        diag::warn!(
            Audio,
            "audio: movement sound has no CEntity identity (typed gap)"
        );
        return;
    };
    let player_view = identity.client() == Some(local.0);
    let quieter = identity
        .client()
        .and_then(|id| presented.player(id))
        .is_some_and(|ps| (ps.perks[0] & playerstate_iw4::PERK_QUIETER) != 0);
    let origin_inches = (!player_view).then_some(sound.event.payload.origin);
    // bo2mp: Black Ops II's bank: its own step or landing for the surface
    // (BO2's list, which every map's surfaces are read in under its rules),
    // or the nearest one it has; never an odd loud default.
    let t6_has = |name: &str| {
        bank.as_deref()
            .is_some_and(|b| b.0.index_in(asset_core::AssetNamespace::T6, name).is_some())
    };
    let bo2_bank = t6_has("fly_step_run_plr_default");
    let snd_ent = Some(u32::from(identity.number()));
    if let Some(index) = sound.event.event.landing_surface_index() {
        if bo2_bank
            && let Some((alias, settled)) = crate::aliases::t6_step_alias(
                &t6_has,
                crate::aliases::T6StepKind::Land,
                index as usize,
                player_view,
            )
        {
            crate::aliases::note_t6_step(index as usize, &alias, settled);
            play.write(crate::AliasCommand::Play(PlayAlias {
                namespace: asset_core::AssetNamespace::T6,
                alias,
                fallback: None,
                origin_inches,
                snd_ent,
            }));
            return;
        }
        let surface_flags = (index as u32) << 20;
        let (alias, fallback) = land_aliases(surface_flags, player_view, quieter);
        land.write(LandSound {
            alias,
            fallback,
            origin_inches,
            snd_ent: Some(u32::from(identity.number())),
        });
        return;
    }
    match sound.event.event {
        EntityEventKind::FOOTSTEP_SPRINT
        | EntityEventKind::FOOTSTEP_RUN
        | EntityEventKind::FOOTSTEP_WALK
        | EntityEventKind::FOOTSTEP_PRONE
        | EntityEventKind::JUMP => {}
        EntityEventKind::MANTLE => {
            let alias = gear_alias(player_view).to_owned();
            let fallback = player_view.then(|| gear_alias(false).to_owned());
            play.write(crate::AliasCommand::Play(PlayAlias {
                namespace: asset_core::AssetNamespace::Iw4,
                alias,
                fallback,
                origin_inches,
                snd_ent: Some(u32::from(identity.number())),
            }));
            return;
        }
        _ => unreachable!("EntityMovementSound must carry a movement event"),
    }
    let gait = match sound.event.event {
        EntityEventKind::FOOTSTEP_SPRINT => StepGait::Sprint,

        EntityEventKind::FOOTSTEP_RUN | EntityEventKind::JUMP => StepGait::Run,
        EntityEventKind::FOOTSTEP_WALK => StepGait::Walk,
        EntityEventKind::FOOTSTEP_PRONE => StepGait::Prone,
        _ => unreachable!("footstep arm is only the four EV_FOOTSTEP_* values plus EV_JUMP"),
    };
    // bo2mp: a jump in Black Ops II is also his launch exertion (its gear
    // rides as the alias's secondary).
    if sound.event.event == EntityEventKind::JUMP
        && bank
            .as_deref()
            .is_some_and(|b| b.0.index_in(asset_core::AssetNamespace::T6, "chr_launch_exert_npc").is_some())
    {
        play.write(crate::AliasCommand::Play(PlayAlias {
            namespace: asset_core::AssetNamespace::T6,
            alias: if player_view { "chr_launch_exert_plr" } else { "chr_launch_exert_npc" }.to_owned(),
            fallback: None,
            origin_inches,
            snd_ent: Some(u32::from(identity.number())),
        }));
    }
    // bo2zm: inside a step trigger (Nuketown's porches, the truck bed) the
    // map's client script sets a sound each of his steps also plays.
    // (Ahead of the BO2-bank step below, which returns early.)
    if player_view
        && let Some(step) = presented
            .snapshot()
            .and_then(|snap| snap.meta.script_dvars(local.0).string("bo2zm_step"))
            .filter(|s| !s.is_empty() && *s != "none")
    {
        // Only the T6 sim sets this dvar, and the alias is in BO2's bank.
        play.write(crate::AliasCommand::Play(PlayAlias {
            namespace: asset_core::AssetNamespace::T6,
            alias: step.to_owned(),
            fallback: None,
            origin_inches: None,
            snd_ent: Some(u32::from(identity.number())),
        }));
    }
    let surf_index = usize::from(sound.event.payload.surf_type);
    if bo2_bank
        && let Some((alias, settled)) = crate::aliases::t6_step_alias(
            &t6_has,
            crate::aliases::T6StepKind::Step(gait),
            surf_index,
            player_view,
        )
    {
        crate::aliases::note_t6_step(surf_index, &alias, settled);
        play.write(crate::AliasCommand::Play(PlayAlias {
            namespace: asset_core::AssetNamespace::T6,
            alias,
            fallback: None,
            origin_inches,
            snd_ent,
        }));
        gear.write(WeaponSound {
            namespace: asset_core::AssetNamespace::Iw4,
            alias: gear_rattle_alias(gait, player_view).to_owned(),
            origin_inches,
            snd_ent,
        });
        return;
    }
    let surface_flags = u32::from(sound.event.payload.surf_type) << 20;
    let (alias, fallback) = footstep_aliases(gait, surface_flags, player_view, quieter);
    footsteps.write(Footstep {
        alias,
        fallback,
        origin_inches,
        snd_ent: Some(u32::from(identity.number())),
    });
    gear.write(WeaponSound {
        namespace: asset_core::AssetNamespace::Iw4,
        alias: gear_rattle_alias(gait, player_view).to_owned(),
        origin_inches,
        snd_ent: Some(u32::from(identity.number())),
    });
}

pub(crate) fn register_entity_event_audio(app: &mut App) {
    app.add_observer(entity_event_sound)
        .add_observer(movement_sound)
        .add_observer(grenade_contact);
}

#[derive(Clone, Debug)]
enum NotetrackSound {
    Bound(usize),
    Unbound(String),
}

#[derive(Clone, Debug)]
struct BoundNotetrack {
    sound: Option<NotetrackSound>,
    rumble: Option<Result<Arc<crate::rumble::Rumble>, String>>,
}

#[derive(Resource, Default)]
pub(crate) struct NotetrackSoundTable {
    owner: Option<(
        Arc<asset_audio::SoundCatalog>,
        Arc<asset_game::WeaponRegistry>,
    )>,
    actions: HashMap<(u32, String), BoundNotetrack>,
    reported: HashSet<(u32, String)>,
}

impl NotetrackSoundTable {
    fn owns(
        &self,
        bank: &Arc<asset_audio::SoundCatalog>,
        weapons: &Arc<asset_game::WeaponRegistry>,
    ) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|(b, w)| Arc::ptr_eq(b, bank) && Arc::ptr_eq(w, weapons))
    }

    fn bind(
        bank: &Arc<asset_audio::SoundCatalog>,
        weapons: &Arc<asset_game::WeaponRegistry>,
    ) -> Self {
        let mut actions = HashMap::new();
        let mut unbound = 0usize;
        let mut rumbles = HashMap::new();
        for weapon in 1..=weapons.len() as u32 {
            let namespace = weapons
                .borrowed_namespace_of(weapon)
                .unwrap_or(asset_core::AssetNamespace::Iw4);
            for (note, action) in weapons.notetrack_actions_of(weapon) {
                let sound = action.sound_alias.as_deref().map(|alias| {
                    let (ns, name) = asset_game::alias_namespace(namespace, alias);
                    match bank.index_in(ns, name) {
                        Some(index) => NotetrackSound::Bound(index),
                        None => {
                            unbound += 1;
                            NotetrackSound::Unbound(alias.to_owned())
                        }
                    }
                });
                actions.insert(
                    (weapon, note.to_owned()),
                    BoundNotetrack {
                        sound,
                        rumble: action.rumble_alias.as_ref().map(|alias| {
                            rumbles
                                .entry((namespace, alias.clone()))
                                .or_insert_with(|| {
                                    crate::rumble::Rumble::prepare(bank, namespace, alias)
                                })
                                .clone()
                        }),
                    },
                );
            }
        }
        Self::bind_ghosts_melee(bank, weapons, &mut actions);
        for ((namespace, alias), rumble) in &rumbles {
            if let Err(error) = rumble {
                diag::warn!(
                    Audio,
                    "audio: {namespace:?} rumble `{alias}` refused during binding: {error}"
                );
            }
        }
        diag::info!(
            Audio,
            "audio: viewmodel notetracks bound to the sound bank: actions={} unbound_sounds={unbound} rumbles={} refused_rumbles={}",
            actions.len(),
            rumbles.len(),
            rumbles.values().filter(|value| value.is_err()).count()
        );
        Self {
            owner: Some((Arc::clone(bank), Arc::clone(weapons))),
            actions,
            reported: HashSet::new(),
        }
    }

    /// bo2mp: the notes of Ghosts' melee clips on every gun that plays
    /// them (`asset_game::ghosts_melee`): the sounds the bank holds under
    /// Ghosts' namespace; a note a gun's own file maps is left to it.
    /// Logs `bo2mp ghosts melee sounds:` (what was found and what is not).
    fn bind_ghosts_melee(
        bank: &Arc<asset_audio::SoundCatalog>,
        weapons: &Arc<asset_game::WeaponRegistry>,
        actions: &mut HashMap<(u32, String), BoundNotetrack>,
    ) {
        use asset_game::ghosts_melee::{self, NoteSound};
        let guns: Vec<u32> = (1..=weapons.len() as u32)
            .filter(|&w| weapons.ghosts_melee_sounds_of(w))
            .collect();
        if guns.is_empty() {
            return;
        }
        let ns = asset_core::AssetNamespace::Iw6;
        let has = |alias: &str| bank.index_in(ns, alias).is_some();
        let bound: Vec<(&str, usize)> = ghosts_melee::MELEE_NOTES
            .iter()
            .filter_map(|note| match ghosts_melee::note_sound(note, has) {
                NoteSound::Alias(alias) => bank.index_in(ns, &alias).map(|index| (*note, index)),
                _ => None,
            })
            .collect();
        for &gun in &guns {
            for &(note, index) in &bound {
                actions
                    .entry((gun, note.to_ascii_lowercase()))
                    .or_insert(BoundNotetrack {
                        sound: Some(NotetrackSound::Bound(index)),
                        rumble: None,
                    });
            }
        }
        let line = ghosts_melee::sounds_report(has, guns.len());
        if ghosts_melee::pick_event_alias(ghosts_melee::MeleeSound::Swipe, true, has).is_some() {
            diag::info!(Audio, "{line}");
        } else {
            diag::warn!(Audio, "{line} (Ghosts' sounds are not in the bank: Black Ops II's play)");
        }
    }

    fn report_once(&mut self, weapon: u32, note: &str, message: impl FnOnce() -> String) {
        if self.reported.insert((weapon, note.to_owned())) {
            diag::warn!(Audio, "{}", message());
        }
    }
}

pub(crate) fn bind_notetrack_sounds(
    bank: Option<Res<SoundBank>>,
    weapons: Option<Res<PreparedWeapons>>,
    mut table: ResMut<NotetrackSoundTable>,
) {
    let (Some(bank), Some(weapons)) = (bank, weapons) else {
        if table.owner.is_some() {
            *table = NotetrackSoundTable::default();
        }
        return;
    };
    if table.owns(&bank.0, &weapons.0) {
        return;
    }
    *table = NotetrackSoundTable::bind(&bank.0, &weapons.0);
}

pub(crate) fn play_viewmodel_notetrack_messages(
    mut notes: MessageReader<crate::ViewmodelNotetracks>,
    generation: Res<frame::WorldGeneration>,
    presented: Res<net::PresentedSnapshot>,
    local: Res<net::LocalPresentClient>,
    weapons: Option<Res<PreparedWeapons>>,
    bank: Option<Res<SoundBank>>,
    mut table: ResMut<NotetrackSoundTable>,
    mut output: MessageWriter<crate::BoundWeaponSound>,
    mut rumbles: MessageWriter<crate::rumble::PlayRumble>,
) {
    let bound_bank = bank
        .as_deref()
        .zip(weapons.as_deref())
        .filter(|(bank, weapons)| table.owns(&bank.0, &weapons.0))
        .map(|(bank, _)| Arc::clone(&bank.0));
    for batch in notes.read() {
        if batch.generation != *generation
            || batch.client != local.0
            || !presented
                .snapshot()
                .and_then(|snapshot| snapshot.meta.for_client(local.0))
                .is_some_and(|meta| {
                    meta.life_sequence == batch.life
                        && meta.lifecycle == sim::ClientLifecycle::Alive
                })
        {
            continue;
        }
        for name in &batch.names {
            apply_viewmodel_notetrack(
                batch.weapon,
                name,
                bound_bank.as_deref(),
                &mut table,
                &mut output,
                &mut rumbles,
            );
        }
    }
}

fn apply_viewmodel_notetrack(
    weapon: u32,
    note: &str,
    bank: Option<&asset_audio::SoundCatalog>,
    table: &mut NotetrackSoundTable,
    output: &mut MessageWriter<crate::BoundWeaponSound>,
    rumbles: &mut MessageWriter<crate::rumble::PlayRumble>,
) {
    if note.eq_ignore_ascii_case("end") {
        return;
    }
    if note.eq_ignore_ascii_case("NVG_on_powerup") || note.eq_ignore_ascii_case("NVG_off_powerdown")
    {
        table.report_once(weapon, note, || {
            format!("audio: NVG notetrack `{note}` has no cgMedia alias (typed gap)")
        });
    }
    let Some(bank) = bank else {
        table.report_once(weapon, note, || {
            format!("audio: notetrack `{note}` dropped — notetracks are not bound to the installed sound bank")
        });
        return;
    };
    let key = (weapon, note.to_ascii_lowercase());
    let Some(action) = table.actions.get(&key).cloned() else {
        // bo2mp: a note of Ghosts' melee clips with no sound of its own
        // (the blade, the contact frame; the swing and the blow are the
        // melee events', or Black Ops II's with the sounds switched off).
        if asset_game::ghosts_melee::is_melee_note(note) {
            return;
        }
        table.report_once(weapon, note, || {
            format!("audio: notetrack `{note}` has no sound or rumble mapping")
        });
        return;
    };
    if let Some(rumble) = &action.rumble {
        match rumble {
            Ok(rumble) => {
                rumbles.write(crate::rumble::PlayRumble {
                    bank_revision: bank.revision(),
                    rumble: Arc::clone(rumble),
                });
            }
            Err(error) => table.report_once(weapon, note, || {
                format!("audio: notetrack `{note}` rumble refused: {error}")
            }),
        }
    }
    match action.sound {
        Some(NotetrackSound::Bound(index)) => {
            output.write(crate::BoundWeaponSound {
                bank_revision: bank.revision(),
                index,
                origin_inches: None,
                snd_ent: Some(crate::SND_ENT_LOCAL),
            });
        }
        Some(NotetrackSound::Unbound(alias)) => {
            table.report_once(weapon, note, || {
                format!(
                    "audio: notetrack `{note}` sound `{alias}` is not in the sound bank (refused when bound)"
                )
            });
        }
        None if action.rumble.is_none() => {
            table.report_once(weapon, note, || {
                format!("audio: notetrack `{note}` has no sound or rumble mapping")
            });
        }
        None => {}
    }
}

fn grenade_contact(
    contact: On<net::EntityGrenadeContact>,
    weapons: Option<Res<PreparedWeapons>>,
    bank: Option<Res<SoundBank>>,
    local: Option<Res<LocalPresentClient>>,
    mut output: MessageWriter<WeaponSound>,
    mut unvoiced: Local<HashSet<(i32, u32)>>,
) {
    let payload = &contact.event.payload;
    if let Some(slots) = equipment_state_slots(contact.event.event) {
        let (Some(weapons), Some(bank)) = (weapons.as_deref(), bank.as_deref()) else {
            return;
        };
        let owner_is_listener = local
            .as_deref()
            .is_some_and(|local| payload.attacker_entity_num == local.0.0 as i32);
        let (slot, own_view) = pick_equipment_state_slot(
            slots,
            owner_is_listener,
            weapons.0.authored_weapon_sound(payload.weapon, slots.1).is_some(),
        );
        let Some((namespace, alias)) = weapons.0.weapon_sound_key(payload.weapon, slot, &bank.0)
        else {
            // Most weapons author none of these: say it once for each.
            if unvoiced.insert((contact.event.event.0, payload.weapon)) {
                diag::info!(
                    Audio,
                    "audio: equipment event {} has no playable alias for weapon {} (slot {slot:?})",
                    contact.event.event.0,
                    payload.weapon
                );
            }
            return;
        };
        if codlegacy_env::var_os("CODLEGACY_AUDIO_EQUIPLOG").is_some() {
            diag::info!(
                Audio,
                "audio: equip state {:?} weapon {} ent {} -> {alias} at {:?}",
                contact.event.event,
                payload.weapon,
                payload.number,
                payload.origin
            );
        }
        output.write(WeaponSound {
            namespace,
            alias: alias.to_owned(),
            origin_inches: (!own_view).then_some(payload.origin),
            snd_ent: if own_view { Some(crate::SND_ENT_LOCAL) } else { ent_from_number(payload.number) },
        });
        return;
    }
    let surf = usize::try_from(payload.event_parm).unwrap_or(usize::from(payload.surf_type));
    let Some((namespace, alias)) = weapons.as_deref().and_then(|weapons| {
        let bank = bank.as_deref()?;
        let alias = weapons
            .0
            .bounce_sound_alias(payload.weapon, surf, &bank.0)?;
        let namespace = weapons
            .0
            .borrowed_namespace_of(payload.weapon)
            .unwrap_or(asset_core::AssetNamespace::Iw4);
        Some((namespace, alias))
    }) else {
        // (The name the weapon gives that surface, if any: no name, or a
        // name the loaded sound banks lack.)
        let named = weapons
            .as_deref()
            .and_then(|w| w.0.bounce_sound_of(payload.weapon, surf).map(str::to_owned));
        diag::warn!(
            Audio,
            "audio: grenade bounce alias for eventParm {} is unavailable (weapon {} names {:?}) (typed gap)",
            payload.event_parm,
            payload.weapon,
            named
        );
        return;
    };
    if codlegacy_env::var_os("CODLEGACY_AUDIO_EQUIPLOG").is_some() {
        diag::info!(
            Audio,
            "audio: equip contact weapon {} ent {} surf {surf} -> {alias} at {:?}",
            payload.weapon,
            payload.number,
            payload.origin
        );
    }
    output.write(WeaponSound {
        namespace,
        alias: alias.to_owned(),
        origin_inches: Some(payload.origin),
        snd_ent: ent_from_number(payload.number),
    });
}

#[cfg(test)]
mod equipment_event_tests {
    use super::*;

    /// Every entity event the equipment path emits plays a weapon-file
    /// sound: the pin pull, the throw, and the detonator key (which used to
    /// reach the audio observer and be dropped as "no handler").
    #[test]
    fn equipment_events_have_sound_slots() {
        assert_eq!(
            event_sound_slots(EntityEventKind::PREP_OFFHAND),
            Some((WeaponSoundSlot::Pullback, WeaponSoundSlot::PullbackPlayer))
        );
        assert_eq!(
            event_sound_slots(EntityEventKind::USE_OFFHAND),
            Some((WeaponSoundSlot::Fire, WeaponSoundSlot::FirePlayer))
        );
        assert_eq!(
            event_sound_slots(EntityEventKind::DETONATE),
            Some((WeaponSoundSlot::Detonate, WeaponSoundSlot::DetonatePlayer))
        );
    }

    /// The placing events (deploy, finish deploy, breakdown, dud) each map to
    /// their weapon-file slots, and are handled by the contact observer, not
    /// the entity-sound one.
    #[test]
    fn placed_equipment_events_have_slots() {
        use WeaponSoundSlot as S;
        let cases = [
            (EntityEventKind::DEPLOY_EQUIPMENT, S::Deploy, S::DeployPlayer),
            (EntityEventKind::FINISH_DEPLOY_EQUIPMENT, S::FinishDeploy, S::FinishDeployPlayer),
            (EntityEventKind::BREAKDOWN_EQUIPMENT, S::Breakdown, S::BreakdownPlayer),
            (
                EntityEventKind::FINISH_BREAKDOWN_EQUIPMENT,
                S::FinishBreakdown,
                S::FinishBreakdownPlayer,
            ),
            (EntityEventKind::PROJ_DUD, S::ProjDud, S::ProjDud),
        ];
        for (event, world, player) in cases {
            assert_eq!(equipment_state_slots(event), Some((world, player)));
            assert!(event_sound_slots(event).is_none());
            assert_eq!(
                entity_iw4::entity_event_action(event),
                Ok(entity_iw4::EntityEventAction::GrenadeContact)
            );
        }
        assert_eq!(equipment_state_slots(EntityEventKind::GRENADE_BOUNCE), None);
    }

    #[test]
    fn owner_hears_player_slot_only_when_authored() {
        use WeaponSoundSlot as S;
        let slots = (S::Deploy, S::DeployPlayer);
        assert_eq!(pick_equipment_state_slot(slots, true, true), (S::DeployPlayer, true));
        assert_eq!(pick_equipment_state_slot(slots, true, false), (S::Deploy, false));
        assert_eq!(pick_equipment_state_slot(slots, false, true), (S::Deploy, false));
        // A dud has one slot for everyone.
        let dud = (S::ProjDud, S::ProjDud);
        assert_eq!(pick_equipment_state_slot(dud, true, true), (S::ProjDud, false));
    }

    /// Everything the dispatcher sends to the sound observer has a slot (or
    /// is one the observer handles by name), so none is dropped as a gap.
    #[test]
    fn every_sound_action_event_is_handled() {
        for code in 0..=0xffu32 {
            let event = EntityEventKind(code as i32);
            if !matches!(
                entity_iw4::entity_event_action(event),
                Ok(entity_iw4::EntityEventAction::Sound)
            ) {
                continue;
            }
            let handled = event_sound_slots(event).is_some()
                || matches!(
                    event,
                    EntityEventKind::MELEE_HIT
                        | EntityEventKind::MELEE_MISS
                        | EntityEventKind::SOUND_ALIAS
                        | EntityEventKind::SOUND_ALIAS_AS_MASTER
                        // (Not weapon-file sounds: outside the equipment audit.)
                        | EntityEventKind::FOLIAGE_SOUND
                        | EntityEventKind::STOP_WEAPON_SOUND
                        | EntityEventKind::STOPSOUNDS
                );
            assert!(handled, "event {code:#x} reaches the sound observer with no handler");
        }
    }
}
