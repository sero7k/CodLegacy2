# A small dedicated server

`CodLegacy serve [zone]` runs matches with no local player (`CODLEGACY_HEADLESS=1`).
A **server pack** is one map's gameplay half in a small file, so a server
hosts a map from a game it does not have installed.

## What a headless match reads

* **The map**, from its pack: collision (`ClipCollision`), spawns, entity
  string and placements; not its drawing, lighting or effects.
* **Black Ops II's `.ff` zones** (no `.ipak`): scripts, tables, weapons,
  skeletons, animations, game settings, and its own maps (not packed).

## The pack

`<folder>/<game>/<map>.iw4lmap`: `IW4LMAP\0`, version, payload size (u32 LE),
LZ4 payload; a damaged file is refused. `CodLegacy pack <folder>` builds them from
the operator's own installs (never shipped) and prints each size. On the
server `CODLEGACY_SERVER_PACKS=<folder>`: a map with no installed zone loads from
its pack.

## Hosting on the master

`.env` beside `CodLegacy.exe` (the master's address as for players, plus):

```
CODLEGACY_MASTER_HOST_NAME="My Server"
CODLEGACY_MASTER_MAX_PLAYERS=18
CODLEGACY_SERVER_ROTATION="iw4:mp_highrise tricky_dm, t6:mp_nuketown_2020 tdm, t6:mp_raid tricky_sd"
CODLEGACY_SERVER_BOTS=0,4,2        # friends, enemies, difficulty 0-3
CODLEGACY_SERVER_TRICKSHOT=0       # 1: the trickshot script in every mode
CODLEGACY_SERVER_VOTE_S=20         # the pre-game map vote's length; 0: the rotation's order
```

`CodLegacy serve` (no zone: the rotation's first) opens the room, which players
find in the lobby browser. Between matches its players stay in the room and
vote the next map from two of the rotation's (never one map in two games'
versions, nor the map just played; a tie is a coin toss); with
`CODLEGACY_SERVER_VOTE_S=0` each match ends into the next entry (`session::server`).
The master must run the same protocol version as the game (11).
`tricky_dm`/`tricky_sd` play `dm`/`sd` with the trickshot script; `tricky_sd`
with two enemy bots only. The server holds one of the room's slots.

## Size (estimates; `CodLegacy pack` measures)

About 1–6 MB per map after LZ4, against a zone of tens to hundreds of MB plus
the game's common zones. Black Ops II: `zone/all/*.ff` only.

## Not done

* Guest guns: a client must have the same guest games as the server (the weapon
  registry is compared on join); stand-ins are not in yet.
* Infinite Warfare and Black Ops III zombies maps; MW2-native rules from a pack.
* Not yet run on a PC: `CodLegacy pack`, `serve` from packs, a rotation on the master.
