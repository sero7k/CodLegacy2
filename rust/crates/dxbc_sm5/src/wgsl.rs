//! Program → WGSL.
//!
//! Registers are `vec4<u32>` holding raw bits, as in DXBC: every
//! instruction bitcasts its sources to the type it computes in and its
//! result back. Constant buffers are arrays of `vec4<u32>` laid out by
//! register; the caller fills them (the shader's own reflection is often
//! stripped, so the layout comes from whoever binds the shader).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::container::{Container, SigElement};
use crate::decode::{Index, Instr, Operand, Program, Select, Stage, op, ot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranslateError {
    Container(crate::container::ContainerError),
    Decode(crate::decode::DecodeError),
    UnsupportedStage(Stage),
    UnsupportedOpcode(u16),
    UnsupportedOperand(u16),
    UnsupportedResource {
        slot: u32,
        dim: u32,
    },
    Malformed(&'static str),
    /// A vertex shader with no SV_Position: it feeds tessellation.
    NoPosition,
}

/// Where a stage's resources go. Constant buffers, textures and samplers
/// each take a group; `binding_base` is added to the register number so
/// the vertex and pixel stages of one pass can share the groups.
#[derive(Clone, Copy, Debug)]
pub struct BindingPlan {
    pub cbuffer_group: u32,
    pub texture_group: u32,
    pub sampler_group: u32,
    pub binding_base: u32,
}

impl BindingPlan {
    pub const VERTEX: Self = Self {
        cbuffer_group: 0,
        texture_group: 1,
        sampler_group: 2,
        binding_base: 0,
    };
    pub const PIXEL: Self = Self {
        cbuffer_group: 0,
        texture_group: 1,
        sampler_group: 2,
        binding_base: 32,
    };
}

/// A varying's location, keyed by its semantic (upper case) and index.
pub type VaryingMap = Vec<(String, u32, u32)>;

#[derive(Clone, Debug)]
pub struct Options {
    pub bindings: BindingPlan,
    /// Locations for vertex outputs / pixel inputs. Empty: the register
    /// number. A pixel input whose semantic is not listed reads zero.
    pub varyings: VaryingMap,
    /// Vertex stage: each output's interpolation as the pixel stage
    /// declares it ((semantic, index, mode), mode as `VaryingRef::interp`),
    /// since both stages must agree and only the pixel stage says.
    pub output_interp: Vec<(String, u32, u32)>,
    /// Compute stage: each typed UAV's WGSL storage format by slot
    /// (`rgba16float`, `r32uint`, ...); unlisted ones take one from their
    /// return type (float `rgba16float`, uint `r32uint`, sint `r32sint`).
    pub uav_formats: Vec<(u32, String)>,
    /// Compute stage: declare typed buffer UAVs (`RWBuffer<uint>` and the
    /// like) as `array<u32>` storage buffers, one 32-bit element per index,
    /// instead of 1-D storage textures, so they can be bound as buffers
    /// (indirect dispatch arguments). Only right for single-channel 32-bit
    /// formats, which is what such buffers use.
    pub typed_buffer_uavs_as_words: bool,
    /// Typed buffer SRVs (`Buffer<uint>`, `Buffer<float4>`): declared as
    /// `array<u32>` storage buffers (bound like raw buffers), each element
    /// this many 32-bit words (slot, words); a slot not listed takes one
    /// word an element, right for single-channel 32-bit formats. `ld`
    /// returns the element's words bitcast to the declared type, missing
    /// channels 0 and alpha 1, as D3D does.
    pub srv_buffer_words: Vec<(u32, u32)>,
    /// Compute stage: declare `override t7_group_offset: u32 = 0u;` and add
    /// it to `SV_GroupID.x`, so one translation can be dispatched in pieces
    /// of at most the device's workgroups per dimension (the pipeline's
    /// override says where its piece starts).
    pub group_offset_override: bool,
    /// Pixel stage: `SV_Position.w` as D3D gives it (the clip w; WGSL's
    /// position builtin holds its reciprocal). Off: the builtin as it is
    /// (what the translations made before keep).
    pub d3d_position_w: bool,
    /// Compute stage (audit): every structured, raw and atomic access to a
    /// buffer first counts, in `t7_oob` (group of the constant buffers,
    /// binding [`OOB_BINDING`]), the accesses that fall outside the bound
    /// buffer (where a real GPU's robust access reads zero and drops the
    /// write), at word `T7_OOB_BASE + (register + 64 * is_uav) * 3 + kind`
    /// (0 read, 1 write, 2 atomic). The caller gives `T7_OOB_BASE` as a
    /// `const` ahead of the module.
    pub bounds_audit: bool,
}

/// Binding of the audit counters (`Options::bounds_audit`).
pub const OOB_BINDING: u32 = 100;

impl Options {
    pub fn for_stage(stage: Stage) -> Self {
        Self {
            bindings: if stage == Stage::Vertex {
                BindingPlan::VERTEX
            } else {
                BindingPlan::PIXEL
            },
            varyings: Vec::new(),
            output_interp: Vec::new(),
            uav_formats: Vec::new(),
            typed_buffer_uavs_as_words: false,
            srv_buffer_words: Vec::new(),
            group_offset_override: false,
            d3d_position_w: false,
            bounds_audit: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureDim {
    D1,
    D2,
    D2Array,
    D3,
    Cube,
    CubeArray,
    D2Ms,
    Buffer,
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureKind {
    Float,
    Sint,
    Uint,
    /// Used with a compare sample: bound as a depth texture.
    Depth,
    /// A raw (byte address) buffer: a read-only storage buffer of u32.
    RawBuffer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureRef {
    pub slot: u32,
    pub binding: u32,
    pub dim: TextureDim,
    pub kind: TextureKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplerRef {
    pub slot: u32,
    pub binding: u32,
    pub comparison: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CbufferRef {
    pub slot: u32,
    pub binding: u32,
    /// Size in 16-byte registers.
    pub registers: u32,
}

/// A vertex input or a varying as the WGSL sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaryingRef {
    pub semantic: String,
    pub index: u32,
    pub location: u32,
    pub register: u32,
    pub mask: u8,
    /// 1 uint, 2 sint, 3 float.
    pub component_type: u32,
    /// Pixel inputs: D3D interpolation mode (1 constant, 2 linear, 3
    /// centroid, 4 noperspective, 5 noperspective centroid, 6 sample,
    /// 7 noperspective sample; 0 unknown).
    pub interp: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Reflection {
    pub cbuffers: Vec<CbufferRef>,
    pub textures: Vec<TextureRef>,
    pub samplers: Vec<SamplerRef>,
    pub inputs: Vec<VaryingRef>,
    pub outputs: Vec<VaryingRef>,
    /// Pixel stage: writes depth.
    pub writes_depth: bool,
    /// Pixel stage: render targets written (SV_Target index).
    pub targets: Vec<u32>,
    /// The shader discards.
    pub discards: bool,
    /// Compute stage: unordered access views.
    pub uavs: Vec<UavRef>,
    /// Compute stage: threads per group.
    pub thread_group: [u32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UavKind {
    /// A typed texture (dimension, WGSL storage format index into `format`).
    Typed(TextureDim),
    /// A byte-address buffer (u32 words).
    Raw,
    /// A structured buffer (stride in bytes).
    Structured(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UavRef {
    pub slot: u32,
    pub binding: u32,
    pub kind: UavKind,
    /// Typed: the storage format the WGSL declares.
    pub format: String,
    /// Atomics touch it (declared `array<atomic<u32>>`).
    pub atomic: bool,
    /// bo2mp: an append/consume buffer (`imm_atomic_alloc` / `_consume`):
    /// D3D's hidden counter is the buffer's last u32 word, which the host
    /// allocates beyond the elements and resets or copies as D3D would
    /// (initial count, `CopyStructureCount`).
    pub counter: bool,
}

#[derive(Clone, Debug)]
pub struct Translation {
    pub stage: Stage,
    pub wgsl: String,
    pub reflection: Reflection,
}

const LET: [char; 4] = ['x', 'y', 'z', 'w'];

fn vt(n: usize, s: &str) -> String {
    if n == 1 {
        s.to_string()
    } else {
        format!("vec{n}<{s}>")
    }
}

fn letters(cs: &[u8]) -> String {
    cs.iter().map(|&c| LET[c as usize & 3]).collect()
}

fn mask_comps(m: u8) -> Vec<u8> {
    (0..4u8).filter(|c| m & (1 << c) != 0).collect()
}

/// Upper-cased semantic, for matching.
fn sem_key(s: &str) -> String {
    s.to_ascii_uppercase()
}

struct Resource {
    dim: TextureDim,
    ret: u32,
    /// Structured buffers: element stride in bytes (raw: 0).
    stride: u32,
}

struct Uav {
    kind: UavKind,
    ret: u32,
    atomic: bool,
    loaded: bool,
    counter: bool,
}

struct Em<'a> {
    prog: &'a Program,
    sig: &'a Container<'a>,
    opts: &'a Options,
    out: String,
    body: String,
    depth: usize,
    refl: Reflection,
    temps: u32,
    indexable: Vec<(u32, u32)>,
    resources: BTreeMap<u32, Resource>,
    compare_textures: Vec<u32>,
    samplers: BTreeMap<u32, bool>,
    cbuffers: BTreeMap<u32, u32>,
    icb: Option<Vec<u32>>,
    /// Pixel inputs declared: register → (mask, interpolation, system value).
    inputs: BTreeMap<u32, (u8, u32, u32)>,
    /// Outputs declared: register → (mask, system value).
    outputs: BTreeMap<u32, (u8, u32)>,
    uses_prim_id: bool,
    uses_coverage: bool,
    writes_depth: bool,
    thread_group: [u32; 3],
    uavs: BTreeMap<u32, Uav>,
    /// Group-shared memory: register → (u32 words, stride bytes (raw 0), atomic).
    tgsm: BTreeMap<u32, (u32, u32, bool)>,
    thread_ids: [bool; 4],
    /// Open `switch`es: (case labels waiting for their body, a case body
    /// open, a default seen).
    switches: Vec<(Vec<String>, bool, bool)>,
}

type R<T> = Result<T, TranslateError>;

impl Em<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.depth + 1 {
            self.body.push_str("    ");
        }
        self.body.push_str(s);
        self.body.push('\n');
    }

    fn idx(&self, i: &Index) -> R<String> {
        Ok(match i {
            Index::Imm(v) => format!("{v}u"),
            Index::Rel(v, o) => {
                let r = self.raw(o, &[o.comp(0)])?;
                if *v == 0 {
                    format!("({r})")
                } else {
                    format!("({r} + {v}u)")
                }
            }
        })
    }

    fn reg_of(o: &Operand) -> R<u32> {
        o.reg().ok_or(TranslateError::UnsupportedOperand(o.ty))
    }

    /// The vec4<u32> expression an operand names (no swizzle).
    fn base(&self, o: &Operand) -> R<String> {
        Ok(match o.ty {
            ot::TEMP => format!("r{}", Self::reg_of(o)?),
            ot::INPUT => format!("v{}", Self::reg_of(o)?),
            ot::OUTPUT => format!("o{}", Self::reg_of(o)?),
            ot::INDEXABLE_TEMP => {
                let i = o
                    .index
                    .get(1)
                    .ok_or(TranslateError::Malformed("x# index"))?;
                format!("x{}[{}]", Self::reg_of(o)?, self.idx(i)?)
            }
            ot::CONSTANT_BUFFER => {
                let i = o
                    .index
                    .get(1)
                    .ok_or(TranslateError::Malformed("cb index"))?;
                format!("cb{}.d[{}]", Self::reg_of(o)?, self.idx(i)?)
            }
            ot::IMMEDIATE_CONSTANT_BUFFER => {
                let i = o
                    .index
                    .first()
                    .ok_or(TranslateError::Malformed("icb index"))?;
                format!("icb[{}]", self.idx(i)?)
            }
            ot::INPUT_PRIMITIVEID => "vPrim".into(),
            ot::THREAD_ID => "vThreadID".into(),
            ot::THREAD_GROUP_ID => "vThreadGroupID".into(),
            ot::THREAD_ID_IN_GROUP => "vThreadIDInGroup".into(),
            ot::THREAD_ID_IN_GROUP_FLATTENED => "vThreadIDInGroupFlattened".into(),
            ot::INPUT_COVERAGE_MASK => "vCoverage".into(),
            t => return Err(TranslateError::UnsupportedOperand(t)),
        })
    }

    /// Raw u32 components `cs` of a source.
    fn raw(&self, o: &Operand, cs: &[u8]) -> R<String> {
        if o.ty == ot::IMM32 {
            let vals: Vec<String> = cs
                .iter()
                .map(|&c| format!("{}u", o.imm[c as usize & 3]))
                .collect();
            return Ok(if cs.len() == 1 {
                vals[0].clone()
            } else {
                format!("vec{}<u32>({})", cs.len(), vals.join(", "))
            });
        }
        Ok(format!("{}.{}", self.base(o)?, letters(cs)))
    }

    /// Source components for destination components `dc`.
    fn cs(o: &Operand, dc: &[u8]) -> Vec<u8> {
        dc.iter().map(|&c| o.comp(c as usize)).collect()
    }

    fn f(&self, o: &Operand, dc: &[u8]) -> R<String> {
        let cs = Self::cs(o, dc);
        let mut e = format!("bitcast<{}>({})", vt(cs.len(), "f32"), self.raw(o, &cs)?);
        if o.abs {
            e = format!("abs({e})");
        }
        if o.neg {
            e = format!("(-{e})");
        }
        Ok(e)
    }

    fn i(&self, o: &Operand, dc: &[u8]) -> R<String> {
        let cs = Self::cs(o, dc);
        let mut e = format!("bitcast<{}>({})", vt(cs.len(), "i32"), self.raw(o, &cs)?);
        if o.abs {
            e = format!("abs({e})");
        }
        if o.neg {
            e = format!("(-{e})");
        }
        Ok(e)
    }

    fn u(&self, o: &Operand, dc: &[u8]) -> R<String> {
        if o.neg || o.abs {
            return Ok(format!(
                "bitcast<{}>({})",
                vt(dc.len(), "u32"),
                self.i(o, dc)?
            ));
        }
        let cs = Self::cs(o, dc);
        self.raw(o, &cs)
    }

    /// Fixed components 0..n of a source (dot products, coordinates).
    fn f_n(&self, o: &Operand, n: usize) -> R<String> {
        let dc: Vec<u8> = (0..n as u8).collect();
        self.f(o, &dc)
    }
    fn i_n(&self, o: &Operand, n: usize) -> R<String> {
        let dc: Vec<u8> = (0..n as u8).collect();
        self.i(o, &dc)
    }

    /// Writes `value` (n components of type `ty`, or 1 broadcast) to `dst`.
    fn write(&mut self, dst: &Operand, value: &str, n: usize, ty: &str, sat: bool) -> R<()> {
        if dst.ty == ot::NULL {
            return Ok(());
        }
        let dc = mask_comps(dst.mask());
        let v = value.to_string();
        // `_sat` as D3D does it: NaN saturates to 0 (WGSL's `saturate` of a
        // NaN is not defined: on Vulkan it stayed NaN; BO3's sky program
        // saturates a NaN when the map has no fog and drew black).
        let sat = sat && ty == "f32";
        let sat_of = |c: &str| if sat { format!("select(0.0, saturate({c}), ({c}) == ({c}))") } else { c.to_string() };
        if matches!(
            dst.ty,
            ot::OUTPUT_DEPTH | ot::OUTPUT_DEPTH_GE | ot::OUTPUT_DEPTH_LE
        ) {
            self.writes_depth = true;
            let s = if n == 1 { v } else { format!("({v}).x") };
            self.line(&format!("oDepth = bitcast<u32>({});", sat_of(&s)));
            return Ok(());
        }
        if dst.ty == ot::OUTPUT_COVERAGE_MASK {
            return Ok(());
        }
        let base = self.base(dst)?;
        self.line(&format!("{{ let t = {v};"));
        for (k, &c) in dc.iter().enumerate() {
            let src = if n == 1 {
                "t".to_string()
            } else {
                format!("t.{}", LET[k])
            };
            let s = format!("    {base}.{} = bitcast<u32>({});", LET[c as usize], sat_of(&src));
            self.line(&s);
        }
        self.line("}");
        Ok(())
    }

    fn cond(&self, o: &Operand, nonzero: bool) -> R<String> {
        let r = self.raw(o, &[o.comp(0)])?;
        Ok(if nonzero {
            format!("({r} != 0u)")
        } else {
            format!("({r} == 0u)")
        })
    }

    fn texture_name(&self, o: &Operand) -> R<(u32, String)> {
        let slot = Self::reg_of(o)?;
        Ok((slot, format!("t{slot}")))
    }

    fn resource(&self, slot: u32) -> R<&Resource> {
        self.resources.get(&slot).ok_or(TranslateError::Malformed(
            "texture used without a declaration",
        ))
    }

    fn is_depth(&self, slot: u32) -> bool {
        self.compare_textures.contains(&slot)
    }

    /// (coordinate expression, array layer expression).
    fn coords(&self, dim: TextureDim, c: &Operand) -> R<(String, Option<String>)> {
        Ok(match dim {
            TextureDim::D1 => (self.f_n(c, 1)?, None),
            TextureDim::D2 | TextureDim::D2Ms => (self.f_n(c, 2)?, None),
            TextureDim::D2Array => (
                self.f_n(c, 2)?,
                Some(format!(
                    "i32(round(bitcast<f32>({})))",
                    self.raw(c, &[c.comp(2)])?
                )),
            ),
            TextureDim::D3 | TextureDim::Cube => (self.f_n(c, 3)?, None),
            TextureDim::CubeArray => (
                self.f_n(c, 3)?,
                Some(format!(
                    "i32(round(bitcast<f32>({})))",
                    self.raw(c, &[c.comp(3)])?
                )),
            ),
            _ => return Err(TranslateError::Malformed("sample from a buffer")),
        })
    }

    fn offset(&self, ins: &Instr, dim: TextureDim) -> Option<String> {
        if ins.offsets == [0; 3] {
            return None;
        }
        let o = ins.offsets;
        match dim {
            TextureDim::D2 | TextureDim::D2Array => Some(format!("vec2<i32>({}, {})", o[0], o[1])),
            TextureDim::D3 => Some(format!("vec3<i32>({}, {}, {})", o[0], o[1], o[2])),
            _ => None,
        }
    }

    /// Writes a texture result (vec4 of the texture's type) through the
    /// resource operand's swizzle.
    fn write_tex(&mut self, ins: &Instr, res_op: &Operand, call: &str, ty: &str) -> R<()> {
        let dst = &ins.operands[0];
        let dc = mask_comps(dst.mask());
        let cs = Self::cs(res_op, &dc);
        let v = format!("({call}).{}", letters(&cs));
        self.write(dst, &v, cs.len(), ty, ins.saturate)
    }

    fn tex_ret(&self, slot: u32) -> &'static str {
        if self.is_depth(slot) {
            return "f32";
        }
        match self.resources.get(&slot).map(|r| r.ret & 0xf) {
            Some(3) => "i32",
            Some(4) => "u32",
            _ => "f32",
        }
    }

    fn sample(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let (slot, t) = self.texture_name(&o[2])?;
        let s = format!("s{}", Self::reg_of(&o[3])?);
        let dim = self.resource(slot)?.dim;
        let (c, layer) = self.coords(dim, &o[1])?;
        let layer = layer.map_or(String::new(), |l| format!(", {l}"));
        let off = self
            .offset(ins, dim)
            .map_or(String::new(), |v| format!(", {v}"));
        let depth = self.is_depth(slot);
        let lod_int = |e: String| if depth { format!("i32({e})") } else { e };
        let call = match ins.op {
            op::SAMPLE => format!("textureSample({t}, {s}, {c}{layer}{off})"),
            op::SAMPLE_L => {
                let l = lod_int(self.f(&o[4], &[0])?);
                format!("textureSampleLevel({t}, {s}, {c}{layer}, {l}{off})")
            }
            op::SAMPLE_B => {
                let b = self.f(&o[4], &[0])?;
                format!("textureSampleBias({t}, {s}, {c}{layer}, {b}{off})")
            }
            op::SAMPLE_D => {
                let n = match dim {
                    TextureDim::D1 => 1,
                    TextureDim::D3 | TextureDim::Cube | TextureDim::CubeArray => 3,
                    _ => 2,
                };
                let dx = self.f_n(&o[4], n)?;
                let dy = self.f_n(&o[5], n)?;
                format!("textureSampleGrad({t}, {s}, {c}{layer}, {dx}, {dy}{off})")
            }
            op::SAMPLE_C => {
                let r = self.f(&o[4], &[0])?;
                format!("textureSampleCompare({t}, {s}, {c}{layer}, {r}{off})")
            }
            op::SAMPLE_C_LZ => {
                let r = self.f(&o[4], &[0])?;
                format!("textureSampleCompareLevel({t}, {s}, {c}{layer}, {r}{off})")
            }
            _ => unreachable!(),
        };
        let compare = matches!(ins.op, op::SAMPLE_C | op::SAMPLE_C_LZ);
        let call = if compare {
            format!("vec4<f32>({call})")
        } else if depth {
            format!("vec4<f32>({call}, 0.0, 0.0, 1.0)")
        } else {
            call
        };
        let ty = if compare || depth {
            "f32"
        } else {
            self.tex_ret(slot)
        };
        let res = o[2].clone();
        self.write_tex(ins, &res, &call, ty)
    }

    /// `lod` (CalculateLevelOfDetail): x the clamped level, y the unclamped
    /// one, from the coordinate's screen derivatives in texels (WGSL has no
    /// query; this is the standard isotropic estimate).
    fn lod(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let (slot, t) = self.texture_name(&o[2])?;
        let dim = self.resource(slot)?.dim;
        let (c, _) = self.coords(dim, &o[1])?;
        let (size, coord) = match dim {
            TextureDim::D1 => (format!("f32(textureDimensions({t}))"), format!("({c})")),
            TextureDim::D2 | TextureDim::D2Array => {
                (format!("vec2<f32>(textureDimensions({t}))"), format!("({c})"))
            }
            TextureDim::D3 => (format!("vec3<f32>(textureDimensions({t}))"), format!("({c})")),
            // A cube: the direction scaled by the face size.
            TextureDim::Cube | TextureDim::CubeArray => (
                format!("f32(textureDimensions({t}).x)"),
                format!("normalize({c})"),
            ),
            _ => return Err(TranslateError::UnsupportedResource { slot, dim: 0 }),
        };
        let levels = format!("f32(textureNumLevels({t}))");
        let dx = format!("dpdx({coord} * {size})");
        let dy = format!("dpdy({coord} * {size})");
        let raw = format!("(0.5 * log2(max(max(dot({dx}, {dx}), dot({dy}, {dy})), 1e-20)))");
        let call = format!(
            "vec4<f32>(clamp({raw}, 0.0, {levels} - 1.0), {raw}, 0.0, 0.0)"
        );
        let res = o[2].clone();
        self.write_tex(ins, &res, &call, "f32")
    }

    fn gather(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let (slot, t) = self.texture_name(&o[2])?;
        let s = format!("s{}", Self::reg_of(&o[3])?);
        let dim = self.resource(slot)?.dim;
        let (c, layer) = self.coords(dim, &o[1])?;
        let layer = layer.map_or(String::new(), |l| format!(", {l}"));
        let off = self
            .offset(ins, dim)
            .map_or(String::new(), |v| format!(", {v}"));
        let call = if ins.op == op::GATHER4_C {
            let r = self.f(&o[4], &[0])?;
            format!("textureGatherCompare({t}, {s}, {c}{layer}, {r}{off})")
        } else if self.is_depth(slot) {
            format!("textureGather({t}, {s}, {c}{layer}{off})")
        } else {
            let comp = match o[3].select {
                Select::Scalar(k) => k,
                Select::Swizzle(sw) => sw[0],
                _ => 0,
            };
            format!("textureGather({comp}, {t}, {s}, {c}{layer}{off})")
        };
        let ty = if ins.op == op::GATHER4_C || self.is_depth(slot) {
            "f32"
        } else {
            self.tex_ret(slot)
        };
        let res = o[2].clone();
        self.write_tex(ins, &res, &call, ty)
    }

    /// Words an element of the typed buffer SRV at `slot` (`Options::srv_buffer_words`).
    fn srv_buffer_words(&self, slot: u32) -> u32 {
        self.opts.srv_buffer_words.iter().find(|(s, _)| *s == slot).map_or(1, |(_, n)| (*n).clamp(1, 4))
    }

    fn load(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let (slot, t) = self.texture_name(&o[2])?;
        let dim = self.resource(slot)?.dim;
        let a = &o[1];
        let mip = format!("bitcast<i32>({})", self.raw(a, &[a.comp(3)])?);
        let off = ins.offsets;
        let add2 = if off == [0; 3] {
            String::new()
        } else {
            format!(" + vec2<i32>({}, {})", off[0], off[1])
        };
        let call = match dim {
            TextureDim::D2 => format!("textureLoad({t}, {}{add2}, {mip})", self.i_n(a, 2)?),
            TextureDim::D2Array => format!(
                "textureLoad({t}, {}{add2}, bitcast<i32>({}), {mip})",
                self.i_n(a, 2)?,
                self.raw(a, &[a.comp(2)])?
            ),
            TextureDim::D3 => format!("textureLoad({t}, {}, {mip})", self.i_n(a, 3)?),
            TextureDim::D1 => format!("textureLoad({t}, {}, {mip})", self.i(a, &[0])?),
            TextureDim::Buffer => {
                let words = self.srv_buffer_words(slot);
                let at = format!("(bitcast<u32>({}) * {words}u)", self.raw(a, &[a.comp(0)])?);
                let one = if self.tex_ret(slot) == "f32" { "0x3f800000u" } else { "1u" };
                let w: Vec<String> = (0..4u32)
                    .map(|k| if k < words { format!("{t}[{at} + {k}u]") } else if k == 3 { String::from(one) } else { String::from("0u") })
                    .collect();
                let bits = format!("vec4<u32>({})", w.join(", "));
                match self.tex_ret(slot) {
                    "u32" => bits,
                    r => format!("bitcast<vec4<{r}>>({bits})"),
                }
            }
            _ => return Err(TranslateError::UnsupportedResource { slot, dim: 0 }),
        };
        let depth = self.is_depth(slot);
        let call = if depth {
            format!("vec4<f32>({call}, 0.0, 0.0, 1.0)")
        } else {
            call
        };
        let ty = if depth { "f32" } else { self.tex_ret(slot) };
        let res = o[2].clone();
        self.write_tex(ins, &res, &call, ty)
    }

    fn resinfo(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        if o[2].ty == ot::UAV {
            let slot = Self::reg_of(&o[2])?;
            let dim = match self.uavs.get(&slot).map(|u| u.kind) {
                Some(UavKind::Typed(d)) => d,
                _ => return Err(TranslateError::Malformed("resinfo on a buffer uav")),
            };
            let t = format!("u{slot}");
            let dims = match dim {
                TextureDim::D1 | TextureDim::Buffer => format!("vec4<u32>(textureDimensions({t}), 0u, 0u, 1u)"),
                TextureDim::D2 => format!("vec4<u32>(textureDimensions({t}), 0u, 1u)"),
                TextureDim::D2Array => format!("vec4<u32>(textureDimensions({t}), textureNumLayers({t}), 1u)"),
                _ => format!("vec4<u32>(textureDimensions({t}), 1u)"),
            };
            let (call, ty) = match (ins.token >> 11) & 3 {
                2 => (dims, "u32"),
                1 => (format!("(vec4<f32>(1.0 / vec3<f32>(({dims}).xyz), 1.0))"), "f32"),
                _ => (format!("vec4<f32>({dims})"), "f32"),
            };
            let res = o[2].clone();
            return self.write_tex(ins, &res, &call, ty);
        }
        let (slot, t) = self.texture_name(&o[2])?;
        let dim = self.resource(slot)?.dim;
        let lvl = self.u(&o[1], &[0])?;
        let dims = match dim {
            TextureDim::D1 => {
                format!("vec4<u32>(textureDimensions({t}, {lvl}), 0u, 0u, textureNumLevels({t}))")
            }
            TextureDim::D2 | TextureDim::Cube => {
                format!("vec4<u32>(textureDimensions({t}, {lvl}), 0u, textureNumLevels({t}))")
            }
            TextureDim::D2Array | TextureDim::CubeArray => format!(
                "vec4<u32>(textureDimensions({t}, {lvl}), textureNumLayers({t}), textureNumLevels({t}))"
            ),
            TextureDim::D3 => {
                format!("vec4<u32>(textureDimensions({t}, {lvl}), textureNumLevels({t}))")
            }
            _ => return Err(TranslateError::UnsupportedResource { slot, dim: 0 }),
        };
        // Return type: 0 float, 1 1/float (xyz), 2 uint.
        let (call, ty) = match (ins.token >> 11) & 3 {
            2 => (dims, "u32"),
            1 => (
                format!("(vec4<f32>(1.0 / vec3<f32>(({dims}).xyz), f32(({dims}).w)))"),
                "f32",
            ),
            _ => (format!("vec4<f32>({dims})"), "f32"),
        };
        let res = o[2].clone();
        self.write_tex(ins, &res, &call, ty)
    }

    /// Starts the body of the case labels waiting in the innermost switch.
    fn open_case(&mut self) {
        let Some(sw) = self.switches.last_mut() else { return };
        if sw.0.is_empty() {
            return;
        }
        let labels = core::mem::take(&mut sw.0);
        sw.1 = true;
        let head = if labels.iter().any(|l| l == "default") {
            let rest: Vec<String> = labels.into_iter().filter(|l| l != "default").collect();
            if rest.is_empty() {
                "default".to_string()
            } else {
                format!("case {}, default", rest.join(", "))
            }
        } else {
            format!("case {}", labels.join(", "))
        };
        self.line(&format!("{head}: {{"));
        self.depth += 1;
    }

    fn uav_entry(&mut self, slot: u32, kind: UavKind, ret: u32) {
        let e = self.uavs.entry(slot).or_insert(Uav {
            kind,
            ret,
            atomic: false,
            loaded: false,
            counter: false,
        });
        e.kind = kind;
        e.ret = ret;
    }

    /// A buffer operand (t#, u#, g#): its WGSL name, whether its words are
    /// atomics, and its stride (0 = byte addressed).
    fn buffer(&self, o: &Operand) -> R<(String, bool, u32)> {
        let r = Self::reg_of(o)?;
        match o.ty {
            ot::RESOURCE => {
                let res = self.resource(r)?;
                Ok((format!("t{r}"), false, res.stride))
            }
            ot::UAV => {
                let u = self.uavs.get(&r).ok_or(TranslateError::Malformed("uav used without a declaration"))?;
                let stride = match u.kind {
                    UavKind::Structured(st) => st,
                    _ => 0,
                };
                Ok((format!("u{r}"), u.atomic, stride))
            }
            ot::TGSM => {
                let g = self.tgsm.get(&r).ok_or(TranslateError::Malformed("g# used without a declaration"))?;
                Ok((format!("g{r}"), g.2, g.1))
            }
            t => Err(TranslateError::UnsupportedOperand(t)),
        }
    }

    fn word_load(name: &str, atomic: bool, at: &str) -> String {
        if atomic {
            format!("atomicLoad(&{name}[{at}])")
        } else {
            format!("{name}[{at}]")
        }
    }

    /// The first word an element access touches: raw `(byte >> 2)`,
    /// structured `index * stride / 4 + (byte >> 2)`.
    fn word_at(&self, stride: u32, index: Option<&Operand>, byte: &Operand) -> R<String> {
        let b = self.u(byte, &[0])?;
        Ok(match index {
            Some(i) if stride > 0 => format!("({} * {}u + ({b} >> 2u))", self.u(i, &[0])?, stride / 4),
            _ => format!("({b} >> 2u)"),
        })
    }

    /// Audit line: counts an access of words `at ..= at + last` to `name`
    /// that falls outside the buffer.
    fn audit(&mut self, name: &str, at: &str, last: u32, kind: u32) {
        if !self.opts.bounds_audit || name.starts_with('g') {
            return;
        }
        let reg: u32 = name[1..].parse().unwrap_or(0);
        let idx = (reg + if name.starts_with('u') { 64 } else { 0 }) * 3 + kind;
        self.line(&format!("if (({at}) + {last}u >= arrayLength(&{name})) {{ atomicAdd(&t7_oob[T7_OOB_BASE + {idx}u], 1u); }}"));
    }

    fn ld_raw(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let structured = ins.op == op::LD_STRUCTURED;
        let res_op = if structured { &o[3] } else { &o[2] };
        let (name, atomic, stride) = self.buffer(res_op)?;
        let at = if structured {
            self.word_at(stride, Some(&o[1]), &o[2])?
        } else {
            self.word_at(0, None, &o[1])?
        };
        let dst = &o[0];
        let dc = mask_comps(dst.mask());
        let cs = Self::cs(res_op, &dc);
        let last = cs.iter().copied().max().unwrap_or(0) as u32;
        self.audit(&name, &at, last, 0);
        let parts: Vec<String> = cs
            .iter()
            .map(|&c| Self::word_load(&name, atomic, &format!("{at} + {c}u")))
            .collect();
        let v = if parts.len() == 1 {
            parts[0].clone()
        } else {
            format!("vec{}<u32>({})", parts.len(), parts.join(", "))
        };
        let dst = dst.clone();
        self.write(&dst, &v, cs.len(), "u32", false)
    }

    fn store_raw(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let structured = ins.op == op::STORE_STRUCTURED;
        let (name, atomic, stride) = self.buffer(&o[0])?;
        let (at, val) = if structured {
            (self.word_at(stride, Some(&o[1]), &o[2])?, &o[3])
        } else {
            (self.word_at(0, None, &o[1])?, &o[2])
        };
        let last = mask_comps(o[0].mask()).into_iter().max().unwrap_or(0) as u32;
        self.audit(&name, &at, last, 1);
        self.line(&format!("{{ let a = {at};"));
        for (k, c) in mask_comps(o[0].mask()).into_iter().enumerate() {
            let v = self.raw(val, &[val.comp(c as usize)])?;
            let _ = k;
            if atomic {
                self.line(&format!("    atomicStore(&{name}[a + {c}u], {v});"));
            } else {
                self.line(&format!("    {name}[a + {c}u] = {v};"));
            }
        }
        self.line("}");
        Ok(())
    }

    /// An atomic's word: raw (address.x >> 2) or structured (address.x
    /// index, address.y byte offset).
    fn atomic_word(&self, stride: u32, addr: &Operand) -> R<String> {
        let x = self.raw(addr, &[addr.comp(0)])?;
        Ok(if stride > 0 {
            let y = self.raw(addr, &[addr.comp(1)])?;
            format!("({x} * {}u + ({y} >> 2u))", stride / 4)
        } else {
            format!("({x} >> 2u)")
        })
    }

    fn atomic(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let imm = ins.op >= op::IMM_ATOMIC_ALLOC;
        if matches!(ins.op, op::IMM_ATOMIC_ALLOC | op::IMM_ATOMIC_CONSUME) {
            // The UAV's hidden counter: its last word (`UavRef::counter`).
            // alloc returns the count before the increment, consume the
            // count after the decrement.
            if o.len() < 2 {
                return Err(TranslateError::Malformed("atomic alloc operands"));
            }
            let (name, _, _) = self.buffer(&o[1])?;
            let p = format!("&{name}[arrayLength(&{name}) - 1u]");
            let call = if ins.op == op::IMM_ATOMIC_ALLOC { format!("atomicAdd({p}, 1u)") } else { format!("(atomicSub({p}, 1u) - 1u)") };
            let d = o[0].clone();
            return self.write(&d, &call, 1, "u32", false);
        }
        if o.len() < if imm { 4 } else { 3 } {
            return Err(TranslateError::Malformed("atomic operands"));
        }
        let (res, addr, v0) = if imm { (&o[1], &o[2], 3) } else { (&o[0], &o[1], 2) };
        let (name, _, stride) = self.buffer(res)?;
        if res.ty == ot::UAV
            && let Some(UavKind::Typed(_)) = self.uavs.get(&Self::reg_of(res)?).map(|u| u.kind)
        {
            return Err(TranslateError::UnsupportedOpcode(ins.op));
        }
        let at = self.atomic_word(stride, addr)?;
        self.audit(&name, &at, 0, 2);
        let val = |k: usize| self.raw(&o[k], &[o[k].comp(0)]);
        let signed = matches!(
            ins.op,
            op::ATOMIC_IMAX | op::ATOMIC_IMIN | op::IMM_ATOMIC_IMAX | op::IMM_ATOMIC_IMIN
        );
        let p = format!("&{name}[{at}]");
        let call = match ins.op {
            op::ATOMIC_AND | op::IMM_ATOMIC_AND => format!("atomicAnd({p}, {})", val(v0)?),
            op::ATOMIC_OR | op::IMM_ATOMIC_OR => format!("atomicOr({p}, {})", val(v0)?),
            op::ATOMIC_XOR | op::IMM_ATOMIC_XOR => format!("atomicXor({p}, {})", val(v0)?),
            op::ATOMIC_IADD | op::IMM_ATOMIC_IADD => format!("atomicAdd({p}, {})", val(v0)?),
            op::ATOMIC_UMAX | op::IMM_ATOMIC_UMAX => format!("atomicMax({p}, {})", val(v0)?),
            op::ATOMIC_UMIN | op::IMM_ATOMIC_UMIN => format!("atomicMin({p}, {})", val(v0)?),
            op::IMM_ATOMIC_EXCH => format!("atomicExchange({p}, {})", val(v0)?),
            op::ATOMIC_CMP_STORE | op::IMM_ATOMIC_CMP_EXCH => {
                format!("atomicCompareExchangeWeak({p}, {}, {}).old_value", val(v0)?, val(v0 + 1)?)
            }
            _ if signed => {
                // (Signed max/min on u32 words: a compare-exchange loop.)
                let f = if matches!(ins.op, op::ATOMIC_IMAX | op::IMM_ATOMIC_IMAX) { "max" } else { "min" };
                let v = val(v0)?;
                self.line(&format!("{{ var old = atomicLoad({p}); loop {{ let n = bitcast<u32>({f}(bitcast<i32>(old), bitcast<i32>({v}))); let r = atomicCompareExchangeWeak({p}, old, n); if r.exchanged {{ break; }} old = r.old_value; }}"));
                if imm {
                    let d = o[0].clone();
                    self.write(&d, "old", 1, "u32", false)?;
                }
                self.line("}");
                return Ok(());
            }
            other => return Err(TranslateError::UnsupportedOpcode(other)),
        };
        if imm && o[0].ty != ot::NULL {
            let d = o[0].clone();
            self.write(&d, &call, 1, "u32", false)
        } else {
            self.line(&format!("{call};"));
            Ok(())
        }
    }

    fn uav_coords(&self, dim: TextureDim, c: &Operand) -> R<String> {
        Ok(match dim {
            TextureDim::D1 | TextureDim::Buffer => self.u(c, &[0])?,
            TextureDim::D2 => format!("vec2<u32>({})", self.u(c, &[0, 1])?),
            TextureDim::D2Array => format!("vec2<u32>({}), {}", self.u(c, &[0, 1])?, self.u(c, &[2])?),
            TextureDim::D3 => format!("vec3<u32>({})", self.u(c, &[0, 1, 2])?),
            _ => return Err(TranslateError::Malformed("uav dimension")),
        })
    }

    fn uav_typed(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let load = ins.op == op::LD_UAV_TYPED;
        let u_op = if load { &o[2] } else { &o[0] };
        let slot = Self::reg_of(u_op)?;
        let (dim, ret) = match self.uavs.get(&slot) {
            Some(Uav { kind: UavKind::Typed(d), ret, .. }) => (*d, *ret),
            _ => return Err(TranslateError::Malformed("typed uav access")),
        };
        let ty = match ret & 0xf {
            3 => "i32",
            4 => "u32",
            _ => "f32",
        };
        if dim == TextureDim::Buffer && self.opts.typed_buffer_uavs_as_words {
            let c = self.uav_coords(dim, &o[1])?;
            if load {
                let call = format!("vec4<{ty}>(bitcast<{ty}>(u{slot}[{c}]), {z}, {z}, {z})", z = if ty == "f32" { "0.0" } else if ty == "i32" { "0i" } else { "0u" });
                let res = o[2].clone();
                return self.write_tex(ins, &res, &call, ty);
            }
            let v = self.u(&o[2], &[0])?;
            self.line(&format!("u{slot}[{c}] = {v};"));
            return Ok(());
        }
        if load {
            let c = self.uav_coords(dim, &o[1])?;
            let call = format!("textureLoad(u{slot}, {c})");
            let res = o[2].clone();
            self.write_tex(ins, &res, &call, ty)
        } else {
            let c = self.uav_coords(dim, &o[1])?;
            let v = match ty {
                "f32" => self.f_n(&o[2], 4)?,
                "i32" => self.i_n(&o[2], 4)?,
                _ => format!("vec4<u32>({})", self.u(&o[2], &[0, 1, 2, 3])?),
            };
            self.line(&format!("textureStore(u{slot}, {c}, {v});"));
            Ok(())
        }
    }

    /// Per-component scalar calls (bit-field ops take scalar offsets).
    fn per_comp(&self, dc: &[u8], f: impl Fn(u8) -> R<String>, ty: &str) -> R<String> {
        let parts = dc.iter().map(|&c| f(c)).collect::<R<Vec<_>>>()?;
        Ok(if parts.len() == 1 {
            parts[0].clone()
        } else {
            format!("{}({})", vt(parts.len(), ty), parts.join(", "))
        })
    }

    fn instr(&mut self, ins: &Instr) -> R<()> {
        let o = &ins.operands;
        let sat = ins.saturate;
        let dc = o.first().map_or(Vec::new(), |d| mask_comps(d.mask()));
        let n = dc.len().max(1);
        let fl = |s: &Self, k: usize| s.f(&o[k], &dc);
        let il = |s: &Self, k: usize| s.i(&o[k], &dc);
        let ul = |s: &Self, k: usize| s.u(&o[k], &dc);
        let cmp = |e: String| {
            if n == 1 {
                format!("select(0u, 0xffffffffu, {e})")
            } else {
                format!("select(vec{n}<u32>(0u), vec{n}<u32>(0xffffffffu), {e})")
            }
        };
        macro_rules! wf {
            ($e:expr) => {{
                let v = $e;
                let d = o[0].clone();
                self.write(&d, &v, n, "f32", sat)?
            }};
        }
        macro_rules! wi {
            ($e:expr) => {{
                let v = $e;
                let d = o[0].clone();
                self.write(&d, &v, n, "i32", false)?
            }};
        }
        macro_rules! wu {
            ($e:expr) => {{
                let v = $e;
                let d = o[0].clone();
                self.write(&d, &v, n, "u32", false)?
            }};
        }
        let us = |k: usize| vt(n, "u32") + &format!("({k}u)");
        if !matches!(ins.op, op::CASE | op::DEFAULT | op::ENDSWITCH) {
            self.open_case();
        }
        match ins.op {
            op::ADD => wf!(format!("({} + {})", fl(self, 1)?, fl(self, 2)?)),
            op::MUL => wf!(format!("({} * {})", fl(self, 1)?, fl(self, 2)?)),
            op::DIV => wf!(format!("({} / {})", fl(self, 1)?, fl(self, 2)?)),
            op::MAD => wf!(format!(
                "({} * {} + {})",
                fl(self, 1)?,
                fl(self, 2)?,
                fl(self, 3)?
            )),
            op::MIN => wf!(format!("min({}, {})", fl(self, 1)?, fl(self, 2)?)),
            op::MAX => wf!(format!("max({}, {})", fl(self, 1)?, fl(self, 2)?)),
            op::FRC => wf!(format!("fract({})", fl(self, 1)?)),
            op::EXP => wf!(format!("exp2({})", fl(self, 1)?)),
            op::LOG => wf!(format!("log2({})", fl(self, 1)?)),
            op::RSQ => wf!(format!("inverseSqrt({})", fl(self, 1)?)),
            op::SQRT => wf!(format!("sqrt({})", fl(self, 1)?)),
            op::RCP => wf!(format!("(1.0 / {})", fl(self, 1)?)),
            op::ROUND_NE => wf!(format!("round({})", fl(self, 1)?)),
            op::ROUND_NI => wf!(format!("floor({})", fl(self, 1)?)),
            op::ROUND_PI => wf!(format!("ceil({})", fl(self, 1)?)),
            op::ROUND_Z => wf!(format!("trunc({})", fl(self, 1)?)),
            op::DERIV_RTX | op::DERIV_RTX_COARSE => wf!(format!("dpdxCoarse({})", fl(self, 1)?)),
            op::DERIV_RTY | op::DERIV_RTY_COARSE => wf!(format!("dpdyCoarse({})", fl(self, 1)?)),
            op::DERIV_RTX_FINE => wf!(format!("dpdxFine({})", fl(self, 1)?)),
            op::DERIV_RTY_FINE => wf!(format!("dpdyFine({})", fl(self, 1)?)),
            op::DP2 | op::DP3 | op::DP4 => {
                let k = (ins.op - op::DP2 + 2) as usize;
                let v = format!("dot({}, {})", self.f_n(&o[1], k)?, self.f_n(&o[2], k)?);
                let d = o[0].clone();
                self.write(&d, &v, 1, "f32", sat)?;
            }
            op::SINCOS => {
                // (sin, cos, a): both from `a` as it was before either is
                // written (the sine's destination may be `a`).
                self.line("{");
                for k in 0..2usize {
                    if o[k].ty != ot::NULL {
                        let d = mask_comps(o[k].mask());
                        let a = self.f(&o[2], &d)?;
                        self.line(&format!("let sincos_a{k} = {a};"));
                    }
                }
                for (k, f) in [(0usize, "sin"), (1, "cos")] {
                    if o[k].ty != ot::NULL {
                        let d = mask_comps(o[k].mask());
                        let dd = o[k].clone();
                        self.write(&dd, &format!("{f}(sincos_a{k})"), d.len(), "f32", sat)?;
                    }
                }
                self.line("}");
            }
            op::EQ => wu!(cmp(format!("({} == {})", fl(self, 1)?, fl(self, 2)?))),
            op::NE => wu!(cmp(format!("({} != {})", fl(self, 1)?, fl(self, 2)?))),
            op::LT => wu!(cmp(format!("({} < {})", fl(self, 1)?, fl(self, 2)?))),
            op::GE => wu!(cmp(format!("({} >= {})", fl(self, 1)?, fl(self, 2)?))),
            op::IEQ => wu!(cmp(format!("({} == {})", il(self, 1)?, il(self, 2)?))),
            op::INE => wu!(cmp(format!("({} != {})", il(self, 1)?, il(self, 2)?))),
            op::ILT => wu!(cmp(format!("({} < {})", il(self, 1)?, il(self, 2)?))),
            op::IGE => wu!(cmp(format!("({} >= {})", il(self, 1)?, il(self, 2)?))),
            op::ULT => wu!(cmp(format!("({} < {})", ul(self, 1)?, ul(self, 2)?))),
            op::UGE => wu!(cmp(format!("({} >= {})", ul(self, 1)?, ul(self, 2)?))),
            op::MOV => {
                if o[1].neg || o[1].abs || sat {
                    wf!(fl(self, 1)?)
                } else {
                    wu!(ul(self, 1)?)
                }
            }
            op::MOVC => {
                let c = ul(self, 1)?;
                let pick = |s: &Self, k: usize| -> R<String> {
                    if o[k].neg || o[k].abs {
                        Ok(format!("bitcast<{}>({})", vt(n, "u32"), s.f(&o[k], &dc)?))
                    } else {
                        s.u(&o[k], &dc)
                    }
                };
                let v = format!(
                    "select({}, {}, {c} != {})",
                    pick(self, 3)?,
                    pick(self, 2)?,
                    us(0)
                );
                if sat {
                    wf!(format!("bitcast<{}>({v})", vt(n, "f32")))
                } else {
                    wu!(v)
                }
            }
            op::FTOI => wi!(format!("{}({})", vt(n, "i32"), fl(self, 1)?)),
            op::FTOU => wu!(format!("{}({})", vt(n, "u32"), fl(self, 1)?)),
            op::ITOF => wf!(format!("{}({})", vt(n, "f32"), il(self, 1)?)),
            op::UTOF => wf!(format!("{}({})", vt(n, "f32"), ul(self, 1)?)),
            op::AND => wu!(format!("({} & {})", ul(self, 1)?, ul(self, 2)?)),
            op::OR => wu!(format!("({} | {})", ul(self, 1)?, ul(self, 2)?)),
            op::XOR => wu!(format!("({} ^ {})", ul(self, 1)?, ul(self, 2)?)),
            op::NOT => wu!(format!("(~{})", ul(self, 1)?)),
            op::IADD => wi!(format!("({} + {})", il(self, 1)?, il(self, 2)?)),
            op::IMAD => wi!(format!(
                "({} * {} + {})",
                il(self, 1)?,
                il(self, 2)?,
                il(self, 3)?
            )),
            op::UMAD => wu!(format!(
                "({} * {} + {})",
                ul(self, 1)?,
                ul(self, 2)?,
                ul(self, 3)?
            )),
            op::IMAX => wi!(format!("max({}, {})", il(self, 1)?, il(self, 2)?)),
            op::IMIN => wi!(format!("min({}, {})", il(self, 1)?, il(self, 2)?)),
            op::UMAX => wu!(format!("max({}, {})", ul(self, 1)?, ul(self, 2)?)),
            op::UMIN => wu!(format!("min({}, {})", ul(self, 1)?, ul(self, 2)?)),
            op::INEG => wi!(format!("(-{})", il(self, 1)?)),
            op::ISHL => wi!(format!(
                "({} << ({} & {}))",
                il(self, 1)?,
                ul(self, 2)?,
                us(31)
            )),
            op::ISHR => wi!(format!(
                "({} >> ({} & {}))",
                il(self, 1)?,
                ul(self, 2)?,
                us(31)
            )),
            op::USHR => wu!(format!(
                "({} >> ({} & {}))",
                ul(self, 1)?,
                ul(self, 2)?,
                us(31)
            )),
            op::COUNTBITS => wu!(format!("countOneBits({})", ul(self, 1)?)),
            op::BFREV => wu!(format!("reverseBits({})", ul(self, 1)?)),
            op::IMUL | op::UMUL => {
                // (hi, lo, a, b): only the low half.
                if o[0].ty != ot::NULL {
                    return Err(TranslateError::UnsupportedOpcode(ins.op));
                }
                let d = mask_comps(o[1].mask());
                let ty = if ins.op == op::IMUL { "i32" } else { "u32" };
                let (a, b) = if ins.op == op::IMUL {
                    (self.i(&o[2], &d)?, self.i(&o[3], &d)?)
                } else {
                    (self.u(&o[2], &d)?, self.u(&o[3], &d)?)
                };
                let dd = o[1].clone();
                self.write(&dd, &format!("({a} * {b})"), d.len(), ty, false)?;
            }
            op::UDIV => {
                // (quotient, remainder, a, b): both from the operands as
                // they were before either is written (`udiv r0.x, r2.x,
                // r0.y, r0.x` must not divide by the new r0.x). Operands
                // are read into locals first.
                let parts: Vec<(usize, &str, usize)> = [(0usize, "/"), (1, "%")]
                    .into_iter()
                    .filter(|(k, _)| o[*k].ty != ot::NULL)
                    .map(|(k, sym)| (k, sym, mask_comps(o[k].mask()).len()))
                    .collect();
                self.line("{");
                for &(k, _, _) in &parts {
                    let d = mask_comps(o[k].mask());
                    let (a, b) = (self.u(&o[2], &d)?, self.u(&o[3], &d)?);
                    self.line(&format!("let udiv_a{k} = {a}; let udiv_b{k} = {b};"));
                }
                for &(k, sym, n) in &parts {
                    let dd = o[k].clone();
                    self.write(&dd, &format!("(udiv_a{k} {sym} udiv_b{k})"), n, "u32", false)?;
                }
                self.line("}");
            }
            op::UBFE | op::IBFE => {
                let signed = ins.op == op::IBFE;
                let v = self.per_comp(
                    &dc,
                    |c| {
                        let w = self.raw(&o[1], &[o[1].comp(c as usize)])?;
                        let off = self.raw(&o[2], &[o[2].comp(c as usize)])?;
                        let x = self.raw(&o[3], &[o[3].comp(c as usize)])?;
                        let x = if signed {
                            format!("bitcast<i32>({x})")
                        } else {
                            x
                        };
                        Ok(format!("extractBits({x}, {off} & 31u, {w} & 31u)"))
                    },
                    if signed { "i32" } else { "u32" },
                )?;
                if signed { wi!(v) } else { wu!(v) }
            }
            op::BFI => {
                let v = self.per_comp(
                    &dc,
                    |c| {
                        let g = |k: usize| self.raw(&o[k], &[o[k].comp(c as usize)]);
                        Ok(format!(
                            "insertBits({}, {}, {} & 31u, {} & 31u)",
                            g(4)?,
                            g(3)?,
                            g(2)?,
                            g(1)?
                        ))
                    },
                    "u32",
                )?;
                wu!(v)
            }
            op::F32TOF16 => {
                let v = self.per_comp(
                    &dc,
                    |c| {
                        Ok(format!(
                            "(pack2x16float(vec2<f32>(bitcast<f32>({}), 0.0)) & 0xffffu)",
                            self.raw(&o[1], &[o[1].comp(c as usize)])?
                        ))
                    },
                    "u32",
                )?;
                wu!(v)
            }
            op::F16TOF32 => {
                let v = self.per_comp(
                    &dc,
                    |c| {
                        Ok(format!(
                            "unpack2x16float({} & 0xffffu).x",
                            self.raw(&o[1], &[o[1].comp(c as usize)])?
                        ))
                    },
                    "f32",
                )?;
                wf!(v)
            }
            op::SAMPLE
            | op::SAMPLE_L
            | op::SAMPLE_B
            | op::SAMPLE_D
            | op::SAMPLE_C
            | op::SAMPLE_C_LZ => self.sample(ins)?,
            op::GATHER4 | op::GATHER4_C => self.gather(ins)?,
            op::LOD => self.lod(ins)?,
            // Sample positions of a resource or the rasterizer: every target
            // and texture made here is single-sampled, for which D3D returns
            // zero.
            op::SAMPLE_POS => wf!(if n == 1 { "0.0".to_string() } else { format!("vec{n}<f32>(0.0)") }),
            op::LD => self.load(ins)?,
            op::RESINFO => self.resinfo(ins)?,
            op::LD_RAW | op::LD_STRUCTURED => self.ld_raw(ins)?,
            op::STORE_RAW | op::STORE_STRUCTURED => self.store_raw(ins)?,
            op::LD_UAV_TYPED | op::STORE_UAV_TYPED => self.uav_typed(ins)?,
            op::ATOMIC_AND..=op::IMM_ATOMIC_UMIN => self.atomic(ins)?,
            op::SYNC => {
                let f = (ins.token >> 11) & 0xf;
                if f & 0xc != 0 {
                    self.line("storageBarrier();");
                }
                if f & 0x3 != 0 {
                    self.line("workgroupBarrier();");
                }
            }
            op::BUFINFO => {
                let (name, _, stride) = self.buffer(&o[1])?;
                let n = if stride > 0 {
                    format!("(arrayLength(&{name}) * 4u / {stride}u)")
                } else {
                    format!("(arrayLength(&{name}) * 4u)")
                };
                wu!(n)
            }
            op::IF => {
                let c = self.cond(&o[0], ins.nonzero)?;
                self.line(&format!("if {c} {{"));
                self.depth += 1;
            }
            op::ELSE => {
                self.depth = self.depth.saturating_sub(1);
                self.line("} else {");
                self.depth += 1;
            }
            op::ENDIF | op::ENDLOOP => {
                self.depth = self.depth.saturating_sub(1);
                self.line("}");
            }
            op::LOOP => {
                self.line("loop {");
                self.depth += 1;
            }
            op::BREAK => self.line("break;"),
            op::CONTINUE => self.line("continue;"),
            op::BREAKC => {
                let c = self.cond(&o[0], ins.nonzero)?;
                self.line(&format!("if {c} {{ break; }}"));
            }
            op::CONTINUEC => {
                let c = self.cond(&o[0], ins.nonzero)?;
                self.line(&format!("if {c} {{ continue; }}"));
            }
            op::DISCARD => {
                self.refl.discards = true;
                let c = self.cond(&o[0], ins.nonzero)?;
                self.line(&format!("if {c} {{ discard; }}"));
            }
            op::RET => {
                if self.depth == 0 {
                    // The last `ret` ends the body.
                } else {
                    self.line("return;");
                }
            }
            op::RETC => {
                let c = self.cond(&o[0], ins.nonzero)?;
                self.line(&format!("if {c} {{ return; }}"));
            }
            op::NOP => {}
            op::SWITCH => {
                let v = self.i(&o[0], &[0])?;
                self.line(&format!("switch ({v}) {{"));
                self.depth += 1;
                self.switches.push((Vec::new(), false, false));
            }
            op::CASE | op::DEFAULT => {
                let label = if ins.op == op::DEFAULT {
                    "default".to_string()
                } else {
                    format!("{}i", o[0].imm[0] as i32)
                };
                let Some(sw) = self.switches.last_mut() else {
                    return Err(TranslateError::Malformed("case outside a switch"));
                };
                if ins.op == op::DEFAULT {
                    sw.2 = true;
                }
                if sw.1 {
                    sw.1 = false;
                    self.depth = self.depth.saturating_sub(1);
                    self.line("}");
                }
                if let Some(sw) = self.switches.last_mut() {
                    sw.0.push(label);
                }
            }
            op::ENDSWITCH => {
                if self.switches.last().is_some_and(|s| !s.0.is_empty()) {
                    self.open_case();
                }
                let Some((_, open, has_default)) = self.switches.pop() else {
                    return Err(TranslateError::Malformed("endswitch outside a switch"));
                };
                if open {
                    self.depth = self.depth.saturating_sub(1);
                    self.line("}");
                }
                if !has_default {
                    self.line("default: {}");
                }
                self.depth = self.depth.saturating_sub(1);
                self.line("}");
            }
            op::FIRSTBIT_LO => wu!(format!("firstTrailingBit({})", ul(self, 1)?)),
            op::FIRSTBIT_HI => wu!(format!(
                "select({0}, 31u - firstLeadingBit({1}), {1} != {0})",
                vt(n, "u32") + "(0xffffffffu)",
                ul(self, 1)?
            )),
            op::FIRSTBIT_SHI => wu!(format!(
                "select({0}, 31u - firstLeadingBit({1}), {1} != {0} && {1} != {2})",
                vt(n, "u32") + "(0xffffffffu)",
                ul(self, 1)?,
                vt(n, "u32") + "(0u)"
            )),
            other => return Err(TranslateError::UnsupportedOpcode(other)),
        }
        Ok(())
    }

    fn declare(&mut self, ins: &Instr) -> R<bool> {
        let o = &ins.operands;
        match ins.op {
            op::DCL_TEMPS => self.temps = ins.extra.first().copied().unwrap_or(0),
            op::DCL_INDEXABLE_TEMP => {
                let e = &ins.extra;
                if e.len() >= 2 {
                    self.indexable.push((e[0], e[1]));
                }
            }
            op::DCL_CONSTANT_BUFFER => {
                let slot = Self::reg_of(&o[0])?;
                let size = match o[0].index.get(1) {
                    Some(Index::Imm(v)) => *v,
                    _ => 4096,
                };
                self.cbuffers.insert(slot, size.max(1));
            }
            op::DCL_SAMPLER => {
                let slot = Self::reg_of(&o[0])?;
                self.samplers.insert(slot, (ins.token >> 11) & 0xf == 1);
            }
            op::DCL_RESOURCE => {
                let slot = Self::reg_of(&o[0])?;
                let d = (ins.token >> 11) & 0x1f;
                let dim = match d {
                    2 => TextureDim::D1,
                    3 => TextureDim::D2,
                    4 => TextureDim::D2Ms,
                    5 => TextureDim::D3,
                    6 => TextureDim::Cube,
                    8 => TextureDim::D2Array,
                    10 => TextureDim::CubeArray,
                    1 => TextureDim::Buffer,
                    _ => return Err(TranslateError::UnsupportedResource { slot, dim: d }),
                };
                self.resources.insert(
                    slot,
                    Resource {
                        dim,
                        ret: ins.extra.first().copied().unwrap_or(0x5555),
                        stride: 0,
                    },
                );
            }
            op::DCL_RESOURCE_RAW => {
                let slot = Self::reg_of(&o[0])?;
                self.resources.insert(
                    slot,
                    Resource {
                        dim: TextureDim::Raw,
                        ret: 0x4444,
                        stride: 0,
                    },
                );
            }
            op::DCL_RESOURCE_STRUCTURED => {
                let slot = Self::reg_of(&o[0])?;
                let stride = ins.extra.first().copied().unwrap_or(4);
                self.resources.insert(
                    slot,
                    Resource {
                        dim: TextureDim::Raw,
                        ret: 0x4444,
                        stride,
                    },
                );
            }
            op::DCL_THREAD_GROUP => {
                let e = &ins.extra;
                self.thread_group = [
                    e.first().copied().unwrap_or(1),
                    e.get(1).copied().unwrap_or(1),
                    e.get(2).copied().unwrap_or(1),
                ];
            }
            op::DCL_UAV_TYPED => {
                let slot = Self::reg_of(&o[0])?;
                let d = (ins.token >> 11) & 0x1f;
                let dim = match d {
                    1 => TextureDim::Buffer,
                    2 => TextureDim::D1,
                    3 => TextureDim::D2,
                    5 => TextureDim::D3,
                    8 => TextureDim::D2Array,
                    _ => return Err(TranslateError::UnsupportedResource { slot, dim: d }),
                };
                let ret = ins.extra.first().copied().unwrap_or(0x5555);
                self.uav_entry(slot, UavKind::Typed(dim), ret);
            }
            op::DCL_UAV_RAW => {
                let slot = Self::reg_of(&o[0])?;
                self.uav_entry(slot, UavKind::Raw, 0x4444);
            }
            op::DCL_UAV_STRUCTURED => {
                let slot = Self::reg_of(&o[0])?;
                let stride = ins.extra.first().copied().unwrap_or(4);
                self.uav_entry(slot, UavKind::Structured(stride), 0x4444);
            }
            op::DCL_TGSM_RAW => {
                let r = Self::reg_of(&o[0])?;
                let bytes = ins.extra.first().copied().unwrap_or(4);
                let e = self.tgsm.entry(r).or_insert((0, 0, false));
                e.0 = bytes.div_ceil(4).max(1);
            }
            op::DCL_TGSM_STRUCTURED => {
                let r = Self::reg_of(&o[0])?;
                let stride = ins.extra.first().copied().unwrap_or(4);
                let count = ins.extra.get(1).copied().unwrap_or(1);
                let e = self.tgsm.entry(r).or_insert((0, 0, false));
                e.0 = (stride * count).div_ceil(4).max(1);
                e.1 = stride;
            }
            op::DCL_INPUT | op::DCL_INPUT_PS => {
                if o[0].ty == ot::INPUT {
                    let r = Self::reg_of(&o[0])?;
                    let e = self.inputs.entry(r).or_insert((0, 0, 0));
                    e.0 |= o[0].mask();
                    e.1 = (ins.token >> 11) & 0xf;
                } else if o[0].ty == ot::INPUT_PRIMITIVEID {
                    self.uses_prim_id = true;
                } else if o[0].ty == ot::INPUT_COVERAGE_MASK {
                    self.uses_coverage = true;
                } else if let Some(k) = [
                    ot::THREAD_ID,
                    ot::THREAD_GROUP_ID,
                    ot::THREAD_ID_IN_GROUP,
                    ot::THREAD_ID_IN_GROUP_FLATTENED,
                ]
                .iter()
                .position(|t| *t == o[0].ty)
                {
                    self.thread_ids[k] = true;
                }
            }
            op::DCL_INPUT_SGV | op::DCL_INPUT_SIV | op::DCL_INPUT_PS_SGV | op::DCL_INPUT_PS_SIV => {
                let r = Self::reg_of(&o[0])?;
                let e = self.inputs.entry(r).or_insert((0, 0, 0));
                e.0 |= o[0].mask();
                e.1 = (ins.token >> 11) & 0xf;
                e.2 = ins.extra.first().copied().unwrap_or(0);
            }
            op::DCL_OUTPUT | op::DCL_OUTPUT_SGV | op::DCL_OUTPUT_SIV => {
                if o[0].ty == ot::OUTPUT {
                    let r = Self::reg_of(&o[0])?;
                    let sv = ins.extra.first().copied().unwrap_or(0);
                    let e = self.outputs.entry(r).or_insert((0, 0));
                    e.0 |= o[0].mask();
                    if sv != 0 {
                        e.1 = sv;
                    }
                } else if matches!(
                    o[0].ty,
                    ot::OUTPUT_DEPTH | ot::OUTPUT_DEPTH_GE | ot::OUTPUT_DEPTH_LE
                ) {
                    self.writes_depth = true;
                }
            }
            op::CUSTOMDATA => {
                if (ins.token >> 11) == 3 {
                    self.icb = Some(ins.data.clone());
                }
            }
            op::DCL_GLOBAL_FLAGS | op::DCL_INDEX_RANGE => {}
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn location(&self, s: &SigElement) -> Option<u32> {
        if self.opts.varyings.is_empty() {
            return Some(s.register);
        }
        let k = sem_key(&s.semantic);
        self.opts
            .varyings
            .iter()
            .find(|(sem, i, _)| *sem == k && *i == s.semantic_index)
            .map(|v| v.2)
    }
}

fn interp_attr(ty: &str, interp: u32) -> &'static str {
    if ty != "f32" {
        return "@interpolate(flat) ";
    }
    match interp {
        1 => "@interpolate(flat) ",
        3 => "@interpolate(perspective, centroid) ",
        4 => "@interpolate(linear) ",
        5 => "@interpolate(linear, centroid) ",
        6 => "@interpolate(perspective, sample) ",
        7 => "@interpolate(linear, sample) ",
        _ => "",
    }
}

fn comp_ty(t: u32) -> &'static str {
    match t {
        1 => "u32",
        2 => "i32",
        _ => "f32",
    }
}

/// Translates one DXBC program.
pub fn translate(bytes: &[u8], opts: &Options) -> Result<Translation, TranslateError> {
    let sig = Container::parse(bytes).map_err(TranslateError::Container)?;
    let prog = crate::decode::decode(sig.shader).map_err(TranslateError::Decode)?;
    translate_program(&prog, &sig, opts)
}

pub fn translate_program(
    prog: &Program,
    sig: &Container,
    opts: &Options,
) -> Result<Translation, TranslateError> {
    if !matches!(prog.stage, Stage::Pixel | Stage::Vertex | Stage::Compute) {
        return Err(TranslateError::UnsupportedStage(prog.stage));
    }
    let mut em = Em {
        prog,
        sig,
        opts,
        out: String::new(),
        body: String::new(),
        depth: 0,
        refl: Reflection::default(),
        temps: 0,
        indexable: Vec::new(),
        resources: BTreeMap::new(),
        compare_textures: Vec::new(),
        samplers: BTreeMap::new(),
        cbuffers: BTreeMap::new(),
        icb: None,
        inputs: BTreeMap::new(),
        outputs: BTreeMap::new(),
        uses_prim_id: false,
        uses_coverage: false,
        writes_depth: false,
        thread_group: [1, 1, 1],
        uavs: BTreeMap::new(),
        tgsm: BTreeMap::new(),
        thread_ids: [false; 4],
        switches: Vec::new(),
    };
    // Pass 1: declarations, and which textures compare-sample.
    for ins in &prog.instrs {
        em.declare(ins)?;
        if matches!(ins.op, op::SAMPLE_C | op::SAMPLE_C_LZ | op::GATHER4_C)
            && let Some(r) = ins.operands.get(2).and_then(|o| o.reg())
            && !em.compare_textures.contains(&r)
        {
            em.compare_textures.push(r);
        }
    }
    // Which buffers atomics touch (their words become atomics), and which
    // typed UAVs are read.
    for ins in &prog.instrs {
        let res = match ins.op {
            op::ATOMIC_AND..=op::ATOMIC_UMIN => ins.operands.first(),
            op::IMM_ATOMIC_ALLOC..=op::IMM_ATOMIC_UMIN => ins.operands.get(1),
            op::LD_UAV_TYPED => {
                if let Some(r) = ins.operands.get(2).and_then(|o| o.reg())
                    && let Some(u) = em.uavs.get_mut(&r)
                {
                    u.loaded = true;
                }
                None
            }
            _ => None,
        };
        if let Some(o) = res
            && let Some(r) = o.reg()
        {
            if o.ty == ot::UAV {
                if let Some(u) = em.uavs.get_mut(&r) {
                    u.atomic = true;
                    u.counter |= matches!(ins.op, op::IMM_ATOMIC_ALLOC | op::IMM_ATOMIC_CONSUME);
                }
            } else if o.ty == ot::TGSM
                && let Some(g) = em.tgsm.get_mut(&r)
            {
                g.2 = true;
            }
        }
    }
    // Pass 2: the body.
    for ins in &prog.instrs {
        if ins.op == op::CUSTOMDATA || em.is_decl(ins.op) {
            continue;
        }
        em.instr(ins)?;
    }
    em.finish()
}

impl Em<'_> {
    fn is_decl(&self, opc: u16) -> bool {
        matches!(
            opc,
            op::DCL_TEMPS
                | op::DCL_INDEXABLE_TEMP
                | op::DCL_CONSTANT_BUFFER
                | op::DCL_SAMPLER
                | op::DCL_RESOURCE
                | op::DCL_RESOURCE_RAW
                | op::DCL_INPUT
                | op::DCL_INPUT_PS
                | op::DCL_INPUT_SGV
                | op::DCL_INPUT_SIV
                | op::DCL_INPUT_PS_SGV
                | op::DCL_INPUT_PS_SIV
                | op::DCL_OUTPUT
                | op::DCL_OUTPUT_SGV
                | op::DCL_OUTPUT_SIV
                | op::DCL_GLOBAL_FLAGS
                | op::DCL_INDEX_RANGE
                | op::DCL_RESOURCE_STRUCTURED
                | op::DCL_THREAD_GROUP
                | op::DCL_UAV_TYPED
                | op::DCL_UAV_RAW
                | op::DCL_UAV_STRUCTURED
                | op::DCL_TGSM_RAW
                | op::DCL_TGSM_STRUCTURED
        )
    }

    fn finish(mut self) -> Result<Translation, TranslateError> {
        let b = self.opts.bindings;
        let mut w = String::new();
        let _ = writeln!(w, "diagnostic(off, derivative_uniformity);");
        // Resources.
        for (&slot, &regs) in &self.cbuffers {
            let binding = b.binding_base + slot;
            let _ = writeln!(w, "struct CB{slot} {{ d: array<vec4<u32>, {regs}> }}");
            let _ = writeln!(
                w,
                "@group({}) @binding({binding}) var<uniform> cb{slot}: CB{slot};",
                b.cbuffer_group
            );
            self.refl.cbuffers.push(CbufferRef {
                slot,
                binding,
                registers: regs,
            });
        }
        for (&slot, res) in &self.resources {
            let binding = b.binding_base + slot;
            let depth = self.compare_textures.contains(&slot);
            let ret = match res.ret & 0xf {
                3 => "i32",
                4 => "u32",
                _ => "f32",
            };
            let (ty, kind) = match (res.dim, depth) {
                (TextureDim::Raw, _) => ("array<u32>".to_string(), TextureKind::RawBuffer),
                (TextureDim::Buffer, _) => ("array<u32>".to_string(), TextureKind::RawBuffer),
                (TextureDim::D2, true) => ("texture_depth_2d".into(), TextureKind::Depth),
                (TextureDim::D2Array, true) => {
                    ("texture_depth_2d_array".into(), TextureKind::Depth)
                }
                (TextureDim::Cube, true) => ("texture_depth_cube".into(), TextureKind::Depth),
                (TextureDim::CubeArray, true) => {
                    ("texture_depth_cube_array".into(), TextureKind::Depth)
                }
                (d, _) => {
                    let base = match d {
                        TextureDim::D1 => "texture_1d",
                        TextureDim::D2 => "texture_2d",
                        TextureDim::D2Array => "texture_2d_array",
                        TextureDim::D3 => "texture_3d",
                        TextureDim::Cube => "texture_cube",
                        TextureDim::CubeArray => "texture_cube_array",
                        TextureDim::D2Ms => "texture_multisampled_2d",
                        _ => return Err(TranslateError::UnsupportedResource { slot, dim: 1 }),
                    };
                    let kind = match ret {
                        "i32" => TextureKind::Sint,
                        "u32" => TextureKind::Uint,
                        _ => TextureKind::Float,
                    };
                    (format!("{base}<{ret}>"), kind)
                }
            };
            if kind == TextureKind::RawBuffer {
                let _ = writeln!(
                    w,
                    "@group({}) @binding({binding}) var<storage, read> t{slot}: {ty};",
                    b.texture_group
                );
            } else {
                let _ = writeln!(
                    w,
                    "@group({}) @binding({binding}) var t{slot}: {ty};",
                    b.texture_group
                );
            }
            self.refl.textures.push(TextureRef {
                slot,
                binding,
                dim: res.dim,
                kind,
            });
        }
        for (&slot, &cmp) in &self.samplers {
            let binding = b.binding_base + slot;
            let ty = if cmp { "sampler_comparison" } else { "sampler" };
            let _ = writeln!(
                w,
                "@group({}) @binding({binding}) var s{slot}: {ty};",
                b.sampler_group
            );
            self.refl.samplers.push(SamplerRef {
                slot,
                binding,
                comparison: cmp,
            });
        }
        // Unordered access views (texture group, after the textures).
        for (&slot, u) in &self.uavs {
            let binding = b.binding_base + 128 + slot;
            let words = self.opts.typed_buffer_uavs_as_words && u.kind == UavKind::Typed(TextureDim::Buffer);
            let (decl, format) = match u.kind {
                _ if words => (format!("var<storage, read_write> u{slot}: array<u32>"), String::new()),
                UavKind::Typed(dim) => {
                    let format = self
                        .opts
                        .uav_formats
                        .iter()
                        .find(|(s, _)| *s == slot)
                        .map(|(_, f)| f.clone())
                        .unwrap_or_else(|| {
                            match u.ret & 0xf {
                                3 => "r32sint",
                                4 => "r32uint",
                                _ => "rgba16float",
                            }
                            .to_string()
                        });
                    let base = match dim {
                        TextureDim::D1 | TextureDim::Buffer => "texture_storage_1d",
                        TextureDim::D2 => "texture_storage_2d",
                        TextureDim::D2Array => "texture_storage_2d_array",
                        TextureDim::D3 => "texture_storage_3d",
                        _ => return Err(TranslateError::UnsupportedResource { slot, dim: 2 }),
                    };
                    let access = if u.loaded { "read_write" } else { "write" };
                    (format!("var u{slot}: {base}<{format}, {access}>"), format)
                }
                _ => {
                    let el = if u.atomic { "atomic<u32>" } else { "u32" };
                    (format!("var<storage, read_write> u{slot}: array<{el}>"), String::new())
                }
            };
            let _ = writeln!(w, "@group({}) @binding({binding}) {decl};", b.texture_group);
            self.refl.uavs.push(UavRef {
                slot,
                binding,
                kind: if words { UavKind::Raw } else { u.kind },
                format,
                atomic: u.atomic,
                counter: u.counter,
            });
        }
        for (&r, &(words, _, atomic)) in &self.tgsm {
            let el = if atomic { "atomic<u32>" } else { "u32" };
            let _ = writeln!(w, "var<workgroup> g{r}: array<{el}, {words}>;");
        }
        for (k, name) in ["vThreadID", "vThreadGroupID", "vThreadIDInGroup", "vThreadIDInGroupFlattened"]
            .iter()
            .enumerate()
        {
            if self.thread_ids[k] {
                let _ = writeln!(w, "var<private> {name}: vec4<u32>;");
            }
        }
        self.refl.thread_group = self.thread_group;
        // Registers.
        for i in 0..self.temps {
            let _ = writeln!(w, "var<private> r{i}: vec4<u32>;");
        }
        for &(n, count) in &self.indexable {
            let _ = writeln!(w, "var<private> x{n}: array<vec4<u32>, {count}>;");
        }
        if let Some(d) = &self.icb {
            let rows = d.len() / 4;
            let mut vals = Vec::new();
            for r in 0..rows {
                vals.push(format!(
                    "vec4<u32>({}u, {}u, {}u, {}u)",
                    d[4 * r],
                    d[4 * r + 1],
                    d[4 * r + 2],
                    d[4 * r + 3]
                ));
            }
            let _ = writeln!(
                w,
                "var<private> icb: array<vec4<u32>, {rows}> = array<vec4<u32>, {rows}>({});",
                vals.join(", ")
            );
        }
        let max_in = self.inputs.keys().max().map(|m| m + 1).unwrap_or(0);
        for i in 0..max_in {
            let _ = writeln!(w, "var<private> v{i}: vec4<u32>;");
        }
        let max_out = self.outputs.keys().max().map(|m| m + 1).unwrap_or(0);
        for i in 0..max_out {
            let _ = writeln!(w, "var<private> o{i}: vec4<u32>;");
        }
        if self.uses_prim_id {
            let _ = writeln!(w, "var<private> vPrim: vec4<u32>;");
        }
        if self.uses_coverage {
            let _ = writeln!(w, "var<private> vCoverage: vec4<u32>;");
        }
        if self.writes_depth {
            let _ = writeln!(w, "var<private> oDepth: u32;");
        }
        let _ = writeln!(w, "fn body() {{");
        w.push_str(&self.body);
        let _ = writeln!(w, "}}");

        // Entry point: inputs → v#, body, o# → outputs.
        let stage = self.prog.stage;
        if stage == Stage::Compute {
            let [x, y, z] = self.thread_group;
            if self.opts.group_offset_override {
                let _ = writeln!(w, "override t7_group_offset: u32 = 0u;");
            }
            if self.opts.bounds_audit {
                let _ = writeln!(w, "@group({}) @binding({OOB_BINDING}) var<storage, read_write> t7_oob: array<atomic<u32>>;", self.opts.bindings.cbuffer_group);
            }
            let _ = writeln!(
                w,
                "@compute @workgroup_size({x}, {y}, {z})\nfn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>, @builtin(local_invocation_index) lidx: u32) {{"
            );
            for (k, e) in [
                "vThreadID = vec4<u32>(gid, 0u);",
                if self.opts.group_offset_override { "vThreadGroupID = vec4<u32>(wid.x + t7_group_offset, wid.y, wid.z, 0u);" } else { "vThreadGroupID = vec4<u32>(wid, 0u);" },
                "vThreadIDInGroup = vec4<u32>(lid, 0u);",
                "vThreadIDInGroupFlattened = vec4<u32>(lidx);",
            ]
            .iter()
            .enumerate()
            {
                if self.thread_ids[k] {
                    let _ = writeln!(w, "    {e}");
                }
            }
            let _ = writeln!(w, "    body();\n}}");
            self.out = w;
            return Ok(Translation {
                stage,
                wgsl: self.out,
                reflection: self.refl,
            });
        }
        let mut in_fields: Vec<String> = Vec::new();
        let mut prologue: Vec<String> = Vec::new();
        let mut used_locs: Vec<u32> = Vec::new();
        let inputs = self.sig.inputs.clone();
        for s in &inputs {
            let Some(&(dmask, interp, _sv)) = self.inputs.get(&s.register) else {
                continue;
            };
            if dmask & s.mask == 0 {
                continue;
            }
            let reg = s.register;
            let comps = mask_comps(s.mask & dmask);
            let ty = comp_ty(s.component_type);
            match (stage, s.system_value) {
                (Stage::Pixel, 1) => {
                    in_fields.push("@builtin(position) sv_pos: vec4<f32>".into());
                    if self.opts.d3d_position_w {
                        prologue.push(format!("v{reg} = bitcast<vec4<u32>>(vec4<f32>(input.sv_pos.xyz, 1.0 / input.sv_pos.w));"));
                    } else {
                        prologue.push(format!("v{reg} = bitcast<vec4<u32>>(input.sv_pos);"));
                    }
                }
                (Stage::Pixel, 9) => {
                    in_fields.push("@builtin(front_facing) sv_ff: bool".into());
                    for &c in &comps {
                        prologue.push(format!(
                            "v{reg}.{} = select(0u, 0xffffffffu, input.sv_ff);",
                            LET[c as usize]
                        ));
                    }
                }
                (Stage::Pixel, 10) => {
                    in_fields.push("@builtin(sample_index) sv_si: u32".into());
                    for &c in &comps {
                        prologue.push(format!("v{reg}.{} = input.sv_si;", LET[c as usize]));
                    }
                }
                (Stage::Vertex, 6) => {
                    in_fields.push("@builtin(vertex_index) sv_vid: u32".into());
                    for &c in &comps {
                        prologue.push(format!("v{reg}.{} = input.sv_vid;", LET[c as usize]));
                    }
                }
                (Stage::Vertex, 8) => {
                    in_fields.push("@builtin(instance_index) sv_iid: u32".into());
                    for &c in &comps {
                        prologue.push(format!("v{reg}.{} = input.sv_iid;", LET[c as usize]));
                    }
                }
                (_, 0) => {
                    let loc = if stage == Stage::Vertex {
                        Some(s.register)
                    } else {
                        self.location(s)
                    };
                    let Some(loc) = loc else { continue };
                    if used_locs.contains(&loc) {
                        continue;
                    }
                    used_locs.push(loc);
                    let interp_attr = if stage == Stage::Pixel {
                        interp_attr(ty, interp)
                    } else {
                        ""
                    };
                    let field = format!("l{loc}");
                    in_fields.push(format!("@location({loc}) {interp_attr}{field}: vec4<{ty}>"));
                    for &c in &comps {
                        let l = LET[c as usize];
                        prologue.push(format!("v{reg}.{l} = bitcast<u32>(input.{field}.{l});"));
                    }
                    self.refl.inputs.push(VaryingRef {
                        semantic: s.semantic.clone(),
                        index: s.semantic_index,
                        location: loc,
                        register: reg,
                        mask: s.mask,
                        component_type: s.component_type,
                        interp: if stage == Stage::Pixel { interp } else { 0 },
                    });
                }
                _ => {}
            }
        }
        if self.uses_prim_id {
            in_fields.push("@builtin(primitive_index) sv_prim: u32".into());
            prologue.push("vPrim = vec4<u32>(input.sv_prim);".into());
        }
        if self.uses_coverage {
            in_fields.push("@builtin(sample_mask) sv_cov: u32".into());
            prologue.push("vCoverage = vec4<u32>(input.sv_cov);".into());
        }
        let mut out_fields: Vec<String> = Vec::new();
        let mut epilogue: Vec<String> = Vec::new();
        let mut out_locs: Vec<u32> = Vec::new();
        let outputs = self.sig.outputs.clone();
        for s in &outputs {
            // Pixel outputs are named, not coded: SV_Target / SV_Depth*.
            let key = sem_key(&s.semantic);
            let sv = match key.as_str() {
                "SV_TARGET" => 64,
                "SV_DEPTH" => 65,
                "SV_DEPTHGREATEREQUAL" => 66,
                "SV_DEPTHLESSEQUAL" => 67,
                "SV_COVERAGE" => 39,
                "SV_POSITION" => 1,
                _ => s.system_value,
            };
            let is_depth = matches!(sv, 65..=67);
            if !self.outputs.contains_key(&s.register) && !(stage == Stage::Pixel && is_depth) {
                continue;
            }
            let reg = s.register;
            let ty = comp_ty(s.component_type);
            match (stage, sv) {
                (Stage::Vertex, 1) => {
                    out_fields.push("@builtin(position) sv_pos: vec4<f32>".into());
                    epilogue.push(format!("output.sv_pos = bitcast<vec4<f32>>(o{reg});"));
                }
                (Stage::Pixel, 64) => {
                    let t = s.semantic_index;
                    out_fields.push(format!("@location({t}) sv_t{t}: vec4<{ty}>"));
                    epilogue.push(format!("output.sv_t{t} = bitcast<vec4<{ty}>>(o{reg});"));
                    self.refl.targets.push(t);
                }
                (Stage::Pixel, 65) | (Stage::Pixel, 66) | (Stage::Pixel, 67) => {
                    if self.writes_depth {
                        out_fields.push("@builtin(frag_depth) sv_depth: f32".into());
                        epilogue.push("output.sv_depth = bitcast<f32>(oDepth);".into());
                    }
                }
                (Stage::Vertex, 0) => {
                    let Some(loc) = self.location(s) else {
                        continue;
                    };
                    if out_locs.contains(&loc) {
                        continue;
                    }
                    out_locs.push(loc);
                    let k = sem_key(&s.semantic);
                    let mode = self
                        .opts
                        .output_interp
                        .iter()
                        .find(|(sem, i, _)| *sem == k && *i == s.semantic_index)
                        .map_or(0, |v| v.2);
                    let interp = interp_attr(ty, mode);
                    out_fields.push(format!("@location({loc}) {interp}l{loc}: vec4<{ty}>"));
                    epilogue.push(format!("output.l{loc} = bitcast<vec4<{ty}>>(o{reg});"));
                    self.refl.outputs.push(VaryingRef {
                        semantic: s.semantic.clone(),
                        index: s.semantic_index,
                        location: loc,
                        register: reg,
                        mask: s.mask,
                        component_type: s.component_type,
                        interp: mode,
                    });
                }
                _ => {}
            }
        }
        if stage == Stage::Vertex && !out_fields.iter().any(|f| f.contains("@builtin(position)")) {
            return Err(TranslateError::NoPosition);
        }
        self.refl.writes_depth = self.writes_depth;
        if !in_fields.is_empty() {
            let _ = writeln!(w, "struct In {{\n    {},\n}}", in_fields.join(",\n    "));
        }
        if out_fields.is_empty() {
            // A pixel shader with no colour output (depth-only passes).
            out_fields.push("@location(0) unused: vec4<f32>".into());
        }
        let _ = writeln!(w, "struct Out {{\n    {},\n}}", out_fields.join(",\n    "));
        let attr = if stage == Stage::Vertex {
            "@vertex"
        } else {
            "@fragment"
        };
        let param = if in_fields.is_empty() {
            String::new()
        } else {
            "input: In".into()
        };
        let _ = writeln!(w, "{attr} fn main({param}) -> Out {{");
        for p in &prologue {
            let _ = writeln!(w, "    {p}");
        }
        let _ = writeln!(w, "    body();");
        let _ = writeln!(w, "    var output: Out;");
        for e in &epilogue {
            let _ = writeln!(w, "    {e}");
        }
        let _ = writeln!(w, "    return output;\n}}");
        self.out = w;
        Ok(Translation {
            stage,
            wgsl: self.out,
            reflection: self.refl,
        })
    }
}

/// Links a vertex stage's outputs to a pixel stage's inputs by semantic:
/// each semantic both use gets one location, in pixel input order.
pub fn link(vs_outputs: &[SigElement], ps_inputs: &[SigElement]) -> VaryingMap {
    let mut out: VaryingMap = Vec::new();
    let mut next = 0u32;
    for p in ps_inputs.iter().filter(|s| s.system_value == 0) {
        let k = sem_key(&p.semantic);
        if out
            .iter()
            .any(|(s, i, _)| *s == k && *i == p.semantic_index)
        {
            continue;
        }
        if vs_outputs.iter().any(|v| {
            v.system_value == 0 && sem_key(&v.semantic) == k && v.semantic_index == p.semantic_index
        }) {
            out.push((k, p.semantic_index, next));
            next += 1;
        }
    }
    // Vertex outputs the pixel stage doesn't read still need a location.
    for v in vs_outputs.iter().filter(|s| s.system_value == 0) {
        let k = sem_key(&v.semantic);
        if !out
            .iter()
            .any(|(s, i, _)| *s == k && *i == v.semantic_index)
        {
            out.push((k, v.semantic_index, next));
            next += 1;
        }
    }
    out
}
