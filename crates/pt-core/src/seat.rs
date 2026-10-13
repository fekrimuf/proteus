//! How a placed object meets the ground, measured on its real shape and not on its bounding box.
//!
//! A bounding box says a 19 m fir touches the ground as soon as the middle of its trunk does, and that a log touches as soon as
//! one branch stub does. The eye disagrees: it sees the slab of scanned soil under the fir standing out of the slope, and a
//! trunk standing in the air on a twig above the water. So an object's underside is described once (`Sole`: the outline of its
//! base, the top of the slab it stands on, and the level where its body starts) and that one description feeds both the
//! placement node (`seat`) and the check (`pt ground-check`): the rule and its guard measure the same thing.

use crate::math::{cross, normalize, xform_point, Mat34};
use crate::Geometry;

/// Air under the base that an eye sees from a path (m): the check fails above it, and `seat` leaves none.
pub const BASE_AIR: f32 = 0.08;
/// Share of an object's surface that may lie below its "body" (branch stubs, root tips, a twig it would stand on).
const BODY_SHARE: f32 = 0.12;

/// The underside of a prototype, in its own space.
pub struct Sole {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    /// Vertices of the base (the slab a scanned tree stands on, else the lowest 3 to 15 cm): the rim first, then a spread.
    pub base: Vec<[f32; 3]>,
    /// Width of the base (m): the root slab of a tree, not its crown.
    pub base_width: f32,
    /// Top of the slab a standing object brings with it (a scanned tree stands on a wedge of soil and roots three times as
    /// wide as its trunk), as a height above the lowest point; 0 when it has none. All of it belongs in the ground.
    pub collar: f32,
    /// Height above the lowest point under which `BODY_SHARE` of the surface lies: where the body starts.
    pub body: f32,
    /// Wider than twice its height: a log, a branch, a grass tile. Such a thing lies along the ground, on its body.
    pub lying: bool,
}

impl Sole {
    pub fn of(g: &Geometry) -> Option<Sole> {
        let (lo, hi) = g.base_bounds()?;
        let height = hi[1] - lo[1];
        if height <= 0.0 {
            return None;
        }
        let p = g.p();
        let lying = height < 0.5 * (hi[0] - lo[0]).max(hi[2] - lo[2]);
        // the slab under a standing object: 10 cm slices from the bottom; the base is wide, and above some level the object is
        // suddenly a trunk (at most 0.6 of the bottom's width). The slab's top is the top of the last wide slice.
        let mut collar = 0.0f32;
        if !lying && height > 3.0 {
            let step = 0.1f32;
            let slices = ((0.08 * height).min(1.0) / step).ceil() as usize;
            let mut ext = vec![(f32::MAX, f32::MIN, f32::MAX, f32::MIN); slices];
            for q in p {
                let i = ((q[1] - lo[1]) / step) as usize;
                if i < slices {
                    let e = &mut ext[i];
                    *e = (e.0.min(q[0]), e.1.max(q[0]), e.2.min(q[2]), e.3.max(q[2]));
                }
            }
            let widths: Vec<Option<f32>> = ext.iter().map(|e| (e.1 >= e.0).then(|| (e.1 - e.0).max(e.3 - e.2))).collect();
            if let Some(bottom) = widths.iter().flatten().next().copied() {
                let mut last_wide = 0usize;
                for (i, w) in widths.iter().enumerate() {
                    match w {
                        Some(w) if *w <= 0.6 * bottom => {
                            collar = (last_wide + 1) as f32 * step;
                            break;
                        }
                        Some(_) => last_wide = i,
                        None => {}
                    }
                }
            }
        }
        let band = collar.max((0.02 * height).clamp(0.03, 0.15));
        let low: Vec<[f32; 3]> = p.iter().filter(|q| q[1] <= lo[1] + band).copied().collect();
        let n = low.len().max(1) as f32;
        let (cx, cz) = (low.iter().map(|q| q[0]).sum::<f32>() / n, low.iter().map(|q| q[2]).sum::<f32>() / n);
        // the rim: the farthest base vertex in each of 16 directions (the downhill edge is always one of them)
        let mut base: Vec<[f32; 3]> = Vec::new();
        let mut width = 0.0f32;
        for k in 0..16 {
            let (s, c) = crate::math::sin_cos(k as f32 * std::f32::consts::FRAC_PI_8);
            let reach = |q: &[f32; 3]| (q[0] - cx) * c + (q[2] - cz) * s;
            if let Some(q) = low.iter().max_by(|a, b| reach(a).partial_cmp(&reach(b)).unwrap_or(std::cmp::Ordering::Equal)) {
                width = width.max(2.0 * reach(q));
                base.push(*q);
            }
        }
        let stride = (low.len() / 32).max(1);
        base.extend(low.iter().step_by(stride).take(32));
        // where the body starts: the height under which a small share of the surface lies
        let mut slabs: Vec<(f32, f32)> = g
            .triangles()
            .iter()
            .map(|t| {
                let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
                let n = cross(crate::math::sub(b, a), crate::math::sub(c, a));
                ((a[1] + b[1] + c[1]) / 3.0 - lo[1], 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt())
            })
            .collect();
        slabs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let total: f32 = slabs.iter().map(|s| s.1).sum();
        let (mut acc, mut body) = (0.0f32, 0.0f32);
        for (h, a) in &slabs {
            acc += a;
            if acc >= BODY_SHARE * total {
                body = *h;
                break;
            }
        }
        Some(Sole { lo, hi, base, base_width: width, collar, body, lying })
    }
}

/// One instance against the ground (and the water over it, where there is any).
pub struct Fit {
    pub y_lo: f32,
    pub height: f32,
    /// World height of the middle of the object's underside.
    pub mid_y: f32,
    /// Ground under the middle of the object.
    pub ground: f32,
    /// Water surface over the middle of the object, where it lies above the ground.
    pub water: Option<f32>,
    /// Largest air gap under the base, measured at the top of its slab (m); zero or less when the whole base, slab and all,
    /// is in the ground or the water.
    pub base_gap: f32,
    /// The slab's thickness in world metres (0 without a slab): `base_gap - collar` is how far the ground falls away.
    pub collar: f32,
    /// Air between the body of a lying object and what carries it (ground or water); `None` for a standing object.
    pub body_gap: Option<f32>,
    /// Height of the body's start above the object's underside, in world metres.
    pub body: f32,
    /// What the object stands on, in world metres across: the base of a standing object, the length of a lying one.
    pub reach: f32,
    /// Least and most air under the bounding box (its middle and a ring at a quarter of its width), over ground or water.
    pub ring: (f32, f32),
}

pub fn fit(sole: &Sole, xf: &Mat34, ground: &dyn Fn(f32, f32) -> Option<f32>, water: &dyn Fn(f32, f32) -> Option<f32>) -> Option<Fit> {
    let (lo, hi) = (sole.lo, sole.hi);
    let (mut y_lo, mut y_hi) = (f32::MAX, f32::MIN);
    for c in 0..8 {
        let q = xform_point(xf, [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }]);
        y_lo = y_lo.min(q[1]);
        y_hi = y_hi.max(q[1]);
    }
    let (cx, cz, qx, qz) = ((lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5, (hi[0] - lo[0]) * 0.25, (hi[2] - lo[2]) * 0.25);
    let mid = xform_point(xf, [cx, lo[1], cz]);
    let g = ground(mid[0], mid[2])?;
    let w = water(mid[0], mid[2]).filter(|w| *w > g + 0.02);
    let mut base_gap = f32::MIN;
    for s in &sole.base {
        // the base is taken as flat at the top of its slab (the object's lowest level when it has none), over its real outline
        let q = xform_point(xf, [s[0], lo[1] + sole.collar, s[2]]);
        if let Some(gy) = ground(q[0], q[2]) {
            let carried = water(q[0], q[2]).map_or(gy, |wy| wy.max(gy));
            base_gap = base_gap.max(q[1] - carried);
        }
    }
    if base_gap == f32::MIN {
        base_gap = 0.0;
    }
    let mut ring = (f32::MAX, f32::MIN);
    for (sx, sz) in [(0.0, 0.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        let q = xform_point(xf, [cx + sx * qx, lo[1], cz + sz * qz]);
        if let Some(gy) = ground(q[0], q[2]) {
            let air = y_lo - water(q[0], q[2]).map_or(gy, |wy| wy.max(gy));
            ring = (ring.0.min(air), ring.1.max(air));
        }
    }
    let scale = |a: usize| (xf[a] * xf[a] + xf[a + 4] * xf[a + 4] + xf[a + 8] * xf[a + 8]).sqrt();
    let body = sole.body * scale(1);
    let wide = scale(0).max(scale(2));
    let reach = if sole.lying { (hi[0] - lo[0]).max(hi[2] - lo[2]) * wide } else { (sole.base_width * wide).max(0.3) };
    let body_gap = sole.lying.then(|| mid[1] + body - w.map_or(g, |w| w.max(g)));
    Some(Fit { y_lo, height: y_hi - y_lo, mid_y: mid[1], ground: g, water: w, base_gap, collar: sole.collar * scale(1), body_gap, body, reach, ring })
}

impl Fit {
    /// The base hangs in the air: a root slab over a slope, a grass tile over a bank's edge.
    pub fn base_in_air(&self) -> bool {
        self.base_gap > BASE_AIR
    }

    /// A lying object whose body is in the air: it stands on a stub or a twig, over the ground or over the water.
    pub fn propped(&self) -> Option<f32> {
        self.body_gap.filter(|g| *g > (0.25 * self.height).max(0.1))
    }

    /// What an eye would call this object, with the metres of air: `float` (nothing touches, or a lying object stands on a
    /// stub), `overhang` (a side of the box, or a part of the real base, hangs in the air), or `None` when it sits right.
    pub fn verdict(&self) -> Option<(&'static str, f32)> {
        let tol = (0.04 * self.height).max(0.08);
        if self.ring.0 > tol {
            Some(("float", self.ring.0))
        } else if self.ring.1 > self.height / 3.0 && self.ring.1 > 0.2 {
            Some(("overhang", self.ring.1))
        } else if self.base_in_air() {
            Some(("overhang", self.base_gap))
        } else {
            self.propped().map(|air| ("float", air))
        }
    }
}

/// A lying object turned to lie along the ground under its whole footprint: the plane that fits the ground best under a 5 x 5
/// grid of its box (not the normal at one point, which a lump or a bank's edge turns any way). Tilts of more than 40 degrees
/// are left alone. The origin stays where it is.
fn conformed(sole: &Sole, xf: &Mat34, ground: &dyn Fn(f32, f32) -> Option<f32>) -> Option<Mat34> {
    let (lo, hi) = (sole.lo, sole.hi);
    // sums for the least squares of y = a x + b z + c, in f64 (the terms differ by orders of magnitude)
    let mut s = [0.0f64; 9]; // xx xz zz x z xy zy y n
    for i in 0..5 {
        for j in 0..5 {
            let q = xform_point(xf, [lo[0] + (hi[0] - lo[0]) * i as f32 / 4.0, lo[1], lo[2] + (hi[2] - lo[2]) * j as f32 / 4.0]);
            if let Some(gy) = ground(q[0], q[2]) {
                let (x, z, y) = ((q[0] - xf[3]) as f64, (q[2] - xf[11]) as f64, gy as f64);
                for (k, v) in [x * x, x * z, z * z, x, z, x * y, z * y, y, 1.0].into_iter().enumerate() {
                    s[k] += v;
                }
            }
        }
    }
    let [sxx, sxz, szz, sx, sz, sxy, szy, sy, n] = s;
    let det = sxx * (szz * n - sz * sz) - sxz * (sxz * n - sz * sx) + sx * (sxz * sz - szz * sx);
    if n < 6.0 || det.abs() < 1e-9 {
        return None;
    }
    let a = (sxy * (szz * n - sz * sz) - sxz * (szy * n - sz * sy) + sx * (szy * sz - szz * sy)) / det;
    let b = (sxx * (szy * n - sz * sy) - sxy * (sxz * n - sz * sx) + sx * (sxz * sy - szy * sx)) / det;
    let up = normalize([-a as f32, 1.0, -b as f32]);
    if up[1] < 0.766 {
        return None;
    }
    let now = normalize([xf[1], xf[5], xf[9]]);
    let axis = cross(now, up);
    let sin = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if sin < 1e-5 {
        return Some(*xf);
    }
    let (k, cos) = ([axis[0] / sin, axis[1] / sin, axis[2] / sin], now[0] * up[0] + now[1] * up[1] + now[2] * up[2]);
    // Rodrigues: turn every column of the linear part about `k`
    let turn = |v: [f32; 3]| {
        let kxv = cross(k, v);
        let kv = (k[0] * v[0] + k[1] * v[1] + k[2] * v[2]) * (1.0 - cos);
        [v[0] * cos + kxv[0] * sin + k[0] * kv, v[1] * cos + kxv[1] * sin + k[1] * kv, v[2] * cos + kxv[2] * sin + k[2] * kv]
    };
    let mut m = *xf;
    for col in 0..3 {
        let v = turn([xf[col], xf[col + 4], xf[col + 8]]);
        m[col] = v[0];
        m[col + 4] = v[1];
        m[col + 8] = v[2];
    }
    Some(m)
}

/// Where `seat` puts one instance: its new transform, or `None` when it does not belong there (the ground falls away under it
/// by more than `limit` of its size: the edge of a bank, a ledge).
///
/// A standing object goes down until its whole base, the slab it stands on included, is in the ground, `bury` metres deep at
/// the rim with the most air (roots go into the soil). A lying object is turned to lie along the ground under its whole
/// footprint and comes down until its body rests on it (its stubs dig in). A thing that floats (`draft` > 0: wood) lies level
/// in the water with that share of its body under the surface wherever the water is deep enough for it, and on the bed where
/// it is not.
pub fn seat(sole: &Sole, xf: &Mat34, ground: &dyn Fn(f32, f32) -> Option<f32>, water: &dyn Fn(f32, f32) -> Option<f32>, limit: f32, draft: f32, bury: f32) -> Option<Mat34> {
    seat_or_why(sole, xf, ground, water, limit, draft, bury).ok()
}

/// `seat`, with the reason an instance does not belong: `limit` (the ground falls away), or the verdict it would still get.
pub fn seat_or_why(sole: &Sole, xf: &Mat34, ground: &dyn Fn(f32, f32) -> Option<f32>, water: &dyn Fn(f32, f32) -> Option<f32>, limit: f32, draft: f32, bury: f32) -> Result<Mat34, &'static str> {
    let dry = |_: f32, _: f32| None;
    let drop_of = |f: &Fit| {
        let base = (f.base_gap + bury.max(0.02)).max(0.0);
        if sole.lying {
            base.max(f.mid_y + f.body - f.ground)
        } else {
            base
        }
    };
    let Some(upright) = fit(sole, xf, ground, &dry) else { return Ok(*xf) };
    if draft > 0.0 {
        if let Some(w) = fit(sole, xf, ground, water).and_then(|f| f.water) {
            let afloat = w - upright.body - draft * (upright.height - upright.body);
            if afloat > upright.mid_y - drop_of(&upright) {
                let mut m = *xf;
                m[7] += afloat - upright.mid_y;
                return Ok(m);
            }
        }
    }
    let mut m = if sole.lying { conformed(sole, xf, ground).unwrap_or(*xf) } else { *xf };
    let Some(f) = fit(sole, &m, ground, &dry) else { return Ok(m) };
    if limit > 0.0 && f.base_gap - f.collar > limit * f.height.min(f.reach) {
        return Err("limit");
    }
    m[7] -= drop_of(&f);
    // seated, it must sit right by the measure the check uses; where it cannot (one end of a log over a bank), it goes
    if limit > 0.0 {
        if let Some((kind, _)) = fit(sole, &m, ground, water).and_then(|f| f.verdict()) {
            return Err(kind);
        }
    }
    Ok(m)
}

/// Heights of a surface (terrain, water) over x and z, with a grid so a lookup tests a handful of triangles. Where two levels
/// of a terrain overlap (a fine patch blended into a coarse one) the finest triangle wins, as in `pt ground-check`.
pub struct Heights {
    tris: Vec<[[f32; 3]; 3]>,
    lo: [f32; 2],
    cell: [f32; 2],
    n: usize,
    grid: Vec<Vec<u32>>,
}

impl Heights {
    pub fn new(g: &Geometry) -> Option<Heights> {
        let p = g.p();
        let tris: Vec<[[f32; 3]; 3]> = g.triangles().iter().map(|t| [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]).collect();
        let (lo, hi) = g.base_bounds()?;
        if tris.is_empty() {
            return None;
        }
        let n = ((tris.len() as f32).sqrt().ceil() as usize).clamp(1, 1024);
        let cell = [(hi[0] - lo[0]).max(1e-6) / n as f32, (hi[2] - lo[2]).max(1e-6) / n as f32];
        let mut h = Heights { tris: Vec::new(), lo: [lo[0], lo[2]], cell, n, grid: vec![vec![]; n * n] };
        for (i, t) in tris.iter().enumerate() {
            let (x0, x1) = (t[0][0].min(t[1][0]).min(t[2][0]), t[0][0].max(t[1][0]).max(t[2][0]));
            let (z0, z1) = (t[0][2].min(t[1][2]).min(t[2][2]), t[0][2].max(t[1][2]).max(t[2][2]));
            let ((c0, r0), (c1, r1)) = (h.cell_of(x0, z0), h.cell_of(x1, z1));
            for r in r0..=r1 {
                for c in c0..=c1 {
                    h.grid[r * n + c].push(i as u32);
                }
            }
        }
        h.tris = tris;
        Some(h)
    }

    fn cell_of(&self, x: f32, z: f32) -> (usize, usize) {
        let top = self.n as f32 - 1.0;
        (((x - self.lo[0]) / self.cell[0]).floor().clamp(0.0, top) as usize, ((z - self.lo[1]) / self.cell[1]).floor().clamp(0.0, top) as usize)
    }

    pub fn at(&self, x: f32, z: f32) -> Option<f32> {
        let (c, r) = self.cell_of(x, z);
        let mut best: Option<(f32, f32)> = None; // (triangle area, height)
        for &i in &self.grid[r * self.n + c] {
            let [a, b, cc] = self.tris[i as usize];
            let d = (b[2] - cc[2]) * (a[0] - cc[0]) + (cc[0] - b[0]) * (a[2] - cc[2]);
            if d.abs() < 1e-12 {
                continue;
            }
            let w0 = ((b[2] - cc[2]) * (x - cc[0]) + (cc[0] - b[0]) * (z - cc[2])) / d;
            let w1 = ((cc[2] - a[2]) * (x - cc[0]) + (a[0] - cc[0]) * (z - cc[2])) / d;
            let w2 = 1.0 - w0 - w1;
            let eps = -1e-4;
            if w0 >= eps && w1 >= eps && w2 >= eps {
                let y = w0 * a[1] + w1 * b[1] + w2 * cc[1];
                if best.is_none_or(|(area, _)| d.abs() < area) {
                    best = Some((d.abs(), y));
                }
            }
        }
        best.map(|(_, y)| y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boxes stacked on the y axis: (half width in x, half width in z, bottom, top).
    fn boxes(list: &[(f32, f32, f32, f32)]) -> Geometry {
        let mut pts = Vec::new();
        let mut polys = Vec::new();
        for &(hx, hz, y0, y1) in list {
            let o = pts.len() as u32;
            for y in [y0, y1] {
                pts.extend([[-hx, y, -hz], [hx, y, -hz], [hx, y, hz], [-hx, y, hz]]);
            }
            polys.extend([vec![o, o + 1, o + 2, o + 3], vec![o + 4, o + 5, o + 6, o + 7], vec![o, o + 1, o + 5, o + 4], vec![o + 1, o + 2, o + 6, o + 5], vec![o + 2, o + 3, o + 7, o + 6], vec![o + 3, o, o + 4, o + 7]]);
        }
        Geometry::from_polys(pts, &polys)
    }

    /// A tree as a planted thing: a root plate 1.6 m wide and 10 cm thick, a trunk, and a crown 6 m wide far above.
    fn tree() -> Geometry {
        boxes(&[(0.8, 0.8, 0.0, 0.1), (0.2, 0.2, 0.1, 14.0), (3.0, 3.0, 14.0, 20.0)])
    }

    /// A tree as a scan delivers it: it stands on a slab of soil 1.6 m wide and 0.4 m thick, and the trunk starts on top.
    fn tree_on_a_slab() -> Geometry {
        boxes(&[(0.8, 0.8, 0.0, 0.4), (0.2, 0.2, 0.4, 0.8), (0.2, 0.2, 0.8, 14.0), (3.0, 3.0, 14.0, 20.0)])
    }

    /// A log 4 m long and 0.4 m thick that stands 0.5 m up on one thin stub.
    fn log_on_a_stub() -> Geometry {
        boxes(&[(0.04, 0.04, 0.0, 0.5), (2.0, 0.2, 0.5, 0.9)])
    }

    const AT: Mat34 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    fn moved(y: f32) -> Mat34 {
        let mut m = AT;
        m[7] = y;
        m
    }
    fn dry(_: f32, _: f32) -> Option<f32> {
        None
    }

    #[test]
    fn the_base_of_a_tree_is_its_root_plate_not_its_crown() {
        let s = Sole::of(&tree()).unwrap();
        assert!(s.base_width >= 1.6 && s.base_width < 2.3, "base width {} (a square plate 1.6 m across, 2.26 m corner to corner)", s.base_width);
        assert!(!s.lying && s.collar == 0.0);
        assert!(s.base.iter().all(|q| q[1] <= 0.15 && q[0].abs() <= 0.81));
    }

    #[test]
    fn a_tree_on_a_slope_has_its_root_plate_in_the_air_until_it_is_seated() {
        // a 25 degree slope falling toward +x: the downhill rim of the plate hangs 0.37 m over it, and the bounding box cannot tell
        let slope = |x: f32, _: f32| Some(-0.466 * x);
        let s = Sole::of(&tree()).unwrap();
        let f = fit(&s, &AT, &slope, &dry).unwrap();
        assert!(f.base_in_air() && (f.base_gap - 0.37).abs() < 0.05, "gap {}", f.base_gap);
        let m = seat(&s, &AT, &slope, &dry, 0.5, 0.0, 0.0).unwrap();
        let f = fit(&s, &m, &slope, &dry).unwrap();
        assert!(!f.base_in_air() && f.base_gap <= 0.0, "gap after seating {}", f.base_gap);
        assert!(m[0] == 1.0 && m[5] == 1.0, "a tree stays upright");
        // at the lip of a 3 m bank the tree does not belong
        let bank = |x: f32, _: f32| Some(if x > 0.3 { -3.0 } else { 0.0 });
        assert!(seat(&s, &AT, &bank, &dry, 0.5, 0.0, 0.0).is_none());
    }

    #[test]
    fn the_slab_a_scanned_tree_stands_on_goes_into_the_ground() {
        // The owner's east frame: firs stood on a dark plate over the slope. The scan brings a wedge of soil 0.4 m thick; with
        // its underside on the ground its top stands out, on flat ground too, and the lowest 15 cm of the tree do not show it.
        let s = Sole::of(&tree_on_a_slab()).unwrap();
        assert!((s.collar - 0.5).abs() < 0.11, "the slab's top is found at {}", s.collar);
        let flat = |_: f32, _: f32| Some(0.0);
        let f = fit(&s, &AT, &flat, &dry).unwrap();
        assert!(f.base_in_air(), "standing on flat ground the slab is in the open: {}", f.base_gap);
        let slope = |x: f32, _: f32| Some(-0.466 * x);
        let m = seat(&s, &AT, &slope, &dry, 0.5, 0.0, 0.2).unwrap();
        // the top of the slab at its downhill rim (x = 0.8, ground -0.37) is 0.2 m under the ground
        let top = m[7] + 0.4;
        assert!(top < -0.37 - 0.19, "the slab's top is at {top}");
        assert!(fit(&s, &m, &slope, &dry).unwrap().verdict().is_none());
        // a young tree with a skirt of low branches has no slab: narrow at the bottom, wider above
        let skirt = boxes(&[(0.1, 0.1, 0.0, 0.3), (1.5, 1.5, 0.3, 0.5), (0.1, 0.1, 0.5, 9.0)]);
        assert_eq!(Sole::of(&skirt).unwrap().collar, 0.0);
    }

    #[test]
    fn a_log_standing_on_a_stub_is_propped_and_seating_lays_its_body_down() {
        let flat = |_: f32, _: f32| Some(0.0);
        let s = Sole::of(&log_on_a_stub()).unwrap();
        assert!(s.lying && (s.body - 0.5).abs() < 0.1, "body starts at {}", s.body);
        let f = fit(&s, &AT, &flat, &dry).unwrap();
        assert!(!f.base_in_air(), "the stub touches, which is all a bounding box sees");
        assert!(f.propped().is_some(), "but the log is in the air");
        let m = seat(&s, &AT, &flat, &dry, 0.0, 0.0, 0.0).unwrap();
        assert!(fit(&s, &m, &flat, &dry).unwrap().propped().is_none());
    }

    #[test]
    fn a_lying_object_is_turned_to_lie_along_the_slope() {
        // a slope of 20 degrees with a lump at the object's middle: the normal at that one point says "flat", the footprint does not
        let slope = |x: f32, z: f32| Some(0.364 * x + 0.3 * (1.0 - (x * x + z * z).min(1.0)));
        let s = Sole::of(&boxes(&[(2.0, 0.2, 0.0, 0.4)])).unwrap();
        let m = seat(&s, &AT, &slope, &dry, 0.0, 0.0, 0.0).unwrap();
        // the log's long axis (local x) rises with the slope: about 0.34 per unit length
        assert!((m[4] - 0.34).abs() < 0.06, "the log leans {} along its length", m[4]);
        let (a, b) = (xform_point(&m, [-2.0, 0.0, 0.0]), xform_point(&m, [2.0, 0.0, 0.0]));
        assert!((a[1] - 0.364 * a[0]) < 0.05 && (b[1] - 0.364 * b[0]) < 0.05, "neither end hangs: {} {}", a[1] - 0.364 * a[0], b[1] - 0.364 * b[0]);
    }

    #[test]
    fn wood_floats_where_the_water_is_deep_and_rests_on_the_bed_where_it_is_shallow() {
        let s = Sole::of(&log_on_a_stub()).unwrap();
        let level = |_: f32, _: f32| Some(0.0);
        // deep water: the bed is 2 m down. Left at the surface on its stub the log hovers over the water.
        let deep = |_: f32, _: f32| Some(-2.0);
        let hover = fit(&s, &AT, &deep, &level).unwrap();
        assert!(hover.propped().is_some(), "a log on its stub over the water hovers");
        let m = seat(&s, &moved(-2.0), &deep, &level, 0.0, 0.55, 0.0).unwrap();
        let f = fit(&s, &m, &deep, &level).unwrap();
        let top = f.y_lo + f.height;
        assert!(f.propped().is_none() && top > 0.05 && top < 0.4, "the log lies in the water line, its top {top} m above it");
        // shallow water: 10 cm over the bed. The log rests on the bed and stands out of the water.
        let shallow = |_: f32, _: f32| Some(-0.1);
        let m = seat(&s, &moved(-0.1), &shallow, &level, 0.0, 0.55, 0.0).unwrap();
        let f = fit(&s, &m, &shallow, &level).unwrap();
        assert!((f.mid_y + f.body - f.ground).abs() < 0.03, "the body lies on the bed");
    }

    #[test]
    fn heights_of_a_tilted_plane() {
        let g = Geometry::from_polys(vec![[-5.0, -5.0, -5.0], [5.0, 5.0, -5.0], [5.0, 5.0, 5.0], [-5.0, -5.0, 5.0]], &[vec![0, 1, 2, 3]]);
        let h = Heights::new(&g).unwrap();
        assert!((h.at(2.0, 1.0).unwrap() - 2.0).abs() < 1e-4);
    }
}
