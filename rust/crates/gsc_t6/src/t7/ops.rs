//! The operations of Black Ops III's script VM.
//!
//! The VM reads a 16-bit opcode (2-aligned) and runs `table[opcode]` for
//! opcodes below 0x2000. The table is static in `BlackOps3.exe`
//! (`.rdata` 0x142eb38b0) and maps the 8192 values onto 126 handlers, so
//! every operation has many opcode values; [`SLOTS`] is that table with
//! each handler named by what it does (`gen/t7gen.py`, which lists the
//! handler addresses). Slots 0x00..0x0b are debugger operations and, like
//! unused slots, [`T7Op::INVALID`] here.
//!
//! bo2mp: the VM clears bit 13 (0x2000) of the opcode it reads; with the
//! bit set the rest indexes a second table of the same handlers in another
//! order ([`SLOTS_HI`]). Retail scripts never set it; the mod tools'
//! linker does, so every Steam Workshop map's own scripts use both
//! (`BO2MP_T7_LOW_OPCODES=1`: values 0x2000 and up invalid, as before).

#![allow(non_camel_case_types)]

pub use super::gen_slots::{SLOTS, SLOTS_HI};

/// An operation (names after Black Ops II's opcodes where it has one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum T7Op {
    INVALID,
    END,
    RETURN,
    GET_UNDEFINED,
    /// A second handler that pushes `undefined` (purpose unknown).
    GET_UNDEFINED_2,
    GET_ZERO,
    GET_BYTE,
    GET_NEG_BYTE,
    GET_UNSIGNED_SHORT,
    GET_NEG_UNSIGNED_SHORT,
    GET_INTEGER,
    /// Push an 8-byte constant (value type 9).
    GET_UINT64,
    GET_FLOAT,
    GET_STRING,
    GET_ISTRING,
    GET_VECTOR,
    GET_HASH,
    /// `level`'s field object / value.
    GET_LEVEL_OBJECT,
    GET_LEVEL,
    /// The two object registers not yet told apart (`anim`, `world`).
    GET_OBJECT_834,
    GET_834,
    GET_OBJECT_844,
    GET_844,
    GET_CLASSES_OBJECT,
    GET_CLASSES,
    GET_GAME,
    GET_GAME_REF,
    GET_SELF,
    GET_SELF_OBJECT,
    GET_ANIMATION,
    GET_FUNCTION,
    GET_API_FUNCTION,
    /// Skips an 8-byte operand and pushes `undefined`.
    GET_UNRESOLVED_FUNCTION,
    SAFE_CREATE_LOCAL_VARIABLES,
    EVAL_LOCAL_VARIABLE_CACHED,
    EVAL_LOCAL_VARIABLE_REF_CACHED,
    SAFE_SET_VARIABLE_FIELD_CACHED,
    SAFE_SET_WAITTILL_VARIABLE_FIELD_CACHED,
    EVAL_ARRAY,
    EVAL_ARRAY_REF,
    EMPTY_ARRAY,
    EVAL_FIELD_VARIABLE,
    EVAL_FIELD_VARIABLE_REF,
    EVAL_SELF_FIELD_VARIABLE,
    EVAL_SELF_FIELD_VARIABLE_REF,
    EVAL_LEVEL_FIELD_VARIABLE,
    EVAL_LEVEL_FIELD_VARIABLE_REF,
    CLEAR_FIELD_VARIABLE,
    SET_VARIABLE_FIELD,
    /// `ref[key] = undefined`: pops the key, clears the element of the
    /// array the reference names.
    CLEAR_ARRAY,
    CLEAR_PARAMS,
    CHECK_CLEAR_PARAMS,
    PRE_SCRIPT_CALL,
    CALL_BUILTIN,
    CALL_BUILTIN_METHOD,
    SCRIPT_FUNCTION_CALL,
    SCRIPT_FUNCTION_CALL_POINTER,
    SCRIPT_METHOD_CALL,
    SCRIPT_METHOD_CALL_POINTER,
    SCRIPT_THREAD_CALL,
    SCRIPT_THREAD_CALL_POINTER,
    SCRIPT_METHOD_THREAD_CALL,
    SCRIPT_METHOD_THREAD_CALL_POINTER,
    CLASS_FUNCTION_CALL,
    CLASS_FUNCTION_THREAD_CALL,
    NEW,
    DEC_TOP,
    SAFE_DEC_TOP,
    CAST_FIELD_OBJECT,
    CAST_BOOL,
    BOOL_NOT,
    BOOL_COMPLEMENT,
    JUMP_ON_FALSE,
    JUMP_ON_TRUE,
    JUMP_ON_FALSE_EXPR,
    JUMP_ON_TRUE_EXPR,
    JUMP,
    DEVBLOCK_BEGIN,
    INC,
    DEC,
    BIT_OR,
    BIT_XOR,
    BIT_AND,
    EQUAL,
    NOT_EQUAL,
    SUPER_EQUAL,
    SUPER_NOT_EQUAL,
    LESS_THAN,
    GREATER_THAN,
    LESS_THAN_OR_EQUAL,
    GREATER_THAN_OR_EQUAL,
    SHIFT_LEFT,
    SHIFT_RIGHT,
    PLUS,
    MINUS,
    MULTIPLY,
    DIVIDE,
    MODULUS,
    SIZE_OF,
    IS_DEFINED,
    VECTOR,
    VECTOR_CONSTANT,
    VECTOR_SCALE,
    SWITCH,
    END_SWITCH,
    FIRST_ARRAY_KEY,
    NEXT_ARRAY_KEY,
    GET_TIME,
    WAIT,
    REAL_WAIT,
    WAIT_TILL_FRAME_END,
    /// `waittillmatch`: a u8 operand (the match count, read when the
    /// thread resumes; the handler itself is `WAIT_TILL`'s).
    WAIT_TILL_MATCH,
    WAIT_TILL,
    NOTIFY,
    END_ON,
}

/// The operation an opcode value runs.
pub fn op_of(code: u16) -> T7Op {
    static LOW_ONLY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let code = usize::from(code);
    match code {
        0..0x2000 => SLOTS[code],
        0x2000..0x4000 if !*LOW_ONLY.get_or_init(|| std::env::var("BO2MP_T7_LOW_OPCODES").is_ok_and(|v| v.trim() == "1")) => SLOTS_HI[code - 0x2000],
        _ => T7Op::INVALID,
    }
}
