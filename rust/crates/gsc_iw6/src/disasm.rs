//! Text disassembly.

use std::collections::BTreeSet;
use std::fmt::Write;

use crate::{Case, CaseValue, Instr, NameTable, Operand, Script, ScriptBytes};

fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\x{:02x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn label(t: usize) -> String {
    if t == usize::MAX { "L_BAD".to_owned() } else { format!("L{t:04x}") }
}

fn float(f: f32) -> String {
    if f.is_finite() && f == f.trunc() && f.abs() < 1e9 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

fn operand(o: &Operand, script: &Script, names: &NameTable) -> String {
    match o {
        Operand::U8(v) => format!("{v}"),
        Operand::U16(v) => format!("{v}"),
        Operand::I32(v) => format!("{v}"),
        Operand::F32(v) => float(*v),
        Operand::Vec3(v) => format!("({}, {}, {})", float(v[0]), float(v[1]), float(v[2])),
        Operand::Token(n) => format!(".{}", names.token(n)),
        Operand::Str(s) => quote(s),
        Operand::Builtin(id) => names.builtin(*id),
        Operand::Argc(n) => format!("argc={n}"),
        Operand::Jump(t) => label(*t),
        Operand::Func(t) => match script.function_at(*t) {
            Some(f) => format!("{}  ; @{t:#x}", names.token(&f.name)),
            None => format!("@{t:#x}?"),
        },
        Operand::Far { file, func } => format!("{}::{}", names.file(file), names.token(func)),
        Operand::Table(t) => format!("table@{t:#x}"),
        Operand::Cases(cs) => {
            let mut s = String::new();
            for Case { value, target } in cs {
                let v = match value {
                    CaseValue::Default => "default".to_owned(),
                    CaseValue::Str(x) => quote(x),
                    CaseValue::Int(i) => format!("{i}"),
                };
                let _ = write!(s, "\n        {v} -> {}", label(*target));
            }
            s
        }
        Operand::Anim { tree, name } => format!("%{}{}{name}", tree, if tree.is_empty() { "" } else { "::" }),
        Operand::Tree(t) => quote(t),
    }
}

fn line(i: &Instr, script: &Script, names: &NameTable, hex: Option<&[u8]>) -> String {
    let mut s = format!("  {:04x}: ", i.offset);
    if let Some(code) = hex {
        let bytes = &code[i.offset..i.offset + i.len.min(8)];
        let mut h = String::new();
        for b in bytes {
            let _ = write!(h, "{b:02x} ");
        }
        let _ = write!(s, "{h:<26}");
    }
    s.push_str(i.mnemonic);
    let parts: Vec<String> = i.operands.iter().map(|o| operand(o, script, names)).collect();
    if !parts.is_empty() {
        s.push(' ');
        s.push_str(&parts.join(" "));
    }
    s
}

/// Disassembles with the built-in names ([`NameTable::known`]).
pub fn disassemble(bytes: &ScriptBytes) -> Result<String, crate::Error> {
    disassemble_with(bytes, &NameTable::known(), false)
}

/// Disassembles `bytes`; `hex` adds the instruction bytes to each line.
pub fn disassemble_with(
    bytes: &ScriptBytes,
    names: &NameTable,
    hex: bool,
) -> Result<String, crate::Error> {
    let script = Script::parse(bytes)?;
    Ok(render(&script, bytes.code, names, hex))
}

/// Text of an already decoded script.
pub fn render(script: &Script, code: &[u8], names: &NameTable, hex: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "; {} functions, {} instructions, code {} bytes, stream {} bytes",
        script.functions.len(),
        script.instrs.len(),
        script.code_len,
        script.stream_len
    );
    let files = script.referenced_files();
    if !files.is_empty() {
        let list: Vec<String> = files.iter().map(|f| names.file(f)).collect();
        let _ = writeln!(out, "; includes (far call files): {}", list.join(" "));
    }
    // (Jump targets get a label line.)
    let mut targets: BTreeSet<usize> = BTreeSet::new();
    for i in &script.instrs {
        for o in &i.operands {
            match o {
                Operand::Jump(t) => {
                    targets.insert(*t);
                }
                Operand::Cases(cs) => {
                    targets.extend(cs.iter().map(|c| c.target));
                }
                _ => {}
            }
        }
    }
    for f in &script.functions {
        let _ = writeln!(
            out,
            "\nfunction {}  ; {:#06x}..{:#06x}, {} params",
            names.token(&f.name),
            f.offset,
            f.end,
            f.params
        );
        for i in script.body(f) {
            if targets.contains(&i.offset) {
                let _ = writeln!(out, " {}:", label(i.offset));
            }
            let _ = writeln!(out, "{}", line(i, script, names, hex.then_some(code)));
        }
        if targets.contains(&f.end) {
            let _ = writeln!(out, " {}:", label(f.end));
        }
    }
    out
}
