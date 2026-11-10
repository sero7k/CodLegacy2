//! Times BO2's front-end read (the menu's zones, pictures, fonts, sounds)
//! part by part: `t6_frontend_time <BO2 install>/zone/all`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone_all = std::path::Path::new(&a[1]);
    let t = std::time::Instant::now();
    let front = assets::load_t6_frontend(zone_all);
    for line in &front.report {
        println!("{line}");
    }
    let reads = diag::process_reads().unwrap_or_default();
    println!(
        "all: {:.1} s, {} reads, {:.1} MB",
        t.elapsed().as_secs_f64(),
        reads.0,
        reads.1 as f64 / 1_048_576.0
    );
}
