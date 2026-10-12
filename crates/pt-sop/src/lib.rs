//! Geometry operators (SOPs). Each op is a pure, deterministic function: params + input
//! geometry -> geometry. Pointwise ops also expose a `PointOp` list so the graph can fuse them.
//! `flow` predicts attributes per node without cooking (the attribute-flow type system).

mod assets;
mod attrs;
pub mod dem;
mod hydro;
pub mod hfield;
mod deform;
pub mod flow;
pub mod kernel;
mod prims;
mod refindex;
mod sdf;
pub mod sparse;
mod sim;
mod terrain;
mod topo;

use kernel::PointOp;
use pt_core::math::{self, Mat34};
use pt_core::rng::rand4;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, Instances, NodeSchema, ParamExt, ParamSchema, Params};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub(crate) fn ps(name: &'static str, default: Value, doc: &'static str) -> ParamSchema {
    ParamSchema { name, default, doc }
}

pub(crate) fn sch(kind: &'static str, doc: &'static str, inputs: usize, diff: D, granularity: G, params: Vec<ParamSchema>) -> NodeSchema {
    NodeSchema { kind, doc, version: 1, inputs, diff, granularity, params }
}

fn base_schemas() -> Vec<NodeSchema> {
    vec![
        sch("box", "Axis-aligned box.", 0, D::Smooth, G::Whole, vec![
            ps("size", json!([1.0, 1.0, 1.0]), "Edge lengths in meters (x,y,z)."),
            ps("center", json!([0.0, 0.0, 0.0]), "Box center."),
        ]),
        sch("sphere", "UV sphere (poles on Y).", 0, D::Smooth, G::Whole, vec![
            ps("radius", json!(0.5), "Sphere radius in meters."),
            ps("rows", json!(12), "Latitude divisions (>=2)."),
            ps("cols", json!(24), "Longitude divisions (>=3)."),
            ps("center", json!([0.0, 0.0, 0.0]), "Sphere center."),
        ]),
        sch("grid", "Flat grid of quads in the XZ plane, normal +Y.", 0, D::Smooth, G::Whole, vec![
            ps("size", json!([10.0, 10.0]), "Extent along X and Z in meters."),
            ps("rows", json!(10), "Quads along Z."),
            ps("cols", json!(10), "Quads along X."),
            ps("center", json!([0.0, 0.0, 0.0]), "Grid center."),
        ]),
        sch("transform", "Scale, then rotate (XYZ, degrees), then translate. Also moves instances. Fusable.", 1, D::Smooth, G::PerPoint, vec![
            ps("t", json!([0.0, 0.0, 0.0]), "Translate."),
            ps("r", json!([0.0, 0.0, 0.0]), "Rotate in degrees."),
            ps("s", json!([1.0, 1.0, 1.0]), "Scale."),
            ps("uniform_scale", json!(1.0), "Extra uniform scale."),
        ]),
        sch("noise", "Displace points along `direction` by fractal value noise. Fusable.", 1, D::Piecewise, G::PerPoint, vec![
            ps("amp", json!(0.5), "Displacement amplitude in meters."),
            ps("freq", json!(1.0), "Spatial frequency (1/m)."),
            ps("octaves", json!(3), "Fractal octaves (1-8)."),
            ps("direction", json!([0.0, 1.0, 0.0]), "Displacement direction."),
            ps("offset", json!([0.0, 0.0, 0.0]), "Noise space offset."),
            ps("seed", json!(1), "Noise seed."),
        ]),
        sch("merge", "Combine up to 4 geometries (points, prims, attributes, instances).", 4, D::Smooth, G::Whole, vec![]),
        sch("scatter", "Scatter random points over the input surface, area-weighted. Writes point attrs `pscale` (random in [min,max]) and `N` (face normal).", 1, D::None, G::Whole, vec![
            ps("count", json!(100), "Number of points."),
            ps("seed", json!(1), "Random seed."),
            ps("pscale_min", json!(1.0), "Min random pscale."),
            ps("pscale_max", json!(1.0), "Max random pscale."),
        ]),
        sch("copy_to_points", "Instance input 0 onto every point of input 1 (lazy: builds an instance table, no geometry copies). Honors `pscale`; copies target point attributes onto instances.", 2, D::Piecewise, G::PerPoint, vec![
            ps("rot_y_random", json!(0.0), "Random Y rotation range in degrees (0..value)."),
            ps("orient_to_normal", json!(0), "1 = rotate +Y onto target `N`."),
            ps("seed", json!(1), "Random seed."),
        ]),
        sch("copy_variants", "Like copy_to_points, but with up to 3 prototypes (inputs 1-3) picked per target point by weight, so a forest or meadow is not one tree repeated. Input 0 = target points; honors `pscale` and `N`.", 4, D::None, G::PerPoint, vec![
            ps("weights", json!([1.0, 1.0, 1.0]), "Relative chance of prototype 1, 2, 3 (extra entries ignored; unconnected prototypes are skipped)."),
            ps("rot_y_random", json!(360.0), "Random Y rotation range in degrees (0..value)."),
            ps("orient_to_normal", json!(0), "1 = rotate +Y onto target `N`."),
            ps("seed", json!(1), "Random seed."),
        ]),
        sch("realize", "Turn instances into real geometry.", 1, D::Piecewise, G::Whole, vec![]),
        sch("normals", "Compute area-weighted point normals into point attr `N`.", 1, D::Piecewise, G::PerPrim, vec![]),
        sch("color_by_height", "Write point attr `Cd` by blending color0->color1 across a Y range (auto range if min==max).", 1, D::Smooth, G::PerPoint, vec![
            ps("color0", json!([0.15, 0.2, 0.35]), "Color at low end."),
            ps("color1", json!([0.95, 0.9, 0.75]), "Color at high end."),
            ps("min", json!(0.0), "Low Y."),
            ps("max", json!(0.0), "High Y (equal to min = auto from bounds)."),
        ]),
        sch("null", "Pass-through; use as a named output marker.", 1, D::Smooth, G::Whole, vec![]),
    ]
}

pub fn schemas() -> Vec<NodeSchema> {
    let mut v = base_schemas();
    v.extend(prims::schemas());
    v.extend(deform::schemas());
    v.extend(assets::schemas());
    v.extend(dem::schemas());
    v.extend(hydro::schemas());
    v.extend(hfield::schemas());
    v.extend(attrs::schemas());
    v.extend(topo::schemas());
    v.extend(terrain::schemas());
    v.extend(sdf::schemas());
    v.extend(sparse::schemas());
    v.extend(sim::schemas());
    v
}

pub fn schema(kind: &str) -> Option<NodeSchema> {
    schemas().into_iter().find(|s| s.kind == kind)
}

/// Minimum number of connected inputs needed to cook (merge accepts 1..4).
pub fn min_inputs(kind: &str) -> usize {
    match kind {
        "merge" | "pop_sim" | "cloth_sim" | "switch" | "wrangle" => 1,
        "copy_variants" => 2,
        _ => schema(kind).map_or(0, |s| s.inputs),
    }
}

/// Ops whose result depends on the playback frame (the graph injects `__frame` for them).
pub fn uses_frame(kind: &str) -> bool {
    matches!(kind, "wrangle" | "group_by_expression" | "pop_sim" | "cloth_sim" | "smoke_sim" | "rbd_sim")
}

/// Fusable ops for pointwise node kinds (None for everything else).
pub fn point_ops(kind: &str, p: &Params) -> Option<Vec<PointOp>> {
    match kind {
        "transform" => {
            let u = p.f("uniform_scale");
            let s = p.v3("s");
            Some(vec![PointOp::Affine(math::trs(p.v3("t"), p.v3("r"), [s[0] * u, s[1] * u, s[2] * u]))])
        }
        "noise" => Some(vec![PointOp::Noise {
            amp: p.f("amp"),
            freq: p.f("freq"),
            octaves: p.i("octaves").clamp(1, 8) as u32,
            dir: p.v3("direction"),
            off: p.v3("offset"),
            seed: p.i("seed") as u32,
        }]),
        _ => None,
    }
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Res {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    if let Some(ops) = point_ops(kind, p) {
        return Ok(kernel::apply(inp(0)?, &ops));
    }
    match kind {
        "box" => return Ok(make_box(p)),
        "sphere" => return Ok(make_sphere(p)),
        "grid" => return Ok(make_grid(p)),
        "merge" => {
            let mut g = Geometry::new();
            for i in inputs.iter().flatten() {
                g.append(i)?;
            }
            return Ok(g);
        }
        "scatter" => return scatter(inp(0)?, p),
        "copy_to_points" => return copy_to_points(inp(0)?, inp(1)?, p),
        "copy_variants" => return copy_variants(inputs, p),
        "realize" => return Ok(inp(0)?.realize()),
        "normals" => {
            let mut g = inp(0)?.clone();
            let n = point_normals(&g);
            g.point.set("N", Column::from_v3(n));
            return Ok(g);
        }
        "color_by_height" => return Ok(color_by_height(inp(0)?, p)),
        "null" => return Ok(inp(0)?.clone()),
        _ => {}
    }
    if let Some(r) = prims::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = deform::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = assets::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = dem::cook(kind, p) {
        return r;
    }
    if let Some(r) = hydro::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = hfield::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = attrs::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = sim::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = sparse::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = sdf::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = terrain::cook(kind, p, inputs) {
        return r;
    }
    if let Some(r) = topo::cook(kind, p, inputs) {
        return r;
    }
    Err(format!("unknown node type '{kind}'"))
}

pub(crate) fn make_box(p: &Params) -> Geometry {
    let (s, c) = (p.v3("size"), p.v3("center"));
    let mut pts = Vec::new();
    for i in 0..8u32 {
        let b = |bit: u32, a: usize| c[a] + if i >> bit & 1 == 1 { 0.5 } else { -0.5 } * s[a];
        pts.push([b(0, 0), b(1, 1), b(2, 2)]);
    }
    let faces: Vec<Vec<u32>> = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]]
        .iter()
        .map(|f| f.to_vec())
        .collect();
    Geometry::from_polys(pts, &faces)
}

fn make_sphere(p: &Params) -> Geometry {
    let r = p.f("radius");
    let rows = p.i("rows").max(2) as u32;
    let cols = p.i("cols").max(3) as u32;
    let c = p.v3("center");
    let mut pts = vec![[c[0], c[1] + r, c[2]]];
    for j in 1..rows {
        let (st, ct) = math::sin_cos(math::PI * j as f32 / rows as f32);
        for i in 0..cols {
            let (sp, cp) = math::sin_cos(2.0 * math::PI * i as f32 / cols as f32);
            pts.push([c[0] + r * st * cp, c[1] + r * ct, c[2] + r * st * sp]);
        }
    }
    pts.push([c[0], c[1] - r, c[2]]);
    let ring = |j: u32, i: u32| 1 + (j - 1) * cols + i % cols;
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for i in 0..cols {
        polys.push(vec![0, ring(1, i + 1), ring(1, i)]);
    }
    for j in 1..rows - 1 {
        for i in 0..cols {
            polys.push(vec![ring(j, i), ring(j, i + 1), ring(j + 1, i + 1), ring(j + 1, i)]);
        }
    }
    let bottom = pts.len() as u32 - 1;
    for i in 0..cols {
        polys.push(vec![bottom, ring(rows - 1, i), ring(rows - 1, i + 1)]);
    }
    Geometry::from_polys(pts, &polys)
}

fn make_grid(p: &Params) -> Geometry {
    let sz = p.v3("size");
    let (rows, cols) = (p.i("rows").max(1) as u32, p.i("cols").max(1) as u32);
    let c = p.v3("center");
    let mut pts = Vec::new();
    for j in 0..=rows {
        for i in 0..=cols {
            pts.push([c[0] + (i as f32 / cols as f32 - 0.5) * sz[0], c[1], c[2] + (j as f32 / rows as f32 - 0.5) * sz[1]]);
        }
    }
    let w = cols + 1;
    let mut polys = Vec::new();
    for j in 0..rows {
        for i in 0..cols {
            let a = j * w + i;
            polys.push(vec![a, a + w, a + w + 1, a + 1]);
        }
    }
    let mut g = Geometry::from_polys(pts, &polys);
    g.detail.insert("grid".into(), json!({"rows": rows, "cols": cols}));
    g
}

type Sample = ([f32; 3], [f32; 3], [f32; 4]);

/// Area-weighted random surface samples (position, face normal, 4 randoms), deterministic.
pub(crate) fn surface_samples(src: &Geometry, n: usize, seed: i64) -> Result<Vec<Sample>, String> {
    let tris = src.triangles();
    if tris.is_empty() {
        return Err("input has no surface to scatter on".into());
    }
    let sp = src.p();
    let mut cum = Vec::with_capacity(tris.len());
    let mut total = 0.0f32;
    for t in &tris {
        let (a, b, c) = (sp[t[0] as usize], sp[t[1] as usize], sp[t[2] as usize]);
        let cr = math::cross(math::sub(b, a), math::sub(c, a));
        total += 0.5 * (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
        cum.push(total);
    }
    if total <= 0.0 {
        return Err("input surface has zero area".into());
    }
    Ok((0..n)
        .into_par_iter()
        .with_min_len(4096)
        .map(|i| {
            let r = rand4(seed, 0, i as u64);
            let ti = cum.partition_point(|&c| c < r[0] * total).min(tris.len() - 1);
            let t = tris[ti];
            let (mut u, mut v) = (r[1], r[2]);
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            let (a, b, c) = (sp[t[0] as usize], sp[t[1] as usize], sp[t[2] as usize]);
            let pos = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * u + (c[k] - a[k]) * v);
            (pos, math::normalize(math::cross(math::sub(b, a), math::sub(c, a))), r)
        })
        .collect())
}

fn scatter(src: &Geometry, p: &Params) -> Res {
    let s = surface_samples(src, p.i("count").max(0) as usize, p.i("seed")).map_err(|e| format!("scatter: {e}"))?;
    let (lo, hi) = (p.f("pscale_min"), p.f("pscale_max"));
    let mut g = Geometry::from_points(s.iter().map(|x| x.0).collect());
    g.point.set("N", Column::from_v3(s.iter().map(|x| x.1).collect()));
    g.point.set("pscale", Column::from_f32(s.iter().map(|x| lo + (hi - lo) * x.2[3]).collect()));
    Ok(g)
}

/// One transform per target point: random Y rotation, optional alignment of +Y to `N`, scale from `pscale`, translation from P.
fn instance_xforms(targets: &Geometry, p: &Params) -> Vec<Mat34> {
    let seed = p.i("seed");
    let rot = math::to_radians(p.f("rot_y_random"));
    let orient = p.i("orient_to_normal") != 0;
    let ps = targets.point.get("pscale").and_then(Column::as_f32);
    let nrm = targets.point.get("N").and_then(Column::as_v3);
    let tp = targets.p();
    (0..tp.len())
        .into_par_iter()
        .with_min_len(4096)
        .map(|k| {
            let s = ps.map_or(1.0, |a| a[k]);
            let r = rand4(seed, 1, k as u64);
            let (sn, cs) = math::sin_cos(r[0] * rot);
            let ry: Mat34 = [cs, 0.0, sn, 0.0, 0.0, 1.0, 0.0, 0.0, -sn, 0.0, cs, 0.0];
            let mut m = ry;
            if orient {
                if let Some(n) = nrm {
                    m = math::mul34(&align_y_to(n[k]), &m);
                }
            }
            for row in 0..3 {
                for col in 0..3 {
                    m[row * 4 + col] *= s;
                }
            }
            m[3] = tp[k][0];
            m[7] = tp[k][1];
            m[11] = tp[k][2];
            m
        })
        .collect()
}

fn instance_attrs(targets: &Geometry) -> pt_core::AttrSet {
    let mut attrs = targets.point.clone();
    attrs.remove("P");
    attrs.remove("__src");
    attrs
}

fn copy_to_points(src: &Geometry, targets: &Geometry, p: &Params) -> Res {
    let xf = instance_xforms(targets, p);
    let n = xf.len();
    let mut g = Geometry::new();
    g.instances = Some(Instances { protos: vec![Arc::new(src.clone())], proto_id: Arc::new(vec![0; n]), xf: Arc::new(xf), attrs: instance_attrs(targets) });
    Ok(g)
}

/// Several prototypes, one picked per point by weight (deterministic: Philox keyed by seed and point index).
fn copy_variants(inputs: Inputs, p: &Params) -> Res {
    let targets = inputs.first().and_then(|g| g.as_deref()).ok_or("copy_variants: input 0 (target points) is not connected")?;
    let protos: Vec<Arc<Geometry>> = inputs.iter().skip(1).flatten().cloned().collect();
    if protos.is_empty() {
        return Err("copy_variants: connect at least one prototype to input 1".into());
    }
    let w: Vec<f32> = (0..protos.len()).map(|i| p.get("weights").and_then(|v| v.get(i)).and_then(Value::as_f64).map_or(1.0, |x| x.max(0.0) as f32)).collect();
    let total: f32 = w.iter().sum();
    if total <= 0.0 {
        return Err("copy_variants: weights must not all be zero".into());
    }
    // a prototype with weight 0 is never picked, so it is not carried at all (smaller exports, honest statistics)
    let (protos, w): (Vec<Arc<Geometry>>, Vec<f32>) = protos.into_iter().zip(w).filter(|(_, wi)| *wi > 0.0).unzip();
    let seed = p.i("seed");
    let proto_id: Vec<u32> = (0..targets.n_points())
        .map(|k| {
            let t = rand4(seed, 7, k as u64)[0] * total;
            let (mut acc, mut pick) = (0.0f32, protos.len() - 1);
            for (i, wi) in w.iter().enumerate() {
                acc += wi;
                if t < acc {
                    pick = i;
                    break;
                }
            }
            pick as u32
        })
        .collect();
    let xf = instance_xforms(targets, p);
    let mut g = Geometry::new();
    g.instances = Some(Instances { protos, proto_id: Arc::new(proto_id), xf: Arc::new(xf), attrs: instance_attrs(targets) });
    Ok(g)
}

/// Rotation taking +Y onto unit vector `n`.
fn align_y_to(n: [f32; 3]) -> Mat34 {
    let n = math::normalize(n);
    let helper = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
    let t = math::normalize(math::cross(helper, n));
    let b = math::cross(n, t);
    [t[0], n[0], b[0], 0.0, t[1], n[1], b[1], 0.0, t[2], n[2], b[2], 0.0]
}

pub(crate) fn point_normals(src: &Geometry) -> Vec<[f32; 3]> {
    let mut acc = vec![[0.0f32; 3]; src.n_points()];
    let sp = src.p();
    for i in 0..src.n_prims() {
        let pr = src.prim_points(i);
        if pr.len() < 3 {
            continue;
        }
        let p0 = sp[pr[0] as usize];
        let mut fnorm = [0.0f32; 3];
        for k in 1..pr.len() - 1 {
            let c = math::cross(math::sub(sp[pr[k] as usize], p0), math::sub(sp[pr[k + 1] as usize], p0));
            for j in 0..3 {
                fnorm[j] += c[j];
            }
        }
        for &v in pr {
            for j in 0..3 {
                acc[v as usize][j] += fnorm[j];
            }
        }
    }
    acc.into_iter().map(math::normalize).collect()
}

fn color_by_height(src: &Geometry, p: &Params) -> Geometry {
    let (c0, c1) = (p.v3("color0"), p.v3("color1"));
    let (mut lo, mut hi) = (p.f("min"), p.f("max"));
    if hi <= lo {
        if let Some((l, h)) = src.base_bounds() {
            lo = l[1];
            hi = h[1];
        }
    }
    let span = if hi > lo { hi - lo } else { 1.0 };
    let cd: Vec<[f32; 3]> = src
        .p()
        .iter()
        .map(|q| {
            let t = ((q[1] - lo) / span).clamp(0.0, 1.0);
            [c0[0] + (c1[0] - c0[0]) * t, c0[1] + (c1[1] - c0[1]) * t, c0[2] + (c1[2] - c0[2]) * t]
        })
        .collect();
    let mut g = src.clone();
    g.point.set("Cd", Column::from_v3(cd));
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cook1(kind: &str, set: &[(&str, Value)], inputs: &[Geometry]) -> Geometry {
        let mut p = Params::new();
        for s in schema(kind).unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        for (k, v) in set {
            p.insert((*k).into(), v.clone());
        }
        let ins: Vec<Option<Arc<Geometry>>> = inputs.iter().map(|g| Some(Arc::new(g.clone()))).collect();
        cook(kind, &p, &ins).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    /// Signed volume via the divergence theorem: positive when faces wind outward.
    fn signed_volume(g: &Geometry) -> f32 {
        let p = g.p();
        g.triangles().iter().map(|t| {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let cr = math::cross(b, c);
            (a[0] * cr[0] + a[1] * cr[1] + a[2] * cr[2]) / 6.0
        }).sum()
    }

    #[test]
    fn deformers_curves_and_placement() {
        let pi = std::f32::consts::PI;
        // helix + sweep: a swept straight pipe has the volume of a tube and winds outward.
        let seg = cook1("curve", &[("points", json!([[0.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 4.0, 0.0]]))], &[]);
        let pipe = cook1("sweep", &[("radius", json!(0.5)), ("cols", json!(64))], &[seg]);
        let v = signed_volume(&pipe);
        assert!((v - pi * 0.25 * 4.0).abs() < 0.05, "pipe volume {v}");
        assert!(pt_core::validate::validate(&pipe).iter().all(|i| i.severity != "error"));
        // revolve a rectangle profile (poles on the axis): closed cylinder of r=1, h=2.
        let prof = cook1("curve", &[("points", json!([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 2.0, 0.0], [0.0, 2.0, 0.0]]))], &[]);
        let lathe = cook1("revolve", &[("divisions", json!(96))], &[prof]);
        let v = signed_volume(&lathe);
        assert!((v - 2.0 * pi).abs() < 0.1, "lathe volume {v}");
        // resample gives exactly `count` points on the original length.
        let line = cook1("line", &[("to", json!([0.0, 3.0, 0.0])), ("segments", json!(3))], &[]);
        let rs = cook1("resample", &[("count", json!(7))], &[line]);
        assert_eq!(rs.n_points(), 7);
        assert!((rs.p()[3][1] - 1.5).abs() < 1e-5);
        // bend with angle 0 is the identity; twist keeps the axis coordinate and radius.
        let b = cook1("box", &[], &[]);
        assert_eq!(cook1("bend", &[("angle", json!(0.0))], &[b.clone()]).p(), b.p());
        let t = cook1("twist", &[("angle", json!(45.0))], &[b.clone()]);
        for (q, o) in t.p().iter().zip(b.p()) {
            assert!((q[1] - o[1]).abs() < 1e-6);
            assert!((q[0].hypot(q[2]) - o[0].hypot(o[2])).abs() < 1e-5);
        }
        // fit_size: target height 3, uniform, grounded.
        let f = cook1("fit_size", &[("size", json!([0.0, 3.0, 0.0]))], &[b.clone()]);
        let (lo, hi) = f.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 3.0).abs() < 1e-5 && lo[1].abs() < 1e-5 && (hi[0] - lo[0] - 3.0).abs() < 1e-5);
        // drop_to_surface: points land on a tilted plane y = x.
        let mut grid = cook1("grid", &[("size", json!([10.0, 10.0])), ("rows", json!(4)), ("cols", json!(4))], &[]);
        for q in grid.p_mut() {
            q[1] = q[0];
        }
        let pts = Geometry::from_points(vec![[1.3, 9.0, 0.7], [-2.1, -5.0, 3.3], [50.0, 0.0, 0.0]]);
        let d = cook1("drop_to_surface", &[], &[pts, grid]);
        assert!((d.p()[0][1] - 1.3).abs() < 1e-4 && (d.p()[1][1] + 2.1).abs() < 1e-4);
        assert_eq!(d.p()[2][1], 0.0, "points outside the surface stay put");
        assert!(d.point.get("N").is_some());
        // jitter is deterministic and bounded.
        let j1 = cook1("point_jitter", &[("amount", json!(0.2))], &[b.clone()]);
        let j2 = cook1("point_jitter", &[("amount", json!(0.2))], &[b.clone()]);
        assert_eq!(j1.p(), j2.p());
        assert!(j1.p().iter().zip(b.p()).all(|(q, o)| (0..3).all(|a| (q[a] - o[a]).abs() <= 0.2 + 1e-6)));
    }

    #[test]
    fn closed_primitives_wind_outward_and_are_manifold() {
        for (kind, set, want_vol) in [
            ("box", vec![], 1.0f32),
            ("sphere", vec![("rows", json!(24)), ("cols", json!(48))], 4.0 / 3.0 * std::f32::consts::PI * 0.125),
            ("tube", vec![("cols", json!(64))], std::f32::consts::PI * 0.25),
            ("torus", vec![("rows", json!(32)), ("cols", json!(64))], 2.0 * std::f32::consts::PI * std::f32::consts::PI * 1.0 * 0.0625),
        ] {
            let g = cook1(kind, &set, &[]);
            let v = signed_volume(&g);
            assert!(v > 0.0, "{kind} winds inward (volume {v})");
            assert!((v / want_vol - 1.0).abs() < 0.06, "{kind} volume {v} vs {want_vol}");
            let issues = pt_core::validate::validate(&g);
            assert!(!issues.iter().any(|i| i.severity == "error" || i.code == "non_manifold_edges"), "{kind}: {:?}", issues.iter().map(|i| i.code).collect::<Vec<_>>());
            assert!(issues.iter().any(|i| i.code == "boundary_edges" && i.message.starts_with("0 ")), "{kind} must be watertight");
        }
    }

    #[test]
    fn subdivide_keeps_watertight_and_shrinks_box_volume() {
        let b = cook1("box", &[], &[]);
        let s = cook1("subdivide", &[("levels", json!(2))], &[b.clone()]);
        assert_eq!(s.n_prims(), 6 * 16);
        let v = signed_volume(&s);
        assert!(v > 0.4 && v < 1.0, "volume {v}");
        let issues = pt_core::validate::validate(&s);
        assert!(issues.iter().any(|i| i.code == "boundary_edges" && i.message.starts_with("0 ")));
    }

    #[test]
    fn polyextrude_adds_expected_faces() {
        let g = cook1("grid", &[("rows", json!(2)), ("cols", json!(2))], &[]);
        let e = cook1("polyextrude", &[("distance", json!(0.5))], &[g]);
        assert_eq!(e.n_prims(), 4 * 5);
        assert!(e.p().iter().any(|p| (p[1] - 0.5).abs() < 1e-5 || (p[1] + 0.5).abs() < 1e-5));
    }

    #[test]
    fn fuse_welds_duplicate_points() {
        let b = cook1("box", &[], &[]);
        let two = cook1("merge", &[], &[b.clone(), b]);
        assert_eq!(two.n_points(), 16);
        let f = cook1("fuse", &[], &[two]);
        assert_eq!(f.n_points(), 8);
        assert_eq!(f.n_prims(), 12);
    }

    #[test]
    fn blast_by_expression_group() {
        let g = cook1("grid", &[("rows", json!(4)), ("cols", json!(4)), ("size", json!([4.0, 4.0]))], &[]);
        let g = cook1("group_by_expression", &[("name", json!("left")), ("expression", json!("@P.x < 0.0"))], &[g]);
        assert!(g.point_groups["left"].iter().filter(|b| **b).count() > 0);
        let n0 = g.n_points();
        let b = cook1("blast", &[("group", json!("left"))], &[g]);
        assert!(b.n_points() < n0 && b.p().iter().all(|p| p[0] >= 0.0));
    }

    #[test]
    fn set_material_is_exclusive_survives_merge_and_realize() {
        let b = cook1("box", &[], &[]);
        let all = cook1("set_material", &[("name", json!("stone"))], &[b.clone()]);
        assert_eq!(all.material_names(), ["stone"]);
        // a point group re-binds only the faces whose points are all inside it
        let g = cook1("group_by_expression", &[("name", json!("top")), ("expression", json!("@P.y > 0.0"))], &[all]);
        let top = cook1("set_material", &[("name", json!("grass")), ("group", json!("top")), ("class", json!("point"))], &[g]);
        let m = top.prim_materials();
        assert_eq!(m.iter().filter(|x| **x == Some("grass")).count(), 1, "only the top face has all four points above 0");
        assert_eq!(m.iter().filter(|x| **x == Some("stone")).count(), 5);
        // materials survive merge with an unpainted box, and realize of instances
        let both = cook1("merge", &[], &[top.clone(), b.clone()]);
        assert_eq!(both.n_prims(), 12);
        assert_eq!(both.prim_materials().iter().filter(|x| x.is_none()).count(), 6);
        let pts = cook1("scatter", &[("count", json!(5))], &[cook1("grid", &[], &[])]);
        let inst = cook1("copy_to_points", &[], &[top, pts]);
        let real = inst.realize();
        assert_eq!(real.prim_materials().iter().filter(|x| **x == Some("grass")).count(), 5);
        // painting an instanced geometry (no group) reaches the prototypes
        let painted = cook1("set_material", &[("name", json!("brick"))], &[inst.clone()]);
        assert!(painted.realize().prim_materials().iter().all(|x| *x == Some("brick")));
        // bad names and missing groups say what to do
        let mut p = Params::new();
        p.insert("name".into(), json!("Rock Granite"));
        assert!(cook("set_material", &p, &[Some(Arc::new(b.clone()))]).unwrap_err().contains("valid material name"));
        p.insert("name".into(), json!("ok_name"));
        p.insert("group".into(), json!("nope"));
        p.insert("class".into(), json!("prim"));
        assert!(cook("set_material", &p, &[Some(Arc::new(b))]).unwrap_err().contains("does not exist"));
    }

    #[test]
    fn blend_materials_normalises_weights_and_exports_them() {
        let g = cook1("grid", &[("rows", json!(4)), ("cols", json!(4)), ("size", json!([4.0, 4.0]))], &[]);
        let w = cook1("wrangle", &[("code", json!("@w_grass = 2.0; @w_rock = smoothstep(-1.0, 1.0, @P.x); @w_snow = 0.0;"))], &[g.clone()]);
        let b = cook1("blend_materials", &[("name", json!("terrain_blend")), ("layers", json!("grass rock snow")), ("group", json!("")), ("class", json!("prim"))], &[w.clone()]);
        let Some(pt_core::Column::V4(wt)) = b.point.get("Wt0") else { panic!("Wt0 missing") };
        for v in wt.iter() {
            assert!((v[0] + v[1] + v[2] + v[3] - 1.0).abs() < 1e-5, "weights sum to 1: {v:?}");
        }
        let left = b.p().iter().position(|p| p[0] < -1.9).unwrap();
        let right = b.p().iter().position(|p| p[0] > 1.9).unwrap();
        assert!(wt[left][1] < 0.01 && wt[right][1] > 0.25, "rock grows with x: {:?} {:?}", wt[left], wt[right]);
        assert!(b.point.get("Wt1").is_none(), "three layers need one weight column");
        let Some(pt_core::Column::V2(uv)) = b.point.get("uv") else { panic!("planar uv missing") };
        assert!(uv.iter().all(|t| (0.0..=1.0).contains(&t[0]) && (0.0..=1.0).contains(&t[1])) && uv.iter().any(|t| t[0] < 0.01) && uv.iter().any(|t| t[0] > 0.99), "uv spans the footprint");
        assert_eq!(b.material_names(), ["terrain_blend"]);
        assert_eq!(b.detail["layers:terrain_blend"], json!(["grass", "rock", "snow"]));
        let (doc, _) = pt_core::gltf::build(&b, 10);
        assert!(doc["meshes"][0]["primitives"][0]["attributes"].get("_WEIGHTS_0").is_some());
        assert_eq!(doc["materials"][0]["extras"]["proteus_layers"][1], "rock");
        // all weights missing, a single layer, and a bad role all say what to do
        let mut p = Params::new();
        for (k, v) in [("name", json!("t")), ("layers", json!("a b")), ("group", json!("")), ("class", json!("prim"))] {
            p.insert(k.into(), v);
        }
        assert!(cook("blend_materials", &p, &[Some(Arc::new(g.clone()))]).unwrap_err().contains("write them with a wrangle"));
        p.insert("layers".into(), json!("only_one"));
        assert!(cook("blend_materials", &p, &[Some(Arc::new(w.clone()))]).unwrap_err().contains("2 to 8"));
        p.insert("layers".into(), json!("a bad-role"));
        assert!(cook("blend_materials", &p, &[Some(Arc::new(w))]).unwrap_err().contains("not a valid role name"));
    }

    #[test]
    fn copy_variants_picks_by_weight_and_is_deterministic() {
        let pts = cook1("scatter", &[("count", json!(3000)), ("seed", json!(5))], &[cook1("grid", &[], &[])]);
        let (a, b) = (cook1("box", &[], &[]), cook1("sphere", &[], &[]));
        let params = [("weights", json!([3.0, 1.0])), ("seed", json!(2)), ("rot_y_random", json!(360.0)), ("orient_to_normal", json!(0))];
        let g = cook1("copy_variants", &params, &[pts.clone(), a.clone(), b.clone()]);
        let inst = g.instances.as_ref().unwrap();
        assert_eq!((inst.protos.len(), inst.len()), (2, 3000));
        let n0 = inst.proto_id.iter().filter(|&&i| i == 0).count() as f32 / 3000.0;
        assert!((n0 - 0.75).abs() < 0.05, "weights 3:1 should give about 75% of the first prototype, got {n0}");
        let again = cook1("copy_variants", &params, &[pts.clone(), a.clone(), b]);
        assert_eq!(again.content_hash(), cook1("copy_variants", &params, &[pts.clone(), a.clone(), cook1("sphere", &[], &[])]).content_hash());
        // a zero-weight prototype is dropped, not carried around unused
        let zero = cook1("copy_variants", &[("weights", json!([1.0, 0.0])), ("seed", json!(2)), ("rot_y_random", json!(0.0)), ("orient_to_normal", json!(0))], &[pts.clone(), a.clone(), cook1("sphere", &[], &[])]);
        let zi = zero.instances.as_ref().unwrap();
        assert_eq!((zi.protos.len(), zi.proto_id.iter().filter(|&&i| i != 0).count()), (1, 0));
        // an unconnected middle slot is skipped; no prototype at all is an error that says what to do
        let single = cook("copy_variants", &Params::from_iter([("weights".to_string(), json!([1.0])), ("seed".to_string(), json!(1)), ("rot_y_random".to_string(), json!(0.0)), ("orient_to_normal".to_string(), json!(0))]), &[Some(Arc::new(pts.clone())), None, Some(Arc::new(a))]).unwrap();
        assert_eq!(single.instances.unwrap().protos.len(), 1);
        let err = cook("copy_variants", &Params::new(), &[Some(Arc::new(pts))]).unwrap_err();
        assert!(err.contains("connect at least one prototype"), "{err}");
    }

    #[test]
    fn poisson_respects_min_distance() {
        let g = cook1("grid", &[("size", json!([10.0, 10.0]))], &[]);
        let s = cook1("scatter_poisson", &[("count", json!(200)), ("min_dist", json!(1.0))], &[g]);
        let p = s.p();
        assert!(p.len() > 20);
        for i in 0..p.len() {
            for j in i + 1..p.len() {
                let d = math::sub(p[i], p[j]);
                assert!(d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= 0.999, "points {i},{j} too close");
            }
        }
    }

    #[test]
    fn wrangle_modifies_points_and_instances() {
        let g = cook1("grid", &[("size", json!([4.0, 4.0]))], &[]);
        let w = cook1("wrangle", &[("code", json!("@P.y = @P.x * ch(\"k\"); @Cd = vec3(@P.x, 0.0, 0.0);")), ("channels", json!({"k": 2.0}))], &[g.clone()]);
        assert!(w.p().iter().all(|p| (p[1] - p[0] * 2.0).abs() < 1e-5));
        assert!(w.point.get("Cd").is_some());
        let s = cook1("scatter", &[("count", json!(50))], &[g]);
        let b = cook1("box", &[], &[]);
        let c = cook1("copy_to_points", &[], &[b, s]);
        let w = cook1("wrangle", &[("class", json!("instance")), ("code", json!("@P.y += 10.0;"))], &[c]);
        assert!(w.instances.unwrap().xf.iter().all(|m| m[7] > 9.0));
    }

    fn col(g: &Geometry, name: &str) -> Vec<f32> {
        g.point.get(name).unwrap_or_else(|| panic!("no attribute {name}")).as_f32().unwrap().to_vec()
    }

    #[test]
    fn wrangle_asks_its_second_input() {
        // a ground tilted to y = 0.1 x that carries w = x
        let flat = cook1("grid", &[("size", json!([10.0, 10.0])), ("rows", json!(10)), ("cols", json!(10))], &[]);
        let ground = cook1("wrangle", &[("code", json!("@P.y = @P.x * 0.1; @w = @P.x;"))], &[flat]);
        let spread = cook1("grid", &[("size", json!([6.0, 6.0])), ("rows", json!(3)), ("cols", json!(3))], &[]);

        // rays from 20 m up: distance and hit height, a miss going up
        let up = cook1("wrangle", &[("code", json!("@P.y = 20.0;"))], &[spread.clone()]);
        let code = "@down = ray_dist(@P, {0.0, -1.0, 0.0}); vec3 hit = ray_pos(@P, {0.0, -1.0, 0.0}); @gy = hit.y; @miss = ray_dist(@P, {0.0, 1.0, 0.0}); @nup = ray_n(@P, {0.0, -1.0, 0.0}).y;";
        let q = cook1("wrangle", &[("code", json!(code))], &[up, ground.clone()]);
        let (down, gy, miss, nup) = (col(&q, "down"), col(&q, "gy"), col(&q, "miss"), col(&q, "nup"));
        for (i, p) in q.p().iter().enumerate() {
            assert!((down[i] - (20.0 - 0.1 * p[0])).abs() < 1e-3, "ray from {p:?}: {}", down[i]);
            assert!((gy[i] - 0.1 * p[0]).abs() < 1e-3 && miss[i] == -1.0 && nup[i] > 0.9);
        }

        // on the ground: the surface is at distance 0 and an attribute is read back where it was written
        let on = cook1("wrangle", &[("code", json!("@P.y = @P.x * 0.1;"))], &[spread.clone()]);
        let q = cook1("wrangle", &[("code", json!("@sd = surf_dist(@P); @wx = surf_f(\"w\", @P); @far = surf_dist(@P + {0.0, 3.0, 0.0});"))], &[on, ground.clone()]);
        let (sd, wx, far) = (col(&q, "sd"), col(&q, "wx"), col(&q, "far"));
        for (i, p) in q.p().iter().enumerate() {
            assert!(sd[i] < 1e-3 && (wx[i] - p[0]).abs() < 1e-3, "at {p:?}: distance {} w {}", sd[i], wx[i]);
            assert!((far[i] - 3.0 / 1.01f32.sqrt()).abs() < 5e-3, "3 m above a 0.1 slope is {} from it", far[i]);
        }

        // asking the same points: crowding counts include the point itself, near_dist_other does not
        let q = cook1("wrangle", &[("code", json!("@crowd = near_count(@P, 2.1); @gap = near_dist_other(@P); @self = near_dist(@P);"))], &[spread.clone(), spread.clone()]);
        let (crowd, gap, me) = (col(&q, "crowd"), col(&q, "gap"), col(&q, "self"));
        assert!(gap.iter().all(|g| (g - 2.0).abs() < 1e-5) && me.iter().all(|m| *m == 0.0));
        assert_eq!((crowd.iter().cloned().fold(0.0, f32::max), crowd.iter().cloned().fold(9.0, f32::min)), (5.0, 3.0), "interior 5, corner 3");

        // instance origins of the second input, with their attributes
        let boxes = cook1("array", &[("count", json!(3)), ("offset", json!([4.0, 0.0, 0.0]))], &[cook1("box", &[], &[])]);
        let probe = cook1("curve", &[("points", json!([[0.5, 0.0, 0.0], [7.0, 0.0, 0.0]]))], &[]);
        let q = cook1("wrangle", &[("ref_class", json!("instance")), ("code", json!("@d = near_dist(@P); @ox = near_pos(@P).x;"))], &[probe, boxes]);
        let (d, ox) = (col(&q, "d"), col(&q, "ox"));
        assert!((d[0] - 0.5).abs() < 1e-5 && (d[1] - 1.0).abs() < 1e-5, "{d:?}");
        assert!((ox[0] - 0.0).abs() < 1e-5 && (ox[1] - 8.0).abs() < 1e-5, "{ox:?}");

        // the same question, the same answer, on any thread count
        let big = cook1("scatter", &[("count", json!(6000)), ("seed", json!(4))], &[cook1("grid", &[("size", json!([20.0, 20.0]))], &[])]);
        let ask = |threads: usize| {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
            pool.install(|| cook1("wrangle", &[("code", json!("@a = near_sum(\"pscale\", @P, 1.5) + near_dist_other(@P) + near_count(@P, 2.0);"))], &[big.clone(), big.clone()]).content_hash())
        };
        assert_eq!(ask(1), ask(4));
    }

    #[test]
    fn wrangle_questions_say_what_to_fix() {
        let g = Arc::new(cook1("grid", &[], &[]));
        let mut p = Params::new();
        p.insert("class".into(), json!("point"));
        p.insert("ref_class".into(), json!("point"));
        p.insert("code".into(), json!("@d = near_dist(@P);"));
        let err = cook("wrangle", &p, &[Some(g.clone())]).unwrap_err();
        assert!(err.contains("second input") && err.contains("connect"), "{err}");
        p.insert("code".into(), json!("@d = surf_f(\"nope\", @P);"));
        let err = cook("wrangle", &p, &[Some(g.clone()), Some(g.clone())]).unwrap_err();
        assert!(err.contains("no attribute \"nope\"") && err.contains("available"), "{err}");
        p.insert("code".into(), json!("@d = near_v(\"P\", @P).x + near_f(\"P\", @P);"));
        let err = cook("wrangle", &p, &[Some(g.clone()), Some(g.clone())]).unwrap_err();
        assert!(err.contains("is a vec3") && err.contains("near_v"), "{err}");
        p.insert("code".into(), json!("@d = near_count(@P);"));
        let err = cook("wrangle", &p, &[Some(g.clone()), Some(g.clone())]).unwrap_err();
        assert!(err.contains("takes 2 argument"), "{err}");
        p.insert("code".into(), json!("@d = near_dist(@P);"));
        p.insert("ref_class".into(), json!("instance"));
        let err = cook("wrangle", &p, &[Some(g.clone()), Some(g)]).unwrap_err();
        assert!(err.contains("no instances"), "{err}");
        // the attribute-flow check reads the second input too, before anything cooks
        use flow::effect;
        let grid = effect("grid", &Params::new(), &[]).unwrap();
        let mut w = Params::new();
        w.insert("class".into(), json!("point"));
        w.insert("code".into(), json!("@h = surf_f(\"P\", @P).x;"));
        let err = effect("wrangle", &w, &[Some(grid.clone()), Some(grid.clone())]).err().unwrap();
        assert!(err[0].message.contains("is a vec3"), "{}", err[0].message);
        w.insert("code".into(), json!("@h = near_dist(@P);"));
        let ok = effect("wrangle", &w, &[Some(grid.clone()), Some(grid.clone())]).unwrap();
        assert!(ok.point.contains_key("h"));
        assert!(effect("wrangle", &w, &[Some(grid)]).err().unwrap()[0].message.contains("second input"));
    }

    #[test]
    fn array_and_radial_array_are_instances() {
        let b = cook1("box", &[], &[]);
        let a = cook1("array", &[("count", json!(4)), ("offset", json!([2.0, 0.0, 0.0]))], &[b.clone()]);
        assert_eq!(a.n_instances(), 4);
        assert_eq!(a.realize().n_points(), 32);
        let r = cook1("radial_array", &[("count", json!(6)), ("radius", json!(3.0))], &[b]);
        let (lo, hi) = r.bounds().unwrap();
        assert!(hi[0] - lo[0] > 6.0);
    }

    #[test]
    fn attribute_transfer_picks_nearest() {
        let a = cook1("grid", &[("size", json!([2.0, 2.0])), ("rows", json!(5)), ("cols", json!(5))], &[]);
        let src = cook1("grid", &[("size", json!([2.0, 2.0])), ("rows", json!(2)), ("cols", json!(2))], &[]);
        let src = cook1("wrangle", &[("code", json!("@w = @P.x;"))], &[src]);
        let t = cook1("attribute_transfer", &[], &[a, src]);
        let w = t.point.get("w").unwrap().as_f32().unwrap().to_vec();
        for (q, w) in t.p().iter().zip(w) {
            assert!((w - q.x_snap()).abs() < 1e-5, "nearest source x for {q:?} was {w}");
        }
    }

    trait Snap {
        fn x_snap(&self) -> f32;
    }
    impl Snap for [f32; 3] {
        fn x_snap(&self) -> f32 {
            (self[0] + 1.0).round() - 1.0 // source grid has x in {-1, 0, 1}
        }
    }

    #[test]
    fn flow_predicts_attributes_and_errors() {
        use flow::{effect, GeoEnv};
        let grid = effect("grid", &Params::new(), &[]).unwrap();
        let sc = effect("scatter", &Params::new(), &[Some(grid.clone())]).unwrap();
        assert!(sc.point.contains_key("N") && sc.point.contains_key("pscale"));
        let mut p = Params::new();
        p.insert("orient_to_normal".into(), json!(1));
        // grid has no N -> must be flagged before cooking.
        let e = effect("copy_to_points", &p, &[Some(grid.clone()), Some(grid.clone())]);
        assert!(e.is_err());
        assert!(effect("copy_to_points", &p, &[Some(grid.clone()), Some(sc)]).is_ok());
        let mut w = Params::new();
        w.insert("code".into(), json!("@P.y = @missing.x;"));
        w.insert("class".into(), json!("point"));
        let err = effect("wrangle", &w, &[Some(grid)]).err().unwrap();
        assert!(err[0].message.contains("@missing"));
        let _ = GeoEnv::default();
    }
}

/// Public surface sampler (positions only), used by objectives like Chamfer distance.
pub fn sample_surface(src: &Geometry, n: usize, seed: i64) -> Result<Vec<[f32; 3]>, String> {
    Ok(surface_samples(src, n, seed)?.into_iter().map(|s| s.0).collect())
}

/// Area-weighted surface samples as (position, unit face normal), deterministic for a given seed.
pub fn sample_surface_normals(src: &Geometry, n: usize, seed: i64) -> Result<Vec<([f32; 3], [f32; 3])>, String> {
    Ok(surface_samples(src, n, seed)?.into_iter().map(|s| (s.0, s.1)).collect())
}

/// Tests that point `PT_CORPUS` at a temporary folder hold this lock so they cannot race.
#[cfg(test)]
pub(crate) static CORPUS_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
