//! Model surfaces with a lightmap unwrap: counts and a few values.
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
    let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
    let (mut with, mut without, mut shown) = (0, 0, 0);
    for ms in &cap.model_surfs {
        for s in &ms.surfs {
            match s.lmap_unwrap {
                Some(u) => {
                    with += 1;
                    if shown < 4 {
                        shown += 1;
                        let b = u.get(&zone.image);
                        let h = |i: usize| {
                            let v = u16::from_le_bytes([b[i], b[i + 1]]);
                            let (sg, e, m) = ((v >> 15) as i32, ((v >> 10) & 31) as i32, (v & 1023) as f32);
                            let x = if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
                            if sg == 1 { -x } else { x }
                        };
                        let n = b.len() / 4;
                        println!("unwrap {} verts: first {:?} {:?} last {:?}", n, (h(0), h(2)), (h(4), h(6)), (h(4 * (n - 1)), h(4 * (n - 1) + 2)));
                    }
                }
                None => without += 1,
            }
        }
    }
    println!("{with} with, {without} without");
}
