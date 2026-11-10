//! The LLVM bitstream container: blocks, abbreviations and records, read
//! into a tree. Nothing here knows what a block means.

use alloc::string::String;
use alloc::vec::Vec;

/// What went wrong reading a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The stream ended inside a field.
    Eof,
    /// A value the format does not allow.
    Bad(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Eof => f.write_str("bitstream ends early"),
            Self::Bad(w) => write!(f, "bad bitstream: {w}"),
        }
    }
}

/// One operand of an abbreviation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbbrevOp {
    Literal(u64),
    Fixed(u32),
    Vbr(u32),
    Array,
    Char6,
    Blob,
}

/// An abbreviation: a record shape.
pub type Abbrev = Vec<AbbrevOp>;

/// A record: code, operands (arrays flattened), and a blob if it has one.
#[derive(Clone, Debug, Default)]
pub struct Record {
    pub code: u32,
    pub ops: Vec<u64>,
    pub blob: Option<Vec<u8>>,
}

/// A block's contents in order.
#[derive(Clone, Debug)]
pub enum Item {
    Record(Record),
    Block(Block),
}

/// A block: its id and everything in it.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub id: u32,
    pub items: Vec<Item>,
}

impl Block {
    /// The records directly in this block.
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.items.iter().filter_map(|i| match i {
            Item::Record(r) => Some(r),
            Item::Block(_) => None,
        })
    }
    /// The sub-blocks directly in this block.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.items.iter().filter_map(|i| match i {
            Item::Block(b) => Some(b),
            Item::Record(_) => None,
        })
    }
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn read(&mut self, n: u32) -> Result<u64, Error> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 || self.pos + n as usize > self.data.len() * 8 {
            return Err(Error::Eof);
        }
        let mut v = 0u64;
        let mut got = 0u32;
        while got < n {
            let byte = self.data[self.pos / 8];
            let off = (self.pos % 8) as u32;
            let take = (8 - off).min(n - got);
            let bits = (u64::from(byte) >> off) & ((1u64 << take) - 1);
            v |= bits << got;
            got += take;
            self.pos += take as usize;
        }
        Ok(v)
    }

    fn vbr(&mut self, n: u32) -> Result<u64, Error> {
        if n < 2 {
            return Err(Error::Bad("vbr width"));
        }
        let hi = 1u64 << (n - 1);
        let mut v = 0u64;
        let mut shift = 0u32;
        loop {
            let piece = self.read(n)?;
            if shift < 64 {
                v |= (piece & (hi - 1)) << shift;
            }
            if piece & hi == 0 {
                return Ok(v);
            }
            shift += n - 1;
            if shift > 70 {
                return Err(Error::Bad("vbr too long"));
            }
        }
    }

    fn align32(&mut self) {
        self.pos = (self.pos + 31) & !31;
    }
}

const CHAR6: &[u8; 64] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._";

struct Parser<'a> {
    bits: Bits<'a>,
    info: Vec<(u32, Vec<Abbrev>)>,
}

impl Parser<'_> {
    fn info_for(&self, id: u32) -> Vec<Abbrev> {
        self.info
            .iter()
            .find(|(b, _)| *b == id)
            .map(|(_, a)| a.clone())
            .unwrap_or_else(Vec::new)
    }

    fn define_abbrev(&mut self) -> Result<Abbrev, Error> {
        let n = self.bits.vbr(5)? as usize;
        let mut ops = Vec::with_capacity(n);
        let mut i = 0;
        while i < n {
            if self.bits.read(1)? == 1 {
                ops.push(AbbrevOp::Literal(self.bits.vbr(8)?));
            } else {
                match self.bits.read(3)? {
                    1 => ops.push(AbbrevOp::Fixed(self.bits.vbr(5)? as u32)),
                    2 => ops.push(AbbrevOp::Vbr(self.bits.vbr(5)? as u32)),
                    3 => ops.push(AbbrevOp::Array),
                    4 => ops.push(AbbrevOp::Char6),
                    5 => ops.push(AbbrevOp::Blob),
                    _ => return Err(Error::Bad("abbrev encoding")),
                }
            }
            i += 1;
        }
        Ok(ops)
    }

    fn scalar(&mut self, op: AbbrevOp) -> Result<u64, Error> {
        match op {
            AbbrevOp::Literal(v) => Ok(v),
            AbbrevOp::Fixed(w) => self.bits.read(w),
            AbbrevOp::Vbr(w) => {
                if w == 0 {
                    Ok(0)
                } else {
                    self.bits.vbr(w)
                }
            }
            AbbrevOp::Char6 => Ok(u64::from(CHAR6[self.bits.read(6)? as usize])),
            AbbrevOp::Array | AbbrevOp::Blob => Err(Error::Bad("array or blob as scalar")),
        }
    }

    fn abbreviated(&mut self, ab: &Abbrev) -> Result<Record, Error> {
        let mut vals: Vec<u64> = Vec::new();
        let mut blob = None;
        let mut i = 0;
        while i < ab.len() {
            match ab[i] {
                AbbrevOp::Array => {
                    let elt = *ab.get(i + 1).ok_or(Error::Bad("array without element"))?;
                    let n = self.bits.vbr(6)? as usize;
                    for _ in 0..n {
                        vals.push(self.scalar(elt)?);
                    }
                    i += 1;
                }
                AbbrevOp::Blob => {
                    let n = self.bits.vbr(6)? as usize;
                    self.bits.align32();
                    let start = self.bits.pos / 8;
                    let bytes = self
                        .bits
                        .data
                        .get(start..start + n)
                        .ok_or(Error::Eof)?
                        .to_vec();
                    self.bits.pos += n * 8;
                    self.bits.align32();
                    blob = Some(bytes);
                }
                op => vals.push(self.scalar(op)?),
            }
            i += 1;
        }
        if vals.is_empty() {
            return Err(Error::Bad("record without code"));
        }
        let code = vals.remove(0) as u32;
        Ok(Record {
            code,
            ops: vals,
            blob,
        })
    }

    /// Reads a block's body; the ENTER_SUBBLOCK header is already consumed.
    fn block(&mut self, id: u32, width: u32) -> Result<Block, Error> {
        let mut out = Block {
            id,
            items: Vec::new(),
        };
        let mut abbrevs = self.info_for(id);
        let mut cur_info: Option<u32> = None;
        loop {
            let a = self.bits.read(width)?;
            match a {
                0 => {
                    self.bits.align32();
                    return Ok(out);
                }
                1 => {
                    let bid = self.bits.vbr(8)? as u32;
                    let w = self.bits.vbr(4)? as u32;
                    self.bits.align32();
                    let _len = self.bits.read(32)?;
                    let b = self.block(bid, w)?;
                    out.items.push(Item::Block(b));
                }
                2 => {
                    let ab = self.define_abbrev()?;
                    if id == 0 {
                        let bid = cur_info.ok_or(Error::Bad("abbrev before SETBID"))?;
                        match self.info.iter_mut().find(|(b, _)| *b == bid) {
                            Some((_, v)) => v.push(ab),
                            None => self.info.push((bid, alloc::vec![ab])),
                        }
                    } else {
                        abbrevs.push(ab);
                    }
                }
                3 => {
                    let code = self.bits.vbr(6)? as u32;
                    let n = self.bits.vbr(6)? as usize;
                    let mut ops = Vec::with_capacity(n.min(1 << 16));
                    for _ in 0..n {
                        ops.push(self.bits.vbr(6)?);
                    }
                    if id == 0 && code == 1 {
                        cur_info = ops.first().map(|&v| v as u32);
                    }
                    out.items.push(Item::Record(Record {
                        code,
                        ops,
                        blob: None,
                    }));
                }
                k => {
                    let ab = abbrevs
                        .get((k - 4) as usize)
                        .ok_or(Error::Bad("unknown abbreviation id"))?
                        .clone();
                    let r = self.abbreviated(&ab)?;
                    out.items.push(Item::Record(r));
                }
            }
        }
    }
}

/// Reads the whole stream (after the `BC` 0xC0DE magic) into a pseudo block of
/// id `u32::MAX` holding the top-level blocks.
pub fn parse(bitcode: &[u8]) -> Result<Block, Error> {
    if bitcode.len() < 8 || bitcode[..4] != [b'B', b'C', 0xC0, 0xDE] {
        return Err(Error::Bad("not bitcode"));
    }
    let mut p = Parser {
        bits: Bits {
            data: bitcode,
            pos: 32,
        },
        info: Vec::new(),
    };
    let mut top = Block {
        id: u32::MAX,
        items: Vec::new(),
    };
    while p.bits.pos + 32 <= bitcode.len() * 8 {
        let a = p.bits.read(2)?;
        if a != 1 {
            return Err(Error::Bad("top level is not a block"));
        }
        let bid = p.bits.vbr(8)? as u32;
        let w = p.bits.vbr(4)? as u32;
        p.bits.align32();
        let _len = p.bits.read(32)?;
        let b = p.block(bid, w)?;
        top.items.push(Item::Block(b));
    }
    Ok(top)
}

/// The text of a record's operands from `from` on, as characters.
#[must_use]
pub fn text(ops: &[u64], from: usize) -> String {
    ops.iter()
        .skip(from)
        .map(|&c| char::from_u32(c as u32).unwrap_or('?'))
        .collect()
}

/// The bytes after the `DXIL` part's program header: the offset and size of
/// the bitcode inside the part, from the header at its start.
///
/// The part is `u32 program version, u32 size in dwords, u32 'DXIL', u32
/// dxil version, u32 bitcode offset (from the magic's address), u32 bitcode
/// size`, then the bitcode.
pub fn bitcode_of_part(part: &[u8]) -> Result<&[u8], Error> {
    let g = |o: usize| {
        part.get(o..o + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or(Error::Eof)
    };
    if g(8)? != u32::from_le_bytes(*b"DXIL") {
        return Err(Error::Bad("no DXIL program header"));
    }
    let off = 8 + g(16)? as usize;
    let size = g(20)? as usize;
    part.get(off..off + size).ok_or(Error::Eof)
}
