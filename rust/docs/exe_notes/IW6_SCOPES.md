# Ghosts (IW6 MP, PC x64) - Scopes, ADS, Reticles, Thermal, Hybrid/PiP: Behavioural Spec

Static reading only. Marks: [C] confirmed from code/data, [I] inferred, [U] unknown.
Image base 0x140000000. FUN_ names refer to the decompile in iw6mp64_ship_all.c.

## 0. Packed weapon id (C)
weaponIdx 8b | weaponVariation 6b | weaponScopes 3b (>>14) | weaponUnderBarrels 2b (>>17) | weaponOthers 7b (>>19) | scopeVariation 6b (>>26) = RETICLE INDEX.
WeaponCompleteDef* = DAT_141734de0[idx]; WeaponDef* = DAT_1417352e0[idx]. FUN_140240bf0 gathers up to 4 attachment ptrs.

## 1. How a scope is drawn
Two independent mechanisms exist; weapons use one or both.

### 1a. HUD overlay (C)
- Source: attachment adsOverlay (WA+0xc0, AttADSOverlay 0x40) or WeaponDef.overlay (+0x5b0).
- Active (FUN_140260c50) iff overlay.shader != 0, or (overlay.reticle != 0 and dual-view not active), or weapon-class-8 special case; AND alpha frac > 0.01.
- Alpha frac (FUN_1402a9f40): p = ps ADS progress (cg+0x424). p>=1 -> 1; p<=0 -> 0; else f = clamp((p-(1-z))/z,0,1) where z = zoomOutFrac (or zoomInFrac when cg+0x3330 != 0 [I: in/out flag]) times product of attachment adsSettingsScale (+0xf8, <=0 treated 1). Skipped in 3rd person.
- Drawn (FUN_140260d70) only when p>=0.99 and (cg+0x124 & 0x3800)==0. Centre = screen centre + sway offset from FUN_14025ee20 (aim-axis projection, 640x480 virtual: x=-320*dx/(tanHalfFovX*dz), y=-240*...). Size = overlay.width x height (splitscreen variants separate).
- Material (FUN_140260bc0): virtual width>480 -> shader, else shaderLowRes if set; if EMP/jammed (cg+0x10 bit10 && !(cg+0xe18 bit14)) -> shaderEMP / shaderEMPLowRes.
- AttADSOverlay: shader 0, lowRes 8, EMP 0x10, EMPLowRes 0x18, reticle(int) 0x20, width 0x24, height 0x28, widthSS 0x2c, heightSS 0x30, hybridToggle 0x38, thermalScope 0x39, thermalToggle 0x3a, outlineEnemies 0x3b.

### 1b. 3D lens + picture-in-picture dual view (useDualFOV, AttSight+8) (C)
- Viewmodel scope model has bones tag_lense, tag_lense_top, tag_lense_hide, tag_camera.
- FUN_140147560 projects lens quad corners to a screen bounding rect, clipped to refdef viewport (FUN_140147c10); FUN_1401473a0 gives lens centre and r^2. FUN_140261ce0 writes lens rect + blend to refdef+0x190..0x1a0, scaled to render resolution into viewInfo+0x2bc/+0x2c0..0x2cc.
- Frame (FUN_1402cf070): ramp blend DAT_14187eb3c by dt / r_ssrFadeInDuration (default 0.05 s, 0.01..1), reset to 0 when PiP on/off toggles. PiP off -> normal scene (FUN_1402cefd0). PiP on -> main scene, then FUN_140146e00 second scene render.
- Activation (FUN_1401471d0), p = ADS progress:
  - going in: active once p>0.4, t=(p-0.4)/0.6
  - going out: stays active until p<0.7, t=(p-0.7)/0.3
  - below: off, fade=(0.4-p)/0.4 clamped >=0
  - requires useDualFOV, (cg+0x124&0x3800)==0, DAT_1417e04ac==0.
  - outputs: DAT_14187eb08 = t*14/3; DAT_14187eb0c = 8*t+1 (meaning [I]: eb08 blur/DOF strength -> refdef+0x70; eb0c projection/FOV scale passed to FUN_14060d490/FUN_140611830).
  - Hybrid: state3 uses 1-afc, state5 uses afc, state4 uses 1-afc (afc = DAT_141823afc).
- PiP pass (FUN_140146e00) [C]: flag DAT_14187eb00=1; r_tessellation forced 0 and restored; refdef rect shrunk to quarter-screen (w/2,h/2) then restored; thermal state set via FUN_14060e790(thermalOn, notFullscreen, thermalBodyMat, 0); re-runs cgame prep (FUN_14029e200, FUN_140353050, FUN_140361980, FUN_140261ce0, FUN_14029bd50, FUN_14029d1f0); FUN_14060d490(&refdef, eb0c); FUN_14060c420 -> FUN_140609a40 (backend view slot 4, offset 0x14b40).
- While DAT_14187eb00==1, CG_GetFov skips the ADS FOV blend: PiP view uses cg_fov*cg_fovScale (i.e. unzoomed base FOV), magnified by the eb0c scale [I].
- Main view in PiP mode: ADS fov f forced to 1.0 (full ADS FOV).
- Render targets (C): $pip_scene (13), $pip_scene_floatz (14), $pip_scene_pingpong (15), $pip_scene_final (16), nodepth variants 17..19, $pip_depth, $msaa_pip_*, DYNAMIC_IRIS_EXPOSURE_0/1_PIP (70/71). All 512x512 (FUN_140606290->140605d60(0x200)->1406069d0(0x200,0x200)); formats 10, 0x29, 0x1c. TEXTURE_SRC_CODE_PIP_SCENE = 41. "pip_scene_overlay" material is only the r_showPIPTexture debug draw (FUN_140631d10).
- Code constants (viewInfo+0x370+idx*16; table 0x1409f3b70, 348 entries): SCOPE_SAMPLE_LIMITS_PS idx143 off 0xc60; DISTORTION_SAMPLE_LIMITS_PS idx142 off 0xc50; THERMAL_COLOR_OFFSET 248 (0x12f0); THERMAL_FADE_CONTROL 249 (0x1300); THERMAL_FADE_COLOR 250 (0x1310); SSR_FADE 138 (0xc10); VIEWPORT_DIMENSIONS 246; FRAMEBUFFER_READ 247.
- SCOPE_SAMPLE_LIMITS (C): written only in FUN_140609a40 (PiP view) = ((x+0.5)/W, (y+0.5)/H, (x+w-0.5)/W, (y+h-0.5)/H) using the MAIN view's viewport (viewInfo[client]+0x170..0x17c) and scene RT size (DAT_1480e8970/74). Purpose [I]: half-texel-inset UV clamp so the lens shader never samples outside the main scene viewport. Consumer shader not identified [U].
- Also in PiP view: const 0xd60 = refdef+0x148*0.984375; snow ambient 0x12c0=(0.32,0.32,0.4).

## 2. Lens shader inputs
(Decoded 2026-11-05: see 'Lens shader' at the end.)
Known: SCOPE_SAMPLE_LIMITS (above), $pip_scene_final as source (TEXTURE_SRC_CODE_PIP_SCENE), lens rect/blend in viewInfo+0x2bc.., THERMAL_* constants in PiP pass. Techset/shader bytecode for the lens material not decoded [U].

## 3. Reticles
- Reticle is a 3D viewmodel (not HUD sprite) [C]. FUN_1402a98f0(weapon, scopeAtt) -> scopeAtt.reticleViewModels[scopeVariation] for 0..15 (WA+0x30, 16 models). Indices 16..63 come from extension attachments "reticlehack", "reticlehack2", "reticlehack3" (banks of 16) loaded via DB_FindXAssetHeader (DAT_1419d96e0/e8/f0).
- Added as extra viewmodel DObj model in FUN_1402aede0 only if not EMP/jammed (DAT_1418213a8==0). When EMP: FUN_1402adff0 hides bones tag_reticle_acog, tag_reticle_red_dot, tag_eotech_reticle, tag_magnifier_eotech_reticle, tag_reticle_tavor_scope, tag_reticle_thermal_scope.
- Red-dot / "on" state (FUN_140245b30): EMP -> hide tag_reticle_hamr, _on, _reflex. Else if altMode || (hybridCapable && hybridScope==0): hide hamr, show _on and _reflex. Else show hamr, hide _on/_reflex. tag_show_alt shown if altMode or anim state 28/29. Tag ids table loaded by FUN_1403c38f0 (also tag_sight_on/off).
- 2D crosshair fields (AttGeneral): reticleSpin45 +2, reticleCenter +8, reticleSide +0x10, reticleCenterSize +0x18, reticleSideSize +0x1c.
- mp/reticleTable.csv referenced (stat keys weaponReticle, unlockedReticles); columns NOT read [U]. Parallax/size/brightness: not in code; presumably model/shader data [U].

## 4. Thermal
- Predicate (FUN_14029ab20 -> FUN_1402a12b0), per frame in CG_DrawActiveFrame (FUN_140299760): false if EMP; false in killcam modes 1/3/7; if (DAT_14176ec10&8) full-screen thermal; else weapon thermalScope (FUN_1402413f0) requiring ((cg+0x124&0x3800)==0 and (PiP active or overlay active)); if thermalToggle&&thermalScope needs PlayerEquippedWeaponState.thermalEnabled (+0xc); if hybridToggle needs hybridScope byte (+0xd)==0. [C]
- State: FUN_14060e790 sets DAT_1480e8890/91/98/a0; body material default "m/thermalbody_default_hdr" (DAT_1480b6ec0) or configstring override (FUN_140261dc0). PiP passes flagD=0 (disables an expensive feature path [I]).
- Draw (R_AddDObjSurfacesCamera 0x1406078b0 / R_AddXModelSurfacesCamera 0x140608f30) [C]: for thermal-flagged objects (players etc.) if techset has thermal technique (techset+0x328 != 0; TECHNIQUE_THERMAL=0x63) use the material in draw list 3; else (non-viewmodel) substitute the thermalbody material. Players are therefore drawn flat hot-body; world uses its thermal technique.
- Constants (FUN_14060afb0): THERMAL_COLOR_OFFSET=(r_thermalColorOffset, r_thermalColorScale, r_thermalDetailScale, 0); THERMAL_FADE_CONTROL=(float(r_thermalFadeControl), 1/(max-min), -min/(max-min), 0) (if min>=max: scale 1, bias -min); THERMAL_FADE_COLOR=r_thermalFadeColor/255.
- Defaults: r_thermalColorScale 1 (-4..4), ColorOffset 0, DetailScale 1; r_thermalFadeControl 0=Disabled; FadeColor (0,0,0,0); FadeMin 400, FadeMax 1000 (0..10000). Detail-from-normal-map is the DetailScale input to the thermal technique [I]; shader bytecode not decoded [U].
- Blur (FUN_14029a300): while thermal on and blur>0: blur=max(base, thermalBlurFactorScope(250) if scope thermal, else thermalBlurFactorNoScope(250) if full-screen thermal). Dvars int 0..10000.
- Tracers (FUN_140580870): thermal flag -> width *= tracer_thermalWidthMult (2.0, min 0.1), material gfx_tracer_thermal; first-person clamp tracer_firstPersonMaxWidth.
- Beacon fx fx/misc/thermal_beacon_inverted per player (FUN_14027e2b0) by thermal state and team; outline mode DAT_14187eaf0 (4, or 0 if thermal on) uses adsOverlay.outlineEnemies (FUN_1402454b0).
- thermal_playerModel dvar: registered, no reader found.
- Predator postfx (killstreak, separate path FUN_140640ca0/FUN_14063fe30; postfx_predator_thermal*) with Cold/Hot/Ally/Enemy colour-grade dvars (ColdContrast 1, Sat 0, Bright 1, PosterColorCount 8, PostContrast 1.75, PostBrightness 1.5, OutputColorScale (0.1,0.2,1.0); Ally contrast .75 sat 4 bright 3 hue .75; Enemy contrast .75 sat 10 bright 2). Use by scope thermal NOT confirmed [U].
- Toggle: HUD hints PLATFORM_THERMAL_TOGGLE / PLATFORM_HYBRID_TOGGLE when ADS and capability flag set; toggle command handler not located [U].

## 5. ADS mechanics
- CG_GetFov (FUN_14029a630): cg_fov default 65 (65..80), cg_fovScale 1.0 (0.2..2), cg_fovMin 1 (1..160), missileRemoteFOV 15. fov*=cg_fovScale; *0.9 3rd person; clamp [cg_fovMin,170]; spectate 90.
- ADS FOV (non-hybrid, PiP flag 0): adsFov = canVariableZoom ? {25,15,8}[zoomLevelIndex] : GetAdsZoomFov; adsFov=min(adsFov,cg_fov); fov=(1-f)*cg_fov + f*adsFov, linear in degrees, f = remapped ADS progress (section 1a). GetAdsZoomFov (FUN_14023a880) = product of adsSettingsScale * (attachment adsSettings.adsZoomFov else WCD.fAdsZoomFov +0x90).
- Hybrid (anim-driven states 0..5, FUN_1402ab400, anim idx 0x1f/0x20/0x21): anim 0x20 weight>0 -> state 4 (frac=animLen*animFrac/adsFovLerpTime, cap 1) while running else 3; anim 0x21 -> state 5 running else 2; only 0x1f -> 1; default 0. FOV: fov0 from adsSettingsMain(+0x90), fov1 from adsSettings(+0x88). State2=fov0, 3=fov1, 4=lerp(fov0->fov1,f), 5=lerp(fov1->fov0,f). Default fade denominator 1/6.
- ADS blend timing: adsTransIn/OutTime, adsZoomIn/OutFrac per AttADSSettings (offsets in layout.rs; 0x40 struct). Weapon-busy mask cg+0x124&0x3800 disables scope features.
- Overlay hint/other dvars: cg_hudDamageIconInScope false, cg_hudGrenadeIconInScope false, cg_drawBreathHint true, player_breath_snd_lerp 2, player_breath_snd_delay 1. DOF dvars: r_dof_enable true, nearBlur 6, farBlur 1.8, viewModelStart 2, viewModelEnd 8, nearStart 10, nearEnd 60, farStart 1000, farEnd 7000.
- [U]: hold-breath & scope-drift code, ads-delay-fire(-scoped) consumer, WCD adsDofStart/End (0xfc/0x100) consumer, viewmodel hiding in overlay ADS.

## 6. Struct offsets (from layout.rs, verified against accessors)
WeaponCompleteDef 0x128: weapDef 8, scopes 0x20, underBarrels 0x28, others 0x30, fAdsZoomFov 0x90, iAdsTransIn 0x94, iAdsTransOut 0x98, altWeapon 0xc8, adsDofStart 0xfc, adsDofEnd 0x100.
WeaponAttachment 0x118: viewModels 0x28, reticleViewModels 0x30, sight 0x40, general 0x58, idle 0x80, adsSettings 0x88, adsSettingsMain 0x90, scopeDrift 0x98, scopeDriftMain 0xa0, adsOverlay 0xc0, ui 0xc8, adsSettingsScale 0xf8, ...Main 0xfc, hideIronSights 0x114, shareAmmoWithAlt 0x115.
AttSight (9): aimDownSight 0, adsFire 1, rechamberWhileAds 2, noAdsWhenMagEmpty 3, canHoldBreath 4, canVariableZoom 5, hideRailWithThisScope 6, useScopeDrift 7, useDualFOV 8.
AttADSSettings 0x40: adsSpread 0, adsAimPitch 4, transIn 8, transOut 0xc, reloadTrans 0x10, crosshairInFrac 0x14, crosshairOutFrac 0x18, adsZoomFov 0x1c, zoomInFrac 0x20, zoomOutFrac 0x24, fovLerpTime 0x28, bobFactor 0x2c, viewBobMult 0x30, fireRateScale 0x34, damageRangeScale 0x38, fireAnimFrac 0x3c.
AttScopeDrift: Delay 0, LerpIn 4, Steady 8, LerpOut 0xc, SteadyFactor 0x10, UnsteadyFactor 0x14.
WeaponDef: overlay 0x5b0, fAdsZoomInFrac 0x5a8, OutFrac 0x5ac, canHoldBreath 0xa55, canVariableZoom 0xa57, thermalScope 0xa7b, thermalToggle 0xa7c, useScopeDrift 0xa86, turretScopeZoomRate/Min/Max 0x958..0x960.
PlayerEquippedWeaponState (16B; array cg+(i+0x32)*0x10): usedBefore 0, dualWielding 1, inAltMode 2, needsRechamber[2] 3..4, zoomLevelIndex(int) 8, thermalEnabled 0xc, hybridScope 0xd.

## 7. Re-implementation recipe (Rust)
1. Per frame compute p (ADS progress); f = remap(p, zoomIn/Out frac * scale); fov = lerp(cg_fov, adsFov, f).
2. If overlay present and p>=0.99: draw overlay quad at screen centre + sway, sized in 640x480 virtual px.
3. If useDualFOV: when p>0.4 (in) / >0.7 (out) render a second scene at 512x512 with base FOV magnified by (8t+1), blur 14t/3, into PiP RT, sample it through the lens quad rect, fading by 0.05 s ramp; clamp UVs to half-texel inset of main viewport.
4. Reticle: spawn reticleViewModels[scopeVariation] on the sight; toggle tag_reticle_hamr/on/reflex per altMode/hybrid; hide on EMP.
5. Thermal: when predicate true, swap player/dynamic objects to thermalbody material and world to thermal technique, upload THERMAL_COLOR_OFFSET/FADE constants, blur 250, tracer width x2.

## 8. Unknowns
Lens/thermal shader bytecode; SCOPE_SAMPLE_LIMITS consumer; reticleTable.csv columns; reticle brightness/parallax; culling/LOD in PiP view (FUN_140611830 only derives LOD scale from view fov * 2.118673 * param * DAT_1480e88a4, clamped by r_clampLodScale); thermal/hybrid toggle command; hold-breath/scope-drift; ads-delay-fire-scoped; adsDof consumers; meaning of eb08/eb0c; cg+0x3330 semantics.

## Hold breath and drift (2026-11-05)

Read: dvars cg_drawBreathHint (draws the "+holdbreath" hint), player_breath_snd_lerp (2), player_breath_snd_delay (1, seconds, converted to ms), perks specialty_holdbreath, specialty_improvedholdbreath, specialty_holdbreathwhileads, and the settings perk_extraBreath (extra seconds, plus one on top of the base perk). Not found: the code that spends and refills the breath, and any consumer of useScopeDrift / AttScopeDriftSettings. `render_iw6::scope::{can_hold_breath, hold_breath_step}` are INFERRED stand-ins with tests.

## Lens shader, reticle table, thermal (2026-11-05)

Tool: `cargo run -p render_iw6 --example iw6_scope_shaders -- <zones>` (techsets_code_post_gfx_mp.ff and code_post_gfx_mp.ff hold the post techniques).

**Consumer of SCOPE_SAMPLE_LIMITS_PS (C, from the install's techniques).** Exactly one technique reads it: `blur_scene_nc` (vs `blur_scene_nc_vs.hlsl`, ps `blur_scene_nc_ps.hlsl`), with code inputs TRANSPOSE_WORLD_MATRIX0, TRANSPOSE_VIEW_PROJECTION_MATRIX, SCOPE_SAMPLE_LIMITS_PS, RENDER_TARGET_SIZE, EYEOFFSET, BLUR_SCENE_PIXEL_POS_TO_UV, the code texture BLUR_SCENE_SAVE (sampler 42) and two material textures (hashes a0ab1041 colour map at 0, b60d1853 offset map at 4). DISTORTION_SAMPLE_LIMITS_PS is read by `distortion_scale` and `distortion_shade` (ps `distortion.hlsl`, with RESOLVED_POST_SUN). The `pip_scene_overlay` technique (ps `textured_simple.hlsl`, PIP_SCENE texture) is the r_showPIPTexture debug draw, as noted. So the lens material is the `blur_scene_nc` technique and its scene texture slot is the saved blur scene, which the PiP pass fills. [I: that the scope's lens material uses this technique; the install has no material naming it.]

**Pixel shader (C, bytecode translated):** `uv = (tex4(v1.zw).xy - 0.5) * v2.xy + v1.xy; uv = min(max(uv, limits.xy), limits.zw); out = tex5(uv) * tex0(v1.zw)`. v1.zw is the quad's texture coordinate, v1.xy and v2.xy come from the vertex stage (screen UV and the pixel-to-UV scale). Rust: `scope::lens_sample_uv`.

**SCOPE_SAMPLE_LIMITS packing (C):** `scope::sample_limits(viewport, target)`.

**Reticle table (C for the read columns, U for the rest).** `mp/reticleTable.csv` is not in the install (no zone holds it; only splashtable, soundtables, intelchallenges, xp_event_table, camerapositions and vehiclevisionsets were found in the English zones). The exe reads it in two places: a lookup by value in column 1 (the reticle name; the name comes from the weapon's attachment string), then column 6 as an integer, which is tested against and set in the player's `unlockedReticles` bank (bit set test with 3 as the group arg). So: col 1 = name, col 6 = unlock bit index [I: of a 64-bit bank, because the bank test rejects indices of 64 and over]. The "extraslots" function loads the table with four others but passes it only to a helper that reads `mp/camo_reticle_unlockTable.csv` ("%s_%s" key, columns 1 and 2). Other columns (parallax, size, brightness) are not read by the exe. Rust: `scope::parse_reticle_table`, `reticle_unlock_bit`, `reticle_unlocked`.

**Thermal (partly C).** No technique in techsets_common, techsets_common_core_mp, techsets_mp_frag, techsets_code_post_gfx_mp, or the code_post_gfx zones reads THERMAL_COLOR_OFFSET / FADE_CONTROL / FADE_COLOR, and the techset slot the exe calls the thermal technique (0x63, 99) is empty in every techset of those zones (slots present: 0..92, 97, 100..). The thermal technique shaders are therefore not in the install and cannot be decoded or checked; the constants (section 4) and blur are in `scope::thermal_constants` and `scope::thermal_blur`, tested against the dvar defaults. `scope::set_idle_constants` sets all three, plus the scope and distortion limits, for a frame with no scope view.
