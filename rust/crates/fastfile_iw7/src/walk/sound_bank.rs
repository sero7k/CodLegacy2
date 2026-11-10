//! Sound banks: x64-zt `iw7/assets/sound_bank.cpp`'s writer: the strings,
//! the alias lists (each list's name, its aliases, each alias's strings),
//! the alias index, the send effects, the sound table's arrays (each with
//! its strings), the ducks (with their 128-float curves) and the music
//! sets. The sound data itself is not in the zone: aliases name their
//! files in the `.sabl` (loaded) / `.sabs` (streamed) sound asset banks.

use super::{AmbientDefRec, AmbientElementRec, AssetId, ReverbRec, SendEffectRec, SndAliasRec, SndZoneRec, SoundBankRec, Walker};
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::layout as L;
use crate::stream::VIRTUAL;

/// An array of `size`-byte records whose string fields follow.
fn records(w: &mut Walker, h: usize, ptr: usize, count: usize, size: usize, mask: u64, strs: &[usize]) -> Result<()> {
    records_kept(w, h, ptr, count, size, mask, strs).map(|_| ())
}

/// `records`, returning where the table is, its count and each record's
/// strings (in `strs` order).
fn records_kept(w: &mut Walker, h: usize, ptr: usize, count: usize, size: usize, mask: u64, strs: &[usize]) -> Result<Option<(usize, usize, Vec<Vec<String>>)>> {
    let n = w.s.u32_at(h + count)? as usize;
    let Some(t) = w.inline_array(h + ptr, size * n, mask)? else { return Ok(None) };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = Vec::with_capacity(strs.len());
        for &f in strs {
            row.push(w.xstr(t + size * i + f)?.unwrap_or_default());
        }
        out.push(row);
    }
    Ok(Some((t, n, out)))
}

pub(super) fn sound_bank(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::SndBank as B;
    w.s.push(VIRTUAL);
    let mut rec = SoundBankRec {
        name: w.xstr(h + B::name)?.unwrap_or_default(),
        zone: w.xstr(h + B::zone)?.unwrap_or_default(),
        ..Default::default()
    };
    rec.game_language = w.xstr(h + B::gameLanguage)?.unwrap_or_default();
    rec.sound_language = w.xstr(h + B::soundLanguage)?.unwrap_or_default();

    use L::SndAlias as A;
    use L::SndAliasList as AL;
    let lists = w.s.u32_at(h + B::aliasCount)? as usize;
    if let Some(t) = w.inline_array(h + B::alias, AL::SIZE * lists, 7)? {
        for i in 0..lists {
            let l = t + AL::SIZE * i;
            let name = w.xstr(l + AL::aliasName)?.unwrap_or_default();
            let n = w.s.i32_at(l + AL::count)?.max(0) as usize;
            let mut entries = Vec::new();
            let mut records = Vec::new();
            if let Some(a) = w.inline_array(l + AL::head, A::SIZE * n, 7)? {
                for j in 0..n {
                    let e = a + A::SIZE * j;
                    let record = w.s.slice(e, A::SIZE)?.to_vec();
                    w.xstr(e + A::aliasName)?;
                    w.xstr(e + A::subtitle)?;
                    let secondary = w.xstr(e + A::secondaryAliasName)?;
                    w.xstr(e + A::stopAliasName)?;
                    records.push((secondary, record));
                    let file = w.xstr(e + A::assetFileName)?;
                    let flags = w.s.u32_at(e + A::flags)?;
                    entries.push((file, ((flags >> 7) & 3) as u8, w.s.u32_at(e + A::assetId)?));
                }
            }
            rec.aliases.push(SndAliasRec { name, entries, records });
        }
    }
    w.blob(h + B::aliasIndex, L::SndIndexEntry::SIZE * lists, 7)?;
    let n = w.s.u32_at(h + B::sendEffectCount)? as usize;
    if let Some(at) = w.blob(h + B::sendEffects, L::SndSendEffectParams::SIZE * n, 3)? {
        use L::SndSendEffectParams as S;
        for i in 0..n {
            let r = at + S::SIZE * i;
            let raw = w.s.slice(r + S::name, S::id - S::name)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let mut values = [0f32; 17];
            for (k, v) in values.iter_mut().enumerate() {
                *v = w.s.f32_at(r + S::smoothing + 4 * k)?;
            }
            rec.send_effects.push(SendEffectRec { name: String::from_utf8_lossy(&raw[..end]).into_owned(), id: w.s.u32_at(r + S::id)?, values });
        }
    }

    records(w, h, B::soundTable__adsrSettings, B::soundTable__adsrCount, L::ADSRSetting::SIZE, 7, &[L::ADSRSetting::name])?;
    if let Some((t, n, names)) = records_kept(w, h, B::soundTable__ambientDefs, B::soundTable__ambientDefCount, L::AmbientDef::SIZE, 7, &[L::AmbientDef::name])? {
        use L::AmbientDef as D;
        for i in 0..n {
            let r = t + D::SIZE * i;
            rec.table.ambient_defs.push(AmbientDefRec {
                name: names[i][0].clone(),
                id: w.s.u32_at(r + D::id)?,
                first_event: w.s.u16_at(r + D::ambientEventIndex)?,
                event_count: w.s.u16_at(r + D::numEvents)?,
                time: [w.s.f32_at(r + D::timeMin)?, w.s.f32_at(r + D::timeMax)?],
            });
        }
    }
    if let Some((t, n, _)) = records_kept(w, h, B::soundTable__ambientEvents, B::soundTable__ambientEventCount, L::AmbientEvent::SIZE, 3, &[])? {
        use L::AmbientEvent as E;
        for i in 0..n {
            let r = t + E::SIZE * i;
            rec.table.ambient_events.push((w.s.u32_at(r + E::ambientElementIndex)?, w.s.f32_at(r + E::weight)?));
        }
    }
    if let Some((t, n, names)) = records_kept(w, h, B::soundTable__ambientElements, B::soundTable__ambientElementsCount, L::AmbientElement::SIZE, 7, &[L::AmbientElement::aliasName])? {
        use L::AmbientElement as E;
        for i in 0..n {
            let r = t + E::SIZE * i;
            rec.table.ambient_elements.push(AmbientElementRec {
                alias: names[i][0].clone(),
                range: [w.s.f32_at(r + E::rangeMin)?, w.s.f32_at(r + E::rangeMax)?],
                cone: [w.s.f32_at(r + E::coneMin)?, w.s.f32_at(r + E::coneMax)?],
            });
        }
    }
    records(w, h, B::soundTable__filters, B::soundTable__filterCount, L::FilterDef::SIZE, 3, &[])?;
    records(w, h, B::soundTable__mixes, B::soundTable__mixCount, L::MixDef::SIZE, 3, &[])?;
    let adsr = [L::AdsrZoneEntry::name, L::AdsrZoneEntry::weaponName];
    records(w, h, B::soundTable__npcADSRZones, B::soundTable__npcADSRZoneCount, L::AdsrZoneEntry::SIZE, 7, &adsr)?;
    records(w, h, B::soundTable__occlusionFilters, B::soundTable__occlusionFilterCount, L::OccludeDef::SIZE, 3, &[])?;
    records(w, h, B::soundTable__playerADSRZones, B::soundTable__playerADSRZoneCount, L::AdsrZoneEntry::SIZE, 7, &adsr)?;
    if let Some((t, n, _)) = records_kept(w, h, B::soundTable__reverbs, B::soundTable__reverbCount, L::ReverbDef::SIZE, 3, &[])? {
        use L::ReverbDef as R;
        for i in 0..n {
            let r = t + R::SIZE * i;
            // (The room type is an inline name, up to the dry level.)
            let raw = w.s.slice(r + R::roomType, R::dryLevel - R::roomType)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            rec.table.reverbs.push(ReverbRec {
                id: w.s.u32_at(r + R::id)?,
                room_type: String::from_utf8_lossy(&raw[..end]).into_owned(),
                dry: w.s.f32_at(r + R::dryLevel)?,
                wet: w.s.f32_at(r + R::wetLevel)?,
                fade: w.s.f32_at(r + R::fadeTime)?,
            });
        }
    }
    records(w, h, B::soundTable__timeScaleSettings, B::soundTable__timeScaleSettingCount, L::TimescaleEntry::SIZE, 3, &[])?;
    {
        use L::ZoneDef as Z;
        let strs = [
            Z::ambientStream,
            Z::zoneName,
            Z::stateName,
            Z::ambientName,
            Z::mixName,
            Z::reverbName,
            Z::filterName,
            Z::occlusionName,
            Z::plrAdsrName,
            Z::npcAdsrName,
            Z::weapReflName,
            Z::fullOccName,
            Z::playerBreathStateName,
        ];
        if let Some((t, n, s)) = records_kept(w, h, B::soundTable__zones, B::soundTable__zoneCount, Z::SIZE, 7, &strs)? {
            for i in 0..n {
                let r = t + Z::SIZE * i;
                rec.table.zones.push(SndZoneRec {
                    ambient_stream: s[i][0].clone(),
                    name: s[i][1].clone(),
                    state: s[i][2].clone(),
                    ambient: s[i][3].clone(),
                    mix: s[i][4].clone(),
                    reverb: s[i][5].clone(),
                    occlusion: s[i][7].clone(),
                    reverb_index: w.s.u16_at(r + Z::reverbIndex)?,
                    ambient_def_index: w.s.u16_at(r + Z::ambientDefIndex)?,
                });
            }
        }
    }
    records(w, h, B::soundTable__unk, B::soundTable__unkCount, L::unk_1453E2FD8::SIZE, 7, &[L::unk_1453E2FD8::name])?;
    records(w, h, B::soundTable__fullOcclusionDefs, B::soundTable__fullOcclusionDefCount, L::FullOcclusionDef::SIZE, 3, &[])?;
    let breath = [L::PlayerBreathStateDef::inhaleAlias, L::PlayerBreathStateDef::exhaleAlias];
    records(w, h, B::soundTable__plrBreathStateDefs, B::soundTable__plrBreathStateDefCount, L::PlayerBreathStateDef::SIZE, 7, &breath)?;

    use L::SndDuck as D;
    let n = w.s.u32_at(h + B::duckCount)? as usize;
    if let Some(t) = w.inline_array(h + B::ducks, D::SIZE * n, 7)? {
        for i in 0..n {
            w.blob(t + D::SIZE * i + D::attenuation, 4 * 128, 31)?;
            w.blob(t + D::SIZE * i + D::lpf, 4 * 128, 31)?;
        }
    }
    use L::SndMusicSet as M;
    use L::SndMusicState as MS;
    let n = w.s.u32_at(h + B::musicSetCount)? as usize;
    if let Some(t) = w.inline_array(h + B::musicSets, M::SIZE * n, 7)? {
        for i in 0..n {
            let m = t + M::SIZE * i;
            let states = w.s.u32_at(m + M::stateCount)? as usize;
            if let Some(st) = w.inline_array(m + M::states, MS::SIZE * states, 7)? {
                for j in 0..states {
                    let s = st + MS::SIZE * j;
                    let loops = w.s.u32_at(s + MS::loopCount)? as usize;
                    w.blob(s + MS::loops, L::SndMusicAsset::SIZE * loops, 3)?;
                }
            }
        }
    }
    w.s.pop();
    w.cap.sound_banks.push(rec);
    Ok(AssetId { ty: AssetType::SoundBank, index: (w.cap.sound_banks.len() - 1) as u32 })
}

/// A zone whose walk stopped before its sound bank (`common_mp`, at its
/// equipment sound table): the bank's header found after the stop (a
/// header whose name string, "<zone>.<language>", follows it) and walked
/// from there to the zone's end.
pub fn recover_sound_bank(zone: &crate::Zone, stop: &super::WalkStop) -> Option<SoundBankRec> {
    let i = zone.assets.assets.iter().enumerate().skip(stop.asset_index).rev().find(|(_, a)| a.asset_type() == Some(AssetType::SoundBank))?.0;
    let img = &zone.image;
    let size = AssetType::SoundBank.header_size();
    let fe = [0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    for p in stop.file_pos..img.len().saturating_sub(size + 8) {
        if img[p..p + 8] != fe {
            continue;
        }
        let name = &img[p + size..];
        let Some(n) = name.iter().take(128).position(|&b| b == 0) else {
            continue;
        };
        let name = &name[..n];
        if n < 3 || !name.iter().all(u8::is_ascii_graphic) || !name.contains(&b'.') {
            continue;
        }
        let mut w = Walker::new(img, &zone.assets, &zone.header.stream_files);
        if w.walk_from(Some((i, p))).is_ok() && w.stream_pos() == img.len() {
            return w.cap.sound_banks.pop();
        }
    }
    None
}
