# Claude UI for Horizon MW

Our front end's look (`crates/ui/src/shell`: `theme.rs`, `view.rs`) laid over
H1's own menus in the owner's client fork (`sero7k/CoD-Legacy2`,
`CodLegacy.exe`), so every menu keeps all it does (server browser, private
match, mods, Create-a-Class, options, pause menu) and only its look changes.

`claude_ui.lua` is built into the client as resource `CLAUDE_UI`
(`src/client/resources/ui_scripts/claude_ui.lua`), loaded after the game's
own LUI scripts and before `CLAUDE_HUD` (`tools/hmw_hud`). Keep both copies
in sync.

What it does, each part guarded (a failing hook leaves H1's piece as it was):

- Palette: every green colour in H1's `Colors` / `Swatches` /
  `CoD.HudStandards` (H1's `mw1_green` accent, the friendly colours) turned
  to the same lightness of our blue.
- Materials: `RegisterMaterial` is wrapped to remember names; images (their
  first state and every animation state naming a material) using H1's
  button / HUD-box materials are drawn as our flat colours instead, or left
  out (scanlines, glows, strokes, contours, glitch bars). Hidden pictures use
  `ui_transparent`.
- Menu rows (`LUI.H1ButtonBackground`): a white bar at the left, the accent's
  lighter blue and a point at the end on the row in focus; button text white
  in focus (`GenericButtonSettings`).
- Every `LUI.MenuTemplate` menu in the front end: a shade down the left and
  Infinite Warfare's lit band over the footer.
- Main menu: our CALL OF DUTY logo in the box MWR's logo had.
- In a match: H1's HUD boxes as our navy panels, the scoreboard's bars,
  headers and highlight in our colours.

How H1's menus were read: its LUI scripts are compiled HavokScript in
`patch_code_post_gfx_mp` / `patch_common_mp` / `patch_ui_mp`; a small reader
of the bytecode (function tree, constants per function) gave each function's
names and numbers, enough to find the types, events and materials used here.
