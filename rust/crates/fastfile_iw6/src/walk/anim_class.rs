//! Animation classes (`AnimationClass`, 88 bytes): the class name, the state
//! machine (header, its states, then per state the anim entries and the anim
//! indices, which are RUNTIME: no file data; then the aim sets), the scriptable (null in the one sample), the sound arrays
//! (`scr_string_t`s), the effect notes, the effect pointers (each loaded as an
//! effect) and the effect tags. Order as in the IW7 reader
//! (`fastfile_iw7::walk::anim_class`), minus its alias lists, which IW6 states
//! do not have.

use super::{AssetId, Walker};
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::layout as L;
use crate::stream::{RUNTIME, VIRTUAL};

/// A pointer to data the loader allocates in RUNTIME (no file data).
fn runtime(w: &mut Walker, field: usize, len: usize) -> Result<()> {
    if w.s.ptr_at(field)?.is_inline() {
        w.s.push(RUNTIME);
        w.inline_array(field, len, 7)?;
        w.s.pop();
    }
    Ok(())
}

fn state_machine(w: &mut Walker, field: usize) -> Result<()> {
    use L::AnimationAimSet as M;
    use L::AnimationState as St;
    use L::AnimationStateMachine as S;
    let Some(sm) = w.inline_array(field, S::SIZE, 7)? else {
        return Ok(());
    };
    let states = w.s.u16_at(sm + S::stateCount)? as usize;
    let aims = w.s.u16_at(sm + S::aimSetCount)? as usize;
    if let Some(t) = w.inline_array(sm + S::states, St::SIZE * states, 7)? {
        for i in 0..states {
            let st = t + St::SIZE * i;
            let entries = w.s.u8_at(st + St::entryCount)? as usize;
            w.blob(st + St::animEntries, L::AnimationEntry::SIZE * entries, 3)?;
            runtime(w, st + St::animIndices, 8 * entries)?;
        }
    }
    w.inline_array(sm + S::aimSets, M::SIZE * aims, 7)?;
    Ok(())
}

pub(super) fn anim_class(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::AnimationClass as C;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + C::className)?.unwrap_or_default();
    state_machine(w, h + C::stateMachine)?;
    w.asset(AssetType::Scriptable, h + C::scriptable)?;
    let sounds = w.s.u16_at(h + C::soundCount)? as usize;
    let effects = w.s.u16_at(h + C::effectCount)? as usize;
    for f in [C::soundNotes, C::soundNames, C::soundOptions] {
        w.blob(h + f, 4 * sounds, 3)?;
    }
    w.blob(h + C::effectNotes, 4 * effects, 3)?;
    if let Some(t) = w.inline_array(h + C::effectDefs, 8 * effects, 7)? {
        for i in 0..effects {
            w.asset(AssetType::Fx, t + 8 * i)?;
        }
    }
    w.blob(h + C::effectTags, 4 * effects, 3)?;
    w.s.pop();
    Ok(w.name_only(AssetType::AnimClass, name))
}
