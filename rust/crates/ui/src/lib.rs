mod bo2_font;
#[cfg(feature = "jup")]
mod jup_pack; // bo2mp: a small copy of Modern Warfare III's guns, sounds, cards
mod word_split; // bo2mp: joined words parted (MWIII card names)
mod custom_cards; // bo2mp: the owner's own calling cards and emblem (CUSTOM)
mod bo_cards; // bo2mp: Black Ops' and Black Ops II's calling cards and emblems
mod bo2_globe;
mod cheat_menu; // bo2zm: the solo cheat menu (Num 1)
pub mod classes;
mod frame_rate; // bo2zm: Draw FPS, Max FPS
mod telemetry; // bo2zm: the Draw FPS overlay (Simple / Full telemetry)
pub mod frontend;
mod gap_hud;
mod launch_report;
mod layers;
mod load_table;
mod loading;
mod lui_hud; // bo2zm M4; bo2mp: the multiplayer front end and match HUD
mod lui_hud_zm; // bo2 merge: Zombies' own LUI front end and HUD (see lui_hud::zombies_turn)
mod menu;
mod menu_load;
mod options;
mod plugin;
mod screen;
mod scope_hint; // bo2mp scope lane
mod overhead_names; // bo2mp: names over heads in BO2's font
mod zm_hud; // bo2zm M4: the Zombies globe
mod oob_screen; // bo2mp: Black Ops III's out-of-bounds screen
mod foreign_maps; // bo2mp: other games' maps in the map list
mod game_folders; // bo2mp: the Game Folders panel
pub mod hud_modern; // bo2mp: the modern in-game HUD
pub mod hud_match; // bo2mp: our kill feed, hit marker, kill banner, scoreboard
mod hud_chat; // bo2mp: text chat (the lines received and the one typed)
mod hud_prints; // bo2mp: the scripts' iprintln / iprintlnbold lines
pub mod hud_nuke; // bo2mp: the Tactical Nuke's MOAB INBOUND banner
pub mod hud_medals; // bo2mp: Black Ops' medal notification
pub(crate) mod hud_cards; // bo2mp: MW2/MW3 calling cards and emblems in the HUD
mod jup_cards; // bo2mp: Modern Warfare III's (2023) emblems and calling cards (kept: owner, 5 Nov 2026)
mod picture_cache; // bo2mp: other games' menu pictures kept on disk
mod jup_hud; // bo2mp: Modern Warfare III's (2023) Warzone HUD pictures
// bo2mp: the small copy's stand-in without the `jup` feature (its guns and
// sounds are left out of the game).
#[cfg(not(feature = "jup"))]
mod dropped;
#[cfg(not(feature = "jup"))]
use dropped::jup_pack;
pub(crate) mod hud_warzone; // bo2mp: the Warzone HUD (Resurgence)
pub(crate) mod hud_prematch; // bo2mp: the match's start countdown
pub mod hud_outcome; // bo2mp: the free-for-all end screen
pub(crate) mod online_match; // bo2mp: the front end's matches through the master
pub mod shell; // bo2mp: the client's own front end and loading screen
mod music_screen; // bo2mp: front-end music context for the audio director
mod load_info; // bo2mp: the loading screen's info bar
mod pad_glyphs; // bo2mp: Xbox button icons on a controller
mod lui_additive; // bo2mp scope: true additive HUD pictures
mod lui_frontend; // bo2mp lane C
mod discord; // bo2mp: Discord Rich Presence
mod lui_scene; // bo2mp menus: the front end's 3D backdrop
mod bo2mp_damage; // bo2mp lane C
mod bo2mp_grenade; // bo2mp: the grenade warning
pub use lui_frontend::{Bo2mpFrontend, Bo2mpFrontendAssets, bo2mp_stats_file};
/// bo2mp `CodLegacy export-mw3 <folder>` (`jup_pack`).
pub use jup_pack::export_from_command_line as export_mw3_small_copy;

pub use classes::equip_txn::{
    EquipTxnWatch, apply_pending_class_equip, resolve_class_equip_transaction,
    sync_class_change_allowed,
};
pub use classes::icons::{
    ClassSelectIconCache, UiAssetRoot, cac_attachment_image, cac_material_iwd_stem,
    cac_weapon_image, pretty_weapon_name,
};
pub use classes::select::{
    ClassChangeAllowed, ClassChangeBlockReason, ClassEquipRefusal, ClassEquipRequest,
    ClassSelectHighlight, ClassSelectOverlayOpen, ClassSelectPhase, ClassSelectStatus,
    PendingClassEquip, accept_class_equip, class_index_by_name, commit_class_equip,
    reject_class_equip,
};
pub use classes::setup::{ClassEditRow, ClassLoadoutCatalog, ClassPickerFolder, ClassSlotState};
pub use classes::store::SessionClassStore;
pub use frame::{AppScreen, LaunchIdentity, LaunchReport};
pub use frame::{ClassPreset, showcase_classes};
pub use gap_hud::GapHud;
pub use lui_additive::AdditiveUi;
pub use overhead_names::{OverheadNameRow, OverheadNameRows};
pub use launch_report::publish_gap_hud;
pub use layers::{
    ApplyUiLayers, GameUiFont, UiCamera, UiDraw, UiLayer, UiLayerVisibility, UiLayers,
    game_text_font,
};
pub use loading::{LoadProgress, LoadingPreviewSource, LoadingScreen};
pub use menu::MenuMapList;
pub use options::{BindingView, PresentModeOverride};
pub use plugin::UiPlugin;
pub use screen::{layers_for_screen, sync_ui_layers};

pub use menu::install_frontend_menus;
