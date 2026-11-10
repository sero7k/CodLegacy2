//! Call of Duty: Ghosts (IW6 MP) compiled GSC: the script file layout, a bytecode decoder and a
//! text disassembler. No game code runs here; it only reads the two blocks of a `ScriptFile`
//! asset (see `docs/exe_notes/IW6_GSC.md` for the layout and the opcode table).
//!
//! ```text
//! ScriptFile { name, buffer (zlib; inflates to the "stream"), bytecode }
//! stream   = for each function, in order: i32 body length, name token (u16, or 0 + text),
//!            then the strings its instructions consume, in instruction order.
//! bytecode = one filler byte (always `end`), then the bodies back to back.
//! ```
//!
//! Names (functions, fields, files) are 16-bit ids into the engine's built-in string table, or
//! inline text. The table itself is not in the scripts, so ids stay ids unless a [`NameTable`]
//! names them.

pub mod builtins;
mod decode;
mod disasm;
mod names;
pub mod opcodes;

pub use decode::{
    Case, CaseValue, Decoder, Error, Event, Function, FunctionHeader, Instr, Name, Operand,
    Problem, Script, ScriptBytes,
};
pub use disasm::{disassemble, disassemble_with, render};
pub use names::NameTable;

#[cfg(test)]
mod tests;
