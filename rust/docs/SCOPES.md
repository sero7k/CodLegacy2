# Scopes, thermal and the Millimeter Scanner

How an aimed scope is drawn in a Black Ops II match, for BO2's own guns and
guest guns. The HUD side is `crates/hud` (`iris.rs`, `sight.rs`,
`scope_vision.rs`); the facts come from the weapon catalog (`asset_game`).

**Zoom and gun.** Aimed, the view zooms to the row's `ads_zoom_fov`
(`render_anim` `view_kick`). A row whose overlay material is set is a HUD
scope ("iris"): the view model hides as the zoom comes in and `iris.rs`
draws the picture with its black surround.

**Where the picture comes from.** `WeaponRegistry::overlay_namespace_of`:
a guest gun's borrowed BO2 sight (`guest.overlay_ns`) or its BO2 donor's
namespace, else its own. `T6` pictures are the BO2 lane's HUD icons
`overlay:<material>` (`t6_m2::hud_icons`, every zone's copy tried until one
decodes); other games' (MW2, MW3, CoD4, WaW, BO1) are their loose images.
A picture missing from the match falls back to BO2's own sniper scope
(`iris::fallback_scope_order`: DSR 50 / Ballista for a bolt action, SVU /
XPR otherwise) and logs `hud: scope "<m>" (...): drawn as overlay:<m>` once.

**Guest snipers.** Ghosts, AW, BO3, MWII/MWIII and MWR/Horizon MW scopes
are 3D lens models or pictures the HUD cannot read, so they aim through
BO2's sniper scope: a lane gives a sniper its own scope models
(`give_guest_scope`), or, found none, `guest_attach::sniper_scope_or` (BO2's
scope only); `build_guest_variants` copies the BO2 row's overlay, size,
zoom, reticle, thermal and scanner (`borrow_sight`). A row of those games
with an overlay of its own and nothing borrowed takes the scope as well.
MWII/MWIII guns are copies of a BO2 donor and show the donor's.

**A map's own copy of a gun.** The BO2 lane keeps the overlay an earlier
zone's copy names when a later zone's copy (a map zone) leaves it unset
(`WeaponCatalog::keep_t6_overlay`), for guns and sight variants.

**Thermal** (`WeaponBodyFacts::thermal_scope`): BO2's `bInfraRed`
(variant or attachment, the `ir` Dual Band), MW3's ADS overlay flag, a row
named `_thermal` / `+thermal` (or `_ir` for Black Ops), and guest
`thermal` sights (a sniper's aimed through BO2's `dsr50_mp+ir`, any other gun's through `an94_mp+ir`). Aimed, the film grade (`film_vision_view.rs`: grey, hard contrast, dark) and `scope_vision.rs` darken
the frame under the scope picture and draws everyone in line of sight as a
white-hot figure (Cold Blooded stays cold). It is a HUD veil, not BO2's
per-material thermal pass (`Material.thermalMaterial` is not read yet).

**Millimeter Scanner** (`WeaponBodyFacts::mms`: `mmsWeapon`, `mmsFOV`,
`mmsAspect`, `mmsMaxDist`, `mmsInScope`; stand-ins when 0): aimed, a screen
centred on the aim scans its own field of view and shows enemies within
range as figures, through walls. Its size on the frame is a choice (36% of
the height, 72% in a scope), not read from the sight model.

`CODLEGACY_SCOPELOG=1` logs `scope: <gun> vision ...` and `scope: <gun> thermal grade on/off`.

Logs to check in a match: `t6 scope overlays: N`, `t6 scope overlay <m>:`,
`thermal scopes named by their weapons:`, `bo2mp guest attachments:`, and
the HUD gap `ads-overlay`.
