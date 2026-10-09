//! Implicit fields: signed distance volumes ("sdf", negative = inside) and the robust
//! mesh booleans built on them (mesh -> SDF -> combine -> mesh). SDF booleans cannot fail on
//! bad input topology the way polygon booleans can.

use crate::{ps, sch};
use pt_core::math;
use pt_core::noise::fbm;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params, Volume};
use rayon::prelude::*;
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

const MAX_VOXELS: usize = 48_000_000;

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("sdf_shape", "Analytic signed distance volume 'sdf' for sphere | box | torus | capsule | cylinder (axis Y).", 0, D::Smooth, G::Whole, vec![
            ps("shape", json!("sphere"), "sphere | box | torus | capsule | cylinder."),
            ps("radius", json!(0.5), "Radius (sphere, torus ring, capsule, cylinder), meters."),
            ps("size", json!([1.0, 1.0, 1.0]), "Box edge lengths."),
            ps("minor_radius", json!(0.15), "Torus tube radius."),
            ps("height", json!(1.0), "Capsule/cylinder height (capsule: between cap centers)."),
            ps("center", json!([0.0, 0.0, 0.0]), "Center."),
            ps("voxel", json!(0.04), "Voxel size in meters."),
            ps("padding", json!(0.2), "Empty margin around the shape (m)."),
        ]),
        sch("sdf_from_mesh", "Convert a closed polygon mesh to a signed distance volume 'sdf' (exact narrow-band distance, ray-parity sign).", 1, D::None, G::Whole, vec![
            ps("voxel", json!(0.05), "Voxel size in meters."),
            ps("padding", json!(0.3), "Empty margin around the mesh (m)."),
            ps("band", json!(3), "Narrow band half-width in voxels (distance beyond it is clamped)."),
        ]),
        sch("sdf_combine", "Boolean of two SDFs: union | intersect | subtract (A - B) | smooth_union | smooth_subtract, with smoothing radius k.", 2, D::Piecewise, G::Whole, vec![
            ps("op", json!("union"), "union | intersect | subtract | smooth_union | smooth_subtract."),
            ps("k", json!(0.1), "Smoothing radius in meters (smooth ops)."),
            ps("voxel", json!(0.0), "Output voxel size (0 = the finer of the two inputs)."),
        ]),
        sch("sdf_offset", "Grow (positive) or shrink (negative) the shape by a distance.", 1, D::Smooth, G::PerPoint, vec![ps("distance", json!(0.05), "Offset in meters.")]),
        sch("sdf_smooth", "Blur the SDF (rounds sharp features).", 1, D::Smooth, G::Whole, vec![ps("iterations", json!(2), "Box-blur iterations.")]),
        sch("sdf_displace", "Add fractal noise to the SDF, roughening the surface.", 1, D::Piecewise, G::PerPoint, vec![
            ps("amp", json!(0.05), "Amplitude in meters."),
            ps("freq", json!(3.0), "Frequency (1/m)."),
            ps("octaves", json!(3), "Octaves."),
            ps("seed", json!(1), "Seed."),
        ]),
        sch("sdf_to_mesh", "Extract the zero isosurface of 'sdf' with smooth normals `N`. method auto = surface nets (quads), falling back to marching tetrahedra (triangles, always manifold) if nets produces non-manifold edges.", 1, D::None, G::Whole, vec![ps("iso", json!(0.0), "Isovalue (offsets the surface)."), ps("method", json!("auto"), "auto | nets | tets.")]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "sdf_shape" => shape(p),
        "sdf_from_mesh" => inp(0).and_then(|g| from_mesh(g, p)),
        "sdf_combine" => inp(0).and_then(|a| inp(1).and_then(|b| combine(a, b, p))),
        "sdf_offset" => inp(0).and_then(|g| map_sdf(g, "sdf_offset", |_, v| v - p.f("distance"))),
        "sdf_smooth" => inp(0).and_then(|g| smooth(g, p)),
        "sdf_displace" => inp(0).and_then(|g| displace(g, p)),
        "sdf_to_mesh" => inp(0).and_then(|g| to_mesh(g, p)),
        _ => return None,
    })
}

fn sdf_of<'a>(g: &'a Geometry, op: &str) -> Result<&'a Arc<Volume>, String> {
    g.volumes.get("sdf").ok_or_else(|| format!("{op}: input has no 'sdf' volume (make one with sdf_shape or sdf_from_mesh)"))
}

fn with_sdf(mut g: Geometry, v: Volume) -> Geometry {
    g.volumes.insert("sdf".into(), Arc::new(v));
    g
}

fn alloc(lo: [f32; 3], hi: [f32; 3], voxel: f32, op: &str) -> Result<Volume, String> {
    let voxel = voxel.max(1e-4);
    let dims = [0, 1, 2].map(|a| ((hi[a] - lo[a]) / voxel).ceil().max(1.0) as usize + 1);
    let n = dims[0] as u128 * dims[1] as u128 * dims[2] as u128;
    if n > MAX_VOXELS as u128 {
        return Err(format!("{op}: {}x{}x{} = {n} voxels exceeds the limit of {MAX_VOXELS}; increase `voxel` (currently {voxel} m)", dims[0], dims[1], dims[2]));
    }
    Ok(Volume::new(dims, lo, voxel, 0.0))
}

/// Fill a volume in parallel (one z-slab per task); `f` gets the voxel position.
fn fill(v: &mut Volume, f: impl Fn([f32; 3]) -> f32 + Sync) {
    let (nx, ny, voxel, origin) = (v.dims[0], v.dims[1], v.voxel, v.origin);
    let slab = nx * ny;
    Arc::make_mut(&mut v.data).par_chunks_mut(slab).enumerate().for_each(|(k, chunk)| {
        for j in 0..ny {
            for i in 0..nx {
                chunk[j * nx + i] = f([origin[0] + i as f32 * voxel, origin[1] + j as f32 * voxel, origin[2] + k as f32 * voxel]);
            }
        }
    });
}

fn len3(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn shape(p: &Params) -> Res {
    let (r, c, sz) = (p.f("radius").max(1e-4), p.v3("center"), p.v3("size"));
    let (mr, h) = (p.f("minor_radius").max(1e-4), p.f("height").max(1e-4));
    let pad = p.f("padding").max(0.0);
    let kind = p.s("shape");
    let half: [f32; 3] = match kind.as_str() {
        "sphere" => [r; 3],
        "box" => [sz[0] * 0.5, sz[1] * 0.5, sz[2] * 0.5],
        "torus" => [r + mr, mr, r + mr],
        "capsule" => [r, h * 0.5 + r, r],
        "cylinder" => [r, h * 0.5, r],
        o => return Err(format!("sdf_shape: unknown shape '{o}' (sphere | box | torus | capsule | cylinder)")),
    };
    let lo = [0, 1, 2].map(|a| c[a] - half[a] - pad);
    let hi = [0, 1, 2].map(|a| c[a] + half[a] + pad);
    let mut v = alloc(lo, hi, p.f("voxel"), "sdf_shape")?;
    fill(&mut v, |q| {
        let d = [q[0] - c[0], q[1] - c[1], q[2] - c[2]];
        match kind.as_str() {
            "sphere" => len3(d) - r,
            "box" => {
                let qd = [d[0].abs() - sz[0] * 0.5, d[1].abs() - sz[1] * 0.5, d[2].abs() - sz[2] * 0.5];
                len3([qd[0].max(0.0), qd[1].max(0.0), qd[2].max(0.0)]) + qd[0].max(qd[1]).max(qd[2]).min(0.0)
            }
            "torus" => {
                let ring = (d[0] * d[0] + d[2] * d[2]).sqrt() - r;
                (ring * ring + d[1] * d[1]).sqrt() - mr
            }
            "capsule" => {
                let y = d[1].clamp(-h * 0.5, h * 0.5);
                len3([d[0], d[1] - y, d[2]]) - r
            }
            _ => {
                let (a, b) = ((d[0] * d[0] + d[2] * d[2]).sqrt() - r, d[1].abs() - h * 0.5);
                a.max(b).min(0.0) + (a.max(0.0) * a.max(0.0) + b.max(0.0) * b.max(0.0)).sqrt()
            }
        }
    });
    Ok(with_sdf(Geometry::new(), v))
}

/// Closest point on triangle abc to p (Ericson, Real-Time Collision Detection).
fn closest_on_tri(p: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let dot = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (ab, ac, ap) = (math::sub(b, a), math::sub(c, a), math::sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = math::sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [a[0] + ab[0] * v, a[1] + ab[1] * v, a[2] + ab[2] * v];
    }
    let cp = math::sub(p, c);
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

fn from_mesh(src: &Geometry, p: &Params) -> Res {
    let g = if src.instances.is_some() { src.realize() } else { src.clone() };
    let tris = g.triangles();
    if tris.is_empty() {
        return Err("sdf_from_mesh: input has no polygons".into());
    }
    let (lo0, hi0) = g.base_bounds().unwrap();
    let (voxel, pad, band) = (p.f("voxel").max(1e-4), p.f("padding").max(0.0), p.i("band").clamp(1, 16) as f32);
    let lo = [0, 1, 2].map(|a| lo0[a] - pad - voxel);
    let hi = [0, 1, 2].map(|a| hi0[a] + pad + voxel);
    let mut vol = alloc(lo, hi, voxel, "sdf_from_mesh")?;
    let [nx, ny, nz] = vol.dims;
    let band_d = band * voxel;
    let pts = g.p();
    let mut dist = vec![band_d; nx * ny * nz];
    // Unsigned narrow-band distance: each triangle updates voxels near it.
    for t in &tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let mut rng = [(0usize, 0usize); 3];
        for ax in 0..3 {
            let mn = a[ax].min(b[ax]).min(c[ax]) - band_d;
            let mx = a[ax].max(b[ax]).max(c[ax]) + band_d;
            let n = [nx, ny, nz][ax];
            rng[ax] = (((mn - lo[ax]) / voxel).floor().max(0.0) as usize, (((mx - lo[ax]) / voxel).ceil() as usize).min(n - 1));
        }
        for k in rng[2].0..=rng[2].1 {
            for j in rng[1].0..=rng[1].1 {
                for i in rng[0].0..=rng[0].1 {
                    let q = [lo[0] + i as f32 * voxel, lo[1] + j as f32 * voxel, lo[2] + k as f32 * voxel];
                    let cp = closest_on_tri(q, a, b, c);
                    let d = len3(math::sub(q, cp));
                    let idx = (k * ny + j) * nx + i;
                    if d < dist[idx] {
                        dist[idx] = d;
                    }
                }
            }
        }
    }
    // Sign by ray parity along +x for every (j, k) column. The ray is nudged by tiny
    // irrational offsets so it never lands exactly on an edge or vertex.
    let mut crossings: Vec<Vec<f32>> = vec![Vec::new(); ny * nz];
    for t in &tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let (miny, maxy) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
        let (minz, maxz) = (a[2].min(b[2]).min(c[2]), a[2].max(b[2]).max(c[2]));
        let j0 = (((miny - lo[1]) / voxel - 1e-3).floor().max(0.0)) as usize;
        let j1 = ((((maxy - lo[1]) / voxel + 1e-3).ceil()) as usize).min(ny - 1);
        let k0 = (((minz - lo[2]) / voxel - 1e-3).floor().max(0.0)) as usize;
        let k1 = ((((maxz - lo[2]) / voxel + 1e-3).ceil()) as usize).min(nz - 1);
        // Plane x = (d - ny*y - nz*z) / nx for the triangle.
        let n = math::cross(math::sub(b, a), math::sub(c, a));
        if n[0].abs() < 1e-12 {
            continue; // parallel to the ray
        }
        let d = n[0] * a[0] + n[1] * a[1] + n[2] * a[2];
        let (v0, v1, v2) = ([a[1], a[2]], [b[1], b[2]], [c[1], c[2]]);
        let area = (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]);
        if area.abs() < 1e-14 {
            continue;
        }
        for k in k0..=k1 {
            let z = lo[2] + k as f32 * voxel + 1.3e-5;
            for j in j0..=j1 {
                let y = lo[1] + j as f32 * voxel + 1.0e-5;
                let w0 = ((v1[0] - y) * (v2[1] - z) - (v1[1] - z) * (v2[0] - y)) / area;
                let w1 = ((v2[0] - y) * (v0[1] - z) - (v2[1] - z) * (v0[0] - y)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    crossings[k * ny + j].push((d - n[1] * y - n[2] * z) / n[0]);
                }
            }
        }
    }
    let data = Arc::make_mut(&mut vol.data);
    data.par_chunks_mut(nx).enumerate().for_each(|(row, line)| {
        let mut xs = crossings[row].clone();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for (i, v) in line.iter_mut().enumerate() {
            let x = lo[0] + i as f32 * voxel;
            let inside = xs.partition_point(|&c| c < x) % 2 == 1;
            let d = dist[row * nx + i];
            *v = if inside { -d } else { d };
        }
    });
    Ok(with_sdf(Geometry::new(), vol))
}

fn map_sdf(g: &Geometry, op: &str, f: impl Fn([f32; 3], f32) -> f32 + Sync) -> Res {
    let v = sdf_of(g, op)?;
    let mut out = (**v).clone();
    let (nx, ny, voxel, origin) = (v.dims[0], v.dims[1], v.voxel, v.origin);
    let src = v.data.clone();
    Arc::make_mut(&mut out.data).par_chunks_mut(nx * ny).enumerate().for_each(|(k, chunk)| {
        for j in 0..ny {
            for i in 0..nx {
                let q = [origin[0] + i as f32 * voxel, origin[1] + j as f32 * voxel, origin[2] + k as f32 * voxel];
                chunk[j * nx + i] = f(q, src[(k * ny + j) * nx + i]);
            }
        }
    });
    let mut res = g.clone();
    res.volumes.insert("sdf".into(), Arc::new(out));
    Ok(res)
}

fn smooth(g: &Geometry, p: &Params) -> Res {
    let v = sdf_of(g, "sdf_smooth")?;
    let [nx, ny, nz] = v.dims;
    let mut cur: Vec<f32> = (*v.data).clone();
    for _ in 0..p.i("iterations").clamp(0, 20) {
        let prev = cur.clone();
        cur.par_chunks_mut(nx * ny).enumerate().for_each(|(k, chunk)| {
            for j in 0..ny {
                for i in 0..nx {
                    let (mut s, mut n) = (0.0f32, 0.0f32);
                    for dk in -1i64..=1 {
                        for dj in -1i64..=1 {
                            for di in -1i64..=1 {
                                let (ii, jj, kk) = (i as i64 + di, j as i64 + dj, k as i64 + dk);
                                if ii < 0 || jj < 0 || kk < 0 || ii >= nx as i64 || jj >= ny as i64 || kk >= nz as i64 {
                                    continue;
                                }
                                s += prev[(kk as usize * ny + jj as usize) * nx + ii as usize];
                                n += 1.0;
                            }
                        }
                    }
                    chunk[j * nx + i] = s / n;
                }
            }
        });
    }
    let mut out = (**v).clone();
    out.data = Arc::new(cur);
    let mut res = g.clone();
    res.volumes.insert("sdf".into(), Arc::new(out));
    Ok(res)
}

fn displace(g: &Geometry, p: &Params) -> Res {
    let (amp, freq, oct, seed) = (p.f("amp"), p.f("freq"), p.i("octaves").clamp(1, 8) as u32, p.i("seed") as u32);
    map_sdf(g, "sdf_displace", move |q, v| v + fbm(q, freq, oct, seed) * amp)
}

fn combine(a: &Geometry, b: &Geometry, p: &Params) -> Res {
    let (va, vb) = (sdf_of(a, "sdf_combine")?, sdf_of(b, "sdf_combine")?);
    let op = p.s("op");
    let k = p.f("k").max(1e-5);
    let (la, ha) = va.bounds();
    let (lb, hb) = vb.bounds();
    let (lo, hi) = ([0, 1, 2].map(|i| la[i].min(lb[i])), [0, 1, 2].map(|i| ha[i].max(hb[i])));
    let voxel = if p.f("voxel") > 0.0 { p.f("voxel") } else { va.voxel.min(vb.voxel) };
    let mut out = alloc(lo, hi, voxel, "sdf_combine")?;
    let smin = move |x: f32, y: f32| {
        let h = (0.5 + 0.5 * (y - x) / k).clamp(0.0, 1.0);
        y + (x - y) * h - k * h * (1.0 - h)
    };
    let f: Box<dyn Fn(f32, f32) -> f32 + Sync> = match op.as_str() {
        "union" => Box::new(f32::min),
        "intersect" => Box::new(f32::max),
        "subtract" => Box::new(|x, y| x.max(-y)),
        "smooth_union" => Box::new(smin),
        "smooth_subtract" => Box::new(move |x, y| -smin(-x, y)),
        o => return Err(format!("sdf_combine: unknown op '{o}' (union | intersect | subtract | smooth_union | smooth_subtract)")),
    };
    fill(&mut out, |q| f(va.sample(q), vb.sample(q)));
    Ok(with_sdf(Geometry::new(), out))
}

fn surface_nets(v: &Volume, iso: f32) -> (Vec<[f32; 3]>, Vec<Vec<u32>>) {
    let [nx, ny, nz] = v.dims;
    let val = |i: usize, j: usize, k: usize| v.data[v.idx(i, j, k)] - iso;
    let (cx, cy, cz) = (nx - 1, ny - 1, nz - 1);
    let cell = |i: usize, j: usize, k: usize| (k * cy + j) * cx + i;
    let mut cell_vert = vec![u32::MAX; cx * cy * cz];
    let mut verts: Vec<[f32; 3]> = Vec::new();
    const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    for k in 0..cz {
        for j in 0..cy {
            for i in 0..cx {
                let mut cv = [0.0f32; 8];
                let mut mask = 0u8;
                for (b, c) in cv.iter_mut().enumerate() {
                    *c = val(i + (b & 1), j + ((b >> 1) & 1), k + ((b >> 2) & 1));
                    if *c < 0.0 {
                        mask |= 1 << b;
                    }
                }
                if mask == 0 || mask == 255 {
                    continue;
                }
                let (mut acc, mut n) = ([0.0f32; 3], 0.0f32);
                for (a, b) in EDGES {
                    if (cv[a] < 0.0) != (cv[b] < 0.0) {
                        let t = cv[a] / (cv[a] - cv[b]);
                        let (pa, pb) = (v.pos(i + (a & 1), j + ((a >> 1) & 1), k + ((a >> 2) & 1)), v.pos(i + (b & 1), j + ((b >> 1) & 1), k + ((b >> 2) & 1)));
                        for c in 0..3 {
                            acc[c] += pa[c] + (pb[c] - pa[c]) * t;
                        }
                        n += 1.0;
                    }
                }
                cell_vert[cell(i, j, k)] = verts.len() as u32;
                verts.push([acc[0] / n, acc[1] / n, acc[2] / n]);
            }
        }
    }
    let mut quads: Vec<Vec<u32>> = Vec::new();
    let lat = [nx, ny, nz];
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let p0 = [i, j, k];
                for ax in 0..3 {
                    let (b, c) = ((ax + 1) % 3, (ax + 2) % 3);
                    if p0[ax] + 1 >= lat[ax] || p0[b] == 0 || p0[c] == 0 {
                        continue;
                    }
                    let mut p1 = p0;
                    p1[ax] += 1;
                    let (s0, s1) = (val(p0[0], p0[1], p0[2]) < 0.0, val(p1[0], p1[1], p1[2]) < 0.0);
                    if s0 == s1 {
                        continue;
                    }
                    let at = |db: usize, dc: usize| {
                        let mut q = p0;
                        q[b] -= db;
                        q[c] -= dc;
                        cell_vert[cell(q[0], q[1], q[2])]
                    };
                    let mut quad = vec![at(1, 1), at(0, 1), at(0, 0), at(1, 0)];
                    if quad.contains(&u32::MAX) {
                        continue;
                    }
                    // Orient outward: from the inside corner toward the outside corner.
                    let dir = if s0 { 1.0 } else { -1.0 };
                    let (v0, v1, v2, v3) = (verts[quad[0] as usize], verts[quad[1] as usize], verts[quad[2] as usize], verts[quad[3] as usize]);
                    let nrm = math::cross(math::sub(v2, v0), math::sub(v3, v1));
                    if nrm[ax] * dir < 0.0 {
                        quad.reverse();
                    }
                    quads.push(quad);
                }
            }
        }
    }
    (verts, quads)
}

/// Marching tetrahedra on the Kuhn cube split: more triangles than surface nets, but the
/// output is a closed manifold by construction (shared lattice-edge vertices, consistent
/// face diagonals), so it is the fallback when surface nets hits an ambiguous cell.
fn marching_tets(v: &Volume, iso: f32) -> (Vec<[f32; 3]>, Vec<Vec<u32>>) {
    let [nx, ny, nz] = v.dims;
    const TETS: [[usize; 4]; 6] = [[0, 1, 3, 7], [0, 1, 5, 7], [0, 2, 3, 7], [0, 2, 6, 7], [0, 4, 5, 7], [0, 4, 6, 7]];
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut edge_vert: std::collections::HashMap<(usize, usize), u32> = std::collections::HashMap::new();
    let mut tris: Vec<Vec<u32>> = Vec::new();
    for k in 0..nz - 1 {
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                let corner = |b: usize| (i + (b & 1), j + ((b >> 1) & 1), k + ((b >> 2) & 1));
                let vals: [f32; 8] = std::array::from_fn(|b| {
                    let (ci, cj, ck) = corner(b);
                    v.data[v.idx(ci, cj, ck)] - iso
                });
                if vals.iter().all(|x| *x < 0.0) || vals.iter().all(|x| *x >= 0.0) {
                    continue;
                }
                for tet in TETS {
                    let inside: Vec<usize> = tet.iter().copied().filter(|&b| vals[b] < 0.0).collect();
                    let outside: Vec<usize> = tet.iter().copied().filter(|&b| vals[b] >= 0.0).collect();
                    if inside.is_empty() || outside.is_empty() {
                        continue;
                    }
                    let mut edge = |a: usize, b: usize| -> u32 {
                        let (ca, cb) = (corner(a), corner(b));
                        let (ia, ib) = ((ca.2 * ny + ca.1) * nx + ca.0, (cb.2 * ny + cb.1) * nx + cb.0);
                        let key = (ia.min(ib), ia.max(ib));
                        *edge_vert.entry(key).or_insert_with(|| {
                            let t = vals[a] / (vals[a] - vals[b]);
                            let (pa, pb) = (v.pos(ca.0, ca.1, ca.2), v.pos(cb.0, cb.1, cb.2));
                            verts.push([pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t, pa[2] + (pb[2] - pa[2]) * t]);
                            verts.len() as u32 - 1
                        })
                    };
                    let mut emit: Vec<[u32; 3]> = Vec::new();
                    match (inside.len(), outside.len()) {
                        (1, 3) => emit.push([edge(inside[0], outside[0]), edge(inside[0], outside[1]), edge(inside[0], outside[2])]),
                        (3, 1) => emit.push([edge(outside[0], inside[0]), edge(outside[0], inside[1]), edge(outside[0], inside[2])]),
                        _ => {
                            let (a, b, c, d) = (edge(inside[0], outside[0]), edge(inside[0], outside[1]), edge(inside[1], outside[1]), edge(inside[1], outside[0]));
                            emit.push([a, b, c]);
                            emit.push([a, c, d]);
                        }
                    }
                    // Orient from the inside toward the outside.
                    let centroid = |ids: &[usize]| {
                        let mut s = [0.0f32; 3];
                        for &b in ids {
                            let (ci, cj, ck) = corner(b);
                            let q = v.pos(ci, cj, ck);
                            for c in 0..3 {
                                s[c] += q[c] / ids.len() as f32;
                            }
                        }
                        s
                    };
                    let dir = math::sub(centroid(&outside), centroid(&inside));
                    for t in emit {
                        let (a, b, c) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
                        let n = math::cross(math::sub(b, a), math::sub(c, a));
                        let flip = n[0] * dir[0] + n[1] * dir[1] + n[2] * dir[2] < 0.0;
                        tris.push(if flip { vec![t[0], t[2], t[1]] } else { t.to_vec() });
                    }
                }
            }
        }
    }
    (verts, tris)
}

fn non_manifold_edges(polys: &[Vec<u32>]) -> usize {
    let mut edges: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
    for pr in polys {
        for k in 0..pr.len() {
            let (a, b) = (pr[k], pr[(k + 1) % pr.len()]);
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    edges.values().filter(|&&c| c > 2).count()
}

fn to_mesh(g: &Geometry, p: &Params) -> Res {
    let v = sdf_of(g, "sdf_to_mesh")?;
    let iso = p.f("iso");
    if v.dims.iter().any(|&d| d < 2) {
        return Err("sdf_to_mesh: volume is too small".into());
    }
    let (verts, quads) = match p.s("method").as_str() {
        "nets" => surface_nets(v, iso),
        "tets" => marching_tets(v, iso),
        "auto" => {
            let (vs, qs) = surface_nets(v, iso);
            if non_manifold_edges(&qs) > 0 { marching_tets(v, iso) } else { (vs, qs) }
        }
        o => return Err(format!("sdf_to_mesh: unknown method '{o}' (auto | nets | tets)")),
    };
    // Smooth normals from the SDF gradient.
    let h = v.voxel * 0.5;
    let normals: Vec<[f32; 3]> = verts
        .par_iter()
        .map(|q| {
            let g = [
                v.sample([q[0] + h, q[1], q[2]]) - v.sample([q[0] - h, q[1], q[2]]),
                v.sample([q[0], q[1] + h, q[2]]) - v.sample([q[0], q[1] - h, q[2]]),
                v.sample([q[0], q[1], q[2] + h]) - v.sample([q[0], q[1], q[2] - h]),
            ];
            math::normalize(g)
        })
        .collect();
    let mut m = Geometry::from_polys(verts, &quads);
    m.point.set("N", Column::from_v3(normals));
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: &str, set: &[(&str, serde_json::Value)], ins: &[&Geometry]) -> Geometry {
        let mut p = Params::new();
        for s in crate::schema(kind).unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        for (k, v) in set {
            p.insert((*k).into(), v.clone());
        }
        let inputs: Vec<Option<Arc<Geometry>>> = ins.iter().map(|g| Some(Arc::new((*g).clone()))).collect();
        crate::cook(kind, &p, &inputs).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    fn volume(g: &Geometry) -> f32 {
        let p = g.p();
        g.triangles().iter().map(|t| {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let cr = math::cross(b, c);
            (a[0] * cr[0] + a[1] * cr[1] + a[2] * cr[2]) / 6.0
        }).sum()
    }

    fn watertight(g: &Geometry) -> bool {
        let iss = pt_core::validate::validate(g);
        iss.iter().any(|i| i.code == "boundary_edges" && i.message.starts_with("0 ")) && !iss.iter().any(|i| i.code == "non_manifold_edges" || i.severity == "error")
    }

    #[test]
    fn sphere_sdf_meshes_to_a_closed_correctly_wound_surface() {
        let s = run("sdf_shape", &[("shape", json!("sphere")), ("radius", json!(0.5)), ("voxel", json!(0.03))], &[]);
        let m = run("sdf_to_mesh", &[], &[&s]);
        assert!(watertight(&m), "{:?}", pt_core::validate::validate(&m).iter().map(|i| (i.code, i.message.clone())).collect::<Vec<_>>());
        let want = 4.0 / 3.0 * std::f32::consts::PI * 0.125;
        let v = volume(&m);
        assert!(v > 0.0 && (v / want - 1.0).abs() < 0.03, "volume {v} vs {want}");
        // Normals point outward.
        let n = m.point.get("N").unwrap().as_v3().unwrap();
        assert!(m.p().iter().zip(n).all(|(p, n)| p[0] * n[0] + p[1] * n[1] + p[2] * n[2] > 0.0));
    }

    #[test]
    fn mesh_to_sdf_round_trips_a_box() {
        let b = crate::cook("box", &{
            let mut p = Params::new();
            p.insert("size".into(), json!([1.0, 0.6, 0.8]));
            p.insert("center".into(), json!([0.0, 0.0, 0.0]));
            p
        }, &[]).unwrap();
        let s = run("sdf_from_mesh", &[("voxel", json!(0.04))], &[&b]);
        let vol = &s.volumes["sdf"];
        assert!(vol.sample([0.0, 0.0, 0.0]) < -0.1, "center is inside (distance clamped to the 3-voxel band)");
        assert!(vol.sample([1.2, 0.0, 0.0]) > 0.1, "far is outside");
        let m = run("sdf_to_mesh", &[], &[&s]);
        assert!(watertight(&m));
        let (lo, hi) = m.base_bounds().unwrap();
        assert!((hi[0] - lo[0] - 1.0).abs() < 0.08 && (hi[1] - lo[1] - 0.6).abs() < 0.08, "bounds {lo:?} {hi:?}");
        let v = volume(&m);
        assert!((v / 0.48 - 1.0).abs() < 0.08, "box volume {v}");
    }

    #[test]
    fn booleans_have_the_right_volumes() {
        let big = run("sdf_shape", &[("shape", json!("box")), ("size", json!([1.0, 1.0, 1.0])), ("voxel", json!(0.03))], &[]);
        let hole = run("sdf_shape", &[("shape", json!("sphere")), ("radius", json!(0.4)), ("voxel", json!(0.03))], &[]);
        let sub = run("sdf_to_mesh", &[], &[&run("sdf_combine", &[("op", json!("subtract"))], &[&big, &hole])]);
        let want = 1.0 - 4.0 / 3.0 * std::f32::consts::PI * 0.064;
        let v = volume(&sub);
        assert!((v / want - 1.0).abs() < 0.04, "box minus sphere {v} vs {want}");
        assert!(watertight(&sub));
        // Two overlapping spheres: union < sum, intersect < each.
        let a = run("sdf_shape", &[("radius", json!(0.4)), ("center", json!([-0.2, 0.0, 0.0])), ("voxel", json!(0.03))], &[]);
        let b = run("sdf_shape", &[("radius", json!(0.4)), ("center", json!([0.2, 0.0, 0.0])), ("voxel", json!(0.03))], &[]);
        let sphere_v = 4.0 / 3.0 * std::f32::consts::PI * 0.064;
        let un = volume(&run("sdf_to_mesh", &[], &[&run("sdf_combine", &[("op", json!("union"))], &[&a, &b])]));
        let it = volume(&run("sdf_to_mesh", &[], &[&run("sdf_combine", &[("op", json!("intersect"))], &[&a, &b])]));
        assert!(un < 2.0 * sphere_v && un > sphere_v, "union {un}");
        assert!(it > 0.0 && it < sphere_v, "intersect {it}");
        assert!(((un + it) / (2.0 * sphere_v) - 1.0).abs() < 0.05, "inclusion-exclusion: {} vs {}", un + it, 2.0 * sphere_v);
        let sm = volume(&run("sdf_to_mesh", &[], &[&run("sdf_combine", &[("op", json!("smooth_union")), ("k", json!(0.2))], &[&a, &b])]));
        assert!(sm > un, "smooth union adds volume in the crease: {sm} vs {un}");
    }

    #[test]
    fn sdf_ops_are_deterministic_and_validated() {
        let s = run("sdf_shape", &[("shape", json!("torus"))], &[]);
        let d = run("sdf_displace", &[], &[&s]);
        let again = run("sdf_displace", &[], &[&s]);
        assert_eq!(d.content_hash(), again.content_hash());
        assert_ne!(d.content_hash(), s.content_hash());
        let off = run("sdf_offset", &[("distance", json!(0.1))], &[&s]);
        assert!(volume(&run("sdf_to_mesh", &[], &[&off])) > volume(&run("sdf_to_mesh", &[], &[&s])));
        // Errors are clear.
        let mut p = Params::new();
        p.insert("iso".into(), json!(0.0));
        let e = crate::cook("sdf_to_mesh", &p, &[Some(Arc::new(Geometry::new()))]).unwrap_err();
        assert!(e.contains("no 'sdf' volume"));
        let e = crate::cook("sdf_shape", &{
            let mut p = Params::new();
            p.insert("shape".into(), json!("sphere"));
            p.insert("radius".into(), json!(50.0));
            p.insert("voxel".into(), json!(0.01));
            p.insert("padding".into(), json!(0.2));
            p
        }, &[]).unwrap_err();
        assert!(e.contains("increase `voxel`"), "{e}");
    }

    /// Fuzz: random shapes, random booleans, random noise. `auto` and `tets` must always give
    /// closed manifold meshes with positive volume; plain surface nets may not.
    #[test]
    fn fuzz_mesher_is_always_manifold_with_auto() {
        use pt_core::rng::rand4;
        let shapes = ["sphere", "box", "torus", "capsule", "cylinder"];
        let ops = ["union", "intersect", "subtract", "smooth_union", "smooth_subtract"];
        let mut nets_bad = 0;
        let cases = 24;
        for case in 0..cases {
            let r = |i: u64| rand4(1000 + case, 9, i);
            let mk = |n: u64| {
                let s = r(n * 3);
                let c = r(n * 3 + 1);
                run(
                    "sdf_shape",
                    &[
                        ("shape", json!(shapes[(s[0] * 5.0) as usize % 5])),
                        ("radius", json!(0.25 + 0.3 * s[1])),
                        ("size", json!([0.4 + 0.6 * s[2], 0.4 + 0.6 * s[3], 0.4 + 0.6 * c[3]])),
                        ("center", json!([(c[0] - 0.5) * 0.8, (c[1] - 0.5) * 0.8, (c[2] - 0.5) * 0.8])),
                        ("voxel", json!(0.06)),
                    ],
                    &[],
                )
            };
            let mut field = mk(0);
            for n in 1..4u64 {
                let o = r(100 + n);
                field = run("sdf_combine", &[("op", json!(ops[(o[0] * 5.0) as usize % 5])), ("k", json!(0.05 + 0.2 * o[1]))], &[&field, &mk(n)]);
            }
            let amp = r(200)[0] * 0.08;
            let field = run("sdf_displace", &[("amp", json!(amp)), ("freq", json!(3.0 + 6.0 * r(200)[1])), ("seed", json!(case))], &[&field]);
            let nets = run("sdf_to_mesh", &[("method", json!("nets"))], &[&field]);
            if nets.n_prims() > 0 && !watertight(&nets) {
                nets_bad += 1;
            }
            for method in ["auto", "tets"] {
                let m = run("sdf_to_mesh", &[("method", json!(method))], &[&field]);
                if m.n_prims() == 0 {
                    continue; // empty result (e.g. intersect of disjoint shapes)
                }
                assert!(watertight(&m), "case {case} method {method} is not a closed manifold");
                assert!(volume(&m) > 0.0, "case {case} method {method} has non-positive volume {}", volume(&m));
            }
        }
        eprintln!("surface nets alone was non-manifold/open in {nets_bad} of {cases} fuzz cases; auto repaired all");
    }

    #[test]
    fn volumes_survive_cache_serialization() {
        let s = run("sdf_shape", &[], &[]);
        let bytes = pt_core::io::to_bytes(&s);
        let back = pt_core::io::from_bytes(&bytes).unwrap();
        assert_eq!(s.content_hash(), back.content_hash());
        assert_eq!(back.volumes["sdf"].dims, s.volumes["sdf"].dims);
    }
}

/// Crate-visible wrappers so other op modules (fracture, volumes) can reuse the SDF pipeline.
pub(crate) fn from_mesh_pub(src: &Geometry, p: &Params) -> Res {
    from_mesh(src, p)
}

pub(crate) fn to_mesh_pub(g: &Geometry, p: &Params) -> Res {
    to_mesh(g, p)
}
