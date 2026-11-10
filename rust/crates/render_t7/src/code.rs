//! Code constants: the values BlackOps3.exe computes per view and writes
//! where a technique's arguments (type 0x400: location = byte offset in
//! the argument's constant buffer, size = bytes, u[0] low byte = code
//! constant index) ask, by the exe's index (`NAMES`, its table).

use glam::Mat4;

/// The exe's code constant table (index, name).
#[rustfmt::skip]
pub const NAMES: &[(u16, &str)] = &[
    (0x16, "materialColor"),
    (0x17, "fogConsts"),
    (0x18, "fogConsts2"),
    (0x19, "fogConsts3"),
    (0x1a, "fogConsts4"),
    (0x1b, "fogConsts5"),
    (0x1c, "fogConsts6"),
    (0x1d, "fogConsts7"),
    (0x1e, "fogConsts8"),
    (0x1f, "fogConsts9"),
    (0x20, "fogColor"),
    (0x23, "sunFogColor"),
    (0x22, "sunFogDir"),
    (0x21, "sunFog"),
    (0x9, "filterTap"),
    (0x1, "renderTargetSize"),
    (0x2, "upscaledTargetSize"),
    (0x0, "shadowParms"),
    (0x15, "zNear"),
    (0x2a, "clipSpaceLookupScale"),
    (0x2b, "clipSpaceLookupOffset"),
    (0x29, "skyRotationTransition"),
    (0x11, "colorMatrixR"),
    (0x12, "colorMatrixG"),
    (0x13, "colorMatrixB"),
    (0x24, "colorBias"),
    (0x25, "colorTintBase"),
    (0x26, "colorTintDelta"),
    (0x3, "gameTime"),
    (0x4, "gameTick"),
    (0x5, "subpixelOffset"),
    (0x6, "viewSpaceScaleBias"),
    (0xd5, "viewMatrix"),
    (0xd6, "inverseViewMatrix"),
    (0xd7, "transposeViewMatrix"),
    (0xd8, "inverseTransposeViewMatrix"),
    (0xd9, "projectionMatrix"),
    (0xda, "inverseProjectionMatrix"),
    (0xdb, "transposeProjectionMatrix"),
    (0xdc, "inverseTransposeProjectionMatrix"),
    (0xdd, "viewProjectionMatrix"),
    (0xde, "inverseViewProjectionMatrix"),
    (0xdf, "transposeViewProjectionMatrix"),
    (0xe0, "inverseTransposeViewProjectionMatrix"),
    (0xe1, "shadowLookupMatrix"),
    (0xe2, "inverseShadowLookupMatrix"),
    (0xe3, "transposeShadowLookupMatrix"),
    (0xe4, "inverseTransposeShadowLookupMatrix"),
    (0x2c, "viewportDimensions"),
    (0x2d, "skyMxR"),
    (0x2e, "skyMxG"),
    (0x2f, "skyMxB"),
    (0x30, "sunMxR"),
    (0x31, "sunMxG"),
    (0x32, "sunMxB"),
    (0x33, "postFxControl0"),
    (0x34, "postFxControl1"),
    (0x35, "postFxControl2"),
    (0x36, "postFxControl3"),
    (0x37, "postFxControl4"),
    (0x38, "postFxControl5"),
    (0x39, "postFxControl6"),
    (0x3a, "postFxControl7"),
    (0x3b, "postFxControl8"),
    (0x3c, "postFxControl9"),
    (0x3d, "postFxControlA"),
    (0x3e, "postFxControlB"),
    (0x3f, "postFxControlC"),
    (0x40, "postFxControlD"),
    (0x41, "postFxControlE"),
    (0x42, "postFxControlF"),
    (0x43, "postFxConst00"),
    (0x44, "postFxConst01"),
    (0x45, "postFxConst02"),
    (0x46, "postFxConst03"),
    (0x47, "postFxConst04"),
    (0x48, "postFxConst05"),
    (0x49, "postFxConst06"),
    (0x4a, "postFxConst07"),
    (0x4b, "postFxConst08"),
    (0x4c, "postFxConst09"),
    (0x4d, "postFxConst10"),
    (0x4e, "postFxConst11"),
    (0x4f, "postFxConst12"),
    (0x50, "postFxConst13"),
    (0x51, "postFxConst14"),
    (0x52, "postFxConst15"),
    (0x53, "postFxConst16"),
    (0x54, "postFxConst17"),
    (0x55, "postFxConst18"),
    (0x56, "postFxConst19"),
    (0x57, "postFxConst20"),
    (0x58, "postFxConst21"),
    (0x59, "postFxConst22"),
    (0x5a, "postFxConst23"),
    (0x5b, "postFxConst24"),
    (0x5c, "postFxConst25"),
    (0x5d, "postFxConst26"),
    (0x5e, "postFxConst27"),
    (0x5f, "postFxConst28"),
    (0x60, "postFxConst29"),
    (0x61, "postFxConst30"),
    (0x62, "postFxConst31"),
    (0x63, "postFxConst32"),
    (0x64, "postFxConst33"),
    (0x65, "postFxConst34"),
    (0x66, "postFxConst35"),
    (0x67, "postFxConst36"),
    (0x68, "postFxConst37"),
    (0x69, "postFxConst38"),
    (0x6a, "postFxConst39"),
    (0x6b, "postFxConst40"),
    (0x6c, "postFxConst41"),
    (0x6d, "postFxConst42"),
    (0x6e, "postFxConst43"),
    (0x6f, "postFxConst44"),
    (0x70, "postFxConst45"),
    (0x71, "postFxConst46"),
    (0x72, "postFxConst47"),
    (0x73, "postFxConst48"),
    (0x74, "postFxConst49"),
    (0x75, "postFxConst50"),
    (0x76, "postFxConst51"),
    (0x77, "postFxConst52"),
    (0x78, "postFxConst53"),
    (0x79, "postFxConst54"),
    (0x7a, "postFxConst55"),
    (0x7b, "postFxConst56"),
    (0x7c, "postFxConst57"),
    (0x7d, "postFxConst58"),
    (0x7e, "postFxConst59"),
    (0x7f, "postFxConst60"),
    (0x80, "postFxConst61"),
    (0x81, "postFxConst62"),
    (0x82, "postFxConst63"),
    (0x83, "postFxBloom00"),
    (0x84, "postFxBloom01"),
    (0x85, "postFxBloom02"),
    (0x86, "postFxBloom03"),
    (0x87, "postFxBloom04"),
    (0x88, "postFxBloom05"),
    (0x89, "postFxBloom06"),
    (0x8a, "postFxBloom07"),
    (0x8b, "postFxBloom08"),
    (0x8c, "postFxBloom09"),
    (0x8d, "postFxBloom10"),
    (0x8e, "postFxBloom11"),
    (0x8f, "postFxBloom12"),
    (0x90, "postFxBloom13"),
    (0x91, "postFxBloom14"),
    (0x92, "postFxBloom15"),
    (0x93, "postFxBloom16"),
    (0x94, "postFxBloom17"),
    (0x95, "postFxBloom18"),
    (0x96, "postFxBloom19"),
    (0x97, "postFxBloom20"),
    (0x98, "postFxBloom21"),
    (0x99, "postFxBloom22"),
    (0x9a, "postFxBloom23"),
    (0x9b, "postFxBloom24"),
    (0x9c, "postFxBloom25"),
    (0x9d, "postfxViewMatrix0"),
    (0x9e, "postfxViewMatrix1"),
    (0x9f, "postfxViewMatrix2"),
    (0xa0, "postfxViewMatrix3"),
    (0xa1, "postfxProjMatrix0"),
    (0xa2, "postfxProjMatrix1"),
    (0xa3, "postfxProjMatrix2"),
    (0xa4, "postfxProjMatrix3"),
    (0xa5, "postfxViewProjMatrix0"),
    (0xa6, "postfxViewProjMatrix1"),
    (0xa7, "postfxViewProjMatrix2"),
    (0xa8, "postfxViewProjMatrix3"),
    (0xa9, "hdrControl0"),
    (0xaa, "hdrControl1"),
    (0xab, "cloudLayerControl0"),
    (0xac, "cloudLayerControl1"),
    (0xad, "cloudLayerControl2"),
    (0xae, "cloudLayerControl3"),
    (0xaf, "cloudLayerControl4"),
    (0xb0, "adsZScale"),
    (0xb1, "ui3dUVSetup0"),
    (0xb2, "ui3dUVSetup1"),
    (0xb3, "ui3dUVSetup2"),
    (0xb4, "ui3dUVSetup3"),
    (0xb5, "ui3dUVSetup4"),
    (0xb6, "ui3dUVSetup5"),
    (0xb7, "cameraLook"),
    (0xb8, "cameraUp"),
    (0xb9, "cameraSide"),
    (0xba, "cameraVelocity"),
    (0xbb, "scriptVector0"),
    (0xbc, "scriptVector1"),
    (0xbd, "scriptVector2"),
    (0xbe, "scriptVector3"),
    (0xbf, "scriptVector4"),
    (0xc0, "scriptVector5"),
    (0xc1, "scriptVector6"),
    (0xc2, "scriptVector7"),
    (0xc3, "eyeOffset"),
    (0xc4, "extraCamParam"),
    (0xc5, "emblemLUTSelector"),
    (0x14, "shaderDebug"),
    (0xc6, "debugColorOverride"),
    (0xc7, "debugAlphaOverride"),
    (0xc8, "debugNormalOverride"),
    (0xc9, "debugSpecularOverride"),
    (0xca, "debugGlossOverride"),
    (0xcb, "debugOcclusionOverride"),
    (0xcc, "debugStreamerControl"),
    (0xcd, "computeSpriteControl"),
    (0xce, "invBcTexSizes"),
    (0xcf, "invMaskTexSizes"),
    (0x7, "relHDRExposure"),
    (0x27, "clothPrimaryTint"),
    (0x28, "clothSecondaryTint"),
    (0xd0, "triDensityFlags"),
    (0xd1, "triDensityParams"),
    (0xd2, "extraClipPlane0"),
    (0xd3, "extraClipPlane1"),
    (0xd4, "isDepthHack"),
    (0x8, "voldecalRevealTextureInfo"),
    (0x0, "MZ"),
];

pub fn name(index: u16) -> &'static str {
    NAMES.iter().find(|(i, _)| *i == index).map_or("?", |(_, n)| n)
}

pub const VIEW_PROJECTION_MATRIX: u16 = 0xdd;
pub const EYE_OFFSET: u16 = 0xc3;

/// `colorMatrixR/G/B`: no program Black Ops III ships reads them (no
/// technique argument in `core_patch`, `core_common`, `core_gfx`,
/// `core_pre_gfx`, `core_post_gfx`, `mp_common`, the UI zones or the map
/// zones staged here), and no store in the exe writes them: the colour
/// grade is the LUT `create_lut2dv` builds from the view's visions. They
/// stay unset; [`readers`] counts any argument that would read them.
pub const COLOR_MATRIX: [u16; 3] = [0x11, 0x12, 0x13];

/// The technique arguments in `cap` (every technique, every draw method)
/// that read one of the code constants `indices`.
pub fn readers(cap: &fastfile_t7::walk::capture::Capture, indices: &[u16]) -> usize {
    let hit = |a: &fastfile_t7::walk::capture::ShaderArg| a.ty & 0xff00 == 0x400 && indices.contains(&((a.u[0] & 0xffff) as u16));
    cap.techsets
        .iter()
        .flat_map(|t| t.techniques.iter().flatten())
        .map(|ti| ti.args.iter().filter(|a| hit(a)).count() + ti.methods.iter().flatten().map(|m| m.args.iter().filter(|a| hit(a)).count()).sum::<usize>())
        .sum()
}

/// The per-view values known so far, by index, as 16-byte rows.
/// `skyRotationTransition`.
pub const SKY_ROTATION_TRANSITION: u16 = 0x29;
/// `skyMxR` (0x2d) .. `sunMxB` (0x32): the sky shader's colour matrices.
pub const SKY_MX_R: u16 = 0x2d;

#[derive(Clone, Debug, Default)]
pub struct CodeConsts {
    values: Vec<(u16, Vec<[f32; 4]>)>,
}

/// A matrix as the programs read it: row-vector convention (the register
/// rows are the column-vector matrix's columns).
pub fn rows(m: Mat4) -> Vec<[f32; 4]> {
    (0..4).map(|i| m.col(i).to_array()).collect()
}

impl CodeConsts {
    pub fn set(&mut self, index: u16, rows: Vec<[f32; 4]>) {
        self.values.retain(|(i, _)| *i != index);
        self.values.push((index, rows));
    }
    pub fn get(&self, index: u16) -> Option<&[[f32; 4]]> {
        self.values.iter().find(|(i, _)| *i == index).map(|(_, r)| r.as_slice())
    }

    /// A view's constants: eye-relative view-projection (reversed depth)
    /// and the eye offset world positions are made relative to.
    pub fn view(camera: &render_h1::code::Camera) -> Self {
        let mut c = Self::default();
        c.set(VIEW_PROJECTION_MATRIX, rows(camera.view_projection));
        c.set(EYE_OFFSET, vec![[camera.eye.x, camera.eye.y, camera.eye.z, 0.0]]);
        c
    }
}
