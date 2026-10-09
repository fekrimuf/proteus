//! Sparse signed-distance ops: `sparse_remesh` and `sparse_boolean`.
//!
//! Meshes become narrow-band SDFs stored in tiles (`pt_core::SparseVolume`), so memory follows the
//! surface area instead of the bounding volume. Booleans combine the tiles, and a sparse mesher
//! (surface nets, with marching tetrahedra as the manifold fallback) extracts the result. The dense
//! `sdf_*` nodes are unchanged; use these when `sdf_from_mesh` would exceed its voxel limit.
//!
//! Inputs must be closed meshes (sign comes from ray parity, exactly like `sdf_from_mesh`).

use crate::sdf::{closest_on_tri, combine_fn, non_manifold_edges};
use crate::{ps, sch};
use pt_core::math::{self, cross, sub};
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params, SparseVolume, Tile, Volume};
use rayon::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

const T: i32 = pt_core::sparse::TILE;
/// Hard cap on stored dense tiles (2 KB each): about 6 GB.
const MAX_DENSE_TILES: usize = 3_000_000;

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("sparse_remesh", "Rebuild a closed mesh through a sparse narrow-band SDF: uniform quads/triangles at `voxel` resolution, fixes non-manifold input, optional `offset` (grow/shrink). Memory follows surface area, so it works at voxel sizes where the dense sdf nodes hit their limit. Writes `N`.", 1, D::None, G::Whole, vec![
            ps("voxel", json!(0.05), "Voxel size (m)."),
            ps("band", json!(3), "Narrow band half-width in voxels (>= 2)."),
            ps("offset", json!(0.0), "Move the surface outward (+) or inward (-) by this many meters."),
            ps("method", json!("auto"), "auto | nets | tets."),
        ]),
        sch("sparse_boolean", "Boolean of two closed meshes through sparse SDFs: union | intersect | subtract (A - B) | smooth_union | smooth_subtract; result is a mesh with normals `N`. No voxel-count limit.", 2, D::None, G::Whole, vec![
            ps("op", json!("union"), "union | intersect | subtract | smooth_union | smooth_subtract."),
            ps("k", json!(0.1), "Smoothing radius (m) for the smooth ops; keep it below band * voxel."),
            ps("voxel", json!(0.05), "Voxel size (m)."),
            ps("band", json!(3), "Narrow band half-width in voxels (>= 2)."),
            ps("method", json!("auto"), "auto | nets | tets."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "sparse_remesh" => inp(0).and_then(|g| remesh(g, p)),
        "sparse_boolean" => inp(0).and_then(|a| inp(1).and_then(|b| boolean(a, b, p))),
        _ => return None,
    })
}

fn params(p: &Params, node: &str) -> Result<(f32, i32), String> {
    let voxel = p.f("voxel");
    if !(voxel >= 1e-4) {
        return Err(format!("{node}: voxel must be >= 0.0001 m"));
    }
    let band = p.i("band");
    if !(2..=16).contains(&band) {
        return Err(format!("{node}: band must be between 2 and 16 voxels (the mesher needs the surface to sit inside stored tiles)"));
    }
    Ok((voxel, band as i32))
}

fn floor_to_i32(x: f32) -> i32 {
    x.floor().clamp(-1.0e9, 1.0e9) as i32
}

/// Build a narrow-band SDF of a closed mesh. Constant tiles are stored deep inside the shape: absent tiles read as
/// "outside", so interior blocks must exist or the mesher would see a false surface where dense tiles border them.
pub fn build_sdf(src: &Geometry, voxel: f32, band: i32, node: &str) -> Result<SparseVolume, String> {
    let g = if src.instances.is_some() { src.realize() } else { src.clone() };
    let tris = g.triangles();
    if tris.is_empty() {
        return Err(format!("{node}: input has no polygons"));
    }
    let pts = g.p();
    let band_d = band as f32 * voxel;
    let mut vol = SparseVolume::new(voxel, [0.0; 3], band_d);

    // 1. Unsigned distance near the surface. Long triangles are split so each piece has a small bbox.
    let max_edge = 6.0 * voxel;
    let len = |a: [f32; 3], b: [f32; 3]| {
        let d = sub(a, b);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    for t in &tris {
        let mut stack = vec![(pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize])];
        while let Some((a, b, c)) = stack.pop() {
            let (ab, bc, ca) = (len(a, b), len(b, c), len(c, a));
            let longest = ab.max(bc).max(ca);
            if longest > max_edge {
                let mid = |p: [f32; 3], q: [f32; 3]| [(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5, (p[2] + q[2]) * 0.5];
                if ab >= bc && ab >= ca {
                    let m = mid(a, b);
                    stack.push((a, m, c));
                    stack.push((m, b, c));
                } else if bc >= ca {
                    let m = mid(b, c);
                    stack.push((a, b, m));
                    stack.push((a, m, c));
                } else {
                    let m = mid(c, a);
                    stack.push((a, b, m));
                    stack.push((m, b, c));
                }
                continue;
            }
            let mut lo = [0i32; 3];
            let mut hi = [0i32; 3];
            for ax in 0..3 {
                lo[ax] = floor_to_i32((a[ax].min(b[ax]).min(c[ax]) - band_d) / voxel);
                hi[ax] = (((a[ax].max(b[ax]).max(c[ax]) + band_d) / voxel).ceil().clamp(-1.0e9, 1.0e9)) as i32;
            }
            for tk in lo[2].div_euclid(T)..=hi[2].div_euclid(T) {
                for tj in lo[1].div_euclid(T)..=hi[1].div_euclid(T) {
                    for ti in lo[0].div_euclid(T)..=hi[0].div_euclid(T) {
                        let key = [ti, tj, tk];
                        if vol.tile(key).is_none() && vol.n_tiles() >= MAX_DENSE_TILES {
                            return Err(format!("{node}: more than {MAX_DENSE_TILES} tiles; increase `voxel` (currently {voxel} m)"));
                        }
                        let tile = vol.dense_mut(key, band_d);
                        for lk in 0..T {
                            let k = tk * T + lk;
                            if k < lo[2] || k > hi[2] {
                                continue;
                            }
                            for lj in 0..T {
                                let j = tj * T + lj;
                                if j < lo[1] || j > hi[1] {
                                    continue;
                                }
                                for li in 0..T {
                                    let i = ti * T + li;
                                    if i < lo[0] || i > hi[0] {
                                        continue;
                                    }
                                    let q = [i as f32 * voxel, j as f32 * voxel, k as f32 * voxel];
                                    let cp = closest_on_tri(q, a, b, c);
                                    let d = len(q, cp);
                                    let slot = &mut tile[((lk * T + lj) * T + li) as usize];
                                    if d < *slot {
                                        *slot = d;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    vol.compact(); // tiles that received no hit collapse to background and disappear

    // 2. Sign by ray parity along +x, with the same nudges as the dense `sdf_from_mesh`.
    let mut cross_x: Vec<(i32, i32, f32)> = Vec::new(); // (k, j, x)
    for t in &tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let n = cross(sub(b, a), sub(c, a));
        if n[0].abs() < 1e-12 {
            continue;
        }
        let d = n[0] * a[0] + n[1] * a[1] + n[2] * a[2];
        let (v0, v1, v2) = ([a[1], a[2]], [b[1], b[2]], [c[1], c[2]]);
        let area = (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]);
        if area.abs() < 1e-14 {
            continue;
        }
        let j0 = floor_to_i32(a[1].min(b[1]).min(c[1]) / voxel - 1e-3);
        let j1 = ((a[1].max(b[1]).max(c[1]) / voxel + 1e-3).ceil()) as i32;
        let k0 = floor_to_i32(a[2].min(b[2]).min(c[2]) / voxel - 1e-3);
        let k1 = ((a[2].max(b[2]).max(c[2]) / voxel + 1e-3).ceil()) as i32;
        for k in k0..=k1 {
            let z = k as f32 * voxel + 1.3e-5;
            for j in j0..=j1 {
                let y = j as f32 * voxel + 1.0e-5;
                let w0 = ((v1[0] - y) * (v2[1] - z) - (v1[1] - z) * (v2[0] - y)) / area;
                let w1 = ((v2[0] - y) * (v0[1] - z) - (v2[1] - z) * (v0[0] - y)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    cross_x.push((k, j, (d - n[1] * y - n[2] * z) / n[0]));
                }
            }
        }
    }
    cross_x.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)));
    // Row table: (k, j, start, end) into cross_x.
    let mut rows: Vec<(i32, i32, usize, usize)> = Vec::new();
    let mut s = 0;
    while s < cross_x.len() {
        let (k, j) = (cross_x[s].0, cross_x[s].1);
        let mut e = s;
        while e < cross_x.len() && cross_x[e].0 == k && cross_x[e].1 == j {
            e += 1;
        }
        rows.push((k, j, s, e));
        s = e;
    }
    let row_of = |k: i32, j: i32| -> Option<&[(i32, i32, f32)]> {
        let idx = rows.binary_search_by(|r| r.0.cmp(&k).then(r.1.cmp(&j))).ok()?;
        Some(&cross_x[rows[idx].2..rows[idx].3])
    };
    for key in vol.dense_keys() {
        let mut signs = vec![false; pt_core::sparse::TILE_VOXELS];
        for lk in 0..T {
            for lj in 0..T {
                let Some(xs) = row_of(key[2] * T + lk, key[1] * T + lj) else { continue };
                for li in 0..T {
                    let x = (key[0] * T + li) as f32 * voxel;
                    let n = xs.partition_point(|c| c.2 < x);
                    signs[((lk * T + lj) * T + li) as usize] = n % 2 == 1;
                }
            }
        }
        let tile = vol.dense_mut(key, band_d);
        for (v, inside) in tile.iter_mut().zip(&signs) {
            if *inside {
                *v = -*v;
            }
        }
    }

    // 3. Deep interior as constant tiles: a tile is interior when all 64 of its rows are inside for its whole x extent.
    // Interior pass groups rows by tile-row (tk, tj), so order a copy of the table that way.
    let mut grouped: Vec<(i32, i32, usize, usize)> = rows.clone();
    grouped.sort_by_key(|r| (r.0.div_euclid(T), r.1.div_euclid(T), r.0, r.1));
    {
        let mut s = 0;
        while s < grouped.len() {
            let (tk, tj) = (grouped[s].0.div_euclid(T), grouped[s].1.div_euclid(T));
            let mut e = s;
            while e < grouped.len() && grouped[e].0.div_euclid(T) == tk && grouped[e].1.div_euclid(T) == tj {
                e += 1;
            }
            if e - s == (T * T) as usize {
                let mut events: Vec<(i32, i32)> = Vec::new();
                for r in &grouped[s..e] {
                    let xs = &cross_x[r.2..r.3];
                    for pair in xs.chunks_exact(2) {
                        let (x0, x1) = (pair[0].2, pair[1].2);
                        // Voxels i with x0 < i*voxel <= x1 are inside (matches the parity test above).
                        let mut i0 = floor_to_i32(x0 / voxel);
                        while (i0 as f32) * voxel <= x0 {
                            i0 += 1;
                        }
                        while ((i0 - 1) as f32) * voxel > x0 {
                            i0 -= 1;
                        }
                        let mut i1 = floor_to_i32(x1 / voxel);
                        while ((i1 + 1) as f32) * voxel <= x1 {
                            i1 += 1;
                        }
                        while (i1 as f32) * voxel > x1 {
                            i1 -= 1;
                        }
                        if i1 < i0 {
                            continue;
                        }
                        let (ta, tb) = ((i0 + T - 1).div_euclid(T), (i1 + 1).div_euclid(T) - 1);
                        if tb >= ta {
                            events.push((ta, 1));
                            events.push((tb + 1, -1));
                        }
                    }
                }
                events.sort_unstable();
                let (mut count, mut last) = (0i32, 0i32);
                for (x, d) in events {
                    if count == (T * T) as i32 {
                        for tx in last..x {
                            if vol.tile([tx, tj, tk]).is_none() {
                                vol.insert_tile([tx, tj, tk], Tile::Const(-band_d));
                            }
                        }
                    }
                    count += d;
                    last = x;
                }
            }
            s = e;
        }
    }
    vol.compact();
    Ok(vol)
}

/// Pointwise combination of two sparse SDFs on the same voxel grid.
pub fn combine(a: &SparseVolume, b: &SparseVolume, f: &(dyn Fn(f32, f32) -> f32 + Sync + Send)) -> SparseVolume {
    let mut keys: Vec<[i32; 3]> = a.keys();
    keys.extend(b.keys());
    keys.sort_unstable();
    keys.dedup();
    let tile_vals = |v: &SparseVolume, key: [i32; 3]| -> Result<Arc<Vec<f32>>, f32> {
        match v.tile(key) {
            Some(Tile::Dense(d)) => Ok(d.clone()),
            Some(Tile::Const(c)) => Err(*c),
            None => Err(v.background),
        }
    };
    let tiles: Vec<(([i32; 3]), Tile)> = keys
        .par_iter()
        .map(|&key| {
            let t = match (tile_vals(a, key), tile_vals(b, key)) {
                (Err(x), Err(y)) => Tile::Const(f(x, y)),
                (Ok(x), Err(y)) => Tile::Dense(Arc::new(x.iter().map(|&v| f(v, y)).collect())),
                (Err(x), Ok(y)) => Tile::Dense(Arc::new(y.iter().map(|&v| f(x, v)).collect())),
                (Ok(x), Ok(y)) => Tile::Dense(Arc::new(x.iter().zip(y.iter()).map(|(&p, &q)| f(p, q)).collect())),
            };
            (key, t)
        })
        .collect();
    let mut out = SparseVolume::new(a.voxel, a.origin, f(a.background, b.background));
    for (k, t) in tiles {
        out.insert_tile(k, t);
    }
    out.compact();
    out
}

/// Dense copy of a box of voxels (inclusive index bounds), for tests and for handing data to dense ops.
pub fn to_dense(v: &SparseVolume, lo: [i32; 3], hi: [i32; 3]) -> Volume {
    let dims = [0, 1, 2].map(|a| (hi[a] - lo[a] + 1) as usize);
    let mut out = Volume::new(dims, v.pos(lo[0], lo[1], lo[2]), v.voxel, 0.0);
    let data = Arc::make_mut(&mut out.data);
    for k in 0..dims[2] {
        for j in 0..dims[1] {
            for i in 0..dims[0] {
                data[(k * dims[1] + j) * dims[0] + i] = v.get(lo[0] + i as i32, lo[1] + j as i32, lo[2] + k as i32);
            }
        }
    }
    out
}

const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

fn corner(c: [i32; 3], b: usize) -> [i32; 3] {
    [c[0] + (b & 1) as i32, c[1] + ((b >> 1) & 1) as i32, c[2] + ((b >> 2) & 1) as i32]
}

/// Voxel coordinates of every voxel stored in a dense tile, in a deterministic order.
fn dense_voxels(v: &SparseVolume) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    for key in v.dense_keys() {
        for lk in 0..T {
            for lj in 0..T {
                for li in 0..T {
                    out.push([key[0] * T + li, key[1] * T + lj, key[2] * T + lk]);
                }
            }
        }
    }
    out
}

pub fn surface_nets(v: &SparseVolume, iso: f32) -> (Vec<[f32; 3]>, Vec<Vec<u32>>) {
    let val = |c: [i32; 3]| v.get(c[0], c[1], c[2]) - iso;
    let voxels = dense_voxels(v);
    let mut cell_vert: HashMap<[i32; 3], u32> = HashMap::new();
    let mut verts: Vec<[f32; 3]> = Vec::new();
    for &c in &voxels {
        let mut cv = [0.0f32; 8];
        let mut mask = 0u8;
        for (b, x) in cv.iter_mut().enumerate() {
            *x = val(corner(c, b));
            if *x < 0.0 {
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
                let (pa, pb) = (corner(c, a), corner(c, b));
                let (pa, pb) = (v.pos(pa[0], pa[1], pa[2]), v.pos(pb[0], pb[1], pb[2]));
                for k in 0..3 {
                    acc[k] += pa[k] + (pb[k] - pa[k]) * t;
                }
                n += 1.0;
            }
        }
        cell_vert.insert(c, verts.len() as u32);
        verts.push([acc[0] / n, acc[1] / n, acc[2] / n]);
    }
    let mut quads: Vec<Vec<u32>> = Vec::new();
    for &p0 in &voxels {
        for ax in 0..3 {
            let (b, c) = ((ax + 1) % 3, (ax + 2) % 3);
            let mut p1 = p0;
            p1[ax] += 1;
            let (s0, s1) = (val(p0) < 0.0, val(p1) < 0.0);
            if s0 == s1 {
                continue;
            }
            let at = |db: i32, dc: i32| {
                let mut q = p0;
                q[b] -= db;
                q[c] -= dc;
                cell_vert.get(&q).copied()
            };
            let (Some(q0), Some(q1), Some(q2), Some(q3)) = (at(1, 1), at(0, 1), at(0, 0), at(1, 0)) else { continue };
            let mut quad = vec![q0, q1, q2, q3];
            let dir = if s0 { 1.0 } else { -1.0 };
            let (v0, v1, v2, v3) = (verts[quad[0] as usize], verts[quad[1] as usize], verts[quad[2] as usize], verts[quad[3] as usize]);
            let nrm = cross(sub(v2, v0), sub(v3, v1));
            if nrm[ax] * dir < 0.0 {
                quad.reverse();
            }
            quads.push(quad);
        }
    }
    (verts, quads)
}

pub fn marching_tets(v: &SparseVolume, iso: f32) -> (Vec<[f32; 3]>, Vec<Vec<u32>>) {
    const TETS: [[usize; 4]; 6] = [[0, 1, 3, 7], [0, 1, 5, 7], [0, 2, 3, 7], [0, 2, 6, 7], [0, 4, 5, 7], [0, 4, 6, 7]];
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut edge_vert: HashMap<([i32; 3], [i32; 3]), u32> = HashMap::new();
    let mut tris: Vec<Vec<u32>> = Vec::new();
    for c in dense_voxels(v) {
        let vals: [f32; 8] = std::array::from_fn(|b| {
            let q = corner(c, b);
            v.get(q[0], q[1], q[2]) - iso
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
                let (ca, cb) = (corner(c, a), corner(c, b));
                let key = if ca <= cb { (ca, cb) } else { (cb, ca) };
                *edge_vert.entry(key).or_insert_with(|| {
                    let t = vals[a] / (vals[a] - vals[b]);
                    let (pa, pb) = (v.pos(ca[0], ca[1], ca[2]), v.pos(cb[0], cb[1], cb[2]));
                    verts.push([pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t, pa[2] + (pb[2] - pa[2]) * t]);
                    verts.len() as u32 - 1
                })
            };
            let mut emit: Vec<[u32; 3]> = Vec::new();
            match (inside.len(), outside.len()) {
                (1, 3) => emit.push([edge(inside[0], outside[0]), edge(inside[0], outside[1]), edge(inside[0], outside[2])]),
                (3, 1) => emit.push([edge(outside[0], inside[0]), edge(outside[0], inside[1]), edge(outside[0], inside[2])]),
                _ => {
                    let (a, b, cc, d) = (edge(inside[0], outside[0]), edge(inside[0], outside[1]), edge(inside[1], outside[1]), edge(inside[1], outside[0]));
                    emit.push([a, b, cc]);
                    emit.push([a, cc, d]);
                }
            }
            let centroid = |ids: &[usize]| {
                let mut s = [0.0f32; 3];
                for &b in ids {
                    let q = corner(c, b);
                    let q = v.pos(q[0], q[1], q[2]);
                    for k in 0..3 {
                        s[k] += q[k] / ids.len() as f32;
                    }
                }
                s
            };
            let dir = sub(centroid(&outside), centroid(&inside));
            for t in emit {
                let (a, b, cc) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
                let n = cross(sub(b, a), sub(cc, a));
                let flip = n[0] * dir[0] + n[1] * dir[1] + n[2] * dir[2] < 0.0;
                tris.push(if flip { vec![t[0], t[2], t[1]] } else { t.to_vec() });
            }
        }
    }
    (verts, tris)
}

pub fn to_mesh(v: &SparseVolume, iso: f32, method: &str, node: &str) -> Res {
    let (verts, polys) = match method {
        "nets" => surface_nets(v, iso),
        "tets" => marching_tets(v, iso),
        "auto" => {
            let (vs, qs) = surface_nets(v, iso);
            if non_manifold_edges(&qs) > 0 { marching_tets(v, iso) } else { (vs, qs) }
        }
        o => return Err(format!("{node}: unknown method '{o}' (auto | nets | tets)")),
    };
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
    let mut m = Geometry::from_polys(verts, &polys);
    m.point.set("N", Column::from_v3(normals));
    m.detail.insert("sparse_tiles_dense".into(), json!(v.n_dense()));
    m.detail.insert("sparse_tiles_const".into(), json!(v.n_const()));
    m.detail.insert("sparse_megabytes".into(), json!((v.memory_bytes() as f64 / 1.0e6 * 100.0).round() / 100.0));
    Ok(m)
}

fn remesh(src: &Geometry, p: &Params) -> Res {
    let (voxel, band) = params(p, "sparse_remesh")?;
    let offset = p.f("offset");
    let reach = band as f32 * voxel - 1.5 * voxel;
    if offset.abs() > reach {
        return Err(format!("sparse_remesh: |offset| = {} m exceeds what the band can hold ({reach} m); raise `band` or lower `offset`", offset.abs()));
    }
    let vol = build_sdf(src, voxel, band, "sparse_remesh")?;
    // The surface moves outward by `offset` when we extract the iso level +offset of the signed distance.
    to_mesh(&vol, offset, &p.s("method"), "sparse_remesh")
}

fn boolean(a: &Geometry, b: &Geometry, p: &Params) -> Res {
    let (voxel, band) = params(p, "sparse_boolean")?;
    let k = p.f("k");
    if k > band as f32 * voxel {
        return Err(format!("sparse_boolean: k = {k} m exceeds the band ({} m); raise `band` or lower `k`", band as f32 * voxel));
    }
    let f = combine_fn(&p.s("op"), k, "sparse_boolean")?;
    let (va, vb) = (build_sdf(a, voxel, band, "sparse_boolean")?, build_sdf(b, voxel, band, "sparse_boolean")?);
    let out = combine(&va, &vb, &*f);
    to_mesh(&out, 0.0, &p.s("method"), "sparse_boolean")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook;
    use serde_json::{json, Value};

    fn run(kind: &str, set: &[(&str, Value)], ins: &[&Geometry]) -> Geometry {
        let mut p = Params::new();
        for s in crate::schema(kind).unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        for (k, v) in set {
            p.insert((*k).into(), v.clone());
        }
        let ins: Vec<Option<Arc<Geometry>>> = ins.iter().map(|g| Some(Arc::new((*g).clone()))).collect();
        cook(kind, &p, &ins).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    fn sphere(r: f32, center: [f32; 3]) -> Geometry {
        run("sphere", &[("radius", json!(r)), ("rows", json!(48)), ("cols", json!(96)), ("center", json!(center))], &[])
    }

    fn volume(g: &Geometry) -> f32 {
        let p = g.p();
        g.triangles().iter().map(|t| {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let cr = cross(b, c);
            (a[0] * cr[0] + a[1] * cr[1] + a[2] * cr[2]) / 6.0
        }).sum()
    }

    fn watertight(g: &Geometry) -> bool {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for i in 0..g.n_prims() {
            let pr = g.prim_points(i);
            for k in 0..pr.len() {
                let (a, b) = (pr[k], pr[(k + 1) % pr.len()]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        !edges.is_empty() && edges.values().all(|&c| c == 2)
    }

    #[test]
    fn sparse_sdf_matches_the_analytic_sphere_and_fills_the_interior() {
        let (r, voxel) = (2.5f32, 0.1f32);
        let v = build_sdf(&sphere(r, [0.0; 3]), voxel, 3, "t").unwrap();
        let mut worst = 0.0f32;
        for k in -32..=32 {
            for j in -32..=32 {
                for i in -32..=32 {
                    let pos = v.pos(i, j, k);
                    let exact = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt() - r;
                    let want = exact.clamp(-0.3, 0.3);
                    let got = v.get(i, j, k);
                    if exact.abs() < 0.3 - 0.02 {
                        worst = worst.max((got - want).abs());
                    } else {
                        assert_eq!(got.signum(), want.signum(), "sign at ({i},{j},{k}) exact {exact}");
                    }
                }
            }
        }
        assert!(worst < 0.01, "narrow-band error {worst}");
        assert_eq!(v.get(0, 0, 0), -0.3, "deep interior stores the clamped negative value");
        assert_eq!(v.get(100, 100, 100), 0.3);
        assert!(v.n_const() > 0, "interior blocks must be constant tiles");
        assert!(v.n_dense() < 600, "only a shell of tiles is dense: {}", v.n_dense());
    }

    #[test]
    fn sparse_mesher_agrees_with_the_dense_mesher_on_identical_data() {
        let v = build_sdf(&sphere(1.0, [0.2, 0.1, -0.1]), 0.08, 3, "t").unwrap();
        let (lo, hi) = v.index_bounds().unwrap();
        let dense = to_dense(&v, lo, hi);
        let (sv, sq) = surface_nets(&v, 0.0);
        let (dv, dq) = crate::sdf::surface_nets(&dense, 0.0);
        assert_eq!((sv.len(), sq.len()), (dv.len(), dq.len()));
        let key = |p: &[f32; 3]| [(p[0] * 1000.0).round() as i64, (p[1] * 1000.0).round() as i64, (p[2] * 1000.0).round() as i64];
        let (mut a, mut b): (Vec<_>, Vec<_>) = (sv.iter().map(key).collect(), dv.iter().map(key).collect());
        a.sort();
        b.sort();
        let off = a.iter().zip(&b).filter(|(x, y)| (0..3).any(|c| (x[c] - y[c]).abs() > 1)).count();
        assert_eq!(off, 0, "{off} vertices differ between sparse and dense meshing");
        // Marching tetrahedra too.
        let (tv, tt) = marching_tets(&v, 0.0);
        let (dtv, dtt) = crate::sdf::marching_tets(&dense, 0.0);
        assert_eq!((tv.len(), tt.len()), (dtv.len(), dtt.len()));
    }

    #[test]
    fn remesh_produces_a_closed_surface_with_the_right_volume_and_offset() {
        let m = run("sparse_remesh", &[("voxel", json!(0.05))], &[&sphere(1.0, [0.0; 3])]);
        assert!(watertight(&m), "remeshed sphere must be closed");
        let v = volume(&m);
        assert!((v - 4.18879).abs() < 0.12, "volume {v}");
        assert!(m.point.get("N").is_some());
        let grown = run("sparse_remesh", &[("voxel", json!(0.05)), ("band", json!(8)), ("offset", json!(0.2))], &[&sphere(1.0, [0.0; 3])]);
        let (lo, hi) = grown.base_bounds().unwrap();
        assert!((hi[0] - 1.2).abs() < 0.04 && (lo[1] + 1.2).abs() < 0.04, "grown bounds {lo:?} {hi:?}");
        let g = &grown;
        assert!(g.detail.get("sparse_tiles_dense").and_then(|v| v.as_u64()).unwrap() > 0);
    }

    #[test]
    fn boolean_volumes_match_closed_form_lens_geometry() {
        let (a, b) = (sphere(1.0, [0.0; 3]), sphere(1.0, [1.0, 0.0, 0.0]));
        let ball = 4.0 / 3.0 * std::f32::consts::PI;
        let lens = std::f32::consts::PI * 5.0 / 12.0; // pi (4r + d)(2r - d)^2 / 12 with r = 1, d = 1
        for (op, want) in [("union", 2.0 * ball - lens), ("intersect", lens), ("subtract", ball - lens)] {
            let m = run("sparse_boolean", &[("op", json!(op)), ("voxel", json!(0.05))], &[&a, &b]);
            let v = volume(&m);
            assert!((v - want).abs() < 0.04 * want, "{op}: volume {v} vs {want}");
            assert!(watertight(&m), "{op} result must be closed");
        }
        // A cavity needs the interior fill: a small sphere fully inside a big one removes volume.
        let big = sphere(2.0, [0.0; 3]);
        let hole = sphere(0.7, [0.0; 3]);
        let m = run("sparse_boolean", &[("op", json!("subtract")), ("voxel", json!(0.08))], &[&big, &hole]);
        let want = 4.0 / 3.0 * std::f32::consts::PI * (8.0 - 0.343);
        assert!((volume(&m) - want).abs() < 0.04 * want, "cavity volume {} vs {want}", volume(&m));
    }

    #[test]
    fn building_is_deterministic_and_validates_parameters() {
        let s = sphere(1.0, [0.3, 0.0, 0.0]);
        let a = build_sdf(&s, 0.07, 3, "t").unwrap();
        let b = build_sdf(&s, 0.07, 3, "t").unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        let mut p = Params::new();
        for sch in crate::schema("sparse_remesh").unwrap().params {
            p.insert(sch.name.into(), sch.default);
        }
        p.insert("band".into(), json!(1));
        assert!(cook("sparse_remesh", &p, &[Some(Arc::new(s.clone()))]).unwrap_err().contains("band"));
        p.insert("band".into(), json!(3));
        p.insert("offset".into(), json!(5.0));
        assert!(cook("sparse_remesh", &p, &[Some(Arc::new(s))]).unwrap_err().contains("offset"));
    }
}
