//! `t7_dis <script object> [function name or #hash]`: a Black Ops III
//! script object's functions, decoded, with known names (objects as
//! `t7_gsc_scan <zone> <dir>` writes them out).
use gsc_t7::names::display;
use gsc_t7::{Insn, T7Object};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).expect("read");
    let o = T7Object::parse(&bytes).expect("parse");
    let want = a.get(2).map(|w| w.strip_prefix('#').and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or_else(|| gsc_t7::canon(w)));
    println!("{} crc {:#010x}, {} functions, includes {:?}", o.name, o.crc, o.functions.len(), o.includes);
    for f in &o.functions {
        if want.is_some_and(|w| w != f.export.name) {
            continue;
        }
        let e = &f.export;
        println!("\n{}::{}({} params) flags {:#x} @ {:#x}", display(e.ns), display(e.name), e.params, e.flags, e.address);
        for (at, insn) in &f.code {
            let text = match insn {
                Insn::Call(op, n, i) => {
                    let imp = &o.imports[*i as usize];
                    format!("{op:?} {n} {}::{} (flags {:#x})", display(imp.ns), display(imp.name), imp.flags)
                }
                Insn::Field(op, h) => format!("{op:?} {}", display(*h)),
                Insn::Method(op, n, h) => format!("{op:?} {n} {}", display(*h)),
                Insn::New(h) => format!("New {}", display(*h)),
                Insn::Locals(v) => format!("Locals {:?}", v.iter().map(|(h, f)| format!("{}{}", display(*h), if *f != 0 { format!("/{f}") } else { String::new() })).collect::<Vec<_>>()),
                other => format!("{other:?}"),
            };
            println!("  {at:#06x} {text}");
        }
    }
}
