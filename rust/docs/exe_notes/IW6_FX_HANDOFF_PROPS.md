# Ghosts props and effect sources: notes for the effect-drawer worker (iw6-fxexe)

Written by worker iw6-props, 2026-11-04. Scope here: which effects are found, converted and placed (assets lane), props and terrain. The drawer
(`fx_iw4`, `render_fx`, the effect passes in `render_gpu`) is not touched by this branch.

## What the lane hands the drawer

- `assets::lane::iw6_fx::prepare` converts every effect it finds to the AW element layout and queues `CreateFxOneshot { fxid, origin_inches,
  angles_deg, delay, repeat_secs }` placements (createfx text, scriptable first states, exploders and script `playfx` by path).
- Element type mapping (`aw_elem_type`): Ghosts 0 (billboard sprite), 1 (oriented sprite), 2 (tail, trail) -> 3, 4 (flare) -> 255 unless
  `BO2MP_FX_FLARE_SPRITE=1`, 8 / 9 (clouds) -> 255 UNLESS `BO2MP_IW6_FX_CLOUDS=1` (default OFF since the owner's snow-map screenshot, see below),
  10, 11 (model; the lane drops the model visual), 12 (light), 14, 16 as the exe's dispatch. 255 is counted as "unsupported" and played as a sound
  element (nothing drawn).
- A placement is played only when the effect has a sprite whose material linked (colour picture present, technique not distortion / radial blur /
  dust-on-lens) or runs a child effect. An effect made only of lights, trails or models is reported as "with nothing drawable". If the drawer
  gained support for light-only or trail-only effects, `playable` in `prepare` is the gate to widen.
- Placements carry the createfx angles as pitch / yaw / roll (the effect points along the entity's forward AND up: roll counts).

## The giant translucent white fans (owner's snow-map screenshot, 2026-11-04)

Not diagnosed to a source offline (only mp_frag is staged, the screenshot's map is not). Evidence: the fans are large triangular, near-white,
additive-looking sheets standing on the ground, many overlapping, each a sharp apex up and a wide base. Candidates, in order:

1. Cloud elements (types 8 and 9) played as plain sprites by the AW path: the exe draws them as a cloud (a field of sprites in a volume), the AW
   sprite path draws one camera-facing quad per particle at a size read from the visual's size samples; if the size samples are in cloud-volume units
   the quad is far too big. This branch turned types 8 and 9 off by default in the lane; if the fans are still there with `BO2MP_IW6_FX_CLOUDS`
   unset, they are not clouds.
2. Type 1 (oriented sprites, "ground decal quads") whose orientation comes from the placement's angles: a quad that should lie on the ground stands
   up when the angles' roll or pitch is applied twice.
3. Dual-source blend `_abl` materials (blend codes 13 / 14) falling to additive in `render_iw6::state` (see the VFX audit, defect 4): this makes
   smoke too bright but would not make the shape.

The shape (apex up, sharp base) looks like a sprite stretched along velocity (a "tail" element, type 2 mapped to 3) with a huge length scale: an
effect with a velocity-aligned particle and a near-zero-length frame. Worth checking in `fx_iw4`: tail length = speed * lifetime scale clamp.

## Where things are logged for the owner (send these lines)

- `iw6 createfx <script>: N placed ...; effects named but in no zone read: [...]; with nothing drawable: [...]`
- `iw6 fx NOT RESOLVED <script>: createfx ids ...; script paths ...; exploder ids ...; scriptable effects ...; child effects ...; with nothing
  drawable ...; sprite materials not linked ...` (one line per map; new, this branch)
- `iw6 script fx: N of M effect paths the scripts name found in the zones`
- `iw6 scriptable fx: ...`, `iw6 exploders: ...`
