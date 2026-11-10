use std::{fs, path::PathBuf};

use bevy::{
    audio::{AudioSink, AudioSinkPlayback, GlobalVolume, Volume},
    input::{
        ButtonInput,
        keyboard::KeyCode,
        mouse::{MouseButton, MouseWheel},
    },
    prelude::*,
};

use crate::{BindButton, KeyBinds, binds::wheel_button, display_button};

#[derive(Resource, Default)]
pub(crate) struct PendingMenuBinding {
    id: Option<u32>,
    armed: bool,
    /// bo2mp: started by the front end's KEY BINDINGS page
    /// (`frame::UiBindEdit::Listen`): keyboard and mouse only (the pad's B
    /// cancels), Delete clears, a key taken from another command swaps.
    keys_only: bool,
}

#[derive(Resource, Default)]
pub(crate) struct UserSettingsPersistence {
    path: Option<PathBuf>,
    last_payload: Option<String>,
}

#[derive(Resource)]
pub(crate) struct AppliedMasterVolume(f32);

pub(crate) fn load_user_settings(
    identity: Res<ui::LaunchIdentity>,
    mut settings: ResMut<frame::GameSettings>,
    mut binds: ResMut<KeyBinds>,
    mut persistence: ResMut<UserSettingsPersistence>,
) {
    let Some(path) = settings_path(&identity.artifacts) else {
        warn!("no HOME or XDG_CONFIG_HOME; user settings are session-only");
        return;
    };
    persistence.path = Some(path.clone());
    match fs::read_to_string(&path) {
        Ok(source) => parse_settings(&source, &mut settings, &mut binds),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => warn!("could not read {}: {error}", path.display()),
    }
    settings.sanitize();
    // bo2mp: the game always boots borderless fullscreen (Options can still
    // make this session windowed).
    settings.fullscreen = true;
    // bo2mp: the upscaler start-up guard. `BO2MP_UPSCALE_SAFE=1` forces Off
    // for this run; a "running" marker left by the last run means it ended
    // with an upscaler on and did not exit cleanly, so this run starts with
    // upscaling Off, and saves that (`upscaler=off`).
    let upscaler_was_reset = match frame::upscale_guard::startup(
        &diag::profile_dir(),
        frame::upscale_guard::safe_env(),
    ) {
        frame::upscale_guard::Startup::Clean => false,
        frame::upscale_guard::Startup::SafeMode => {
            frame::Upscaler::force_off(true);
            warn!(
                "bo2mp upscale: BO2MP_UPSCALE_SAFE is set; upscaling is Off for this run (the saved setting stays {})",
                settings.upscaler.key()
            );
            false
        }
        frame::upscale_guard::Startup::Crashed(what) => {
            warn!(
                "bo2mp upscale: the last run did not exit cleanly with an upscaler on ({what}); \
                 starting with upscaling Off (setting was {}). Turn it on again in Options > Advanced Video; \
                 BO2MP_UPSCALE_SAFE=1 keeps it off.",
                settings.upscaler.key()
            );
            let was_on = settings.upscaler.active();
            settings.upscaler = frame::Upscaler::Off;
            was_on
        }
    };
    persistence.last_payload = Some(serialize_settings(&settings, &binds));
    if upscaler_was_reset {
        // (Saved with the next pass of the writer, which sees a changed payload.)
        settings.touch();
        persistence.last_payload = None;
    }
    // bo2mp: his friend code, made the first time (saved with the next
    // change of settings, and at once here).
    if settings.friend_code == 0 {
        settings.friend_code = new_friend_code();
        settings.touch();
        persistence.last_payload = None;
    }
}

/// bo2mp: a fresh 8-digit friend code (from the clock, the process and the
/// standard library's random hasher seed).
fn new_friend_code() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    10_000_000 + (h.finish() % 90_000_000) as u32
}

pub(crate) fn consume_menu_binding(
    mut intents: MessageReader<frame::UiBindRequest>,
    mut edits: MessageReader<frame::UiBindEdit>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    (gamepads, active, mut settings): (
        Query<&bevy::input::gamepad::Gamepad>,
        Res<frame::ActivePad>,
        ResMut<frame::GameSettings>,
    ),
    mut pending: ResMut<PendingMenuBinding>,
    mut capture: ResMut<frame::UiBindingCapture>,
    mut binds: ResMut<KeyBinds>,
    mut view: ResMut<ui::BindingView>,
) {
    capture.consumed_input = false;
    if capture.command.is_none() {
        pending.id = None;
        view.listening = None;
    }
    let wheel_direction = wheel
        .read()
        .fold(None, |first, event| first.or_else(|| wheel_button(event.y)));
    let mut began = false;
    // bo2mp: the front end's KEY BINDINGS page: listen (keys only), clear a
    // command's keys, Black Ops II's keys back.
    let mut listens: Vec<(String, bool)> = Vec::new();
    for edit in edits.read() {
        match edit {
            frame::UiBindEdit::Listen(command) => listens.push((command.clone(), true)),
            frame::UiBindEdit::Clear(command) => {
                if let Some(id) = input_iw4::command_id_lookup(command) {
                    binds.clear_command_on(id, false);
                    view.revision = view.revision.wrapping_add(1);
                }
            }
            frame::UiBindEdit::ResetKeys => {
                binds.reset_keys();
                view.revision = view.revision.wrapping_add(1);
            }
        }
    }
    listens.extend(intents.read().map(|intent| (intent.command.clone(), false)));
    for (command, keys_only) in listens {
        if let Some(id) = input_iw4::command_id_lookup(&command) {
            capture.command = Some(command);
            pending.id = Some(id);
            pending.keys_only = keys_only;
            view.listening = Some(id);
            view.revision = view.revision.wrapping_add(1);
            pending.armed = false;
            began = true;
        } else {
            capture.command = None;
            pending.id = None;
            view.listening = None;
        }
    }
    let Some(id) = pending.id else { return };
    if began || !pending.armed {
        pending.armed = true;
        return;
    }
    let pad = active.0.and_then(|entity| gamepads.get(entity).ok());
    let pad_start = pad.is_some_and(|pad| {
        pad.just_pressed(bevy::input::gamepad::GamepadButton::Start)
            || (pending.keys_only && pad.just_pressed(bevy::input::gamepad::GamepadButton::East))
    });
    let pad_button = pad.and_then(|pad| {
        pad.get_just_pressed()
            .copied()
            .filter_map(crate::PadButton::from_gamepad_button)
            .find(|button| *button != crate::PadButton::Start)
    });
    if let Some(button) = pad_button.filter(|_| !pad_start && !pending.keys_only) {
        capture.command = None;
        capture.consumed_input = true;
        binds.clear_command_on(id, true);
        binds.set(BindButton::Pad(button), id);
        if settings.pad_layout != frame::GameSettings::PAD_LAYOUT_CUSTOM {
            settings.pad_layout = frame::GameSettings::PAD_LAYOUT_CUSTOM;
            settings.touch();
        }
        pending.id = None;
        pending.armed = false;
        view.listening = None;
        view.revision = view.revision.wrapping_add(1);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || pad_start {
        capture.command = None;
        capture.consumed_input = true;
        pending.id = None;
        pending.armed = false;
        view.listening = None;
        view.revision = view.revision.wrapping_add(1);
        return;
    }
    // (KEY BINDINGS: Delete clears the command's keys.)
    if pending.keys_only && keys.just_pressed(KeyCode::Delete) {
        capture.command = None;
        capture.consumed_input = true;
        binds.clear_command_on(id, false);
        pending.id = None;
        pending.armed = false;
        view.listening = None;
        view.revision = view.revision.wrapping_add(1);
        return;
    }
    let button = keys
        .get_just_pressed()
        .copied()
        .map(BindButton::Key)
        .next()
        .or_else(|| {
            mouse
                .get_just_pressed()
                .copied()
                .map(BindButton::Mouse)
                .next()
        })
        .or(wheel_direction);
    let Some(button) = button else { return };
    capture.command = None;
    capture.consumed_input = true;
    if pending.keys_only {
        view.taken = binds
            .rebind_key(id, button)
            .and_then(|(other, swapped)| input_iw4::command_name(other).map(|name| (name.to_owned(), swapped)));
    } else {
        binds.clear_command_on(id, false);
        binds.set(button, id);
    }
    pending.id = None;
    pending.armed = false;
    view.listening = None;
    view.revision = view.revision.wrapping_add(1);
}

pub(crate) fn sync_binding_view(
    binds: Res<KeyBinds>,
    devices: Res<frame::InputDevices>,
    mut view: ResMut<ui::BindingView>,
    mut dvars: ResMut<frame::UiMenuDvars>,
    mut style: Local<Option<frame::PromptStyle>>,
) {
    if !binds.is_changed() && *style == Some(devices.style) {
        return;
    }
    *style = Some(devices.style);
    let mut chords = std::collections::BTreeMap::<u32, (Vec<String>, Vec<String>)>::new();
    for (button, id) in binds.iter() {
        let (keys, buttons) = chords.entry(id).or_default();
        match button {
            BindButton::Pad(pad) => buttons.push(pad.prompt(devices.style).to_owned()),
            _ => keys.push(display_button(button)),
        }
    }
    view.chords.clear();
    view.keys.clear();
    for (id, (mut keys, mut buttons)) in chords {
        keys.sort();
        keys.dedup();
        buttons.sort();
        buttons.dedup();
        if !keys.is_empty()
            && let Some(name) = input_iw4::command_name(id)
        {
            view.keys.insert(name.to_owned(), keys.join(" OR "));
        }
        let text = match (keys.is_empty(), buttons.is_empty()) {
            (false, false) => format!("{} | {}", keys.join(" OR "), buttons.join(" OR ")),
            (false, true) => keys.join(" OR "),
            (true, false) => buttons.join(" OR "),
            (true, true) => continue,
        };
        view.chords.insert(id, text);
    }
    // Publish every command so removing its last binding clears the old label.
    for (id, command) in input_iw4::INPUT_COMMAND_NAMES.iter().enumerate().skip(1) {
        dvars.set(&format!("ui_bind_{command}"), view.chord(id as u32));
    }
    view.revision = view.revision.wrapping_add(1);
}

pub(crate) fn apply_master_volume(
    settings: Res<frame::GameSettings>,
    mut global: ResMut<GlobalVolume>,
    mut sinks: Query<(Entity, &mut AudioSink, &bevy::audio::PlaybackSettings)>,
    mut muted_volumes: Local<std::collections::HashMap<Entity, f32>>,
    applied: Option<ResMut<AppliedMasterVolume>>,
    mut commands: Commands,
) {
    if !settings.is_changed() {
        return;
    }
    // bo2mp: the BO3 native renderer option, read by the map loader.
    frame::settings::T7_NATIVE_RENDER.store(settings.t7_native, std::sync::atomic::Ordering::Relaxed);
    frame::settings::RT_LIGHTING.store(settings.rt_lighting, std::sync::atomic::Ordering::Relaxed);
    // bo2mp: what the native renderers draw with (the preset, the rows).
    settings.publish_native_quality();
    let previous = applied.as_ref().map_or(1.0, |value| value.0);
    let next = settings.master_volume;
    global.volume = Volume::Linear(next);
    muted_volumes.retain(|entity, _| sinks.contains(*entity));
    for (entity, mut sink, playback) in &mut sinks {
        let base = if previous > f32::EPSILON {
            sink.volume().to_linear() / previous
        } else {
            muted_volumes
                .get(&entity)
                .copied()
                .unwrap_or(playback.volume.to_linear())
        };
        if next == 0.0 {
            muted_volumes.insert(entity, base);
        }
        sink.set_volume(Volume::Linear(base * next));
    }
    if next > 0.0 {
        muted_volumes.clear();
    }
    if let Some(mut applied) = applied {
        applied.0 = next;
    } else {
        commands.insert_resource(AppliedMasterVolume(next));
    }
}

pub(crate) fn sync_player_name(
    settings: Res<frame::GameSettings>,
    generation: Res<frame::WorldGeneration>,
    has_world: Res<frame::HasWorld>,
    role: Res<frame::RuntimeRole>,
    local: Option<Res<net::LocalPresentClient>>,
    link: Option<Res<net::UdpClientLink>>,
    mut inbox: Option<ResMut<net::ClientActionInbox>>,
    mut seq: ResMut<net::ActionRequestIds>,
    mut sent: Local<Option<(frame::WorldGeneration, sim::ClientId, [u8; 16])>>,
) {
    if !has_world.0 || *role == frame::RuntimeRole::Replay {
        *sent = None;
        return;
    }
    let (Some(local), Some(inbox)) = (local, inbox.as_deref_mut()) else {
        return;
    };
    if let Some(link) = link
        && (link.connection.is_none()
            || link.assigned_client != Some(local.0)
            || !link.has_entered_match())
    {
        *sent = None;
        return;
    }
    let name = entity_iw4::pack_client_state_name(&settings.shown_name());
    let next = (*generation, local.0, name);
    if sent.as_ref() == Some(&next) {
        return;
    }
    let request_id = seq.allocate();
    if let Err(error) = inbox.push(local.0, sim::ClientAction::SetName { request_id, name }) {
        diag::warn!(
            Console,
            "name: request_id={request_id} not queued — {error}"
        );
    } else {
        *sent = Some(next);
    }
}

pub(crate) fn save_user_settings(
    settings: Res<frame::GameSettings>,
    binds: Res<KeyBinds>,
    mut persistence: ResMut<UserSettingsPersistence>,
) {
    if !settings.is_changed() && !binds.is_changed() {
        return;
    }
    let payload = serialize_settings(&settings, &binds);
    if persistence.last_payload.as_deref() == Some(payload.as_str()) {
        return;
    }
    let Some(path) = persistence.path.clone() else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if let Err(error) = fs::create_dir_all(parent) {
        warn!("could not create {}: {error}", parent.display());
        return;
    }
    let temporary = path.with_extension("cfg.tmp");
    if let Err(error) =
        fs::write(&temporary, payload.as_bytes()).and_then(|()| fs::rename(&temporary, &path))
    {
        warn!("could not atomically save {}: {error}", path.display());
        return;
    }
    persistence.last_payload = Some(payload);
}

/// bo2mp: the GRAPHICS API in the settings file, read before the App is built
/// (the backend is chosen when the renderer starts). Auto when there is no
/// file or no such line.
pub fn early_graphics_api() -> frame::settings::GraphicsApi {
    let Some(path) = settings_path(std::path::Path::new("")) else {
        return frame::settings::GraphicsApi::Auto;
    };
    let Ok(source) = fs::read_to_string(path) else {
        return frame::settings::GraphicsApi::Auto;
    };
    source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("graphics_api="))
        .filter_map(frame::settings::GraphicsApi::from_key)
        .next_back()
        .unwrap_or_default()
}

fn settings_path(artifacts: &std::path::Path) -> Option<PathBuf> {
    if let Some(path) = codlegacy_env::var_os("CODLEGACY_SETTINGS_PATH") {
        return Some(PathBuf::from(path));
    }
    if cfg!(windows) {
        let _ = artifacts;
        return Some(diag::profile_file("settings.cfg"));
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".config")))
        .map(|path| path.join("iw4l/settings.cfg"))
}

pub(crate) fn apply_pad_layout_setting(
    settings: Res<frame::GameSettings>,
    mut binds: ResMut<KeyBinds>,
    mut seen: Local<Option<u8>>,
) {
    let layout = settings.pad_layout;
    let previous = seen.replace(layout);
    if previous.is_none()
        || previous == Some(layout)
        || layout == frame::GameSettings::PAD_LAYOUT_CUSTOM
    {
        return;
    }
    binds.apply_pad_layout(usize::from(layout));
}

fn parse_into<T: std::str::FromStr>(value: &str, slot: &mut T) {
    if let Ok(value) = value.parse() {
        *slot = value;
    }
}

fn serialize_settings(settings: &frame::GameSettings, binds: &KeyBinds) -> String {
    let safe_name = settings.player_name.replace(['\n', '\r', '='], " ");
    let mut lines = vec![
        "// CodLegacy user settings v2".to_owned(),
        format!(
            "resolution={}x{}",
            settings.resolution.width, settings.resolution.height
        ),
        format!("fullscreen={}", settings.fullscreen),
        format!("vsync={}", settings.vsync),
        format!("master_volume={:.3}", settings.master_volume),
        format!("music_volume={:.3}", settings.music_volume),
        format!("match_music={}", settings.match_music),
        format!("map_intro_music={}", settings.map_intro_music),
        format!("sfx_volume={:.3}", settings.sfx_volume),
        format!("voice_volume={:.3}", settings.voice_volume),
        format!("cinematic_volume={:.3}", settings.cinematic_volume),
        format!("brightness={:.3}", settings.brightness),
        format!("fov={:.0}", settings.fov),
        format!("shadows={}", settings.shadows),
        format!("depth_of_field={}", settings.depth_of_field),
        format!("bloom={}", settings.bloom),
        format!("motion_blur={}", settings.motion_blur),
        format!("sensitivity={:.3}", settings.sensitivity),
        format!("invert_mouse={}", settings.invert_mouse),
        format!("player_name={safe_name}"),
        format!("pad_layout={}", settings.pad_layout),
        format!("pad_stick_layout={}", settings.pad_stick_layout),
        format!("pad_sensitivity={:.2}", settings.pad_sensitivity),
        format!("pad_ads_sensitivity={:.2}", settings.pad_ads_sensitivity),
        format!("pad_invert={}", settings.pad_invert),
        format!("pad_curve={}", settings.pad_curve),
        format!("pad_aim_assist={}", settings.pad_aim_assist),
        format!("pad_prompts={}", settings.pad_prompts),
        format!("pad_vibration={}", settings.pad_vibration),
        format!("pad_deadzone_left={:.2}", settings.pad_deadzone_left),
        format!("pad_deadzone_right={:.2}", settings.pad_deadzone_right),
        format!(
            "bo2_quality={}",
            if settings.bo2_fast { "fast" } else { "full" }
        ),
        format!("map_game_bodies={}", settings.map_game_bodies),
        format!("soldiers={}", settings.soldiers.key()),
        format!("animated_cards={}", settings.animated_cards),
        format!("classic_menu={}", settings.classic_menu),
        format!("chat_tts={}", settings.chat_tts),
        format!("t7_native={}", settings.t7_native),
        format!("prepare_maps_at_launch={}", settings.prepare_maps_at_launch),
        format!("prepare_games_off={}", settings.prepare_games_off),
        format!("rt_lighting={}", settings.rt_lighting),
        format!("graphics_quality={}", settings.graphics_quality.key()),
        format!("friend_code={}", settings.friend_code),
        format!(
            "friends={}",
            settings.friends.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        ),
        format!("mature={}", settings.mature),
        format!("corpses={}", settings.corpses),
        format!("draw_fps={}", settings.draw_fps),
        format!("fps_full={}", settings.fps_full),
        format!("fps_left={}", settings.fps_left),
        format!("max_fps={}", settings.max_fps),
        format!("fxaa={}", settings.fxaa),
        format!("aa_samples={}", settings.aa_samples),
        format!("tex_filter={}", settings.tex_filter),
        format!("tex_quality={}", settings.tex_quality),
        format!("upscaler={}", settings.upscaler.key()),
        format!("upscale_quality={}", settings.upscale_quality.key()),
        format!("upscale_sharpness={:.2}", settings.upscale_sharpness),
        format!("graphics_api={}", settings.graphics_api.key()),
        format!("binds_layout={BINDS_LAYOUT}"),
        "unbindall".to_owned(),
    ];
    lines.extend(binds.list_lines());
    lines.push(String::new());
    lines.join("\n")
}

/// bo2zm: the saved binds' layout. 4 = Black Ops II's own PC keys
/// (`default_controls.cfg`, from the retail bindings_mp.bdg); a file with an
/// older layout (or none, layout 1) is moved over on load.
const BINDS_LAYOUT: u32 = 4;

/// Older layouts' keyboard keys go back to Black Ops II's PC defaults (his
/// 10-07 "use black ops defaults": the old E use / F frag were our own
/// defaults, not his picks). Saved pad buttons stay.
fn migrate_binds(script: &str, from_layout: u32) -> String {
    if from_layout >= BINDS_LAYOUT {
        return script.to_owned();
    }
    let mut out = String::from("unbindall\n");
    let defaults = crate::binds::DEFAULT_CONTROLS
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("bind "));
    let pad = script.lines().map(str::trim).filter(|l| {
        let mut parts = l.split(' ');
        parts.next() == Some("bind")
            && parts.next().is_some_and(|k| {
                let k = k.to_ascii_uppercase();
                k.starts_with("BUTTON_") || k.starts_with("DPAD_")
            })
    });
    for line in defaults.chain(pad) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn parse_settings(source: &str, settings: &mut frame::GameSettings, binds: &mut KeyBinds) {
    let mut bind_script = String::new();
    let mut layout = 1u32;
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with("bind ") || line == "unbindall" {
            bind_script.push_str(line);
            bind_script.push('\n');
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            warn!("ignored malformed setting line: {line}");
            continue;
        };
        match key {
            "binds_layout" => layout = value.trim().parse().unwrap_or(1),
            "resolution" => {
                if let Some((w, h)) = value.split_once('x')
                    && let (Ok(width), Ok(height)) = (w.parse(), h.parse())
                {
                    settings.resolution = frame::DisplayResolution::new(width, height);
                }
            }
            "fullscreen" => {
                if let Ok(value) = value.parse() {
                    settings.fullscreen = value;
                }
            }
            "vsync" => {
                if let Ok(value) = value.parse() {
                    settings.vsync = value;
                }
            }
            "fov" => {
                if let Ok(v) = value.parse() {
                    settings.fov = v;
                }
            }
            "brightness" => {
                if let Ok(v) = value.parse() {
                    settings.brightness = v;
                }
            }
            "shadows" => {
                if let Ok(v) = value.parse() {
                    settings.shadows = v;
                }
            }
            "depth_of_field" => {
                if let Ok(v) = value.parse() {
                    settings.depth_of_field = v;
                }
            }
            "bloom" => {
                if let Ok(v) = value.parse() {
                    settings.bloom = v;
                }
            }
            "motion_blur" => {
                if let Ok(v) = value.parse() {
                    settings.motion_blur = v;
                }
            }
            "master_volume" => {
                if let Ok(value) = value.parse() {
                    settings.master_volume = value;
                }
            }
            "music_volume" => parse_into(value, &mut settings.music_volume),
            "match_music" => parse_into(value, &mut settings.match_music),
            "map_intro_music" => parse_into(value, &mut settings.map_intro_music),
            "sensitivity" => {
                if let Ok(value) = value.parse() {
                    settings.sensitivity = value;
                }
            }
            "invert_mouse" => {
                if let Ok(value) = value.parse() {
                    settings.invert_mouse = value;
                }
            }
            "player_name" => settings.player_name = value.to_owned(),
            "sfx_volume" => parse_into(value, &mut settings.sfx_volume),
            "voice_volume" => parse_into(value, &mut settings.voice_volume),
            "cinematic_volume" => parse_into(value, &mut settings.cinematic_volume),
            "pad_layout" => parse_into(value, &mut settings.pad_layout),
            "pad_stick_layout" => parse_into(value, &mut settings.pad_stick_layout),
            "pad_sensitivity" => parse_into(value, &mut settings.pad_sensitivity),
            "pad_ads_sensitivity" => parse_into(value, &mut settings.pad_ads_sensitivity),
            "pad_invert" => parse_into(value, &mut settings.pad_invert),
            "pad_curve" => parse_into(value, &mut settings.pad_curve),
            "pad_aim_assist" => parse_into(value, &mut settings.pad_aim_assist),
            "pad_prompts" => parse_into(value, &mut settings.pad_prompts),
            "pad_vibration" => parse_into(value, &mut settings.pad_vibration),
            // The removed "Omni Movement" (not Black Ops II's): an older
            // settings file's line is dropped without a warning.
            "omni_movement" => {}
            "map_game_bodies" => parse_into(value, &mut settings.map_game_bodies),
            "soldiers" => {
                if let Some(soldiers) = frame::settings::Soldiers::from_key(value) {
                    settings.soldiers = soldiers;
                }
            }
            // bo2mp: the earlier on/off choice. Its "on" was the default
            // every settings file was written with, not a pick: it reads as
            // the default (Black Ops' soldiers); "off" was Black Ops II's.
            "mw3_operators" => {
                if value.trim() == "false" {
                    settings.soldiers = frame::settings::Soldiers::BlackOps2;
                }
            }
            "animated_cards" => parse_into(value, &mut settings.animated_cards),
            "classic_menu" => parse_into(value, &mut settings.classic_menu),
            "chat_tts" => parse_into(value, &mut settings.chat_tts),
            "t7_native" => parse_into(value, &mut settings.t7_native),
            "prepare_maps_at_launch" => parse_into(value, &mut settings.prepare_maps_at_launch),
            "prepare_games_off" => settings.prepare_games_off = value.to_owned(),
            "rt_lighting" => parse_into(value, &mut settings.rt_lighting),
            "graphics_quality" => {
                if let Some(p) = frame::settings::native_quality::Preset::from_key(value) {
                    settings.graphics_quality = p;
                }
            }
            "friend_code" => parse_into(value, &mut settings.friend_code),
            "friends" => {
                settings.friends = value.split(',').filter_map(|c| c.trim().parse().ok()).collect();
            }
            "pad_deadzone_left" => parse_into(value, &mut settings.pad_deadzone_left),
            "pad_deadzone_right" => parse_into(value, &mut settings.pad_deadzone_right),
            // bo2zm: Black Ops II quality, `full` or `fast`.
            "bo2_quality" => {
                settings.bo2_fast = value.trim().eq_ignore_ascii_case("fast");
                asset_core::t6_set_fast(settings.bo2_fast);
            }
            "mature" => parse_into(value, &mut settings.mature),
            "corpses" => parse_into(value, &mut settings.corpses),
            "draw_fps" => parse_into(value, &mut settings.draw_fps),
            "fps_full" => parse_into(value, &mut settings.fps_full),
            "fps_left" => parse_into(value, &mut settings.fps_left),
            "max_fps" => parse_into(value, &mut settings.max_fps),
            "fxaa" => parse_into(value, &mut settings.fxaa),
            "aa_samples" => parse_into(value, &mut settings.aa_samples),
            "upscaler" => {
                if let Some(v) = frame::settings::Upscaler::from_key(value) {
                    settings.upscaler = v;
                }
            }
            "upscale_quality" => {
                if let Some(v) = frame::settings::UpscaleQuality::from_key(value) {
                    settings.upscale_quality = v;
                }
            }
            "upscale_sharpness" => parse_into(value, &mut settings.upscale_sharpness),
            "graphics_api" => {
                if let Some(v) = frame::settings::GraphicsApi::from_key(value) {
                    settings.graphics_api = v;
                }
            }
            // Texture filtering: the samplers made at load.
            "tex_filter" => {
                parse_into(value, &mut settings.tex_filter);
                asset_core::t6_set_aniso(match settings.tex_filter {
                    0 => 1,
                    1 => 4,
                    _ => 16,
                });
            }
            "tex_quality" => parse_into(value, &mut settings.tex_quality),
            _ => warn!("ignored unknown setting `{key}`"),
        }
    }
    if !bind_script.is_empty() && layout < BINDS_LAYOUT {
        bind_script = migrate_binds(&bind_script, layout);
        info!("saved binds moved to Black Ops II's keys (layout {layout} -> {BINDS_LAYOUT})");
    }
    if !bind_script.is_empty() {
        let warnings = binds.apply_config_script(&bind_script);
        for warning in warnings {
            warn!("settings bind: {warning}");
        }
    }
    if !binds.has_pad_binds() {
        binds.apply_pad_layout(usize::from(settings.pad_layout.min(4)));
    }
    if source
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "// IW4L user settings v1")
        && binds.get(BindButton::Key(KeyCode::Digit4)).is_none()
        && !binds.iter().any(|(_, id)| id == 21)
    {
        binds.set(BindButton::Key(KeyCode::Digit4), 21);
    }
}

pub(crate) fn native_menu_settings(
    mut events: MessageReader<crate::ConsoleCommand>,
    mut settings: ResMut<frame::GameSettings>,
    mut dvars: ResMut<frame::UiMenuDvars>,
    mut shadows: ResMut<render_frontend::prepare::scene::view_parms::SmEnableDvar>,
    mut dof: ResMut<render_frontend::assemble::drawsurf::dof::DofDvars>,
    mut glow: ResMut<render_frontend::assemble::drawsurf::dof::GlowDvars>,
    mut test_rumble: MessageWriter<frame::TestControllerRumble>,
) {
    for command in events.read() {
        if !matches!(command.name.as_str(), "set" | "seta") {
            continue;
        }
        let [name, value, ..] = command.args.as_slice() else {
            continue;
        };
        match name.as_str() {
            "ui_r_mode" => {
                if let Some((w, h)) = value.split_once('x')
                    && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
                {
                    settings.resolution = frame::DisplayResolution::new(w, h);
                }
            }
            "ui_r_displayMode" => settings.fullscreen = value == "1",
            "ui_r_vsync" => settings.vsync = value == "1",
            "ui_volume" => {
                if let Ok(v) = value.parse::<f32>()
                    && v.is_finite()
                {
                    settings.master_volume = v;
                }
            }
            "ui_player_name" => settings.player_name = value.clone(),
            "ui_fov" => {
                if let Ok(v) = value.parse::<f32>() {
                    settings.fov = v;
                }
            }
            "ui_brightness" => {
                if let Ok(v) = value.parse::<f32>()
                    && v.is_finite()
                {
                    settings.brightness = v;
                }
            }
            "ui_shadows" => settings.shadows = value == "1",
            "ui_dof" => settings.depth_of_field = value == "1",
            "ui_bloom" => settings.bloom = value == "1",
            "ui_pad_layout" => parse_into(value, &mut settings.pad_layout),
            "ui_pad_stick_layout" => parse_into(value, &mut settings.pad_stick_layout),
            "ui_pad_sensitivity" => parse_into(value, &mut settings.pad_sensitivity),
            "ui_pad_ads_sensitivity" => parse_into(value, &mut settings.pad_ads_sensitivity),
            "ui_pad_invert" => settings.pad_invert = value == "1",
            "ui_pad_curve" => parse_into(value, &mut settings.pad_curve),
            "ui_pad_aim_assist" => parse_into(value, &mut settings.pad_aim_assist),
            "ui_pad_prompts" => parse_into(value, &mut settings.pad_prompts),
            "ui_pad_test_rumble" => {
                if value == "1" {
                    test_rumble.write(frame::TestControllerRumble);
                }
            }
            "ui_pad_vibration" => settings.pad_vibration = value == "1",
            "ui_pad_deadzone_left" => parse_into(value, &mut settings.pad_deadzone_left),
            "ui_pad_deadzone_right" => parse_into(value, &mut settings.pad_deadzone_right),
            _ => continue,
        }
        settings.sanitize();
        settings.touch();
    }
    if settings.is_changed() {
        shadows.enabled = Some(settings.shadows);
        dof.enable = settings.depth_of_field;
        glow.enable = settings.bloom;
    }
    dvars.set("ui_volume", settings.master_volume.to_string());
    dvars.set("ui_brightness", settings.brightness.to_string());
    dvars.set("ui_fov", settings.fov.to_string());
    dvars.set("ui_player_name", settings.player_name.clone());
    dvars.set("ui_shadows", if settings.shadows { "1" } else { "0" });
    dvars.set("ui_dof", if settings.depth_of_field { "1" } else { "0" });
    dvars.set("ui_bloom", if settings.bloom { "1" } else { "0" });
    dvars.set("ui_r_mode", settings.resolution.to_string());
    dvars.set(
        "ui_r_displayMode",
        if settings.fullscreen { "1" } else { "0" },
    );
    dvars.set("ui_r_vsync", if settings.vsync { "1" } else { "0" });
    dvars.set("ui_pad_layout", settings.pad_layout.to_string());
    dvars.set("ui_pad_stick_layout", settings.pad_stick_layout.to_string());
    dvars.set("ui_pad_sensitivity", settings.pad_sensitivity.to_string());
    dvars.set(
        "ui_pad_ads_sensitivity",
        settings.pad_ads_sensitivity.to_string(),
    );
    dvars.set("ui_pad_invert", if settings.pad_invert { "1" } else { "0" });
    dvars.set("ui_pad_curve", settings.pad_curve.to_string());
    dvars.set("ui_pad_aim_assist", settings.pad_aim_assist.to_string());
    dvars.set("ui_pad_prompts", settings.pad_prompts.to_string());
    dvars.set(
        "ui_pad_vibration",
        if settings.pad_vibration { "1" } else { "0" },
    );
    dvars.set(
        "ui_pad_deadzone_left",
        settings.pad_deadzone_left.to_string(),
    );
    dvars.set(
        "ui_pad_deadzone_right",
        settings.pad_deadzone_right.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::migrate_binds;

    /// His saved binds (10-07, layout 2): the old E use / F frag keyboard
    /// keys go to Black Ops II's; his pad buttons stay.
    #[test]
    fn old_layout_gets_bo2_defaults() {
        let old = [
            "unbindall",
            "bind BUTTON_A +gostand",
            "bind DPAD_UP +actionslot 1",
            "bind MOUSE2 +speed_throw",
            "bind SHIFT +sprint",
            "bind e +activate",
            "bind f +frag",
            "bind c +movedown",
        ]
        .join("
");
        let new = migrate_binds(&old, 2);
        for want in [
            "bind F +activate",
            "bind G +frag",
            "bind C +stance",
            "bind CTRL toggleprone",
            "bind SHIFT +breath_sprint",
            "bind MOUSE2 +speed_throw",
            "bind BUTTON_A +gostand",
            "bind DPAD_UP +actionslot 1",
        ] {
            assert!(new.lines().any(|l| l == want), "missing {want:?} in {new}");
        }
        for gone in ["bind e +activate", "bind f +frag", "bind c +movedown"] {
            assert!(!new.lines().any(|l| l == gone), "kept {gone:?} in {new}");
        }
    }

    #[test]
    fn current_layout_is_kept() {
        let saved = "unbindall
bind e +activate";
        assert_eq!(migrate_binds(saved, super::BINDS_LAYOUT), saved);
    }
}
