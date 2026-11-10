//! A material's techset, sort key, state flags and state-bit words (hex), for
//! every material whose name holds a word:
//! `cargo run -p fastfile_h1 --release --example h1_mat_info -- <zone.ff> <word>`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&args[0])).expect("zone");
    let (_map, cap) = fastfile_h1::map::IwMap::load(&zone);
    for m in &cap.materials {
        if !m.name.contains(args[1].as_str()) {
            continue;
        }
        let ts = m.techset.map_or("?".to_owned(), |t| cap.name_of(t).to_string());
        println!("{}: techset {ts}, sort {}, game {:#x}, state {:#x}, type {}", m.name, m.sort_key, m.game_flags, m.state_flags, m.material_type);
        for t in &m.textures {
            println!("   {t:?}");
        }
        for (i, b) in m.state_bits.iter().enumerate() {
            println!("   bits[{i}] {}", b.iter().map(|x| format!("{x:02x}")).collect::<String>());
        }
    }
}
