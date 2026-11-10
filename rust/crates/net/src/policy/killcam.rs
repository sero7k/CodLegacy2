use bevy::prelude::*;
use playerstate_iw4::SeatFocus;
use sim::ClientId;

use crate::policy::seat::{ActiveKillcams, KillcamSession};
use crate::transport::archive::{ARCHIVE_TICK_MS, FrameArchive};

/// Arms each script seat for this tick. Returns the seats whose
/// `archivetime` the engine moved, for the scripts to read back: real
/// SpectatorClientEndFrame keeps the stepped `sess.archiveTime`, so the
/// replay plays on from where the watched player has a saved state (and
/// `_killcam.gsc` drops a killcam whose time fell to the death or to 0).
pub(crate) fn play_script_seats(
    seats: &mut ActiveKillcams,
    archive: &FrameArchive,
    now_ms: i32,
    scripted: &[(ClientId, sim::ScriptSeat)],
) -> Vec<(ClientId, i32)> {
    let mut stepped = Vec::new();
    for viewer in seats.viewers() {
        if !scripted.iter().any(|(id, _)| *id == viewer) {
            seats.clear(viewer);
        }
    }
    // bo2mp: Black Ops II's final killcam slows on the killing shot
    // (setslowmotion): the replay falls behind live time by what it takes.
    let scale = sim::slow_motion_scale();
    for (viewer, seat) in scripted {
        let script_focus = ClientId(seat.spectator_client as u32);
        let mut focus = SeatFocus::hitscan(seat.spectator_client);
        if seat.kill_cam_entity >= 0 {
            focus.kill_cam_entity = seat.kill_cam_entity;
        }
        if seat.look_at_entity >= 0 {
            focus.kill_cam_look_at_entity = seat.look_at_entity;
        }
        let started_at_ms = seats
            .get(*viewer)
            .filter(|s| {
                s.killcamoffset_ms == seat.archive_ms
                    && (s.focus_client == script_focus || seat.kill_cam_entity >= 0)
            })
            .map_or(now_ms, |s| s.started_at_ms);
        if started_at_ms == now_ms {
            diag::info!(
                Net,
                "killcam: script seat viewer={} spectating={} entity={} archive_ms={} length_ms={}",
                viewer.0,
                seat.spectator_client,
                seat.kill_cam_entity,
                seat.archive_ms,
                seat.length_ms
            );
        }
        let lag = if seat.final_kill { seats.slow_lag_ms(*viewer, started_at_ms == now_ms, now_ms, scale) } else { 0 };
        let mut session = KillcamSession {
            archivetime_ms: seat.archive_ms + lag,
            focus_client: script_focus,
            focus,
            ends_at_ms: i32::MAX,
            final_kill: seat.final_kill,
            started_at_ms,
            killcamoffset_ms: seat.archive_ms,
            predelay_ms: 0,
            postdelay_ms: 0,
            recalc_pending: false,
            entity_focus: None,
        };
        let found = follow_archived_focus(archive, &mut session, *viewer, seat);
        // The lookup lands on 50 ms ticks; only a real step is kept. The
        // scripts get it back without the slow-motion lag (that is added
        // here every tick).
        let asked = seat.archive_ms + lag;
        if session.archivetime_ms < asked / ARCHIVE_TICK_MS * ARCHIVE_TICK_MS {
            diag::info!(
                Net,
                "killcam: viewer={} archive_ms {} -> {} (spectating {} has no saved state){}",
                viewer.0,
                asked,
                session.archivetime_ms,
                seat.spectator_client,
                if found { "" } else { ", none left: the killcam ends" }
            );
            session.killcamoffset_ms = (session.archivetime_ms - lag).max(0);
            stepped.push((*viewer, session.killcamoffset_ms));
        }
        if found {
            seats.arm(*viewer, session);
        } else {
            seats.clear(*viewer);
        }
    }
    stepped
}

/// Real SpectatorClientEndFrame: while the watched client has no saved
/// state at `archiveTime`, an entity killcam first turns to
/// `killCamTargetEntity` (or the viewer), otherwise `archiveTime` moves
/// 50 ms nearer the present; below 1 ms the seat lets go. Returns whether a
/// saved state was found.
pub(crate) fn follow_archived_focus(
    archive: &FrameArchive,
    session: &mut KillcamSession,
    viewer: ClientId,
    seat: &sim::ScriptSeat,
) -> bool {
    // bo2mp: the frame the replay time falls in has the killer: the time is
    // kept as it is, between two frames. (Snapped to the frame, the slowed
    // final killcam held each frame for several ticks and then jumped: the
    // blend between frames, `seat::between_frames`, never saw a time between
    // two.)
    let first = archive.lookup(session.archivetime_ms.max(0));
    if first
        .tick
        .and_then(|tick| archive.frame(tick))
        .is_some_and(|f| f.player_state_exists(session.focus_client))
    {
        return true;
    }
    loop {
        let lookup = archive.lookup(session.archivetime_ms.max(0));
        let found = lookup.tick.and_then(|tick| archive.frame(tick));
        session.archivetime_ms = if found.is_some() {
            lookup.attained_ms
        } else {
            0
        };
        if found.is_some_and(|f| f.player_state_exists(session.focus_client)) {
            return true;
        }
        if session.archivetime_ms < 1 {
            session.archivetime_ms = 0;
            return false;
        }
        if seat.kill_cam_entity >= 0 {
            let target = if (0..18).contains(&seat.target_client) {
                ClientId(seat.target_client as u32)
            } else {
                viewer
            };
            if session.focus_client != target {
                session.focus_client = target;
                session.focus.kill_cam_client_num = target.0 as i32;
                continue;
            }
        }
        session.archivetime_ms = (session.archivetime_ms - ARCHIVE_TICK_MS).max(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::archive::ArchivedFrame;
    use playerstate_iw4::PlayerState;
    use sim::{Snapshot, Tick};

    fn archive_with(killer: ClientId, frames: u32) -> FrameArchive {
        let mut archive = FrameArchive::new();
        for t in 1..=frames {
            let mut ps = PlayerState::ZERO;
            ps.origin = [t as f32 * 10.0, 0.0, 0.0];
            let snapshot = Snapshot { tick: Tick(t), players: vec![(killer, ps)], projectiles: Vec::new(), meta: Default::default() };
            archive.push(ArchivedFrame { tick: Tick(t), snapshot, no_player_state: Vec::new() });
        }
        archive
    }

    fn session(killer: ClientId, archivetime_ms: i32) -> KillcamSession {
        KillcamSession {
            archivetime_ms,
            focus_client: killer,
            focus: SeatFocus::hitscan(killer.0 as i32),
            ends_at_ms: i32::MAX,
            final_kill: true,
            started_at_ms: 0,
            killcamoffset_ms: archivetime_ms,
            predelay_ms: 0,
            postdelay_ms: 0,
            recalc_pending: false,
            entity_focus: None,
        }
    }

    /// A slowed replay's time between two frames stays there, so the camera
    /// is blended between them (it was snapped to a frame: the final
    /// killcam held each frame for several ticks, then jumped).
    #[test]
    fn a_replay_time_between_frames_is_kept() {
        let killer = ClientId(3);
        let archive = archive_with(killer, 40);
        let mut s = session(killer, 5 * ARCHIVE_TICK_MS + 17);
        follow_archived_focus(&archive, &mut s, killer, &sim::ScriptSeat::default());
        assert_eq!(s.archivetime_ms, 5 * ARCHIVE_TICK_MS + 17);
    }

    /// A frame without the killer still moves on to the next one with him
    /// (toward the present, as the engine does).
    #[test]
    fn a_frame_without_the_killer_moves_on() {
        let killer = ClientId(3);
        let mut archive = FrameArchive::new();
        for t in 1..=40u32 {
            let players = if t == 30 { Vec::new() } else { vec![(killer, PlayerState::ZERO)] };
            let snapshot = Snapshot { tick: Tick(t), players, projectiles: Vec::new(), meta: Default::default() };
            archive.push(ArchivedFrame { tick: Tick(t), snapshot, no_player_state: Vec::new() });
        }
        // (41 frames counted: tick 30 is 11 frames back.)
        let mut s = session(killer, 11 * ARCHIVE_TICK_MS + 17);
        follow_archived_focus(&archive, &mut s, killer, &sim::ScriptSeat::default());
        assert_eq!(s.archivetime_ms, 10 * ARCHIVE_TICK_MS);
    }
}
