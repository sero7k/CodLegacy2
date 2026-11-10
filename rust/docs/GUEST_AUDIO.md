# Guest guns: how their sounds and effects are chosen

A guest gun is another game's gun in a Black Ops II match. This is how what it names becomes what plays.

## Sounds

- **Banks.** Each game's loader in `asset_audio` (`sound_load_{iw6,s1,h1,t7,t4,jup}.rs`) reads its aliases under its own
  namespace. `compose_sound_bank` adds them to the match bank. MW2/MW3/BO1 aliases come from their `common_mp`.
- **Alias.** The local player hears `fireSoundPlayer` in 2D, with no origin. Everyone else hears `fireSound` at the
  shooter, in 3D. The last shot plays `fireLastSoundPlayer`/`fireLastSound`, else the plain fire. An MWIII gun with no
  last-shot sound of its own repeats its fire sound, not the donor's.
- **Records.** One record of a list is picked at random. Records for a sound context are not variants:
  - AW and Horizon MW (`wpn_int_med`, `wpn_int_sml`, `wpn_semi_open`: a tail in a room) keep only the plain records
    (`outdoor_records`) when a list has any.
  - BO3 keeps the dry, outdoor, normal-movement records (`context_rows`).
- **Layers.** A record's `secondary` plays with it (shot → punch → sub → mech → LFE → tail), each with its own falloff.
- **Falloff.** AW and Horizon MW records take their own curve when the zone loads it (`h1/curve/<name>`). Any other guest
  record without one takes `GUEST_FALLOFF`, the IW family's `default` curve. A world sound with no curve does not start.
- **Clips.** AW's and Ghosts' primed sounds play their whole stream, not the head kept in the zone. A clip of more than two
  channels is folded to stereo (`PreparedPcm::folded_to_stereo`): the mixer plays only the first two channels of a wider
  clip. MWIII Opus clips decoded at 48 kHz keep their whole length. Guest weapon sounds follow the effects slider.
- **No bank.** CoD4's aliases are not read, so its guns play the sounds of the first BO2 gun of their weapon type and class
  (`borrow_class_sounds`). IW7's borrow per gun (`IW7_GUNS.md`).

## Effects

`WeaponBuild::resolve_combat_fx` binds each gun's own effects first. `borrow_t6_combat_fx` then fills every slot still
unbound from the first BO2 gun of the same weapon type and class that has a flash:

- guns get the view and world flash, both shell ejects, the last-shot ejects (as a pair) and the tracer;
- launchers get the view and world flash and the explosion.

A borrowed slot is looked up in BO2's namespace (`borrowed`, `borrowed_ns`). Each resolve restores the gun's own names
first, so a late own effect wins. This covers the lanes that read no effects at all (Ghosts, BO3, CoD4, World at War) and
effects a lane names but does not find.

## Check it

- The match log has one line per game: `guest effects <game>: N weapons, own/fallback/none: vflash a/b/c, …`.
- `CodLegacy check-guns` prints the same per gun, from the lane's own effects only. It has no BO2 donors, so its `none` is
  `fallback` in a match.
- `CODLEGACY_HMW_ZONE=<dir> cargo test -p asset_audio --lib hmw_fire_alias_probe -- --ignored --nocapture` lists each Horizon
  MW gun's fire, raise and reload records: context, curve, layers and clip. `PROBE_OUT=<dir>` also dumps the clips.

## Gaps

- The sound context is always outdoors: indoor tails never play.
- Low-pass curves, volume modifiers (`volModIndex`) and master/slave ducking are not applied.
- Horizon MW's primed sounds would need its sound paks, which are not read. MWIII Opus clips of more than two channels do
  not decode.
