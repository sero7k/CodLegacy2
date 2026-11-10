# MW3 (2023, "jup") `cod23.exe` pass, 5 Nov 2026: stopped at the first check

Private notes (addresses allowed here). Exe: `cod23.exe` from `cod23.rar` (442,079,160 bytes, RAR5).
Tools: pefile, numpy, capstone on the PE headers, the section table, the `.pdata` table and per-chunk entropy only.
No Ghidra project was created. The extracted copy was deleted. Nothing was run.

## Verdict: nothing was extracted, because this file is not the exe as shipped

The owner's rule for this pass: read only what is plainly readable in the file as shipped; no unpacking, no
decrypting, no dumps. The uploaded file is a **memory image of a running process written back to disk**. Header-level
evidence (each one alone is enough):

- The import address table (directory at RVA 0x162ed1e8, 0x1b20 bytes) holds **resolved DLL entry points**
  (`0x7ffa03ad03b0`, `0x7ffa03acb090`, ...). A shipped file holds name or ordinal RVAs there.
- Of 225,842 base-relocation sites in `.text`/`.rdata`, 225,837 already hold values with the high half `0x7ff7`
  (the load address of that run), none the header's `0x140000000`. The loader had already applied the relocations.
- The TLS directory (RVA 0xab44b80) holds `0x7FF7...` addresses too, while the header image base is `0x140000000`.
- The header checksum (0x1a5a6634) does not match the file (computed 0x1a5a2017), and the Authenticode directory is
  still present (offset 0x1a596e00, 0x29b8 bytes), i.e. the headers are a signed shipped header over a later body.

So the readable first `.text` is code as it stood in memory at dump time. Reading it would be using the result of
running the game and copying its memory, which the standing rules forbid ("never run them", no unpacking or
decrypting, no dumps). The pass therefore stops here. Nothing below was read from the code or strings.

## What the headers alone say (structure only, no content)

| part | RVA | size | entropy (4 MB chunks) | note |
|---|---|---|---|---|
| `.text` #1 | 0x1000 | 0x924c000 (153 MB) | 5.6 to 7.0 | decodes as x86-64; `.pdata` covers it |
| `.rdata` | 0x924d000 | 0x2318800 | 6.0 to 7.6 | plausible tables |
| `.data` | 0xb566000 | file 0x13aac00 of virtual 0xa90ae44 | 0 to 1.6 | mostly zero fill; only part saved |
| `.pdata` | 0x15e71000 | 0x258c00 | 7.3 (normal for unwind tables) | 205,056 entries, begin addresses sorted, every one inside `.text` #1; unwind data points into `.rdata` |
| `.text` #2 | 0x162ef000 | 0xd80b000 (226 MB) | 7.2 to 8.0, long runs at 8.0 | no `.pdata` entry points into it; flat bytes |

- Sampling 6,000 `.pdata` functions: about 97 percent of them decode end to end in `.text` #1. `.text` #2 has no
  function table and its chunks sit at the maximum entropy, so it is protected or compressed data, not code to read.
- Imports: d3d12, dxgi, amd_ags, bink2w64, oo2core_8 (Oodle decompress only), discord_game_sdk, WinHTTP, bcrypt,
  crypt32. Exports include AMD FidelityFX (`ffx...` frame interpolation, optical flow), `D3D12SDKPath`.
  So the renderer is D3D12 with FSR3-style frame interpolation. No Vulkan.
- Link time stamp 1741637549 (10 Mar 2025), linker 14.x.

## Verdict per area

| area | verdict | why |
|---|---|---|
| lighting (probe grid, octahedron probes, sun/cascades, exposure, tonemap, SSAO, fog) | not enough (not analysed) | the only copy offered is a process dump; the zones must supply the numbers, as they already do |
| vfx | not enough (not analysed) | same |
| collision | not enough (not analysed) | same; `fastfile_jup::map::{havok, clip}` stay as read from zones |
| map layout | not enough (not analysed) | same; the zone and xpak data stay the source |

Rust: no change. `fastfile_jup` and `render_jup` are untouched (and, since 5 Nov, behind the off-by-default `jup`
feature).

## What would unblock it

The exe exactly as the Battle.net installer lays it down (no run, no dump; header IAT holding name RVAs, relocation
sites holding `0x140000000`-based values). If that file's code is itself encrypted at rest (the second `.text`
suggests part of it is), then those parts stay out of scope, and the readable remainder would be assessed with the
method of `IW6_LIGHTING.md` and `MWR_H1_LIGHTGRID.md`.
