# Claude HUD for Horizon MW

Our in-match HUD (`crates/ui/src/hud_modern.rs`) rebuilt in H1's LUI for the
Horizon MW client: the score bar top centre, the gun's name over the ammo box
bottom right, his name over a health bar bottom left. It wraps H1's
`weaponInfoHudDef` and `teamScoresHudDef` and falls back to H1's own on any
error.

Built into the CoD-Legacy2 client (`CodLegacy.exe`) as a resource; release builds of the client do not read loose
`ui_scripts` folders (debug builds do: `<MWR folder>/ui_scripts/claude_hud/__init__.lua`).
Also in the owner's client fork (`sero7k/CoD-Legacy2`,
`data/cdata/ui_scripts/claude_hud`).
