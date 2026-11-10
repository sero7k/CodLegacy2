# Animation: what lives where

Three floors, deliberately different: the **curve facts** (pure mathematics),
the **runtime tree** (pose state) and **who picked which clip** (the game).

## `anim_iw4` — facts only, `#![no_std]`, no clock and no allocator

* `XAnimParts` — curve storage, unpacking of `half_quat` / `full_quat` and
  quantized translations (`XAnimCalcParts`); translations are **deltas**;
* span/sample/`time_to_frame` — a pure function of (clip, time);
* the blend rule `bind + scale * Δ`, rotation replaced (`DObjCompose`);
* `DObj` — how model, skeleton and attachments compose (0x9c, prefix mapped out);
* `XBoneInfo` — the OBBs for bone hits (`DObjGeom_RayBones`);
* `PartBits` — the six-word requested/ignore mask (`DObjCalcAnimInternal`);
* `PlayerAnimValue` — 10-bit `legsAnim` / `torsoAnim`, a 9-bit index and the
  `0x200` "restart" bit: a restart changes **only** that bit, not the index;
* the leaf time step and the goal-weight ramp are measured IW4 constants;
* script name tables: `ANIM_MT_NAMES`, `ANIM_BODY_PART_NAMES`, `ANIM_COND_NAMES`.

No `Vec`, no `AnimationClip`, no glam matrices and no playback clock here.

## `xmodel_runtime` — a runtime without Bevy and without ECS

A pure evaluator, used by **both** the authority **and** the client:

* `XAnimTreeDefinition` (the immutable hierarchy) + `XAnimTreeRuntime` (state;
  leaf time normalized into `[0,1]`, `update` is the per-leaf advance);
* `DObjAnimRuntime` — `update` / `pose_request` / `compose`; DObj reuse is
  equality of `(eType, model*)`, not "always a fresh one";
* `RetainedModelCapability` — pose, `XBoneInfo`, collSurf/collTris for hitscan;
* `build_body_head_weapon_dobj` — body + head on `j_spine4` + weapon on its tag.

A missing clip **never** turns into the bind pose; additive nodes are a typed
gap, not "ordinary blending".

## Who picks the clip

* `sim` — `player_anim_script.rs`, `mantle_xanim.rs`, `anim_script_gap.rs`:
  what animation the player is in, and it travels into the snapshot as
  semantics, not as a tree;
* `assets` — `XAnimParts` → owning clips;
* `render_frontend/adapters/anim/` + `render_anim` — the viewmodel (`fpv*.rs`,
  `viewmodel_controller.rs`), third person (`third_person.rs`, `remote_body.rs`),
  kick and sway (`view_kick.rs`, `view_sway.rs`), items and projectiles.
  FPV actions come from the presented `weapAnim`, **never** from the
  keyboard; the identity of the FPV mesh is
  `weapon_iw4::get_viewmodel_weapon_index`.
  The FPV consumer reads `PreparedFpv` only, never the weapon registry: views,
  rigs and material bindings are built before Ready; an equip picks one, and a
  mandatory material that failed is a refusal naming model, surface and
  material. Remote kits (`PreparedRemoteKits`: models, hide bits, shared DObj)
  and dropped items are composed at install, not on first sight.

## Ghosts' melee on every gun (`asset_game/src/ghosts_melee.rs`)

First-person knifing is one clip for all guns: Ghosts' `viewmodel_knife_melee_slice`
(all 80-odd Ghosts weapon defs name it in their melee swipe slot; the stab,
`viewmodel_knife_melee_stab`, has `contact@0.167`). It keys Ghosts' arm rig
(`tag_torso`, `tag_player`, `j_shoulder_*`..`j_wrist*`, finger chains,
`tag_weapon_left/right`, `tag_knife_attach`, `tag_weapon`), named as BO2's arms.

* Binding: after `resolve_sz_xanim_edges`, `WeaponRegistry::apply_ghosts_melee`
  points every row's bound melee slots (`Melee`, `MeleeEmpty` -> slice;
  `MeleeCharge`, `MeleeChargeEmpty` -> stab; right/left sets too) at the Ghosts
  clips, so `FpvClipTracks::bind` makes the track -> bone tables as for any
  clip (by name; the Ghosts clips also fall back `tag_weapon` ->
  `tag_weapon_right` -> `j_gun` when a rig has no `tag_weapon`). BO2/BO1 guns
  melee with `knife_mp`/`knife`'s rows (`melee_weapon_of`), guest guns with
  their own. Exempt: shield bashes (clip name has "shield"); T7 and IW7 rows
  (their clips pose their own arm skeleton, `fpv_retarget`).
* Timing is the gun's: `WeaponAnimations::with_ghosts_melee_timing` resamples
  the clip (`AnimClip::retimed`, `HitWarp`: two linear pieces) to
  `iMeleeTime` with the blow on `iMeleeDelay` (charge: `iMeleeChargeTime`,
  `iMeleeChargeDelay`), so the viewmodel controller's rate is 1 and the sim's
  hit frame lands on the swing. The clip's blow is its `contact` note or the
  instant the right arm turns fastest (`AnimClip::hit_time`). No delay or time
  on the gun: native proportions / speed.
* Sounds (`ghosts_melee::{MeleeSound, NoteSound}`, `audio::entity_events`).
  Notes (read from `common_core_mp`): slice `ps_melee_knife_swing_plr@0`; stab
  `melee_start_upper@0`, `melee_knife_stab_upper_plr@.033`, `blade_hide@.133`,
  `contact@.167`, `melee_contact_upper@.2`, `melee_upper_push@.467`,
  `melee_upper_pullout@.6`, `blade_show@.767`, `start_ragdoll@.833`. The retime
  carries them (`HitWarp::retime_note`, same two-piece axis as the poses).
  `iw6_knifeonly_mp`'s weapon file: swing `melee_pistol_swing_npc/_plr`, hit and
  miss empty, notetrack map `melee_knife_stab`, `melee_upper_push`,
  `melee_upper_pullout`, `melee_lower_pullout`. The aliases are in `common_mp`
  (the sound bank's Ghosts source, namespace `Iw6`). A gun whose melee slots
  took the clips (`WeaponRegistry::ghosts_melee_sounds_of`; not Ghosts' own
  guns) plays, through the melee entity events (`selected_alias`, so the sim's
  timing and other players' hearing are unchanged): swipe `melee_knife_swing_plr`
  (own view) / `_npc`; hit (body) `melee_knife_hit_body_plr` / `melee_knife_hit_body`;
  miss (a blow on a surface) `melee_knife_hit_other`. The stab's notes play at
  their warped times through the viewmodel notetrack table (an alias is the
  note, with `_plr` tried second): `melee_start_upper_plr`,
  `melee_knife_stab_upper_plr`, `melee_upper_push`, `melee_upper_pullout`. The
  swing note and `melee_contact_upper` are covered by the events; blade, contact
  and ragdoll notes are silent. No Ghosts alias: Black Ops II's plays and a
  warning is logged once. Log: `bo2mp ghosts melee sounds:`.
* `BO2MP_GHOSTS_MELEE_SOUNDS=0` keeps the Ghosts animation with Black Ops II's
  sounds. `BO2MP_GHOSTS_MELEE=0` restores every gun's own melee clip and sounds.
  Not seen or heard in game.

## Black Ops III guest guns (`assets/src/lane/t7.rs`)

BO3's view clips are BO2's in kind and in timing, so they play by BO2's rules
(rate = clip length / the gun's timer). What was measured on `core_common`:

* `XAnimParts` header: `0x24` loop flag, `0x26` delta flag, `0x30` the ten
  bone counts, `0x44` the kind (1 whole pose: every gun slot; 6 additive: the
  1-bone walk, jump and juke bobs, not mapped), `0x50` frame rate. Reading the
  loop flag at `0x44` made every clip loop: reloads, raises, drops and sprint
  in/out never finished (`action_remaining` is `None` for a looping clip) and
  the ADS scrub's end wrapped to frame 0. Looping ones: sprint, crawl,
  wall-run, swim loops, shotgun `reload_loop`, walks.
* Translations follow BO2: the arms' bones are keyed absolutely (their rest
  offsets are 0), the gun's tags as deltas from the model's rest (bolt 0 at
  rest, -2.8 at the shot), so `bind + Δ` holds.
* `ads_base_up` moves `tag_torso` from the hip to the sights, `ads_base_down`
  back: BO2's `adsUp`/`adsDown`, not swapped.
* Clip / timer ratios (reload 1.1-1.6, raise ~1.7, quick swaps 2.5-3.3) match
  BO2's own guns'.
* The sprint rhythm: `0xa5c` sprint cycle scale (0.95), `0xaa8` dive's; BO3
  has no bob pairs (BO2's 1 is used). Left 0 the step rhythm stopped while
  sprinting.

* Pictures: first-person guns are read at up to 2048 px (`BO2MP_BO3_TEXTURE=<64..4096>`, or `BO2MP_GUN_TEXTURE`); they were
  1024 and the receivers' maps (`i_wpn_t7_arak_metal_c/_n/_g/_s`, plastic) are authored at 2048. Over the 30 guns and their
  attachments' 1061 view materials that is 1.57 GB at 1024 and 1.73 GB at 2048 (most pictures are 1024 or less). Third person
  stays 512, camo patterns 1024. The converted pictures cache (`t7_gun_pics`) keys on the size, so it rebuilds once.

To watch it live: `CODLEGACY_PERF=1` / [`PERF.md`](PERF.md).
