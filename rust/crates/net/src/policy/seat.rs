use std::collections::HashMap;

use bevy::prelude::Resource;
use playerstate_iw4::{PlayerState, SeatFocus, apply_killcam_seat, rebase_archived_timers};
use sim::{ClientId, ClientLifecycle, Snapshot};

use crate::authority::inbox::AUTHORITY_MS;
use crate::player_state_to_entity_state;
use crate::transport::archive::{ARCHIVE_TICK_MS, ArchiveLookup, FrameArchive};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KillcamSession {
    pub archivetime_ms: i32,

    pub focus_client: ClientId,

    pub focus: SeatFocus,

    pub ends_at_ms: i32,

    pub final_kill: bool,

    pub started_at_ms: i32,

    pub killcamoffset_ms: i32,

    pub predelay_ms: i32,

    pub postdelay_ms: i32,

    pub recalc_pending: bool,

    pub entity_focus: Option<killcam_iw4::focus::Entity>,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct ActiveKillcams {
    sessions: HashMap<ClientId, KillcamSession>,
    /// bo2mp: each viewer's final killcam held back by slow motion: how far
    /// behind it has fallen (ms) and when it was last moved on.
    slow: HashMap<ClientId, (f32, i32)>,
}

impl ActiveKillcams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, viewer: ClientId) -> Option<&KillcamSession> {
        self.sessions.get(&viewer)
    }

    pub fn arm(&mut self, viewer: ClientId, session: KillcamSession) {
        self.sessions.insert(viewer, session);
    }

    pub fn clear(&mut self, viewer: ClientId) {
        self.sessions.remove(&viewer);
        self.slow.remove(&viewer);
    }

    /// How far a viewer's replay has fallen behind under slow motion by
    /// `now_ms`, at the game's time scale `scale` (a new replay starts level).
    pub(crate) fn slow_lag_ms(&mut self, viewer: ClientId, fresh: bool, now_ms: i32, scale: f32) -> i32 {
        let entry = self.slow.entry(viewer).or_insert((0.0, now_ms));
        if fresh {
            *entry = (0.0, now_ms);
        }
        let dt = (now_ms - entry.1).max(0) as f32;
        entry.0 += (1.0 - scale.clamp(0.0, 1.0)) * dt;
        entry.1 = now_ms;
        entry.0 as i32
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn expire_clients(&mut self, now_ms: i32) -> Vec<ClientId> {
        let mut expired = Vec::new();
        self.sessions.retain(|id, s| {
            if now_ms < s.ends_at_ms {
                true
            } else {
                expired.push(*id);
                false
            }
        });
        expired
    }

    pub fn viewers(&self) -> Vec<ClientId> {
        self.sessions.keys().copied().collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeatSample {
    pub player_state: PlayerState,
    pub lookup: ArchiveLookup,

    pub rebase_ms: i32,
}

/// bo2mp: a replay between two archived frames (slow motion moves it on by
/// less than a frame a tick): the older frame and how far from it toward the
/// one `lookup` found (0..1); none when it sits on a frame.
fn between_frames<'a>(archive: &'a FrameArchive, lookup: &ArchiveLookup, requested_ms: i32) -> Option<(&'a sim::Snapshot, f32)> {
    let tick = lookup.tick?;
    let behind = requested_ms - lookup.attained_ms;
    if behind <= 0 || behind >= ARCHIVE_TICK_MS || tick.0 == 0 {
        return None;
    }
    let older = archive.frame(sim::Tick(tick.0 - 1))?;
    Some((&older.snapshot, 1.0 - behind as f32 / ARCHIVE_TICK_MS as f32))
}

/// A player's state blended from the older frame toward `ps` by `t`: where he
/// stands and looks (the rest is the newer frame's).
fn blend_player(older: &sim::Snapshot, id: ClientId, ps: &mut PlayerState, t: f32) {
    let Some((_, old)) = older.players.iter().find(|(o, _)| *o == id) else { return };
    for i in 0..3 {
        ps.origin[i] = old.origin[i] + (ps.origin[i] - old.origin[i]) * t;
        let d = (ps.viewangles[i] - old.viewangles[i] + 540.0).rem_euclid(360.0) - 180.0;
        ps.viewangles[i] = old.viewangles[i] + d * t;
        ps.velocity[i] = old.velocity[i] + (ps.velocity[i] - old.velocity[i]) * t;
    }
    // bo2mp: what the first-person gun reads (its bob, lean, aim and height)
    // moves with the camera: left on the newer frame's value, a quarter-speed
    // replay held each of them for four ticks and then jumped, which showed
    // as a stepped gun while the camera was smooth.
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let mut bob_new = ps.bob_cycle;
    if bob_new < old.bob_cycle {
        bob_new += 256;
    }
    ps.bob_cycle = old.bob_cycle + ((bob_new - old.bob_cycle) as f32 * t) as i32;
    ps.leanf = lerp(old.leanf, ps.leanf);
    ps.view_height_current = lerp(old.view_height_current, ps.view_height_current);
    ps.f_weapon_pos_frac = lerp(old.f_weapon_pos_frac, ps.f_weapon_pos_frac);
    ps.aim_spread_scale = lerp(old.aim_spread_scale, ps.aim_spread_scale);
}

pub fn sample_killcam_seat(
    archive: &FrameArchive,
    snapshot_now: &Snapshot,
    viewer: ClientId,
    session: &KillcamSession,
    now_ms: i32,
) -> Option<SeatSample> {
    let lookup = archive.lookup(session.archivetime_ms);
    if lookup.nothing_to_show() {
        return None;
    }
    let tick = lookup.tick?;
    let frame = archive.frame(tick)?;
    if !frame.player_state_exists(session.focus_client) {
        return None;
    }
    let archived = frame
        .snapshot
        .players
        .iter()
        .find(|(id, _)| *id == session.focus_client)
        .map(|(_, ps)| *ps)?;

    let viewer_ps = snapshot_now
        .players
        .iter()
        .find(|(id, _)| *id == viewer)
        .map(|(_, ps)| *ps)
        .unwrap_or(PlayerState::ZERO);

    let frame_base_ms = i32::try_from(tick.0).ok()?.saturating_mul(ARCHIVE_TICK_MS);
    let rebase_ms = now_ms.saturating_sub(frame_base_ms);

    let mut remapped = archived;
    if let Some((older, t)) = between_frames(archive, &lookup, session.archivetime_ms) {
        blend_player(older, session.focus_client, &mut remapped, t);
    }
    rebase_archived_timers(&mut remapped, rebase_ms);
    let player_state = apply_killcam_seat(&viewer_ps, &remapped, session.focus);

    Some(SeatSample {
        player_state,
        lookup,
        rebase_ms,
    })
}

pub fn apply_seat_to_snapshot(
    archive: &FrameArchive,
    snapshot: &mut Snapshot,
    viewer: ClientId,
    session: &KillcamSession,
    now_ms: i32,
) -> Option<SeatSample> {
    let sample = sample_killcam_seat(archive, snapshot, viewer, session, now_ms)?;
    if let Some((_, ps)) = snapshot.players.iter_mut().find(|(id, _)| *id == viewer) {
        *ps = sample.player_state;
    } else {
        snapshot.players.push((viewer, sample.player_state));
    }
    Some(sample)
}

pub fn snapshot_for_viewer(
    archive: &FrameArchive,
    seats: &ActiveKillcams,
    live: &Snapshot,
    viewer: ClientId,
    now_ms: i32,
) -> Snapshot {
    snapshot_and_sample_for_viewer(archive, seats, live, viewer, now_ms).0
}

pub fn snapshot_and_sample_for_viewer(
    archive: &FrameArchive,
    seats: &ActiveKillcams,
    live: &Snapshot,
    viewer: ClientId,
    now_ms: i32,
) -> (Snapshot, Option<SeatSample>) {
    let (mut out, sample) = seat_snapshot(archive, seats, live, viewer, now_ms);
    follow_teammate(&mut out, viewer);
    reveal_shown_movers(&mut out, viewer);
    (out, sample)
}

/// bo2zm: a player who bled out in co-op sees through the teammate he
/// follows: BO2 copies that player's state into his (SpectatorClientEndFrame).
/// His client draws it as a seat (no prediction), with no killcam.
fn follow_teammate(out: &mut Snapshot, viewer: ClientId) {
    let Some(&(_, followed)) = out.meta.follows.iter().find(|(v, _)| *v == viewer) else {
        return;
    };
    // Not a dead or third-person teammate: a killcam seat's third-person
    // camera has no archive clock (offset_third_person_view).
    let Some(seen) = out
        .players
        .iter()
        .find(|(id, _)| *id == followed)
        .map(|(_, ps)| *ps)
        .filter(|ps| ps.pm_type < playerstate_iw4::PM_TYPE_DEAD && ps.other_flags & 2 == 0)
    else {
        return;
    };
    let focus = SeatFocus {
        kill_cam_entity: playerstate_iw4::ENTITYNUM_NONE,
        kill_cam_look_at_entity: playerstate_iw4::ENTITYNUM_NONE,
        kill_cam_client_num: followed.0 as i32,
    };
    if let Some((_, ps)) = out.players.iter_mut().find(|(id, _)| *id == viewer) {
        *ps = apply_killcam_seat(ps, &seen, focus);
    }
    // The copy carries his ammo and grenades too, so the watcher's HUD counts
    // the teammate's clip, stock and grenades. His points and the hint stay
    // the watcher's own (BO2 rebuilds the current hud elems for him).
    let Some((clip, stock, by_weapon, dvars)) = out.meta.for_client(followed).map(|m| {
        let dvars: Vec<(String, String)> = m
            .client_dvars
            .iter()
            .filter(|(k, _)| k == "bo2zm_offhand" || k == "bo2zm_actionslots")
            .cloned()
            .collect();
        (m.ammo_clip, m.ammo_stock, m.ammo_by_weapon.clone(), dvars)
    }) else {
        return;
    };
    let Some((_, mine)) = out.meta.clients.iter_mut().find(|(id, _)| *id == viewer) else {
        return;
    };
    mine.ammo_clip = clip;
    mine.ammo_stock = stock;
    mine.ammo_by_weapon = by_weapon;
    for (name, value) in dvars {
        match mine.client_dvars.iter_mut().find(|(k, _)| *k == name) {
            Some(row) => row.1 = value,
            None => mine.client_dvars.push((name, value)),
        }
    }
}

fn reveal_shown_movers(out: &mut Snapshot, viewer: ClientId) {
    let Some(bit) = 1u64.checked_shl(viewer.0) else {
        return;
    };
    let Snapshot { meta, .. } = out;
    for mover in &mut meta.script_movers {
        // A brush that does not stop this player goes to his prediction
        // nonsolid (the others' still clip against it).
        if mover.passes_for & bit != 0 {
            mover.nonsolid = true;
        }
        if mover.shown_to == 0 || mover.state.e_flags & entity_iw4::CG_SCRIPT_MOVER_NODRAW == 0 {
            continue;
        }
        if mover.shown_to & bit == 0 {
            meta.entity_dobjs
                .retain(|(owner, _)| owner.script_model() != Some(mover.id));
            continue;
        }
        let hidden = mover.state.clone();
        mover.state.e_flags &= !entity_iw4::CG_SCRIPT_MOVER_NODRAW;
        for es in meta.entities.iter_mut().filter(|es| **es == hidden) {
            *es = mover.state.clone();
        }
    }
}

fn seat_snapshot(
    archive: &FrameArchive,
    seats: &ActiveKillcams,
    live: &Snapshot,
    viewer: ClientId,
    now_ms: i32,
) -> (Snapshot, Option<SeatSample>) {
    let Some(session) = seats.get(viewer) else {
        return (live.clone(), None);
    };

    if !session.final_kill
        && live
            .meta
            .for_client(viewer)
            .is_some_and(|m| m.lifecycle == ClientLifecycle::Alive)
    {
        return (live.clone(), None);
    }
    let lookup = archive.lookup(session.archivetime_ms);
    let mut out = match lookup.tick.and_then(|tick| archive.frame(tick)) {
        Some(frame) => overlay_archived_world(live, &frame.snapshot, viewer, session.focus_client),
        None => live.clone(),
    };
    if let Some((older, t)) = between_frames(archive, &lookup, session.archivetime_ms) {
        for (id, ps) in out.players.iter_mut().filter(|(id, _)| *id != viewer) {
            blend_player(older, *id, ps, t);
        }
    }
    let sample = apply_seat_to_snapshot(archive, &mut out, viewer, session, now_ms);
    let archived = lookup.tick.and_then(|tick| archive.frame(tick));
    overlay_killcam_hud(
        &mut out,
        live,
        archived.map(|f| &f.snapshot),
        viewer,
        session,
        sample.as_ref().map(|s| s.rebase_ms).unwrap_or(0),
    );
    (out, sample)
}

fn overlay_archived_world(
    live: &Snapshot,
    archived: &Snapshot,
    viewer: ClientId,
    focus: ClientId,
) -> Snapshot {
    let mut out = archived.clone();
    out.meta
        .entity_events
        .retain(|record| record.audience.projects_to(focus));
    for record in &mut out.meta.entity_events {
        record.audience = sim::EventAudience::Client(viewer);
    }
    out.tick = live.tick;
    out.meta.phase = live.meta.phase;
    out.meta.match_elapsed_ms = live.meta.match_elapsed_ms;
    out.meta.prematch = live.meta.prematch;
    out.meta.score_limit = live.meta.score_limit;
    out.meta.time_limit_ms = live.meta.time_limit_ms;
    out.meta.kind = live.meta.kind;
    out.meta.hud_strings = live.meta.hud_strings.clone();
    out.meta.hud_materials = live.meta.hud_materials.clone();

    let archived_viewer = archived
        .players
        .iter()
        .find(|(id, _)| *id == viewer)
        .map(|(_, ps)| *ps);
    if let Some(ps) = archived_viewer {
        let es = player_state_to_entity_state(viewer, &ps);
        if !out.meta.entities.iter().any(|e| e.number == es.number) {
            out.meta.entities.push(es);
        }
    }

    let viewer_row = live.players.iter().find(|(id, _)| *id == viewer).cloned();
    out.players.retain(|(id, _)| *id != viewer);
    if let Some(row) = viewer_row {
        out.players.push(row);
    }

    let viewer_meta = live
        .meta
        .clients
        .iter()
        .find(|(id, _)| *id == viewer)
        .cloned();
    out.meta.clients.retain(|(id, _)| *id != viewer);
    if let Some(row) = viewer_meta {
        out.meta.clients.push(row);
    }
    let delta_ms = sim::level_time_ms(live.tick).wrapping_sub(sim::level_time_ms(archived.tick));
    rebase_archived_world(&mut out, viewer, delta_ms);
    out
}

fn rebase_entity_state(es: &mut entity_iw4::EntityState, delta_ms: i32) {
    if es.tr_time != 0 {
        es.tr_time = es.tr_time.wrapping_add(delta_ms);
    }
    if es.apos_tr_time != 0 {
        es.apos_tr_time = es.apos_tr_time.wrapping_add(delta_ms);
    }
    if es.time2 != 0 {
        es.time2 = es.time2.wrapping_add(delta_ms);
    }
    // General's data[0] and missile launchTime share one union slot.
    if es.e_type == entity_iw4::ET_GENERAL || es.e_type == entity_iw4::ET_MISSILE {
        es.set_launch_time(es.launch_time().wrapping_add(delta_ms));
    }
}

fn rebase_archived_world(out: &mut Snapshot, viewer: ClientId, delta_ms: i32) {
    for es in &mut out.meta.entities {
        rebase_entity_state(es, delta_ms);
    }
    // Adopt requires each typed mover to equal its entityState row.
    for mover in &mut out.meta.script_movers {
        rebase_entity_state(&mut mover.state, delta_ms);
    }
    for p in &mut out.projectiles {
        if p.pos.tr_time != 0 {
            p.pos.tr_time = p.pos.tr_time.wrapping_add(delta_ms);
        }
        if p.apos.tr_time != 0 {
            p.apos.tr_time = p.apos.tr_time.wrapping_add(delta_ms);
        }
        p.launch_time = p.launch_time.wrapping_add(delta_ms);
        p.spawn_time_ms = p.spawn_time_ms.wrapping_add(delta_ms);
        p.detonate_at_ms = p.detonate_at_ms.map(|t| t.wrapping_add(delta_ms));
        p.cleanup_at_ms = p.cleanup_at_ms.wrapping_add(delta_ms);
    }
    // A body that went limp inside the replay falls on the replay's clock
    // (on the live one it is seconds into its fall, far under the map).
    for slot in &mut out.meta.corpses.slots {
        if slot.tr_time != 0 {
            slot.tr_time = slot.tr_time.wrapping_add(delta_ms);
        }
    }
    for (id, ps) in &mut out.players {
        if *id != viewer {
            rebase_archived_timers(ps, delta_ms);
        }
    }
}

fn overlay_killcam_hud(
    out: &mut Snapshot,
    live: &Snapshot,
    archived: Option<&Snapshot>,
    viewer: ClientId,
    session: &KillcamSession,
    rebase_ms: i32,
) {
    let current = live
        .meta
        .for_client(viewer)
        .map(|m| m.hud_current.clone())
        .unwrap_or_default();
    let wipe = session.focus.kill_cam_entity != playerstate_iw4::HITSCAN_KILL_CAM_ENTITY;
    let mut archival = if wipe {
        Vec::new()
    } else {
        archived
            .and_then(|snap| snap.meta.for_client(session.focus_client))
            .map(|m| m.hud_archival.clone())
            .unwrap_or_default()
    };
    sim::rebase_hud_archival(&mut archival, rebase_ms);
    // bo2mp: Black Ops II's script HUD: the killer's hit marker as it stood
    // at the replayed moment (his archived `bo2zm_hud` rows), in place of the
    // viewer's own.
    let killer_marker: Vec<String> = archived
        .map(|snap| snap.meta.script_dvars(session.focus_client).string("bo2zm_hud").unwrap_or_default().to_owned())
        .unwrap_or_default()
        .split(SCRIPT_HUD_ROW)
        .filter(|row| row.contains(SCRIPT_HUD_HIT_MARKER))
        .map(str::to_owned)
        .collect();
    if let Some((_, meta)) = out.meta.clients.iter_mut().find(|(id, _)| *id == viewer) {
        meta.killcam_hud = Some(sim::KillcamHud {
            final_kill: session.final_kill,
        });
        meta.hud_current = current;
        meta.hud_archival = archival;
        if session.focus_client != viewer {
            with_killer_marker(&mut meta.client_dvars, &killer_marker);
        }
    }
}

/// bo2mp: a script HUD value's row separator and the hit marker's picture
/// as a row names it (sim's t6 HUD publishing).
const SCRIPT_HUD_ROW: char = '\u{1e}';
const SCRIPT_HUD_HIT_MARKER: &str = "#shader:damage_feedback";

/// The viewer's script HUD rows with his own hit marker swapped for the
/// killer's (`marker`, none when the killer has none up).
fn with_killer_marker(dvars: &mut Vec<(String, String)>, marker: &[String]) {
    let own = dvars.iter().find(|(k, _)| k == "bo2zm_hud").map(|(_, v)| v.as_str()).unwrap_or("");
    let mut rows: Vec<&str> = own.split(SCRIPT_HUD_ROW).filter(|r| !r.is_empty() && !r.contains(SCRIPT_HUD_HIT_MARKER)).collect();
    rows.extend(marker.iter().map(String::as_str));
    let value = rows.join(&SCRIPT_HUD_ROW.to_string());
    match dvars.iter_mut().find(|(k, _)| k == "bo2zm_hud") {
        Some(row) => row.1 = value,
        None if !value.is_empty() => dvars.push(("bo2zm_hud".to_owned(), value)),
        None => {}
    }
}

pub const fn killcam_seconds_to_ms(seconds: f32) -> i32 {
    let ms = seconds * 1000.0;
    let ticks = (ms / (AUTHORITY_MS as f32) + 0.5) as i32;
    ticks.saturating_mul(AUTHORITY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::{ClientSnapshotMeta, SnapshotMeta, Tick};

    fn meta(clip: i32, offhand: &str) -> ClientSnapshotMeta {
        ClientSnapshotMeta {
            ammo_clip: clip,
            ammo_stock: clip * 10,
            score: clip * 100,
            client_dvars: vec![("bo2zm_offhand".to_owned(), offhand.to_owned())],
            ..Default::default()
        }
    }

    /// A replay between two frames blends the fields the first-person gun
    /// reads, the bob cycle the short way round its wrap.
    #[test]
    fn a_blend_between_frames_moves_the_gun_inputs() {
        let id = ClientId(1);
        let mut old = PlayerState::ZERO;
        old.bob_cycle = 250;
        old.velocity = [100.0, 0.0, 0.0];
        old.f_weapon_pos_frac = 0.0;
        let mut newer = PlayerState::ZERO;
        newer.bob_cycle = 10;
        newer.velocity = [200.0, 0.0, 0.0];
        newer.f_weapon_pos_frac = 1.0;
        let snap = Snapshot { tick: Tick(1), players: vec![(id, old)], projectiles: Vec::new(), meta: Default::default() };
        let mut ps = newer;
        blend_player(&snap, id, &mut ps, 0.5);
        assert_eq!(ps.bob_cycle, 250 + 8);
        assert_eq!(ps.velocity[0], 150.0);
        assert_eq!(ps.f_weapon_pos_frac, 0.5);
    }

    #[test]
    fn the_killcam_shows_the_killers_hit_marker() {
        let f = |v: &[&str]| v.join("\u{1f}");
        let mine = f(&["0", "0", "center", "middle", "center", "middle", "1", "1", "1", "1", "0.80", "2", "#shader:damage_feedback:24:48"]);
        let text = f(&["0", "0", "left", "top", "left", "top", "1", "1", "1", "1", "1.00", "1", "PRESS F"]);
        let his = f(&["0", "0", "center", "middle", "center", "middle", "1", "1", "1", "1", "1.00", "2", "#shader:damage_feedback:24:48"]);
        let mut dvars = vec![("bo2zm_hud".to_owned(), format!("{mine}\u{1e}{text}"))];
        with_killer_marker(&mut dvars, std::slice::from_ref(&his));
        assert_eq!(dvars[0].1, format!("{text}\u{1e}{his}"));
        // (No marker of his up: his own goes too.)
        with_killer_marker(&mut dvars, &[]);
        assert_eq!(dvars[0].1, text);
    }

    #[test]
    fn a_watcher_counts_the_teammates_ammo_but_keeps_his_points() {
        let (host, watcher) = (ClientId(0), ClientId(1));
        let mut out = Snapshot {
            tick: Tick(1),
            players: vec![(host, PlayerState::ZERO), (watcher, PlayerState::ZERO)],
            projectiles: Vec::new(),
            meta: SnapshotMeta {
                clients: vec![
                    (host, meta(30, "frag_grenade_zm:2")),
                    (watcher, meta(8, "")),
                ],
                follows: vec![(watcher, host)],
                ..Default::default()
            },
        };
        follow_teammate(&mut out, watcher);
        let mine = out.meta.for_client(watcher).unwrap();
        assert_eq!(
            (mine.ammo_clip, mine.ammo_stock, mine.score),
            (30, 300, 800)
        );
        assert_eq!(
            out.meta.script_dvars(watcher).string("bo2zm_offhand"),
            Some("frag_grenade_zm:2")
        );
    }
}
