//! A weapon's effect package: `iw7_vfxpkg <zone.ff> <package name part>`:
//! each effect slot (its offset in `WeaponVFXPackageDefs`), its kind
//! (`fx` is a classic effect, `vfx` a particle system) and name.
use fastfile_iw7::asset_type::AssetType;
use fastfile_iw7::walk::walk_fitted;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&args[1])).expect("zone");
    let (w, stop) = walk_fitted(&zone);
    if let Some(stop) = stop {
        eprintln!("walk stopped: {stop:?}");
    }
    let cap = &w.cap;
    let vfx = cap.vfx_packages.iter().flat_map(|p| &p.effects).filter(|e| e.1.ty == AssetType::Vfx).count();
    println!("{} packages, {} fx assets, {vfx} vfx slots", cap.vfx_packages.len(), cap.fx.len());
    for p in cap.vfx_packages.iter().filter(|p| p.name.contains(&args[2])) {
        println!("{}", p.name);
        for (o, id) in &p.effects {
            println!("  +{o:4} {:<5} {}", id.ty.name(), cap.name_of(*id));
        }
    }
}
