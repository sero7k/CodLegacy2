# Ghosts (IW6 MP) bots: path nodes, node tests and sight traces

Static reading of `iw6mp64_ship_all.c` (the owner's unpacked-dump decompile; nothing run, unpacked or decrypted by this work). Tags: [C] read from the decompile or data, [I] inferred.

## Where the nodes come from
- The engine keeps nodes in one global `PathData` (the DB asset type with the name `aipaths`, zone asset id 0x16; layout in `fastfile_iw6::layout::PathData`) [C]. Fields at the global: node count at +8, node array at +0x10 (stride 0xb0 = `pathnode_t`), vis bytes at +0x1c, `pathVis` +0x20, exposure bytes/array +0x48/+0x50, no-peek vis +0x58/0x60, zone count +0x68 [C by the offsets used in the path code; global base 0x14424bad0].
- Node limit 0x2000, error text `Path_LoadNode: node out of range (%i)` and `Max number of disconnected paths exceeded` [C].
- The entity spawn function for `node_*` classes (0x140379190, strings `angles`, `script_angles`) binds the map entities to the loaded nodes in order, stops when the loaded count equals the PathData node count, and does not create nodes [C]. So nodes with links need the asset. It lives in its own zone, `<map>_path.ff` (36 maps staged on 5 Nov 2026; before that the asset was thought missing because only the map, patch, load and common zones had been checked) [C data]. Format below.
- The map's entity string has every node as a class `node_pathnode`, `node_cover_*`, `node_negotiation_begin/end` etc. (mp_frag 1902, counts in `docs/IW6_NATIVE.md`) [C data].
- Node type table (0x1409e5e50, 31 pointers): Error, Path, Cover Stand, Cover Crouch, Cover Crouch Window, Cover Prone, Cover Right, Cover Left, Cover Multi, Ambush, Exposed, Conceal Stand, Conceal Crouch, Conceal Prone, Door, Door Interior, Scripted, Begin, End, Turret, Guard, Path 3D, Cover Up 3D, Cover Right 3D, Cover Left 3D, Exposed 3D, Scripted 3D, Begin 3D, End 3D, Jump, Jump Attack [C]. Class names for them are the editor's: [I].

## Node tests the engine runs
- Nearest node with a capsule check (0x14037b9f0): candidates from a spatial query (radius arg), sorted by distance; a node counts as reached when the query point is within 16 units in x and y and within 37 of 36 above the node in z [C]; otherwise the candidate is tested with a capsule trace from the point (contents mask `& 0xFDFFBFFF`) using stand / crouch / prone bounds (centre 36 / 24.5 / 14.5, half extents 15, 15, same) [C]. The hull constants are at 0x14086c8a0, c8d0, c8e8.
- Path search keeps an open list as a binary heap on `fCost + fHeuristic` (0x14036c9f0, 0x14036cb20) [C].
- The script `Cover` family (0x1403795a0) returns nodes by type within a radius, sorted, optionally filtered by a squared distance [C].
- Sight lines: up to 32 `AI_SIGHT_LINE` entities (0x140379950) switch groups of node visibility on and off [C].

## Sight / visibility
- `sighttracepassed` builtin (0x1403b1f50): args start, end, hit-characters int, optional ignore entity. Contents `0x0080A831` when the int is 0, `0x0280E831` otherwise [C]. Calls the point sight trace wrapper 0x14039f9d0, which calls 0x14048bcc0: world descent (0x1403edb30), then the static-model sight test (0x1403f6070, enabled by the last argument = 1) and entities [C].
- The explosion / AI visibility helper (0x140393860) traces from the point to up to 5 sample points on the target's bounds (centre plus corners of a cross section) with the same wrapper, optional direction cone [C].
- Contents bits (see `IW6_COLLISION.md` section 5): 0x1 solid, 0x2 foliage, 0x10 glass, 0x20 water, 0x800 sky, 0x1000 ai_nosight, 0x2000 clipshot; 0x4000 and 0x2000000 are the character bits (absent from the sight mask) [C for the mask value; names of 0x4000/0x2000000 I].

## Bots in the engine
- Strings: `maps/mp/bots/_bots`, `maps/mp/bots/_bots_gametype_%s`, `mp/botDifficultyTable.csv`, dvars `bot_AutoConnectDefault`, `bot_DifficultyDefault`, `botsConnectType`, `botsDifficulty`, `BotsAreAllowed`, `ERROR: SV_BotFindNodeRandom() %d valid nodes exceeded in %s.` [C]. Add-bot: `SV_AddBot` 0x140470920. Agent movement dvars `agent_jumpSpeed`, `agent_jumpWallSpeed`, `agent_jumpGravity`, `agent_jumpWallGravity` (0x140375d40) [C names; values not read].
- The bot behaviour is script (`maps/mp/bots/*`): those scripts are not in the staged zones, so goal choice, arrival radius, node skipping, stuck handling and speed rules cannot be read here [C: absent].

## Evidence
`cargo run -p fastfile_iw6 --example iw6_asset_types -- <zones>`, `iw6_raw_list`, `iw6_ent_classes <zone> node`, `iw6_path_nodes <zone>`; `cargo test -p bots --test ghosts_nodes -- --nocapture`.

## The `<map>_path.ff` zone: PathData (read 5 Nov 2026, [C] from the bytes of all 36 staged zones; code `fastfile_iw6::pathdata`, walker `walk/misc.rs path_data`)
The zone holds two assets: the `PathData` and a raw file `<map>_path` (a few bytes). The body is packed with no file alignment. In file order:
1. `PathData` header, 136 bytes (`layout::PathData`): name pointer, `nodeCount` (+8; 1965 on mp_frag, 2178 on mp_snow), `nodes` pointer, `version` (+26: 21), `visBytes` (+28), `pathVis`, `nodeTreeCount` (+40), `nodeTree`, `dynamicNodeGroupCount` (+56), `dynamicNodeGroups`, `exposureBytes` (+72), `pathExposure`, `noPeekVisBytes` (+88), `pathNoPeekVis`, `zoneCount` (+104), `zonesBytes` (+108), `pathZones`, `dynStatesBytes` (+120), `pathDynStates`.
2. The name string (`maps/mp/mp_frag.d3dbsp`).
3. `nodeCount` x `pathnode_t` (176 bytes, contiguous). Constant part (the first 80 bytes): type (+0, the 31-name table above), spawnflags (+4), then five script-string ids (targetname +8, script_linkName +12, script_noteworthy +16, target +20, animscript +24, animscriptfunc +28) that index the zone's script string list, origin (+32), `fLocalAngle` (+44) and the cached facing vector (+48), parent (+56), `wOverlapNode[2]` (+64), `totalLinkCount` (+68, u16), `Links` pointer (+72). The rest is runtime state (dynamic flags, search fields), zero in the file.
4. For each node in order, its `Links`: `totalLinkCount` x `pathlink_s` (12 bytes): `fDist` f32, `nodeNum` u16, `disconnectCount` u8 (0 at load), `negotiationLink` u8 (1 for a traversal), `flags` u8, `ubBadPlaceCount[3]`. Links are directed; the walk links are almost all mutual. mp_frag: 26858 links, 137 of them traversals (one per Begin/End pair); mp_snow: 18268 links, 352 traversals.
5. `pathVis`: a triangular visibility bit matrix, `n(n-1)/2` bits rounded up to bytes (241204 bytes for 1965 nodes). The bit order inside is not decoded [I].
6. `nodeTree`: `nodeTreeCount` x `pathnode_tree_t` (24 bytes) as one flat array: `axis` (i32: 0 x, 1 y, 2 z, -1 for a leaf), `dist` f32, then either two child pointers (references into this array, VIRTUAL block) or, for a leaf, `nodeCount` (+8) and a pointer (+16) to `nodeCount` u16 node numbers. The leaves' lists follow the array, in array order. The lists together hold every node once (mp_frag 1902). This is the k-d tree the nearest-node query descends [C structure, I use].
7. Dynamic node groups (6 of 36 maps have them: boneyard_ns, ca_red_river, fahrenheit, lonestar, mine, sovereign): `PathDynamicNodeGroup` (16 bytes: parent index u32, tree count u32, tree pointer), each followed by its own tree in the same flat form.
8. `pathExposure` (65091 bytes on mp_frag), `pathNoPeekVis` (61002), `pathZones` (`zonesBytes` = node count bytes; `zoneCount` zones: 63 on mp_frag), `pathDynStates` (89468): kept as sizes, not decoded.

The sizes add up to the next asset's header exactly on all 36 zones. The asset's nodes are the map's `node_*` entities in order, then `zoneCount` pseudo nodes of type `Error` (0), one per AI zone, which form a component of their own (63 on mp_frag, 99 on mp_snow). The entities' z is the editor's origin; the asset's z is lower (a fixed seat above the floor that the compiler computed): the xy, type, targetname and animscript equal the entity's. Every traversal an entity pair names is a negotiation link of the asset.

Bots use the asset's nodes and links directly (`bots::from_path_nodes` with `Iw6PathNode::links`): no walk link is rebuilt, `Negotiate` edges come from the negotiation links, the door gates (`NavGraph::set_gate`) cut links by geometry as before. `BO2MP_IW6_PATHDATA=0` restores the entity nodes with rebuilt links. Tests: `cargo test -p fastfile_iw6 --lib pathdata`, `cargo test -p bots --test ghosts_nodes`.
