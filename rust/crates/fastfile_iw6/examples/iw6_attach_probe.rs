//! Lists a Ghosts zone's guns' scope and sight attachments: their view
//! models' root bones (and whether the gun has them), their reticle models'
//! root bones (and which model holds them), the sight flags (`useDualFOV`,
//! `hideRailWithThisScope`), the ADS overlay flags (thermal, hybrid,
//! outline) and the zoom: `iw6_attach_probe <zone.ff> [gun word]`.
use fastfile_iw6::AssetType;
use fastfile_iw6::walk::XModelRec;

fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let word = a.next().unwrap_or_default().to_ascii_lowercase();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let strings = &zone.assets.script_strings;
    let s = |i: u32| strings.get(i as usize).cloned().flatten().unwrap_or_default();
    let model = |id: Option<fastfile_iw6::walk::AssetId>| -> Option<&XModelRec> {
        let id = id?;
        (id.ty == AssetType::XModel).then(|| cap.xmodels.get(id.index as usize)).flatten()
    };
    let bones = |m: &XModelRec| -> Vec<String> { m.bone_names.iter().map(|&b| s(b)).collect() };
    let f32_at = |b: &[u8], o: usize| b.get(o..o + 4).map_or(0.0, |c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
    let mut dual = 0;
    for w in cap.weapons.iter().filter(|w| w.name.to_ascii_lowercase().contains(&word)) {
        let gun = model(w.gun_models.first().copied().flatten());
        let gb = gun.map(bones).unwrap_or_default();
        let tags: Vec<&String> = gb.iter().filter(|b| b.starts_with("tag_sight") || b.starts_with("tag_rail") || b.starts_with("tag_tip") || b.contains("reticle") || b.starts_with("tag_lens")).collect();
        println!("weapon {} gun {:?} tags {tags:?} hide {:?}", w.name, gun.map(|m| m.name.clone()), w.hide_tags.iter().map(|&t| s(t)).collect::<Vec<_>>());
        for (kind, list) in [("scope", &w.scopes), ("under", &w.under_barrels), ("other", &w.others)] {
            for id in list.iter().flatten() {
                let Some(att) = (id.ty == AssetType::Attachment).then(|| cap.attachments.get(id.index as usize)).flatten() else {
                    continue;
                };
                let view = model(att.view_models.first().copied().flatten());
                let vb = view.map(bones).unwrap_or_default();
                let root = vb.first().cloned().unwrap_or_default();
                let part = |n: &str| att.parts.iter().find(|(p, _)| *p == n).map(|(_, sp)| sp.get(&zone.image).to_vec());
                let sight = part("sight");
                let overlay = part("adsOverlay");
                let ads = part("adsSettings");
                let rec = att.record.get(&zone.image);
                let use_dual = sight.as_ref().is_some_and(|b| b.get(8).copied().unwrap_or(0) != 0);
                dual += usize::from(use_dual);
                let reticles: Vec<String> = att
                    .reticle_view_models
                    .iter()
                    .enumerate()
                    .filter_map(|(k, r)| {
                        let m = model(*r)?;
                        let rb = bones(m);
                        let r0 = rb.first().cloned().unwrap_or_default();
                        let on = if gb.contains(&r0) { "gun" } else if vb.contains(&r0) { "attachment" } else { "none" };
                        Some(format!("{k}:{} root {r0} on {on} ({} bones)", m.name, rb.len()))
                    })
                    .collect();
                println!(
                    "  {kind} {} view {:?} root {root} on gun {} lens {:?} sight {:?} overlay flags {:?} zoom {:.1} in {:.2} out {:.2} scale {:.2} reticles {reticles:?}",
                    att.name,
                    view.map(|m| m.name.clone()),
                    gb.contains(&root),
                    vb.iter().filter(|b| b.starts_with("tag_lens") || b.starts_with("tag_camera") || b.contains("reticle")).collect::<Vec<_>>(),
                    sight.as_ref().map(|b| b.to_vec()),
                    overlay.as_ref().map(|b| b.get(56..60).map(<[u8]>::to_vec)),
                    ads.as_ref().map_or(0.0, |b| f32_at(b, 28)),
                    ads.as_ref().map_or(0.0, |b| f32_at(b, 32)),
                    ads.as_ref().map_or(0.0, |b| f32_at(b, 36)),
                    f32_at(rec, 248),
                );
            }
        }
    }
    println!("{dual} attachment entries with useDualFOV");
}
