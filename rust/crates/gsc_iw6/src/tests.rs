//! Unit tests. The byte arrays are whole scripts copied from Ghosts' `mp_frag.ff` (the map's
//! precache script, its createfx stub and its crane animation script).

use super::*;

/// `maps/mp/mp_frag_precache` (script id 1399): one function, `main`.
const PRECACHE_STREAM: [u8; 42] = [
    0x16, 0x00, 0x00, 0x00, 0xdd, 0x4f, 0xac, 0x03, 0xdd, 0x4f, 0x74, 0x6f, 0x79, 0x5f, 0x74,
    0x72, 0x61, 0x6e, 0x73, 0x66, 0x6f, 0x72, 0x6d, 0x65, 0x72, 0x5f, 0x73, 0x6d, 0x61, 0x6c,
    0x6c, 0x30, 0x31, 0x00, 0x9f, 0x03, 0x8a, 0x08, 0xb9, 0x04, 0xdd, 0x4f,
];
const PRECACHE_CODE: [u8; 23] = [
    0x34, 0x32, 0x88, 0x60, 0x21, 0x30, 0x5a, 0x53, 0xa5, 0x77, 0x00, 0x00, 0x2e, 0x74, 0x61,
    0xb4, 0x69, 0x2d, 0x50, 0xd4, 0x90, 0x69, 0x34,
];

/// Script id 1246: sets two level fields and calls a builtin with a string.
const STUB_STREAM: [u8; 18] = [
    0x18, 0x00, 0x00, 0x00, 0xdd, 0x4f, 0xdf, 0x04, 0xdd, 0x4f, 0x6d, 0x70, 0x5f, 0x66, 0x72,
    0x61, 0x67, 0x00,
];
const STUB_CODE: [u8; 25] = [
    0x34, 0x32, 0x9f, 0x01, 0x24, 0x4c, 0x85, 0x60, 0x42, 0xfe, 0xdb, 0x24, 0xee, 0x5a, 0x78,
    0x53, 0x4d, 0x3a, 0x00, 0x00, 0x1c, 0x36, 0x01, 0x69, 0x34,
];

/// Script id 1209: a branch, an animation reference, array element assignments.
const CRANE_STREAM: [u8; 92] = [
    0x45, 0x00, 0x00, 0x00, 0xdd, 0x4f, 0x6d, 0x70, 0x5f, 0x66, 0x72, 0x61, 0x67, 0x5f, 0x63,
    0x72, 0x61, 0x6e, 0x65, 0x5f, 0x61, 0x6e, 0x69, 0x6d, 0x00, 0x9f, 0x03, 0x63, 0x49, 0x61,
    0x6e, 0x69, 0x6d, 0x61, 0x74, 0x65, 0x64, 0x5f, 0x70, 0x72, 0x6f, 0x70, 0x73, 0x00, 0x6d,
    0x70, 0x5f, 0x66, 0x72, 0x61, 0x67, 0x5f, 0x63, 0x72, 0x61, 0x6e, 0x65, 0x5f, 0x73, 0x77,
    0x61, 0x79, 0x00, 0x69, 0x64, 0x6c, 0x65, 0x00, 0x6d, 0x70, 0x5f, 0x66, 0x72, 0x61, 0x67,
    0x5f, 0x63, 0x72, 0x61, 0x6e, 0x65, 0x5f, 0x73, 0x77, 0x61, 0x79, 0x00, 0x69, 0x64, 0x6c,
    0x65, 0x00,
];
const CRANE_CODE: [u8; 70] = [
    0x34, 0x32, 0x55, 0x8b, 0x0d, 0x1b, 0x2d, 0x00, 0x40, 0x04, 0x00, 0x36, 0x24, 0x8b, 0x0d,
    0x53, 0xd4, 0x11, 0x00, 0x00, 0x17, 0x00, 0x2d, 0x57, 0x39, 0x70, 0xaf, 0x16, 0x00, 0x92,
    0x8d, 0x61, 0x58, 0x84, 0x8b, 0x1c, 0x29, 0x2c, 0x53, 0xcd, 0x78, 0x00, 0x00, 0x6e, 0x8b,
    0x8b, 0x0d, 0x96, 0x96, 0x5b, 0x34, 0x53, 0x95, 0x68, 0x00, 0x00, 0x53, 0x91, 0x51, 0x00,
    0x00, 0x6e, 0x8b, 0x8b, 0x0d, 0x96, 0x96, 0x5b, 0x34, 0x34,
];

fn parse(stream: &'static [u8], code: &'static [u8]) -> Script {
    let s = Script::parse(&ScriptBytes { stream, code }).expect("decodes");
    assert_eq!(s.validate(), vec![]);
    s
}

fn mnemonics(s: &Script) -> Vec<&'static str> {
    s.instrs.iter().map(|i| i.mnemonic).collect()
}

#[test]
fn precache_script_decodes() {
    let s = parse(&PRECACHE_STREAM, &PRECACHE_CODE);
    assert_eq!(s.prelude, 0x34);
    assert_eq!(s.stream_used, PRECACHE_STREAM.len());
    assert_eq!(s.functions.len(), 1);
    let f = &s.functions[0];
    assert_eq!((f.name.clone(), f.offset, f.end, f.params), (Name::Id(20445), 1, 23, 0));
    assert_eq!(
        mnemonics(&s),
        [
            "check_params",
            "pre_call",
            "get_function_far",
            "get_string",
            "call_far",
            "dec_top",
            "call0_far",
            "dec_top",
            "end"
        ]
    );
    // (0x60: a far function reference; its stream pair is file 940, function 20445.)
    assert_eq!(
        s.instrs[2].operands,
        [Operand::Far { file: Name::Id(940), func: Name::Id(20445) }]
    );
    assert_eq!(s.instrs[3].operands, [Operand::Str("toy_transformer_small01".to_owned())]);
    assert_eq!(
        s.instrs[4].operands,
        [Operand::Far { file: Name::Id(927), func: Name::Id(2186) }]
    );
    assert_eq!(
        s.instrs[6].operands,
        [Operand::Far { file: Name::Id(1209), func: Name::Id(20445) }]
    );
    assert_eq!(s.referenced_files(), [Name::Id(940), Name::Id(927), Name::Id(1209)]);
}

#[test]
fn stub_script_pushes_and_builtins() {
    let s = parse(&STUB_STREAM, &STUB_CODE);
    assert_eq!(
        mnemonics(&s),
        [
            "check_params",
            "get_byte",
            "set_level_field",
            "get_function_far",
            "set_level_field",
            "get_zero",
            "get_string",
            "call_builtin2",
            "dec_top",
            "end"
        ]
    );
    // `9f 01`: the integer 1; `24 4c 85`: level field token 0x854c.
    assert_eq!(s.instrs[1].operands, [Operand::U8(1)]);
    assert_eq!(s.instrs[2].operands, [Operand::Token(Name::Id(0x854c))]);
    // `1c 36 01`: builtin function 0x136 with two arguments.
    assert_eq!(s.instrs[7].operands, [Operand::Builtin(0x136)]);
    assert_eq!(s.instrs[7].len, 3);
}

#[test]
fn crane_script_jumps_animation_and_arrays() {
    let s = parse(&CRANE_STREAM, &CRANE_CODE);
    assert_eq!(s.instrs.len(), 27);
    let at = |off: usize| s.instrs.iter().find(|i| i.offset == off).expect("boundary");
    // `af 16 00` at 0x1a: jump_false, 3 bytes, distance 0x16 from the end (0x1d) -> 0x33.
    assert_eq!(at(0x1a).mnemonic, "jump_false");
    assert_eq!(at(0x1a).operands, [Operand::Jump(0x33)]);
    // `92` + 8 placeholder bytes + the stream's tree and animation names.
    assert_eq!(at(0x1d).mnemonic, "get_animation");
    assert_eq!(
        at(0x1d).operands,
        [Operand::Anim {
            tree: "animated_props".to_owned(),
            name: "mp_frag_crane_sway".to_owned()
        }]
    );
    assert_eq!(at(0x1d).len, 9);
    // `1b 2d 00`: call_builtin1 0x2d; `40 04 00`: jump_true over the 1 byte `36 24 8b 0d`.
    assert_eq!(at(0x05).operands, [Operand::Builtin(0x2d)]);
    assert_eq!(at(0x08).operands, [Operand::Jump(0x0f)]);
    // `96 96 5b`: two array references and the assignment.
    let tail: Vec<_> = s.body(&s.functions[0]).iter().rev().skip(1).take(3).map(|i| i.mnemonic).collect();
    assert_eq!(tail, ["end", "set_variable_field", "ref_array"]);
}

#[test]
fn local_call_target_is_a_function_start() {
    // A two-function script: `5c` calls the second function.
    // body 1: check_params, call0_local, dec_top, end = 1 + 4 + 1 + 1 = 7 bytes (1..8);
    // the call's operand starts at 3; the target 8 is 5 bytes on, stored x4 = 20 = 0x14.
    let code = [0x34, 0x32, 0x5c, 0x14, 0x00, 0x00, 0x69, 0x34, 0x32, 0x34];
    let mut stream = Vec::new();
    stream.extend_from_slice(&7i32.to_le_bytes());
    stream.extend_from_slice(&20445u16.to_le_bytes());
    stream.extend_from_slice(&2i32.to_le_bytes());
    stream.extend_from_slice(&0u16.to_le_bytes());
    stream.extend_from_slice(b"helper\0");
    let s = Script::parse(&ScriptBytes { stream: &stream, code: &code }).expect("decodes");
    assert_eq!(s.validate(), vec![]);
    assert_eq!(s.functions[1].name, Name::Text("helper".to_owned()));
    assert_eq!(s.instrs[1].operands, [Operand::Func(8)]);
}

#[test]
fn inline_token_reads_the_stream() {
    // `18 ff ff`: get_self_field with an inline name "origin" (stream: 00 00 "origin").
    let code = [0x34, 0x18, 0xff, 0xff, 0x34];
    let mut stream = Vec::new();
    stream.extend_from_slice(&4i32.to_le_bytes());
    stream.extend_from_slice(&20445u16.to_le_bytes());
    stream.extend_from_slice(&[0, 0]);
    stream.extend_from_slice(b"origin\0");
    let s = Script::parse(&ScriptBytes { stream: &stream, code: &code }).expect("decodes");
    assert_eq!(s.instrs[0].operands, [Operand::Token(Name::Text("origin".to_owned()))]);
    assert_eq!(s.stream_used, stream.len());
}

#[test]
fn errors_are_reported_not_panicked() {
    // A function longer than the code.
    let mut stream = Vec::new();
    stream.extend_from_slice(&99i32.to_le_bytes());
    stream.extend_from_slice(&1u16.to_le_bytes());
    assert!(matches!(
        Script::parse(&ScriptBytes { stream: &stream, code: &[0x34, 0x34] }),
        Err(Error::BadLength { .. })
    ));
    // An instruction cut off by the function end.
    let mut stream = Vec::new();
    stream.extend_from_slice(&1i32.to_le_bytes());
    stream.extend_from_slice(&1u16.to_le_bytes());
    assert!(matches!(
        Script::parse(&ScriptBytes { stream: &stream, code: &[0x34, 0x2b, 0, 0, 0, 0] }),
        Err(Error::CrossesEnd { .. } | Error::CodeEnd { .. })
    ));
    // An opcode the VM does not implement.
    let mut stream = Vec::new();
    stream.extend_from_slice(&1i32.to_le_bytes());
    stream.extend_from_slice(&1u16.to_le_bytes());
    assert!(matches!(
        Script::parse(&ScriptBytes { stream: &stream, code: &[0x34, 0x01] }),
        Err(Error::Unknown { op: 1, .. })
    ));
    // A truncated stream.
    assert!(matches!(
        Script::parse(&ScriptBytes { stream: &[1, 0], code: &[0x34, 0x34] }),
        Err(Error::StreamEnd { .. })
    ));
}

#[test]
fn opcode_table_is_consistent_with_the_loader_lengths() {
    // The total instruction length per opcode as the engine's script loader walks it.
    let by_len: [(usize, &[u8]); 8] = [
        (
            2,
            &[
                0x17, 0x26, 0x27, 0x2c, 0x31, 0x3a, 0x45, 0x4a, 0x4f, 0x5f, 0x65, 0x66, 0x74, 0x75,
                0x77, 0x7e, 0x87, 0x94, 0x98, 0x9a, 0x9e, 0x9f, 0xa0,
            ],
        ),
        (
            3,
            &[
                0x18, 0x24, 0x2a, 0x51, 0x52, 0x55, 0x5a, 0x7b, 0x82, 0x8b, 0x93, 0x97, 0x1a, 0x1b,
                0x1c, 0x1d, 0x1e, 0x1f, 0x23, 0x3f, 0x40, 0x4b, 0x83, 0x91, 0x99, 0xa4, 0xa6, 0xa8,
                0xa9, 0xaa, 0xab, 0xac, 0xad, 0xaf,
            ],
        ),
        (4, &[0x20, 0x50, 0x5c, 0x5d, 0xa5, 0xae, 0x2d, 0x2e, 0x60, 0x9d]),
        (5, &[0x22, 0x2f, 0x3e, 0x42, 0x2b, 0x43, 0x4c, 0x6b, 0x6c, 0x76, 0x7f, 0x80, 0x53, 0x95]),
        (9, &[0x92]),
        (13, &[0x56]),
        (0, &[0x3b]),
        (1, &[]),
    ];
    for op in 0..=255u8 {
        let want = by_len
            .iter()
            .find(|(_, l)| l.contains(&op))
            .map_or(1, |(n, _)| *n);
        match opcodes::info(op) {
            Some(i) => match i.layout.operand_len() {
                Some(n) => assert_eq!(1 + n, want, "opcode {op:#04x} {}", i.mnemonic),
                None => assert_eq!(op, 0x3b),
            },
            None => assert_eq!(want, 1, "opcode {op:#04x} is a no-op but the loader gives it operands"),
        }
    }
    assert_eq!(opcodes::OPS.len(), 150);
    assert!(opcodes::OPS.windows(2).all(|w| w[0].code < w[1].code));
}

#[test]
fn disassembly_reads_like_gsc() {
    let text = disassemble(&ScriptBytes { stream: &PRECACHE_STREAM, code: &PRECACHE_CODE })
        .expect("disassembles");
    assert!(text.contains("function main"));
    assert!(text.contains("get_function_far 940::main"));
    assert!(text.contains("call_far 927::#2186"));
    assert!(text.contains("get_string \"toy_transformer_small01\""));
}

#[test]
fn name_tables_parse() {
    let mut t = NameTable::parse("token 2186 flag_init\nfile 927 common_scripts/utility # c\nbuiltin 0x32 setdvar\n");
    assert_eq!(t.token(&Name::Id(2186)), "flag_init");
    assert_eq!(t.file(&Name::Id(927)), "common_scripts/utility");
    assert_eq!(t.builtin(0x32), "setdvar");
    assert_eq!(t.builtin(0x8073), "method#0x8073");
    t.merge("token 1 x");
    assert_eq!(t.token(&Name::Id(1)), "x");
}
