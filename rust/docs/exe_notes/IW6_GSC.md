# Ghosts (IW6 MP) compiled GSC: file layout and opcode table

Implemented by `crates/gsc_iw6` (decoder, `Script::parse`, `disassemble`, examples `gsc_dis`, `gsc_check`, `gsc_ops`, `gsc_extract`).
Read from the Ghosts decompile (exe notes; addresses allowed in this file only) and verified by decoding the real zones.
[I] marks an item whose meaning is INFERRED from usage, not read directly.

## Exe locations (static analysis only)
- Loader pre-pass (operand lengths, stream consumption): 0x1404378c0. VM loop: 0x14043a4c0. Linker: 0x14042e150 (helpers 0x14042df70, 0x14042e3a0).
- Scr_RegisterFunction: 0x14042e330. Builtin function slots 0x1446b77a0 (ids 1..0x25d), method slots 0x1446b8a90 (id - 0x8000).
- Builtin registration table 0x1409e86b0 (0x9e entries of 24 bytes). It holds handler pointers only; the NAMES are not in the exe.
- SL_GetCanonicalString 0x14042e4f0 returns slot + 0x95a1, so token ids >= 0x95a2 are dynamic strings.
- Token / name reads from the stream: 0x140439220 and 0x140439180. childthread helper 0x140432720.

## ScriptFile asset
`{name, compressedLen, len, bytecodeLen, buffer, bytecode}`. `buffer` is zlib and inflates to the **stream** (`len` bytes); `bytecode` is the **code** (`bytecodeLen` bytes).
Script names are decimal file ids ("1399") except a few real names (`maps/mp/mp_frag`, `animscripts/*`). Function, field and file names are ids into the engine string table, which is not in the scripts or staged data (only 20445 = `main` is inferred).

## Bytecode (code) layout
- Byte 0 is a filler `end` (0x34). The first function body starts at offset 1; bodies are contiguous and each ends with `end`/`return`.
- 3-byte local-call / function-ref field: target = operand_start + (sext24(field) >> 2). Far-call 3-byte fields are placeholders patched by the linker.
- Forward jumps (`jump_false`, `jump_true`, `*_expr`): u16 distance from the end of the instruction. `jump_back`: target = end - u16. `jump` (0x76): signed i32 from the end.
- `switch` (0x4c): i32 from the end, pointing at (table opcode + 1). Table: 0x3b, u16 n, n x (u32 value, rel24 target). Case values >= 0x40000 are ints encoded 0x800000 + n; smaller values are string tokens whose text is in the stream.

## Stream layout (per function, in order)
`i32 body length`, then the name token (`u16` id, or `00 00` + NUL string if inline). Then the strings the body's instructions consume, in instruction order:
- get_string / get_istring: one NUL string. get_animtree: one string. get_animation: two strings (tree, name).
- far calls: a (file token, function token) pair. Any token operand equal to 0xFFFF: `00 00` + string.
- switch_table: one string per case whose value is < 0x40000; the string `\x01` marks the default case.

## VM model notes
- Arguments are pushed last-to-first (first argument on top). `pre_call`/`pre_call_b` push a type-8 marker; `check_params` verifies it and turns it into the type-7 frame base.
- `get_local`/`set_local`/`clear_local` take a **depth** from the newest local (0 = newest). `init_local`, `new_local`, `param` take the variable's own name number (creation order).
- Value types: 0 undef, 1 object, 2 string, 3 istring, 4 vector, 5 float, 6 int, 7 codepos/frame base, 8 precall marker, 9 function pointer, 10 builtin function pointer, 11 builtin method pointer, 12 wait record, 13 animation.
- Waittill: `[name][obj] waittill`, then `new_local`/`set_waittill_arg` pairs, then `clear_params`. wait = round(secs*20) ticks.
- Object register for field reads (`self_obj`, `level_obj`, `anim_obj`, `local_obj`, `cast_obj`); variable register for stores (`ref_*` then `set_variable_field`).
- thread = AllocThread; childthread passes the parent thread id.
- 150 opcodes implemented: 0x17..0xaf except 0x3c, 0x7c, 0xa3. 0x00..0x16 and 0xb0+ are VM no-ops and never occur.

## Builtins
Function ids are < 0x8000 (slots 1..0x25d); method ids are >= 0x8000 (slot = id - 0x8000). No names exist in the exe or scripts; names were recovered from the handler functions and are in `iw6_builtins.tsv` / `IW6_GSC_BUILTINS.md` (254 ids, each marked VERIFIED, LIKELY or GUESS) and `crates/gsc_iw6/data/builtins_iw6.names`. Engine field ids are in `fields_iw6.names`. `gsc_dis` loads both through `NameTable::known()`; extra names can still be supplied with `--names` (lines `token|file|builtin <id> <name>`).
Corrections to earlier inference: 0x8056/0x8057 are connectpaths/disconnectpaths (not hide/show), 0x15e is precachemodel, 0x804a is linkto. Example verified names: 0x2d isdefined, 0x32 setdvar, 0x63 spawn, 0x143 loadfx, 0x188 getent, 0x189 getentarray, 0x1a8 tablelookup; methods 0x80a9 delete, 0x8073 playloopsound, 0x817b gettagorigin, 0x82a3 moveto, 0x82b2 rotateto.

## Identified file ids (INFERRED by content)
927 common_scripts/utility, 922 _exploder, 916 _createfx, 917 createfx menu, 923 _fx, 919 _destructible, 1373 maps/mp/_load, 1357 _compass, 1385 maps/mp/_utility, 1474 maps/mp/_movers, 1399 mp_frag precache, 1398 mp_frag fx, 1310 mp_frag createfx data, 1246/1247 createfx stub/audio, 1209 crane anim, 940 toy_transformer destructible.
927::#12465 (3 params) calls a level function pointer: probably exploder(num). mp_frag's sprinkler function (#31100) calls it with 1.

## Validation (gsc_check)
10 zones: 328 scripts, 10,770 functions, 612,308 instructions, 0 unknown opcodes, 0 problems (every function ends on a boundary, every jump/case target on a boundary, streams fully consumed). 147 distinct opcodes used; unused: 0x47, 0x65, 0x8e.
common_mp, common_core_mp, common_bots_mp hold no scripts. `common` stops at 4819/4871 in the walker (equipment sound table), 147 of its 150 listed scripts decode. A few scripts have no buffer ("no data").

## Opcode table
Columns: stack effect, use count over the corpus, effect.

| op | mnemonic | operands | stack | uses | effect |
|---|---|---|---|---|---|
| 0x17 | `init_local` | u8 | `v ->` | 16225 | create a local named u8 (its creation number) and assign v to it |
| 0x18 | `get_self_field` | u16 token (0xFFFF: inline name in stream) | `-> v` | 15043 | push self.<token> |
| 0x19 | `return` | - | `v ->` | 5895 | return v from the function |
| 0x1a | `call_builtin0` | u16 builtin id | `-> r` | 1428 | builtin function id, 0 arguments |
| 0x1b | `call_builtin1` | u16 builtin id | `a -> r` | 19763 | builtin function id, 1 argument |
| 0x1c | `call_builtin2` | u16 builtin id | `a b -> r` | 5606 | builtin function id, 2 arguments |
| 0x1d | `call_builtin3` | u16 builtin id | `a b c -> r` | 627 | builtin function id, 3 arguments |
| 0x1e | `call_builtin4` | u16 builtin id | `a b c d -> r` | 635 | builtin function id, 4 arguments |
| 0x1f | `call_builtin5` | u16 builtin id | `a..e -> r` | 110 | builtin function id, 5 arguments |
| 0x20 | `call_builtin` | u8 argc, u16 builtin id | `args -> r` | 97 | builtin function id, u8 arguments |
| 0x21 | `bool_not` | - | `v -> !v` | 1991 | logical not |
| 0x22 | `thread_method_far` | 3-byte placeholder, u8 argc; file and function tokens in stream | `args obj -> r` | 519 | obj thread file::func(args) |
| 0x23 | `jump_true_expr` | u16 forward distance | `v -> v` (kept if jumping, popped otherwise) | 1726 | short-circuit or: jump when true keeping the value |
| 0x24 | `set_level_field` | u16 token (0xFFFF: inline name in stream) | `v ->` | 2254 | level.<token> = v |
| 0x25 | `cast_bool` | - | `v -> b` | 6300 | convert to a condition value [I] |
| 0x26 | `new_local_array_ref` | u8 | `key ->` | 78 | create a local named u8, then as ref_array_local0 (rare) [I] |
| 0x27 | `call_builtin_ptr` | u8 | `fn args -> r` | 58 | call a builtin function pointer, u8 arguments |
| 0x28 | `inequality` | - | `a b -> a!=b` | 1446 | != |
| 0x29 | `get_thisthread` | - | `-> t` | 1 | push the running thread object |
| 0x2a | `clear_field` | u16 token (0xFFFF: inline name in stream) | `->` | 1151 | <object register>.<token> = undefined (removes the field) [I] |
| 0x2b | `get_float` | f32 | `-> f` | 4904 | push float |
| 0x2c | `param` | u8 | `arg ->` | 12569 | declare a parameter local named u8 and take the next argument (first argument first; undefined when none left) |
| 0x2d | `call0_far` | 3-byte placeholder; file and function tokens in stream | `-> r` | 2540 | file::func() with no arguments (pushes its own call marker) |
| 0x2e | `call_far` | 3-byte placeholder; file and function tokens in stream | `marker args -> r` | 6725 | file::func(args) after pre_call |
| 0x2f | `childthread_far` | 3-byte placeholder, u8 argc; file and function tokens in stream | `args -> r` | 3 | childthread file::func(args) |
| 0x30 | `clear_local0` | - | `->` | 1522 | clear the local at depth 0 (the newest) [I] |
| 0x31 | `clear_local` | u8 | `->` | 1898 | clear the local at depth u8 (0 = newest) [I] |
| 0x32 | `check_params` | - | `->` | 10770 | end of the parameter block: the call marker must be next; becomes the frame base [I] |
| 0x33 | `cast_obj` | - | `obj ->` | 7912 | make the popped object the field-access object register [I] |
| 0x34 | `end` | - | `->` | 16227 | return undefined (function end) |
| 0x35 | `size` | - | `v -> n` | 3586 | v.size |
| 0x36 | `empty_array` | - | `-> a` | 2514 | push a new empty array |
| 0x37 | `bit_and` | - | `a b -> a&b` | 51 | & |
| 0x38 | `less_equal` | - | `a b -> a<=b` | 344 | <= |
| 0x39 | `pre_call_b` | - | `-> marker` | 1945 | push a call marker (same as pre_call) [I] |
| 0x3a | `thread_method_ptr` | u8 | `args fn obj -> r` | 149 | obj thread [[fn]](args), u8 arguments |
| 0x3b | `switch_table` | u16 n, n x (u32 case, 3-byte jump); strings in stream | `->` | 374 | case table of a switch; skipped by execution (reached through switch) |
| 0x3d | `divide` | - | `a b -> a/b` | 603 | / |
| 0x3e | `childthread_method_far` | 3-byte placeholder, u8 argc; file and function tokens in stream | `args obj -> r` | 2 | obj childthread file::func(args) |
| 0x3f | `get_ushort` | u16 | `-> n` | 1565 | push u16 as integer |
| 0x40 | `jump_true` | u16 forward distance | `v ->` | 4879 | pop; jump forward if true |
| 0x41 | `get_self` | - | `-> self` | 12698 | push self |
| 0x42 | `thread_far` | 3-byte placeholder, u8 argc; file and function tokens in stream | `args -> r` | 604 | thread file::func(args) |
| 0x43 | `thread_local` | 3-byte distance (x4), u8 argc | `args -> r` | 1823 | thread func(args), a function of this file |
| 0x44 | `set_local0` | - | `v ->` | 5773 | assign v to the local at depth 0 (the newest) |
| 0x45 | `set_local` | u8 | `v ->` | 8011 | assign v to the local at depth u8 (0 = newest) |
| 0x46 | `plus` | - | `a b -> a+b` | 4539 | + (numbers, vectors, string concatenation) |
| 0x47 | `bit_not` | - | `v -> ~v` | 0 | ~ (never emitted) [I] |
| 0x48 | `call_method_ptr` | - | `marker args fn obj -> r` | 160 | obj [[fn]](args) |
| 0x49 | `inc` | - | `->` | 1692 | ++ on the referenced variable |
| 0x4a | `free_locals` | u8 | `->` | 1531 | drop the newest u8 locals |
| 0x4b | `jump_false_expr` | u16 forward distance | `v -> v | 4574 | ` |
| 0x4c | `switch` | i32 distance to the table | `v ->` | 374 | pop v; jump to the matching case of the table (default entry when none) |
| 0x4d | `clear_params` | - | `..marker ->` | 1652 | pop everything down to the marker (after waittill / notify arguments) [I] |
| 0x4e | `ref_local0` | - | `->` | 721 | variable register = the local at depth 0 (the newest) |
| 0x4f | `ref_local` | u8 | `->` | 841 | variable register = the local at depth u8 (0 = newest) |
| 0x50 | `call_method_local` | 3-byte distance (x4) | `marker args obj -> r` | 1091 | obj func(args), function of this file |
| 0x51 | `get_field` | u16 token (0xFFFF: inline name in stream) | `-> v` | 15250 | push <object register>.<token> |
| 0x52 | `ref_field` | u16 token (0xFFFF: inline name in stream) | `->` | 8921 | variable register = <object register>.<token> |
| 0x53 | `get_string` | u32 placeholder + string in stream | `-> s` | 51289 | push string literal |
| 0x54 | `call_ptr` | - | `marker args fn -> r` | 293 | [[fn]](args) |
| 0x55 | `get_level_field` | u16 token (0xFFFF: inline name in stream) | `-> v` | 11378 | push level.<token> |
| 0x56 | `get_vector` | 3 x f32 | `-> v` | 2371 | push vector literal |
| 0x57 | `endon` | - | `name obj ->` | 4767 | obj endon(name) |
| 0x58 | `greater_equal` | - | `a b -> a>=b` | 431 | >= |
| 0x59 | `self_obj` | - | `->` | 779 | object register = self |
| 0x5a | `set_anim_field` | u16 token (0xFFFF: inline name in stream) | `v ->` | 176 | anim.<token> = v |
| 0x5b | `set_variable_field` | - | `v ->` | 18186 | assign v to the variable register (undefined removes a field) |
| 0x5c | `call0_local` | 3-byte distance (x4) | `-> r` | 2551 | func() with no arguments, function of this file (pushes its own call marker) |
| 0x5d | `call_local` | 3-byte distance (x4) | `marker args -> r` | 6394 | func(args), function of this file, after pre_call |
| 0x5e | `ref_array_local0` | - | `key ->` | 2141 | variable register = (local at depth 0)[key] |
| 0x5f | `ref_array_local` | u8 | `key ->` | 890 | variable register = (local at depth u8)[key] |
| 0x60 | `get_function_far` | 3-byte placeholder; file and function tokens in stream | `-> fn` | 411 | push &file::func |
| 0x61 | `less` | - | `a b -> a<b` | 2471 | < |
| 0x62 | `ref_game` | - | `->` | 470 | variable register = game [I] |
| 0x63 | `wait_frame_end` | - | `->` | 142 | waittillframeend |
| 0x64 | `set_param0` | - | `arg ->` | 1 | depth-0 local = next argument unless there is none (never emitted) [I] |
| 0x65 | `set_param` | u8 | `arg ->` | 0 | depth-u8 local = next argument unless there is none (never emitted) [I] |
| 0x66 | `childthread_method_ptr` | u8 | `args fn obj -> r` | 1 | obj childthread [[fn]](args), u8 arguments |
| 0x67 | `get_level` | - | `-> level` | 2150 | push level |
| 0x68 | `notify` | - | `marker args name obj ->` | 1945 | obj notify(name, args) |
| 0x69 | `dec_top` | - | `v ->` | 26069 | pop and discard |
| 0x6a | `shift_left` | - | `a b -> a<<b` | 3 | << |
| 0x6b | `thread_method_local` | 3-byte distance (x4), u8 argc | `args obj -> r` | 1299 | obj thread func(args), function of this file |
| 0x6c | `childthread_method_local` | 3-byte distance (x4), u8 argc | `args obj -> r` | 29 | obj childthread func(args), function of this file |
| 0x6d | `greater` | - | `a b -> a>b` | 1506 | > |
| 0x6e | `get_local0` | - | `-> v` | 29074 | push the local at depth 0 (the newest) |
| 0x6f | `get_local1` | - | `-> v` | 13182 | push the local at depth 1 |
| 0x70 | `get_local2` | - | `-> v` | 8494 | push the local at depth 2 |
| 0x71 | `get_local3` | - | `-> v` | 5538 | push the local at depth 3 |
| 0x72 | `get_local4` | - | `-> v` | 3803 | push the local at depth 4 |
| 0x73 | `get_local5` | - | `-> v` | 2844 | push the local at depth 5 |
| 0x74 | `get_local` | u8 | `-> v` | 9384 | push the local at depth u8 (0 = newest) |
| 0x75 | `set_waittill_arg` | u8 | `->` | 1040 | assign the next waittill argument to the local at depth u8 (undefined when none left) |
| 0x76 | `jump` | i32 distance | `->` | 7119 | jump (i32 distance, either direction) |
| 0x77 | `thread_ptr` | u8 | `args fn -> r` | 69 | thread [[fn]](args), u8 arguments |
| 0x78 | `get_zero` | - | `-> 0` | 12327 | push integer 0 |
| 0x79 | `wait` | - | `secs ->` | 2448 | wait secs |
| 0x7a | `minus` | - | `a b -> a-b` | 2040 | - |
| 0x7b | `set_self_field` | u16 token (0xFFFF: inline name in stream) | `v ->` | 3513 | self.<token> = v |
| 0x7d | `multiply` | - | `a b -> a*b` | 1959 | * |
| 0x7e | `new_local` | u8 | `->` | 5585 | create a local named u8, undefined |
| 0x7f | `childthread_local` | 3-byte distance (x4), u8 argc | `args -> r` | 85 | childthread func(args), function of this file |
| 0x80 | `get_int` | i32 | `-> n` | 233 | push i32 as integer |
| 0x81 | `modulo` | - | `a b -> a%b` | 49 | % |
| 0x82 | `ref_anim_field` | u16 token (0xFFFF: inline name in stream) | `->` | 788 | variable register = anim.<token> |
| 0x83 | `get_builtin_function` | u16 builtin id | `-> fn` | 25 | push a builtin function pointer |
| 0x84 | `get_game` | - | `-> g` | 675 | push game [I] |
| 0x85 | `waittill` | - | `name obj ->` | 1576 | obj waittill(name, ...): suspends; set_waittill_arg ops follow |
| 0x86 | `dec` | - | `->` | 216 | -- on the referenced variable |
| 0x87 | `local_obj` | u8 | `->` | 16509 | object register = the local at depth u8 (must be an object) |
| 0x88 | `pre_call` | - | `-> marker` | 16595 | push a call marker; arguments follow |
| 0x89 | `get_anim` | - | `-> anim` | 26 | push anim |
| 0x8a | `get_undefined` | - | `-> undefined` | 1537 | push undefined |
| 0x8b | `ref_level_field` | u16 token (0xFFFF: inline name in stream) | `->` | 2954 | variable register = level.<token> |
| 0x8c | `anim_obj` | - | `->` | 19 | object register = anim |
| 0x8d | `level_obj` | - | `->` | 103 | object register = level |
| 0x8e | `bit_xor` | - | `a b -> a^b` | 0 | ^ |
| 0x8f | `equality` | - | `a b -> a==b` | 4554 | == |
| 0x90 | `clear_array_elem` | - | `key ->` | 381 | remove the element key of the array in the variable register [I] |
| 0x91 | `jump_back` | u16 backward distance | `->` | 4918 | jump backward (loop) |
| 0x92 | `get_animation` | 8 bytes + tree and name strings in stream | `-> a` | 1811 | push %animation |
| 0x93 | `get_anim_field` | u16 token (0xFFFF: inline name in stream) | `-> v` | 459 | push anim.<token> |
| 0x94 | `get_animtree` | u8 + tree name string in stream | `-> n` | 14 | push the anim tree number (#using_animtree) |
| 0x95 | `get_istring` | u32 placeholder + string in stream | `-> s` | 578 | push localized string literal |
| 0x96 | `ref_array` | - | `arr key ->` | 9509 | variable register = arr[key] |
| 0x97 | `ref_self_field` | u16 token (0xFFFF: inline name in stream) | `->` | 763 | variable register = self.<token> |
| 0x98 | `get_neg_byte` | u8 | `-> n` | 878 | push -u8 as integer |
| 0x99 | `get_builtin_method` | u16 builtin id | `-> fn` | 70 | push a builtin method pointer |
| 0x9a | `call_builtin_method_ptr` | u8 | `args fn obj -> r` | 69 | call a builtin method pointer, u8 arguments |
| 0x9b | `get_array_elem` | - | `arr key -> v` | 12362 | arr[key] |
| 0x9c | `make_vector` | - | `x y z -> v` | 589 | (x, y, z) |
| 0x9d | `call_method_far` | 3-byte placeholder; file and function tokens in stream | `marker args obj -> r` | 1932 | obj file::func(args) after pre_call |
| 0x9e | `get_local_array_elem` | u8 | `key -> v` | 5983 | (local at depth u8)[key] |
| 0x9f | `get_byte` | u8 | `-> n` | 15120 | push u8 as integer |
| 0xa0 | `childthread_ptr` | u8 | `args fn -> r` | 7 | childthread [[fn]](args), u8 arguments |
| 0xa1 | `bit_or` | - | `a b -> a| 3 | b` |
| 0xa2 | `array_push` | - | `arr v -> arr` | 805 | arr[arr.size] = v (array literals) |
| 0xa4 | `waittillmatch` | u16 | `match name obj ->` | 76 | obj waittillmatch(name, match); the u16 is not read by the VM [I] |
| 0xa5 | `get_function` | 3-byte distance (x4) | `-> fn` | 1319 | push &func, a function of this file |
| 0xa6 | `get_neg_ushort` | u16 | `-> n` | 41 | push -u16 as integer |
| 0xa7 | `shift_right` | - | `a b -> a>>b` | 1 | >> |
| 0xa8 | `call_builtin_method0` | u16 builtin id | `obj -> r` | 2678 | builtin method id, 0 arguments |
| 0xa9 | `call_builtin_method1` | u16 builtin id | `a obj -> r` | 3437 | builtin method id, 1 argument |
| 0xaa | `call_builtin_method2` | u16 builtin id | `a b obj -> r` | 1664 | builtin method id, 2 arguments |
| 0xab | `call_builtin_method3` | u16 builtin id | `a b c obj -> r` | 598 | builtin method id, 3 arguments |
| 0xac | `call_builtin_method4` | u16 builtin id | `a..d obj -> r` | 318 | builtin method id, 4 arguments |
| 0xad | `call_builtin_method5` | u16 builtin id | `a..e obj -> r` | 150 | builtin method id, 5 arguments |
| 0xae | `call_builtin_method` | u8 argc, u16 builtin id | `args obj -> r` | 152 | builtin method id, u8 arguments |
| 0xaf | `jump_false` | u16 forward distance | `v ->` | 20827 | pop; jump forward if false |


## Script VM semantics (`crates/gsc_iw6_vm`)

Read from the VM main loop (`FUN_14043a4c0` in `iw6mp64_ship_all.c`). Items marked [I] are inferred, not read.

Values: 0 undefined, 1 object, 2 string, 3 istring, 4 vector, 5 float, 6 int, 7 frame base, 8 call marker, 9 function pointer, 10 builtin fn, 11 builtin method, 12 wait record, 13 animation.

Corrections to the opcode table above:
- Pointer calls: the function value is on top, then the object (methods), then the arguments. Arguments are pushed last-to-first, so the first argument is on top.
- `get_array_elem` pops the container (top), then the key. `ref_array` pops only the key; the array is the variable register, and an undefined base becomes a new array.
- `make_vector` pops x (top), y, z.
- `waittillmatch`: the low byte of the u16 operand is the number of match values; the high byte is 0xA3. The match values are compared against the notify arguments.
- `inc`/`dec` push the new value; the next `set_variable_field` stores it (all 652 + 216 uses are followed by one).

Calls: `pre_call` pushes a call marker. `param` pops an argument, or yields undefined if the top is the marker. `check_params` turns the marker into a frame base and errors on extra arguments ("too many parameters"). `return` truncates the stack to the frame base and pushes the value. A waittill wake pushes a frame base, then the arguments reversed; `set_waittill_arg` takes the top or undefined; `clear_params` pops to the frame base. Nested frame depth is about 32 ("stack overflow").

Conditions (`jump_*`, `!`, `cast_bool`): int or float only. Anything else, undefined included, is a script error.

Equality: after int/float promotion and vector/scalar broadcast the types must match. undefined == undefined is true, undefined against anything else is an error, arrays cannot be compared, floats compare within 1e-6. Strings are case-insensitive [I]. `<=` is `!(>)`, `>=` is `!(<)`.

Arithmetic: int/int divides to float; a zero divisor is an error; int*int stays int; vectors are componentwise and broadcast scalars. `+` with a string concatenates (int decimal, float `%g`, vector `(x, y, z)`). Bit operations and `%` are int only. `size`: array count, string byte length, 1 for an object or entity, otherwise an error.

Threads: `thread` runs the new thread immediately and nested until its first block. `wait`: ticks = floor(secs*20+0.5), at least 1 for nonzero; a tick is 50 ms. Per tick: the bucket until empty, then the frame-end list, then tick+1. Notify walks waiters oldest-first; an endon entry kills its thread at once; a waittill entry queues the thread on the current tick. `childthread` dies when the spawning function returns or its thread dies [I]. Switch string cases are case-insensitive; the default is the last table entry.

State: `game` starts as an empty array; level and anim are objects; entity fields go through the host, with script variables as fallback.

Core builtin ids handled in the VM [I]: 0x2d isdefined, 0x18c spawnstruct, 0x1c0 getarraykeys, 0x1c1 getfirstarraykey, 0x1a6 getnextarraykey. Seen with the real `mp_frag` script and stubbed by the test host [I]: 0x37 getdvar, 0x38 getdvarint, 0x66 bullettrace, 0x107 vectortoangles, 0x1a7 sortbydistance, 0x8092 an origin getter.

Field ids [I]: entity 430 origin, 47 angles, 97 classname, 542 spawnflags, 640 target, 392 animation, 743 destructible type; struct 642 targetname; level 7879 console, 502 script, 31988 struct list, 31994 struct class names, 5594 bot bookkeeping.

Uncertain: array iteration order (insertion order here, the engine uses linked lists), FIFO order within a tick bucket, childthread kill rules, case folding, `shr` signedness, `get_thisthread` pushing an int, and `+` broadcasting a vector with a scalar (the engine probably errors).
