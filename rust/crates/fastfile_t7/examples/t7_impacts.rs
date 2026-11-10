//! The zone's impact tables: `t7_impacts <zone.ff>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    if std::env::var("T7_IMPACT_MAP").is_ok() {
        let src = fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]));
        let map = fastfile_t7::map::T7Map::load(&src).expect("map");
        for t in ["bullet_small_enter", "grenade_explosion", "knife_large_enter"] {
            let e = map.impact_effects(t);
            let found = e.as_ref().map(|(s, f)| (s.iter().flatten().count(), f.iter().flatten().count(), s.iter().flatten().filter(|n| map.effect(n).is_some()).count()));
            println!("{t}: (surfaces named, flesh named, surfaces whose effect loads) {found:?}");
        }
        return;
    }
    let w = fastfile_t7::walk::walk(&z);
    let mut v: Vec<_> = w.capture.impact_fields.iter().collect();
    v.sort();
    for ((ty, n), f) in v {
        println!("{} {n}:", ty.name());
        for (off, name) in f {
            println!("  {off:#x} {name}");
        }
    }
}

/// `T7_IMPACT_MAP=1 t7_impacts <map.ff>`: the map's view of BO3's bullet table.
#[allow(dead_code)]
fn unused() {}
