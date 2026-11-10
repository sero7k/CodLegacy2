#!/usr/bin/env python3
"""Writes crates/gsc_t6/src/t7/gen_slots.rs and gen_builtins.rs from BlackOps3.exe.

usage: t7gen.py <BlackOps3.exe> <out dir> <scripts dir> [names file...]

Facts read from the exe (PC, the build the owner's Steam install runs):
- the VM's opcode handler table: 0x2000 u64 handler addresses in .rdata at
  0x142eb38b0 (the loop at 0x14138e5b0 calls table[op] for op < 0x2000),
  and with opcode bit 13 set a second table at 0x142e838b0 (op & 0x1fff);
  each handler is named here by what its decompiled code does (see
  docs/T7_ZOMBIES_PLAN.md section 4);
- the builtin tables: {canon hash u32, min args u32, max args u32, pad u32,
  handler u64, type u32, pad u32} records, either in .data or written there
  by init code (`mov dword [rip+x], imm32` / `lea rax, f; mov [rip+x], rax`).
Names files (one identifier per line) give builtins their names by hash;
`gen_names.rs` keeps every other name whose hash occurs as a u32 anywhere
in the compiled scripts under <scripts dir> (fields, functions, locals,
namespaces), so messages and the host can say `origin`, not a hash.
"""
import glob
import os
import re
import struct
import sys

HANDLERS = {
    0x141370f40: 'INC', 0x1413710b0: 'DEC', 0x141371220: 'BIT_OR', 0x141371260: 'BIT_XOR',
    0x1413712a0: 'BIT_AND', 0x1413712e0: 'EQUAL', 0x141371310: 'NOT_EQUAL',
    0x141371350: 'SUPER_EQUAL', 0x1413713d0: 'SUPER_NOT_EQUAL',
    0x141371450: 'LESS_THAN', 0x141371480: 'GREATER_THAN',
    0x1413714b0: 'LESS_THAN_OR_EQUAL', 0x1413714f0: 'GREATER_THAN_OR_EQUAL',
    0x141371530: 'SHIFT_LEFT', 0x141371570: 'SHIFT_RIGHT', 0x1413715b0: 'PLUS',
    0x1413715e0: 'MINUS', 0x141371780: 'MULTIPLY', 0x141371920: 'DIVIDE', 0x141371950: 'MODULUS',
    0x1413719f0: 'CAST_BOOL', 0x141371a00: 'BOOL_NOT', 0x141371a30: 'BOOL_COMPLEMENT',
    0x141371ab0: 'GET_UNDEFINED', 0x141371ad0: 'GET_ZERO', 0x141371af0: 'GET_BYTE',
    0x141371b20: 'GET_NEG_BYTE', 0x141371b50: 'GET_UNSIGNED_SHORT',
    0x141371b80: 'GET_NEG_UNSIGNED_SHORT', 0x141371bc0: 'GET_UINT64', 0x141371c00: 'GET_INTEGER',
    0x141371c30: 'GET_FLOAT', 0x141371c60: 'GET_STRING', 0x141371ca0: 'GET_ISTRING',
    0x141371ce0: 'GET_VECTOR', 0x141371d10: 'VECTOR', 0x141371e40: 'GET_HASH',
    0x141371e80: 'VECTOR_CONSTANT', 0x141371f60: 'VECTOR_SCALE',
    0x1413720c0: 'JUMP_ON_FALSE', 0x141372100: 'JUMP_ON_TRUE', 0x141372140: 'JUMP_ON_FALSE_EXPR',
    0x141372190: 'JUMP_ON_TRUE_EXPR', 0x1413721e0: 'JUMP', 0x141372200: 'DEVBLOCK_BEGIN',
    0x141372280: 'SWITCH', 0x141372490: 'END_SWITCH', 0x141372c90: 'PRE_SCRIPT_CALL',
    0x141372cb0: 'CALL_BUILTIN', 0x141372d20: 'CALL_BUILTIN_METHOD', 0x141372d70: 'END',
    0x141372e70: 'RETURN', 0x141372f80: 'SCRIPT_FUNCTION_CALL',
    0x141373080: 'SCRIPT_FUNCTION_CALL_POINTER', 0x1413731f0: 'CLASS_FUNCTION_CALL',
    0x141373320: 'SCRIPT_METHOD_CALL', 0x141373440: 'SCRIPT_METHOD_CALL_POINTER',
    0x1413735f0: 'SCRIPT_THREAD_CALL', 0x141373730: 'SCRIPT_THREAD_CALL_POINTER',
    0x1413738a0: 'CLASS_FUNCTION_THREAD_CALL', 0x141373a20: 'SCRIPT_METHOD_THREAD_CALL',
    0x141373b80: 'SCRIPT_METHOD_THREAD_CALL_POINTER', 0x141373d40: 'CLEAR_PARAMS',
    0x141373d90: 'CHECK_CLEAR_PARAMS', 0x141373de0: 'SAFE_CREATE_LOCAL_VARIABLES',
    0x141374960: 'GET_UNRESOLVED_FUNCTION', 0x141374990: 'GET_UNDEFINED_2',
    0x141374ba0: 'GET_OBJECT_834', 0x141374bc0: 'GET_CLASSES_OBJECT',
    0x141374be0: 'GET_LEVEL_OBJECT', 0x141374c00: 'GET_OBJECT_844', 0x141374c20: 'GET_SELF',
    0x141374c70: 'GET_834', 0x141374cc0: 'GET_CLASSES', 0x141374d10: 'GET_LEVEL',
    0x141374d60: 'GET_GAME', 0x141374da0: 'GET_844', 0x141374df0: 'GET_ANIMATION',
    0x141374e30: 'GET_GAME_REF', 0x141374e50: 'GET_FUNCTION', 0x141374e90: 'GET_API_FUNCTION',
    0x141374ed0: 'EVAL_LOCAL_VARIABLE_CACHED', 0x141374f30: 'EVAL_ARRAY',
    0x141374f60: 'EVAL_ARRAY_REF', 0x1413752a0: 'CLEAR_ARRAY',
    0x1413752d0: 'EMPTY_ARRAY', 0x141375300: 'GET_SELF_OBJECT', 0x1413753b0: 'EVAL_FIELD_VARIABLE',
    0x141375400: 'EVAL_FIELD_VARIABLE_REF', 0x141375440: 'CLEAR_FIELD_VARIABLE',
    0x1413755e0: 'SAFE_SET_WAITTILL_VARIABLE_FIELD_CACHED',
    0x141375640: 'SAFE_SET_VARIABLE_FIELD_CACHED', 0x1413756f0: 'EVAL_LOCAL_VARIABLE_REF_CACHED',
    0x141375720: 'SET_VARIABLE_FIELD', 0x1413757d0: 'DEC_TOP', 0x1413757f0: 'CAST_FIELD_OBJECT',
    0x1413758f0: 'SIZE_OF', 0x141375900: 'NEW', 0x141375b90: 'EVAL_SELF_FIELD_VARIABLE_REF',
    0x141375c80: 'EVAL_SELF_FIELD_VARIABLE', 0x141375d80: 'EVAL_LEVEL_FIELD_VARIABLE_REF',
    0x141375e70: 'EVAL_LEVEL_FIELD_VARIABLE', 0x141375f70: 'IS_DEFINED', 0x141376010: 'GET_TIME',
    0x141377880: 'FIRST_ARRAY_KEY', 0x141377a30: 'NEXT_ARRAY_KEY', 0x141377c30: 'SAFE_DEC_TOP',
    0x1413791f0: 'END_ON', 0x141379500: 'WAIT', 0x141379840: 'WAIT_TILL_FRAME_END',
    0x1413799e0: 'WAIT_TILL_MATCH', 0x141379e10: 'WAIT_TILL', 0x141379e20: 'NOTIFY',
    0x14137a0a0: 'REAL_WAIT',
}
DEBUG = set(range(0x141374740, 0x1413747c1, 0x10)) | {0x141374870, 0x141374940, 0x141374950}
INVALID = 0x14138e580

RDATA = (0x142dab000, 0x2da9800)
DATA = (0x1430e9000, 0x30e6e00, 0x4ba200)
TEXT = (0x140001000, 0x400, 0x2da1000)


def canon(s):
    x = 0x4b9ace2f
    for c in s.lower().encode():
        x = ((x ^ c) * 0x1000193) & 0xffffffff
    return (x * 0x1000193) & 0xffffffff


def main():
    exe = open(sys.argv[1], 'rb').read()
    out = sys.argv[2]
    names = {}
    scripts = sys.argv[3]

    def add(n):
        n = n.lower()
        if re.fullmatch(r'[a-z_][a-z0-9_]{1,63}', n):
            names.setdefault(canon(n), n)

    # Candidate names: identifiers in the exe's strings (and with the
    # engine's function prefixes taken off), in the scripts' own strings,
    # and in the names files given (our BO2/IW builtin catalogs).
    prefixes = ('gscr_', 'scr_', 'cscr_', 'cg_scr_', 'gscrcmd_', 'scrcmd_', 'playercmd_', 'actor_', 'botcmd_', 'vehiclecmd_', 'cscrcmd_')
    for tok in re.findall(rb'[A-Za-z_][A-Za-z0-9_]{2,48}', exe):
        t = tok.decode().lower()
        add(t)
        for p in prefixes:
            if t.startswith(p):
                add(t[len(p):])
    for f in glob.glob(os.path.join(scripts, '**', '*'), recursive=True):
        if os.path.isfile(f):
            for tok in re.findall(rb'[A-Za-z_][A-Za-z0-9_]{1,48}', open(f, 'rb').read()):
                add(tok.decode())
    for f in sys.argv[4:]:
        for line in open(f, encoding='latin1'):
            add(line.strip())
    # Opcodes.
    def slots_at(va):
        table = struct.unpack_from('<8192Q', exe, va - RDATA[0] + RDATA[1])
        slots = []
        for h in table:
            if h == INVALID or h in DEBUG:
                slots.append('INVALID')
            elif h in HANDLERS:
                slots.append(HANDLERS[h])
            else:
                raise SystemExit(f'unknown handler {h:#x}')
        return slots
    # (The loop at 0x14138e770 clears bit 13 of the opcode: without it,
    # table[op] at 0x142eb38b0; with it, a second table of the same
    # handlers in another order at 0x142e838b0. The mod tools' linker
    # writes those: every Workshop map's scripts have them.)
    tables = [('SLOTS', 'The operation each of the VM\'s 0x2000 opcode values runs.', slots_at(0x142eb38b0)),
              ('SLOTS_HI', 'The operation of each opcode value with bit 13 (0x2000) set, by its other bits.', slots_at(0x142e838b0))]
    with open(f'{out}/gen_slots.rs', 'w') as w:
        w.write('// Generated by crates/gsc_t7/gen/t7gen.py from BlackOps3.exe; do not edit.\n\n')
        w.write('use super::ops::T7Op::{self, *};\n')
        for name, doc, slots in tables:
            w.write(f'\n/// {doc}\n')
            w.write(f'pub static {name}: [T7Op; 0x2000] = [\n')
            for i in range(0, 0x2000, 4):
                w.write('    ' + ' '.join(f'{s},' for s in slots[i:i + 4]) + '\n')
            w.write('];\n')
    # Builtins: static records in .data, then init-code stores.
    seg = exe[DATA[1]:DATA[1] + DATA[2]]
    recs = {}

    def ok(o):
        if o + 0x20 > len(seg):
            return False
        h, mn, mx, pad, p, t, pad2 = struct.unpack_from('<IIIIQII', seg, o)
        return h != 0 and mn <= 64 and (mn <= mx <= 64 or mx == 0x7fffffff) and pad == 0 and 0x140001000 <= p < 0x142da2000 and t < 8 and pad2 == 0

    o = 0
    while o < len(seg) - 0x20:
        if ok(o):
            s = o
            rows = []
            while ok(o):
                rows.append((DATA[0] + o,) + struct.unpack_from('<IIIIQII', seg, o))
                o += 0x20
            if len(rows) >= 8:
                for r in rows:
                    recs.setdefault(r[1], (r[0], r[2], r[3], r[5], r[6]))
        else:
            o += 8
    mem = {}
    rax = None
    text = exe[TEXT[1]:TEXT[1] + TEXT[2]]
    for m in re.finditer(rb'\xc7\x05(....)(....)|\x48\x8d\x05(....)|\x48\x89\x05(....)', text, re.S):
        va = TEXT[0] + m.start()
        if m.group(1) is not None:
            rel, imm = struct.unpack('<iI', m.group(1) + m.group(2))
            mem[va + 10 + rel] = ('i', imm)
        elif m.group(3) is not None:
            rax = va + 7 + struct.unpack('<i', m.group(3))[0]
        elif rax is not None:
            mem[va + 7 + struct.unpack('<i', m.group(4))[0]] = ('q', rax)
    for a, (k, v) in mem.items():
        if k != 'i':
            continue
        # (Argument counts stored from a register are not tracked: 0..255.)
        f, mn, mx = mem.get(a + 0x10), mem.get(a + 4, ('r', 0)), mem.get(a + 8, ('r', 255))
        if f and f[0] == 'q' and v != 0 and 0x140001000 <= f[1] < 0x142da2000:
            mn = mn[1] if mn[0] == 'i' else 0
            mx = mx[1] if mx[0] == 'i' else 255
            if mn <= 64 and (mn <= mx <= 255 or mx == 0x7fffffff):
                recs.setdefault(v, (a, mn, mx, f[1], mem.get(a + 0x18, ('i', 0))[1]))
    with open(f'{out}/gen_builtins.rs', 'w') as w:
        w.write('// Generated by crates/gsc_t7/gen/t7gen.py from BlackOps3.exe; do not edit.\n\n')
        w.write('/// (canonical hash, name or "", min args, max args (255: any), handler address in the exe), by hash.\n')
        w.write('pub static BUILTINS: &[(u32, &str, u8, u8, u64)] = &[\n')
        for h in sorted(recs):
            a, mn, mx, fn, ty = recs[h]
            w.write(f'    ({h:#010x}, "{names.get(h, "")}", {mn}, {min(mx, 255)}, {fn:#x}),\n')
        w.write('];\n')
    used = set()
    for f in glob.glob(os.path.join(scripts, '**', '*'), recursive=True):
        if os.path.isfile(f):
            b = open(f, 'rb').read()
            for k in range(4):
                n = (len(b) - k) // 4
                used.update(struct.unpack_from(f'<{n}I', b, k))
    kept = sorted((h, n) for h, n in names.items() if h in used and h not in recs)
    with open(f'{out}/gen_names.rs', 'w') as w:
        w.write('// Generated by crates/gsc_t7/gen/t7gen.py; do not edit.\n\n')
        w.write('/// (canonical hash, name) of names the scripts use that are not builtins, by hash.\n')
        w.write('pub static NAMES: &[(u32, &str)] = &[\n')
        for h, n in kept:
            w.write(f'    ({h:#010x}, "{n}"),\n')
        w.write('];\n')
    print(f'{len(kept)} other names')
    print(f'{len(slots)} slots, {len(ops)} operations, {len(recs)} builtins, {sum(1 for h in recs if h in names)} named')


if __name__ == '__main__':
    main()
