//! bo2mp: an Infinite Warfare (IW7) gun's definition laid out as Advanced
//! Warfare's `WeaponDef`, so AW's weapon capture reads it (Black Ops II's
//! gunplay rules, as AW's and Horizon MW's guns get).
//!
//! IW7 splits what AW keeps in one record: the `WeaponDef`, the
//! `WeaponCompleteDef` (clip, aim-down-sight times and zoom) and the
//! animation package's state timers (u16 milliseconds). Each AW field the
//! capture reads is taken from the IW7 field of the same meaning
//! (`FIELDS`); enums whose numbers differ are turned into AW's (`s1_def`).

use fastfile_iw7::layout::{
    WeaponAnimPackageStateTimers as t, WeaponCompleteDef as c, WeaponDef as d,
};
use fastfile_s1::layout::WeaponDef as s;

/// Where an AW field's value is in IW7's records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Src {
    /// `WeaponDef`, copied as it is.
    Def(usize),
    /// `WeaponCompleteDef`, copied as it is.
    Complete(usize),
    /// A state timer: u16 milliseconds, widened to AW's i32.
    Timer(usize),
}

use Src::{Complete as C, Def as D, Timer as T};

/// (IW7 source, AW offset, bytes).
pub(super) const FIELDS: &[(Src, usize, usize)] = &[
    // Kinds and handling.
    (D(d::weapType), s::weapType, 4),
    (D(d::weapClass), s::weapClass, 4),
    (D(d::inventoryType), s::inventoryType, 4),
    (D(d::offhandClass), s::offhandClass, 4),
    (D(d::shotCount), s::shotCount, 4),
    (D(d::ammoCounterClip), s::ammoCounterClip, 4),
    (D(d::lowAmmoWarningThreshold), s::lowAmmoWarningThreshold, 4),
    (C(c::iClipSize), s::clipSize, 4),
    (D(d::iMaxAmmo), s::maxAmmo, 4),
    (D(d::iStartAmmo), s::startAmmo, 4),
    (D(d::iReloadAmmoAdd), s::reloadAmmoAdd, 4),
    (D(d::iReloadStartAdd), s::reloadStartAdd, 4),
    (D(d::damage), s::damage, 4),
    (D(d::minDamage), s::minDamage, 4),
    (D(d::minPlayerDamage), s::minPlayerDamage, 4),
    (D(d::maxDamageRange), s::maxDamageRange, 4),
    (D(d::meleeDamage), s::meleeDamage, 4),
    (D(d::killIconRatio), s::killIconRatio, 4),
    (D(d::autoAimRange), s::autoAimRange, 4),
    (D(d::aimAssistRange), s::aimAssistRange, 4),
    (D(d::aimAssistRangeAds), s::aimAssistRangeAds, 4),
    (D(d::moveSpeedScale), s::moveSpeedScale, 4),
    (D(d::adsMoveSpeedScale), s::adsMoveSpeedScale, 4),
    (D(d::sprintDurationScale), s::sprintDurationScale, 4),
    (
        D(d::iPositionReloadTransTime),
        s::positionReloadTransTime,
        4,
    ),
    // Aiming down the sight.
    (C(c::fAdsZoomFov), s::adsZoomFov, 4),
    (C(c::iAdsTransInTime), s::adsTransInTime, 4),
    (C(c::iAdsTransOutTime), s::adsTransOutTime, 4),
    (C(c::adsDofStart), s::adsDofStart, 4),
    (C(c::adsDofEnd), s::adsDofEnd, 4),
    (C(c::fAdsViewKickCenterSpeed), s::adsViewKickCenterSpeed, 4),
    (C(c::fHipViewKickCenterSpeed), s::hipViewKickCenterSpeed, 4),
    (C(c::motionTracker), s::motionTracker, 1),
    (D(d::fAdsZoomInFrac), s::adsZoomInFrac, 4),
    (D(d::fAdsZoomOutFrac), s::adsZoomOutFrac, 4),
    (D(d::fAdsAimPitch), s::adsAimPitch, 4),
    (D(d::fAdsCrosshairInFrac), s::adsCrosshairInFrac, 4),
    (D(d::fAdsCrosshairOutFrac), s::adsCrosshairOutFrac, 4),
    (D(d::fAdsSpread), s::adsSpread, 4),
    (D(d::fAdsBobFactor), s::adsBobFactor, 4),
    (D(d::fAdsViewBobMult), s::adsViewBobMult, 4),
    (D(d::fAdsIdleAmount), s::adsIdleAmount, 4),
    (D(d::fHipIdleAmount), s::hipIdleAmount, 4),
    (D(d::adsIdleSpeed), s::adsIdleSpeed, 4),
    (D(d::hipIdleSpeed), s::hipIdleSpeed, 4),
    (D(d::fIdleCrouchFactor), s::idleCrouchFactor, 4),
    (D(d::fIdleProneFactor), s::idleProneFactor, 4),
    (D(d::overlay__reticle), s::overlay__reticle, 4),
    (D(d::overlay__width), s::overlay__width, 4),
    (D(d::overlay__height), s::overlay__height, 4),
    (D(d::overlayInterface), s::overlayInterface, 4),
    // Spread.
    (D(d::fHipSpreadStandMin), s::hipSpreadStandMin, 4),
    (D(d::fHipSpreadDuckedMin), s::hipSpreadDuckedMin, 4),
    (D(d::fHipSpreadProneMin), s::hipSpreadProneMin, 4),
    (D(d::hipSpreadStandMax), s::hipSpreadStandMax, 4),
    (D(d::hipSpreadSprintMax), s::hipSpreadSprintMax, 4),
    (D(d::slideSpreadMax), s::hipSpreadSlideMax, 4),
    (D(d::hipSpreadDuckedMax), s::hipSpreadDuckedMax, 4),
    (D(d::hipSpreadProneMax), s::hipSpreadProneMax, 4),
    (D(d::fHipSpreadDecayRate), s::hipSpreadDecayRate, 4),
    (D(d::fHipSpreadFireAdd), s::hipSpreadFireAdd, 4),
    (D(d::fHipSpreadTurnAdd), s::hipSpreadTurnAdd, 4),
    (D(d::fHipSpreadMoveAdd), s::hipSpreadMoveAdd, 4),
    (D(d::fHipSpreadDuckedDecay), s::hipSpreadDuckedDecay, 4),
    (D(d::fHipSpreadProneDecay), s::hipSpreadProneDecay, 4),
    (D(d::fHipReticleSidePos), s::hipReticleSidePos, 4),
    (D(d::iReticleCenterSize), s::reticleCenterSize, 4),
    (D(d::iReticleSideSize), s::reticleSideSize, 4),
    (D(d::iReticleMinOfs), s::reticleMinOfs, 4),
    // Kick.
    (D(d::fGunMaxPitch), s::gunMaxPitch, 4),
    (D(d::fGunMaxYaw), s::gunMaxYaw, 4),
    (
        D(d::adsGunKickReducedKickBullets),
        s::adsGunKickReducedKickBullets,
        4,
    ),
    (
        D(d::adsGunKickReducedKickPercent),
        s::adsGunKickReducedKickPercent,
        4,
    ),
    (D(d::fAdsGunKickPitchMin), s::adsGunKickPitchMin, 4),
    (D(d::fAdsGunKickPitchMax), s::adsGunKickPitchMax, 4),
    (D(d::fAdsGunKickYawMin), s::adsGunKickYawMin, 4),
    (D(d::fAdsGunKickYawMax), s::adsGunKickYawMax, 4),
    (D(d::fAdsGunKickAccel), s::adsGunKickAccel, 4),
    (D(d::fAdsGunKickSpeedMax), s::adsGunKickSpeedMax, 4),
    (D(d::fAdsGunKickSpeedDecay), s::adsGunKickSpeedDecay, 4),
    (D(d::fAdsGunKickStaticDecay), s::adsGunKickStaticDecay, 4),
    (D(d::fAdsViewKickPitchMin), s::adsViewKickPitchMin, 4),
    (D(d::fAdsViewKickPitchMax), s::adsViewKickPitchMax, 4),
    (D(d::fAdsViewKickYawMin), s::adsViewKickYawMin, 4),
    (D(d::fAdsViewKickYawMax), s::adsViewKickYawMax, 4),
    (
        D(d::hipGunKickReducedKickBullets),
        s::hipGunKickReducedKickBullets,
        4,
    ),
    (
        D(d::hipGunKickReducedKickPercent),
        s::hipGunKickReducedKickPercent,
        4,
    ),
    (D(d::fHipGunKickPitchMin), s::hipGunKickPitchMin, 4),
    (D(d::fHipGunKickPitchMax), s::hipGunKickPitchMax, 4),
    (D(d::fHipGunKickYawMin), s::hipGunKickYawMin, 4),
    (D(d::fHipGunKickYawMax), s::hipGunKickYawMax, 4),
    (D(d::fHipGunKickAccel), s::hipGunKickAccel, 4),
    (D(d::fHipGunKickSpeedMax), s::hipGunKickSpeedMax, 4),
    (D(d::fHipGunKickSpeedDecay), s::hipGunKickSpeedDecay, 4),
    (D(d::fHipGunKickStaticDecay), s::hipGunKickStaticDecay, 4),
    (D(d::fHipViewKickPitchMin), s::hipViewKickPitchMin, 4),
    (D(d::fHipViewKickPitchMax), s::hipViewKickPitchMax, 4),
    (D(d::fHipViewKickYawMin), s::hipViewKickYawMin, 4),
    (D(d::fHipViewKickYawMax), s::hipViewKickYawMax, 4),
    // Sway.
    (D(d::swayMaxAngle), s::swayMaxAngle, 4),
    (D(d::swayLerpSpeed), s::swayLerpSpeed, 4),
    (D(d::swayPitchScale), s::swayPitchScale, 4),
    (D(d::swayYawScale), s::swayYawScale, 4),
    (D(d::swayHorizScale), s::swayHorizScale, 4),
    (D(d::swayVertScale), s::swayVertScale, 4),
    (D(d::swayShellShockScale), s::swayShellShockScale, 4),
    (D(d::adsSwayMaxAngle), s::adsSwayMaxAngle, 4),
    (D(d::adsSwayLerpSpeed), s::adsSwayLerpSpeed, 4),
    (D(d::adsSwayPitchScale), s::adsSwayPitchScale, 4),
    (D(d::adsSwayYawScale), s::adsSwayYawScale, 4),
    (D(d::adsSwayHorizScale), s::adsSwayHorizScale, 4),
    (D(d::adsSwayVertScale), s::adsSwayVertScale, 4),
    // The view model's movement.
    (D(d::vStandMove), s::standMove, 12),
    (D(d::vStandRot), s::standRot, 12),
    (D(d::strafeMove), s::strafeMove, 12),
    (D(d::strafeRot), s::strafeRot, 12),
    (D(d::vDuckedOfs), s::duckedOfs, 12),
    (D(d::vDuckedMove), s::duckedMove, 12),
    (D(d::vDuckedRot), s::duckedRot, 12),
    (D(d::vProneOfs), s::proneOfs, 12),
    (D(d::vProneMove), s::proneMove, 12),
    (D(d::vProneRot), s::proneRot, 12),
    (D(d::fPosMoveRate), s::posMoveRate, 4),
    (D(d::fPosProneMoveRate), s::posProneMoveRate, 4),
    (D(d::fStandMoveMinSpeed), s::standMoveMinSpeed, 4),
    (D(d::fDuckedMoveMinSpeed), s::duckedMoveMinSpeed, 4),
    (D(d::fProneMoveMinSpeed), s::proneMoveMinSpeed, 4),
    (D(d::fPosRotRate), s::posRotRate, 4),
    (D(d::fPosProneRotRate), s::posProneRotRate, 4),
    // Projectiles and explosions.
    (D(d::iExplosionRadius), s::explosionRadius, 4),
    (D(d::iExplosionRadiusMin), s::explosionRadiusMin, 4),
    (D(d::iExplosionInnerDamage), s::explosionInnerDamage, 4),
    (D(d::iExplosionOuterDamage), s::explosionOuterDamage, 4),
    (D(d::damageConeAngle), s::damageConeAngle, 4),
    (D(d::iProjectileSpeed), s::projectileSpeed, 4),
    (D(d::iProjectileSpeedUp), s::projectileSpeedUp, 4),
    (D(d::iProjectileSpeedForward), s::projectileSpeedForward, 4),
    (D(d::iProjectileActivateDist), s::projectileActivateDist, 4),
    (D(d::stickiness), s::stickiness, 4),
    (D(d::fuseTime), s::stateTimers__fuseTime, 4),
    // Flags.
    (D(d::aimDownSight), s::aimDownSight, 1),
    (D(d::canHoldBreath), s::canHoldBreath, 1),
    (D(d::noAdsWhenMagEmpty), s::noAdsWhenMagEmpty, 1),
    (D(d::bRechamberWhileAds), s::rechamberWhileAds, 1),
    (D(d::offhandHoldIsCancelable), s::offhandHoldIsCancelable, 1),
    (
        D(d::disableSwitchToWhenEmpty),
        s::disableSwitchToWhenEmpty,
        1,
    ),
    (D(d::bRifleBullet), s::rifleBullet, 1),
    (D(d::flipKillIcon), s::flipKillIcon, 1),
    (D(d::bNoPartialReload), s::noPartialReload, 1),
    (D(d::bBoltAction), s::boltAction, 1),
    (D(d::bSegmentedReload), s::segmentedReload, 1),
    (D(d::inheritsPerks), s::inheritsPerks, 1),
    (D(d::bCookOffHold), s::cookOffHold, 1),
    (D(d::bClipOnly), s::clipOnly, 1),
    (D(d::rotationParams__rotate), s::rotate, 1),
    (D(d::timedDetonation), s::timedDetonation, 1),
    (D(d::bProjImpactExplode), s::projImpactExplode, 1),
    (D(d::stickToPlayers), s::stickToPlayers, 1),
    (D(d::requireLockonToFire), s::requireLockonToFire, 1),
    // State timers.
    (D(d::iFireTime), s::stateTimers__fireTime, 4),
    (D(d::iFireDelay), s::stateTimers__fireDelay, 4),
    (T(t::iMeleeDelay), s::stateTimers__meleeDelay, 4),
    (T(t::meleeChargeDelay), s::stateTimers__meleeChargeDelay, 4),
    (T(t::iDetonateDelay), s::stateTimers__detonateDelay, 4),
    (T(t::iRechamberTime), s::stateTimers__rechamberTime, 4),
    (
        T(t::iRechamberBoltTime),
        s::stateTimers__rechamberBoltTime,
        4,
    ),
    (T(t::iHoldFireTime), s::stateTimers__holdFireTime, 4),
    (T(t::iDetonateTime), s::stateTimers__detonateTime, 4),
    (T(t::iMeleeTime), s::stateTimers__meleeTime, 4),
    (T(t::meleeChargeTime), s::stateTimers__meleeChargeTime, 4),
    (T(t::iReloadTime), s::stateTimers__reloadTime, 4),
    (
        T(t::reloadShowRocketTime),
        s::stateTimers__reloadShowRocketTime,
        4,
    ),
    (T(t::iReloadEmptyTime), s::stateTimers__reloadEmptyTime, 4),
    (T(t::iReloadAddTime), s::stateTimers__reloadAddTime, 4),
    (
        T(t::iReloadEmptyAddTime),
        s::stateTimers__reloadEmptyAddTime,
        4,
    ),
    (T(t::iReloadStartTime), s::stateTimers__reloadStartTime, 4),
    (
        T(t::iReloadStartAddTime),
        s::stateTimers__reloadStartAddTime,
        4,
    ),
    (T(t::iReloadEndTime), s::stateTimers__reloadEndTime, 4),
    (T(t::iFastReloadTime), s::stateTimers__speedReloadTime, 4),
    (
        T(t::iFastReloadAddTime),
        s::stateTimers__speedReloadAddTime,
        4,
    ),
    (T(t::iDropTime), s::stateTimers__dropTime, 4),
    (T(t::iRaiseTime), s::stateTimers__raiseTime, 4),
    (T(t::iAltDropTime), s::stateTimers__altDropTime, 4),
    (T(t::iAltRaiseTime), s::stateTimers__altRaiseTime, 4),
    (T(t::quickDropTime), s::stateTimers__quickDropTime, 4),
    (T(t::quickRaiseTime), s::stateTimers__quickRaiseTime, 4),
    (T(t::iFirstRaiseTime), s::stateTimers__firstRaiseTime, 4),
    (T(t::iBreachRaiseTime), s::stateTimers__breachRaiseTime, 4),
    (T(t::iEmptyRaiseTime), s::stateTimers__emptyRaiseTime, 4),
    (T(t::iEmptyDropTime), s::stateTimers__emptyDropTime, 4),
    (T(t::sprintInTime), s::stateTimers__sprintInTime, 4),
    (T(t::sprintLoopTime), s::stateTimers__sprintLoopTime, 4),
    (T(t::sprintOutTime), s::stateTimers__sprintOutTime, 4),
    (T(t::stunnedTimeBegin), s::stateTimers__stunnedTimeBegin, 4),
    (T(t::stunnedTimeLoop), s::stateTimers__stunnedTimeLoop, 4),
    (T(t::stunnedTimeEnd), s::stateTimers__stunnedTimeEnd, 4),
    (
        T(t::nightVisionWearTime),
        s::stateTimers__nightVisionWearTime,
        4,
    ),
    (
        T(t::nightVisionRemoveTime),
        s::stateTimers__nightVisionRemoveTime,
        4,
    ),
];

/// IW7's sound package fields as AW's `WeaponDef` sound fields.
pub(super) const SOUNDS: &[(usize, usize)] = {
    use fastfile_iw7::layout::WeaponSFXPackageSounds as p;
    &[
        (p::pickupSound, s::pickupSound),
        (p::pickupSoundPlayer, s::pickupSoundPlayer),
        (p::ammoPickupSound, s::ammoPickupSound),
        (p::ammoPickupSoundPlayer, s::ammoPickupSoundPlayer),
        (p::projectileSound, s::projectileSound),
        (p::pullbackSound, s::pullbackSound),
        (p::pullbackSoundPlayer, s::pullbackSoundPlayer),
        (p::fireSound, s::fireSound),
        (p::fireSoundPlayer, s::fireSoundPlayer),
        (p::fireLoopSound, s::fireLoopSound),
        (p::fireLoopSoundPlayer, s::fireLoopSoundPlayer),
        (p::fireStopSound, s::fireStopSound),
        (p::fireStopSoundPlayer, s::fireStopSoundPlayer),
        (p::fireLastSound, s::fireLastSound),
        (p::fireLastSoundPlayer, s::fireLastSoundPlayer),
        (p::emptyFireSound, s::emptyFireSound),
        (p::emptyFireSoundPlayer, s::emptyFireSoundPlayer),
        (p::rechamberSound, s::rechamberSound),
        (p::rechamberSoundPlayer, s::rechamberSoundPlayer),
        (p::reloadSound, s::reloadSound),
        (p::reloadSoundPlayer, s::reloadSoundPlayer),
        (p::reloadEmptySound, s::reloadEmptySound),
        (p::reloadEmptySoundPlayer, s::reloadEmptySoundPlayer),
        (p::reloadStartSound, s::reloadStartSound),
        (p::reloadStartSoundPlayer, s::reloadStartSoundPlayer),
        (p::reloadEndSound, s::reloadEndSound),
        (p::reloadEndSoundPlayer, s::reloadEndSoundPlayer),
        (p::altSwitchSound, s::altSwitchSound),
        (p::altSwitchSoundPlayer, s::altSwitchSoundPlayer),
        (p::raiseSound, s::raiseSound),
        (p::raiseSoundPlayer, s::raiseSoundPlayer),
        (p::firstRaiseSound, s::firstRaiseSound),
        (p::firstRaiseSoundPlayer, s::firstRaiseSoundPlayer),
        (p::putawaySound, s::putawaySound),
        (p::putawaySoundPlayer, s::putawaySoundPlayer),
        (p::enterAdsSoundPlayer, s::adsUpSound),
        (p::leaveAdsSoundPlayer, s::adsDownSound),
        (p::projExplosionSound, s::projExplosionSound),
        (p::projIgnitionSound, s::projIgnitionSound),
        (p::bounceSound, s::bounceSound),
    ]
};

fn i32_at(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4)
        .map_or(0, |x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4)
        .map_or(0.0, |x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// IW7's fire type (full auto, single shot, burst with `burstCount`
/// rounds) as AW's (bursts of 2, 3 and 4 are their own types).
fn s1_fire_type(fire: i32, burst: i32) -> i32 {
    match fire {
        0 => 0,
        2 => burst.clamp(2, 4),
        _ => 1,
    }
}

/// IW7's impact type as AW's numbering (`asset_game::remap_s1_impact_type`
/// reads it): its ballistic ones are AW's; its energy rounds (small 15,
/// large 16) hit as bullets of their size, its energy and sonic shotguns
/// (17, 18) as shotguns.
pub(super) fn s1_impact_type(iw7: i32) -> i32 {
    match iw7 {
        0..=5 => iw7,
        15 => 1,
        16 => 2,
        17 | 18 => 5,
        _ => 2,
    }
}

/// IW7's penetration (none, small, medium, large, and its energy rounds' 4
/// and 5) as AW's (none to large): energy rounds go through as medium.
fn s1_penetrate_type(iw7: i32) -> i32 {
    match iw7 {
        0..=3 => iw7,
        _ => 2,
    }
}

/// The player's body animation set for a gun of this IW7 class, in the
/// numbering a guest gun's is read in (Modern Warfare 2's: pistol 2, SMG 3,
/// rifle 4, machine gun 5, sniper 6, rocket launcher 7). IW7's own numbers
/// name its per-gun animation sets.
fn s1_player_anim_type(weap_class: i32) -> u8 {
    match weap_class {
        1 => 6,
        2 => 5,
        3 => 3,
        5 => 2,
        7 => 7,
        _ => 4,
    }
}

/// Where Black Ops II's damage falls to the gun's minimum: IW7 steps
/// through up to three middle damages (`mid1Damage` to its range, ...)
/// before its minimum; the last middle range is where the minimum starts,
/// else just past the maximum range (a step).
fn s1_min_damage_range(def: &[u8]) -> f32 {
    let max = f32_at(def, d::maxDamageRange);
    let mids = [
        (d::mid1Damage, d::mid1DamageRange),
        (d::mid2Damage, d::mid2DamageRange),
        (d::mid3Damage, d::mid3DamageRange),
    ];
    let last = mids
        .iter()
        .filter(|(dmg, range)| i32_at(def, *dmg) > 0 && f32_at(def, *range) > max)
        .map(|(_, range)| f32_at(def, *range))
        .fold(0.0f32, f32::max);
    let own = f32_at(def, d::minDamageRange);
    if own > max {
        own
    } else if last > max {
        last
    } else {
        max + 1.0
    }
}

/// An IW7 gun's `WeaponDef`, `WeaponCompleteDef` and animation package
/// timers as Advanced Warfare's `WeaponDef`.
pub(super) fn s1_def(def: &[u8], complete: &[u8], timers: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; s::SIZE];
    for &(from, to, n) in FIELDS {
        let dst = &mut out[to..to + n];
        match from {
            Src::Def(at) => {
                if let Some(src) = def.get(at..at + n) {
                    dst.copy_from_slice(src);
                }
            }
            Src::Complete(at) => {
                if let Some(src) = complete.get(at..at + n) {
                    dst.copy_from_slice(src);
                }
            }
            Src::Timer(at) => {
                let ms = timers
                    .get(at..at + 2)
                    .map_or(0, |x| u16::from_le_bytes([x[0], x[1]]));
                dst.copy_from_slice(&i32::from(ms).to_le_bytes()[..n]);
            }
        }
    }
    let put = |out: &mut [u8], at: usize, v: i32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(
        &mut out,
        s::fireType,
        s1_fire_type(i32_at(def, d::fireType), i32_at(def, d::burstCount)),
    );
    put(
        &mut out,
        s::impactType,
        s1_impact_type(i32_at(complete, c::vfxImpactType)),
    );
    put(
        &mut out,
        s::penetrateType,
        s1_penetrate_type(i32_at(def, d::penetrateType)),
    );
    out[s::playerAnimType] = s1_player_anim_type(i32_at(def, d::weapClass));
    out[s::minDamageRange..s::minDamageRange + 4]
        .copy_from_slice(&s1_min_damage_range(def).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_fit_both_records() {
        for &(from, to, n) in FIELDS {
            assert!(to + n <= s::SIZE, "AW {to}");
            match from {
                Src::Def(at) => assert!(at + n <= d::SIZE, "IW7 def {at}"),
                Src::Complete(at) => assert!(at + n <= c::SIZE, "IW7 complete {at}"),
                Src::Timer(at) => assert!(at + 2 <= t::SIZE && n == 4, "IW7 timer {at}"),
            }
        }
        let mut seen: Vec<usize> = FIELDS.iter().map(|f| f.1).collect();
        seen.sort_unstable();
        let n = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), n, "an AW field filled twice");
    }

    #[test]
    fn an_iw7_gun_reads_as_aw() {
        let mut def = vec![0u8; d::SIZE];
        let mut complete = vec![0u8; c::SIZE];
        let mut timers = vec![0u8; t::SIZE];
        let put = |b: &mut [u8], at: usize, v: &[u8]| b[at..at + v.len()].copy_from_slice(v);
        put(&mut def, d::damage, &30i32.to_le_bytes());
        put(&mut def, d::iFireTime, &98i32.to_le_bytes());
        put(&mut def, d::fireType, &2i32.to_le_bytes());
        put(&mut def, d::burstCount, &3i32.to_le_bytes());
        put(&mut def, d::weapClass, &1i32.to_le_bytes());
        put(&mut def, d::maxDamageRange, &780f32.to_le_bytes());
        put(&mut def, d::mid1Damage, &23i32.to_le_bytes());
        put(&mut def, d::mid1DamageRange, &1300f32.to_le_bytes());
        put(&mut def, d::bBoltAction, &[1]);
        put(&mut complete, c::iClipSize, &36i32.to_le_bytes());
        put(&mut complete, c::vfxImpactType, &16i32.to_le_bytes());
        put(&mut timers, t::iReloadTime, &2150u16.to_le_bytes());
        let out = s1_def(&def, &complete, &timers);
        let i = |at: usize| i32_at(&out, at);
        assert_eq!(
            (i(s::damage), i(s::stateTimers__fireTime), i(s::clipSize)),
            (30, 98, 36)
        );
        assert_eq!(i(s::stateTimers__reloadTime), 2150);
        assert_eq!(i(s::fireType), 3, "a three-round burst");
        assert_eq!(
            i(s::impactType),
            2,
            "a large energy round hits as a large bullet"
        );
        assert_eq!(out[s::playerAnimType], 6);
        assert_eq!(out[s::boltAction], 1);
        assert_eq!(f32_at(&out, s::minDamageRange), 1300.0);
        // (No middle damage: a step just past the maximum range.)
        put(&mut def, d::mid1Damage, &0i32.to_le_bytes());
        assert_eq!(
            f32_at(&s1_def(&def, &complete, &timers), s::minDamageRange),
            781.0
        );
    }
}
