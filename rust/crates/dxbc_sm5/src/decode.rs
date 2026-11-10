//! SM4/SM5 token stream → instruction list.

use alloc::boxed::Box;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Pixel,
    Vertex,
    Geometry,
    Hull,
    Domain,
    Compute,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    BadStage(u32),
    ZeroLength { at: usize, opcode: u16 },
    BadIndexRep(u32),
}

/// Opcode numbers (D3D10_SB / D3D11_SB).
#[allow(missing_docs)]
pub mod op {
    pub const ADD: u16 = 0;
    pub const AND: u16 = 1;
    pub const BREAK: u16 = 2;
    pub const BREAKC: u16 = 3;
    pub const CALL: u16 = 4;
    pub const CALLC: u16 = 5;
    pub const CASE: u16 = 6;
    pub const CONTINUE: u16 = 7;
    pub const CONTINUEC: u16 = 8;
    pub const CUT: u16 = 9;
    pub const DEFAULT: u16 = 10;
    pub const DERIV_RTX: u16 = 11;
    pub const DERIV_RTY: u16 = 12;
    pub const DISCARD: u16 = 13;
    pub const DIV: u16 = 14;
    pub const DP2: u16 = 15;
    pub const DP3: u16 = 16;
    pub const DP4: u16 = 17;
    pub const ELSE: u16 = 18;
    pub const EMIT: u16 = 19;
    pub const EMITTHENCUT: u16 = 20;
    pub const ENDIF: u16 = 21;
    pub const ENDLOOP: u16 = 22;
    pub const ENDSWITCH: u16 = 23;
    pub const EQ: u16 = 24;
    pub const EXP: u16 = 25;
    pub const FRC: u16 = 26;
    pub const FTOI: u16 = 27;
    pub const FTOU: u16 = 28;
    pub const GE: u16 = 29;
    pub const IADD: u16 = 30;
    pub const IF: u16 = 31;
    pub const IEQ: u16 = 32;
    pub const IGE: u16 = 33;
    pub const ILT: u16 = 34;
    pub const IMAD: u16 = 35;
    pub const IMAX: u16 = 36;
    pub const IMIN: u16 = 37;
    pub const IMUL: u16 = 38;
    pub const INE: u16 = 39;
    pub const INEG: u16 = 40;
    pub const ISHL: u16 = 41;
    pub const ISHR: u16 = 42;
    pub const ITOF: u16 = 43;
    pub const LABEL: u16 = 44;
    pub const LD: u16 = 45;
    pub const LD_MS: u16 = 46;
    pub const LOG: u16 = 47;
    pub const LOOP: u16 = 48;
    pub const LT: u16 = 49;
    pub const MAD: u16 = 50;
    pub const MIN: u16 = 51;
    pub const MAX: u16 = 52;
    pub const CUSTOMDATA: u16 = 53;
    pub const MOV: u16 = 54;
    pub const MOVC: u16 = 55;
    pub const MUL: u16 = 56;
    pub const NE: u16 = 57;
    pub const NOP: u16 = 58;
    pub const NOT: u16 = 59;
    pub const OR: u16 = 60;
    pub const RESINFO: u16 = 61;
    pub const RET: u16 = 62;
    pub const RETC: u16 = 63;
    pub const ROUND_NE: u16 = 64;
    pub const ROUND_NI: u16 = 65;
    pub const ROUND_PI: u16 = 66;
    pub const ROUND_Z: u16 = 67;
    pub const RSQ: u16 = 68;
    pub const SAMPLE: u16 = 69;
    pub const SAMPLE_C: u16 = 70;
    pub const SAMPLE_C_LZ: u16 = 71;
    pub const SAMPLE_L: u16 = 72;
    pub const SAMPLE_D: u16 = 73;
    pub const SAMPLE_B: u16 = 74;
    pub const SQRT: u16 = 75;
    pub const SWITCH: u16 = 76;
    pub const SINCOS: u16 = 77;
    pub const UDIV: u16 = 78;
    pub const ULT: u16 = 79;
    pub const UGE: u16 = 80;
    pub const UMUL: u16 = 81;
    pub const UMAD: u16 = 82;
    pub const UMAX: u16 = 83;
    pub const UMIN: u16 = 84;
    pub const USHR: u16 = 85;
    pub const UTOF: u16 = 86;
    pub const XOR: u16 = 87;
    pub const DCL_RESOURCE: u16 = 88;
    pub const DCL_CONSTANT_BUFFER: u16 = 89;
    pub const DCL_SAMPLER: u16 = 90;
    pub const DCL_INDEX_RANGE: u16 = 91;
    pub const DCL_INPUT: u16 = 95;
    pub const DCL_INPUT_SGV: u16 = 96;
    pub const DCL_INPUT_SIV: u16 = 97;
    pub const DCL_INPUT_PS: u16 = 98;
    pub const DCL_INPUT_PS_SGV: u16 = 99;
    pub const DCL_INPUT_PS_SIV: u16 = 100;
    pub const DCL_OUTPUT: u16 = 101;
    pub const DCL_OUTPUT_SGV: u16 = 102;
    pub const DCL_OUTPUT_SIV: u16 = 103;
    pub const DCL_TEMPS: u16 = 104;
    pub const DCL_INDEXABLE_TEMP: u16 = 105;
    pub const DCL_GLOBAL_FLAGS: u16 = 106;
    // 107 is reserved (D3D10_SB_OPCODE_RESERVED0), 112 too.
    pub const LOD: u16 = 108;
    pub const GATHER4: u16 = 109;
    pub const SAMPLE_POS: u16 = 110;
    pub const SAMPLE_INFO: u16 = 111;
    pub const HS_DECLS: u16 = 113;
    pub const HS_CONTROL_POINT_PHASE: u16 = 114;
    pub const HS_FORK_PHASE: u16 = 115;
    pub const HS_JOIN_PHASE: u16 = 116;
    pub const BUFINFO: u16 = 121;
    pub const DERIV_RTX_COARSE: u16 = 122;
    pub const DERIV_RTX_FINE: u16 = 123;
    pub const DERIV_RTY_COARSE: u16 = 124;
    pub const DERIV_RTY_FINE: u16 = 125;
    pub const GATHER4_C: u16 = 126;
    pub const GATHER4_PO: u16 = 127;
    pub const GATHER4_PO_C: u16 = 128;
    pub const RCP: u16 = 129;
    pub const F32TOF16: u16 = 130;
    pub const F16TOF32: u16 = 131;
    pub const UADDC: u16 = 132;
    pub const USUBB: u16 = 133;
    pub const COUNTBITS: u16 = 134;
    pub const FIRSTBIT_HI: u16 = 135;
    pub const FIRSTBIT_LO: u16 = 136;
    pub const FIRSTBIT_SHI: u16 = 137;
    pub const UBFE: u16 = 138;
    pub const IBFE: u16 = 139;
    pub const BFI: u16 = 140;
    pub const BFREV: u16 = 141;
    pub const SWAPC: u16 = 142;
    pub const DCL_INPUT_CONTROL_POINT_COUNT: u16 = 147;
    pub const DCL_OUTPUT_CONTROL_POINT_COUNT: u16 = 148;
    pub const DCL_TESS_DOMAIN: u16 = 149;
    pub const DCL_TESS_PARTITIONING: u16 = 150;
    pub const DCL_TESS_OUTPUT_PRIMITIVE: u16 = 151;
    pub const DCL_HS_MAX_TESSFACTOR: u16 = 152;
    pub const DCL_HS_FORK_PHASE_INSTANCE_COUNT: u16 = 153;
    pub const DCL_THREAD_GROUP: u16 = 155;
    pub const DCL_RESOURCE_RAW: u16 = 161;
    pub const DCL_RESOURCE_STRUCTURED: u16 = 162;
    pub const LD_RAW: u16 = 165;
    pub const LD_STRUCTURED: u16 = 167;
    pub const DCL_UAV_TYPED: u16 = 156;
    pub const DCL_UAV_RAW: u16 = 157;
    pub const DCL_UAV_STRUCTURED: u16 = 158;
    pub const DCL_TGSM_RAW: u16 = 159;
    pub const DCL_TGSM_STRUCTURED: u16 = 160;
    pub const LD_UAV_TYPED: u16 = 163;
    pub const STORE_UAV_TYPED: u16 = 164;
    pub const STORE_RAW: u16 = 166;
    pub const STORE_STRUCTURED: u16 = 168;
    pub const ATOMIC_AND: u16 = 169;
    pub const ATOMIC_OR: u16 = 170;
    pub const ATOMIC_XOR: u16 = 171;
    pub const ATOMIC_CMP_STORE: u16 = 172;
    pub const ATOMIC_IADD: u16 = 173;
    pub const ATOMIC_IMAX: u16 = 174;
    pub const ATOMIC_IMIN: u16 = 175;
    pub const ATOMIC_UMAX: u16 = 176;
    pub const ATOMIC_UMIN: u16 = 177;
    pub const IMM_ATOMIC_ALLOC: u16 = 178;
    pub const IMM_ATOMIC_CONSUME: u16 = 179;
    pub const IMM_ATOMIC_IADD: u16 = 180;
    pub const IMM_ATOMIC_AND: u16 = 181;
    pub const IMM_ATOMIC_OR: u16 = 182;
    pub const IMM_ATOMIC_XOR: u16 = 183;
    pub const IMM_ATOMIC_EXCH: u16 = 184;
    pub const IMM_ATOMIC_CMP_EXCH: u16 = 185;
    pub const IMM_ATOMIC_IMAX: u16 = 186;
    pub const IMM_ATOMIC_IMIN: u16 = 187;
    pub const IMM_ATOMIC_UMAX: u16 = 188;
    pub const IMM_ATOMIC_UMIN: u16 = 189;
    pub const SYNC: u16 = 190;
}

/// Operand types.
#[allow(missing_docs)]
pub mod ot {
    pub const TEMP: u16 = 0;
    pub const INPUT: u16 = 1;
    pub const OUTPUT: u16 = 2;
    pub const INDEXABLE_TEMP: u16 = 3;
    pub const IMM32: u16 = 4;
    pub const IMM64: u16 = 5;
    pub const SAMPLER: u16 = 6;
    pub const RESOURCE: u16 = 7;
    pub const CONSTANT_BUFFER: u16 = 8;
    pub const IMMEDIATE_CONSTANT_BUFFER: u16 = 9;
    pub const LABEL: u16 = 10;
    pub const INPUT_PRIMITIVEID: u16 = 11;
    pub const OUTPUT_DEPTH: u16 = 12;
    pub const NULL: u16 = 13;
    pub const OUTPUT_COVERAGE_MASK: u16 = 15;
    pub const INPUT_COVERAGE_MASK: u16 = 35;
    pub const OUTPUT_DEPTH_GE: u16 = 38;
    pub const OUTPUT_DEPTH_LE: u16 = 39;
    pub const UAV: u16 = 30;
    pub const TGSM: u16 = 31;
    pub const THREAD_ID: u16 = 32;
    pub const THREAD_GROUP_ID: u16 = 33;
    pub const THREAD_ID_IN_GROUP: u16 = 34;
    pub const THREAD_ID_IN_GROUP_FLATTENED: u16 = 36;
}

/// How an operand picks its components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    /// No components (labels, samplers' bare form, NULL).
    None,
    /// Destination write mask (bits xyzw).
    Mask(u8),
    /// Source swizzle.
    Swizzle([u8; 4]),
    /// Source single component.
    Scalar(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Index {
    Imm(u32),
    Rel(u32, Box<Operand>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Operand {
    pub ty: u16,
    /// 0, 1 or 4 components.
    pub ncomp: u8,
    pub select: Select,
    pub index: Vec<Index>,
    pub neg: bool,
    pub abs: bool,
    /// Immediate values (IMM32): 1 or 4 words.
    pub imm: [u32; 4],
}

impl Operand {
    /// The first index as an immediate (register number), if it is one.
    pub fn reg(&self) -> Option<u32> {
        match self.index.first()? {
            Index::Imm(v) => Some(*v),
            Index::Rel(..) => None,
        }
    }
    /// Destination mask (4 components → its mask; 1 → x).
    pub fn mask(&self) -> u8 {
        match self.select {
            Select::Mask(m) => m,
            _ if self.ncomp == 1 => 1,
            _ => 0xf,
        }
    }
    /// The component a source reads for destination component `c`.
    pub fn comp(&self, c: usize) -> u8 {
        match self.select {
            Select::Swizzle(s) => s[c],
            Select::Scalar(s) => s,
            Select::Mask(_) => c as u8,
            Select::None => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Instr {
    pub op: u16,
    /// The raw opcode token (bits 11..23 carry per-opcode controls).
    pub token: u32,
    pub saturate: bool,
    /// Conditional ops: true = test non-zero.
    pub nonzero: bool,
    /// Sample offsets (aoffimmi), signed.
    pub offsets: [i8; 3],
    pub operands: Vec<Operand>,
    /// Extra words of declarations (counts, return types, names).
    pub extra: Vec<u32>,
    /// Custom data payload (immediate constant buffer).
    pub data: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub stage: Stage,
    pub major: u8,
    pub minor: u8,
    pub instrs: Vec<Instr>,
}

struct Rd<'a> {
    w: &'a [u8],
    at: usize,
}

impl Rd<'_> {
    fn next(&mut self) -> Result<u32, DecodeError> {
        let s = self
            .w
            .get(self.at..self.at + 4)
            .ok_or(DecodeError::Truncated)?;
        self.at += 4;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

fn operand(r: &mut Rd) -> Result<Operand, DecodeError> {
    let t = r.next()?;
    let ncomp = match t & 3 {
        0 => 0,
        1 => 1,
        2 => 4,
        _ => 4,
    };
    let select = if ncomp == 4 {
        match (t >> 2) & 3 {
            0 => Select::Mask(((t >> 4) & 0xf) as u8),
            1 => {
                let s = (t >> 4) & 0xff;
                Select::Swizzle([
                    (s & 3) as u8,
                    ((s >> 2) & 3) as u8,
                    ((s >> 4) & 3) as u8,
                    ((s >> 6) & 3) as u8,
                ])
            }
            _ => Select::Scalar(((t >> 4) & 3) as u8),
        }
    } else {
        Select::None
    };
    let ty = ((t >> 12) & 0xff) as u16;
    let dims = (t >> 20) & 3;
    let mut o = Operand {
        ty,
        ncomp,
        select,
        index: Vec::new(),
        neg: false,
        abs: false,
        imm: [0; 4],
    };
    let mut ext = t >> 31 != 0;
    while ext {
        let e = r.next()?;
        if e & 0x3f == 1 {
            let m = (e >> 6) & 0xff;
            o.neg = m & 1 != 0;
            o.abs = m & 2 != 0;
        }
        ext = e >> 31 != 0;
    }
    if ty == ot::IMM32 {
        let n = if ncomp == 1 { 1 } else { 4 };
        for i in 0..n {
            o.imm[i] = r.next()?;
        }
        if n == 1 {
            o.imm = [o.imm[0]; 4];
        }
    } else if ty == ot::IMM64 {
        let n = if ncomp == 1 { 1 } else { 2 };
        for i in 0..n {
            o.imm[2 * i] = r.next()?;
            o.imm[2 * i + 1] = r.next()?;
        }
    }
    for d in 0..dims {
        let rep = (t >> (22 + 3 * d)) & 7;
        let idx = match rep {
            0 => Index::Imm(r.next()?),
            1 => {
                let lo = r.next()?;
                let _hi = r.next()?;
                Index::Imm(lo)
            }
            2 => Index::Rel(0, Box::new(operand(r)?)),
            3 => {
                let v = r.next()?;
                Index::Rel(v, Box::new(operand(r)?))
            }
            _ => return Err(DecodeError::BadIndexRep(rep)),
        };
        o.index.push(idx);
    }
    Ok(o)
}

/// How many trailing non-operand words a declaration carries after its operands.
fn decl_shape(opc: u16) -> (usize, usize) {
    // (operand count, extra words)
    use op::*;
    match opc {
        DCL_RESOURCE => (1, 1),
        DCL_CONSTANT_BUFFER | DCL_SAMPLER => (1, 0),
        DCL_INDEX_RANGE => (1, 1),
        DCL_INPUT | DCL_INPUT_PS | DCL_OUTPUT => (1, 0),
        DCL_INPUT_SGV | DCL_INPUT_SIV | DCL_INPUT_PS_SGV | DCL_INPUT_PS_SIV | DCL_OUTPUT_SGV
        | DCL_OUTPUT_SIV => (1, 1),
        DCL_TEMPS => (0, 1),
        DCL_INDEXABLE_TEMP => (0, 3),
        DCL_GLOBAL_FLAGS => (0, 0),
        DCL_RESOURCE_RAW => (1, 0),
        DCL_RESOURCE_STRUCTURED => (1, 1),
        DCL_HS_MAX_TESSFACTOR => (0, 1),
        DCL_HS_FORK_PHASE_INSTANCE_COUNT => (0, 1),
        DCL_THREAD_GROUP => (0, 3),
        DCL_UAV_TYPED => (1, 1),
        DCL_UAV_RAW => (1, 0),
        DCL_UAV_STRUCTURED => (1, 1),
        DCL_TGSM_RAW => (1, 1),
        DCL_TGSM_STRUCTURED => (1, 2),
        _ => (usize::MAX, 0),
    }
}

pub fn decode(shader: &[u8]) -> Result<Program, DecodeError> {
    let mut r = Rd { w: shader, at: 0 };
    let ver = r.next()?;
    let _len = r.next()?;
    let stage = match ver >> 16 {
        0 => Stage::Pixel,
        1 => Stage::Vertex,
        2 => Stage::Geometry,
        3 => Stage::Hull,
        4 => Stage::Domain,
        5 => Stage::Compute,
        s => return Err(DecodeError::BadStage(s)),
    };
    let mut instrs = Vec::new();
    while r.at < shader.len() {
        let start = r.at;
        let t = r.next()?;
        let opc = (t & 0x7ff) as u16;
        if opc == op::CUSTOMDATA {
            let len = r.next()? as usize;
            let mut data = Vec::with_capacity(len.saturating_sub(2));
            for _ in 2..len {
                data.push(r.next()?);
            }
            instrs.push(Instr {
                op: opc,
                token: t,
                saturate: false,
                nonzero: false,
                offsets: [0; 3],
                operands: Vec::new(),
                extra: Vec::new(),
                data,
            });
            continue;
        }
        let len = ((t >> 24) & 0x7f) as usize;
        if len == 0 {
            return Err(DecodeError::ZeroLength {
                at: start,
                opcode: opc,
            });
        }
        let end = start + len * 4;
        let mut ins = Instr {
            op: opc,
            token: t,
            saturate: (t >> 13) & 1 != 0,
            nonzero: (t >> 18) & 1 != 0,
            offsets: [0; 3],
            operands: Vec::new(),
            extra: Vec::new(),
            data: Vec::new(),
        };
        let mut ext = t >> 31 != 0;
        while ext {
            let e = r.next()?;
            if e & 0x3f == 1 {
                let s4 = |v: u32| -> i8 { (((v & 0xf) << 4) as u8 as i8) >> 4 };
                ins.offsets = [s4(e >> 9), s4(e >> 13), s4(e >> 17)];
            }
            ext = e >> 31 != 0;
        }
        let (nops, _extra) = decl_shape(opc);
        if nops == usize::MAX {
            while r.at < end {
                ins.operands.push(operand(&mut r)?);
            }
        } else {
            for _ in 0..nops {
                ins.operands.push(operand(&mut r)?);
            }
            while r.at < end {
                ins.extra.push(r.next()?);
            }
        }
        r.at = end;
        instrs.push(ins);
    }
    Ok(Program {
        stage,
        major: ((ver >> 4) & 0xf) as u8,
        minor: (ver & 0xf) as u8,
        instrs,
    })
}
