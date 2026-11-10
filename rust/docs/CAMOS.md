# Weapon camos

Numbers (`asset_core::weapon_camos`, all under `MAX_OPTION` 1023, kept in a
class's camo byte and in `bo2mp_camos`): BO2's own below 100, MW2 100+, MW3 120+,
Black Ops 140+, **special 200+**, MW III (2023) 300+, MW II (2022) 500+, BO3 850+.

How a camo reaches a gun: `swap` (a material camo: gold, diamond; a different
material drawn tiled) or `pattern` (a picture laid over the material; under BO2's
colour-map alpha mask on its own guns, over the whole material on every other
game's). Both only on "camo-ready" materials: BO2's camo sets, plus every material of
another game's gun model that `camo_skips_material` (match_walk.rs) does not leave
out (glass, sights, lights, hands: by whole words of the name, so a `handguard`
or `flashhider` is metal that wears the camo).

Special camos (`EFFECT_CAMOS`, the "SPECIAL" tab in Create-a-Class): a row of
data each: name, the camo whose picture it moves (or none: any loaded camo
picture carries a procedural look), a style and a speed. The style travels in
instance slot 22 as `10000 * (style + 32 * speed)` above the usual 3000/5000 +
repeat, and `camo_animate` in `geometry_diagnostic.wgsl` reads it with the game
time `t6_dyn.count.y`. To add one: append a row (never reorder: the row index is the
camo number, `FX_FIRST` + index). A new *style* is a branch in `camo_animate` and
a `STYLE_*` constant.

Special camos 8-12 are our own looks over Black Ops III's pictures, not Black Ops III's movement: in that game only Dark Matter, Ritual and Diamond move (see below).

Styles: scroll (the picture slides), pulse, sweep (a glint glides over it), plasma
(Dark Matter-like swirl), gold (procedural gold glint), ghost.

Ghost: white and see-through. Black Ops' guns once drew that way by mistake (their
technique sets' extra-light passes were read as the gun's blend state;
`render_material::catalog::t5_solid_by_name` now keeps them solid). As a camo the
gun's draw code is made blended (`render::extract::ghosted`) and the shader gives
the pixels alpha; the blended pipelines are only made while someone wears it.

Not done: camos of Ghosts, Advanced Warfare, Infinite Warfare and MWR (their pictures
are not catalogued; their guns do wear every listed camo). World at War has no camos.
To catalogue them, run on the owner's PC `cargo run -p fastfile_h1 --example
h1_camo_names -- <zone.ff>...` (MWR) and `cargo run -p fastfile_iw6 --example
iw6_camo_names -- <zone.ff>...` (Ghosts) and read the names back; the camo
list is then a row each in `asset_game::weapon_camo_models`.

## Camo packs (your own camos)

Put them in a `camos` folder next to the game (or point `CODLEGACY_CAMOS` at one).
They show in Create-a-Class under **CUSTOM** and work on every gun of every game;
up to 40, numbered from 220 in name order (start names with `01_`, `02_` to keep
the order: the prefix is not shown). The picture is laid over each gun's own
texture coordinates like the stock Black Ops camos, so there is no per-gun
setting: one `scale` fits all. PNG only, up to 4096 x 4096, any colour type.

Three ways to make one, easiest first:

1. **A PNG** is a still camo. A last word in the name makes it move:
   `lava_scroll.png`, `fire_pulse2.png`, `gem_sweep.png`, `ion_plasma.png`,
   `shine_gold.png` (a digit 0-3 after the word is the speed, 1 if none).
2. **A folder of PNGs** is an animation: the pictures are the frames, played in
   name order (`f2` before `f10`), up to 64. A folder with one PNG is a still.
3. **A folder with a `camo.txt`** to say more. Every line is optional:

```
name      = Hot Lava          # the picker's name (default: the folder's)
move      = scroll            # still scroll pulse sweep plasma gold reveal frames
direction = up-left           # scroll: right left up down up-right ... or degrees
speed     = 2                 # 0 slow ... 3 fast (frames: 6 12 24 48 per second)
fps       = 24                # frames: the nearest of those four
scale     = 3                 # times the picture repeats over a gun (default 2)
image     = lava.png          # which picture (default: the first)
grid      = 4x4               # the picture is a sheet of 4 x 4 frames (reads
                              # left to right, top to bottom); 4x4x13: 13 frames
```

Moves: **frames** plays the pictures (a folder of PNGs or a `grid` sheet; frames
of different sizes are stretched to one square, so draw them tiling);
**scroll** slides the picture one repeat per 24 / 12 / 6 / 3 seconds in the
direction given; **pulse**, **sweep**, **plasma**, **gold** are the shader looks of
the special camos, over your picture; **reveal** reads the PNG's *alpha channel*
as a mask (0 to 1) and sweeps a threshold through it every 8 s / pace: the
picture is brighter where the mask is above it (never black elsewhere), a hot edge between
(the Modern Warfare Dark Matter reveal map, with the nebula as the picture). Draw the picture so its edges meet
(tileable), as it repeats `scale` times across a gun and scrolls without a seam.

Under the hood: the filmstrip (`camo_pack::filmstrip`) stacks the frames into one
square-framed picture, and `camo_animate`'s sampling (style `STYLE_FRAMES`, 7; `STYLE_REVEAL` is 16)
reads the frame count as height over width and picks the frame from the game
time. A scroll is style `STYLE_SCROLL_DIR` (8) + a compass step 0-7. Mistakes in a
`camo.txt` are reported in the match load report ("own camo <name>: ...") and
the camo falls back to a still.

`scripts/make_example_camo.py` writes an example pack of each kind into a folder.

Built-in custom camos (`camo_pack::builtin`, numbers from 260, listed first in the
CUSTOM tab, their pictures in `assets/camos/` compiled into the game): **bo6 dark
matter** (the nebula over its reveal map, `move = reveal`). To add one: put the PNG
in `assets/camos/`, append a row to `builtin()` (never reorder: the row is the number).

## How Black Ops III animates its camos (read from core_common.ff, 9 Nov 2026)

Read statically from the inflated zone (no exe run). The gun camo is one
shader family: `gbuffer_weapon_camo.hlsl` (defines `WEAPON_CAMO`,
`CAMO_DETAIL_MAP`, `USE_COLOR_SPEC`; 10 variants, with and without motion
vectors) and the forward pass `lit_weapon_camo` (`mc/lit_weapon_camo`,
`mc/lit_weapon_camo_detail`). The G-buffer pixel shader only reads pictures
(`colorMap`, `specColorMap`, `normalMap`, `glossMap`, `aoMap`, `camoMaskMap`,
`camoDetailMap`, `normalBodyMap`, `glossBodyMap`) and `baseNormalHeight`,
`glossRange`: a still camo.

The motion is in the forward pixel shader (94 KB). Its `$Globals` holds, per
material: `colorTint`, `rowCount`, `columnCount`, `imageTime` (a flipbook of
rows x columns frames, `imageTime` seconds a frame), `rotateUVs`, `scrollUVs`,
`scaleUVs`, `offsetUVs`, `zoomUVs`, `zoomRate`, `layerDepth`, `clampU`,
`clampV`, `flickerSeed`, `flickerSpeed`, `flickerRange`, `flickerPower`
(a glow that flickers, read from `flickerLookupMap`), `useAsCamo`, `hdrScale`,
`emissiveFalloffPower`, `emissiveTint1`, `relativeHDR`, `invertFalloff`,
`uvMotionToggle1`; pictures `emissiveMap`, `flickerLookupMap`; the clock is
`PerSceneConsts.gameTime`. So Dark Matter and its kind are a flipbook and/or a
scroll / zoom of the picture plus a flickering emissive glow, per material
constants. Our counterparts: `STYLE_FRAMES` (flipbook), `STYLE_SCROLL_DIR`,
`STYLE_PULSE` (flicker).

The per-material numbers are not read yet: run on the owner's PC
`cargo run -p fastfile_t7 --example t7_camo_consts -- <zone>/core_common.ff
> camos.txt` (and `core_patch.ff`) and send `camos.txt`; each camo's rows,
columns, frame time, scroll, zoom and flicker become its `EFFECT_CAMOS` row.

### The numbers (core_common, dumped with `t7_camo_consts`)

Only three gun camos move; every other one is `lit_weapon_camo` (still).

* **Dark Matter** (`lit_emissive_scroll_3layer_plus`): the emissive picture
  `inifinity_e2` on three layers, scale (1, 0.8) / (1, 2) / (1, 1), scroll a
  second (0.02, 0.02) / (0.06, -0.07) / (0, -0.03), tints red (1, 0.006,
  0.004) / blue (0.002, 0.007, 0.06) / violet (0.04, 0.003, 0.16), flicker
  speed 0.228, power 0.5, `hdrScale` 36, a black body. Done as `STYLE_LAYERS`
  (17) in `camo_animate`'s call site ("Dark Matter Live").
* **Ritual (zod pap)**: the same shader, dim grey tints (0.41), `hdrScale`
  256, slow scrolls (0.02, 0.01), (0.01, -0.01). Done as `STYLE_RITUAL` (18), the special camo "Ritual".
* **Diamond** (`lit_emissive_transparent_scroll_advanced_fullspec`): the
  emissive picture `diamond_inlay_e` as a 4 x 4 flipbook, `imageTime` 2,
  scale 6 x 6, scroll (0.02, -0.7), flicker speed 0.3, `hdrScale` 100. Done as
  `STYLE_DIAMOND` (19), the special camo "Diamond Flipbook" (the glow pictures
  of Diamond and Ritual are linked under `T7_GLOW_FIRST` + the camo's number).
* Gold, Chameleon and the rest: still. `core_patch` holds no camo materials;
  the DLC camos are in the `base_dlc*` packs.
