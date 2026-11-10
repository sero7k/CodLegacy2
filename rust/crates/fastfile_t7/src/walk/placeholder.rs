//! Asset types read only as "," placeholders so far: the header (its size
//! measured on retail zones, name first) and the name, every other 8-byte
//! field required not to load data. An inline asset of one of these types
//! that loads more fails the walk at that field, naming it, until the type
//! gets a real walker.
//!
//! ```text
//! lightdescription 0x240   (mp_common: ",gfx_dlight_gen_omni_r200599854254")
//! aimtable         0x330   (mp_common: ",soldier_aim")
//! customizationtablecolor 0x30 (mp_common: ",cc_c_t7_mp_warrior_body1_skin1")
//! customizationtable 0x28 (mp_common: ",mp_character_customization")
//! maptable         0x28    (mp_common: ",maptable_mp_base")
//! xcam             0xa8    (mp_common: ",ui_cam_cac_lmg_light")
//! ```

use super::simple::placeholder;
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::stream::Stream;

macro_rules! placeholders {
    ($($f:ident => $t:ident;)*) => {
        $(
            pub fn $f(s: &mut Stream, h: usize) -> Result<Option<String>> {
                placeholder(s, AssetType::$t, h, 0)
            }
        )*
    };
}

placeholders! {
    xcam => XCam;
    customization_table => CustomizationTable;
}

/// `CustomizationTableColor` (0x30), as the exe's loader reads it: 0x00
/// name; 0x08 char*; 0x20 u32 count; 0x28 `CustomizationColorInfo`[count]
/// (0x138 each, aligned 8): 0x00 u16 n; 0x08 {Material*, GfxImage*}[n]
/// (16 each, aligned 8); 0x18 three inline 0x48 structs, each a Material*
/// at +8 and a GfxImage* at +0x30.
pub fn customization_table_color(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::CustomizationTableColor.header_size(), &[0, 8, 0x28])?;
    let name = s.string(h)?;
    s.string(h + 8)?;
    let n = s.u32_at(h + 0x20)? as usize;
    if let Some(t) = s.records(h + 0x28, n, 0x138, 7)? {
        for i in 0..n {
            let r = t + 0x138 * i;
            let mut known = vec![8];
            known.extend((0..3).flat_map(|j| [0x18 + 0x48 * j + 8, 0x18 + 0x48 * j + 0x30]));
            s.no_loads(r, 0x138, &known)?;
            let m = s.u16_at(r)? as usize;
            if let Some(e) = s.records(r + 8, m, 16, 7)? {
                for k in 0..m {
                    s.asset_named(AssetType::Material, e + 16 * k)?;
                    s.asset_named(AssetType::Image, e + 16 * k + 8)?;
                }
            }
            for j in 0..3 {
                let b = r + 0x18 + 0x48 * j;
                s.asset_named(AssetType::Material, b + 8)?;
                s.asset_named(AssetType::Image, b + 0x30)?;
            }
        }
    }
    Ok(name)
}

/// `AimTable` (0x330), as the exe's loader reads it: 0x00 name; 0x08 u32
/// script string; 0x10 25 inline entries (0x20 each): a char* at +0 (and a
/// script string at +8).
pub fn aim_table(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let mut known = vec![0];
    known.extend((0..25).map(|i| 0x10 + 0x20 * i));
    s.no_loads(h, AssetType::AimTable.header_size(), &known)?;
    let name = s.string(h)?;
    for i in 0..25 {
        s.string(h + 0x10 + 0x20 * i)?;
    }
    Ok(name)
}

/// `MapTable` (0x28), as the exe's loader reads it: name; u32 count at 8;
/// `MapTableEntry`[count] (0xf8, aligned 8) at 0x10: strings at 0, 0x10,
/// 0x18, 0x20, 0x38, 0x28, 0x30, 0x40, 0x48, 0x50; image 0x58; string 0x60;
/// images 0x68, 0x70; strings 0x80, 0x88, 0x90; objective list 0x98;
/// string 0xa0; script bundle list 0xa8; strings 0xb0, 0xc8; image 0xd0;
/// u32 count 0xe4, pairs[count] (0x18, aligned 8: script strings, u32 kind
/// at +8, an asset at +0x10 by kind: 3/4 animation, 6 model, 0xd camera,
/// 0xf image) at 0xe8.
pub fn map_table(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let name = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    if let Some(t) = s.records(h + 0x10, n, 0xf8, 7)? {
        for i in 0..n {
            let e = t + 0xf8 * i;
            for o in [0usize, 0x10, 0x18, 0x20, 0x38, 0x28, 0x30, 0x40, 0x48, 0x50] {
                s.string(e + o)?;
            }
            s.asset_named(AssetType::Image, e + 0x58)?;
            s.string(e + 0x60)?;
            s.asset_named(AssetType::Image, e + 0x68)?;
            s.asset_named(AssetType::Image, e + 0x70)?;
            for o in [0x80usize, 0x88, 0x90] {
                s.string(e + o)?;
            }
            s.asset_named(AssetType::ObjectiveList, e + 0x98)?;
            s.string(e + 0xa0)?;
            s.asset_named(AssetType::ScriptBundleList, e + 0xa8)?;
            s.string(e + 0xb0)?;
            s.string(e + 0xc8)?;
            s.asset_named(AssetType::Image, e + 0xd0)?;
            let m = s.u32_at(e + 0xe4)? as usize;
            if let Some(p) = s.records(e + 0xe8, m, 0x18, 7)? {
                for k in 0..m {
                    let r = p + 0x18 * k;
                    if s.u32_at(r + 4)? == 0 {
                        continue;
                    }
                    let ty = match s.u32_at(r + 8)? {
                        3 | 4 => AssetType::XAnimParts,
                        6 => AssetType::XModel,
                        0xd => AssetType::XCam,
                        0xf => AssetType::Image,
                        _ => continue,
                    };
                    s.asset_named(ty, r + 0x10)?;
                }
            }
        }
    }
    Ok(name)
}
