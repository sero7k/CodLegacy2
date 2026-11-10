//! The opcode table of Ghosts' (IW6 MP) compiled GSC: one entry per byte value the script VM
//! implements (the rest are no-ops the VM skips and no shipped script uses).
//!
//! Operand lengths are READ from the engine's script loader (which walks every instruction of a
//! script once, to patch string ids and far calls). Mnemonics and stack effects are INFERRED from
//! the VM's behaviour for each opcode; see `docs/exe_notes/IW6_GSC.md`.

/// How an opcode's operand bytes (after the opcode byte) are laid out, and where its stream
/// strings (if any) come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// No operand.
    None,
    U8,
    U16,
    I32,
    F32,
    /// Three f32.
    Vec3,
    /// A u16 name token. 0xFFFF stands for "the name is inline": the string comes from the stream
    /// (stream form: `00 00` then the NUL-terminated name).
    Token,
    /// A u32 string reference (a placeholder in the file); the text is the next NUL-terminated
    /// string of the stream.
    Str,
    /// 8 placeholder bytes; the stream holds the anim tree name (may be empty) and the animation
    /// name, both NUL-terminated.
    Anim,
    /// A u8 (the tree number) and the tree name from the stream.
    AnimTree,
    /// A u16 builtin id (functions below 0x8000, methods from 0x8000).
    BuiltinId,
    /// A u8 argument count and a u16 builtin id.
    BuiltinArgc,
    /// A u16 forward distance, counted from the end of the instruction.
    JumpFwd,
    /// A u16 backward distance, counted from the end of the instruction.
    JumpBack,
    /// An i32 distance from the end of the instruction.
    Jump32,
    /// An i32 distance from the end of the instruction to the operand of a `switch_table`.
    Switch,
    /// A u16 count, then count * (u32 case value, 3-byte jump). Case values below 0x40000 are
    /// strings taken from the stream; the others are `0x800000 + integer`.
    SwitchTable,
    /// A 3-byte placeholder (the loader patches it) and a (file, function) pair of name tokens
    /// from the stream.
    CallFar,
    /// As `CallFar`, then a u8 argument count.
    CallFarArgc,
    /// A 3-byte call distance (x4, signed) from the start of the operand to the function.
    CallLocal,
    /// As `CallLocal`, then a u8 argument count.
    CallLocalArgc,
}

impl Layout {
    /// Bytes after the opcode byte (None for the variable-length switch table).
    pub const fn operand_len(self) -> Option<usize> {
        Some(match self {
            Layout::None => 0,
            Layout::U8 | Layout::AnimTree => 1,
            Layout::U16 | Layout::Token | Layout::BuiltinId | Layout::JumpFwd | Layout::JumpBack => 2,
            Layout::BuiltinArgc | Layout::CallFar | Layout::CallLocal => 3,
            Layout::I32 | Layout::F32 | Layout::Str | Layout::Jump32 | Layout::Switch => 4,
            Layout::CallFarArgc | Layout::CallLocalArgc => 4,
            Layout::Anim => 8,
            Layout::Vec3 => 12,
            Layout::SwitchTable => return Option::None,
        })
    }
}

/// One opcode.
#[derive(Clone, Copy, Debug)]
pub struct OpInfo {
    pub code: u8,
    pub mnemonic: &'static str,
    pub layout: Layout,
    /// Stack effect, `before -> after` (top of stack last).
    pub stack: &'static str,
    pub desc: &'static str,
}

macro_rules! ops {
    ($( $code:literal $name:literal $layout:ident [$stack:literal] $desc:literal; )*) => {
        /// Every implemented opcode, by value.
        pub static OPS: &[OpInfo] = &[
            $( OpInfo { code: $code, mnemonic: $name, layout: Layout::$layout, stack: $stack, desc: $desc }, )*
        ];
    };
}

ops! {
    0x17 "init_local" U8 ["v ->"] "create a local named u8 (its creation number) and assign v to it";
    0x18 "get_self_field" Token ["-> v"] "push self.<token>";
    0x19 "return" None ["v ->"] "return v from the function";
    0x1a "call_builtin0" BuiltinId ["-> r"] "builtin function id, 0 arguments";
    0x1b "call_builtin1" BuiltinId ["a -> r"] "builtin function id, 1 argument";
    0x1c "call_builtin2" BuiltinId ["a b -> r"] "builtin function id, 2 arguments";
    0x1d "call_builtin3" BuiltinId ["a b c -> r"] "builtin function id, 3 arguments";
    0x1e "call_builtin4" BuiltinId ["a b c d -> r"] "builtin function id, 4 arguments";
    0x1f "call_builtin5" BuiltinId ["a..e -> r"] "builtin function id, 5 arguments";
    0x20 "call_builtin" BuiltinArgc ["args -> r"] "builtin function id, u8 arguments";
    0x21 "bool_not" None ["v -> !v"] "logical not";
    0x22 "thread_method_far" CallFarArgc ["args obj -> r"] "obj thread file::func(args)";
    0x23 "jump_true_expr" JumpFwd ["v -> v | "] "short-circuit ||: jump forward keeping v when true, else pop";
    0x24 "set_level_field" Token ["v ->"] "level.<token> = v";
    0x25 "cast_bool" None ["v -> b"] "convert to a condition value";
    0x26 "new_local_array_ref" U8 ["key ->"] "create a local named u8, then as ref_array_local0 (rare)";
    0x27 "call_builtin_ptr" U8 ["fn args -> r"] "call a builtin function pointer, u8 arguments";
    0x28 "inequality" None ["a b -> a!=b"] "!=";
    0x29 "get_thisthread" None ["-> t"] "push the running thread object";
    0x2a "clear_field" Token ["->"] "<object register>.<token> = undefined (removes the field)";
    0x2b "get_float" F32 ["-> f"] "push float";
    0x2c "param" U8 ["arg ->"] "declare a parameter local named u8 and take the next argument (first argument first; undefined when none left)";
    0x2d "call0_far" CallFar ["-> r"] "file::func() with no arguments (pushes its own call marker)";
    0x2e "call_far" CallFar ["marker args -> r"] "file::func(args) after pre_call";
    0x2f "childthread_far" CallFarArgc ["args -> r"] "childthread file::func(args)";
    0x30 "clear_local0" None ["->"] "clear the local at depth 0 (the newest)";
    0x31 "clear_local" U8 ["->"] "clear the local at depth u8 (0 = newest)";
    0x32 "check_params" None ["->"] "end of the parameter block: the call marker must be next; becomes the frame base";
    0x33 "cast_obj" None ["obj ->"] "make the popped object the field-access object register";
    0x34 "end" None ["->"] "return undefined (function end)";
    0x35 "size" None ["v -> n"] "v.size";
    0x36 "empty_array" None ["-> a"] "push a new empty array";
    0x37 "bit_and" None ["a b -> a&b"] "&";
    0x38 "less_equal" None ["a b -> a<=b"] "<=";
    0x39 "pre_call_b" None ["-> marker"] "push a call marker (same as pre_call)";
    0x3a "thread_method_ptr" U8 ["args fn obj -> r"] "obj thread [[fn]](args), u8 arguments";
    0x3b "switch_table" SwitchTable ["->"] "case table of a switch; skipped by execution (reached through switch)";
    0x3d "divide" None ["a b -> a/b"] "/";
    0x3e "childthread_method_far" CallFarArgc ["args obj -> r"] "obj childthread file::func(args)";
    0x3f "get_ushort" U16 ["-> n"] "push u16 as integer";
    0x40 "jump_true" JumpFwd ["v ->"] "pop; jump forward if true";
    0x41 "get_self" None ["-> self"] "push self";
    0x42 "thread_far" CallFarArgc ["args -> r"] "thread file::func(args)";
    0x43 "thread_local" CallLocalArgc ["args -> r"] "thread func(args), a function of this file";
    0x44 "set_local0" None ["v ->"] "assign v to the local at depth 0 (the newest)";
    0x45 "set_local" U8 ["v ->"] "assign v to the local at depth u8 (0 = newest)";
    0x46 "plus" None ["a b -> a+b"] "+ (numbers, vectors, string concatenation)";
    0x47 "bit_not" None ["v -> ~v"] "~ (never emitted)";
    0x48 "call_method_ptr" None ["marker args fn obj -> r"] "obj [[fn]](args)";
    0x49 "inc" None ["->"] "++ on the referenced variable";
    0x4a "free_locals" U8 ["->"] "drop the newest u8 locals";
    0x4b "jump_false_expr" JumpFwd ["v -> v | "] "short-circuit &&: jump forward keeping v when false, else pop";
    0x4c "switch" Switch ["v ->"] "pop v; jump to the matching case of the table (default entry when none)";
    0x4d "clear_params" None ["..marker ->"] "pop everything down to the marker (after waittill / notify arguments)";
    0x4e "ref_local0" None ["->"] "variable register = the local at depth 0 (the newest)";
    0x4f "ref_local" U8 ["->"] "variable register = the local at depth u8 (0 = newest)";
    0x50 "call_method_local" CallLocal ["marker args obj -> r"] "obj func(args), function of this file";
    0x51 "get_field" Token ["-> v"] "push <object register>.<token>";
    0x52 "ref_field" Token ["->"] "variable register = <object register>.<token>";
    0x53 "get_string" Str ["-> s"] "push string literal";
    0x54 "call_ptr" None ["marker args fn -> r"] "[[fn]](args)";
    0x55 "get_level_field" Token ["-> v"] "push level.<token>";
    0x56 "get_vector" Vec3 ["-> v"] "push vector literal";
    0x57 "endon" None ["name obj ->"] "obj endon(name)";
    0x58 "greater_equal" None ["a b -> a>=b"] ">=";
    0x59 "self_obj" None ["->"] "object register = self";
    0x5a "set_anim_field" Token ["v ->"] "anim.<token> = v";
    0x5b "set_variable_field" None ["v ->"] "assign v to the variable register (undefined removes a field)";
    0x5c "call0_local" CallLocal ["-> r"] "func() with no arguments, function of this file (pushes its own call marker)";
    0x5d "call_local" CallLocal ["marker args -> r"] "func(args), function of this file, after pre_call";
    0x5e "ref_array_local0" None ["key ->"] "variable register = (local at depth 0)[key]";
    0x5f "ref_array_local" U8 ["key ->"] "variable register = (local at depth u8)[key]";
    0x60 "get_function_far" CallFar ["-> fn"] "push &file::func";
    0x61 "less" None ["a b -> a<b"] "<";
    0x62 "ref_game" None ["->"] "variable register = game";
    0x63 "wait_frame_end" None ["->"] "waittillframeend";
    0x64 "set_param0" None ["arg ->"] "depth-0 local = next argument unless there is none (never emitted)";
    0x65 "set_param" U8 ["arg ->"] "depth-u8 local = next argument unless there is none (never emitted)";
    0x66 "childthread_method_ptr" U8 ["args fn obj -> r"] "obj childthread [[fn]](args), u8 arguments";
    0x67 "get_level" None ["-> level"] "push level";
    0x68 "notify" None ["marker args name obj ->"] "obj notify(name, args)";
    0x69 "dec_top" None ["v ->"] "pop and discard";
    0x6a "shift_left" None ["a b -> a<<b"] "<<";
    0x6b "thread_method_local" CallLocalArgc ["args obj -> r"] "obj thread func(args), function of this file";
    0x6c "childthread_method_local" CallLocalArgc ["args obj -> r"] "obj childthread func(args), function of this file";
    0x6d "greater" None ["a b -> a>b"] ">";
    0x6e "get_local0" None ["-> v"] "push the local at depth 0 (the newest)";
    0x6f "get_local1" None ["-> v"] "push the local at depth 1";
    0x70 "get_local2" None ["-> v"] "push the local at depth 2";
    0x71 "get_local3" None ["-> v"] "push the local at depth 3";
    0x72 "get_local4" None ["-> v"] "push the local at depth 4";
    0x73 "get_local5" None ["-> v"] "push the local at depth 5";
    0x74 "get_local" U8 ["-> v"] "push the local at depth u8 (0 = newest)";
    0x75 "set_waittill_arg" U8 ["->"] "assign the next waittill argument to the local at depth u8 (undefined when none left)";
    0x76 "jump" Jump32 ["->"] "jump (i32 distance, either direction)";
    0x77 "thread_ptr" U8 ["args fn -> r"] "thread [[fn]](args), u8 arguments";
    0x78 "get_zero" None ["-> 0"] "push integer 0";
    0x79 "wait" None ["secs ->"] "wait secs";
    0x7a "minus" None ["a b -> a-b"] "-";
    0x7b "set_self_field" Token ["v ->"] "self.<token> = v";
    0x7d "multiply" None ["a b -> a*b"] "*";
    0x7e "new_local" U8 ["->"] "create a local named u8, undefined";
    0x7f "childthread_local" CallLocalArgc ["args -> r"] "childthread func(args), function of this file";
    0x80 "get_int" I32 ["-> n"] "push i32 as integer";
    0x81 "modulo" None ["a b -> a%b"] "%";
    0x82 "ref_anim_field" Token ["->"] "variable register = anim.<token>";
    0x83 "get_builtin_function" BuiltinId ["-> fn"] "push a builtin function pointer";
    0x84 "get_game" None ["-> g"] "push game";
    0x85 "waittill" None ["name obj ->"] "obj waittill(name, ...): suspends; set_waittill_arg ops follow";
    0x86 "dec" None ["->"] "-- on the referenced variable";
    0x87 "local_obj" U8 ["->"] "object register = the local at depth u8 (must be an object)";
    0x88 "pre_call" None ["-> marker"] "push a call marker; arguments follow";
    0x89 "get_anim" None ["-> anim"] "push anim";
    0x8a "get_undefined" None ["-> undefined"] "push undefined";
    0x8b "ref_level_field" Token ["->"] "variable register = level.<token>";
    0x8c "anim_obj" None ["->"] "object register = anim";
    0x8d "level_obj" None ["->"] "object register = level";
    0x8e "bit_xor" None ["a b -> a^b"] "^";
    0x8f "equality" None ["a b -> a==b"] "==";
    0x90 "clear_array_elem" None ["key ->"] "remove the element key of the array in the variable register";
    0x91 "jump_back" JumpBack ["->"] "jump backward (loop)";
    0x92 "get_animation" Anim ["-> a"] "push %animation";
    0x93 "get_anim_field" Token ["-> v"] "push anim.<token>";
    0x94 "get_animtree" AnimTree ["-> n"] "push the anim tree number (#using_animtree)";
    0x95 "get_istring" Str ["-> s"] "push localized string literal";
    0x96 "ref_array" None ["arr key ->"] "variable register = arr[key]";
    0x97 "ref_self_field" Token ["->"] "variable register = self.<token>";
    0x98 "get_neg_byte" U8 ["-> n"] "push -u8 as integer";
    0x99 "get_builtin_method" BuiltinId ["-> fn"] "push a builtin method pointer";
    0x9a "call_builtin_method_ptr" U8 ["args fn obj -> r"] "call a builtin method pointer, u8 arguments";
    0x9b "get_array_elem" None ["arr key -> v"] "arr[key]";
    0x9c "make_vector" None ["x y z -> v"] "(x, y, z)";
    0x9d "call_method_far" CallFar ["marker args obj -> r"] "obj file::func(args) after pre_call";
    0x9e "get_local_array_elem" U8 ["key -> v"] "(local at depth u8)[key]";
    0x9f "get_byte" U8 ["-> n"] "push u8 as integer";
    0xa0 "childthread_ptr" U8 ["args fn -> r"] "childthread [[fn]](args), u8 arguments";
    0xa1 "bit_or" None ["a b -> a|b"] "|";
    0xa2 "array_push" None ["arr v -> arr"] "arr[arr.size] = v (array literals)";
    0xa4 "waittillmatch" U16 ["match name obj ->"] "obj waittillmatch(name, match); the u16 is not read by the VM";
    0xa5 "get_function" CallLocal ["-> fn"] "push &func, a function of this file";
    0xa6 "get_neg_ushort" U16 ["-> n"] "push -u16 as integer";
    0xa7 "shift_right" None ["a b -> a>>b"] ">>";
    0xa8 "call_builtin_method0" BuiltinId ["obj -> r"] "builtin method id, 0 arguments";
    0xa9 "call_builtin_method1" BuiltinId ["a obj -> r"] "builtin method id, 1 argument";
    0xaa "call_builtin_method2" BuiltinId ["a b obj -> r"] "builtin method id, 2 arguments";
    0xab "call_builtin_method3" BuiltinId ["a b c obj -> r"] "builtin method id, 3 arguments";
    0xac "call_builtin_method4" BuiltinId ["a..d obj -> r"] "builtin method id, 4 arguments";
    0xad "call_builtin_method5" BuiltinId ["a..e obj -> r"] "builtin method id, 5 arguments";
    0xae "call_builtin_method" BuiltinArgc ["args obj -> r"] "builtin method id, u8 arguments";
    0xaf "jump_false" JumpFwd ["v ->"] "pop; jump forward if false";
}

/// The definition of opcode `op`, `None` for the values the VM treats as no-ops.
pub fn info(op: u8) -> Option<&'static OpInfo> {
    OPS.binary_search_by_key(&op, |o| o.code).ok().map(|i| &OPS[i])
}

/// The name of an opcode (`unknown_XX` for no-op values).
pub fn mnemonic(op: u8) -> &'static str {
    info(op).map_or("unknown", |o| o.mnemonic)
}
