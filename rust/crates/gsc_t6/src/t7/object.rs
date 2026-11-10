//! A Black Ops III (PC) compiled script object: header, tables and the
//! decoded functions.
//!
//! Layouts (read on the retail patch zones, 2026-10-27; every one of
//! `core_patch`, `zm_patch` and `zm_zod_patch`'s objects decodes):
//! - header: magic `80 47 53 43 0D 0A 00 1C`; u32 at 0x08 crc, 0x0c
//!   includes, 0x10 animtrees, 0x14 code, 0x18 string fixups, 0x1c
//!   developer string fixups, 0x20 exports, 0x24 imports, 0x28 fixups,
//!   0x2c profile, 0x30 code size, 0x34 name; u16 at 0x38 strings, 0x3a
//!   exports, 0x3c imports, 0x3e fixups, 0x40 profile, 0x42 developer
//!   strings; u8 at 0x44 includes, 0x45 animtrees, 0x46 flags.
//! - include: u32 offset of the script's name (no extension).
//! - export (0x14): crc u32, address u32, name u32, namespace u32 (both
//!   canonical hashes, [`super::canon`]), params u8, flags u8, pad u16.
//! - import: name u32, namespace u32, count u16, params u8, flags u8, then
//!   `count` u32 opcode addresses. Flags low nibble: the call kind.
//! - string fixup: string u32, count u8, type u8, pad u16, then `count` u32
//!   operand addresses.
//! - animtree: name u32, tree refs u16, anim refs u16, then the tree refs
//!   (u32) and anim refs (name u64, operand address u64).
//!
//! Code: a 16-bit opcode, read after aligning the pc to 2 ([`super::ops::op_of`]);
//! operands aligned to their size (u16 2, u32/f32 4, u64 8). Calls carry
//! a parameter count byte, then the target (8 bytes, written by the
//! linker; zero in the file) or, for class methods, the method hash.
//! Jumps are i16 relative to the end of the operand. A switch's u32 operand
//! is relative to its end and leads (4-aligned) to `count` u32, then
//! `count` (value u32, offset i32) cases, each offset relative to the end
//! of its case; the default case is the last, with value 0.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use super::ops::{T7Op, op_of};

pub const MAGIC: &[u8; 8] = b"\x80GSC\r\n\x00\x1c";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub crc: u32,
    pub address: u32,
    pub name: u32,
    pub ns: u32,
    pub params: u8,
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub name: u32,
    pub ns: u32,
    pub params: u8,
    pub flags: u8,
}

impl Import {
    /// The call kind (flags low nibble): 1 function reference, 2 call,
    /// 3 thread, 4 method, 5 method thread.
    pub fn kind(&self) -> u8 {
        self.flags & 0xf
    }

    pub fn developer(&self) -> bool {
        self.flags & 0x10 != 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaseValue {
    Int(i32),
    Str(String),
    Hash(u32),
    Default,
}

/// A decoded instruction. Addresses are byte offsets in the object.
#[derive(Clone, Debug, PartialEq)]
pub enum Insn {
    Plain(T7Op),
    /// An integer operand: a constant (sign applied), a local index, a
    /// parameter count.
    Int(T7Op, i32),
    U64(u64),
    Float(f32),
    Vector([f32; 3]),
    Hash(u32),
    Str(T7Op, String),
    /// A field operation and the field's hash.
    Field(T7Op, u32),
    /// `SafeCreateLocalVariables`: each local's hash and flags (1: by
    /// reference, 2: the variadic `...`).
    Locals(Vec<(u32, u8)>),
    /// A call or function reference and the import it names (index into
    /// [`T7Object::imports`]); the parameter count for calls.
    Call(T7Op, u8, u32),
    /// A class method call: parameter count, method hash.
    Method(T7Op, u8, u32),
    /// `new <class>`.
    New(u32),
    Anim {
        tree: String,
        anim: String,
    },
    AnimTree(String),
    Jump(T7Op, u32),
    DevBlock {
        to: u32,
    },
    Switch {
        cases: Vec<(CaseValue, u32)>,
        end: u32,
    },
    EndSwitch {
        end: u32,
    },
}

#[derive(Clone, Debug)]
pub struct Function {
    pub export: Export,
    /// Reachable instructions in address order.
    pub code: Vec<(u32, Insn)>,
}

#[derive(Clone, Debug)]
pub struct T7Object {
    pub name: String,
    pub crc: u32,
    pub includes: Vec<String>,
    pub imports: Vec<Import>,
    pub functions: Vec<Function>,
    /// String fixups the table lists and how many the decoder read.
    pub string_refs: (usize, usize),
    /// Import sites the table lists and how many the decoder read.
    pub import_sites: (usize, usize),
}

fn u16_at(b: &[u8], o: usize) -> Result<u16, String> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| format!("u16 at {o:#x} past the end"))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("u32 at {o:#x} past the end"))
}

fn u64_at(b: &[u8], o: usize) -> Result<u64, String> {
    Ok(u64::from(u32_at(b, o)?) | u64::from(u32_at(b, o + 4)?) << 32)
}

fn byte_at(b: &[u8], o: usize) -> Result<u8, String> {
    b.get(o).copied().ok_or_else(|| format!("byte at {o:#x} past the end"))
}

fn cstr_at(b: &[u8], o: usize) -> Result<String, String> {
    let tail = b.get(o..).ok_or_else(|| format!("string at {o:#x} past the end"))?;
    let end = tail.iter().position(|&c| c == 0).ok_or("unterminated string")?;
    Ok(tail[..end].iter().map(|&c| char::from(c)).collect())
}

const fn align(p: usize, n: usize) -> usize {
    (p + n - 1) & !(n - 1)
}

struct Tables {
    /// Opcode address -> import index.
    imports: FxHashMap<usize, u32>,
    /// Operand address -> string.
    strings: FxHashMap<usize, String>,
    trees: FxHashMap<usize, String>,
    anims: FxHashMap<usize, (String, String)>,
    code_start: usize,
    code_end: usize,
}

/// Is this a Black Ops III script object?
pub fn is_t7_object(bytes: &[u8]) -> bool {
    bytes.get(..8) == Some(MAGIC.as_slice())
}

impl T7Object {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if !is_t7_object(bytes) {
            return Err("not a Black Ops III script object".into());
        }
        let b = bytes;
        let crc = u32_at(b, 0x08)?;
        let include_off = u32_at(b, 0x0c)? as usize;
        let animtree_off = u32_at(b, 0x10)? as usize;
        let code = u32_at(b, 0x14)? as usize;
        let strings_off = u32_at(b, 0x18)? as usize;
        let exports_off = u32_at(b, 0x20)? as usize;
        let imports_off = u32_at(b, 0x24)? as usize;
        let code_size = u32_at(b, 0x30)? as usize;
        let name = cstr_at(b, u32_at(b, 0x34)? as usize)?;
        let n_strings = usize::from(u16_at(b, 0x38)?);
        let n_exports = usize::from(u16_at(b, 0x3a)?);
        let n_imports = usize::from(u16_at(b, 0x3c)?);
        let n_includes = usize::from(byte_at(b, 0x44)?);
        let n_trees = usize::from(byte_at(b, 0x45)?);
        let includes = (0..n_includes).map(|i| cstr_at(b, u32_at(b, include_off + 4 * i)? as usize)).collect::<Result<Vec<_>, _>>()?;

        let mut imports = Vec::with_capacity(n_imports);
        let mut sites = FxHashMap::default();
        let mut o = imports_off;
        for i in 0..n_imports {
            let imp = Import { name: u32_at(b, o)?, ns: u32_at(b, o + 4)?, params: byte_at(b, o + 10)?, flags: byte_at(b, o + 11)? };
            let n = usize::from(u16_at(b, o + 8)?);
            o += 12;
            for k in 0..n {
                sites.insert(u32_at(b, o + 4 * k)? as usize, i as u32);
            }
            o += 4 * n;
            imports.push(imp);
        }
        let mut strings = FxHashMap::default();
        // (Developer code's strings have a table of their own.)
        let dev_strings_off = u32_at(b, 0x1c)? as usize;
        let n_dev_strings = usize::from(u16_at(b, 0x42)?);
        for (mut o, count) in [(strings_off, n_strings), (dev_strings_off, n_dev_strings)] {
            for _ in 0..count {
                let s = cstr_at(b, u32_at(b, o)? as usize)?;
                let n = usize::from(byte_at(b, o + 4)?);
                o += 8;
                for k in 0..n {
                    strings.insert(u32_at(b, o + 4 * k)? as usize, s.clone());
                }
                o += 4 * n;
            }
        }
        let mut trees = FxHashMap::default();
        let mut anims = FxHashMap::default();
        let mut o = animtree_off;
        for _ in 0..n_trees {
            let tree = cstr_at(b, u32_at(b, o)? as usize)?;
            let nt = usize::from(u16_at(b, o + 4)?);
            let na = usize::from(u16_at(b, o + 6)?);
            o += 8;
            for k in 0..nt {
                trees.insert(u32_at(b, o + 4 * k)? as usize, tree.clone());
            }
            o += 4 * nt;
            for k in 0..na {
                let anim = cstr_at(b, u32_at(b, o + 16 * k)? as usize)?;
                anims.insert(u32_at(b, o + 16 * k + 8)? as usize, (tree.clone(), anim));
            }
            o += 16 * na;
        }
        let t = Tables { imports: sites, strings, trees, anims, code_start: code, code_end: (code + code_size).min(b.len()) };

        let mut functions = Vec::with_capacity(n_exports);
        for i in 0..n_exports {
            let o = exports_off + 0x14 * i;
            let export = Export {
                crc: u32_at(b, o)?,
                address: u32_at(b, o + 4)?,
                name: u32_at(b, o + 8)?,
                ns: u32_at(b, o + 12)?,
                params: byte_at(b, o + 16)?,
                flags: byte_at(b, o + 17)?,
            };
            let code = decode_function(b, &t, export.address as usize).map_err(|e| format!("{name} {:#010x}: {e}", export.name))?;
            functions.push(Function { export, code });
        }
        functions.sort_by_key(|f| f.export.address);

        // The check: every string and import site the tables list is read
        // by some instruction (or a switch case).
        let mut seen_strings = rustc_hash::FxHashSet::default();
        let mut seen_imports = rustc_hash::FxHashSet::default();
        for f in &functions {
            for (at, insn) in &f.code {
                let at = *at as usize;
                match insn {
                    Insn::Str(..) => {
                        seen_strings.insert(align(at + 2, 4));
                    }
                    Insn::Call(..) => {
                        seen_imports.insert(at);
                    }
                    Insn::Switch { .. } => {
                        let table = switch_table(b, at)?;
                        let n = u32_at(b, table)? as usize;
                        for k in 0..n {
                            let a = table + 4 + 8 * k;
                            if t.strings.contains_key(&a) {
                                seen_strings.insert(a);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let read_strings = seen_strings.iter().filter(|a| t.strings.contains_key(a)).count();
        let read_imports = seen_imports.iter().filter(|a| t.imports.contains_key(a)).count();
        Ok(Self {
            name,
            crc,
            includes,
            imports,
            functions,
            string_refs: (t.strings.len(), read_strings),
            import_sites: (t.imports.len(), read_imports),
        })
    }

    /// The object's namespace: its exports' (the first one's).
    pub fn namespace(&self) -> Option<u32> {
        self.functions.first().map(|f| f.export.ns)
    }
}

/// The address of a switch's case table (its count).
fn switch_table(b: &[u8], at: usize) -> Result<usize, String> {
    let p = align(at + 2, 4);
    let rel = u32_at(b, p)? as i32;
    Ok(align((p as i64 + 4 + i64::from(rel)) as usize, 4))
}

/// Decode the instruction at `pc`: (instruction, next pc, extra successors).
fn decode_one(b: &[u8], t: &Tables, at: usize) -> Result<(Insn, usize, Vec<u32>), String> {
    use T7Op::*;
    let code = u16_at(b, at)?;
    let op = op_of(code);
    let mut pc = at + 2;
    let mut succ = Vec::new();
    let import = |at: usize| -> Result<u32, String> { t.imports.get(&at).copied().ok_or_else(|| format!("{op:?} at {at:#x} has no import")) };
    let insn = match op {
        INVALID => return Err(format!("invalid opcode {code:#06x} at {at:#x}")),
        GET_BYTE | GET_NEG_BYTE => {
            let v = i32::from(byte_at(b, pc)?);
            pc += 1;
            Insn::Int(GET_INTEGER, if op == GET_NEG_BYTE { -v } else { v })
        }
        GET_UNSIGNED_SHORT | GET_NEG_UNSIGNED_SHORT => {
            pc = align(pc, 2);
            let v = i32::from(u16_at(b, pc)?);
            pc += 2;
            Insn::Int(GET_INTEGER, if op == GET_NEG_UNSIGNED_SHORT { -v } else { v })
        }
        GET_INTEGER => {
            pc = align(pc, 4);
            let v = u32_at(b, pc)? as i32;
            let operand = pc;
            pc += 4;
            match t.trees.get(&operand) {
                Some(tree) => Insn::AnimTree(tree.clone()),
                None => Insn::Int(GET_INTEGER, v),
            }
        }
        GET_UINT64 => {
            pc = align(pc, 8);
            let v = u64_at(b, pc)?;
            pc += 8;
            Insn::U64(v)
        }
        GET_FLOAT => {
            pc = align(pc, 4);
            let v = f32::from_bits(u32_at(b, pc)?);
            pc += 4;
            Insn::Float(v)
        }
        GET_VECTOR => {
            pc = align(pc, 4);
            let v = [f32::from_bits(u32_at(b, pc)?), f32::from_bits(u32_at(b, pc + 4)?), f32::from_bits(u32_at(b, pc + 8)?)];
            pc += 12;
            Insn::Vector(v)
        }
        VECTOR_CONSTANT => {
            // Two bits a component, x high: 2 is +1, 1 is -1 (the exe's
            // constants 1.0 and -1.0), else 0.
            let f = byte_at(b, pc)?;
            pc += 1;
            let c = |hi: u8, lo: u8| {
                if f & hi != 0 {
                    1.0
                } else if f & lo != 0 {
                    -1.0
                } else {
                    0.0
                }
            };
            Insn::Vector([c(0x20, 0x10), c(0x08, 0x04), c(0x02, 0x01)])
        }
        GET_HASH => {
            pc = align(pc, 4);
            let v = u32_at(b, pc)?;
            pc += 4;
            Insn::Hash(v)
        }
        GET_STRING | GET_ISTRING => {
            pc = align(pc, 4);
            let s = t.strings.get(&pc).cloned().ok_or_else(|| format!("{op:?} at {at:#x} has no string"))?;
            pc += 4;
            Insn::Str(op, s)
        }
        EVAL_FIELD_VARIABLE | EVAL_FIELD_VARIABLE_REF | EVAL_SELF_FIELD_VARIABLE | EVAL_SELF_FIELD_VARIABLE_REF | EVAL_LEVEL_FIELD_VARIABLE
        | EVAL_LEVEL_FIELD_VARIABLE_REF | CLEAR_FIELD_VARIABLE => {
            pc = align(pc, 4);
            let h = u32_at(b, pc)?;
            pc += 4;
            Insn::Field(op, h)
        }
        SAFE_CREATE_LOCAL_VARIABLES => {
            let n = byte_at(b, pc)?;
            pc += 1;
            let mut vars = Vec::with_capacity(usize::from(n));
            for _ in 0..n {
                pc = align(pc, 4);
                let h = u32_at(b, pc)?;
                let flags = byte_at(b, pc + 4)?;
                pc += 5;
                vars.push((h, flags));
            }
            Insn::Locals(vars)
        }
        EVAL_LOCAL_VARIABLE_CACHED | EVAL_LOCAL_VARIABLE_REF_CACHED | SAFE_SET_VARIABLE_FIELD_CACHED | SAFE_SET_WAITTILL_VARIABLE_FIELD_CACHED
        | SCRIPT_FUNCTION_CALL_POINTER | SCRIPT_METHOD_CALL_POINTER | SCRIPT_THREAD_CALL_POINTER | SCRIPT_METHOD_THREAD_CALL_POINTER
        | WAIT_TILL_MATCH => {
            let v = byte_at(b, pc)?;
            pc += 1;
            Insn::Int(op, i32::from(v))
        }
        GET_FUNCTION | GET_API_FUNCTION | GET_UNRESOLVED_FUNCTION => {
            pc = align(pc, 8);
            pc += 8;
            Insn::Call(op, 0, import(at)?)
        }
        GET_ANIMATION => {
            pc = align(pc, 8);
            let operand = pc;
            pc += 8;
            let (tree, anim) = t.anims.get(&operand).or_else(|| t.anims.get(&at)).cloned().ok_or_else(|| format!("animation at {at:#x} not in the animtree table"))?;
            Insn::Anim { tree, anim }
        }
        CALL_BUILTIN | CALL_BUILTIN_METHOD | SCRIPT_FUNCTION_CALL | SCRIPT_METHOD_CALL | SCRIPT_THREAD_CALL | SCRIPT_METHOD_THREAD_CALL => {
            let n = byte_at(b, pc)?;
            pc += 1;
            pc = align(pc, 8);
            pc += 8;
            Insn::Call(op, n, import(at)?)
        }
        CLASS_FUNCTION_CALL | CLASS_FUNCTION_THREAD_CALL => {
            let n = byte_at(b, pc)?;
            pc += 1;
            pc = align(pc, 4);
            let h = u32_at(b, pc)?;
            pc += 4;
            Insn::Method(op, n, h)
        }
        NEW => {
            pc = align(pc, 4);
            let h = u32_at(b, pc)?;
            pc += 4;
            Insn::New(h)
        }
        JUMP_ON_FALSE | JUMP_ON_TRUE | JUMP_ON_FALSE_EXPR | JUMP_ON_TRUE_EXPR | JUMP | DEVBLOCK_BEGIN => {
            pc = align(pc, 2);
            let v = i32::from(u16_at(b, pc)? as i16);
            pc += 2;
            // (An odd target is the instruction after it: the VM aligns
            // the pc before reading the opcode.)
            let to = align((pc as i64 + i64::from(v)) as usize, 2) as u32;
            succ.push(to);
            if op == DEVBLOCK_BEGIN { Insn::DevBlock { to } } else { Insn::Jump(op, to) }
        }
        SWITCH => {
            pc = align(pc, 4);
            pc += 4;
            let table = switch_table(b, at)?;
            let n = u32_at(b, table)? as usize;
            let mut cases = Vec::with_capacity(n);
            for k in 0..n {
                let a = table + 4 + 8 * k;
                let v = u32_at(b, a)?;
                let off = u32_at(b, a + 4)? as i32;
                let target = align((a as i64 + 8 + i64::from(off)) as usize, 2) as u32;
                let value = if let Some(s) = t.strings.get(&a) {
                    CaseValue::Str(s.clone())
                } else if v == 0 && k + 1 == n {
                    CaseValue::Default
                } else {
                    CaseValue::Int(v as i32)
                };
                succ.push(target);
                cases.push((value, target));
            }
            let end = (table + 4 + 8 * n) as u32;
            if !cases.iter().any(|c| c.0 == CaseValue::Default) {
                succ.push(end);
            }
            Insn::Switch { cases, end }
        }
        END_SWITCH => {
            pc = align(pc, 4);
            let n = u32_at(b, pc)? as usize;
            pc += 4 + 8 * n;
            Insn::EndSwitch { end: pc as u32 }
        }
        _ => Insn::Plain(op),
    };
    Ok((insn, pc, succ))
}

/// Decode a function by following its control flow from `start`.
fn decode_function(b: &[u8], t: &Tables, start: usize) -> Result<Vec<(u32, Insn)>, String> {
    let mut seen: BTreeMap<u32, Insn> = BTreeMap::new();
    let mut work = vec![align(start, 2) as u32];
    while let Some(mut pc) = work.pop() {
        loop {
            if seen.contains_key(&pc) {
                break;
            }
            if (pc as usize) < t.code_start || (pc as usize) >= t.code_end {
                return Err(format!("code address {pc:#x} outside the code segment"));
            }
            let (insn, next, succ) = decode_one(b, t, pc as usize)?;
            work.extend(succ.into_iter().map(|a| align(a as usize, 2) as u32));
            let stop = matches!(insn, Insn::Plain(T7Op::END | T7Op::RETURN) | Insn::Jump(T7Op::JUMP, _) | Insn::Switch { .. });
            seen.insert(pc, insn);
            if stop {
                break;
            }
            // (The VM aligns the pc to 2 before reading each opcode.)
            pc = align(next, 2) as u32;
        }
    }
    for insn in seen.values() {
        let targets: Vec<u32> = match insn {
            Insn::Jump(_, to) | Insn::DevBlock { to } => vec![*to],
            Insn::Switch { cases, .. } => cases.iter().map(|c| c.1).collect(),
            _ => Vec::new(),
        };
        for to in targets {
            if !seen.contains_key(&(align(to as usize, 2) as u32)) {
                return Err(format!("jump to {to:#x} is not an instruction"));
            }
        }
    }
    Ok(seen.into_iter().collect())
}
