//! `XAssetType` of Modern Warfare Remastered (H1) PC x64, in the game's order (0x00..0x47).
//!
//! The values and names follow x64-zt `zonetool/h1/structs.hpp`;
//! `header_size` is the x64 size of the asset's top-level struct (`layout.rs`),
//! `0` where the tools have no struct for it.

macro_rules! asset_types {
    ($($variant:ident = $value:literal, $name:literal, $size:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u32)]
        pub enum AssetType {
            $($variant = $value,)*
        }

        impl AssetType {
            pub const ALL: &[AssetType] = &[$(AssetType::$variant,)*];

            pub const fn from_u32(value: u32) -> Option<Self> {
                match value {
                    $($value => Some(AssetType::$variant),)*
                    _ => None,
                }
            }

            /// The name the game's tools use (`sound`, `xmodel`, ...).
            pub const fn name(self) -> &'static str {
                match self {
                    $(AssetType::$variant => $name,)*
                }
            }

            /// Size in bytes of the asset's top-level struct (x64), `0` when unknown.
            pub const fn header_size(self) -> usize {
                match self {
                    $(AssetType::$variant => $size,)*
                }
            }
        }
    };
}

asset_types! {
    PhysPreset = 0x00, "physpreset", 96;
    PhysCollmap = 0x01, "physcollmap", 88;
    PhysWaterPreset = 0x02, "physwaterpreset", 128;
    PhysWorld = 0x03, "physworld", 56;
    PhysConstraint = 0x04, "physconstraint", 40;
    XAnimParts = 0x05, "xanim", 216;
    XModelSurfs = 0x06, "xmodelsurfs", 56;
    XModel = 0x07, "xmodel", 680;
    Material = 0x08, "material", 592;
    ComputeShader = 0x09, "computeshader", 32;
    VertexShader = 0x0a, "vertexshader", 32;
    HullShader = 0x0b, "hullshader", 32;
    DomainShader = 0x0c, "domainshader", 32;
    PixelShader = 0x0d, "pixelshader", 32;
    VertexDecl = 0x0e, "vertexdecl", 2112;
    TechniqueSet = 0x0f, "techset", 1936;
    Image = 0x10, "image", 104;
    Sound = 0x11, "sound", 32;
    SoundSubmix = 0x12, "soundsubmix", 24;
    SoundCurve = 0x13, "sndcurve", 152;
    LpfCurve = 0x14, "lpfcurve", 152;
    ReverbCurve = 0x15, "reverbcurve", 152;
    SoundContext = 0x16, "sndcontext", 16;
    LoadedSound = 0x17, "loaded_sound", 64;
    ClipMap = 0x18, "clipmap", 640;
    ComWorld = 0x19, "comworld", 40;
    GlassWorld = 0x1a, "glassworld", 16;
    PathData = 0x1b, "pathdata", 136;
    VehicleTrack = 0x1c, "vehicletrack", 0;
    MapEnts = 0x1d, "map_ents", 296;
    FxWorld = 0x1e, "fxworld", 176;
    GfxWorld = 0x1f, "gfxworld", 2864;
    LightDef = 0x20, "lightdef", 48;
    UiMap = 0x21, "uimap", 0;
    MenuList = 0x22, "menulist", 24;
    Menu = 0x23, "menu", 200;
    AnimClass = 0x24, "animclass", 88;
    LocalizeEntry = 0x25, "localize", 16;
    Attachment = 0x26, "attachment", 144;
    Weapon = 0x27, "weapon", 3616;
    SndDriverGlobals = 0x28, "snddriverglobals", 56;
    Fx = 0x29, "fx", 64;
    ImpactFx = 0x2a, "impactfx", 16;
    SurfaceFx = 0x2b, "surfacefx", 16;
    AiType = 0x2c, "aitype", 0;
    MpType = 0x2d, "mptype", 0;
    Character = 0x2e, "character", 0;
    XModelAlias = 0x2f, "xmodelalias", 0;
    RawFile = 0x30, "rawfile", 24;
    ScriptFile = 0x31, "scriptfile", 40;
    StringTable = 0x32, "stringtable", 24;
    Leaderboard = 0x33, "leaderboard", 48;
    VirtualLeaderboard = 0x34, "virtualleaderboard", 40;
    StructuredDataDef = 0x35, "structureddatadef", 24;
    Ddl = 0x36, "ddl", 16;
    Proto = 0x37, "proto", 80;
    Tracer = 0x38, "tracer", 128;
    Vehicle = 0x39, "vehicle", 2256;
    AddonMapEnts = 0x3a, "addon_map_ents", 136;
    NetConstStrings = 0x3b, "netconststrings", 32;
    ReverbPreset = 0x3c, "reverbpreset", 72;
    LuaFile = 0x3d, "luafile", 24;
    Scriptable = 0x3e, "scriptable", 64;
    EquipmentSndTable = 0x3f, "equipsndtable", 88;
    VectorField = 0x40, "vectorfield", 48;
    DopplerPreset = 0x41, "dopplerpreset", 32;
    ParticleSimAnimation = 0x42, "particlesimanimation", 96;
    Laser = 0x43, "laser", 152;
    SkeletonScript = 0x44, "skeletonscript", 64;
    Clut = 0x45, "clut", 32;
    Ttf = 0x46, "ttf", 32;
}
