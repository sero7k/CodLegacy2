//! Prints each world material's state bits for the lit slots.
fn main() {
    let p = std::env::args().nth(1).expect("zone");
    let z = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&z);
    for m in cap
        .materials
        .iter()
        .filter(|m| m.techset.is_some())
        .take(25)
    {
        let e: Vec<(usize, u8)> = m
            .state_bits_entry
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| *v != 255)
            .collect();
        println!(
            "{} sort {} entries {:?}",
            m.name,
            m.sort_key,
            &e[..e.len().min(12)]
        );
        for (i, b) in m.state_bits.iter().enumerate() {
            let w: Vec<String> = (0..6)
                .map(|k| {
                    format!(
                        "{:08x}",
                        u32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]])
                    )
                })
                .collect();
            println!(
                "   [{i}] {} tail {:02x?} -> {:?}",
                w.join(" "),
                &b[24..40],
                render_h1::state::PassState::from_bits(b)
            );
        }
    }
}
