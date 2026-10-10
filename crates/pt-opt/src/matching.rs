//! Reference matching: how close is a rebuilt shape to a professional reference? (PRO_QUALITY_PLAN section B3)
//!
//! Every shape is normalised first (centre on the bounding-box centre, scale to a unit bounding-box diagonal;
//! Y is up everywhere in this project). Then:
//!
//! * **F-score**: 20 000 surface samples each side, threshold 0.02 of the diagonal; precision = share of our samples
//!   within the threshold of the reference surface, recall = the same the other way round. Distances are *exact*
//!   point-to-triangle distances (BVH), so the only sampling noise is in which points we ask about.
//! * **Chamfer**: mean of both directional mean distances, in diagonal units.
//! * **Silhouette IoU**: mean over 6 axis views and 4 diagonal views at 256 px, same camera for both shapes.
//! * **Normal consistency**: mean dot of the face normal at a sample with the normal of the nearest surface triangle.
//!
//! Identical inputs score exactly 1.0 on F-score and silhouette IoU, 0.0 on Chamfer.

use pt_core::math::{cross, normalize, sub};
use pt_core::math::Mat34;
use pt_core::Geometry;
use pt_render::{Mode, Opts};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const F_THRESHOLD: f32 = 0.02;
pub const N_SAMPLES: usize = 20_000;
pub const SIL_RES: usize = 256;
/// 6 axis views then 4 diagonal views, as (azimuth, elevation) in degrees.
pub const MATCH_VIEWS: [(f32, f32); 10] = [(0.0, 0.0), (90.0, 0.0), (180.0, 0.0), (270.0, 0.0), (0.0, 89.0), (0.0, -89.0), (45.0, 35.0), (135.0, 35.0), (225.0, 35.0), (315.0, 35.0)];

pub type Tri = [[f32; 3]; 3];

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn tri_normal(t: &Tri) -> [f32; 3] {
    normalize(cross(sub(t[1], t[0]), sub(t[2], t[0])))
}

pub fn tri_area(t: &Tri) -> f32 {
    let c = cross(sub(t[1], t[0]), sub(t[2], t[0]));
    0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

/// Triangles of a geometry after realizing instances.
pub fn tris_of(g: &Geometry) -> Vec<Tri> {
    let g = g.realize();
    let p = g.p();
    g.triangles().into_iter().map(|t| [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]).collect()
}

pub fn geometry_from_tris(tris: &[Tri]) -> Geometry {
    let mut pts = Vec::with_capacity(tris.len() * 3);
    let mut polys = Vec::with_capacity(tris.len());
    for t in tris {
        let b = pts.len() as u32;
        pts.extend_from_slice(t);
        polys.push(vec![b, b + 1, b + 2]);
    }
    Geometry::from_polys(pts, &polys)
}

pub fn bounds_of_tris(tris: &[Tri]) -> Option<([f32; 3], [f32; 3])> {
    let first = *tris.first()?.first()?;
    let (mut lo, mut hi) = (first, first);
    for t in tris {
        for p in t {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    Some((lo, hi))
}

pub fn diagonal(lo: [f32; 3], hi: [f32; 3]) -> f32 {
    let d = sub(hi, lo);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

// ---------------------------------------------------------------------------------------------
// Normalisation
// ---------------------------------------------------------------------------------------------

/// Centre + uniform scale that maps a shape's bounding box to a unit diagonal centred on the origin.
#[derive(Clone, Copy, Debug)]
pub struct Norm {
    pub centre: [f32; 3],
    pub scale: f32,
}

impl Norm {
    pub fn of_tris(tris: &[Tri]) -> Result<Norm, String> {
        let (lo, hi) = bounds_of_tris(tris).ok_or("shape has no triangles")?;
        let d = diagonal(lo, hi);
        if !(d > 1e-9 && d.is_finite()) {
            return Err("shape has zero size".into());
        }
        Ok(Norm { centre: [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5], scale: 1.0 / d })
    }

    pub fn of(g: &Geometry) -> Result<Norm, String> {
        Norm::of_tris(&tris_of(g))
    }

    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        [(p[0] - self.centre[0]) * self.scale, (p[1] - self.centre[1]) * self.scale, (p[2] - self.centre[2]) * self.scale]
    }

    pub fn tris(&self, tris: &[Tri]) -> Vec<Tri> {
        tris.iter().map(|t| [self.point(t[0]), self.point(t[1]), self.point(t[2])]).collect()
    }

    pub fn apply(&self, g: &Geometry) -> Geometry {
        let mut out = g.realize();
        for q in out.p_mut() {
            *q = self.point(*q);
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Closest-point BVH over triangles
// ---------------------------------------------------------------------------------------------

struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    /// Interior: indices of the children. Leaf (`count > 0`): `left` is the first triangle.
    left: u32,
    right: u32,
    count: u32,
}

pub struct TriBvh {
    tris: Vec<Tri>,
    nrm: Vec<[f32; 3]>,
    nodes: Vec<Node>,
}

/// Closest point on triangle `abc` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_tri(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v];
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [a[0] + ac[0] * w, a[1] + ac[1] * w, a[2] + ac[2] * w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [b[0] + (c[0] - b[0]) * w, b[1] + (c[1] - b[1]) * w, b[2] + (c[2] - b[2]) * w];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [a[0] + ab[0] * v + ac[0] * w, a[1] + ab[1] * v + ac[1] * w, a[2] + ab[2] * v + ac[2] * w]
}

fn box_d2(lo: &[f32; 3], hi: &[f32; 3], q: [f32; 3]) -> f32 {
    let mut d = 0.0;
    for a in 0..3 {
        let e = (lo[a] - q[a]).max(q[a] - hi[a]).max(0.0);
        d += e * e;
    }
    d
}

impl TriBvh {
    /// Builds the tree; zero-area triangles are dropped (they have no normal and no area).
    pub fn new(tris: &[Tri]) -> Result<TriBvh, String> {
        let mut keep: Vec<Tri> = tris.iter().copied().filter(|t| tri_area(t) > 1e-14).collect();
        if keep.is_empty() {
            return Err("shape has no non-degenerate triangles".into());
        }
        let cent: Vec<[f32; 3]> = keep.iter().map(|t| [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0, (t[0][2] + t[1][2] + t[2][2]) / 3.0]).collect();
        let mut idx: Vec<u32> = (0..keep.len() as u32).collect();
        let mut nodes = Vec::with_capacity(keep.len() / 2 + 2);
        Self::build(&keep, &cent, &mut idx, 0, &mut nodes);
        keep = idx.iter().map(|&i| keep[i as usize]).collect();
        let nrm = keep.iter().map(tri_normal).collect();
        Ok(TriBvh { tris: keep, nrm, nodes })
    }

    fn build(tris: &[Tri], cent: &[[f32; 3]], idx: &mut [u32], base: usize, nodes: &mut Vec<Node>) -> u32 {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let (mut clo, mut chi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &i in idx.iter() {
            for p in &tris[i as usize] {
                for a in 0..3 {
                    lo[a] = lo[a].min(p[a]);
                    hi[a] = hi[a].max(p[a]);
                }
            }
            let c = cent[i as usize];
            for a in 0..3 {
                clo[a] = clo[a].min(c[a]);
                chi[a] = chi[a].max(c[a]);
            }
        }
        let me = nodes.len() as u32;
        if idx.len() <= 4 {
            nodes.push(Node { lo, hi, left: base as u32, right: 0, count: idx.len() as u32 });
            return me;
        }
        nodes.push(Node { lo, hi, left: 0, right: 0, count: 0 });
        let ext = [chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2]];
        let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] { 0 } else if ext[1] >= ext[2] { 1 } else { 2 };
        let mid = idx.len() / 2;
        idx.select_nth_unstable_by(mid, |&a, &b| cent[a as usize][axis].partial_cmp(&cent[b as usize][axis]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        let (l, r) = idx.split_at_mut(mid);
        let left = Self::build(tris, cent, l, base, nodes);
        let right = Self::build(tris, cent, r, base + mid, nodes);
        nodes[me as usize].left = left;
        nodes[me as usize].right = right;
        me
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn normal(&self, tri: u32) -> [f32; 3] {
        self.nrm[tri as usize]
    }

    /// Squared distance to the surface and the index of the closest triangle.
    pub fn closest(&self, q: [f32; 3]) -> (f32, u32) {
        let mut best = (f32::INFINITY, 0u32);
        let mut stack = [0u32; 96];
        let mut sp = 1;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if box_d2(&n.lo, &n.hi, q) >= best.0 {
                continue;
            }
            if n.count > 0 {
                for i in n.left..n.left + n.count {
                    let t = &self.tris[i as usize];
                    let c = closest_on_tri(q, t[0], t[1], t[2]);
                    let d = sub(c, q);
                    let d2 = dot(d, d);
                    if d2 < best.0 {
                        best = (d2, i);
                    }
                }
            } else {
                let (l, r) = (&self.nodes[n.left as usize], &self.nodes[n.right as usize]);
                let (dl, dr) = (box_d2(&l.lo, &l.hi, q), box_d2(&r.lo, &r.hi, q));
                // Push the farther child first so the nearer one is visited next.
                let (far, near, dfar, dnear) = if dl <= dr { (n.right, n.left, dr, dl) } else { (n.left, n.right, dl, dr) };
                if sp + 2 < stack.len() {
                    if dfar < best.0 {
                        stack[sp] = far;
                        sp += 1;
                    }
                    if dnear < best.0 {
                        stack[sp] = near;
                        sp += 1;
                    }
                }
            }
        }
        best
    }
}

/// Area-weighted surface samples (position, unit normal) of a triangle list, deterministic for a seed.
pub fn sample_tris(tris: &[Tri], n: usize, seed: i64) -> Vec<([f32; 3], [f32; 3])> {
    let mut cum = Vec::with_capacity(tris.len());
    let mut total = 0.0f32;
    for t in tris {
        total += tri_area(t);
        cum.push(total);
    }
    if total <= 0.0 || n == 0 {
        return vec![];
    }
    (0..n)
        .into_par_iter()
        .with_min_len(2048)
        .map(|i| {
            let r = pt_core::rng::rand4(seed, 5, i as u64);
            let ti = cum.partition_point(|&c| c < r[0] * total).min(tris.len() - 1);
            let t = &tris[ti];
            let (mut u, mut v) = (r[1], r[2]);
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            let pos = [0, 1, 2].map(|k| t[0][k] + (t[1][k] - t[0][k]) * u + (t[2][k] - t[0][k]) * v);
            (pos, tri_normal(t))
        })
        .collect()
}

/// Directional distances from `samples` to the surface held by `bvh`: (distance, normal dot).
pub fn distances_to(bvh: &TriBvh, samples: &[([f32; 3], [f32; 3])]) -> Vec<(f32, f32)> {
    samples
        .par_iter()
        .with_min_len(512)
        .map(|(p, n)| {
            let (d2, ti) = bvh.closest(*p);
            (d2.sqrt(), dot(*n, bvh.normal(ti)))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub f_score: f64,
    pub precision: f64,
    pub recall: f64,
    pub chamfer: f64,
    pub silhouette_iou: f64,
    pub normal_consistency: f64,
    /// Candidate bounding-box diagonal over the reference's, in the original units (1.0 = same size).
    pub size_ratio: f64,
}

impl Metrics {
    pub fn to_json(&self) -> Value {
        json!({"f_score": self.f_score, "precision": self.precision, "recall": self.recall, "chamfer": self.chamfer,
               "silhouette_iou": self.silhouette_iou, "normal_consistency": self.normal_consistency, "size_ratio": self.size_ratio})
    }
}

/// F-score pieces from the two directional distance lists.
pub fn f_from(a_to_ref: &[(f32, f32)], ref_to_a: &[(f32, f32)], tau: f32) -> (f64, f64, f64) {
    let frac = |v: &[(f32, f32)]| if v.is_empty() { 0.0 } else { v.iter().filter(|d| d.0 <= tau).count() as f64 / v.len() as f64 };
    let (p, r) = (frac(a_to_ref), frac(ref_to_a));
    (if p + r > 0.0 { 2.0 * p * r / (p + r) } else { 0.0 }, p, r)
}

pub fn mean_dist(v: &[(f32, f32)]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().map(|d| d.0 as f64).sum::<f64>() / v.len() as f64
    }
}

pub fn mean_dot(v: &[(f32, f32)]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().map(|d| (d.1 as f64).clamp(-1.0, 1.0)).sum::<f64>() / v.len() as f64
    }
}

/// How a candidate is brought into the reference's frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Normalise the candidate by its own bounding box (the plan's definition; hides pure scale errors).
    Bbox,
    /// Use the reference's centre and scale (strict: a candidate of the wrong size is penalised).
    Reference,
}

/// A reference shape prepared once: normalised triangles, BVH, samples and silhouette masks.
pub struct Reference {
    pub norm: Norm,
    pub tris: Vec<Tri>,
    pub bvh: TriBvh,
    pub samples: Vec<([f32; 3], [f32; 3])>,
    pub masks: Vec<Vec<bool>>,
    pub raw_diag: f32,
    geo: Geometry,
    sil_bounds: ([f32; 3], [f32; 3]),
}

fn sil_opts(az: f32, el: f32, bounds: ([f32; 3], [f32; 3])) -> Opts {
    Opts { width: SIL_RES, height: SIL_RES, azimuth_deg: az, elevation_deg: el, fit_bounds: Some(bounds), mode: Mode::Shaded, ..Default::default() }
}

fn iou(a: &[bool], b: &[bool]) -> f64 {
    let (mut i, mut u) = (0usize, 0usize);
    for (x, y) in a.iter().zip(b) {
        i += (*x && *y) as usize;
        u += (*x || *y) as usize;
    }
    if u == 0 { 1.0 } else { i as f64 / u as f64 }
}

impl Reference {
    pub fn new(g: &Geometry) -> Result<Reference, String> {
        let raw = tris_of(g);
        let norm = Norm::of_tris(&raw)?;
        let tris = norm.tris(&raw);
        let (lo, hi) = bounds_of_tris(&tris).ok_or("empty reference")?;
        let pad = sub(hi, lo);
        let sil_bounds = ([0, 1, 2].map(|a| lo[a] - pad[a] * 0.25), [0, 1, 2].map(|a| hi[a] + pad[a] * 0.25));
        let bvh = TriBvh::new(&tris)?;
        let samples = sample_tris(&tris, N_SAMPLES, 17);
        let geo = geometry_from_tris(&tris);
        let masks = MATCH_VIEWS.iter().map(|v| pt_render::silhouette(&geo, &sil_opts(v.0, v.1, sil_bounds))).collect();
        let (rlo, rhi) = bounds_of_tris(&raw).ok_or("empty reference")?;
        Ok(Reference { norm, tris, bvh, samples, masks, raw_diag: diagonal(rlo, rhi), geo, sil_bounds })
    }

    pub fn normalised_geometry(&self) -> &Geometry {
        &self.geo
    }

    /// Full B3 geometry metrics of `cand` against this reference.
    pub fn score(&self, cand: &Geometry, align: Align) -> Result<Metrics, String> {
        let raw = tris_of(cand);
        let (clo, chi) = bounds_of_tris(&raw).ok_or("candidate has no triangles")?;
        let cnorm = match align {
            Align::Bbox => Norm::of_tris(&raw)?,
            Align::Reference => self.norm,
        };
        let tris = cnorm.tris(&raw);
        let cb = TriBvh::new(&tris)?;
        let cs = sample_tris(&tris, N_SAMPLES, 23);
        let a = distances_to(&self.bvh, &cs);
        let b = distances_to(&cb, &self.samples);
        let (f, p, r) = f_from(&a, &b, F_THRESHOLD);
        let cgeo = geometry_from_tris(&tris);
        let sil: f64 = MATCH_VIEWS.iter().zip(&self.masks).map(|(v, m)| iou(m, &pt_render::silhouette(&cgeo, &sil_opts(v.0, v.1, self.sil_bounds)))).sum::<f64>() / MATCH_VIEWS.len() as f64;
        Ok(Metrics {
            f_score: f,
            precision: p,
            recall: r,
            chamfer: (mean_dist(&a) + mean_dist(&b)) * 0.5,
            silhouette_iou: sil,
            normal_consistency: (mean_dot(&a) + mean_dot(&b)) * 0.5,
            size_ratio: (diagonal(clo, chi) / self.raw_diag.max(1e-12)) as f64,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Parts
// ---------------------------------------------------------------------------------------------

pub struct Part {
    pub name: String,
    pub tris: Vec<Tri>,
    pub area: f32,
    pub lo: [f32; 3],
    pub hi: [f32; 3],
}

impl Part {
    pub fn new(name: String, tris: Vec<Tri>) -> Option<Part> {
        let (lo, hi) = bounds_of_tris(&tris)?;
        Some(Part { name, area: tris.iter().map(tri_area).sum(), tris, lo, hi })
    }
    pub fn geometry(&self) -> Geometry {
        geometry_from_tris(&self.tris)
    }
}

struct Dsu(Vec<u32>);
impl Dsu {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            self.0[x as usize] = self.0[self.0[x as usize] as usize];
            x = self.0[x as usize];
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[rb.max(ra) as usize] = rb.min(ra);
        }
    }
}

/// Groups of triangle indices that touch each other (vertices closer than `weld` count as one).
pub fn components_of_tris(tris: &[Tri], weld: f32) -> Vec<Vec<usize>> {
    let inv = 1.0 / weld.max(1e-9);
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut corner = Vec::with_capacity(tris.len());
    for t in tris {
        let mut c = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let key = [(p[0] * inv).round() as i64, (p[1] * inv).round() as i64, (p[2] * inv).round() as i64];
            let n = ids.len() as u32;
            c[k] = *ids.entry(key).or_insert(n);
        }
        corner.push(c);
    }
    let mut dsu = Dsu((0..ids.len() as u32).collect());
    for c in &corner {
        dsu.union(c[0], c[1]);
        dsu.union(c[0], c[2]);
    }
    let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, c) in corner.iter().enumerate() {
        groups.entry(dsu.find(c[0])).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| g[0]); // stable order: by first triangle
    out
}

/// Split into parts: first by `groups` (e.g. glTF nodes), then each group by connectivity. Tiny fragments
/// (under `min_area_frac` of the total area) are dropped from the list but still count in the metrics.
pub fn split_parts(groups: &[(String, Vec<Tri>)], min_area_frac: f32) -> Vec<Part> {
    let all: Vec<Tri> = groups.iter().flat_map(|g| g.1.iter().copied()).collect();
    let Some((lo, hi)) = bounds_of_tris(&all) else { return vec![] };
    let weld = diagonal(lo, hi) * 1e-4;
    let total: f32 = all.iter().map(tri_area).sum();
    let mut parts = Vec::new();
    for (name, tris) in groups {
        let comps = components_of_tris(tris, weld);
        let multi = comps.len() > 1;
        for (ci, comp) in comps.iter().enumerate() {
            let t: Vec<Tri> = comp.iter().map(|&i| tris[i]).collect();
            let label = if multi { format!("{name}.{ci}") } else { name.clone() };
            if let Some(p) = Part::new(label, t) {
                if p.area >= total * min_area_frac {
                    parts.push(p);
                }
            }
        }
    }
    parts.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap_or(std::cmp::Ordering::Equal));
    parts
}

// ---------------------------------------------------------------------------------------------
// Validity and tiers
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Validity {
    pub errors: usize,
    pub watertight: bool,
    pub ref_watertight: bool,
    pub triangles: usize,
    pub ref_triangles: usize,
    /// Triangle count at most 2x the source.
    pub within_budget: bool,
    pub ok: bool,
}

impl Validity {
    pub fn to_json(&self) -> Value {
        json!({"errors": self.errors, "watertight": self.watertight, "reference_watertight": self.ref_watertight,
               "triangles": self.triangles, "reference_triangles": self.ref_triangles, "within_budget": self.within_budget, "ok": self.ok})
    }
}

/// True when every edge is shared by exactly two triangles after welding vertices closer than `weld`.
pub fn welded_watertight(tris: &[Tri], weld: f32) -> bool {
    if tris.is_empty() {
        return false;
    }
    let inv = 1.0 / weld.max(1e-9);
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for t in tris {
        let mut c = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let key = [(p[0] * inv).round() as i64, (p[1] * inv).round() as i64, (p[2] * inv).round() as i64];
            let n = ids.len() as u32;
            c[k] = *ids.entry(key).or_insert(n);
        }
        for k in 0..3 {
            let (a, b) = (c[k], c[(k + 1) % 3]);
            if a != b {
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
    }
    edges.values().all(|&n| n == 2)
}

/// "Valid" per B3: no validation errors, watertight if the source is, triangle count at most 2x the source.
pub fn validity(cand: &Geometry, reference: &Geometry) -> Validity {
    let cand = cand.realize();
    let errors = pt_core::validate::validate(&cand).iter().filter(|i| i.severity == "error").count();
    let boundary_free = {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        for i in 0..cand.n_prims() {
            let pr = cand.prim_points(i);
            for k in 0..pr.len() {
                let (a, b) = (pr[k], pr[(k + 1) % pr.len()]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        cand.n_prims() > 0 && edges.values().all(|&n| n % 2 == 0)
    };
    let rt = tris_of(reference);
    let ref_watertight = match bounds_of_tris(&rt) {
        Some((lo, hi)) => welded_watertight(&rt, diagonal(lo, hi) * 1e-4),
        None => false,
    };
    let triangles = cand.triangles().len();
    let within_budget = triangles <= 2 * rt.len().max(1);
    let ok = errors == 0 && (!ref_watertight || boundary_free) && within_budget;
    Validity { errors, watertight: boundary_free, ref_watertight, triangles, ref_triangles: rt.len(), within_budget, ok }
}

/// B3 tiers: bronze F>=0.80 and IoU>=0.80; silver F>=0.90, IoU>=0.90, valid, 3 to 40 free parameters;
/// gold F>=0.95 and IoU>=0.95 on top of silver. "Solved" means silver or better.
pub fn tier(m: &Metrics, valid: bool, n_params: usize) -> &'static str {
    let silver = m.f_score >= 0.90 && m.silhouette_iou >= 0.90 && valid && (3..=40).contains(&n_params);
    if silver && m.f_score >= 0.95 && m.silhouette_iou >= 0.95 {
        "gold"
    } else if silver {
        "silver"
    } else if m.f_score >= 0.80 && m.silhouette_iou >= 0.80 {
        "bronze"
    } else {
        "none"
    }
}

pub fn is_solved(tier: &str) -> bool {
    tier == "silver" || tier == "gold"
}

/// Apply a Mat34 to triangles (used by the solver for primitives).
pub fn transform_tris(tris: &[Tri], m: &Mat34) -> Vec<Tri> {
    tris.iter().map(|t| [pt_core::math::xform_point(m, t[0]), pt_core::math::xform_point(m, t[1]), pt_core::math::xform_point(m, t[2])]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: [f32; 3], at: [f32; 3]) -> Geometry {
        let mut p = Vec::new();
        for z in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for x in [0.0, 1.0f32] {
                    p.push([at[0] + (x - 0.5) * size[0], at[1] + (y - 0.5) * size[1], at[2] + (z - 0.5) * size[2]]);
                }
            }
        }
        let q = vec![vec![0, 2, 3, 1], vec![4, 5, 7, 6], vec![0, 1, 5, 4], vec![2, 6, 7, 3], vec![1, 3, 7, 5], vec![0, 4, 6, 2]];
        Geometry::from_polys(p, &q)
    }

    #[test]
    fn identical_shapes_score_perfectly() {
        let g = cube([2.0, 1.0, 0.5], [3.0, 1.0, -2.0]);
        let r = Reference::new(&g).unwrap();
        let m = r.score(&g, Align::Bbox).unwrap();
        assert_eq!(m.f_score, 1.0);
        assert_eq!(m.silhouette_iou, 1.0);
        assert!(m.chamfer < 1e-5, "chamfer {}", m.chamfer);
        assert!(m.normal_consistency > 0.999, "normal consistency {}", m.normal_consistency);
        assert!((m.size_ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn scores_fall_monotonically_as_the_shape_is_perturbed() {
        let reference = cube([2.0, 1.0, 1.0], [0.0; 3]);
        let r = Reference::new(&reference).unwrap();
        let mut last = (2.0f64, 2.0f64, -1.0f64);
        for k in 0..6 {
            // Grow one dimension: more and more of the surface is wrong.
            let cand = cube([2.0 + 0.5 * k as f32, 1.0, 1.0], [0.0; 3]);
            let m = r.score(&cand, Align::Reference).unwrap();
            assert!(m.f_score <= last.0 + 1e-9, "F-score rose at step {k}: {} -> {}", last.0, m.f_score);
            assert!(m.silhouette_iou <= last.1 + 1e-9, "IoU rose at step {k}");
            assert!(m.chamfer >= last.2 - 1e-9, "chamfer fell at step {k}");
            last = (m.f_score, m.silhouette_iou, m.chamfer);
        }
        assert!(last.0 < 0.7, "a long bar is far from a cube: F = {}", last.0);
    }

    #[test]
    fn bbox_alignment_hides_pure_scale_but_reference_alignment_does_not() {
        let g = cube([1.0, 2.0, 3.0], [0.0; 3]);
        let big = cube([2.0, 4.0, 6.0], [10.0, 0.0, 0.0]);
        let r = Reference::new(&g).unwrap();
        let a = r.score(&big, Align::Bbox).unwrap();
        let b = r.score(&big, Align::Reference).unwrap();
        assert!(a.f_score > 0.999 && (a.size_ratio - 2.0).abs() < 1e-5);
        assert!(b.f_score < 0.5);
    }

    #[test]
    fn flipped_surface_is_caught_by_normal_consistency() {
        let g = cube([1.0, 1.0, 1.0], [0.0; 3]);
        let flipped = g.with_polys(&(0..g.n_prims()).map(|i| g.prim_points(i).iter().rev().copied().collect::<Vec<_>>()).collect::<Vec<_>>());
        let r = Reference::new(&g).unwrap();
        let m = r.score(&flipped, Align::Bbox).unwrap();
        assert!(m.f_score > 0.999 && m.normal_consistency < -0.99, "nc {}", m.normal_consistency);
    }

    #[test]
    fn bvh_matches_brute_force() {
        let g = cube([1.0, 2.0, 3.0], [0.3, 0.1, -0.2]);
        let tris = tris_of(&g);
        let bvh = TriBvh::new(&tris).unwrap();
        for i in 0..200u64 {
            let r = pt_core::rng::rand4(9, 1, i);
            let q = [r[0] * 6.0 - 3.0, r[1] * 6.0 - 3.0, r[2] * 6.0 - 3.0];
            let brute = tris.iter().map(|t| {
                let c = closest_on_tri(q, t[0], t[1], t[2]);
                let d = sub(c, q);
                dot(d, d)
            }).fold(f32::INFINITY, f32::min);
            assert!((bvh.closest(q).0 - brute).abs() < 1e-5, "query {i}");
        }
    }

    #[test]
    fn two_boxes_split_into_two_parts_with_bounds() {
        let mut g = cube([1.0, 1.0, 1.0], [0.0; 3]);
        g.append(&cube([1.0, 2.0, 1.0], [5.0, 0.0, 0.0])).unwrap();
        let parts = split_parts(&[("model".to_string(), tris_of(&g))], 0.0);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].area > parts[1].area, "largest first");
        assert!((parts[0].hi[1] - parts[0].lo[1] - 2.0).abs() < 1e-5);
        assert!((parts[1].hi[0] - parts[1].lo[0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn validity_checks_watertightness_and_size() {
        let g = cube([1.0; 3], [0.0; 3]);
        let v = validity(&g, &g);
        assert!(v.ok && v.watertight && v.ref_watertight);
        let open = g.with_polys(&[vec![0, 2, 3, 1]]);
        assert!(!validity(&open, &g).ok, "an open surface cannot stand in for a watertight reference");
    }

    #[test]
    fn tiers_follow_the_published_thresholds() {
        let m = |f: f64, i: f64| Metrics { f_score: f, silhouette_iou: i, ..Default::default() };
        assert_eq!(tier(&m(0.96, 0.96), true, 12), "gold");
        assert_eq!(tier(&m(0.92, 0.91), true, 12), "silver");
        assert_eq!(tier(&m(0.92, 0.91), false, 12), "bronze");
        assert_eq!(tier(&m(0.92, 0.91), true, 41), "bronze");
        assert_eq!(tier(&m(0.92, 0.91), true, 2), "bronze");
        assert_eq!(tier(&m(0.7, 0.9), true, 12), "none");
    }
}

/// Largest patch of the reference surface that the candidate does not cover.
///
/// Reference samples farther than `F_THRESHOLD` from the candidate are grouped into patches that touch (voxels of 3% of the
/// diagonal, 26-neighbourhood). Returns the largest patch as a share of the whole reference surface, and the number of
/// patches above 2%. A thin missing part (one rung of a ladder, one shelf) is a small share of the *area* the F-score averages
/// over, so it can pass the F-score; as a patch it stands out. This is a second reading that fails differently.
pub fn uncovered_patches(reference: &Reference, cand: &Geometry) -> Result<(f64, usize), String> {
    let raw = tris_of(cand);
    let norm = Norm::of_tris(&raw)?;
    let cb = TriBvh::new(&norm.tris(&raw))?;
    let d = distances_to(&cb, &reference.samples);
    let cell = 0.03f32;
    let key = |p: [f32; 3]| [(p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32, (p[2] / cell).floor() as i32];
    let mut voxels: HashMap<[i32; 3], usize> = HashMap::new();
    for (s, dist) in reference.samples.iter().zip(&d) {
        if dist.0 > F_THRESHOLD {
            *voxels.entry(key(s.0)).or_default() += 1;
        }
    }
    let mut seen: std::collections::HashSet<[i32; 3]> = Default::default();
    let (mut largest, mut big) = (0usize, 0usize);
    let n = reference.samples.len().max(1);
    let keys: Vec<[i32; 3]> = voxels.keys().copied().collect();
    for k in keys {
        if !seen.insert(k) {
            continue;
        }
        let (mut count, mut stack) = (0usize, vec![k]);
        while let Some(c) = stack.pop() {
            count += voxels[&c];
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let nb = [c[0] + dx, c[1] + dy, c[2] + dz];
                        if voxels.contains_key(&nb) && seen.insert(nb) {
                            stack.push(nb);
                        }
                    }
                }
            }
        }
        largest = largest.max(count);
        if count * 50 >= n {
            big += 1;
        }
    }
    Ok((largest as f64 / n as f64, big))
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn a_missing_part_shows_up_as_a_patch() {
        let slab = |size: [f32; 3], at: [f32; 3]| {
            let p = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]].map(|v| [at[0] + (v[0] - 0.5) * size[0], at[1] + (v[1] - 0.5) * size[1], at[2] + (v[2] - 0.5) * size[2]]);
            Geometry::from_polys(p.to_vec(), &[vec![0, 3, 2, 1], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![2, 3, 7, 6], vec![1, 2, 6, 5], vec![0, 4, 7, 3]])
        };
        let mut full = slab([2.0, 0.2, 0.2], [0.0, 0.0, 0.0]);
        full.append(&slab([0.2, 2.0, 0.2], [-0.9, 1.0, 0.0])).unwrap();
        full.append(&slab([0.2, 2.0, 0.2], [0.9, 1.0, 0.0])).unwrap();
        let r = Reference::new(&full).unwrap();
        let (none, _) = uncovered_patches(&r, &full).unwrap();
        assert!(none < 1e-9, "identical shapes leave no patch: {none}");
        // Drop one of the two posts: a big missing patch.
        let mut partial = slab([2.0, 0.2, 0.2], [0.0, 0.0, 0.0]);
        partial.append(&slab([0.2, 2.0, 0.2], [-0.9, 1.0, 0.0])).unwrap();
        let (share, big) = uncovered_patches(&r, &partial).unwrap();
        assert!(share > 0.15 && big >= 1, "a missing post is a large patch: {share} ({big})");
    }
}
