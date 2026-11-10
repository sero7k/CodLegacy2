//! The decoder: bytes of a `ScriptFile` -> functions and instructions.

use std::collections::HashMap;
use std::fmt;

use crate::opcodes::{self, Layout};

/// The two blocks of a `ScriptFile` asset: the inflated `buffer` ("stream") and the `bytecode`.
#[derive(Clone, Copy, Debug)]
pub struct ScriptBytes<'a> {
    pub stream: &'a [u8],
    pub code: &'a [u8],
}

/// A name as the file stores it: an id into the engine's string table, or inline text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Name {
    Id(u16),
    Text(String),
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Name::Id(i) => write!(f, "#{i}"),
            Name::Text(s) => f.write_str(s),
        }
    }
}

/// A `switch` case label.
#[derive(Clone, Debug, PartialEq)]
pub enum CaseValue {
    Default,
    Str(String),
    Int(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub value: CaseValue,
    /// Absolute code offset of the case body.
    pub target: usize,
}

/// A decoded operand. Jump and call targets are absolute code offsets (`usize::MAX` when the
/// stored distance leaves the code block).
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    U8(u8),
    U16(u16),
    I32(i32),
    F32(f32),
    Vec3([f32; 3]),
    /// A name token (field).
    Token(Name),
    /// A string literal (text from the stream; invalid UTF-8 replaced).
    Str(String),
    Builtin(u16),
    Argc(u8),
    /// A jump target.
    Jump(usize),
    /// The function a local call / function reference leads to.
    Func(usize),
    /// A far call's (file, function) pair.
    Far { file: Name, func: Name },
    /// The `switch_table` instruction a `switch` reads.
    Table(usize),
    Cases(Vec<Case>),
    Anim { tree: String, name: String },
    Tree(String),
}

/// One instruction.
#[derive(Clone, Debug, PartialEq)]
pub struct Instr {
    /// Offset in the bytecode block.
    pub offset: usize,
    pub op: u8,
    pub mnemonic: &'static str,
    /// Bytes including the opcode.
    pub len: usize,
    pub operands: Vec<Operand>,
}

/// A function's header (from the stream).
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionHeader {
    pub name: Name,
    /// Offset of the first instruction.
    pub offset: usize,
    /// Body length in bytes.
    pub len: usize,
}

/// What the decoder yields: a function header, then that function's instructions.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Function(FunctionHeader),
    Instr(Instr),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The stream ended inside `what`.
    StreamEnd { what: &'static str, at: usize },
    /// The bytecode ended inside the instruction at `at`.
    CodeEnd { at: usize },
    /// A function header with a length that is negative or leaves the code.
    BadLength { at: usize, len: i32 },
    /// An instruction runs past the end of its function.
    CrossesEnd { at: usize, function_end: usize },
    /// An opcode the VM does not implement.
    Unknown { at: usize, op: u8 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::StreamEnd { what, at } => write!(f, "stream ends in {what} at {at:#x}"),
            Error::CodeEnd { at } => write!(f, "bytecode ends in the instruction at {at:#x}"),
            Error::BadLength { at, len } => {
                write!(f, "function at {at:#x} has length {len}")
            }
            Error::CrossesEnd { at, function_end } => {
                write!(f, "instruction at {at:#x} crosses the function end {function_end:#x}")
            }
            Error::Unknown { at, op } => write!(f, "unknown opcode {op:#04x} at {at:#x}"),
        }
    }
}

impl std::error::Error for Error {}

/// Token operands from here up are not engine ids: the name is inline in the stream (the
/// compiler writes 0xFFFF).
const INLINE_TOKEN_MIN: u16 = 0x95a2;
/// Case values below this are strings.
const CASE_STRING_LIMIT: u32 = 0x40000;

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// The streaming decoder. Iterate for [`Event`]s; stops after the first error.
pub struct Decoder<'a> {
    bytes: ScriptBytes<'a>,
    ip: usize,
    sp: usize,
    fend: usize,
    failed: bool,
}

impl<'a> Decoder<'a> {
    pub fn new(bytes: ScriptBytes<'a>) -> Decoder<'a> {
        // (Byte 0 of the bytecode is a filler `end`; the first body starts at 1.)
        Decoder { bytes, ip: 1, sp: 0, fend: 1, failed: false }
    }

    /// Bytes of the stream consumed so far.
    pub fn stream_pos(&self) -> usize {
        self.sp
    }

    fn s_u16(&mut self, what: &'static str) -> Result<u16, Error> {
        let b = self
            .bytes
            .stream
            .get(self.sp..self.sp + 2)
            .ok_or(Error::StreamEnd { what, at: self.sp })?;
        self.sp += 2;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn s_i32(&mut self, what: &'static str) -> Result<i32, Error> {
        let b = self
            .bytes
            .stream
            .get(self.sp..self.sp + 4)
            .ok_or(Error::StreamEnd { what, at: self.sp })?;
        self.sp += 4;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn s_cstr(&mut self, what: &'static str) -> Result<&'a [u8], Error> {
        let rest = self
            .bytes
            .stream
            .get(self.sp..)
            .ok_or(Error::StreamEnd { what, at: self.sp })?;
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::StreamEnd { what, at: self.sp })?;
        self.sp += n + 1;
        Ok(&rest[..n])
    }

    /// A name from the stream: `u16 id`, or `0` and NUL-terminated text.
    fn s_name(&mut self, what: &'static str) -> Result<Name, Error> {
        let id = self.s_u16(what)?;
        if id == 0 {
            Ok(Name::Text(lossy(self.s_cstr(what)?)))
        } else {
            Ok(Name::Id(id))
        }
    }

    fn header(&mut self) -> Result<FunctionHeader, Error> {
        let len = self.s_i32("function length")?;
        let name = self.s_name("function name")?;
        let end = usize::try_from(len)
            .ok()
            .and_then(|l| self.ip.checked_add(l))
            .filter(|&e| e <= self.bytes.code.len())
            .ok_or(Error::BadLength { at: self.ip, len })?;
        let h = FunctionHeader { name, offset: self.ip, len: end - self.ip };
        self.fend = end;
        Ok(h)
    }

    /// `base + sext24(raw) / 4` as an absolute offset, or `usize::MAX` outside the code.
    fn rel24(&self, raw: [u8; 3], base: usize) -> usize {
        let v = i32::from_le_bytes([raw[0], raw[1], raw[2], 0]) << 8 >> 8;
        self.target(base, i64::from(v >> 2))
    }

    fn target(&self, base: usize, delta: i64) -> usize {
        let t = base as i64 + delta;
        if (0..=self.bytes.code.len() as i64).contains(&t) {
            t as usize
        } else {
            usize::MAX
        }
    }

    fn instr(&mut self) -> Result<Instr, Error> {
        let code = self.bytes.code;
        let at = self.ip;
        let op = code[at];
        let info = opcodes::info(op).ok_or(Error::Unknown { at, op })?;
        let layout = info.layout;
        let body_len = match layout.operand_len() {
            Some(n) => n,
            None => {
                let c = code.get(at + 1..at + 3).ok_or(Error::CodeEnd { at })?;
                2 + 7 * usize::from(u16::from_le_bytes([c[0], c[1]]))
            }
        };
        let end = at + 1 + body_len;
        let b = code.get(at + 1..end).ok_or(Error::CodeEnd { at })?;
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let i32_at = |i: usize| i32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let operands = match layout {
            Layout::None => Vec::new(),
            Layout::U8 => vec![Operand::U8(b[0])],
            Layout::U16 => vec![Operand::U16(u16_at(0))],
            Layout::I32 => vec![Operand::I32(i32_at(0))],
            Layout::F32 => vec![Operand::F32(f32::from_bits(i32_at(0) as u32))],
            Layout::Vec3 => {
                let f = |i: usize| f32::from_bits(i32_at(i) as u32);
                vec![Operand::Vec3([f(0), f(4), f(8)])]
            }
            Layout::Token => {
                let v = u16_at(0);
                let name = if v >= INLINE_TOKEN_MIN {
                    Name::Text(lossy(self.s_name_text("inline token")?))
                } else {
                    Name::Id(v)
                };
                vec![Operand::Token(name)]
            }
            Layout::Str => vec![Operand::Str(lossy(self.s_cstr("string literal")?))],
            Layout::Anim => {
                let tree = lossy(self.s_cstr("animation tree")?);
                let name = lossy(self.s_cstr("animation name")?);
                vec![Operand::Anim { tree, name }]
            }
            Layout::AnimTree => {
                vec![Operand::U8(b[0]), Operand::Tree(lossy(self.s_cstr("anim tree name")?))]
            }
            Layout::BuiltinId => vec![Operand::Builtin(u16_at(0))],
            Layout::BuiltinArgc => vec![Operand::Argc(b[0]), Operand::Builtin(u16_at(1))],
            Layout::JumpFwd => vec![Operand::Jump(self.target(end, i64::from(u16_at(0))))],
            Layout::JumpBack => vec![Operand::Jump(self.target(end, -i64::from(u16_at(0))))],
            Layout::Jump32 => vec![Operand::Jump(self.target(end, i64::from(i32_at(0))))],
            // (The operand points at the u16 count, one past the table's opcode.)
            Layout::Switch => {
                vec![Operand::Table(self.target(end, i64::from(i32_at(0)) - 1))]
            }
            Layout::SwitchTable => {
                let n = usize::from(u16_at(0));
                let mut cases = Vec::with_capacity(n);
                for k in 0..n {
                    let e = 2 + 7 * k;
                    let raw = u32::from_le_bytes([b[e], b[e + 1], b[e + 2], b[e + 3]]);
                    let target = self.rel24([b[e + 4], b[e + 5], b[e + 6]], at + 1 + e + 4);
                    let value = if raw < CASE_STRING_LIMIT {
                        let s = self.s_cstr("case string")?;
                        if s == [1] { CaseValue::Default } else { CaseValue::Str(lossy(s)) }
                    } else {
                        CaseValue::Int((raw & 0xff_ffff) as i32 - 0x80_0000)
                    };
                    cases.push(Case { value, target });
                }
                vec![Operand::Cases(cases)]
            }
            Layout::CallFar | Layout::CallFarArgc => {
                let file = self.s_name("far call file")?;
                let func = self.s_name("far call function")?;
                let mut v = vec![Operand::Far { file, func }];
                if layout == Layout::CallFarArgc {
                    v.push(Operand::Argc(b[3]));
                }
                v
            }
            Layout::CallLocal | Layout::CallLocalArgc => {
                let mut v = vec![Operand::Func(self.rel24([b[0], b[1], b[2]], at + 1))];
                if layout == Layout::CallLocalArgc {
                    v.push(Operand::Argc(b[3]));
                }
                v
            }
        };
        if end > self.fend {
            return Err(Error::CrossesEnd { at, function_end: self.fend });
        }
        self.ip = end;
        Ok(Instr { offset: at, op, mnemonic: info.mnemonic, len: end - at, operands })
    }

    /// An inline token's text: the stream holds `00 00` and the text.
    fn s_name_text(&mut self, what: &'static str) -> Result<&'a [u8], Error> {
        let zero = self.s_u16(what)?;
        if zero != 0 {
            // (Not the inline form; the stream does not agree with the code.)
            return Err(Error::StreamEnd { what, at: self.sp - 2 });
        }
        self.s_cstr(what)
    }
}

impl Iterator for Decoder<'_> {
    type Item = Result<Event, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.ip >= self.bytes.code.len() {
            return None;
        }
        let r = if self.ip == self.fend {
            self.header().map(Event::Function)
        } else {
            self.instr().map(Event::Instr)
        };
        if r.is_err() {
            self.failed = true;
        }
        Some(r)
    }
}

/// A function of a script.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: Name,
    /// Offset of the first instruction.
    pub offset: usize,
    /// Offset one past the last byte.
    pub end: usize,
    /// Index of the first instruction in `Script::instrs`.
    pub first: usize,
    pub count: usize,
    /// Parameters: the leading `param` instructions.
    pub params: usize,
}

/// A problem `Script::validate` found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub offset: usize,
    pub message: String,
}

/// A fully decoded script file.
#[derive(Clone, Debug)]
pub struct Script {
    /// The filler byte at the start of the bytecode (always `end`).
    pub prelude: u8,
    pub code_len: usize,
    pub stream_len: usize,
    /// Stream bytes the decode consumed (all of them for a well-formed script).
    pub stream_used: usize,
    pub functions: Vec<Function>,
    pub instrs: Vec<Instr>,
}

impl Script {
    pub fn parse(bytes: &ScriptBytes) -> Result<Script, Error> {
        let mut dec = Decoder::new(*bytes);
        let mut functions: Vec<Function> = Vec::new();
        let mut instrs: Vec<Instr> = Vec::new();
        while let Some(ev) = dec.next() {
            match ev? {
                Event::Function(h) => {
                    functions.push(Function {
                        name: h.name,
                        offset: h.offset,
                        end: h.offset + h.len,
                        first: instrs.len(),
                        count: 0,
                        params: 0,
                    });
                }
                Event::Instr(i) => {
                    if let Some(f) = functions.last_mut() {
                        f.count += 1;
                        if i.op == 0x2c && f.params == f.count - 1 {
                            f.params += 1;
                        }
                    }
                    instrs.push(i);
                }
            }
        }
        Ok(Script {
            prelude: bytes.code.first().copied().unwrap_or(0),
            code_len: bytes.code.len(),
            stream_len: bytes.stream.len(),
            stream_used: dec.stream_pos(),
            functions,
            instrs,
        })
    }

    /// The instructions of function `f`.
    pub fn body(&self, f: &Function) -> &[Instr] {
        &self.instrs[f.first..f.first + f.count]
    }

    /// The function starting at code offset `offset`.
    pub fn function_at(&self, offset: usize) -> Option<&Function> {
        self.functions
            .binary_search_by_key(&offset, |f| f.offset)
            .ok()
            .map(|i| &self.functions[i])
    }

    /// The function with a given name token, first match.
    pub fn function_named(&self, name: &Name) -> Option<&Function> {
        self.functions.iter().find(|f| &f.name == name)
    }

    /// Files this script calls into or takes functions from (its includes), in first-use order.
    pub fn referenced_files(&self) -> Vec<Name> {
        let mut seen: HashMap<Name, ()> = HashMap::new();
        let mut out = Vec::new();
        for i in &self.instrs {
            for o in &i.operands {
                if let Operand::Far { file, .. } = o
                    && seen.insert(file.clone(), ()).is_none()
                {
                    out.push(file.clone());
                }
            }
        }
        out
    }

    /// Checks what a sound decode guarantees: the whole stream used, the prelude an `end`,
    /// every jump, switch case and switch table on an instruction boundary, every local call
    /// and function reference on a function start. Empty when all is well.
    pub fn validate(&self) -> Vec<Problem> {
        let mut bad = Vec::new();
        let mut problem = |offset: usize, message: String| bad.push(Problem { offset, message });
        if self.stream_used != self.stream_len {
            problem(0, format!("stream: {} of {} bytes used", self.stream_used, self.stream_len));
        }
        if self.prelude != 0x34 {
            problem(0, format!("prelude byte {:#04x} is not end", self.prelude));
        }
        let mut boundary = vec![false; self.code_len + 1];
        for i in &self.instrs {
            boundary[i.offset] = true;
        }
        boundary[self.code_len] = true;
        let ok = |t: usize| t != usize::MAX && boundary[t];
        for i in &self.instrs {
            for o in &i.operands {
                match o {
                    Operand::Jump(t) if !ok(*t) => {
                        problem(i.offset, format!("{} target {t:#x} is not an instruction", i.mnemonic));
                    }
                    Operand::Table(t) => {
                        let is_table = self
                            .instrs
                            .binary_search_by_key(t, |x| x.offset)
                            .ok()
                            .is_some_and(|k| self.instrs[k].op == 0x3b);
                        if !is_table {
                            problem(i.offset, format!("switch table {t:#x} is not a switch_table"));
                        }
                    }
                    Operand::Cases(cs) => {
                        for c in cs {
                            if !ok(c.target) {
                                problem(i.offset, format!("case target {:#x} is not an instruction", c.target));
                            }
                        }
                    }
                    Operand::Func(t) if self.function_at(*t).is_none() => {
                        problem(i.offset, format!("{} target {t:#x} is not a function", i.mnemonic));
                    }
                    _ => {}
                }
            }
        }
        for f in &self.functions {
            if f.count == 0 && f.end != f.offset {
                problem(f.offset, "function without instructions".to_owned());
            } else if let Some(last) = self.body(f).last()
                && last.offset + last.len != f.end
            {
                problem(f.offset, "function does not end on an instruction".to_owned());
            }
        }
        bad
    }
}
