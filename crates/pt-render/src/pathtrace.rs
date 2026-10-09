//! CPU path tracer: BVH over triangles, diffuse + glossy-coat surfaces, a sun with next-event
//! estimation, a sky dome and an optional ground plane. Fully deterministic: every pixel
//! sample draws from a counter-based RNG keyed by (seed, pixel, sample, bounce), so images
//! are bit-identical for any thread count.

use crate::{Image, Mode};
use pt_core::math;
use pt_core::rng::rand4;
use pt_core::{Column, Geometry};
use rayon::prelude::*;

#[derive(Clone, Debug)]
pub struct PtOpts {
    pub width: usize,
    pub height: usize,
    pub spp: usize,
    pub max_depth: usize,
    pub azimuth_deg: f32,
    pub elevation_deg: f32,
    pub fov_deg: f32,
    /// Direction the sun light travels *from* (toward the scene), normalized internally.
    pub sun_dir: [f32; 3],
    pub sun_intensity: f32,
    pub sky_top: [f32; 3],
    pub sky_horizon: [f32; 3],
    pub sky_intensity: f32,
    pub ground: bool,
    pub ground_albedo: [f32; 3],
    pub exposure: f32,
    /// 0 = mirror-ish coat, 1 = pure diffuse.
    pub roughness: f32,
    pub seed: i64,
    pub mode: Mode,
    pub max_instances: usize,
}

impl Default for PtOpts {
    fn default() -> Self {
        PtOpts {
            width: 640,
            height: 480,
            spp: 32,
            max_depth: 4,
            azimuth_deg: 35.0,
            elevation_deg: 22.0,
            fov_deg: 34.0,
            sun_dir: [-0.5, 0.8, 0.35],
            sun_intensity: 3.2,
            sky_top: [0.35, 0.55, 0.95],
            sky_horizon: [0.85, 0.9, 1.0],
            sky_intensity: 0.75,
            ground: true,
            ground_albedo: [0.42, 0.42, 0.40],
            exposure: 0.8,
            roughness: 0.65,
            seed: 1,
            mode: Mode::Shaded,
            max_instances: 200_000,
        }
    }
}

#[derive(Clone, Copy)]
struct Tri {
    p: [[f32; 3]; 3],
    n: [[f32; 3]; 3],
    c: [[f32; 3]; 3],
}

fn centroid(t: &Tri) -> [f32; 3] {
    [0, 1, 2].map(|a| (t.p[0][a] + t.p[1][a] + t.p[2][a]) / 3.0)
}

fn tri_bounds(t: &Tri) -> ([f32; 3], [f32; 3]) {
    let mut lo = t.p[0];
    let mut hi = t.p[0];
    for p in &t.p[1..] {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    (lo, hi)
}

// BVH with explicit child indices (deterministic median split on the widest centroid axis).
struct Flat {
    nodes: Vec<FNode>,
    tris: Vec<Tri>,
}

#[derive(Clone, Copy)]
struct FNode {
    lo: [f32; 3],
    hi: [f32; 3],
    a: u32, // leaf: first tri; interior: left child
    b: u32, // leaf: count;     interior: right child
    leaf: bool,
}

impl Flat {
    fn build(mut tris: Vec<Tri>) -> Flat {
        let mut nodes = Vec::with_capacity(tris.len() / 2 + 2);
        let n = tris.len();
        if n > 0 {
            Self::rec(&mut nodes, &mut tris, 0, n);
        }
        Flat { nodes, tris }
    }

    fn rec(nodes: &mut Vec<FNode>, tris: &mut [Tri], start: usize, end: usize) -> u32 {
        let (mut lo, mut hi) = tri_bounds(&tris[start]);
        let (mut clo, mut chi) = (centroid(&tris[start]), centroid(&tris[start]));
        for t in &tris[start..end] {
            let (l, h) = tri_bounds(t);
            let c = centroid(t);
            for a in 0..3 {
                lo[a] = lo[a].min(l[a]);
                hi[a] = hi[a].max(h[a]);
                clo[a] = clo[a].min(c[a]);
                chi[a] = chi[a].max(c[a]);
            }
        }
        let idx = nodes.len() as u32;
        nodes.push(FNode { lo, hi, a: start as u32, b: (end - start) as u32, leaf: true });
        let ext = [chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2]];
        let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] { 0 } else if ext[1] >= ext[2] { 1 } else { 2 };
        if end - start <= 4 || ext[axis] <= 0.0 {
            return idx;
        }
        tris[start..end].sort_by(|a, b| centroid(a)[axis].partial_cmp(&centroid(b)[axis]).unwrap_or(std::cmp::Ordering::Equal));
        let mid = (start + end) / 2;
        let l = Self::rec(nodes, tris, start, mid);
        let r = Self::rec(nodes, tris, mid, end);
        let n = &mut nodes[idx as usize];
        n.leaf = false;
        n.a = l;
        n.b = r;
        idx
    }

    #[inline]
    fn hit_box(n: &FNode, o: [f32; 3], inv: [f32; 3], tmax: f32) -> bool {
        let (mut t0, mut t1) = (0.0f32, tmax);
        for a in 0..3 {
            let (mut a0, mut a1) = ((n.lo[a] - o[a]) * inv[a], (n.hi[a] - o[a]) * inv[a]);
            if a0 > a1 {
                std::mem::swap(&mut a0, &mut a1);
            }
            t0 = t0.max(a0);
            t1 = t1.min(a1);
            if t0 > t1 {
                return false;
            }
        }
        true
    }

    /// Closest hit: (t, triangle index, u, v).
    fn intersect(&self, o: [f32; 3], d: [f32; 3], tmax: f32, any: bool) -> Option<(f32, usize, f32, f32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = d.map(|x| if x.abs() < 1e-12 { 1e12f32.copysign(x) } else { 1.0 / x });
        let mut best: Option<(f32, usize, f32, f32)> = None;
        let mut tclip = tmax;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let n = &self.nodes[stack[sp] as usize];
            if !Self::hit_box(n, o, inv, tclip) {
                continue;
            }
            if n.leaf {
                for i in n.a..n.a + n.b {
                    let t = &self.tris[i as usize];
                    if let Some((tt, u, v)) = moller(o, d, t) {
                        if tt > 1e-4 && tt < tclip {
                            tclip = tt;
                            best = Some((tt, i as usize, u, v));
                            if any {
                                return best;
                            }
                        }
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = n.a;
                stack[sp + 1] = n.b;
                sp += 2;
            }
        }
        best
    }
}

#[inline]
fn moller(o: [f32; 3], d: [f32; 3], t: &Tri) -> Option<(f32, f32, f32)> {
    let e1 = math::sub(t.p[1], t.p[0]);
    let e2 = math::sub(t.p[2], t.p[0]);
    let p = math::cross(d, e2);
    let det = e1[0] * p[0] + e1[1] * p[1] + e1[2] * p[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = math::sub(o, t.p[0]);
    let u = (s[0] * p[0] + s[1] * p[1] + s[2] * p[2]) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = math::cross(s, e1);
    let v = (d[0] * q[0] + d[1] * q[1] + d[2] * q[2]) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let tt = (e2[0] * q[0] + e2[1] * q[1] + e2[2] * q[2]) * inv;
    Some((tt, u, v))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn mulv(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// Orthonormal basis around unit `n`.
fn basis(n: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let h = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let t = math::normalize(math::cross(h, n));
    (t, math::cross(n, t))
}

fn cosine_dir(n: [f32; 3], u1: f32, u2: f32) -> [f32; 3] {
    let r = u1.sqrt();
    let (s, c) = math::sin_cos(2.0 * math::PI * u2);
    let (t, b) = basis(n);
    let z = (1.0 - u1).max(0.0).sqrt();
    math::normalize(add(add(scale(t, r * c), scale(b, r * s)), scale(n, z)))
}

fn sky(opts: &PtOpts, d: [f32; 3]) -> [f32; 3] {
    let t = d[1].clamp(0.0, 1.0);
    let t = t.sqrt();
    let c = [0, 1, 2].map(|a| opts.sky_horizon[a] + (opts.sky_top[a] - opts.sky_horizon[a]) * t);
    scale(c, opts.sky_intensity)
}

/// Local-space shading triangles of a geometry's own surface.
fn bake_local(geo: &Geometry, o: &PtOpts) -> Vec<Tri> {
    let base = [0.58, 0.60, 0.64];
    let p = geo.p();
    let n = geo.point.get("N").and_then(Column::as_v3);
    let cd = match &o.mode {
        Mode::Attr(name) => geo.point.get(name).and_then(Column::as_v3),
        _ => geo.point.get("Cd").and_then(Column::as_v3),
    };
    geo.triangles()
        .into_iter()
        .map(|t| {
            let pos = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
            let fnorm = math::normalize(math::cross(math::sub(pos[1], pos[0]), math::sub(pos[2], pos[0])));
            let nn = match n {
                Some(nn) => [0, 1, 2].map(|k| nn[t[k] as usize]),
                None => [fnorm; 3],
            };
            let cc = [0, 1, 2].map(|k| cd.map(|c| c[t[k] as usize]).unwrap_or(base));
            Tri { p: pos, n: nn, c: cc }
        })
        .collect()
}

fn place_tri(t: &Tri, m: &math::Mat34, tint: Option<[f32; 3]>) -> Tri {
    let rot = |v: [f32; 3]| math::normalize([m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[4] * v[0] + m[5] * v[1] + m[6] * v[2], m[8] * v[0] + m[9] * v[1] + m[10] * v[2]]);
    Tri {
        p: [math::xform_point(m, t.p[0]), math::xform_point(m, t.p[1]), math::xform_point(m, t.p[2])],
        n: [rot(t.n[0]), rot(t.n[1]), rot(t.n[2])],
        c: tint.map_or(t.c, |c| [c; 3]),
    }
}

const MAX_TRIS: usize = 12_000_000;

/// Collect shading triangles; nested instances are expanded recursively and only the top
/// level is subsampled (by a fixed stride) beyond `max_instances`.
fn gather(g: &Geometry, o: &PtOpts) -> Vec<Tri> {
    let mut out = Vec::new();
    gather_rec(g, None, None, o, &mut out, 0);
    out
}

fn gather_rec(g: &Geometry, xf: Option<math::Mat34>, tint: Option<[f32; 3]>, o: &PtOpts, out: &mut Vec<Tri>, depth: usize) {
    if out.len() >= MAX_TRIS || depth > 8 {
        return;
    }
    let m0 = xf.unwrap_or(math::IDENTITY);
    out.extend(bake_local(g, o).iter().map(|t| place_tri(t, &m0, tint)));
    let Some(inst) = &g.instances else { return };
    let stride = if depth == 0 { (inst.len() / o.max_instances.max(1)).max(1) } else { 1 };
    let baked: Vec<Option<Vec<Tri>>> = inst.protos.iter().map(|p| if p.instances.is_none() { Some(bake_local(p, o)) } else { None }).collect();
    let cd_inst = inst.attrs.get("Cd").and_then(Column::as_v3);
    for i in (0..inst.len()).step_by(stride) {
        if out.len() >= MAX_TRIS {
            break;
        }
        let pid = inst.proto_id[i] as usize;
        let m = match xf {
            Some(x) => math::mul34(&x, &inst.xf[i]),
            None => inst.xf[i],
        };
        let t = cd_inst.map(|c| c[i]).or(tint);
        match &baked[pid] {
            Some(b) => out.extend(b.iter().map(|tr| place_tri(tr, &m, t))),
            None => gather_rec(&inst.protos[pid], Some(m), t, o, out, depth + 1),
        }
    }
}

pub struct PtStats {
    pub triangles: usize,
    pub bvh_nodes: usize,
    pub rays: u64,
}

pub fn pathtrace(g: &Geometry, o: &PtOpts) -> (Image, PtStats) {
    let tris = gather(g, o);
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for t in &tris {
        for p in &t.p {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
    }
    if tris.is_empty() {
        lo = [-1.0; 3];
        hi = [1.0; 3];
    }
    let ext = math::sub(hi, lo);
    let radius = (dot(ext, ext).sqrt() * 0.5).max(1e-3);
    let mut all = tris;
    let ground_y = lo[1];
    if o.ground {
        // Two huge triangles forming a ground plane just below the subject.
        let r = radius * 40.0;
        let c = [(lo[0] + hi[0]) * 0.5, ground_y - 1e-3, (lo[2] + hi[2]) * 0.5];
        let up = [0.0, 1.0, 0.0];
        let (a, b, cc, d) = ([c[0] - r, c[1], c[2] - r], [c[0] - r, c[1], c[2] + r], [c[0] + r, c[1], c[2] + r], [c[0] + r, c[1], c[2] - r]);
        for p in [[a, b, cc], [a, cc, d]] {
            all.push(Tri { p, n: [up; 3], c: [o.ground_albedo; 3] });
        }
    }
    let bvh = Flat::build(all);
    let ntris = bvh.tris.len();

    // Camera (same framing rule as the rasterizer: look at the subject's center).
    let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let (sa, ca) = math::sin_cos(math::to_radians(o.azimuth_deg));
    let (se, ce) = math::sin_cos(math::to_radians(o.elevation_deg));
    let dir = [ce * sa, se, ce * ca];
    let half = math::to_radians(o.fov_deg) * 0.5;
    let (sh, ch) = math::sin_cos(half);
    let dist = radius / sh * 0.98;
    let eye = [center[0] + dir[0] * dist, center[1] + dir[1] * dist, center[2] + dir[2] * dist];
    let fwd = scale(dir, -1.0);
    let right = math::normalize(math::cross(fwd, [0.0, 1.0, 0.0]));
    let up = math::cross(right, fwd);
    let focal = ch / sh;
    let aspect = o.width as f32 / o.height as f32;
    let sun = math::normalize(o.sun_dir);
    let (w, h) = (o.width, o.height);
    let rays = std::sync::atomic::AtomicU64::new(0);

    let rows: Vec<Vec<[f32; 3]>> = (0..h)
        .into_par_iter()
        .map(|y| {
            let mut row = Vec::with_capacity(w);
            let mut local_rays = 0u64;
            for x in 0..w {
                let pix = (y * w + x) as u64;
                let mut acc = [0.0f32; 3];
                for s in 0..o.spp {
                    let jit = rand4(o.seed, 1, pix * 4096 + s as u64);
                    let (px, py) = ((x as f32 + jit[0]) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + jit[1]) / h as f32 * 2.0);
                    let d = math::normalize(add(add(scale(right, px * aspect / focal), scale(up, py / focal)), fwd));
                    let (mut ro, mut rd) = (eye, d);
                    let mut thr = [1.0f32; 3];
                    for bounce in 0..o.max_depth {
                        local_rays += 1;
                        let Some((t, ti, u, v)) = bvh.intersect(ro, rd, 1e9, false) else {
                            acc = add(acc, mulv(thr, sky(o, rd)));
                            break;
                        };
                        let tri = &bvh.tris[ti];
                        let w0 = 1.0 - u - v;
                        let mut n = math::normalize([0, 1, 2].map(|a| tri.n[0][a] * w0 + tri.n[1][a] * u + tri.n[2][a] * v));
                        let albedo = [0, 1, 2].map(|a| tri.c[0][a] * w0 + tri.c[1][a] * u + tri.c[2][a] * v);
                        if dot(n, rd) > 0.0 {
                            n = scale(n, -1.0);
                        }
                        let hp = add(ro, scale(rd, t));
                        let origin = add(hp, scale(n, 2e-3));
                        let r = rand4(o.seed, 2, pix * 4096 * 8 + s as u64 * 8 + bounce as u64);
                        // Glossy coat probability (Schlick) over a diffuse base.
                        let cosv = (-dot(rd, n)).clamp(0.0, 1.0);
                        let f0 = 0.04f32;
                        let one_minus = 1.0 - cosv;
                        let fres = f0 + (1.0 - f0) * one_minus * one_minus * one_minus * one_minus * one_minus;
                        let spec_p = fres * (1.0 - o.roughness * 0.7);
                        // Next-event estimation toward the sun (diffuse part only).
                        let ndl = dot(n, sun).max(0.0);
                        if ndl > 0.0 {
                            local_rays += 1;
                            if bvh.intersect(origin, sun, 1e9, true).is_none() {
                                let k = ndl / math::PI * o.sun_intensity * (1.0 - spec_p);
                                acc = add(acc, mulv(thr, scale(mulv(albedo, [1.0, 0.96, 0.9]), k)));
                            }
                        }
                        if r[3] < spec_p {
                            // Rough mirror bounce.
                            let refl = math::normalize(math::sub(rd, scale(n, 2.0 * dot(rd, n))));
                            let rnd = cosine_dir(refl, r[0], r[1]);
                            rd = math::normalize(add(scale(refl, 1.0 - o.roughness), scale(rnd, o.roughness)));
                            if dot(rd, n) < 0.0 {
                                rd = refl;
                            }
                            thr = scale(thr, 1.0 / spec_p.max(1e-3) * fres);
                        } else {
                            thr = mulv(thr, albedo);
                            rd = cosine_dir(n, r[0], r[1]);
                        }
                        ro = origin;
                        if bounce >= 2 {
                            let q = thr[0].max(thr[1]).max(thr[2]).clamp(0.05, 0.95);
                            let rr = rand4(o.seed, 3, pix * 4096 * 8 + s as u64 * 8 + bounce as u64);
                            if rr[0] > q {
                                break;
                            }
                            thr = scale(thr, 1.0 / q);
                        }
                    }
                }
                row.push(scale(acc, o.exposure / o.spp.max(1) as f32));
            }
            rays.fetch_add(local_rays, std::sync::atomic::Ordering::Relaxed);
            row
        })
        .collect();

    // Filmic tone map (Narkowicz ACES fit) + display gamma.
    let mut rgb = Vec::with_capacity(w * h * 3);
    for row in rows {
        for c in row {
            for v in c {
                let v = v.max(0.0);
                let mapped = (v * (2.51 * v + 0.03)) / (v * (2.43 * v + 0.59) + 0.14);
                let m = mapped.clamp(0.0, 1.0);
                rgb.push(((m * 0.35 + m.sqrt() * 0.65) * 255.0 + 0.5) as u8);
            }
        }
    }
    (Image { w, h, rgb }, PtStats { triangles: ntris, bvh_nodes: bvh.nodes.len(), rays: rays.load(std::sync::atomic::Ordering::Relaxed) })
}

/// Render `frames` evenly spaced azimuths and tile them into one sheet (turntable contact sheet).
pub fn turntable_sheet(g: &Geometry, o: &PtOpts, frames: usize) -> Image {
    let frames = frames.clamp(1, 36);
    let cols = (frames as f64).sqrt().ceil() as usize;
    let rows = frames.div_ceil(cols);
    let mut sheet = Image { w: cols * o.width, h: rows * o.height, rgb: vec![0; cols * o.width * rows * o.height * 3] };
    for f in 0..frames {
        let mut fo = o.clone();
        fo.azimuth_deg = o.azimuth_deg + 360.0 * f as f32 / frames as f32;
        let (img, _) = pathtrace(g, &fo);
        let (ox, oy) = ((f % cols) * o.width, (f / cols) * o.height);
        for y in 0..o.height {
            let dst = ((oy + y) * sheet.w + ox) * 3;
            sheet.rgb[dst..dst + o.width * 3].copy_from_slice(&img.rgb[y * o.width * 3..(y + 1) * o.width * 3]);
        }
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere_on_ground() -> Geometry {
        // Build a UV sphere inline (pt-render has no dependency on pt-sop).
        let (rows, cols) = (24usize, 48usize);
        let mut pts = vec![[0.0, 1.0, 0.0]];
        for j in 1..rows {
            let (st, ct) = math::sin_cos(math::PI * j as f32 / rows as f32);
            for i in 0..cols {
                let (sp, cp) = math::sin_cos(2.0 * math::PI * i as f32 / cols as f32);
                pts.push([st * cp, ct, st * sp]);
            }
        }
        pts.push([0.0, -1.0, 0.0]);
        let ring = |j: usize, i: usize| (1 + (j - 1) * cols + i % cols) as u32;
        let mut polys: Vec<Vec<u32>> = vec![];
        for i in 0..cols {
            polys.push(vec![0, ring(1, i + 1), ring(1, i)]);
        }
        for j in 1..rows - 1 {
            for i in 0..cols {
                polys.push(vec![ring(j, i), ring(j, i + 1), ring(j + 1, i + 1), ring(j + 1, i)]);
            }
        }
        let b = pts.len() as u32 - 1;
        for i in 0..cols {
            polys.push(vec![b, ring(rows - 1, i), ring(rows - 1, i + 1)]);
        }
        let mut g = Geometry::from_polys(pts.clone(), &polys);
        g.point.set("N", Column::from_v3(pts));
        g
    }

    fn luma(img: &Image, x: usize, y: usize) -> f32 {
        let i = (y * img.w + x) * 3;
        0.299 * img.rgb[i] as f32 + 0.587 * img.rgb[i + 1] as f32 + 0.114 * img.rgb[i + 2] as f32
    }

    #[test]
    fn lit_sphere_has_shadowed_ground_and_is_deterministic() {
        let g = sphere_on_ground();
        let o = PtOpts { width: 96, height: 72, spp: 16, ..Default::default() };
        let (a, st) = pathtrace(&g, &o);
        assert!(st.rays > 96 * 72 * 16);
        // Lit side of the sphere is brighter than its shaded side.
        let (mut bright, mut dark) = (0.0f32, f32::INFINITY);
        for y in 15..60 {
            for x in 25..75 {
                let l = luma(&a, x, y);
                bright = bright.max(l);
                dark = dark.min(l);
            }
        }
        assert!(bright > dark + 40.0, "shading contrast {bright} vs {dark}");
        let pool = |threads: usize| rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| pathtrace(&g, &o).0.rgb);
        assert_eq!(pool(1), pool(5), "bit-identical across thread counts");
        assert_eq!(a.rgb, pathtrace(&g, &o).0.rgb);
    }

    #[test]
    fn more_samples_converge_toward_the_reference() {
        let g = sphere_on_ground();
        let base = PtOpts { width: 48, height: 36, ..Default::default() };
        let reference = pathtrace(&g, &PtOpts { spp: 256, seed: 99, ..base.clone() }).0;
        let err = |spp: usize| -> f64 {
            let img = pathtrace(&g, &PtOpts { spp, seed: 3, ..base.clone() }).0;
            img.rgb.iter().zip(&reference.rgb).map(|(a, b)| (*a as f64 - *b as f64).powi(2)).sum::<f64>() / img.rgb.len() as f64
        };
        let (e4, e64) = (err(4), err(64));
        assert!(e64 < e4 * 0.6, "mse {e4} (4 spp) -> {e64} (64 spp)");
    }

    #[test]
    fn bvh_matches_brute_force() {
        let g = sphere_on_ground();
        let tris = gather(&g, &PtOpts::default());
        let bvh = Flat::build(tris.clone());
        for k in 0..200u64 {
            let r = rand4(5, 1, k);
            let o = [(r[0] - 0.5) * 6.0, (r[1] - 0.5) * 6.0, 4.0];
            let d = math::normalize([(r[2] - 0.5) * 0.6, (r[3] - 0.5) * 0.6, -1.0]);
            let fast = bvh.intersect(o, d, 1e9, false).map(|h| h.0);
            let slow = tris.iter().filter_map(|t| moller(o, d, t)).map(|h| h.0).filter(|t| *t > 1e-4).fold(None, |a: Option<f32>, t| Some(a.map_or(t, |x| x.min(t))));
            assert_eq!(fast.is_some(), slow.is_some(), "ray {k}");
            if let (Some(a), Some(b)) = (fast, slow) {
                assert!((a - b).abs() < 1e-4, "ray {k}: {a} vs {b}");
            }
        }
    }
}
