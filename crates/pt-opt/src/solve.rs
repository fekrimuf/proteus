//! Solver stack v0, step (a): a primitive baseline that needs no language model.
//!
//! A reference is rebuilt as a union of boxes, cylinders/cones and ellipsoids:
//!
//! 1. split the surface into pieces that touch each other (parts);
//! 2. per piece, fit the best single primitive; if it is a poor fit, search axis-aligned cuts and recurse
//!    (the published part-by-part idea, MeshCoder/ShapeCoder);
//! 3. drop the least useful primitives down to the primitive budget and the parameter cap;
//! 4. fit the triangle budget (B3: at most 2x the source) by lowering the tessellation of round primitives;
//! 5. refine every continuous parameter together with CMA-ES against F-score + Chamfer;
//! 6. emit a real recipe (`box`/`tube`/`sphere` + `transform` + `merge` nodes) and cook it.
//!
//! The score reported to the owner is always computed on the *cooked recipe* by `matching::Reference`, never on
//! the solver's internal approximation of it.

use crate::cma::Cma;
use crate::matching::*;
use pt_core::math::{self, Mat34};
use pt_core::{Geometry, Params};
use pt_graph::Graph;
use rayon::prelude::*;
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Box,
    Tube,
    Sphere,
}

/// One primitive in world coordinates. `dims` means: box edge lengths; tube [radius, height, top radius];
/// sphere diameters per axis. `rot` is in degrees (the same Euler convention as the `transform` node).
#[derive(Clone, Debug)]
pub struct Prim {
    pub kind: Kind,
    /// Radial divisions for tubes (spheres use tess/2 rows). Chosen to fit the source and the triangle budget.
    pub tess: u32,
    pub dims: [f32; 3],
    pub pos: [f32; 3],
    pub rot: [f32; 3],
}

const DEFAULT_TESS: u32 = 16;
const TESS_LEVELS: [u32; 6] = [6, 8, 10, 12, 16, 24];

fn base_params(kind: Kind, dims: [f32; 3], tess: u32) -> (&'static str, Params) {
    // The node cooker takes raw parameters; the graph normally fills in schema defaults, so do the same here.
    let mut p = Params::new();
    let name = match kind {
        Kind::Box => "box",
        Kind::Tube => "tube",
        Kind::Sphere => "sphere",
    };
    for s in pt_sop::schema(name).map(|s| s.params).unwrap_or_default() {
        p.insert(s.name.into(), s.default.clone());
    }
    match kind {
        Kind::Box => {
            p.insert("size".into(), json!(dims));
        }
        Kind::Tube => {
            p.insert("radius".into(), json!(dims[0]));
            p.insert("height".into(), json!(dims[1]));
            p.insert("radius_top".into(), json!(dims[2]));
            p.insert("cols".into(), json!(tess));
        }
        Kind::Sphere => {
            p.insert("radius".into(), json!(0.5));
            p.insert("cols".into(), json!(tess));
            p.insert("rows".into(), json!((tess / 2).max(2)));
        }
    }
    (name, p)
}

impl Prim {
    pub fn matrix(&self) -> Mat34 {
        let s = if self.kind == Kind::Sphere { self.dims } else { [1.0; 3] };
        math::trs(self.pos, self.rot, s)
    }

    pub fn tris(&self) -> Vec<Tri> {
        let (kind, params) = base_params(self.kind, self.dims, self.tess);
        let g = pt_sop::cook(kind, &params, &[]).unwrap_or_default();
        transform_tris(&tris_of(&g), &self.matrix())
    }

    /// Boxes and upright tubes have a free yaw about Y (the angular phase of a tube matters when the source is a
    /// low-sided prism).
    fn has_yaw(&self) -> bool {
        match self.kind {
            Kind::Box => true,
            Kind::Tube => self.rot[0] == 0.0 && self.rot[2] == 0.0,
            Kind::Sphere => false,
        }
    }

    fn n_free(&self) -> usize {
        6 + usize::from(self.has_yaw())
    }

    fn to_vec(&self) -> Vec<f64> {
        let d = self.dims.map(f64::from);
        let p = self.pos.map(f64::from);
        let mut v = vec![d[0], d[1], d[2], p[0], p[1], p[2]];
        if self.has_yaw() {
            v.push(f64::from(self.rot[1]));
        }
        v
    }

    fn set_vec(&mut self, v: &[f64]) {
        self.dims = [v[0] as f32, v[1] as f32, v[2] as f32];
        self.pos = [v[3] as f32, v[4] as f32, v[5] as f32];
        if self.has_yaw() {
            self.rot[1] = v[6] as f32;
        }
    }

    /// Search interval for each free scalar (relative to the model diagonal `diag`).
    fn bounds(&self, diag: f32) -> Vec<(f64, f64)> {
        let v = self.to_vec();
        let d = f64::from(diag);
        let floor = 0.004 * d;
        v.iter()
            .enumerate()
            .map(|(i, &x)| {
                if i < 3 {
                    ((x - (0.3 * x.abs()).max(0.02 * d)).max(floor), x + (0.4 * x.abs()).max(0.02 * d))
                } else if i < 6 {
                    (x - 0.05 * d, x + 0.05 * d)
                } else if self.kind == Kind::Tube {
                    let half = 180.0 / f64::from(self.tess.max(3)) + 2.0;
                    (x - half, x + half)
                } else {
                    (x - 12.0, x + 12.0)
                }
            })
            .collect()
    }
}

fn rot_matrix(rot: [f32; 3]) -> Mat34 {
    math::trs([0.0; 3], rot, [1.0; 3])
}

/// `R * p` for a rotation-only matrix.
fn rot_apply(m: &Mat34, p: [f32; 3]) -> [f32; 3] {
    [m[0] * p[0] + m[1] * p[1] + m[2] * p[2], m[4] * p[0] + m[5] * p[1] + m[6] * p[2], m[8] * p[0] + m[9] * p[1] + m[10] * p[2]]
}

/// `R^T * p` for a rotation-only matrix.
fn rot_apply_t(m: &Mat34, p: [f32; 3]) -> [f32; 3] {
    [m[0] * p[0] + m[4] * p[1] + m[8] * p[2], m[1] * p[0] + m[5] * p[1] + m[9] * p[2], m[2] * p[0] + m[6] * p[1] + m[10] * p[2]]
}

/// Axis-aligned bounds of `pts` expressed in the frame rotated by `rot`: (lo, hi) in that frame.
fn local_bounds(pts: &[[f32; 3]], rot: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let m = rot_matrix(rot);
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for &p in pts {
        let q = rot_apply_t(&m, p);
        for a in 0..3 {
            lo[a] = lo[a].min(q[a]);
            hi[a] = hi[a].max(q[a]);
        }
    }
    (lo, hi)
}

/// Candidate primitives for a point set: a yaw-aligned box, three cylinder/cone axes, an ellipsoid.
fn fit_candidates(pts: &[[f32; 3]], diag: f32) -> Vec<Prim> {
    if pts.is_empty() {
        return vec![];
    }
    let thin = 0.005 * diag;
    let mut out = Vec::new();

    // Box with the yaw that minimises volume (0..90 in 5 degree steps; plain AABB wins ties).
    let mut best = (f32::INFINITY, 0.0f32);
    for k in 0..18 {
        let yaw = 5.0 * k as f32;
        let (lo, hi) = local_bounds(pts, [0.0, yaw, 0.0]);
        let vol = (hi[0] - lo[0]).max(thin) * (hi[1] - lo[1]).max(thin) * (hi[2] - lo[2]).max(thin);
        if vol < best.0 * 0.995 {
            best = (vol, yaw);
        }
    }
    let rot = [0.0, best.1, 0.0];
    let (lo, hi) = local_bounds(pts, rot);
    let centre_local = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    out.push(Prim { kind: Kind::Box, tess: DEFAULT_TESS, dims: [(hi[0] - lo[0]).max(thin), (hi[1] - lo[1]).max(thin), (hi[2] - lo[2]).max(thin)], pos: rot_apply(&rot_matrix(rot), centre_local), rot });

    // Ellipsoid on the plain bounding box.
    let (alo, ahi) = local_bounds(pts, [0.0; 3]);
    out.push(Prim {
        kind: Kind::Sphere,
        tess: DEFAULT_TESS,
        dims: [(ahi[0] - alo[0]).max(thin), (ahi[1] - alo[1]).max(thin), (ahi[2] - alo[2]).max(thin)],
        pos: [(alo[0] + ahi[0]) * 0.5, (alo[1] + ahi[1]) * 0.5, (alo[2] + ahi[2]) * 0.5],
        rot: [0.0; 3],
    });

    // Tubes along Y, X (rotate 90 about Z) and Z (rotate 90 about X).
    for rot in [[0.0, 0.0, 0.0], [0.0, 0.0, 90.0], [90.0, 0.0, 0.0]] {
        let m = rot_matrix(rot);
        let local: Vec<[f32; 3]> = pts.iter().map(|&p| rot_apply_t(&m, p)).collect();
        let (lo, hi) = local_bounds(&local, [0.0; 3]);
        let (cx, cz) = ((lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5);
        let h = (hi[1] - lo[1]).max(thin);
        // Least-squares line r(y) through the radial distances: radius at the bottom and at the top.
        let (mut sy, mut sr, mut syy, mut syr) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for p in &local {
            let r = ((p[0] - cx) * (p[0] - cx) + (p[2] - cz) * (p[2] - cz)).sqrt() as f64;
            let y = (p[1] - lo[1]) as f64;
            sy += y;
            sr += r;
            syy += y * y;
            syr += y * r;
        }
        let n = local.len() as f64;
        let den = n * syy - sy * sy;
        let (a, b) = if den.abs() > 1e-12 { ((sr * syy - sy * syr) / den, (n * syr - sy * sr) / den) } else { (sr / n, 0.0) };
        let (r0, r1) = ((a as f32).max(thin), ((a + b * f64::from(h)) as f32).max(thin));
        let centre_local = [cx, (lo[1] + hi[1]) * 0.5, cz];
        out.push(Prim { kind: Kind::Tube, tess: DEFAULT_TESS, dims: [r0, h, r1], pos: rot_apply(&m, centre_local), rot });
    }
    out
}

fn dedup_points(tris: &[Tri]) -> Vec<[f32; 3]> {
    tris.iter().flat_map(|t| t.iter().copied()).collect()
}

/// A cluster of reference triangles with what is needed to score primitives against it.
struct Cluster {
    tris: Vec<Tri>,
    bvh: TriBvh,
    samples: Vec<([f32; 3], [f32; 3])>,
}

impl Cluster {
    fn new(tris: Vec<Tri>, seed: i64) -> Option<Cluster> {
        let bvh = TriBvh::new(&tris).ok()?;
        let samples = sample_tris(&tris, 1500, seed);
        Some(Cluster { tris, bvh, samples })
    }

    /// F-score of a primitive set against this cluster's surface.
    fn f_of(&self, prims: &[Prim], tau: f32, seed: i64) -> f64 {
        let ct: Vec<Tri> = prims.iter().flat_map(Prim::tris).collect();
        let Ok(cb) = TriBvh::new(&ct) else { return 0.0 };
        let cs = sample_tris(&ct, 1200, seed);
        let a = distances_to(&self.bvh, &cs);
        let b = distances_to(&cb, &self.samples);
        f_from(&a, &b, tau).0
    }
}

struct Fitter {
    tau: f32,
    diag: f32,
    seed: i64,
    out: Vec<Prim>,
    budget_left: usize,
}

impl Fitter {
    fn best_single(&self, cl: &Cluster) -> Option<(Prim, f64)> {
        let pts = dedup_points(&cl.tris);
        fit_candidates(&pts, self.diag)
            .into_iter()
            .map(|p| {
                let f = cl.f_of(std::slice::from_ref(&p), self.tau, self.seed);
                (p, f)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Tune what only matters for round things: radial divisions and angular phase, against the cluster itself.
    /// Low-poly sources are often 6 to 12-sided prisms, and a 16-gon cylinder misses them by more than the
    /// F-score threshold.
    fn finish(&self, p: Prim, cl: &Cluster) -> Prim {
        if p.kind == Kind::Box {
            return p;
        }
        let mut best: Option<(f64, Prim)> = None;
        for tess in TESS_LEVELS {
            let phases: Vec<f32> = if p.has_yaw() { (0..4).map(|k| 360.0 / tess as f32 * k as f32 / 4.0).collect() } else { vec![0.0] };
            for ph in phases {
                let mut c = p.clone();
                c.tess = tess;
                if c.has_yaw() {
                    c.rot[1] = ph;
                }
                let f = cl.f_of(std::slice::from_ref(&c), self.tau, self.seed + 5);
                // Prefer fewer divisions unless a finer one wins by a clear margin.
                if best.as_ref().is_none_or(|b| f > b.0 + 0.004) {
                    best = Some((f, c));
                }
            }
        }
        best.map_or(p, |b| b.1)
    }

    /// Fit a connected-ish set of triangles: one primitive if it fits, otherwise the best cut and recurse.
    fn fit(&mut self, tris: Vec<Tri>, depth: usize) {
        let weld = self.diag * 1e-4;
        let comps = components_of_tris(&tris, weld);
        if comps.len() > 1 {
            // Largest pieces first; ignore specks that cannot matter for the score.
            let total: f32 = tris.iter().map(tri_area).sum();
            let mut parts: Vec<Vec<Tri>> = comps.iter().map(|c| c.iter().map(|&i| tris[i]).collect()).collect();
            parts.sort_by(|a, b| b.iter().map(tri_area).sum::<f32>().partial_cmp(&a.iter().map(tri_area).sum::<f32>()).unwrap_or(std::cmp::Ordering::Equal));
            for p in parts {
                if p.iter().map(tri_area).sum::<f32>() >= total * 0.003 {
                    self.fit(p, depth);
                }
            }
            return;
        }
        let Some(cl) = Cluster::new(tris, self.seed + depth as i64) else { return };
        let Some((single, f1)) = self.best_single(&cl) else { return };
        if f1 >= 0.93 || depth >= 4 || cl.tris.len() < 12 || self.out.len() >= self.budget_left {
            let p = self.finish(single, &cl);
            self.out.push(p);
            return;
        }
        // Try axis-aligned cuts; keep the one whose two best primitives cover the cluster best.
        let (lo, hi) = bounds_of_tris(&cl.tris).unwrap();
        let mut best: Option<(f64, Vec<Tri>, Vec<Tri>)> = None;
        for axis in 0..3 {
            for k in 1..10 {
                let at = lo[axis] + (hi[axis] - lo[axis]) * k as f32 / 10.0;
                let (mut l, mut r) = (Vec::new(), Vec::new());
                for t in &cl.tris {
                    let c = (t[0][axis] + t[1][axis] + t[2][axis]) / 3.0;
                    if c < at {
                        l.push(*t)
                    } else {
                        r.push(*t)
                    }
                }
                if l.len() < 4 || r.len() < 4 {
                    continue;
                }
                let (Some(cl_l), Some(cl_r)) = (Cluster::new(l.clone(), self.seed + 11), Cluster::new(r.clone(), self.seed + 12)) else { continue };
                let (Some(pl), Some(pr)) = (self.best_single(&cl_l), self.best_single(&cl_r)) else { continue };
                let f = cl.f_of(&[pl.0, pr.0], self.tau, self.seed + 13);
                if best.as_ref().is_none_or(|b| f > b.0) {
                    best = Some((f, l, r));
                }
            }
        }
        match best {
            Some((f, l, r)) if f > f1 + 0.02 => {
                self.fit(l, depth + 1);
                self.fit(r, depth + 1);
            }
            _ => {
                let p = self.finish(single, &cl);
                self.out.push(p);
            }
        }
    }
}

/// Shared pieces for scoring a whole primitive set against the reference during refinement.
struct Scorer<'a> {
    bvh: &'a TriBvh,
    samples: &'a [([f32; 3], [f32; 3])],
    tau: f32,
    diag: f32,
    seed: i64,
}

impl Scorer<'_> {
    /// (F-score, Chamfer in diagonal units).
    fn eval(&self, prims: &[Prim]) -> (f64, f64) {
        let ct: Vec<Tri> = prims.iter().flat_map(Prim::tris).collect();
        let Ok(cb) = TriBvh::new(&ct) else { return (0.0, 1.0) };
        let cs = sample_tris(&ct, 2500, self.seed);
        let a = distances_to(self.bvh, &cs);
        let b = distances_to(&cb, self.samples);
        let (f, _, _) = f_from(&a, &b, self.tau);
        (f, (mean_dist(&a) + mean_dist(&b)) * 0.5 / f64::from(self.diag))
    }

    fn loss(&self, prims: &[Prim]) -> f64 {
        let (f, c) = self.eval(prims);
        (1.0 - f) + 10.0 * c
    }

    /// Index of the primitive whose removal costs the least loss, and that cost (negative = removing helps).
    fn least_useful(&self, prims: &[Prim]) -> (usize, f64) {
        let base = self.loss(prims);
        let costs: Vec<f64> = (0..prims.len())
            .into_par_iter()
            .map(|i| {
                let mut rest = prims.to_vec();
                rest.remove(i);
                self.loss(&rest) - base
            })
            .collect();
        costs.iter().enumerate().fold((0, f64::INFINITY), |a, (i, &c)| if c < a.1 { (i, c) } else { a })
    }
}

pub struct SolverOptions {
    pub seed: i64,
    pub max_prims: usize,
    /// Hard cap on free numeric parameters in the emitted recipe (silver allows 40).
    pub max_params: usize,
    /// CMA-ES objective evaluations for the refinement (0 skips it).
    pub evals: usize,
    pub time_budget_s: f64,
}

impl Default for SolverOptions {
    fn default() -> Self {
        SolverOptions { seed: 1, max_prims: 12, max_params: 40, evals: 900, time_budget_s: 600.0 }
    }
}

pub struct Solution {
    pub prims: Vec<Prim>,
    pub graph: Graph,
    pub out: String,
    pub hcs: String,
    /// Non-default numeric parameters in the recipe (structural integers such as `cols` excluded).
    pub n_params: usize,
    pub evals: usize,
    pub secs: f64,
    pub notes: Vec<String>,
}

impl Solution {
    /// The cooked recipe geometry.
    pub fn geometry(&mut self) -> Result<std::sync::Arc<Geometry>, String> {
        let out = self.out.clone();
        self.graph.cook(&out)
    }
}

/// Count numeric components that differ from the node schema default (structural integers are not tunable).
pub fn count_free_params(g: &Graph) -> usize {
    const STRUCTURAL: [&str; 6] = ["rows", "cols", "caps", "divisions", "seed", "octaves"];
    fn comps(v: &Value, d: &Value) -> usize {
        match (v, d) {
            (Value::Number(a), Value::Number(b)) => usize::from((a.as_f64().unwrap_or(0.0) - b.as_f64().unwrap_or(0.0)).abs() > 1e-9),
            (Value::Array(a), Value::Array(b)) => a.iter().zip(b).map(|(x, y)| comps(x, y)).sum(),
            (Value::Number(_), _) | (Value::Array(_), _) => 1,
            _ => 0,
        }
    }
    let mut n = 0;
    for node in g.nodes.values() {
        let Some(schema) = pt_graph::schema(&node.kind) else { continue };
        for (k, v) in &node.params {
            if STRUCTURAL.contains(&k.as_str()) {
                continue;
            }
            if let Some(p) = schema.params.iter().find(|p| p.name == k.as_str()) {
                n += comps(v, &p.default);
            }
        }
    }
    n
}

fn round_to(v: f32, step: f32) -> f64 {
    f64::from((v / step).round() * step)
}

/// Build the recipe graph for a primitive set; returns (graph, output node id).
pub fn build_graph(prims: &[Prim], diag: f32) -> Result<(Graph, String), String> {
    let step = if diag >= 0.5 { 1e-4 } else { 1e-5 };
    let mut g = Graph::new();
    let mut leaves = Vec::new();
    for (i, p) in prims.iter().enumerate() {
        let (kind, _) = base_params(p.kind, p.dims, p.tess);
        let base = g.create(kind, Some(&format!("part{}_{}", i + 1, kind)))?;
        match p.kind {
            Kind::Box => g.set_param(&base, "size", json!(p.dims.map(|v| round_to(v, step))))?,
            Kind::Tube => {
                g.set_param(&base, "radius", json!(round_to(p.dims[0], step)))?;
                g.set_param(&base, "height", json!(round_to(p.dims[1], step)))?;
                if (p.dims[2] - p.dims[0]).abs() > 0.005 * p.dims[0].max(1e-6) {
                    g.set_param(&base, "radius_top", json!(round_to(p.dims[2], step)))?;
                }
                g.set_param(&base, "cols", json!(p.tess))?;
            }
            Kind::Sphere => {
                g.set_param(&base, "cols", json!(p.tess))?;
                g.set_param(&base, "rows", json!((p.tess / 2).max(2)))?;
            }
        }
        let xf = g.create("transform", Some(&format!("part{}_place", i + 1)))?;
        if p.pos.iter().any(|v| v.abs() > 1e-9) {
            g.set_param(&xf, "t", json!(p.pos.map(|v| round_to(v, step))))?;
        }
        if p.rot.iter().any(|v| v.abs() > 1e-6) {
            g.set_param(&xf, "r", json!(p.rot.map(|v| round_to(v, 1e-3))))?;
        }
        if p.kind == Kind::Sphere {
            g.set_param(&xf, "s", json!(p.dims.map(|v| round_to(v, step))))?;
        }
        g.connect(&base, &xf, 0)?;
        leaves.push(xf);
    }
    if leaves.is_empty() {
        return Err("no primitives to build".into());
    }
    // Chain merges of up to four inputs each.
    let mut acc = leaves[0].clone();
    let mut rest = leaves[1..].iter();
    let mut n = 0;
    loop {
        let chunk: Vec<&String> = rest.by_ref().take(3).collect();
        if chunk.is_empty() {
            break;
        }
        n += 1;
        let m = g.create("merge", Some(&format!("join{n}")))?;
        g.connect(&acc, &m, 0)?;
        for (slot, id) in chunk.iter().enumerate() {
            g.connect(id, &m, slot + 1)?;
        }
        acc = m;
    }
    let out = g.create("null", Some("result"))?;
    g.connect(&acc, &out, 0)?;
    Ok((g, out))
}

/// Free parameters the recipe could end up with after refinement (every position component and yaw non-zero).
fn worst_case_params(prims: &[Prim], diag: f32) -> Result<usize, String> {
    let mut p: Vec<Prim> = prims.to_vec();
    for q in &mut p {
        for v in q.pos.iter_mut() {
            if v.abs() < 1e-6 {
                *v = 1e-3 * diag;
            }
        }
        if q.has_yaw() && q.rot[1].abs() < 1e-6 {
            q.rot[1] = 0.01;
        }
        // A cylinder whose top radius equals its bottom radius omits that number; refinement may separate them.
        if q.kind == Kind::Tube && (q.dims[2] - q.dims[0]).abs() <= 0.005 * q.dims[0].max(1e-6) {
            q.dims[2] = q.dims[0] * 1.1;
        }
    }
    Ok(crate::recipe::count_source_params(&pt_hcs::to_hcs(&build_graph(&p, diag)?.0)))
}

/// Lower the tessellation of the most expensive round primitive until the triangle count fits `budget` (B3: at most 2x
/// the source). Returns true when it fits.
fn fit_budget(prims: &mut [Prim], budget: usize) -> bool {
    loop {
        let total: usize = prims.iter().map(|p| p.tris().len()).sum();
        if total <= budget {
            return true;
        }
        let pick = prims.iter().enumerate().filter(|(_, p)| p.kind != Kind::Box && p.tess > TESS_LEVELS[0]).max_by_key(|(_, p)| p.tris().len()).map(|(i, _)| i);
        let Some(i) = pick else { return false };
        let below = TESS_LEVELS.iter().copied().filter(|&l| l < prims[i].tess).max().unwrap_or(TESS_LEVELS[0]);
        prims[i].tess = below;
    }
}

/// Solve `reference` (original units, Y up).
pub fn solve(reference: &Geometry, opts: &SolverOptions) -> Result<Solution, String> {
    let t0 = Instant::now();
    let mut notes = Vec::new();
    let tris = tris_of(reference);
    let (lo, hi) = bounds_of_tris(&tris).ok_or("reference has no triangles")?;
    let diag = diagonal(lo, hi);
    if diag.is_nan() || diag <= 1e-9 {
        return Err("reference has zero size".into());
    }
    let tau = F_THRESHOLD * diag;
    let ref_bvh = TriBvh::new(&tris)?;
    let ref_samples = sample_tris(&tris, 5000, 3);

    // 1-2. structure
    let mut fitter = Fitter { tau, diag, seed: opts.seed, out: vec![], budget_left: opts.max_prims * 2 };
    fitter.fit(tris.clone(), 0);
    let mut prims = std::mem::take(&mut fitter.out);
    if prims.is_empty() {
        return Err("could not fit any primitive".into());
    }
    let scorer = Scorer { bvh: &ref_bvh, samples: &ref_samples, tau, diag, seed: 29 };

    // 3a. primitive budget, and anything whose removal does not hurt.
    while prims.len() > 1 {
        let (i, c) = scorer.least_useful(&prims);
        if prims.len() > opts.max_prims || c < 0.002 {
            notes.push(format!("dropped primitive {} (loss {c:+.4})", i + 1));
            prims.remove(i);
        } else {
            break;
        }
    }
    // 3b. hard parameter cap (silver allows 3 to 40 free parameters): drop the least useful primitive until it holds.
    // Counted in the worst case: refinement may turn a zero position or yaw into a literal.
    loop {
        if worst_case_params(&prims, diag)? <= opts.max_params || prims.len() <= 1 {
            break;
        }
        let (i, c) = scorer.least_useful(&prims);
        notes.push(format!("parameter cap {}: dropped primitive {} (loss {c:+.4})", opts.max_params, i + 1));
        prims.remove(i);
    }

    // 4. triangle budget (B3: at most 2x the source).
    let fits = fit_budget(&mut prims, 2 * tris.len());
    notes.push(format!(
        "{} triangles for a {}-triangle source{}",
        prims.iter().map(|p| p.tris().len()).sum::<usize>(),
        tris.len(),
        if fits { "" } else { " (over budget even at the coarsest tessellation)" }
    ));

    // 5. refine continuous parameters together.
    let mut evals = 0usize;
    if opts.evals > 0 {
        let bounds: Vec<(f64, f64)> = prims.iter().flat_map(|p| p.bounds(diag)).collect();
        let sizes: Vec<usize> = prims.iter().map(Prim::n_free).collect();
        let apply = |u: &[f64]| -> Vec<Prim> {
            let mut out = prims.clone();
            let mut at = 0;
            for (p, &n) in out.iter_mut().zip(&sizes) {
                let v: Vec<f64> = (0..n).map(|k| bounds[at + k].0 + u[at + k].clamp(0.0, 1.0) * (bounds[at + k].1 - bounds[at + k].0)).collect();
                p.set_vec(&v);
                at += n;
            }
            out
        };
        let start: Vec<f64> = prims.iter().flat_map(Prim::to_vec).zip(&bounds).map(|(x, b)| ((x - b.0) / (b.1 - b.0)).clamp(0.0, 1.0)).collect();
        let mut best = (start.clone(), scorer.loss(&prims));
        evals += 1;
        let mut cma = Cma::new(&start, 0.12, opts.seed);
        let mut stagnant = 0;
        while evals < opts.evals && t0.elapsed().as_secs_f64() < opts.time_budget_s {
            let xs = cma.ask();
            let fs: Vec<f64> = xs
                .par_iter()
                .map(|x| {
                    let c: Vec<f64> = x.iter().map(|v| v.clamp(0.0, 1.0)).collect();
                    let pen: f64 = x.iter().zip(&c).map(|(a, b)| (a - b) * (a - b)).sum();
                    scorer.loss(&apply(&c)) + 1e-3 * pen
                })
                .collect();
            evals += xs.len();
            let mut improved = false;
            for (x, &f) in xs.iter().zip(&fs) {
                if f < best.1 - 1e-6 {
                    best = (x.iter().map(|v| v.clamp(0.0, 1.0)).collect(), f);
                    improved = true;
                }
            }
            stagnant = if improved { 0 } else { stagnant + 1 };
            cma.tell(&xs, &fs);
            if cma.sigma < 1e-3 || stagnant > 25 {
                break;
            }
        }
        let before = scorer.loss(&prims);
        let refined = apply(&best.0);
        let after = scorer.loss(&refined);
        if after < before {
            notes.push(format!("CMA-ES: loss {before:.4} -> {after:.4} in {evals} evaluations"));
            prims = refined;
        }
    }

    // 6. emit and cook.
    let (mut graph, out) = build_graph(&prims, diag)?;
    graph.cook(&out)?;
    let hcs = pt_hcs::to_hcs(&graph);
    let n_params = crate::recipe::count_source_params(&hcs);
    Ok(Solution { prims, graph, out, hcs, n_params, evals, secs: t0.elapsed().as_secs_f64(), notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table: a 1.6 x 0.1 x 0.9 top on four 0.1 square legs (separate closed boxes).
    fn table() -> Geometry {
        let mut g = Geometry::new();
        let mut add = |size: [f32; 3], at: [f32; 3]| {
            let p = Prim { kind: Kind::Box, tess: 16, dims: size, pos: at, rot: [0.0; 3] };
            g.append(&geometry_from_tris(&p.tris())).unwrap();
        };
        add([1.6, 0.1, 0.9], [0.0, 0.75, 0.0]);
        for (x, z) in [(-0.7, -0.35), (0.7, -0.35), (-0.7, 0.35), (0.7, 0.35)] {
            add([0.1, 0.7, 0.1], [x, 0.35, z]);
        }
        g
    }

    #[test]
    fn rebuilds_a_table_from_boxes() {
        let reference = table();
        let mut sol = solve(&reference, &SolverOptions { evals: 300, ..Default::default() }).unwrap();
        let r = Reference::new(&reference).unwrap();
        let cooked = sol.geometry().unwrap();
        let m = r.score(&cooked, Align::Bbox).unwrap();
        assert!(m.f_score > 0.93, "F-score {} with {} primitives", m.f_score, sol.prims.len());
        assert!(m.silhouette_iou > 0.9, "IoU {}", m.silhouette_iou);
        assert!(sol.prims.len() >= 4 && sol.prims.len() <= 8, "{} primitives", sol.prims.len());
        assert!(sol.hcs.contains("box(") && sol.hcs.contains("transform("), "{}", sol.hcs);
        assert!(sol.n_params >= 3 && sol.n_params <= 40, "{} parameters", sol.n_params);
        assert!(validity(&cooked, &reference).ok, "a rebuild of a 60-triangle table must stay within the triangle budget");
    }

    #[test]
    fn solving_is_deterministic() {
        let reference = table();
        let o = SolverOptions { evals: 150, ..Default::default() };
        let a = solve(&reference, &o).unwrap();
        let b = solve(&reference, &o).unwrap();
        assert_eq!(a.hcs, b.hcs);
    }

    #[test]
    fn a_cylinder_is_found_as_a_tube_with_the_right_number_of_sides() {
        // An 8-sided prism, the kind of thing low-poly packs are full of.
        let cyl = Prim { kind: Kind::Tube, tess: 8, dims: [0.4, 1.0, 0.4], pos: [1.0, 0.5, 2.0], rot: [0.0, 22.5, 0.0] };
        let g = geometry_from_tris(&cyl.tris());
        let mut sol = solve(&g, &SolverOptions { evals: 200, ..Default::default() }).unwrap();
        assert_eq!(sol.prims.len(), 1);
        assert_eq!(sol.prims[0].kind, Kind::Tube, "{:?}", sol.prims[0]);
        assert_eq!(sol.prims[0].tess, 8, "{:?}", sol.prims[0]);
        let m = Reference::new(&g).unwrap().score(&sol.geometry().unwrap(), Align::Reference).unwrap();
        assert!(m.f_score > 0.97, "F {}", m.f_score);
    }

    #[test]
    fn the_parameter_cap_is_enforced() {
        let reference = table();
        let sol = solve(&reference, &SolverOptions { evals: 0, max_params: 20, ..Default::default() }).unwrap();
        assert!(sol.n_params <= 20, "{} parameters with a cap of 20", sol.n_params);
    }
}
