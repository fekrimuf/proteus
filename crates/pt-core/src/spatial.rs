//! Spatial indices behind the questions a program can ask of a second geometry (nearest point, points within a radius,
//! closest point on a surface, rays). Deterministic: only + - * / and sqrt, ties go to the lower index, builds break ties by
//! index, so the same input gives the same answer on every machine and thread count.

use crate::geo::Geometry;
use crate::math::{cross, normalize, sub};

type V3 = [f32; 3];

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn d2(a: V3, b: V3) -> f32 {
    let d = sub(a, b);
    dot(d, d)
}

// ---------------------------------------------------------------------------------------------
// Points: a balanced k-d tree stored implicitly (the median of every index range is its node)
// ---------------------------------------------------------------------------------------------

pub struct PointTree {
    pts: Vec<V3>,
    idx: Vec<u32>,
    axis: Vec<u8>,
}

impl PointTree {
    pub fn new(pts: &[V3]) -> PointTree {
        let mut idx: Vec<u32> = (0..pts.len() as u32).collect();
        let mut axis = vec![0u8; pts.len()];
        Self::build(pts, &mut idx, &mut axis);
        PointTree { pts: pts.to_vec(), idx, axis }
    }

    fn build(pts: &[V3], idx: &mut [u32], axis: &mut [u8]) {
        if idx.len() <= 1 {
            return;
        }
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &i in idx.iter() {
            for a in 0..3 {
                lo[a] = lo[a].min(pts[i as usize][a]);
                hi[a] = hi[a].max(pts[i as usize][a]);
            }
        }
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let ax = if ext[0] >= ext[1] && ext[0] >= ext[2] { 0 } else if ext[1] >= ext[2] { 1 } else { 2 };
        let mid = idx.len() / 2;
        idx.select_nth_unstable_by(mid, |&a, &b| pts[a as usize][ax].total_cmp(&pts[b as usize][ax]).then(a.cmp(&b)));
        axis[mid] = ax as u8;
        let (l, rest) = idx.split_at_mut(mid);
        let (_, r) = rest.split_at_mut(1);
        let (al, arest) = axis.split_at_mut(mid);
        let (_, ar) = arest.split_at_mut(1);
        Self::build(pts, l, al);
        Self::build(pts, r, ar);
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    pub fn pos(&self, i: u32) -> V3 {
        self.pts[i as usize]
    }

    /// Index and distance of the point closest to `q`, ignoring point `skip`; equal distances go to the lower index.
    pub fn nearest(&self, q: V3, skip: Option<u32>) -> Option<(u32, f32)> {
        let mut best = (f32::INFINITY, u32::MAX);
        self.nn(q, skip, 0, self.idx.len(), &mut best);
        (best.1 != u32::MAX).then(|| (best.1, best.0.sqrt()))
    }

    fn nn(&self, q: V3, skip: Option<u32>, lo: usize, hi: usize, best: &mut (f32, u32)) {
        if lo >= hi {
            return;
        }
        let mid = (lo + hi) / 2;
        let i = self.idx[mid];
        if Some(i) != skip {
            let d = d2(self.pts[i as usize], q);
            if d < best.0 || (d == best.0 && i < best.1) {
                *best = (d, i);
            }
        }
        if hi - lo == 1 {
            return;
        }
        let ax = self.axis[mid] as usize;
        let diff = q[ax] - self.pts[i as usize][ax];
        let (near, far) = if diff < 0.0 { ((lo, mid), (mid + 1, hi)) } else { ((mid + 1, hi), (lo, mid)) };
        self.nn(q, skip, near.0, near.1, best);
        if diff * diff <= best.0 {
            self.nn(q, skip, far.0, far.1, best);
        }
    }

    /// Calls `f` with every point within `r` of `q`, in a fixed order.
    pub fn within(&self, q: V3, r: f32, f: &mut dyn FnMut(u32)) {
        self.range(q, r, r * r, 0, self.idx.len(), f);
    }

    fn range(&self, q: V3, r: f32, r2: f32, lo: usize, hi: usize, f: &mut dyn FnMut(u32)) {
        if lo >= hi {
            return;
        }
        let mid = (lo + hi) / 2;
        let i = self.idx[mid];
        if d2(self.pts[i as usize], q) <= r2 {
            f(i);
        }
        if hi - lo == 1 {
            return;
        }
        let ax = self.axis[mid] as usize;
        let diff = q[ax] - self.pts[i as usize][ax];
        if diff - r <= 0.0 {
            self.range(q, r, r2, lo, mid, f);
        }
        if diff + r >= 0.0 {
            self.range(q, r, r2, mid + 1, hi, f);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Surface: a triangle BVH with closest-point and ray queries
// ---------------------------------------------------------------------------------------------

/// A point on a surface: where, how far, the face normal (turned toward the asker), and the triangle's three point ids with the
/// barycentric weights of the hit (so a point attribute can be interpolated there).
#[derive(Clone, Copy, Debug)]
pub struct SurfHit {
    pub dist: f32,
    pub pos: V3,
    pub n: V3,
    pub pts: [u32; 3],
    pub bary: [f32; 3],
}

struct Node {
    lo: V3,
    hi: V3,
    /// Interior: indices of the children. Leaf (`count > 0`): `left` is the first triangle.
    left: u32,
    right: u32,
    count: u32,
}

pub struct SurfaceTree {
    ids: Vec<[u32; 3]>,
    v: Vec<[V3; 3]>,
    nodes: Vec<Node>,
}

/// Closest point on triangle `abc` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_on_tri(p: V3, a: V3, b: V3, c: V3) -> V3 {
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

fn box_d2(lo: &V3, hi: &V3, q: V3) -> f32 {
    let mut d = 0.0;
    for a in 0..3 {
        let e = (lo[a] - q[a]).max(q[a] - hi[a]).max(0.0);
        d += e * e;
    }
    d
}

fn bary_of(p: V3, a: V3, b: V3, c: V3) -> [f32; 3] {
    let (v0, v1, v2) = (sub(b, a), sub(c, a), sub(p, a));
    let (d00, d01, d11, d20, d21) = (dot(v0, v0), dot(v0, v1), dot(v1, v1), dot(v2, v0), dot(v2, v1));
    let den = d00 * d11 - d01 * d01;
    if den.abs() < 1e-20 {
        return [1.0, 0.0, 0.0];
    }
    let v = (d11 * d20 - d01 * d21) / den;
    let w = (d00 * d21 - d01 * d20) / den;
    [1.0 - v - w, v, w]
}

impl SurfaceTree {
    /// Indexes the base primitives of `g` (polygons are fanned into triangles; instances are not included: `realize` first).
    /// `None` when there is no triangle with an area.
    pub fn new(g: &Geometry) -> Option<SurfaceTree> {
        let p = g.p();
        let mut ids: Vec<[u32; 3]> = g.triangles().into_iter().filter(|t| t.iter().all(|&i| (i as usize) < p.len())).collect();
        ids.retain(|t| {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let n = cross(sub(b, a), sub(c, a));
            dot(n, n) > 1e-24
        });
        if ids.is_empty() {
            return None;
        }
        let cent: Vec<V3> = ids
            .iter()
            .map(|t| {
                let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0]
            })
            .collect();
        let mut order: Vec<u32> = (0..ids.len() as u32).collect();
        let mut nodes = Vec::with_capacity(ids.len() / 2 + 2);
        Self::build(p, &ids, &cent, &mut order, 0, &mut nodes);
        let ids: Vec<[u32; 3]> = order.iter().map(|&i| ids[i as usize]).collect();
        let v = ids.iter().map(|t| [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]).collect();
        Some(SurfaceTree { ids, v, nodes })
    }

    fn build(p: &[V3], ids: &[[u32; 3]], cent: &[V3], order: &mut [u32], base: usize, nodes: &mut Vec<Node>) -> u32 {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let (mut clo, mut chi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for &i in order.iter() {
            for &k in &ids[i as usize] {
                for a in 0..3 {
                    lo[a] = lo[a].min(p[k as usize][a]);
                    hi[a] = hi[a].max(p[k as usize][a]);
                }
            }
            let c = cent[i as usize];
            for a in 0..3 {
                clo[a] = clo[a].min(c[a]);
                chi[a] = chi[a].max(c[a]);
            }
        }
        let me = nodes.len() as u32;
        if order.len() <= 4 {
            nodes.push(Node { lo, hi, left: base as u32, right: 0, count: order.len() as u32 });
            return me;
        }
        nodes.push(Node { lo, hi, left: 0, right: 0, count: 0 });
        let ext = [chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2]];
        let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] { 0 } else if ext[1] >= ext[2] { 1 } else { 2 };
        let mid = order.len() / 2;
        order.select_nth_unstable_by(mid, |&a, &b| cent[a as usize][axis].total_cmp(&cent[b as usize][axis]).then(a.cmp(&b)));
        let (l, r) = order.split_at_mut(mid);
        let left = Self::build(p, ids, cent, l, base, nodes);
        let right = Self::build(p, ids, cent, r, base + mid, nodes);
        nodes[me as usize].left = left;
        nodes[me as usize].right = right;
        me
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn face_normal(&self, t: usize) -> V3 {
        let [a, b, c] = self.v[t];
        normalize(cross(sub(b, a), sub(c, a)))
    }

    /// The point of the surface closest to `q`; the normal is turned to face `q`.
    pub fn closest(&self, q: V3) -> SurfHit {
        let mut best = (f32::INFINITY, 0usize, [0.0f32; 3]);
        let mut stack = [0u32; 96];
        let mut sp = 1;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if box_d2(&n.lo, &n.hi, q) > best.0 {
                continue;
            }
            if n.count > 0 {
                for i in n.left..n.left + n.count {
                    let t = &self.v[i as usize];
                    let c = closest_on_tri(q, t[0], t[1], t[2]);
                    let d = d2(c, q);
                    if d < best.0 {
                        best = (d, i as usize, c);
                    }
                }
            } else {
                let (l, r) = (&self.nodes[n.left as usize], &self.nodes[n.right as usize]);
                let (dl, dr) = (box_d2(&l.lo, &l.hi, q), box_d2(&r.lo, &r.hi, q));
                let (far, near, dfar, dnear) = if dl <= dr { (n.right, n.left, dr, dl) } else { (n.left, n.right, dl, dr) };
                if sp + 2 < stack.len() {
                    if dfar <= best.0 {
                        stack[sp] = far;
                        sp += 1;
                    }
                    if dnear <= best.0 {
                        stack[sp] = near;
                        sp += 1;
                    }
                }
            }
        }
        let (dd, t, pos) = best;
        let [a, b, c] = self.v[t];
        let mut n = self.face_normal(t);
        if dot(n, sub(q, pos)) < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        SurfHit { dist: dd.sqrt(), pos, n, pts: self.ids[t], bary: bary_of(pos, a, b, c) }
    }

    /// The first surface met by the ray from `o` along `d` (any length; zero gives `None`), either side of a triangle. The
    /// normal faces the ray's origin.
    pub fn ray(&self, o: V3, d: V3) -> Option<SurfHit> {
        let len = dot(d, d).sqrt();
        if !(len > 0.0) {
            return None;
        }
        let d = [d[0] / len, d[1] / len, d[2] / len];
        let mut best = (f32::INFINITY, 0usize, 0.0f32, 0.0f32);
        let mut stack = [0u32; 96];
        let mut sp = 1;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if !slab(&n.lo, &n.hi, o, d, best.0) {
                continue;
            }
            if n.count > 0 {
                for i in n.left..n.left + n.count {
                    if let Some((t, u, v)) = tri_ray(o, d, &self.v[i as usize]) {
                        if t < best.0 {
                            best = (t, i as usize, u, v);
                        }
                    }
                }
            } else if sp + 2 < stack.len() {
                stack[sp] = n.right;
                stack[sp + 1] = n.left;
                sp += 2;
            }
        }
        if !best.0.is_finite() {
            return None;
        }
        let (t, i, u, v) = best;
        let mut nrm = self.face_normal(i);
        if dot(nrm, d) > 0.0 {
            nrm = [-nrm[0], -nrm[1], -nrm[2]];
        }
        Some(SurfHit { dist: t, pos: [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t], n: nrm, pts: self.ids[i], bary: [1.0 - u - v, u, v] })
    }
}

/// Does the ray (unit direction) cross the box before `tmax`?
fn slab(lo: &V3, hi: &V3, o: V3, d: V3, tmax: f32) -> bool {
    let (mut t0, mut t1) = (0.0f32, tmax);
    for a in 0..3 {
        if d[a].abs() < 1e-12 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return false;
            }
            continue;
        }
        let inv = 1.0 / d[a];
        let (mut ta, mut tb) = ((lo[a] - o[a]) * inv, (hi[a] - o[a]) * inv);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// Moller-Trumbore, either side. Returns (distance, u, v). A hair of tolerance on the edges so a ray through a shared edge of a
/// grid does not fall between its two triangles.
fn tri_ray(o: V3, d: V3, t: &[V3; 3]) -> Option<(f32, f32, f32)> {
    const EPS: f32 = 1e-6;
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let pv = cross(d, e2);
    let det = dot(e1, pv);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = sub(o, t[0]);
    let u = dot(tv, pv) * inv;
    if u < -EPS || u > 1.0 + EPS {
        return None;
    }
    let qv = cross(tv, e1);
    let v = dot(d, qv) * inv;
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let dist = dot(e2, qv) * inv;
    (dist > 1e-6).then_some((dist, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::rand4;

    fn cloud(n: usize, seed: i64) -> Vec<V3> {
        (0..n as u64).map(|i| { let r = rand4(seed, 1, i); [r[0] * 20.0 - 10.0, r[1] * 6.0, r[2] * 20.0 - 10.0] }).collect()
    }

    #[test]
    fn nearest_and_within_match_brute_force() {
        let pts = cloud(700, 3);
        let tree = PointTree::new(&pts);
        for (k, q) in cloud(60, 9).into_iter().enumerate() {
            let skip = (k % 3 == 0).then_some(k as u32);
            let (bi, bd) = (0..pts.len() as u32).filter(|&i| Some(i) != skip).map(|i| (i, d2(pts[i as usize], q))).fold((u32::MAX, f32::INFINITY), |a, b| if b.1 < a.1 { (b.0, b.1) } else { a });
            let (ti, td) = tree.nearest(q, skip).unwrap();
            assert_eq!(ti, bi, "nearest index for query {k}");
            assert!((td - bd.sqrt()).abs() < 1e-5);
            let mut got = Vec::new();
            tree.within(q, 2.5, &mut |i| got.push(i));
            got.sort();
            let want: Vec<u32> = (0..pts.len() as u32).filter(|&i| d2(pts[i as usize], q) <= 6.25).collect();
            assert_eq!(got, want, "within for query {k}");
        }
        assert!(PointTree::new(&[]).nearest([0.0; 3], None).is_none());
    }

    #[test]
    fn equal_distances_go_to_the_lower_index() {
        let pts = vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]];
        let tree = PointTree::new(&pts);
        assert_eq!(tree.nearest([0.0; 3], None).unwrap().0, 0);
        assert_eq!(tree.nearest([0.0; 3], Some(0)).unwrap().0, 1);
    }

    fn bumpy_grid() -> Geometry {
        let n = 24usize;
        let mut pts = Vec::new();
        for z in 0..=n {
            for x in 0..=n {
                let (fx, fz) = (x as f32 * 0.5, z as f32 * 0.5);
                pts.push([fx, 0.3 * (fx * 0.9).sin() + 0.2 * (fz * 1.3).cos(), fz]);
            }
        }
        let w = (n + 1) as u32;
        let mut prims = Vec::new();
        for z in 0..n as u32 {
            for x in 0..n as u32 {
                prims.push(vec![z * w + x, z * w + x + 1, (z + 1) * w + x + 1, (z + 1) * w + x]);
            }
        }
        Geometry::from_polys(pts, &prims)
    }

    #[test]
    fn closest_point_matches_brute_force() {
        let g = bumpy_grid();
        let tree = SurfaceTree::new(&g).unwrap();
        let tris = g.triangles();
        let p = g.p();
        for q in cloud(80, 5).into_iter().map(|q| [q[0] + 6.0, q[1] - 1.0, q[2] + 6.0]) {
            let best = tris.iter().map(|t| d2(closest_on_tri(q, p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]), q)).fold(f32::INFINITY, f32::min);
            let h = tree.closest(q);
            assert!((h.dist - best.sqrt()).abs() < 1e-4, "{} vs {}", h.dist, best.sqrt());
            let back = [0, 1, 2].map(|c| (0..3).map(|k| p[h.pts[k] as usize][c] * h.bary[k]).sum::<f32>());
            assert!(d2(back, h.pos) < 1e-8, "barycentric weights rebuild the point");
        }
    }

    #[test]
    fn rays_hit_the_first_surface_from_either_side() {
        let g = bumpy_grid();
        let tree = SurfaceTree::new(&g).unwrap();
        let p = g.p();
        for (x, z) in [(3.3f32, 4.1f32), (6.0, 6.0), (0.5, 0.5), (11.9, 2.2)] {
            let down = tree.ray([x, 50.0, z], [0.0, -1.0, 0.0]).expect("hit from above");
            let up = tree.ray([x, -50.0, z], [0.0, 1.0, 0.0]).expect("hit from below");
            assert!((down.pos[1] - up.pos[1]).abs() < 1e-3, "same surface from both sides");
            assert!((50.0 - down.dist - down.pos[1]).abs() < 1e-3);
            assert!(down.n[1] > 0.0 && up.n[1] < 0.0, "the normal faces the origin");
            let y = (0..3).map(|k| p[down.pts[k] as usize][1] * down.bary[k]).sum::<f32>();
            assert!((y - down.pos[1]).abs() < 1e-3);
        }
        assert!(tree.ray([100.0, 5.0, 100.0], [0.0, -1.0, 0.0]).is_none(), "off the grid");
        assert!(tree.ray([3.0, 5.0, 3.0], [0.0, 0.0, 0.0]).is_none());
    }
}
