# In-game HUD audit (2026-10-25)

Read from the code only (no game data on the box; not yet tested on the PC).
Two faces are the project's own: **Barlow** (bundled `crates/ui/assets/BarlowCondensed-600.ttf`,
`GameUiFont` / `ShellFonts`, helper `layers::game_text_font`; used by the FFA outcome
screen, medal text, kill feed, prints, chat, pause menu) and the **BO2 sheet font**
(`bo2_font::Bo2Fonts`, BO2's own glyph sheet read from the game files; used where the
HUD copies BO2's measured layout). No text anywhere falls back to Bevy's default font:
every `Text`/`TextSpan` in `crates/ui` and `crates/console` names a font (checked by a scan).

| element | where drawn | before | done | left, and why |
|---|---|---|---|---|
| Scope hints ("Press SHIFT and hold to steady", "Press V to zoom") | `ui/scope_hint.rs` | BO2 sheet font | now Barlow, centred, same wording, key colour kept | - |
| Use hint ("Hold [F] for ...") in a multiplayer match | `ui/zm_hud.rs` `zm_hud` | BO2 sheet font | MP now takes the Barlow path; Zombies keeps BO2 font | - |
| Overhead player names | `ui/overhead_names.rs` | BO2 sheet font | now Barlow, centred in a 480 px box, team colour kept | size vs BO2 font to eyeball on PC |
| Draw FPS counter | `ui/frame_rate.rs` | BO2 sheet font | now Barlow, same colours | - |
| Script HUD text (hudelems: "Waiting for players", prompts, timers, perk list) | `ui/zm_hud.rs` `zm_script_hud` | BO2 sheet font, placed to BO2's measured metrics | not changed | Calibrated against real captures (key-prompt size, outcome layout); a font swap shifts it and cannot be judged without the PC. Barlow fallback path exists in the same function. Next step |
| Zombies HUD (round, points, ammo, popups, waypoints) | `ui/zm_hud.rs`, `lui_hud_zm.rs` | BO2 font / BO2 LUI | not changed | Zombies' own classic look, not the MP HUD |
| BO2 LUI match HUD (script-driven widgets not replaced) | `ui/lui_hud.rs`, `hks_t6` | BO2 LUI, BO2 font | not changed | Replaced already: ammo, score, minimap, scorestreaks (`hide_for_bo1_hud`), kill feed, hit marker, killcam, scoreboard, outcome, medals, prints, chat. Which other BO2 widgets still draw in MP needs a run on the PC (log lines `bo2mp modern hud: ... hidden`) |
| Modern HUD text (score bar, ammo, compass) | `ui/hud_modern.rs` | BO2 font when loaded, Barlow stand-in | not changed | Part of the project HUD; sheet font chosen on purpose |
| Overhead/aim helpers `hud_warzone` markers | `ui/hud_warzone.rs` | BO2 font | not changed | Resurgence only |
| MW2 match HUD (score bar, killfeed, scoreboard, centerprint, hints, script menus) | `crates/hud/*`, `hud_iw4` | MW2 menu font | not changed | Off in every launch mode (`plugin.rs` `mw2_hud()` = `!t6_rules()`, always true-rules), so nothing draws; reticle/scope/sight/flash stay (pictures only). Dead code, removal not clearly safe (shared tessellation) |
| Developer console, `showpos` overlay | `console/plugin/mod.rs`, `debug_move.rs` | embedded mono font | not changed | Developer tools (backtick / command), not match UI |
| Cheat menu (Num 1, solo) | `ui/cheat_menu.rs` | BO2 sheet font | not changed | Developer tool for recording; solo only |
| Launch gap overlay | `ui/gap_hud.rs` | Barlow (bevy default only if font missing) | not changed | Not shown in a match or the main menu |
| Pause menu, Choose Class, loading, end-of-match, medals, chat, killcam | `ui/shell/*`, `hud_*.rs` | Barlow | already done | - |
| Create-a-Class | `ui/shell/cac.rs`, `classes/` | Barlow | untouched (as asked) | - |
| HMW client UI/HUD | `tools/hmw_ui`, `tools/hmw_hud` (Lua restyle inside the owner's fork) | H1 menus recoloured | not changed | Not in this repo's runtime; replacing H1's menus is a fork change |
| `eprintln!`/`println!` | many crates | stdout only | not changed | Never drawn to the player |
