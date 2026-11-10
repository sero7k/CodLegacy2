//! Prints a `DXBC` container's resource ranges with their declared type
//! names, and its signatures: `dxil_res <file.dxbc>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = std::fs::read(a.get(1).expect("usage: dxil_res <file.dxbc>")).expect("read");
    let n = u32::from_le_bytes(b[28..32].try_into().unwrap()) as usize;
    for k in 0..n {
        let o = u32::from_le_bytes(b[32 + 4 * k..36 + 4 * k].try_into().unwrap()) as usize;
        if &b[o..o + 4] == b"DXIL" {
            let len = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap()) as usize;
            let m = dxil_sm6::read_part(&b[o + 8..o + 8 + len]).expect("module");
            let e = dxil_sm6::meta::entry(&m).expect("entry");
            println!("entry {} stage {:?}", e.name, e.stage);
            for r in &e.ranges {
                println!(
                    "{:?} id {} space {} lower {} size {} kind {} elem {} stride {} cb {} type {}",
                    r.class, r.id, r.space, r.lower, r.size, r.kind, r.elem, r.stride, r.cb_bytes, r.type_name
                );
            }
            for i in &e.inputs {
                println!("in  {} idx {} comp {} sem {} rows {} cols {} row0 {}", i.name, i.semantic_index, i.comp, i.sem, i.rows, i.cols, i.start_row);
            }
            for i in &e.outputs {
                println!("out {} idx {} comp {} sem {} rows {} cols {} row0 {}", i.name, i.semantic_index, i.comp, i.sem, i.rows, i.cols, i.start_row);
            }
        }
    }
}
