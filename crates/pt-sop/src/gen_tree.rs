//! A conifer made of knobs, not of a scan: `tree_conifer`.
//!
//! The tree is a trunk (a curved, tapering tube), whorls of branches (curved, drooping and curling tubes whose length follows a
//! crown profile) and sprays of needles (crossed cards that carry a needle picture with an alpha channel). Every proportion is a
//! knob, so a spruce, a fir, a wind-bent pine or a tree that never grew on Earth are points in the same space (DECISIONS B16a): a
//! style sets the knobs, a seed makes each tree different. Deterministic.
//!
//! Units are metres, +Y up, the foot of the trunk at the origin. Materials: `bark` for wood and `conifer_needles` for the cards
//! (both are role names the style maps to library materials).

use crate::{ps, sch};
use pt_core::math::{self, cross, normalize, sq, sub};
use pt_core::rng::rand4;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::sync::Arc;

type V3 = [f32; 3];
type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![sch("tree_conifer", "A procedural conifer (spruce, fir, pine and stranger): trunk, whorls of drooping branches and sprays of needle cards. All proportions are knobs: `height`, `crown_base` (the clear trunk below the first branches, as a share of height), `crown_width` (longest branch as a share of height), `crown_power` (1 = cone, below 1 = fuller, above 1 = narrower at the top), `whorl_spacing`, `branches`, `droop` and `lift` (branch angle at the crown's foot and top, degrees), `curl` (upturned tips), `asymmetry` and `gaps` (a real tree is lopsided and has holes), `lean`, `wobble`, `spray_width`, `spray_length`, `sprays_per_m`. The seed makes every tree different. Materials: `bark` and `conifer_needles` (roles). The needle cards need the picture from `pt gen-texture needles`.", 0, D::None, G::Whole, vec![
        ps("height", json!(18.0), "Tree height (m)."),
        ps("trunk_ratio", json!(0.022), "Trunk radius at the foot as a share of the height."),
        ps("taper", json!(0.85), "How much the trunk narrows toward the top (0..1)."),
        ps("crown_base", json!(0.2), "Height of the first branches as a share of the height."),
        ps("crown_width", json!(0.3), "Longest branch as a share of the height (spruce 0.2, pine 0.4)."),
        ps("crown_power", json!(0.85), "Branch length falls with height as (1 - t)^power: 1 = cone, less = rounder."),
        ps("whorl_spacing", json!(0.45), "Distance between whorls of branches (m)."),
        ps("branches", json!(8), "Branches per whorl (3..10)."),
        ps("droop", json!(28.0), "Angle of the lowest branches below horizontal (degrees)."),
        ps("lift", json!(22.0), "Angle of the topmost branches above horizontal (degrees)."),
        ps("curl", json!(0.35), "Upturn of the branch tips (0..1)."),
        ps("asymmetry", json!(0.35), "How much branches differ in length (0..1)."),
        ps("gaps", json!(0.06), "Share of branches that are missing (0..0.5)."),
        ps("lean", json!(2.0), "Lean of the trunk (degrees)."),
        ps("wobble", json!(0.02), "Sideways waviness of the trunk as a share of the height."),
        ps("spray_width", json!(0.42), "Width of a needle spray card (m)."),
        ps("spray_length", json!(0.55), "Length of a needle spray card (m)."),
        ps("sprays_per_m", json!(22.0), "Sprays per metre of branch (each is two crossed cards)."),
        ps("detail", json!(1.0), "Scale of the number of branches, sprays and sides (0.3 = distant tree, 1.5 = hero)."),
        ps("seed", json!(1), "Seed: every tree of the same knobs differs."),
        ps("bark_material", json!("bark"), "Material role of wood."),
        ps("needle_material", json!("conifer_needles"), "Material role of the needle cards."),
    ])]
}

pub fn cook(kind: &str, p: &Params, _inputs: Inputs) -> Option<Res> {
    (kind == "tree_conifer").then(|| tree_conifer(p))
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Mesh under construction: positions, polygons, and (for cards) UVs and normals.
#[derive(Default)]
struct Mesh {
    p: Vec<V3>,
    polys: Vec<Vec<u32>>,
    uv: Vec<[f32; 2]>,
    n: Vec<V3>,
}

impl Mesh {
    /// A tube along `pts` with a radius at each, `sides` sides, closed at the thin end by collapsing it (radius ~0).
    fn tube(&mut self, pts: &[V3], radii: &[f32], sides: usize, v_per_m: f32) {
        let base = self.p.len() as u32;
        let mut v_acc = 0.0f32;
        for j in 0..pts.len() {
            let t = if j + 1 < pts.len() { sub(pts[j + 1], pts[j]) } else { sub(pts[j], pts[j - 1]) };
            let t = normalize(if j > 0 && j + 1 < pts.len() { add(normalize(sub(pts[j], pts[j - 1])), normalize(t)) } else { t });
            let reference = if t[1].abs() < 0.95 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
            let n1 = normalize(cross(t, reference));
            let n2 = cross(t, n1);
            if j > 0 {
                let d = sub(pts[j], pts[j - 1]);
                v_acc += dot(d, d).sqrt() * v_per_m;
            }
            for i in 0..sides {
                let a = std::f32::consts::TAU * i as f32 / sides as f32;
                let (s, c) = pt_core::math::sin_cos(a);
                let dir = add(mul(n1, c), mul(n2, s));
                self.p.push(add(pts[j], mul(dir, radii[j])));
                self.n.push(dir);
                self.uv.push([i as f32 / sides as f32, v_acc]);
            }
        }
        for j in 0..pts.len() - 1 {
            for i in 0..sides {
                let i1 = (i + 1) % sides;
                let (a, b, c, d) = (base + (j * sides + i) as u32, base + (j * sides + i1) as u32, base + ((j + 1) * sides + i1) as u32, base + ((j + 1) * sides + i) as u32);
                self.polys.push(vec![a, b, c, d]);
            }
        }
    }

    /// A card: corners `a` (bottom left), `b` (bottom right), `c` (top right), `d` (top left), UV over the whole picture.
    fn card(&mut self, q: [V3; 4], normal: V3) {
        let base = self.p.len() as u32;
        let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for k in 0..4 {
            self.p.push(q[k]);
            self.n.push(normal);
            self.uv.push(uv[k]);
        }
        self.polys.push(vec![base, base + 1, base + 2, base + 3]);
    }

    fn geometry(self, material: &str) -> Geometry {
        let n = self.p.len();
        let mut g = Geometry::from_polys(self.p, &self.polys);
        if n > 0 {
            g.point.set("N", Column::from_v3(self.n));
            g.point.set("uv", Column::V2(Arc::new(self.uv)));
            g.assign_material(material, &vec![true; g.n_prims()]);
        }
        g
    }
}

fn tree_conifer(p: &Params) -> Res {
    let height = p.f("height").clamp(0.5, 120.0);
    let detail = p.f("detail").clamp(0.2, 2.0);
    let seed = p.i("seed");
    let rnd = |stream: u32, i: u64| rand4(seed, stream, i)[0];
    let (crown_base, crown_width) = (p.f("crown_base").clamp(0.0, 0.8), p.f("crown_width").clamp(0.02, 1.0));
    let crown_power = p.f("crown_power").clamp(0.2, 3.0);
    let r0 = (p.f("trunk_ratio").clamp(0.003, 0.08) * height).max(0.04);
    let taper = p.f("taper").clamp(0.0, 0.98);
    let lean = (p.f("lean") + (rnd(1, 0) - 0.5) * 2.0).to_radians();
    let wobble = p.f("wobble") * height;
    let phase = [rnd(1, 1) * std::f32::consts::TAU, rnd(1, 2) * std::f32::consts::TAU];
    let (lean_dir_s, lean_dir_c) = pt_core::math::sin_cos(rnd(1, 3) * std::f32::consts::TAU);

    // the trunk: where it is and how thick, at height y
    let axis = |y: f32| -> V3 {
        let t = y / height;
        let off = math::tan(lean) * y + wobble * (math::sin(t * 5.0 + phase[0]) * 0.5 * t);
        let off2 = wobble * (math::sin(t * 4.3 + phase[1]) * 0.5 * t);
        [off * lean_dir_c + off2 * lean_dir_s, y, off * lean_dir_s - off2 * lean_dir_c]
    };
    let radius = |y: f32| -> f32 {
        let t = (y / height).clamp(0.0, 1.0);
        (r0 * math::pow(1.0 - taper * t, 1.15) + r0 * 0.7 * math::exp(-y * 3.0 / (1.0 + r0 * 4.0))).max(0.012)
    };

    let mut wood = Mesh::default();
    let mut cards = Mesh::default();
    let sides = if detail >= 0.8 { 7 } else { 5 };
    // trunk
    let n_rings = ((height / 0.55).ceil() as usize).max(4);
    let pts: Vec<V3> = (0..=n_rings).map(|j| axis(height * j as f32 / n_rings as f32)).collect();
    let rad: Vec<f32> = (0..=n_rings).map(|j| radius(height * j as f32 / n_rings as f32)).collect();
    wood.tube(&pts, &rad, sides, 0.8);

    // whorls of branches
    let y0 = crown_base * height;
    let spacing = p.f("whorl_spacing").max(0.15) / detail.sqrt();
    let n_whorls = (((height * 0.985 - y0) / spacing).floor() as usize).max(1);
    let per = p.i("branches").clamp(3, 10) as f32 * detail.sqrt();
    let (droop, lift) = (p.f("droop").to_radians(), p.f("lift").to_radians());
    let (curl, asym, gaps) = (p.f("curl").clamp(0.0, 1.0), p.f("asymmetry").clamp(0.0, 1.0), p.f("gaps").clamp(0.0, 0.5));
    let (card_w, card_l) = (p.f("spray_width") * math::pow(height / 18.0, 0.35), p.f("spray_length") * math::pow(height / 18.0, 0.35));
    let per_m = p.f("sprays_per_m").max(0.2) * detail;
    let mut bid = 0u64;
    for w in 0..n_whorls {
        let y = y0 + (w as f32 + 0.5 * rnd(2, w as u64)) * spacing;
        if y >= height * 0.99 {
            break;
        }
        let t = ((y - y0) / (height * 0.985 - y0)).clamp(0.0, 1.0);
        let l_max = crown_width * height * math::pow((1.0 - t).max(0.0), crown_power) + 0.12 * height * 0.0;
        let nb = ((per + (rnd(3, w as u64) - 0.5) * 2.0).round() as usize).clamp(3, 14);
        let ph = rnd(4, w as u64) * std::f32::consts::TAU;
        for k in 0..nb {
            bid += 1;
            if rnd(5, bid) < gaps {
                continue;
            }
            let phi = ph + std::f32::consts::TAU * k as f32 / nb as f32 + (rnd(6, bid) - 0.5) * 0.6;
            let len = (l_max * (0.72 + 0.5 * rnd(7, bid)) * (1.0 - asym * sq(rnd(8, bid)))).max(0.25);
            let base = add(axis(y + (rnd(9, bid) - 0.5) * spacing * 0.6), [0.0, 0.0, 0.0]);
            let alpha0 = -droop * (1.0 - t) + lift * t + (rnd(10, bid) - 0.5) * 0.25;
            let (sf, cf) = pt_core::math::sin_cos(phi);
            let seg = 6usize;
            let mut bp = vec![base];
            let mut dirs: Vec<V3> = Vec::new();
            let step = len / seg as f32;
            for j in 0..seg {
                let s = (j as f32 + 0.5) / seg as f32;
                let a = alpha0 - 0.22 * s + curl * 0.9 * s * s * s;
                let (sa, ca) = pt_core::math::sin_cos(a);
                let d = [ca * cf, sa, ca * sf];
                dirs.push(d);
                bp.push(add(bp[j], mul(d, step)));
            }
            let rb = (0.010 + 0.035 * len / (crown_width * height).max(0.5)).clamp(0.008, r0 * 0.5);
            let br: Vec<f32> = (0..=seg).map(|j| rb * (1.0 - 0.9 * j as f32 / seg as f32).max(0.08)).collect();
            wood.tube(&bp, &br, 4, 1.2);
            // needle sprays along the branch (from a fifth of it to its tip), each two crossed cards
            let n_sp = ((len * per_m).round() as usize).max(2);
            for q in 0..n_sp {
                let s = (0.2 + 0.8 * (q as f32 + rnd(11, bid * 64 + q as u64)) / n_sp as f32).min(0.99);
                let jf = s * seg as f32;
                let (j, fr) = ((jf.floor() as usize).min(seg - 1), jf - jf.floor());
                let at = add(bp[j], mul(sub(bp[j + 1], bp[j]), fr));
                // each spray points a little away from the branch (sideways, up or down), so the outline is shaggy, not tiered
                let d0 = dirs[j];
                let side = normalize(cross(d0, [0.0, 1.0, 0.0]));
                let (j1, j2) = (rnd(14, bid * 64 + q as u64) - 0.5, rnd(15, bid * 64 + q as u64) - 0.5);
                let d = normalize(add(d0, add(mul(side, j1 * 1.1), [0.0, j2 * 1.1 - 0.12, 0.0])));
                let scale = 0.55 + 0.6 * (1.0 - s * 0.55);
                let (cw, cl) = (card_w * scale * (0.85 + 0.3 * rnd(12, bid * 64 + q as u64)), card_l * scale);
                for c in 0..2 {
                    let roll = rnd(13, bid * 64 + q as u64) * std::f32::consts::TAU + c as f32 * 1.9;
                    let up = [0.0, 1.0, 0.0];
                    let l0 = normalize(cross(d, up));
                    let l1 = normalize(cross(d, l0));
                    let (sr, cr) = pt_core::math::sin_cos(roll);
                    let lat = add(mul(l0, cr), mul(l1, sr));
                    let tail = sub(at, mul(d, cl * 0.12));
                    let tip = add(tail, mul(d, cl));
                    let (hw, ml) = (cw * 0.5, mul(lat, cw * 0.5));
                    let _ = hw;
                    let out = normalize([at[0] - axis(at[1])[0], 0.0, at[2] - axis(at[1])[2]]);
                    let nrm = normalize(add(mul(out, 0.7), [0.0, 0.55, 0.0]));
                    cards.card([sub(tail, ml), add(tail, ml), add(tip, ml), sub(tip, ml)], nrm);
                }
            }
        }
    }
    // the leader: a last spray pointing at the sky
    let top = axis(height);
    for c in 0..2 {
        let roll = c as f32 * 1.57;
        let (sr, cr) = pt_core::math::sin_cos(roll);
        let lat = [cr, 0.0, sr];
        let ml = mul(lat, card_w * 0.4);
        let tail = sub(top, [0.0, card_l * 0.9, 0.0]);
        cards.card([sub(tail, ml), add(tail, ml), add(top, ml), sub(top, ml)], [0.0, 1.0, 0.0]);
    }

    let mut g = wood.geometry(&p.s("bark_material"));
    g.append(&cards.geometry(&p.s("needle_material")))?;
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook;

    fn make(set: &[(&str, serde_json::Value)]) -> Geometry {
        let mut p = Params::new();
        for s in crate::schema("tree_conifer").unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        for (k, v) in set {
            p.insert((*k).into(), v.clone());
        }
        cook("tree_conifer", &p, &[]).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_tree_has_the_size_the_knobs_say_two_materials_and_uvs() {
        let g = make(&[("height", json!(20.0))]);
        let (lo, hi) = g.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 20.0).abs() < 1.5, "height {}", hi[1] - lo[1]);
        assert!(lo[1] > -0.2, "the foot is on the ground");
        let mats = g.material_names();
        assert!(mats.contains(&"bark".to_string()) && mats.contains(&"conifer_needles".to_string()), "{mats:?}");
        assert!(matches!(g.point.get("uv"), Some(Column::V2(_))));
        let tris = g.triangles().len();
        assert!((6_000..80_000).contains(&tris), "{tris} triangles");
        assert!(g.p().iter().all(|q| q.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn knobs_change_the_shape_and_seeds_change_the_tree() {
        let width = |g: &Geometry| {
            let (lo, hi) = g.base_bounds().unwrap();
            (hi[0] - lo[0]).max(hi[2] - lo[2])
        };
        let spruce = make(&[("crown_width", json!(0.18)), ("seed", json!(1))]);
        let pine = make(&[("crown_width", json!(0.42)), ("seed", json!(1))]);
        assert!(width(&pine) > width(&spruce) * 1.5, "{} vs {}", width(&pine), width(&spruce));
        let a = make(&[("seed", json!(1))]);
        assert_eq!(a.content_hash(), make(&[("seed", json!(1))]).content_hash(), "deterministic");
        assert_ne!(a.content_hash(), make(&[("seed", json!(2))]).content_hash());
        let far = make(&[("detail", json!(0.4))]);
        assert!(far.triangles().len() * 2 < a.triangles().len(), "a distant tree is cheaper");
    }
}
