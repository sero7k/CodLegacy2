# Ghosts (IW6 MP, PC x64): Visual Effects and Sound/Ambience Behaviour Spec

Method: static reading of the decompile only. Nothing was run, unpacked or decrypted.
Tags: **[C]** = CONFIRMED by reading code. **[I]** = INFERRED. Addresses appear only in the Evidence section.
Coverage is partial (about 14% of functions are named). Section 9 lists what was not determined.

---------------------------------------------------------------------------
## A. VISUAL EFFECTS

### A1. Data model
- FxEffectDef header is 64 bytes; FxElemDef is 312 bytes (0x138); vel-state sample 96 B; vis-state sample 80 B. These match the repo's layout.rs. [C]
- Element types (draw handler order): 0 billboard sprite, 1 oriented sprite, 2 tail, 3 trail, 4 flare, 5 particle-sim animation, 6 and 7 cloud-like (identity unknown), 8 and 9 clouds (gated by fx_drawClouds), 10 spark fountain, 11 model, 12 omni light, 13 spot light, 14 sound, 15 decal, 16 runner (spawns another effect), 17 vector field. [C for numbering and dispatch; I for 6/7 semantics]
- Types 14 and 17 are never drawn by the generic draw dispatcher. Elements whose life byte is 0, or not yet spawned, are skipped. [C]
- Element lists per effect: types 0..7 on list 0, types 10+ on list 1, types 8..9 on list 2. [C]

### A2. Spawn model
- Each element has a spawn delay: `spawnAt = effectStart + delay.base + ((rand16(seed) * (delay.amp + 1)) >> 16)` (amp 0 means no random part). [C]
- An element whose computed life has already expired at queue time is skipped, unless it has on-impact, on-death or emitted effects. [C]
- Type 14 (sound) elements only fire if spawned within 200 ms of now (late-joining sounds are dropped). [C]
- Type 15 decal, 16 runner and 17 vectorfield have their own spawn functions. Type 13 spotlights go to a deferred list. Type 9 allocates a 0xa10-byte cloud block. Type 10 initialises a spark fountain. [C]
- Per-element deterministic random: `seed = (elemIndexByte * 0x277f + effectDef.ushort@0x26 + elemDef.randomSeed + spawnTime) % 0x3fe9`. All randoms are lookups into a table of 16361 uniform floats (4-byte stride); separate slots in the table give independent random channels (color, size, rotation, velocity axes). Re-evaluating the same element every frame gives identical results. [C] Re-implementation: any table of 16361 uniform values works if the exact original table is unavailable. The original is in the exe image. [I that the exact values do not matter]
- Play API is lock-free: callers push a 0x80-byte command record; the FX system drains it at frame setup. [C]
- Bolt attachment is kept only if the effect has an element of type 3 or 7, or any element whose run-relative mode is "relative to effect/bolt" (flags & 0x1c0 == 0x80). Otherwise the effect is detached and played in world space. [C]

### A3. Per-frame evaluation of an element
- age = now - spawnTime. life = lifeSpan.base + ((rand16(seed) * (lifeSpan.amp + 1)) >> 16). If age >= life: age = 0.99 * life and t = 1 (element is dead and removed elsewhere). Else t = age / life. [C]
- Origin frame, selected by flags & 0x1c0: 0 world/identity, 0x40 relative to spawn point, 0x80 relative to effect/bolt, 0xc0 offset/general, 0x100 camera. [C]
- Spawn offset (flags & 0x30): 0x10 sphere, 0x20 cylinder. Angle = rand * 2pi, radius = rand * amp + base, height likewise. [C]
- Velocity displacement: n = velIntervalCount; s = t * n; i = floor(s); f = (s - i) * n; each axis is blended with weights (n - f) and f over the stored samples; world-space samples (flag 0x2000000) plus local-space samples (flag 0x1000000, rotated by the element axis); the sum is scaled by 1000 (units per second) and added to the origin. Per-axis random scaling comes from independent table slots. [C for the structure; the exact interpolation weights are as read, but the per-sample integration of the "delta" term is I]
- Gravity: element def field at offset 156 (positive = downward). [C for field; I for use as 0.5*g*age^2 added to Z in the displacement stage. The exact integration was not read.]
- Visual state (color, size, scale, rotation, alpha): segment = min(floor(visIntervalCount * t), count - 1); frac = count * t - segment; each channel = lerp(base, amplitude, r) where r is one shared random per group; the result is then lerped between segment and segment+1 by frac. NOTE: "amplitude" is the other end of the range, not a delta. [C]
- Rotation: `rot = initialRot + dur * (rotDelta0 * (f - f*f/2) + rotTotal0 + rotDelta1 * f*f/2)`, with base/amp lerped by a random r. [C]
- Alpha: final alpha *= fadeIn * fadeOut (distance factors, see below). If the material flag (+0xe & 1) is set RGB is multiplied by the same factor (premultiplied-style blend). [C]
- Distance fade: fadeInRange is a FAR fade: opacity = clamp(1 - (d - base)/amp, 0, 1). fadeOutRange is a NEAR fade: opacity rises from 0 at base to 1 at base + amp. Result = min of both. [C]
- fx_alphaThreshold (0..256, default 0): sprites, oriented sprites and tails with alpha <= threshold/255 are not drawn. [C]
- Sorting/blend: the material (blend mode, sort key) is part of the element's material handle; the FX system submits elements to the scene by material and the standard transparent sort applies. No FX-specific sorter was found. [I]
- Soft particles and distortion: r_distortion defaults true; distortion and soft-depth behaviour live in material techniques, not FX code. [I]

### A4. Lighting of particles
- elemLitType (byte 181): 3 = sample light grid once per effect (using the FX average colour; function chosen by r_FXAverageColorFunc: 0 IWrad, 1 legacy, 2 spherical-harmonic 1-coefficient, default 2); the sampled colour is rate-limited to litMaxColorChangePerSec * dt per frame. 4 = per-vertex lit (four corner samples when building sprite vertices). Flag 0x80000 adds dynamic lights at 0.25 weight. [C]
- Blend formula: `rgb *= (1 - b) * unlitScale * sceneScale + b * lit * litHDRScalar` where b = litUnlitBlendFactor (+0x134), litHDRScalar at +0x130; sceneScale comes from the effect context (1.0 if flag 0x800000). [C]
- Flag 0x10000 elements get an extra lit-brightness scale. [C]

### A5. FX dvars and defaults [C]
fx_enable true; fx_draw true; fx_draw_spotLight true; fx_draw_omniLight true; fx_cull_elem_spawn true; fx_cull_elem_draw true; fx_marks true; fx_marks_smodels true; fx_marks_ents true; fx_marks_nearlimit 5 (0..1000, 0 = unlimited); fx_freeze false; fx_visMinTraceDist 80; fx_drawClouds true; fx_deferelem true; fx_draw_simd true; fx_killEffectOnRewind false; fx_alphaThreshold 0; fx_maxNumApexFx 2; r_FXAverageColorFunc 2; r_distortion true.
Glass: glass_fx_chance 0.25; glass_physics_chance 0.15; glass_physics_maxdist 512; glass_max_shatter_fx_per_frame 6.
Weapon-fx: cg_brass true; cg_weapHitCullEnable true; cg_weapHitCullAngle 45; cg_marks_ents_player_only false; useTagFlashSilenced true; cg_cullBullets true; cg_minCullBulletDist 128; cg_cullBulletAngle 50.
Per-frame setup copies fx_enable, fx_draw, spotlight, deferelem and omni dvars into effect system flag bits.

### A6. Weapon effects
**Override lookup [C]**: each weapon def has FXOverride entries (32 bytes: attachment1 u16, attachment2 u16, overrideFX, altmodeFX, fxType). For a given fxType, each entry is scored by how many of its non-zero attachment ids are equipped; the highest score wins; zero matches = no override. Result is altmodeFX if the alt-mode flag is set, else overrideFX. With no override, the WeaponDef default is used. This is the ONLY per-attachment effect replacement mechanism found (silencer flash replacement therefore lives in data). [I for the silencer case]
fxType map [C]: 1 view flash, 2 world flash, 8 view flash while ADS, 3 view shell eject, 4 world shell eject, 5 view last-shot eject, 6 world last-shot eject, 7 view mag eject.
WeaponDef offsets (decimal): viewFlash 184, worldFlash 192, viewFlashADS 200, viewShellEject 608, worldShellEject 616, viewLastShotEject 624, worldLastShotEject 632, viewMagEject 640. [C]

**Fire order [C]**: flash, then secondary effects, then brass (unless the weapon uses notetrack-driven eject, in which case brass is spawned from the animation event), then sound/other.
**Muzzle flash [C]**: world view uses fxType 2. First-person: if ADS fraction is exactly 1.0 (and the "no ADS flash" check is false) try fxType 8 first, else fxType 1. Played attached to tag_flash (tag_flash_silenced when useTagFlashSilenced is set and the weapon is silenced). Turrets and vehicles handled similarly.
**Shell eject [C]**: needs cg_brass. Chooses last-shot variants (5/6) when the event is a last shot and the def has one, else 3/4. Spawned on tag_brass; first-person uses the viewmodel entity. Ejection velocity and spin are authored in the effect, not code. EJECT_BRASS events exist as separate event numbers (35, 38).
**Tracers [C]**: spawn only when shot length > 5 units. Def: material, drawInterval (byte), speed, beamLength, beamWidth, screwRadius, screwDist, 5 colours (16 B each). A local tracer entity is created: start, velocity = dir * speed, length, end time = start + dist * 1000 / speed ms, with random start jitter of (rand() % N)/2 ms. Frequency: a per-entity counter decrements per shot and fires at 0, reloaded from drawInterval (+1 for akimbo to desync). Draw: head = current position, tail = head + dir * min(beamLength, remaining); width = beamWidth, times tracer_thermalWidthMult (2.0) in thermal, clamped to tracer_firstPersonMaxWidth (4.0) for the player's own tracer; thermal uses a thermal tracer material; screw segments = clamp(length * 8 / screwDist, 1, 255); queue capacity 48 per frame. Overrides: explosive bullets (width 15, colours (1,.75,.75,0),(1,.75,.75,1)x4) when tracer_explosiveOverride; Stopping Power (width 6, colours (0,0,0,0),(1,1,1,1),(1,0,0,1),(1,0,0,1),(1,1,0,1)) only when tracer_stoppingPowerOverride (default false).
**Impacts [C]**: effect looked up in an ImpactFx table indexed [row * 38 + surface]; 38 columns = surfaces 0..30 plus flesh variants 31..37. Row chosen by weapon impact type (mapping: 1->0/1, 2->2/3, 3->8/9, 4->4, 5->5/6, 6->7, 7->10, 8->12, 9->13, 10->14; the alternate row depends on a flag bit). Flesh hits pick a sub-row from a flags mask and are suppressed when cg_blood is off. Effects are played oriented to the hit normal; if the target is a moving brush/script mover it is attached to that entity.
**Hit culling [C]**: when cg_weapHitCullEnable and the target is the world or a non-player: skip if dot(viewer->hit direction, hit normal) >= cos(cg_weapHitCullAngle) (back-facing), or the hit point is not potentially visible. Marks follow cg_marks_*.
**Penetration/ricochet [I]**: ricochet has its own event (33); penetration exit uses "_exit" sound aliases; no distinct visual rule beyond the same ImpactFx table was found.

### A7. createfx, exploders, loop vs once
- Scripts call createOneshotEffect / createLoopEffect / createExploder with an fxid string; maps load `maps/createfx/<map>_fx.gsc` and `maps/mp/<map>_fx.gsc`. [C]
- The native `playLoopedFx(fxId, repeatSec, origin, cullDist, [fwd, [up]])` creates an entity of type 9 (loop fx) with fxid in the low byte, repeat interval in ms (rounded, min 1) and cull distance. [C]
- Therefore "loop vs once" is decided by the script's placement call (repeat time), NOT by a flag in the effect def. No code-side rule inspects the effect for looping. [C that no such rule was found; the client handler for entity type 9 was not located, so the replay/cull behaviour is I: replay every repeat ms while within cullDist of the viewer]
- Exploders: EV_EXPLODER event triggers the exploder group's effects by id. Delay and loopsound are script-side fields (createfx data) not present in compiled data. [I]

---------------------------------------------------------------------------
## B. SOUND AND AMBIENCE

### B1. Alias selection and parameters
Alias record is 200 bytes. Fields: runtime sequence counter, volMin/volMax, pitchMin/pitchMax, distMin/distMax, velocityMin, flags, masterPriority, master/slave percent, probability, startDelay, volume-falloff curve, LPF curve, reverb-send curve, SpeakerMap ref, wetMixOverride, OcclusionShape ref, allowDoppler, DopplerPreset ref. [C layout]
Flags: bit 3 (0x08) full dry level; bit 4 (0x10) reverb override (use wetMixOverride); bit 6 (0x40) cull when inside distMin; bits 7-8 file mode (1 loaded, 2/3 streamed); bits 9-15 entity channel. Bit 0 probably looping. [C except bit 0 = I]
**Selection [C]**: weighted random by probability (MSVC LCG rand: seed = seed*0x343FD + 0x269EC3, take (seed>>16)&0x7FFF; reservoir style: accept candidate i if rand15 * accumulatedProb < prob_i * 32768). If the list has more than 2 aliases and the pick is the most recently played (highest sequence), re-roll among the others. The chosen alias gets sequence = max + 1. So there is no immediate repeat for lists larger than 2.
**Per-play values [C]**: volume = clamp01((rand01 * (volMax - volMin) + volMin) * volScale); pitch = rand01 * (pitchMax - pitchMin) + pitchMin. Blending two aliases uses the same formula with factor t. rand01 is a separate LCG.
**Play gating [C]**: restricted channels or flag 0x40: reject if dist^2 < distMin^2 (flag 0x40 only), or dist^2 > distMax^2, or listener speed value < velocityMin. Channel table (channels.def) decides unrestricted/restricted/nopause/replaceOldest/replaceQuieter; priority obeys snd_useHardOuterEntchannelPriorities (true). Loaded vs streamed dispatch by file mode.

### B2. 3D voice update, curves, speaker matrix
- Emitter position = entity position + offset rotated by entity axis. [C]
- t = clamp01((d - distMin) / (distMax - distMin)), with distMin/Max blended between two aliases when blending. [C]
- Volume = piecewise-linear lookup of the volume-falloff curve at t (curve = knot count + (x,y) pairs; beyond last knot returns -1/end value). Reverb send = reverb-send curve if present, else the same value as volume. [C]
- Speaker matrix: azimuth of source relative to listener axes, 5.1 sector blending with pair interpolation at 45/135/225 degrees; zero offset gives 0.35355 across four speakers. Centre/LFE split and omnidirectional blend: snd_omnidirectionalPercentage (0.25) acts as a distance-dependent blend, with per-alias omni distance/percentage fields. [C structure; the exact LFE rule and SpeakerMap table use are I/unknown]
- Dry/wet: dry = 1.0 if alias flag 0x08 else current reverb dry; wet = wetMixOverride if flag 0x10 else current reverb wet; wet = 0 if snd_enableReverb false. Final gains pass dry * volumeCurve and wet * reverbSendCurve, times voice volmod and per-slot scale. [C]
- LPF curve (+0x80) application: not located. [unknown]

### B3. Occlusion
- OcclusionShape (per alias): not-default flag, inner angle, outer angle, outer gain. Gain = 1 inside inner angle, lerp to outer gain between inner and outer, outer gain beyond (angle between emitter forward and the direction to the listener). [C structure; I for exact gain curve being linear]
- World occlusion: a boolean per voice from a world trace emitter(+10 z) -> listener head, only for entity types/channels flagged occlusion-capable; refresh not more often than snd_occlusionDelay (150 ms); one voice is traced per frame (earliest due); results cached per entity (128-entry table). When occluded, an EQ filter from the occlusion table is applied on the channel (frequency 0..20000, filter type lowpass/highpass/lowshelf/highshelf, q, gain). Transition time snd_occlusionLerpTime (500 ms) - usage site not located. [C except lerp usage]

### B4. Doppler [C]
Only voice slots 8..39. Parameters from snd_doppler* dvars or an active global preset, overridden by the alias DopplerPreset (alias allowDoppler must be non-zero). With c = speedOfSound * dt and r = unit vector listener->source:
`num = clamp((r . listenerDelta) * playerVelScale + c, 0.1, 4c)`; `den = clamp(r . sourceDelta + c, 0.1, 4c)`; ratio = num/den. If ratio < 1: t = (1 - ratio)/(pitchMax - 1); ratio = t * pitchMin + (1 - t). Clamp to [pitchMin, pitchMax]; smooth `out = (smoothing * prev + ratio) / (smoothing + 1)`. Falls back to 1.0 when distance <= 0.001, doppler disabled, or alias flag off.
Defaults: snd_dopplerEnable true; BaseSpeedOfSound 900; PlayerVelocityScale 0.1; PitchMin 0.1; PitchMax 4.0; Smoothing 6.0.

### B5. Reverb [C]
- 6 reverb slots (name, active, dry, wet, fade). Current = highest active slot. Set(slot, preset, dry, wet, fadeMs) ramps linearly over fadeMs (min 1). Clear(slot, fadeMs) deactivates and falls back to the next lower active slot. Zones use slot 1; scripted/shock/remote-camera use other slots.
- ReverbPreset (72 B): name + 16 floats; 14 feed the DSP (I3DL2-like). Preset lookup by name, fall back to "default".
- Zone reverb row: name, room type, dry, wet, fade seconds (x1000 ms).
- **ReverbPreset floats to the DSP (read 2026-11-03).** The reverb slot change (0x1406573c0, picks the highest
  active slot and sets the dry/wet ramps) looks the preset up by name
  (0x14049c730, "default" fallback), starts from the DSP block's defaults (0x140495180) and copies 14 of
  the 16 floats into a 0x70-byte block that is handed to the mixer thread (0x1404910a0 -> globals
  0x1474fac80.., picked up by 0x140495730 into a 4-slot ring 0x140497380/0x1404973c0, consumed per
  480-sample block by 0x140497170 -> DSP 0x140494390). Block layout: +0 sample rate 48000.0, +4 fade ms,
  +8 a global; preset floats 0..3 -> +0x10..+0x1c, floats 5..9 -> +0x30..+0x40, floats 11..14 -> +0x54..+0x60,
  float 15 -> +0x6c. Floats 4 and 10 are not used. Defaults: +0x10 0.8, +0x14 0.8, +0x18..+0x30 1.0, +0x34..+0x40
  0, +0x44..+0x50 0.5, +0x54 0, +0x58 1.0, +0x5c 1.0, +0x60 0.5, +0x6c 1.0. Clamps (0x140495210): +0xc..+0x14
  and +0x34..+0x54, +0x60, +0x6c to [0, 1]; +0x18..+0x30 to [0, 4]; +0x58 and +0x5c to [0.1, 16]. In the DSP
  every field is blended toward its target per block (`new = k * target + (1 - k) * old`); +0x58 and +0x5c
  scale the delay lines' lengths (the only use read directly). So the preset is not I3DL2 (no dB, no decay
  seconds): it is the engine's own reverb, eleven 0..1 or 0..4 gains/mixes and two delay-length scales
  (floats 12 and 13). Which gain is which (early/late, damping, diffusion) needs the DSP body traced
  further; not done. Neither `common_mp`, `common_core_mp` nor `mp_frag` holds a `ReverbPreset` (only one
  `ReverbCurve` each). **In the Rust: not mapped**; `BO2MP_IW6_REVERB` stays off (the echo DSP has no
  counterpart for the unnamed gains, and there is no preset in the zones read to drive it).

### B6. Sound tables and zones [C]
Where they are (2026-11-03, staged zones): `StringTable` assets in the localized zones only
(`english/eng_common_mp.ff`: `soundtables/mp_common_{zone,reverb,filter,occlusion}.csv` and
`soundtables/common_mp.csv`; `eng_patch_common_mp.ff`: a newer `common_mp.csv`; `eng_<map>.ff`:
`soundtables/<map>.csv`). The loaders (0x1402933c0: `StringTable_GetAsset("soundtables/common_mp.csv")`
then `"soundtables/%s.csv"`; 0x140293570: the `mp_common_*` four together) find them by name, whichever zone loaded them.
Loaded: soundtables/common_mp.csv then soundtables/<map>.csv (plus zone/occlusion/filter/reverb lists). Row type is column 0: filter_name, zone_name, reverb_name, occlusion_name, whizby_name, mix_name, timescale_name, ambient_name, ambient_event, ambient_element, adsr_name, adsr_zone_player, adsr_zone_npc, END_OF_FILE. Capacity 256 per type; names up to 0x40 chars.
Row formats: occlusion (name, channel, freq, type, col4, col5 - applied if col5 > 0); filter (name, channel, int, type, 3 floats); mix (name, channel, volume, fade); adsr (name + 5 floats); ambient_element (alias, 4 floats, min time, max time in seconds x1000); ambient_event lists elements with weights; zone (name, music alias, ambient, mix, reverb, filter, occlusion, adsr/timescale index; 0x100 = none; "default" fallback).
**Zone apply [C]**: set or clear reverb (clear = 1000 ms); apply zone occlusion entries and filters (two layer slots); reset all channel mixes then apply the zone's mix; music alias hash 0 stops music else plays it crossfading between channel pairs (2,3) and (4,5); set the ambient event list.
**Ambient events [C]**: an element plays at a random 3D position around the listener: distance rand(min,max), azimuth from the angle range; alias via the normal selector; next play = now + rand(timeRange).

### B7. Whizby [C]
Aliases "whizby_near_NN_L/R", "whizby_medium_NN_L/R", "whizby_far_NN_L/R" (NN 0..15) with fallback "whizby". Closest point on the bullet segment to the listener, t restricted to [64, length]; perpendicular dist >= 1. near when dist < 40, medium when 40 <= dist < 80, far when dist <= 160 (defaults, overridable by whizby_name rows). L/R emitters offset +/- width/2 along the bullet direction: near width lerp 400->600 over dist/40, medium 600->1000, far 1000. Variation index drawn from a shuffled bag. Played on the world entity. Gated by snd_newWhizby (true).

### B8. Footsteps, landing, impacts [C]
Aliases are "<base>_<surfaceName>" with "<base>_default" fallback over 31 surfaces: step_walk/run/sprint (+_plr), land_plr, land_damage, slide_start, bullet_small/large/ap (+_exit), grenade/rocket/bullet/trophy explode. Perk/volume dvars: perk_footstepVolumePlayer 0.25, Ally 0.25, Enemy 2.0, SelectiveHearingMin 0.25. cg_footsteps, cg_footstepsSprint, cg_landingSounds, cg_equipmentSounds default true.
Volume-mod groups: soundaliases/volumemodgroups.svmod (up to 128 groups); alias volModIndex picks the group.

### B9. Other snd dvars [C]
snd_volume 1.0; snd_loadFadeTime 500; snd_slaveFadeTime 500; snd_levelFadeTime 250; snd_enable2D/3D/Stream/Reverb/Eq true; snd_cinematicVolumeScale 0.42; snd_omnidirectionalPercentage 0.25; snd_occlusionDelay 150; snd_occlusionLerpTime 500; snd_errorOnMissing false; shock: bg_shock_sound* (dry 1.0, wet 0.5, fades 0.25/2.5/1.5); RemoteCamera dry 0.1 wet 0.5; player_breath_snd_lerp 2.0, player_breath_snd_delay 1.0; cg_hearKillerTime/VictimTime 2000.

### B10. Weapon fire-sound selection [C unless marked]
Read on 2026-11-03 from the client fire path (see the Evidence list).

**WeaponDef sound fields** (byte offsets, as `fastfile_iw6::layout::WeaponDef`
and `walk/weapon.rs`): pickupSound 208, pickupSoundPlayer 216, ammoPickupSound
224, ammoPickupSoundPlayer 232, projectileSound 240, pullbackSound 248 / Player
256, **fireSound 264, fireSoundPlayer 272, fireSoundPlayerAkimbo 280**,
fireLoopSound 288 / Player 296, fireStopSound 304 / Player 312, **fireLastSound
320, fireLastSoundPlayer 328**, emptyFireSound 336 / Player 344, meleeSwipeSound
352 / Player 360, meleeHitSound 368, meleeMissSound 376, rechamberSound 384 /
Player 392, reloadSound 400 / Player 408, reloadEmptySound 416 / Player 424,
reloadStartSound 432 / Player 440, reloadEndSound 448 / Player 456,
detonateSound 464 / Player 472, nightVisionWear 480 / 488, nightVisionRemove
496 / 504, altSwitchSound 512 / 520, raiseSound 528 / 536, firstRaiseSound 544 /
552, putawaySound 560 / 568, scanSound 576, changeVariableZoomSound 584,
bounceSound 592 and rollingSound 600 (per-surface lists); projectile
explosion/dud/ignition sounds on the projectile part; turret overheat and
barrel spin sounds; missile cone sounds. WeaponCompleteDef carries
`soundOverrides` (count + array of 32-byte SoundOverrideEntry: attachment1 u16
@0, attachment2 u16 @2, overrideSound @8, altmodeSound @16, soundType u32 @24)
and notetrack sound overrides.

**Who hears which alias.** The fire event handler computes `localView` = the
event's entity is the client being viewed in first person (or, for a
turret/vehicle, the local player's own). Then:
1. `s = fireSound` (override type 1 first).
2. If `localView` or the "followed in a killcam/remote" flag: `s =
   fireSoundPlayer` if set (override type 2 first); and if the shot is from
   the **left** akimbo gun, `s = fireSoundPlayerAkimbo` if set (override
   type 3 first).
3. Last shot (events EV_FIRE_WEAPON_LASTSHOT 32, ..._LEFT 37, HITCLIENT
   lastshot 40/42, SV lastshot 44/46; for the local player also a
   player-state bit): `last` = override type 4 if any, else (`localView` and
   fireLastSoundPlayer) else fireLastSound; if `last` is set it replaces `s`.
4. Nothing set -> no sound. Third person: played at the gun's tag_flash on
   the entity (falls back to the entity origin); first person: at the
   viewmodel's flash tag, flagged as the player's own (2D-ish) voice.
   Non-local, non-last-shot shots go through a 64-slot per-entity queue
   (one pending fire sound per entity/weapon) before the same function.
So "player vs NPC" is exactly fireSoundPlayer vs fireSound by first-person
view; there is no separate NPC/AI rule in MP.

**Silenced.** No silenced flag or field is read. A suppressor is an
attachment, and its sounds come from the sound-override table: each entry
whose soundType matches is scored by how many of its two attachment ids are
on the gun (attachment ids built from the weapon index bits); the highest
score wins (strictly greater, so the first best entry); `altmodeSound` is used
in alt mode, else `overrideSound`; score 0 = no override. Retail data
(common_core_mp, dumped once with a throwaway probe over the walk's
`WeaponRec::sound_overrides`; the probe was not kept): e.g. p226: att 128 type 1
`weap_p226sup_fire_npc`, type 2 `weap_p226sup_fire_plr`, att 128+8 type 3
`weap_p226sup_fire_aki`; minigun underbarrel entries have only altmodeSound.
No type 4 entries were seen in that zone.

**Tails / distant / echo.** No code chooses a distant or tail alias. Layers
are authored in the alias data: after an alias plays, its
`secondaryAliasName` (+16) is looked up and played with the same position and
parameters (only if both are one-shot or both looping; nesting depth < 11),
and `chainAliasName` (+24) is played when a one-shot voice finishes. Indoor /
outdoor is not chosen in code either (reverb/zone only, B5/B6). [C for the
secondary/chain mechanics; I that retail tails use them]

**Rumble.** After the sound, the weapon's fire rumble (+0x930, attachment
override) plays only for the local view.

**In the Rust.** `audio::select_cg_fire_alias` already makes steps 1-3's
choice for the base gun (akimbo left-hand aside). Guest attachment sounds
(`assets::lane::guest_attach`) were slotted by alias name (`plr` -> player,
`npc` -> world); `BO2MP_IW6_FIRE_SOUND_TYPE=1` slots them by the entry's
soundType instead (1, 2, 3, 4). Default off: on the retail data the names
agree with the types. Since 2026-11-03 the akimbo override (3, `_aki`) has a
slot of its own (`WeaponSoundSlot::FirePlayerAkimbo`), which the left gun's
shot in the shooter's view plays (`BO2MP_IW6_AKIMBO_SOUND`).

---------------------------------------------------------------------------
## 9. NOT DETERMINED
1. Client handler for loop-fx entities (replay, culling, start/stop on repeat).
2. Exact gravity/acceleration integration and bounce/collision of particles.
3. Element types 6 and 7 identity; sprite/oriented corner math; trail and spark-fountain internals.
4. FX sort order and soft-particle/distortion specifics (material-side).
5. Weapon fire-sound selection: now read (B10). Still open: the fire-loop/stop sound path for loop weapons and the local-player last-shot state bit's name.
6. LPF curve application, SpeakerMap table use, LFE rule, music/mixer ducking, adsr and timescale semantics, occlusion lerp usage.
7. Exploder delay/loopsound fields (script-side).
8. The random table values (present in exe image; not extracted).

---------------------------------------------------------------------------
## EVIDENCE (function addresses, image base 0x140000000)
- FX dvars: 0x14033ca90. Per-frame setup: 0x1403620f0. Spawn: 0x14035c150 (runner 14035bc40, decal 1403534d0, sound 14035bf10, vectorfield 14035c010, spark init 359980). Draw table 0x14085abc0; dispatcher 0x140338bd0; draw handlers 336bb0, 336f80, 337210, 337c40, 337490, 338540, 3388c0, 3350e0, 335580, 358a10, 3377c0, 337960, 337aa0.
- Origin frame 0x140367360; offsets 0x1403676e0, 0x140367a40; velocity 0x140367770; vis state 0x140362e70, 0x1403628f0, 0x140362b80; distance fade 0x14033a3f0; lit scale 0x14033a630; light grid 0x140636b00; colour clamp 0x14035ef30; sprite verts 0x140339030. Random table ~0x14085bc60.
- FX play queue: 0x14035a5c0, 0x14035a2c0; wrappers 0x14035a9f0, 0x14035aac0, 0x14035a850, 0x14035a9a0.
- Weapon: override lookup 0x14023c9f0; fire 0x1402a8b00; flash 0x1402a8d80; eject 0x1402a86b0; tag play 0x14026a4b0; impact 0x1402a6bb0/0x1402a9350; hit cull 0x1402a7fb0; tracer spawn 0x1405809b0 (callers 1402a7090), freq 0x140580e60, override 0x140580c70, draw 0x14026ffc0/0x140580870.
- Loop fx builtin: 0x1403cdf10; entity snapshot switch inspected 0x14028e950 (no type 9 handling there).
- Sound: alias select 0x1404e5b40; vol/pitch 0x140656f10; play gating 0x140656400; SND_PlayAlias 0x14065ac00; 3D update 0x14065eee0; curve eval 0x1404e5fd0, 0x1404e5fa0; speaker/LFE 0x140656b30; mixer submit 0x14048fc00; doppler 0x1406606d0; occlusion trace 0x1406599a0, 0x1406611e0, cache 0x140658060, filter apply 0x140659520/0x14065e250; reverb preset lookup 0x14049c730; zone apply 0x1402902f0; ambient 0x14065a0d0; whizby 0x1402b1390 (loader 0x1402901b0, bag 0x140658310).

- Fire sounds (B10): fire handler 0x1402a8b00; sound dispatch 0x1402a8fa0; play 0x1402a88c0; queue 0x1402ad870; getters fireSound 0x14023ce30, fireSoundPlayer 0x14023cf70, akimbo 0x14023d000, last 0x14023cec0; override scorer 0x14023cbb0; last-shot event test 0x140242390; rumble 0x1402a90a0 / 0x14023cd70; secondary alias play 0x14065a8c0; chain alias 0x14065fed0; play 0x14065ac00.

---------------------------------------------------------------------------
## VFX audit addendum (2026-11-04)
- Element type numbering confirmed against the draw dispatch: clouds are 8 and 9 (list 2), not 6 and 7. The lane's raw 6/7 -> cloud mapping predates this; raw 8/9 are now kept (BO2MP_IW6_FX_CLOUDS).
- Spawn path read again: effect create 0x14035ae70, element list setup 0x14035b6f0, element spawn 0x14035c150, looping respawn 0x14035c790 / 0x14035c830 (uses the 0x3fe9-entry random table at 0x14085bc94 for delay and life; same formulas as fx_iw4::iw6). Cull-distance test 0x14035bb50 (squared distance of (spawn range + range delta) times the lowest set bit position of the cull mask against the two stored camera positions).
- Material blend code table (GfxStateBits word 2, 4 bits each): index 0..14 -> D3D11 [1,1,2,3,4,5,6,7,8,9,10,16,17,18,19]; 11..14 are SRC1_COLOR, INV_SRC1_COLOR, SRC1_ALPHA, INV_SRC1_ALPHA, i.e. the `_abl` techniques are dual-source.

---------------------------------------------------------------------------
## C. THE EFFECT SYSTEM, END TO END (2026-11-04, second pass)

Method: the staged C export of the Ghidra decompile (`iw6_decompile/complete/iw6mp64_ship_all.c`, static reading only; nothing
run, unpacked or decrypted by this work). Tags as above: [C] read, [I] inferred. Addresses are in the Evidence list at the end of
this part. It corrects part A where marked **CORRECTS A**.

### C1. Element types and the draw table (CORRECTS A1: 6 and 7 are identified)

`draw table[type]` (18 slots, from the image): 0 billboard sprite, 1 oriented sprite, 2 tail, **3 null (trails are not drawn by the
dispatcher; they have their own pass, C9)**, 4 flare, 5 particle-simulation animation, **6 "Apex Turbulence" (NVIDIA APEX, PhysX),
7 "Nvidia Hair" (HairWorks)**, 8 cloud, 9 spark cloud (a cloud with a position history), 10 spark fountain, 11 model, 12 omni light,
13 spot light, 14 sound (never drawn), 15 decal (own pass), 16 runner, 17 vector field (never drawn). [C]
Types 6 and 7 are third-party middleware hooks (`fx_maxNumApexFx` 2): they draw nothing the Rust can reproduce, and no zone read
uses them. The lane's old 6/7 -> cloud mapping is therefore wrong in kind, and harmless only because no data uses them. [C]

Dispatch gate (every element, every frame): skip types 14 and 17; skip when `visualCount` (def byte 0xb6) is 0 or the element has not
spawned (`spawnTime > now`); the per-element seed is `(seq * 0x277f + effect.seed(u16 @0x26) + def.randomSeed(@0x124) + spawnTime) %
0x3fe9` (all as A2); then **def flag 0x2000 draws only when the view is the secondary one (view byte +0x95 != 0) and flag 0x20000 only
when it is the main one** (the view context is built by the frustum setup; +0x95 is its `param_4 != 0`: the view-model / second
view). The same setup stores five frustum planes (a sixth near plane when the near distance is > 0). [C]

### C2. Per-element preamble (the same in every handler) [C]

`life = delay-free life = lifeSpan.base + ((u16(seed, ch 17) * (amp + 1)) >> 16)` (u16 table entry), `age = now - spawnTime`;
`t = age / life`; when `age >= life`: `age = 0.99 * life`, `t = 1`. Stored in the draw context as `+0x64 life`, `+0x68 age`,
`+0x6c t`. Random channels are the table slots `seed + k` of 4-byte entries; the channel numbers read from the code (byte address
minus the table base, over 4): 0..2 velocity x/y/z, 3..5 angular velocity, 6..8 spawn origin x/y/z, 9 offset yaw (sphere azimuth),
10 offset height (sphere: z), 11 offset radius, 12..14 spawn angles, 15 gravity, 16 reflection/velocity scale, 17 life (u16), 18 delay
(u16), 19 one-shot count, 20 emit distance, 21 visual pick (u16), 22 atlas, 23 colour, 24 initial rotation, 25 rotation delta, 26 size0,
27 size1, 28 scale. **These are exactly `fx_iw4::random` / `h1::ch`: the channel numbering is now READ, not inferred.** [C]

### C3. Origin frame (`flags & 0x1c0`) [C]

* 0: identity axes, origin 0 (world). * 0x40: the effect's frame as it was at spawn (origin `+0x64..` of the effect, axes from its
  spawn quaternion). * 0x80: the effect's current (bolt-following) frame. * 0x100: the camera's origin and view quaternion.
* 0xc0 (offset): origin = effect origin + spawnOrigin sample (x: base `+0x3c`, amplitude `+0x40`, y `+0x44/+0x48`, z `+0x4c/+0x50`, each by
  its own channel) rotated by the frame when flag 0x2 is set; plus the spawn offset vector (flag 0x30: 0x10 sphere
  `dir(z = 2*r10 - 1, azimuth = r9 * 2pi) * (base + r11 * amp)`, 0x20 cylinder `radius (base `+0x54` + r11 * amp `+0x58`)` round the
  frame's 2nd/3rd axes at azimuth r9, height (`+0x5c`, `+0x60`, r10) along its first). **The frame's axes are then built from that
  offset: axis0 = normalised offset (1,0,0 when zero), sphere: axes 1 and 2 any perpendicular pair; cylinder: axis1 = normalised
  (-a.y, a.x, 0) (or (0,-a.z... when |a.z| >= 0.999), axis2 = axis0 x axis1.** `fx_iw4::orientation::get_orientation` already does
  this (IW4's). [C]
* The element's position is `origin + axes * inst.offset` where `inst.offset` (instance +0x18..0x20) was set at spawn
  (`spawnOrigin` sample, then the offset vector expressed back into the frame for the 0x40 mode) and is advanced by the physics step
  (C4). The draw handlers add nothing else (a billboard at draw is just `frame.origin + frame.axes * inst.offset`). [C]
* A bolt is kept only when the effect holds a type 3 or 7 element or an element whose mode is 0x80 (A2). [C]

### C4. The physics step (UNKNOWN 2 of part 9, read) [C for structure and constants, I for the integration order]

Per live element instance (0x60 bytes: next index, spawn time, ..., `+0x18` offset, `+0x28` previous state, `+0x58` previous time) the
client steps it forward in time (the draw loop passes `(now - lastStepTime) / 1000 * def[+0x128]` as the step):

* **Velocity sample** (`FUN_140367770`): `n = velIntervalCount`; `s = (age / life) * n`, `i = floor(s)`, `f = (s - i) * n`; the 96-byte
  sample `i` holds, for local and world frames (flags 0x1000000 local, 0x2000000 world; the local one is rotated by the frame's axes),
  `base + R * amp` for the velocity (+0xc/+0x0 pairs, channels 0..2 per axis) and for the "total delta" term; the result is
  `(velocity_i * (n - f) + velocity_{i+1} * f) * 1000` (units per second) added to the effect's base velocity (`effect+8`). So the
  graph is **per-ms authored, x1000 at use**, and the blend is between consecutive samples. [C]
* **Gravity**: `vel.z += (R15 * gravity.amp(+0xa0) + gravity.base(+0x9c)) * 0.8 * dt_ms` (the field is gravity / 800 units/s2,
  matching `docs/IW6.md`: 0.0025 x 800 = 2 u/s2). [C]
* The displacement is scaled by `R16 * reflection.amp + reflection.base` (def +0xa8/+0xa4, the same pair that scales a bounce) and,
  when the element has a collision box (def +0xf0 != 0) and the speed squared exceeds 1, a trace against the world runs
  (`FUN_365ad0`); a hit reflects the velocity: `v' = v - 2 (n . v) n` about the hit normal with the factor above; a hit with
  `n.z > 0.7` and an already-grounded element comes to rest (`inst.restTime` set, velocity zeroed). Flag 0x200 (die on touch) and
  0x100 (use collision) as in the family. [C structure; the grounded-rest detail I]
* The result is stored back into the frame's local offset (`FUN_367c00`: `(pos - origin) . axes`).
`fx_iw4`'s analytic IW4 position (graph integral at draw time) differs from this in the 0.5 * g * t^2 versus stepped integration only
by the step size; both integrate the same samples. [I]

### C5. Visual state (colour, alpha, sizes, scale, rotation) [C]

`FUN_140362e70`: `seg = min(floor(visIntervalCount * t), count - 1)`; `frac = count * t - seg`; weights `(frac, 1 - frac)` on samples `seg+1`
and `seg` (80 bytes: base block 40, amplitude block 40).
* **Colour** (5 floats: r g b a, unlit HDR scale): `value = (R23 * ampNext + (1 - R23) * baseNext) * frac + (R23 * ampCur + (1 - R23) *
  baseCur) * (1 - frac)` (a lerp from base to amplitude). [C]
* **Sizes and scale are additive, not lerps (CORRECTS A3):** `size0 = ((R26 * ampNext + baseNext) * frac + (R26 * ampCur + baseCur) *
  (1 - frac))`, size1 the same on channel 27 and the second pair, scale on channel 28. This is what `docs/IW6.md` found from the data
  and `fx_iw4::iw6` implements. [C]
* Rotation (angle `+0xa0`): `(R24 * def.initialRot.amp + def.initialRot.base) + ((rotDelta0 * (f1 - f1^2/2) + rotTotal0 + rotDelta1 *
  f1^2 / 2) * age_dt...)` with the sample's own base+R25*amp pairs (`FUN_1403628f0` tail). [C structure]
* Alpha: `alpha *= fadeIn * fadeOut`; the material's `renderFlags` bit 0 (`Material +0xe`) additionally multiplies RGB by the same
  factor (a premultiplied material) (`param_3[0..2] *= fade` only when set). [C]
* **sizes of `size1`**: `size1 = size0` unless def flag **0x10000000** is set (then its own second size) (`FUN_140362850`). [C]
* **Flag 0x10000 (CORRECTS A4):** not a lit-brightness scale. `FUN_14033a630` multiplies **size0 and size1** by `effectDef.scaleBase
  (+0x30) + effectDef.scaleAmp (+0x34) * effect.scaleParam (+0xd8)`: an effect-level script/size parameter. The same value is
  compared with `fx_alphaThreshold` (an element whose scale param is <= the threshold is not drawn). [C]
* Draw order of the culls in every sprite-like handler: frustum (bounding radius max(size0, size1) or the cloud's scale + size,
  unless def flag 0x400 forces 5 planes) -> alpha threshold (`fx_alphaThreshold / 255` against the distance fade, then the lit scale,
  then the sample alpha) -> build. [C]
* Distance fades (`FUN_14033a3f0`): far fade `1 - (d - base) / amp` clamped (0 beyond, 1 inside base); near fade
  `1 - (1 - (d - base2)/amp2)` (0 inside base2, ramps to 1 over amp2); the element's factor is the smaller. [C]

### C6. Colour lighting (`FUN_140362b80`) [C]

`rgb = blend * litColor * rgb * (sceneLit * litHDRScalar(+0x130)) + (1 - blend) * rgb * unlitHDR(sample) * sceneUnlit` with
`blend = litUnlitBlend(+0x134)`; `litColor` = the effect's cached light colour (effect +0x2c) or, for lit type 3, a light-grid sample
(`FUN_14035eec0`, or the rate-limited `FUN_14035ef30`), for lit type 4 one sample per quad corner. Lit type 4 skips the block
(the colour is lit per vertex at build time). `sceneLit` / `sceneUnlit` are **`r_litSurfaceHDRScalar` / `r_unlitSurfaceHDRScalar`**
(both default 1.0, range 0..10) or the view's own copies (`r_surfaceHDRScalarUseTweaks` off: the scene definition's), 1 when def flag
0x800000 is set. The vertex colour written to the buffer is four **unclamped floats** (no 0..1 clamp: HDR goes through). [C]

### C7. Sprite quads [C]

`FUN_140339030(elem, A, B, normal)` builds four vertices round `pos`: `c = cos(rot)`, `s = sin(rot)` (rational-polynomial sine, not a
table), `w = size0 * (B * s + A * c)`, `h = size1 * (A * s - B * c)`, corners `pos +- w +- h`; the packed tangent is `w` normalised, the
packed normal the third axis; texcoords from the atlas (`FUN_14033bbc0`: frame blend between two atlas cells when `def+0xac & 0x10`).
**Both sizes are half extents.** Per type:
* **Billboard (0)**: A = the view's right (the negated view y axis), B = the view's up, normal = the view's backward: the view-plane
  billboard. When `def.flags2 (+4) & 2` and the element is type 0: the **look-at** variant: `d = normalise(camera - pos)`, A and B built
  perpendicular to d (a fixed up hint), normal = d (a true camera-facing sprite, not view-plane parallel). [C]
* **Oriented (1): A = frame axis 0 (forward), B = frame axis 2 (up), normal = frame axis 1 (left)**, using the element's own frame axes
  of C3 (world / spawn / effect / offset per its mode) **with no per-element random angles** (def spawnAngles/angularVelocity are read
  by the model and light handlers only, via `FUN_140367280`: Euler from `angles base + R * amp + (angVel base + R * amp) * time`).
  IW4's oriented sprite uses axes 1 and 2 of the *effect's* axes with its random angles. [C]
* **Tail (2)**: `v = normalise(velocity)` (`FUN_140367770` at the element's age; a zero velocity stays zero and the quad degenerates),
  head at `pos`, `pos' = pos - size1 * v` (the centre, so the quad spans `pos - 2 size1 v .. pos`), side = normalise(cross(camera - pos,
  v)), A = side, B = v, normal = cross(side, v); width half = size0, length half = size1 (always the second size, the flag does not
  matter). `fx_iw4::tail_draw` is identical. [C]
* Elements with def flag 0x1000 also push `(pos, size0 * 16, 1 - alpha)` into a per-frame list (max 255: the sight blocker list). [C]

### C8. Clouds (types 8 and 9): not vertex geometry [C]

A cloud element builds no quad. `FUN_1403350e0` (type 8) / `FUN_140335580` (9) evaluate the same state (C5), require `fx_drawClouds` and a
non-zero **scale**, frustum-cull by a sphere of radius `scale + max(size0, size1)`, count 1 (8) or 3 (9) toward the frame's vertex
budget, take a **particle-cloud record** from the scene (`FUN_140608730` / `FUN_1406087b0`, keyed by the material) and fill it:
colour (rgba as C6), the HDR light colour, **size0 (the particle size), size1, scale (the volume's extent)**, the position, the previous
position (type 8: `pos - lastPos`, i.e. a motion vector for the vertex shader), and the lit-type bits. The quads are generated by the
**`particle_cloud_*` technique sets' vertex shaders** (`particle_cloud_replace*`, `particle_cloud_outdoor_replace*`,
`particle_cloud_sparkf_add*`): each cloud is a volume of particles of size `size0` spread through a sphere of radius `scale`, not
one sprite. On mp_frag: `cloud_dust_mote` size 0.5, scale 6.5..12 (a mote volume 13 to 24 units across, one new cloud every 800 ms,
life 4 to 10 s); `cloud_snowflake_a` etc. Type 9 keeps a 32-entry ring of 0x50-byte samples (position, colour, size, scale per frame) and
emits three of them: now, now - 0.5 * size1 seconds and now - size1 seconds (a streak). [C structure; the vertex shader that scatters
the particles was not read here: INFERRED to be a hash of the vertex index, as IW4's particle cloud]

### C9. Trails (type 3), spark fountains (10), flares (4), lights (12, 13), models (11) [C in outline]

* **Trails** have no draw-table entry: their elements sit on list 0 and are drawn by the trail pass (`FUN_140335de0`), which walks the
  element's chain of samples (instance links), builds the strip from the `FxTrailDef` verts/indices (def +0x118) at the repeat distance and
  uses the element's size/colour per sample. The strip detail was not read (not used on mp_frag).
* **Spark fountain** (`FUN_140358a10`): the element first goes through the cloud record (as type 8), then per spark walks the def's key
  table (4 time keys per spark, atlas-cell choice by column/row bits) to emit its streak. Initialised at spawn (`FUN_140359980`).
* **Flare** (`FUN_140337c40`): a **screen-space** sprite: the element's position is projected with the view's own matrix (view axes at
  view +0x70, projection scale at +0xa4/+0xa8); drawn only when in front (view-space depth > 0). The `FlareDef` (def +0x118) gives a
  base radius, flags (bit 1: scale by distance; bit 2: gradient-lookup by screen position; bit 3: lookup by the angle between the
  flare's own axis and the view direction), up to four gradient tables (angle->alpha, angle->size, x and y screen position -> alpha),
  a distance-fade pair and a rotation (`atan2` of the screen offset when def +4 != 0). Final alpha is multiplied by the sample alpha and
  tested against `fx_alphaThreshold`; two quads' worth of screen-space corners go to `FUN_14033a840`. The occlusion (the element's
  `visibility` byte `inst+1`, from a GPU query of the position at spawn `FUN_140166110`) multiplies in: **spawn-time visibility byte
  `bVar4 < 0x20` is the element's keep test** (types 6 and 7 use the same slot). [C structure]
* **Omni light (12)** (`FUN_140337960`): position, **radius = size0**, colour = rgb * **scale** (the scale channel is the intensity), to
  the omni list when `fx_draw_omniLight`. **Spot light (13)** (`FUN_140337aa0`): position, frame axes 0 and 2, the spot def (`+0x118`), colour =
  rgb * scale, radius size0 (deferred list; `fx_draw_spotLight`). No shadow flag is read here. [C]
* **Model (11)** (`FUN_1403377c0`): transform = Euler(spawn angles + angular velocity * time) in the frame, alpha from the colour sample, drawn
  with the draw flags 0xd00 (0xd01 when def flag 0x800: the view-model pass); flag 0x8000000 makes spawn call the model-physics init. [C]

### C10. Draw state (materials) and the frame [C unless marked]

* Blend state (GfxStateBits word 2, 4 bits each for source and destination colour, 4 for the op, then the alpha pair): the factor table
  is `0 off, 1 ZERO, 2 ONE, 3 SRC_COLOR, 4 INV_SRC_COLOR, 5 SRC_ALPHA, 6 INV_SRC_ALPHA, 7 DEST_ALPHA, 8 INV_DEST_ALPHA, 9 DEST_COLOR, 10
  INV_DEST_COLOR, 11 SRC1_COLOR, 12 INV_SRC1_COLOR, 13 SRC1_ALPHA, 14 INV_SRC1_ALPHA` (D3D11 enum 16..19 for the last four): the `_abl` ("alpha
  blend with bloom": the pixel shader's second output, see C11) techniques use **13/14 = dual-source blending**, the `add` ones
  ONE/ONE (premultiplied: the colour is scaled by alpha by the vertex/pixel stage) or SRC_ALPHA/ONE (`cloud_dust_mote`), the `screen`
  ones **10/2 (INV_DEST_COLOR, ONE)** which is the screen blend `s + d - s*d`.
* **Technique-set families used by effects (mp_frag census):** `effect_{add,blend}_lin_ndw_bl_{blmod,abl}`, `effect_zfeather_*` (soft
  particles: `featherParms` constants (scale, range) fade alpha by the depth gap to the scene), `effect_*falloff*` (alpha by the view angle
  to the sprite / beam; constants `falloffBegin`, `falloffEnd...`), `effect_*eyeoffset*` (the vertex shader pulls the quad toward the eye by
  `eyeOffsetPar.x` units), `effect_*screen*`, `effect_*nofog`, `particle_cloud_*`, `distortion_scale` (refraction). `ndw` = no depth write,
  `bl` = bloom aware, `lin` = linear colour. Depth test on (test3), depth write off, two-sided (cull2 is "none" in the engine's cull
  numbering) except falloff beams (cull1). [C for the state words, I for the token meanings]
* Sort: the quads are appended to buffers per **(material, sort key)** (`FUN_1405bc450` allocates 4 vertices and 6 indices from the scene's
  dynamic buffer for the material); the material's own sort key (34 dust, 47 clouds, 48 effects, 50 some glows) places it in the scene's
  translucent order; **no per-element sort by distance was found**: elements of one effect draw in list order (0, 1, 2 of A1). The
  def's `sortOrder` byte (def +0x120) is kept in the cloud record. [C that no distance sort exists in this code; I that the scene sorts by
  material key only]
* Frame: FX drawing is a job (job commands 0xa..0xd and 0x1b in the per-view setup `FUN_140361fe0`): vertices are built on a worker thread
  into the scene buffers and submitted with the scene's translucent surfaces; the exact post stage (before or after tone mapping) is the
  scene graph's and was **not read** here. [I: HDR, before the tone map, as the surface HDR scalars imply]
* Per-frame dvar flags (A5) as before; `fx_cull_elem_draw` toggles every cull above.

### C11. What stays UNKNOWN after this pass

1. The `_abl` pixel shader's second output (what SRC1_ALPHA reads: see the note in `docs/IW6_NATIVE.md`, "Effect system from the exe"): read by decoding the shader, not the exe.
2. The cloud vertex shaders' scatter (C8) and the trail strip builder (C9).
3. Ghosts' own random table values (the channels are READ; any uniform table serves).
4. The loop-fx entity handler (A9 item 1) and exploder delays.
5. The post stage the effects fall before (C10).

### Evidence (C)

Draw table 0x14085abc0 (entries 0x140336bb0, 0x140336f80, 0x140337210, null, 0x140337c40, 0x140337490, 0x140338540 "Apex Turbulence",
0x1403388c0 "Nvidia Hair", 0x1403350e0, 0x140335580, 0x140358a10, 0x1403377c0, 0x140337960, 0x140337aa0). Dispatcher 0x140338bd0 (flag gate on
view +0x95); alpha threshold test 0x140338cb0; frustum/near/far fades 0x14033a1b0/0x14033a210/0x14033a280/0x14033a3f0; effect-scale size
0x14033a630; sprite vertex build 0x140339030; atlas 0x14033bbc0; origin frame 0x140367360 (spawn-origin sample 0x1403676e0, offset 0x140367a40,
sphere dir 0x1403577c0, frame-rotate 0x140368110); spawn instance offset 0x14035caa0; velocity sample 0x140367770; physics step
0x140364700 (trace 0x140365ad0, local offset 0x140367c00); vis state 0x140362e70, colours 0x1403628f0, sizes 0x140362850, scale
0x140362800, lighting 0x140362b80; model/light transform 0x140367280; cloud 0x140335210 (record), 0x140335630, 0x1403356c0 (type 9
history), cloud alloc 0x140608730 / 0x1406087b0; scene HDR scalars 0x140361e30 (the per-view setup writing FX system +0x2bc/+0x2c0 from
`r_unlitSurfaceHDRScalar` / `r_litSurfaceHDRScalar` or the scene definition); view/frustum setup 0x140367ca0; type 6/7 spawn probes
0x14035cc20, 0x14035d070; trail pass 0x140335de0; per-effect draw loops 0x140335a10, 0x140335b90, 0x140335930.
