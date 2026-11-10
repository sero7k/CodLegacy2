//! An engine-independent script virtual machine for Call of Duty: Ghosts (IW6 MP) compiled GSC.
//!
//! It runs the bytecode `gsc_iw6` decodes: threads with the engine's scheduling order, `wait`,
//! `waittill`/`notify`/`endon`, arrays and structs by reference, the engine's operator rules.
//! The game is reached through the [`Host`] trait (builtin functions and methods, entity
//! fields). Names are ids: functions, fields and files stay `#N` unless the host knows more.
//! See `docs/exe_notes/IW6_GSC.md` ("VM semantics") for what was read from the engine and what
//! is inferred.

mod analysis;
mod census;
mod host;
pub mod ops;
mod scripts;
mod value;
mod vm;

pub use analysis::{builtin_uses, builtin_uses_within, literal_args, reachable, string_literals, BuiltinUse};
pub use census::Census;
pub use gsc_iw6::Name;
pub use host::{Action, CallCtx, Host, HostReturn, NullHost};
pub use scripts::{file_name, Prepared, ScriptSet};
pub use value::{fmt_g, fmt_vector, AnimRef, ArrRef, EntityId, FuncRef, Key, ObjKind, ObjRef, Value};
pub use vm::{
    CoreIds, ScriptError, StepReport, ThreadId, ThreadInfo, ThreadState, TraceEvent, Vm, MAX_DEPTH, TICK_MS,
};

#[cfg(test)]
mod tests;
