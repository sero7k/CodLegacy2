//! Prints the bitstream tree of one `DXBC` container's DXIL part:
//! `dxil_raw <file.dxbc> [block id]` (all blocks, or one id).
use dxil_sm6::bitstream::{self, Block, Item};

fn show(b: &Block, depth: usize, only: Option<u32>) {
    let pad = "  ".repeat(depth);
    let print = only.is_none_or(|o| o == b.id);
    if print {
        println!("{pad}block {}", b.id);
    }
    for it in &b.items {
        match it {
            Item::Record(r) => {
                if print {
                    let blob = r.blob.as_ref().map_or(String::new(), |x| format!(" blob[{}]", x.len()));
                    println!("{pad}  r{} {:?}{blob}", r.code, r.ops);
                }
            }
            Item::Block(c) => show(c, depth + 1, only),
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = std::fs::read(&a[1]).expect("read");
    let n = u32::from_le_bytes(b[28..32].try_into().unwrap()) as usize;
    for k in 0..n {
        let o = u32::from_le_bytes(b[32 + 4 * k..36 + 4 * k].try_into().unwrap()) as usize;
        if &b[o..o + 4] == b"DXIL" {
            let len = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap()) as usize;
            let bc = bitstream::bitcode_of_part(&b[o + 8..o + 8 + len]).expect("bitcode");
            let top = bitstream::parse(bc).expect("parse");
            show(&top, 0, a.get(2).and_then(|s| s.parse().ok()));
        }
    }
}
