//! bo2mp: an offline audit of every Modern Warfare III / II guest
//! gun's first-person model, as `jup_gun` builds it: how its parts are put
//! together and where the Black Ops II donor's hands meet it.
//!
//! Per gun, from the part zones alone (no game is run):
//!
//! - the gun's long axis and its bore (the barrel part's long axis) against
//!   the view's forward axis (+x): yaw, pitch, and the roll of the
//!   magazine's hang about the long axis;
//! - the tags the parts were put on: how far each attach bone (and each
//!   part's own root) is turned from the receiver's frame;
//! - where the donor's right palm is against the gun's grip, and the left
//!   palm against the forend, once the gun is placed (`jup_gun::view_fit`);
//! - parts whose box lies away from the receiver's (stray, or on a wrong
//!   tag) and parts whose size is out of keeping with the receiver;
//! - the gun's length against the donor's model.

use std::fmt::Write as _;

use fastfile_jup::gun::{self, Bone, PartModel};

use super::jup_gun::{self as jg, JupGun};

/// A part as put on the gun.
pub(super) struct PartAudit {
    pub zone: String,
    pub model: String,
    pub tag: String,
    /// Surface range in the assembled gun.
    pub surfaces: std::ops::Range<usize>,
    /// Box of its vertices in the receiver's space.
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    /// Degrees the attach bone it was put on is turned from the receiver's
    /// frame, and its own root bone from its model's.
    pub tag_turn: f32,
    pub root_turn: f32,
    /// Whether it went on the bone named as its root (else the listed tag).
    pub by_root: bool,
    /// Where the root bone's placement and the listed tag's differ (units;
    /// `None` when either bone is absent).
    pub tag_gap: Option<f32>,
    /// Farthest vertex from the bone it hangs on, in receiver lengths.
    pub reach: f32,
    /// Gap between its box and the receiver's.
    pub gap: f32,
}

/// One gun's audit.
pub(super) struct GunAudit {
    pub gun: &'static str,
    pub donor: &'static str,
    pub parts: Vec<PartAudit>,
    pub unplaced: Vec<String>,
    /// The whole gun's long axis (degrees: yaw about z, pitch up).
    pub axis: (f32, f32),
    /// The bore: the barrel part's long axis.
    pub bore: Option<(f32, f32)>,
    /// Roll of the magazine (else grip) hang about the long axis.
    pub roll: Option<f32>,
    /// Receiver box and the whole gun's box (receiver space).
    pub receiver: ([f32; 3], [f32; 3]),
    pub whole: ([f32; 3], [f32; 3]),
    /// The hand anchor of the gun and the grip tag.
    pub anchor: [f32; 3],
    pub has_grip_tag: bool,
    /// The fit: scale and offset.
    pub fit: jg::ViewFit,
    /// The grip's centre and the forend's centre (the barrel part's), in
    /// `tag_weapon`'s space (after the fit).
    pub grip: Option<[f32; 3]>,
    pub forend: Option<[f32; 3]>,
    /// Donor palms (right, left) and the gaps (gun minus donor).
    pub donor_hands: Option<([f32; 3], [f32; 3])>,
    /// The receiver's top and the donor's sights.
    pub top: f32,
    /// The gun's length before and behind the grip, and the donor's.
    pub span: (f32, f32),
    pub donor_span: Option<(f32, f32)>,
    /// Parts of other guns' families (model names `base::variant`).
    pub families: Vec<String>,
    /// What the audit flags.
    pub flags: Vec<String>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn bounds<'a>(surfaces: impl Iterator<Item = &'a gun::PartSurface>) -> Option<([f32; 3], [f32; 3])> {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in surfaces.flat_map(|s| &s.positions) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

fn turn(q: [f32; 4]) -> f32 {
    (2.0 * f64::from(q[3].abs().min(1.0)).acos()).to_degrees() as f32
}

/// Eigenvectors of a symmetric 3x3 (Jacobi), largest eigenvalue first.
fn eigen(mut a: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let (mut p, mut q, mut big) = (0, 1, 0.0f64);
        for i in 0..3 {
            for j in i + 1..3 {
                if a[i][j].abs() > big {
                    big = a[i][j].abs();
                    p = i;
                    q = j;
                }
            }
        }
        if big < 1e-12 {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let t = if theta == 0.0 { 1.0 } else { t };
        let c = 1.0 / (t * t + 1.0).sqrt();
        let s = t * c;
        for k in 0..3 {
            let (akp, akq) = (a[k][p], a[k][q]);
            a[k][p] = c * akp - s * akq;
            a[k][q] = s * akp + c * akq;
        }
        for k in 0..3 {
            let (apk, aqk) = (a[p][k], a[q][k]);
            a[p][k] = c * apk - s * aqk;
            a[q][k] = s * apk + c * aqk;
        }
        for k in 0..3 {
            let (vkp, vkq) = (v[k][p], v[k][q]);
            v[k][p] = c * vkp - s * vkq;
            v[k][q] = s * vkp + c * vkq;
        }
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|&i, &j| a[j][j].total_cmp(&a[i][i]));
    order.map(|i| [v[0][i], v[1][i], v[2][i]])
}

/// The long axis of a point cloud (unit, pointing ahead), as (yaw, pitch)
/// degrees.
fn long_axis<'a>(surfaces: impl Iterator<Item = &'a gun::PartSurface>) -> Option<(f32, f32, [f64; 3])> {
    let pts: Vec<[f64; 3]> = surfaces
        .flat_map(|s| s.positions.iter().map(|p| p.map(f64::from)))
        .collect();
    if pts.len() < 3 {
        return None;
    }
    let n = pts.len() as f64;
    let mean: [f64; 3] = std::array::from_fn(|k| pts.iter().map(|p| p[k]).sum::<f64>() / n);
    let mut c = [[0.0f64; 3]; 3];
    for p in &pts {
        for i in 0..3 {
            for j in 0..3 {
                c[i][j] += (p[i] - mean[i]) * (p[j] - mean[j]) / n;
            }
        }
    }
    let mut e = eigen(c)[0];
    if e[0] < 0.0 {
        e = e.map(|x| -x);
    }
    Some((
        e[1].atan2(e[0]).to_degrees() as f32,
        e[2].clamp(-1.0, 1.0).asin().to_degrees() as f32,
        mean,
    ))
}

fn centroid<'a>(surfaces: impl Iterator<Item = &'a gun::PartSurface>) -> Option<[f64; 3]> {
    let mut sum = [0.0f64; 3];
    let mut n = 0usize;
    for p in surfaces.flat_map(|s| &s.positions) {
        for k in 0..3 {
            sum[k] += f64::from(p[k]);
        }
        n += 1;
    }
    (n > 0).then(|| sum.map(|x| x / n as f64))
}

/// A part's family: the `<class>_<gun>` of its model name's first segment
/// (`att_vm_p02_ar_golf3_stock_v12` -> `ar_golf3`).
fn family(model: &str) -> Option<String> {
    let first = model.split("::").next()?;
    let mut words = first.split('_').peekable();
    // skip `att_vm_pNN` / `mp_av_att_vm_pNN` / `jp19`-style prefixes
    let mut out = Vec::new();
    while let Some(w) = words.next() {
        if out.is_empty()
            && (matches!(w, "att" | "vm" | "wm" | "mp" | "av" | "aw" | "jup")
                || (w.len() <= 4 && w.chars().skip(1).all(|c| c.is_ascii_digit())))
        {
            continue;
        }
        out.push(w);
        if out.len() == 2 {
            break;
        }
    }
    (out.len() == 2).then(|| out.join("_"))
}

pub(super) fn audit_gun(
    g: &'static JupGun,
    read: &dyn Fn(&str) -> Result<PartModel, String>,
) -> Result<GunAudit, String> {
    let (gun, top) = jg::assemble_gun(g, true, read)?;
    // The same assembly without the lane's pruning, to see each part.
    let receiver_part = g.parts.iter().find(|p| p.tag.is_none()).ok_or("no receiver")?;
    let rec = read(receiver_part.view)?;
    let mut read_parts: Vec<(&jg::Part, PartModel)> = Vec::new();
    let mut unplaced = Vec::new();
    for p in g.parts.iter().filter(|p| p.tag.is_some()) {
        match read(p.view) {
            Ok(m) => read_parts.push((p, m)),
            Err(e) => unplaced.push(e),
        }
    }
    let mut parts = Vec::new();
    let mut so_far = gun::assemble(&rec, &[]);
    let rec_box = bounds(rec.surfaces.iter()).ok_or("receiver has no surfaces")?;
    let rec_len = (rec_box.1[0] - rec_box.0[0]).max(0.1);
    for (i, (p, m)) in read_parts.iter().enumerate() {
        let tag = p.tag.unwrap_or_default();
        let refs: Vec<(&PartModel, &str)> = read_parts[..=i].iter().map(|(p, m)| (m, p.tag.unwrap_or_default())).collect();
        let before = so_far.surfaces.len();
        let next = gun::assemble(&rec, &refs);
        let root = m.bones.first();
        let by_root = root.and_then(|r| so_far.bones.iter().find(|b| b.name == r.name).copied());
        let at: Option<Bone> = by_root.or_else(|| so_far.bone(tag).copied());
        if next.surfaces.len() == before {
            unplaced.push(format!("{}: no {tag}", m.name));
            continue;
        }
        let range = before..next.surfaces.len();
        let (lo, hi) = bounds(next.surfaces[range.clone()].iter()).unwrap_or(([0.0; 3], [0.0; 3]));
        let reach = at
            .map(|b| {
                next.surfaces[range.clone()]
                    .iter()
                    .flat_map(|s| &s.positions)
                    .map(|p| {
                        let d = sub(*p, b.pos);
                        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                    })
                    .fold(0.0f32, f32::max)
                    / rec_len
            })
            .unwrap_or(0.0);
        let gap = (0..3)
            .map(|k| (lo[k] - rec_box.1[k]).max(rec_box.0[k] - hi[k]).max(0.0))
            .fold(0.0f32, f32::max);
        parts.push(PartAudit {
            zone: p.view.to_owned(),
            model: m.name.clone(),
            tag: tag.to_owned(),
            surfaces: range,
            lo,
            hi,
            tag_turn: at.map_or(0.0, |b| turn(b.rot)),
            root_turn: root.map_or(0.0, |r| turn(r.rot)),
            by_root: by_root.is_some(),
            tag_gap: by_root.zip(so_far.bone(tag).copied()).map(|(r, t)| {
                let d = sub(r.pos, t.pos);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            }),
            reach,
            gap,
        });
        so_far = next;
    }
    let whole = bounds(gun.surfaces.iter()).ok_or("no surfaces")?;
    let (yaw, pitch, _) = long_axis(gun.surfaces.iter()).ok_or("too few vertices")?;
    // The bore: the barrel part's own long axis.
    let barrel = parts.iter().find(|p| p.tag == "tag_barrel_attach");
    let bore = barrel
        .and_then(|p| long_axis(so_far.surfaces[p.surfaces.clone()].iter()))
        .map(|(y, p, _)| (y, p));
    // Roll: where the magazine (else the grip) hangs about the long axis.
    let hang = parts
        .iter()
        .find(|p| p.tag == "tag_mag_attach")
        .or_else(|| parts.iter().find(|p| p.tag == "tag_pistolgrip_attach"));
    let roll = hang.and_then(|p| {
        let m = centroid(so_far.surfaces[p.surfaces.clone()].iter())?;
        let r = centroid(so_far.surfaces[..so_far.receiver_surfaces].iter())?;
        // (About the x axis, once the axis is level: the angle of the hang
        // from straight down.)
        let (dy, dz) = (m[1] - r[1], m[2] - r[2]);
        let a = dy.atan2(-dz).to_degrees() as f32;
        // A magazine hanging down is zero.
        Some(a)
    });
    let anchor = jg::hand_anchor(&gun);
    let fit = jg::view_fit(g, &gun, top);
    let place = |p: [f64; 3]| -> [f32; 3] {
        std::array::from_fn(|k| (p[k] as f32 - anchor[k]) * fit.scale + fit.offset[k])
    };
    let grip = parts
        .iter()
        .find(|p| p.tag == "tag_pistolgrip_attach")
        .and_then(|p| centroid(so_far.surfaces[p.surfaces.clone()].iter()))
        .map(place);
    // The forend: the barrel part's middle ahead of the receiver (its box
    // from its attach to the muzzle tag, else its box).
    let forend = barrel.map(|p| {
        let front = so_far.bone("tag_flash").map_or(p.hi[0], |b| b.pos[0]);
        let from = so_far.bone("tag_barrel_attach").map_or(p.lo[0], |b| b.pos[0]);
        place([
            f64::from(0.5 * (from + front)),
            0.5 * f64::from(p.lo[1] + p.hi[1]),
            f64::from(0.5 * (p.lo[2] + p.hi[2])),
        ])
    });
    let hands = jg::donor_hands(g.donor);
    let span = (
        (whole.0[0] - anchor[0]) * fit.scale + fit.offset[0],
        (whole.1[0] - anchor[0]) * fit.scale + fit.offset[0],
    );
    let donor_span = jg::donor_view_span(g.donor);
    let families: Vec<String> = {
        let mine = family(&rec.name);
        parts
            .iter()
            .filter_map(|p| {
                let f = family(&p.model)?;
                (Some(&f) != mine.as_ref() && p.model.contains("::")).then(|| format!("{} ({f})", p.tag.trim_start_matches("tag_").trim_end_matches("_attach")))
            })
            .collect()
    };
    let mut flags = Vec::new();
    let rec_diag = {
        let d = sub(rec_box.1, rec_box.0);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    for p in &parts {
        let name = p.tag.trim_start_matches("tag_").trim_end_matches("_attach");
        if p.gap > 1.0 {
            flags.push(format!("{name} detached ({:.1} from the receiver)", p.gap));
        }
        if p.reach > 1.5 {
            flags.push(format!("{name} reaches {:.1} receiver lengths from its tag", p.reach));
        }
        let d = sub(p.hi, p.lo);
        let diag = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if diag > 1.6 * rec_diag {
            flags.push(format!("{name} is {:.1}x the receiver's size", diag / rec_diag));
        }
        if p.tag_turn > 1.0 || p.root_turn > 1.0 {
            flags.push(format!("{name} on a turned frame (tag {:.0} deg, root {:.0} deg)", p.tag_turn, p.root_turn));
        }
    }
    if axis_off(yaw, pitch) {
        flags.push(format!("gun axis off forward (yaw {yaw:.1}, pitch {pitch:.1})"));
    }
    if let Some((y, p)) = bore
        && axis_off(y, p)
    {
        flags.push(format!("bore off forward (yaw {y:.1}, pitch {p:.1})"));
    }
    if let Some(r) = roll
        && r.abs() > 8.0
    {
        flags.push(format!("magazine hangs {r:.0} deg off down"));
    }
    if let (Some(d), Some(gp)) = (hands, grip) {
        let e = sub(gp, d.0);
        if e[0].abs() > 3.0 || e[2].abs() > 3.0 {
            flags.push(format!("grip {:+.1} ahead, {:+.1} up of the donor's right palm", e[0], e[2]));
        }
    }
    if let (Some(d), Some(f)) = (hands, forend) {
        let e = sub(f, d.1);
        if e[0].abs() > 5.0 || e[2].abs() > 3.0 {
            flags.push(format!("forend {:+.1} ahead, {:+.1} up of the donor's left palm", e[0], e[2]));
        }
    }
    if let Some((rear, front)) = donor_span {
        let (len, dlen) = (span.1 - span.0, front - rear);
        if len > 1.5 * dlen || len < 0.6 * dlen {
            flags.push(format!("length {len:.0} against the donor's {dlen:.0}"));
        }
    }
    Ok(GunAudit {
        gun: g.gun,
        donor: g.donor,
        parts,
        unplaced,
        axis: (yaw, pitch),
        bore,
        roll,
        receiver: rec_box,
        whole,
        anchor,
        has_grip_tag: jg::grip_tag(&gun).is_some(),
        fit,
        grip,
        forend,
        donor_hands: hands,
        top: top - anchor[2],
        span,
        donor_span,
        families,
        flags,
    })
}

fn axis_off(yaw: f32, pitch: f32) -> bool {
    yaw.abs() > 3.0 || pitch.abs() > 6.0
}

/// Every gun's audit, `read` giving a part zone's model by its name.
pub(super) fn audit_all(
    read: &dyn Fn(&str) -> Result<PartModel, String>,
    filter: Option<&str>,
) -> Vec<(&'static str, Result<GunAudit, String>)> {
    jg::all_guns()
        .filter(|g| filter.is_none_or(|f| g.gun.contains(f)))
        .map(|g| (g.gun, audit_gun(g, read)))
        .collect()
}

fn f1(x: f32) -> String {
    format!("{x:.1}")
}

fn opt(x: Option<f32>) -> String {
    x.map_or_else(|| "-".into(), |v| format!("{v:.1}"))
}

fn game(name: &str) -> &'static str {
    if name.starts_with("iw9_") {
        "Modern Warfare II (2022)"
    } else {
        "Modern Warfare III (2023)"
    }
}

/// The audit as Markdown tables, one per game.
pub(super) fn render(rows: &[(&'static str, Result<GunAudit, String>)]) -> String {
    let mut out = String::new();
    let mut games: Vec<&str> = Vec::new();
    for (name, _) in rows {
        let g = game(name);
        if !games.contains(&g) {
            games.push(g);
        }
    }
    for game_name in games {
        let _ = writeln!(out, "### {game_name}\n");
        let _ = writeln!(
            out,
            "| gun | donor | yaw / pitch | bore yaw / pitch | roll | grip vs right palm (ahead, across, up) | forend vs left palm (ahead, up) | top vs sights | length vs donor | flags |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|");
        for (name, r) in rows.iter().filter(|(n, _)| game(n) == game_name) {
            match r {
                Err(e) => {
                    let _ = writeln!(out, "| `{name}` | | | | | | | | | not audited: {e} |");
                }
                Ok(a) => {
                    let d = a.donor_hands;
                    let grip = a.grip.zip(d).map(|(g, d)| sub(g, d.0));
                    let forend = a.forend.zip(d).map(|(f, d)| sub(f, d.1));
                    let top = jg::donor_view_top(a.donor).map(|t| (a.top * a.fit.scale + a.fit.offset[2]) - t);
                    let len = a.donor_span.map(|(r, f)| (a.span.1 - a.span.0) / (f - r));
                    let _ = writeln!(
                        out,
                        "| `{}` | {} | {} / {} | {} | {} | {} | {} | {} | {} | {} |",
                        a.gun,
                        a.donor.trim_end_matches("_mp"),
                        f1(a.axis.0),
                        f1(a.axis.1),
                        a.bore.map_or_else(|| "-".into(), |(y, p)| format!("{y:.1} / {p:.1}")),
                        opt(a.roll),
                        grip.map_or_else(|| "-".into(), |g| format!("{:+.1}, {:+.1}, {:+.1}", g[0], g[1], g[2])),
                        forend.map_or_else(|| "-".into(), |f| format!("{:+.1}, {:+.1}", f[0], f[2])),
                        top.map_or_else(|| "-".into(), |t| format!("{t:+.1}")),
                        len.map_or_else(|| "-".into(), |l| format!("{l:.2}x")),
                        if a.flags.is_empty() && a.unplaced.is_empty() {
                            "-".to_owned()
                        } else {
                            a.flags
                                .iter()
                                .cloned()
                                .chain(a.unplaced.iter().map(|u| format!("unplaced: {u}")))
                                .collect::<Vec<_>>()
                                .join("; ")
                        },
                    );
                }
            }
        }
        out.push('\n');
    }
    out
}

/// Summary numbers over every audited gun: worst and mean of each
/// measure (for the before and after comparison).
pub(super) fn summary(rows: &[(&'static str, Result<GunAudit, String>)]) -> String {
    let audits: Vec<&GunAudit> = rows.iter().filter_map(|(_, r)| r.as_ref().ok()).collect();
    let mut out = String::new();
    let _ = writeln!(out, "guns audited: {} of {}", audits.len(), rows.len());
    let stat = |label: &str, vals: Vec<f32>, out: &mut String| {
        if vals.is_empty() {
            return;
        }
        let n = vals.len() as f32;
        let mean = vals.iter().map(|v| v.abs()).sum::<f32>() / n;
        let worst = vals.iter().fold(0.0f32, |a, v| if v.abs() > a.abs() { *v } else { a });
        let _ = writeln!(out, "{label}: n {} mean |x| {mean:.2} worst {worst:+.2}", vals.len());
    };
    let hands = |f: &dyn Fn(&GunAudit) -> Option<f32>| audits.iter().filter_map(|a| f(a)).collect::<Vec<f32>>();
    stat("gun axis yaw (deg)", hands(&|a| Some(a.axis.0)), &mut out);
    stat("gun axis pitch (deg)", hands(&|a| Some(a.axis.1)), &mut out);
    stat("bore yaw (deg)", hands(&|a| a.bore.map(|b| b.0)), &mut out);
    stat("bore pitch (deg)", hands(&|a| a.bore.map(|b| b.1)), &mut out);
    stat("magazine roll (deg)", hands(&|a| a.roll), &mut out);
    let gd = |a: &GunAudit, k: usize| a.grip.zip(a.donor_hands).map(|(g, d)| g[k] - d.0[k]);
    stat("grip vs right palm, ahead", hands(&|a| gd(a, 0)), &mut out);
    stat("grip vs right palm, across", hands(&|a| gd(a, 1)), &mut out);
    stat("grip vs right palm, up", hands(&|a| gd(a, 2)), &mut out);
    let fd = |a: &GunAudit, k: usize| a.forend.zip(a.donor_hands).map(|(f, d)| f[k] - d.1[k]);
    stat("forend vs left palm, ahead", hands(&|a| fd(a, 0)), &mut out);
    stat("forend vs left palm, up", hands(&|a| fd(a, 2)), &mut out);
    stat(
        "receiver top vs donor sights",
        hands(&|a| jg::donor_view_top(a.donor).map(|t| a.top * a.fit.scale + a.fit.offset[2] - t)),
        &mut out,
    );
    stat(
        "turned attach frames (deg)",
        audits.iter().flat_map(|a| a.parts.iter().map(|p| p.tag_turn.max(p.root_turn))).collect(),
        &mut out,
    );
    let flagged = audits.iter().filter(|a| !a.flags.is_empty()).count();
    let _ = writeln!(out, "guns with a flag: {flagged}");
    out
}

/// The audit of every gun whose name holds `filter` (all when `None`),
/// its part zones read from `dir`: a decompressed `<zone>.zone`, else the
/// install's `<zone>.ff` (Windows, through the game's Oodle DLL).
pub(crate) fn run(dir: &std::path::Path, filter: Option<&str>) -> String {
    let read = |zone: &str| -> Result<PartModel, String> {
        let plain = dir.join(format!("{zone}.zone"));
        let bytes = if let Ok(b) = std::fs::read(&plain) {
            b
        } else {
            let install = dir.parent().unwrap_or(dir);
            let ff = dir.join(format!("{zone}.ff"));
            let oodle = fastfile_jup::oodle::load(install).map_err(|e| format!("{zone}: {e}"))?;
            fastfile_jup::read_zone(&ff, oodle.as_ref()).map_err(|e| format!("{zone}: {e}"))?.1
        };
        gun::read_part(&bytes).map_err(|e| format!("{zone}: {e}"))
    };
    let rows = audit_all(&read, filter);
    let parts = if filter.is_some() { render_parts(&rows) } else { String::new() };
    format!("{}\n{}{}", summary(&rows), render(&rows), parts)
}

/// Each audited gun's parts (their boxes in the assembled gun, how far
/// they reach from their tag, their frames), for a filtered run.
fn render_parts(rows: &[(&'static str, Result<GunAudit, String>)]) -> String {
    let mut out = String::new();
    for (_, r) in rows {
        let Ok(a) = r else { continue };
        let _ = writeln!(
            out,
            "#### `{}` parts (receiver x {:.1}..{:.1}, y {:.1}..{:.1}, z {:.1}..{:.1})\n",
            a.gun, a.receiver.0[0], a.receiver.1[0], a.receiver.0[1], a.receiver.1[1], a.receiver.0[2], a.receiver.1[2]
        );
        let _ = writeln!(out, "| tag | model | x | y | z | reach | gap | tag turn | root turn | by root (off the tag) |\n|---|---|---|---|---|---|---|---|---|---|");
        for p in &a.parts {
            let _ = writeln!(
                out,
                "| {} | {} | {:.1}..{:.1} | {:.1}..{:.1} | {:.1}..{:.1} | {:.2} | {:.1} | {:.0} | {:.0} | {} |",
                p.tag, p.model, p.lo[0], p.hi[0], p.lo[1], p.hi[1], p.lo[2], p.hi[2], p.reach, p.gap, p.tag_turn, p.root_turn,
                if p.by_root { p.tag_gap.map_or("yes".into(), |g| format!("yes ({g:.1})")) } else { "-".into() }
            );
        }
        out.push('\n');
    }
    out
}
