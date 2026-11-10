//! `CODLEGACY_T7_DEBUG_SURF`: switches for finding surfaces drawn wrongly or not
//! at all (the floors missing on most maps, Skyjacked's cream wall). A
//! comma-separated list; none is on by default, and the log's
//! `t7 surfaces:` lines (once per map) say what each draw did.
//!
//! * `ranges=<letters>`: draw only the world surfaces in these of the
//!   GfxWorld's four surface ranges (u32 begin/end pairs at +0x304, +0x30c,
//!   +0x314, +0x31c: A, B, C, D; `141d70780` walks A and C for the main
//!   view's lists 0/7/0x17/0x1a and B and D for lists 0x1c/0xa/0xf);
//!   `main_ranges` is `ranges=AC`. A surface in no range is drawn only with
//!   `outside` in the list.
//! * `smodel_flags`: leave out the static models whose draw record has bit
//!   0 of +0x60 set or a non-zero +0x70 (the exe's sun and spot shadow
//!   traversals, `141d71b00` / `141d71d90`, skip those; the main view's
//!   traversal is not decoded, so this is a test, not the exe's rule).
//! * `hide=<text>` (repeatable): leave out every draw whose material, technique
//!   set or model name contains the text (case ignored); `only=<text>`
//!   (repeatable): keep only those.
//! * `no_world`, `no_models`, `no_forward`: leave out the world surfaces,
//!   the static models, or every forward (post-lighting) draw.
//! * `strict_images`: a draw with any picture missing is left out (the old
//!   rule); `stand_in_colour`: a missing colour map is stood in for (grey)
//!   also on masked and blended surfaces.
//! * `cull_none`: no face culling on any surface pipeline (winding test).
//! * `shadow_casters`: shadow-only geometry (`shadowcaster` materials) is
//!   drawn in the view too (the old way; left out by default).
//! * `no_shared`: a material or model the map zone holds only as a
//!   reference is not looked up whole in the shared zones (the old rule:
//!   such materials were skipped, such models not built).
//! * `dark_flicker`: a missing `flickerLookupMap` takes the dark mask again
//!   (default: white, emissive screens lit).
//! * `no_reflect`: the deferred lighting's reflection probe array is one
//!   black cube (a floor that shows the world mirrored goes dark if the
//!   mirror is the probe reflection). render_t7 has no screen-space or
//!   planar reflection pass, so there is nothing else to turn off.
//! * `no_probe`: no `t7 probe:` pixel readbacks (`probe`; those run only with
//!   `CODLEGACY_T7_DEBUG_PROBE=1`); `no_coverage`: no
//!   `t7 coverage:` audit (every G-buffer draw alone with an occlusion query;
//!   runs only with `CODLEGACY_T7_DEBUG_COVERAGE=1`).
//! * `stand_in_mask=r.g.b.a` (0..255 each): the colour every stood-in mask
//!   (gloss, specular, unknown) takes instead of the default; `no_stand_in`
//!   is `strict_images`.

use std::sync::OnceLock;

#[derive(Debug, Default, Clone)]
pub struct Switches {
    /// Which of the four world surface ranges (A..D) are drawn, and whether
    /// surfaces in none are; None: all.
    pub ranges: Option<([bool; 4], bool)>,
    pub smodel_flags: bool,
    pub hide: Vec<String>,
    pub only: Vec<String>,
    pub no_world: bool,
    pub no_models: bool,
    pub no_forward: bool,
    pub strict_images: bool,
    pub stand_in_colour: bool,
    pub cull_none: bool,
    pub no_shared: bool,
    pub shadow_casters: bool,
    pub no_reflect: bool,
    pub no_probe: bool,
    pub no_coverage: bool,
    /// `dark_flicker`: a missing flicker lookup is stood in for dark (the old way).
    pub dark_flicker: bool,
    pub stand_in_mask: Option<[u8; 4]>,
    /// The variable as given (for the log).
    pub raw: String,
}

impl Switches {
    pub fn parse(s: &str) -> Self {
        let mut w = Switches {
            raw: s.to_owned(),
            ..Default::default()
        };
        for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let (k, v) = tok
                .split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .unwrap_or((tok, ""));
            match k.to_ascii_lowercase().as_str() {
                "main_ranges" => w.ranges = Some(([true, false, true, false], false)),
                "ranges" => {
                    let v = v.to_ascii_uppercase();
                    w.ranges = Some((
                        ['A', 'B', 'C', 'D'].map(|c| v.contains(c)),
                        w.ranges.is_some_and(|r| r.1),
                    ));
                }
                "outside" => {
                    let r = w.ranges.get_or_insert(([true; 4], true));
                    r.1 = true;
                }
                "smodel_flags" => w.smodel_flags = true,
                "hide" if !v.is_empty() => w.hide.push(v.to_ascii_lowercase()),
                "only" if !v.is_empty() => w.only.push(v.to_ascii_lowercase()),
                "no_world" => w.no_world = true,
                "no_models" => w.no_models = true,
                "no_forward" => w.no_forward = true,
                "strict_images" => w.strict_images = true,
                "stand_in_colour" | "stand_in_color" => w.stand_in_colour = true,
                "cull_none" => w.cull_none = true,
                "no_shared" => w.no_shared = true,
                "shadow_casters" => w.shadow_casters = true,
                "no_reflect" | "no_probes" => w.no_reflect = true,
                "no_probe" => w.no_probe = true,
                "no_coverage" => w.no_coverage = true,
                "dark_flicker" => w.dark_flicker = true,
                "no_stand_in" => w.strict_images = true,
                "stand_in_mask" => {
                    let c: Vec<u8> = v.split(['.', ':', '/']).filter_map(|x| x.trim().parse().ok()).collect();
                    if c.len() == 4 {
                        w.stand_in_mask = Some([c[0], c[1], c[2], c[3]]);
                    }
                }
                _ => {}
            }
        }
        w
    }

    /// Whether a draw of `names` (material, technique set, model) is left out
    /// by `hide` / `only`.
    pub fn hidden(&self, names: &[&str]) -> bool {
        let low: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        let has = |t: &String| low.iter().any(|n| n.contains(t.as_str()));
        self.hide.iter().any(has) || (!self.only.is_empty() && !self.only.iter().any(has))
    }

    pub fn any(&self) -> bool {
        !self.raw.trim().is_empty()
    }
}

/// The process's switches (`CODLEGACY_T7_DEBUG_SURF`).
pub fn get() -> &'static Switches {
    static S: OnceLock<Switches> = OnceLock::new();
    S.get_or_init(|| {
        Switches::parse(&codlegacy_env::var("CODLEGACY_T7_DEBUG_SURF").unwrap_or_default())
    })
}

/// What a material picture missing from the zone and the packages is bound
/// as, by the program's name for the binding (RDEF) and the image's name:
/// None leaves the draw out (and it is listed). Normal maps take a flat
/// normal in every encoding (x, y from r, g or from a, g: all 0.5; z 1),
/// gloss and specular maps a dark, rough value in every channel, occlusion
/// white, the other masks a darkish grey with a low alpha; a colour map is
/// stood in for (mid grey) only on a surface that neither blends nor
/// discards (`masked_or_blended`), since a grey opaque card would replace a
/// cut-out or blended one.
///
/// 2026-10-26 (second pass): every stand-in had alpha 255, and BO3 packs
/// gloss into a channel we do not know per material (often alpha), so a
/// floor whose gloss/specular picture was missing could read gloss 1 and
/// draw as a perfect mirror of the reflection probe (the owner's Skyjacked
/// screenshot: the world upside down below the horizon). No channel of a
/// mask stand-in is high now.
pub fn stand_in(
    binding: &str,
    image: &str,
    masked_or_blended: bool,
) -> Option<([u8; 4], &'static str)> {
    let s = get();
    if s.strict_images {
        return None;
    }
    let b = binding.to_ascii_lowercase();
    let i = image.to_ascii_lowercase();
    let i = i.trim_start_matches(',');
    let normal = b.contains("normal")
        || i.ends_with("_n")
        || i.ends_with("_nml")
        || i.contains("_normal")
        || i.contains("_nml_");
    if normal {
        return Some(([128, 128, 255, 128], "flat normal"));
    }
    if b.contains("reveal")
        || b.contains("alpha")
        || b.contains("opacity")
        || b.contains("mask") && !b.contains("gloss")
    {
        return None;
    }
    let colour = b.contains("color")
        || b.contains("colour")
        || b.contains("diffuse")
        || b.contains("albedo")
        || i.ends_with("_c")
        || i.ends_with("_col");
    if colour && !b.contains("spec") {
        return (!masked_or_blended || s.stand_in_colour)
            .then_some(([128, 128, 128, 255], "mid grey colour (untextured)"));
    }
    if b.contains("occlusion") || b.contains("cavity") || i.ends_with("_o") || i.contains("_ao") {
        return Some(([255, 255, 255, 255], "white occlusion"));
    }
    // The emissive-scroll materials' `flickerLookupMap` (`i_generic_lookup`)
    // scales the emissive by a looked-up value: a dark stand-in turns every
    // screen, sign and LED off (mp_arena's 90). White is the steady lit
    // state. (`CODLEGACY_T7_DEBUG_SURF=dark_flicker`: the old dark mask.)
    if (b.contains("flicker") || i.contains("generic_lookup")) && !s.dark_flicker {
        return Some(([255, 255, 255, 255], "white flicker lookup"));
    }
    if let Some(c) = s.stand_in_mask {
        return Some((c, "mask (stand_in_mask)"));
    }
    if b.contains("gloss") || i.ends_with("_g") {
        return Some(([48, 48, 48, 48], "rough gloss"));
    }
    if b.contains("spec") || i.ends_with("_s") {
        return Some(([56, 56, 56, 48], "dark rough specular"));
    }
    Some(([80, 80, 80, 48], "dark rough mask"))
}

/// A reason a draw was left out, without its particulars (for counting).
fn category(reason: &str) -> String {
    let r = reason;
    let masked = if r.contains("masked or blended") {
        " (masked or blended surface)"
    } else {
        ""
    };
    if r.contains("image") && r.contains("missing") {
        format!("picture missing{masked}")
    } else if r.contains("not named in the zone") {
        "texture not named in the zone".into()
    } else if r.contains("not in the material's table") {
        "texture or sampler not in the material's table".into()
    } else if r.contains("code texture")
        || r.contains("not made by the frame")
        || r.contains("code buffer")
    {
        "code resource not made yet".into()
    } else if r.contains("constant buffer") {
        "constant buffer not in the material".into()
    } else if r.starts_with("pipeline failed") {
        "pipeline failed".into()
    } else if r.contains("view list") {
        "forward surface in a list drawn before the lighting".into()
    } else if r.contains("no_forward") {
        "no_forward".into()
    } else {
        r.chars().take(60).collect()
    }
}

/// The `t7 surfaces:` lines, once per map: what each draw did (G-buffer,
/// forward, left out and why), the stand-ins for missing pictures, and the
/// largest draws (to name a surface that should not be there).
pub fn summary(
    scene: &crate::scene::Scene,
    gbuffer: (&[usize], &[(usize, String)], &[String]),
    forward: (&[usize], &[(usize, String)], &[String]),
) -> Vec<String> {
    use std::collections::{HashMap, HashSet};
    let mut out = Vec::new();
    let n = scene.draws.len();
    let (g_ready, f_ready): (HashSet<usize>, HashSet<usize>) = (
        gbuffer.0.iter().copied().collect(),
        forward.0.iter().copied().collect(),
    );
    let mut why: HashMap<usize, &str> = HashMap::new();
    for (d, r) in gbuffer.1.iter().chain(forward.1.iter()) {
        why.entry(*d).or_insert(r.as_str());
    }
    let mut cats: HashMap<String, usize> = HashMap::new();
    // (material, reason) -> count
    let mut per_mat: HashMap<(usize, String), usize> = HashMap::new();
    let mut skipped = 0;
    for (i, d) in scene.draws.iter().enumerate() {
        if g_ready.contains(&i) || f_ready.contains(&i) {
            continue;
        }
        skipped += 1;
        let m = &scene.materials[d.material];
        let r = match why.get(&i) {
            Some(r) => (*r).to_owned(),
            None if m.technique.is_none() && m.forward.is_none() => {
                "material has no G-buffer or forward technique".to_owned()
            }
            None => "not made (no pipeline or reason recorded)".to_owned(),
        };
        *cats.entry(category(&r)).or_default() += 1;
        *per_mat.entry((d.material, r)).or_default() += 1;
    }
    let mut c: Vec<_> = cats.into_iter().collect();
    c.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let world = scene.draws.iter().filter(|d| d.group == 3).count();
    let mut scene_out: Vec<String> = scene
        .not_drawn
        .iter()
        .map(|(r, k)| format!("{r} x{k}"))
        .collect();
    if scene_out.is_empty() {
        scene_out.push("none".into());
    }
    out.push(format!(
        "t7 surfaces: {}: drawn {} of {n} draws ({world} world, {} model/sky; G-buffer {}, forward {}), skipped {skipped} ({}); not made into draws: {}",
        scene.name,
        g_ready.len() + f_ready.len(),
        n - world,
        g_ready.len(),
        f_ready.len(),
        if c.is_empty() { "none".to_owned() } else { c.iter().map(|(r, k)| format!("{r} x{k}")).collect::<Vec<_>>().join("; ") },
        scene_out.join("; ")
    ));
    let mut pm: Vec<_> = per_mat.into_iter().collect();
    pm.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for ((mi, r), k) in pm.iter().take(32) {
        let m = &scene.materials[*mi];
        out.push(format!(
            "t7 surfaces: skipped {} ({}) x{k}: {r}",
            m.name, m.techset
        ));
    }
    if pm.len() > 32 {
        out.push(format!(
            "t7 surfaces: ... {} more materials skipped",
            pm.len() - 32
        ));
    }
    let mut si: Vec<&String> = gbuffer.2.iter().chain(forward.2.iter()).collect();
    si.sort();
    si.dedup();
    out.push(format!("t7 surfaces: {} missing pictures stood in for (a mid grey colour is an untextured, fully lit fallback)", si.len()));
    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for l in &si {
        *kinds.entry(l.rsplit(": ").next().unwrap_or("")).or_default() += 1;
    }
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort();
    out.push(format!("t7 surfaces: stand-ins by kind: {}", kinds.iter().map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", ")));
    for l in si.iter().take(64) {
        out.push(format!("t7 stand-in: {l}"));
    }
    // The largest drawn draws by their bounds' diagonal.
    let mut big: Vec<(f32, usize)> = scene
        .draws
        .iter()
        .enumerate()
        .filter(|(i, d)| d.source != u32::MAX && (g_ready.contains(i) || f_ready.contains(i)))
        .map(|(i, d)| {
            (
                (0..3)
                    .map(|k| (d.maxs[k] - d.mins[k]).powi(2))
                    .sum::<f32>()
                    .sqrt(),
                i,
            )
        })
        .collect();
    big.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (diag, i) in big.iter().take(16) {
        let d = &scene.draws[*i];
        let m = &scene.materials[d.material];
        let size: Vec<String> = (0..3)
            .map(|k| format!("{:.0}", d.maxs[k] - d.mins[k]))
            .collect();
        let at: Vec<String> = (0..3)
            .map(|k| format!("{:.0}", 0.5 * (d.maxs[k] + d.mins[k])))
            .collect();
        let from = if d.group == 3 {
            format!(
                "world surface #{} range {} flags {:#04x} list {}",
                d.source,
                ["A", "B", "C", "D", "none"]
                    .get(d.range as usize)
                    .unwrap_or(&"?"),
                d.flags,
                d.list
            )
        } else {
            format!(
                "model {} x{} (+0x60 {:#04x})",
                scene
                    .model_names
                    .get(d.source as usize)
                    .map(String::as_str)
                    .unwrap_or("?"),
                d.instances,
                d.flags
            )
        };
        out.push(format!(
            "t7 big draw: {} {} ({}) {:.0} units ({}) at ({}): {from}",
            if g_ready.contains(i) {
                "gbuffer"
            } else {
                "forward"
            },
            m.name,
            m.techset,
            diag,
            size.join("x"),
            at.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_hide() {
        let s = Switches::parse("main_ranges, hide=Glass ,only=wc/,smodel_flags,cull_none");
        assert_eq!(s.ranges, Some(([true, false, true, false], false)));
        assert!(s.smodel_flags && s.cull_none);
        assert!(s.hidden(&["mtl_glass_01", "wc/lit"]));
        assert!(!s.hidden(&["mtl_floor", "wc/lit"]));
        assert!(s.hidden(&["mtl_floor", "mc/lit"]));
        let r = Switches::parse("ranges=bd,outside");
        assert_eq!(r.ranges, Some(([false, true, false, true], true)));
        assert!(!Switches::parse("").any());
    }

    #[test]
    fn stand_ins() {
        assert_eq!(
            stand_in("normalMap", "i_t7_micro_metal_brushed_01_n", false)
                .unwrap()
                .1,
            "flat normal"
        );
        assert!(stand_in("colorMap", "i_floor_c", true).is_none());
        assert!(stand_in("colorMap", "i_floor_c", false).is_some());
        assert_eq!(
            stand_in("specularMap", "x", true).unwrap().1,
            "dark rough specular"
        );
        // No mask stand-in may read as a mirror in any channel.
        for b in ["specularMap", "glossMap", "detailMap", "whatever"] {
            assert!(stand_in(b, "x", false).unwrap().0.iter().all(|&c| c <= 96), "{b}");
        }
        assert_eq!(stand_in("flickerLookupMap", "i_generic_lookup", false).unwrap().0, [255; 4]);
        assert!(stand_in("revealMap", "x", false).is_none());
    }
}
