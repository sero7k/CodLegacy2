#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponDecodeError {
    UnknownWeaponState(i32),
    UnknownFireType(i32),
}

/// Black Ops II's weapon states, by their own numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum WeaponState {
    Ready = 0x00,
    Raising = 0x01,
    RaisingAltswitch = 0x02,
    Dropping = 0x03,
    DroppingQuick = 0x04,
    DroppingAltswitch = 0x05,
    Firing = 0x06,
    Rechambering = 0x07,
    RechamberingBolted = 0x08,
    State09 = 0x09,
    State0A = 0x0A,
    Reloading = 0x0B,
    ReloadingInterrupt = 0x0C,
    ReloadStart = 0x0D,
    ReloadStartBolt = 0x0E,
    ReloadStartInterrupt = 0x0F,
    ReloadEnd = 0x10,
    ReloadQuick = 0x11,
    ReloadQuickEmpty = 0x12,
    MeleeInit = 0x13,
    MeleeFire = 0x14,
    MeleeEnd = 0x15,
    OffhandInit = 0x16,
    OffhandPrepare = 0x17,
    OffhandHold = 0x18,
    OffhandStart = 0x19,
    Offhand = 0x1A,
    OffhandEnd = 0x1B,
    OffhandFinish = 0x1C,
    OffhandDone = 0x1D,
    DeployInit = 0x1E,
    DeployPrepare = 0x1F,
    Deploying = 0x20,
    Deploy21 = 0x21,
    Deploy22 = 0x22,
    Detonating = 0x23,
    SprintIn = 0x24,
    SprintLoop = 0x25,
    SprintOut = 0x26,
    ContFireIn = 0x27,
    ContFireLoop = 0x28,
    ContFireOut = 0x29,
    NightVisionWear = 0x2A,
    NightVisionRemove = 0x2B,
    Mounted = 0x2C,
    Mounting = 0x2D,
    Unmounting = 0x2E,
    UnderwaterDrop = 0x2F,
    UnderwaterRaise = 0x30,
    DtpIn = 0x31,
    DtpLoop = 0x32,
    DtpOut = 0x33,
    Special34 = 0x34,
    State35 = 0x35,
    State36 = 0x36,
    Lockout = 0x37,
    LowReadyIn = 0x38,
    LowReadyLoop = 0x39,
    LowReadyOut = 0x3A,
    CrawlIn = 0x3B,
    CrawlForward = 0x3C,
    CrawlBack = 0x3D,
    CrawlRight = 0x3E,
    CrawlLeft = 0x3F,
    CrawlOut = 0x40,
}

impl WeaponState {
    pub fn from_i32(v: i32) -> Result<Self, WeaponDecodeError> {
        Ok(match v {
            0x00 => Self::Ready,
            0x01 => Self::Raising,
            0x02 => Self::RaisingAltswitch,
            0x03 => Self::Dropping,
            0x04 => Self::DroppingQuick,
            0x05 => Self::DroppingAltswitch,
            0x06 => Self::Firing,
            0x07 => Self::Rechambering,
            0x08 => Self::RechamberingBolted,
            0x09 => Self::State09,
            0x0A => Self::State0A,
            0x0B => Self::Reloading,
            0x0C => Self::ReloadingInterrupt,
            0x0D => Self::ReloadStart,
            0x0E => Self::ReloadStartBolt,
            0x0F => Self::ReloadStartInterrupt,
            0x10 => Self::ReloadEnd,
            0x11 => Self::ReloadQuick,
            0x12 => Self::ReloadQuickEmpty,
            0x13 => Self::MeleeInit,
            0x14 => Self::MeleeFire,
            0x15 => Self::MeleeEnd,
            0x16 => Self::OffhandInit,
            0x17 => Self::OffhandPrepare,
            0x18 => Self::OffhandHold,
            0x19 => Self::OffhandStart,
            0x1A => Self::Offhand,
            0x1B => Self::OffhandEnd,
            0x1C => Self::OffhandFinish,
            0x1D => Self::OffhandDone,
            0x1E => Self::DeployInit,
            0x1F => Self::DeployPrepare,
            0x20 => Self::Deploying,
            0x21 => Self::Deploy21,
            0x22 => Self::Deploy22,
            0x23 => Self::Detonating,
            0x24 => Self::SprintIn,
            0x25 => Self::SprintLoop,
            0x26 => Self::SprintOut,
            0x27 => Self::ContFireIn,
            0x28 => Self::ContFireLoop,
            0x29 => Self::ContFireOut,
            0x2A => Self::NightVisionWear,
            0x2B => Self::NightVisionRemove,
            0x2C => Self::Mounted,
            0x2D => Self::Mounting,
            0x2E => Self::Unmounting,
            0x2F => Self::UnderwaterDrop,
            0x30 => Self::UnderwaterRaise,
            0x31 => Self::DtpIn,
            0x32 => Self::DtpLoop,
            0x33 => Self::DtpOut,
            0x34 => Self::Special34,
            0x35 => Self::State35,
            0x36 => Self::State36,
            0x37 => Self::Lockout,
            0x38 => Self::LowReadyIn,
            0x39 => Self::LowReadyLoop,
            0x3A => Self::LowReadyOut,
            0x3B => Self::CrawlIn,
            0x3C => Self::CrawlForward,
            0x3D => Self::CrawlBack,
            0x3E => Self::CrawlRight,
            0x3F => Self::CrawlLeft,
            0x40 => Self::CrawlOut,
            other => return Err(WeaponDecodeError::UnknownWeaponState(other)),
        })
    }

    pub fn is_raise_or_drop(self) -> bool {
        matches!(
            self,
            Self::Raising
                | Self::RaisingAltswitch
                | Self::Dropping
                | Self::DroppingQuick
                | Self::DroppingAltswitch
        )
    }

    pub fn is_sprint(self) -> bool {
        matches!(self, Self::SprintIn | Self::SprintLoop | Self::SprintOut)
    }

    /// The reload states (the ones a reload-rate perk scales).
    pub fn is_reload_family(self) -> bool {
        (Self::Reloading as i32..=Self::ReloadQuickEmpty as i32).contains(&(self as i32))
    }

    pub fn is_melee(self) -> bool {
        matches!(self, Self::MeleeInit | Self::MeleeFire | Self::MeleeEnd)
    }

    pub fn is_offhand(self) -> bool {
        (Self::OffhandInit as i32..=Self::OffhandDone as i32).contains(&(self as i32))
    }

    pub fn is_rechamber(self) -> bool {
        matches!(self, Self::Rechambering | Self::RechamberingBolted)
    }
}

#[must_use]
pub fn viewmodel_rocket_should_be_attached(
    clip: i32,
    weaponstate: i32,
    weapon_time: i32,
    reload_time_ms: i32,
    reload_show_rocket_time_ms: i32,
) -> bool {
    if clip > 0 {
        return true;
    }
    matches!(WeaponState::from_i32(weaponstate), Ok(state) if state.is_reload_family())
        && reload_time_ms.wrapping_sub(weapon_time) > reload_show_rocket_time_ms
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum FireType {
    FullAuto = 0,
    SingleShot = 1,
    BurstFire2 = 2,
    BurstFire3 = 3,
    BurstFire4 = 4,
}

impl FireType {
    pub fn from_i32(v: i32) -> Result<Self, WeaponDecodeError> {
        Ok(match v {
            0 => Self::FullAuto,
            1 => Self::SingleShot,
            2 => Self::BurstFire2,
            3 => Self::BurstFire3,
            4 => Self::BurstFire4,
            other => return Err(WeaponDecodeError::UnknownFireType(other)),
        })
    }

    pub fn is_full_auto(self) -> bool {
        matches!(self, Self::FullAuto)
    }

    pub fn is_single(self) -> bool {
        matches!(self, Self::SingleShot)
    }

    pub fn is_burst(self) -> bool {
        matches!(self, Self::BurstFire2 | Self::BurstFire3 | Self::BurstFire4)
    }

    pub fn burst_limit(self) -> Option<u8> {
        match self {
            Self::BurstFire2 => Some(2),
            Self::BurstFire3 => Some(3),
            Self::BurstFire4 => Some(4),
            _ => None,
        }
    }

    pub fn shot_limit_reached(self, shot_count: u8) -> bool {
        match self {
            Self::FullAuto => false,
            Self::SingleShot => shot_count != 0,
            Self::BurstFire2 => shot_count > 1,
            Self::BurstFire3 => shot_count > 2,
            Self::BurstFire4 => shot_count > 3,
        }
    }
}
