//! Topology-changing ops: fuse, reverse, triangulate, facet, smooth, peak, polyextrude,
//! Catmull-Clark subdivide and Poisson-style scatter.

use crate::{ps, sch};
use pt_core::geo::gather_groups;
use pt_core::math;
use pt_core::{AttrSet, Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("fuse", "Merge points that snap to the same grid cell of size `distance`; removes collapsed faces.", 1, D::None, G::Whole, vec![ps("distance", json!(0.001), "Snap grid size (m).")]),
        sch("reverse", "Flip primitive winding.", 1, D::Smooth, G::PerPrim, vec![]),
        sch("triangulate", "Split polygons into triangles (fan).", 1, D::Smooth, G::PerPrim, vec![]),
        sch("facet", "Make every polygon own its points (flat shading / detached faces).", 1, D::Smooth, G::PerPrim, vec![]),
        sch("smooth", "Laplacian smoothing of positions.", 1, D::Smooth, G::Whole, vec![
            ps("iterations", json!(5), "Iterations."),
            ps("strength", json!(0.5), "Blend toward neighbor average (0..1)."),
            ps("pin_boundary", json!(1), "1 = keep open-boundary points fixed."),
        ]),
        sch("peak", "Move points along their (area-weighted) normals.", 1, D::Smooth, G::Whole, vec![ps("amount", json!(0.1), "Distance in meters (negative = inward).")]),
        sch("polyextrude", "Extrude every polygon along its normal with optional inset (faces detach from neighbors).", 1, D::Smooth, G::PerPrim, vec![
            ps("distance", json!(0.2), "Extrusion distance (m)."),
            ps("inset", json!(0.0), "Shrink the new face toward its center (0..1)."),
        ]),
        sch("subdivide", "Catmull-Clark subdivision (levels 1-4). Interpolates point attributes; drops primitive attributes; segments are dropped.", 1, D::Smooth, G::Whole, vec![ps("levels", json!(1), "Subdivision levels (1-4).")]),
        sch("scatter_poisson", "Scatter points on a surface with a minimum spacing (dart throwing, deterministic). Writes `N` and `pscale`.", 1, D::None, G::Whole, vec![
            ps("count", json!(100), "Maximum points."),
            ps("min_dist", json!(0.5), "Minimum distance between points (m)."),
            ps("seed", json!(1), "Seed."),
            ps("tries", json!(10), "Candidates per wanted point."),
            ps("pscale_min", json!(1.0), "Min random pscale."),
            ps("pscale_max", json!(1.0), "Max random pscale."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "fuse" => inp(0).map(|g| fuse(g, p)),
        "reverse" => inp(0).map(reverse),
        "triangulate" => inp(0).map(|g| g.with_polys(&g.triangles().iter().map(|t| t.to_vec()).collect::<Vec<_>>())),
        "facet" => inp(0).map(facet),
        "smooth" => inp(0).map(|g| smooth(g, p)),
        "peak" => inp(0).map(|g| peak(g, p)),
        "polyextrude" => inp(0).map(|g| polyextrude(g, p)),
        "subdivide" => inp(0).map(|g| subdivide(g, p.i("levels").clamp(1, 4) as usize)),
        "scatter_poisson" => inp(0).and_then(|g| scatter_poisson(g, p)),
        _ => return None,
    })
}

fn fuse(src: &Geometry, p: &Params) -> Geometry {
    let tol = p.f("distance").max(1e-9);
    let mut map: HashMap<[i64; 3], u32> = HashMap::new();
    let (mut remap, mut keep) = (Vec::with_capacity(src.n_points()), Vec::<u32>::new());
    for (i, q) in src.p().iter().enumerate() {
        let k = [(q[0] / tol).round() as i64, (q[1] / tol).round() as i64, (q[2] / tol).round() as i64];
        let id = *map.entry(k).or_insert_with(|| {
            keep.push(i as u32);
            keep.len() as u32 - 1
        });
        remap.push(id);
    }
    let (mut polys, mut kept_prims) = (Vec::new(), Vec::<u32>::new());
    for i in 0..src.n_prims() {
        let orig = src.prim_points(i);
        let mut v: Vec<u32> = Vec::new();
        for &o in orig {
            let m = remap[o as usize];
            if v.last() != Some(&m) {
                v.push(m);
            }
        }
        if v.len() > 1 && v.first() == v.last() {
            v.pop();
        }
        let min = if orig.len() >= 3 { 3 } else { 2 };
        if v.len() >= min {
            polys.push(v);
            kept_prims.push(i as u32);
        }
    }
    let pts: Vec<[f32; 3]> = keep.iter().map(|&i| src.p()[i as usize]).collect();
    let mut g = Geometry::from_polys(pts, &polys);
    g.point = src.point.gather(&keep);
    g.prim = src.prim.gather(&kept_prims);
    g.prim_groups = gather_groups(&src.prim_groups, &kept_prims);
    g.point_groups = gather_groups(&src.point_groups, &keep);
    g.detail = src.detail.clone();
    g.instances = src.instances.clone();
    g
}

fn reverse(src: &Geometry) -> Geometry {
    let polys: Vec<Vec<u32>> = (0..src.n_prims()).map(|i| src.prim_points(i).iter().rev().copied().collect()).collect();
    let mut g = src.with_polys(&polys);
    g.prim = src.prim.clone();
    g.prim_groups = src.prim_groups.clone();
    g
}

fn facet(src: &Geometry) -> Geometry {
    let mut orig: Vec<u32> = Vec::with_capacity(src.n_vertices());
    let mut polys = Vec::with_capacity(src.n_prims());
    for i in 0..src.n_prims() {
        let mut poly = Vec::new();
        for &pt in src.prim_points(i) {
            poly.push(orig.len() as u32);
            orig.push(pt);
        }
        polys.push(poly);
    }
    let pts: Vec<[f32; 3]> = orig.iter().map(|&i| src.p()[i as usize]).collect();
    let mut g = Geometry::from_polys(pts, &polys);
    g.point = src.point.gather(&orig);
    g.point_groups = gather_groups(&src.point_groups, &orig);
    g.prim = src.prim.clone();
    g.prim_groups = src.prim_groups.clone();
    g.detail = src.detail.clone();
    g.instances = src.instances.clone();
    g
}

fn neighbors(src: &Geometry) -> (Vec<Vec<u32>>, Vec<bool>) {
    let n = src.n_points();
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for i in 0..src.n_prims() {
        let pr = src.prim_points(i);
        for k in 0..pr.len() {
            let (a, b) = (pr[k], pr[(k + 1) % pr.len()]);
            if pr.len() == 2 && k == 1 {
                continue;
            }
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let mut adj = vec![Vec::new(); n];
    let mut boundary = vec![false; n];
    let mut sorted: Vec<_> = edges.into_iter().collect();
    sorted.sort_unstable();
    for ((a, b), c) in sorted {
        adj[a as usize].push(b);
        adj[b as usize].push(a);
        if c == 1 {
            boundary[a as usize] = true;
            boundary[b as usize] = true;
        }
    }
    (adj, boundary)
}

fn smooth(src: &Geometry, p: &Params) -> Geometry {
    let (adj, boundary) = neighbors(src);
    let (iters, s) = (p.i("iterations").clamp(0, 200), p.f("strength").clamp(0.0, 1.0));
    let pin = p.i("pin_boundary") != 0;
    let mut cur = src.p().to_vec();
    for _ in 0..iters {
        let prev = cur.clone();
        for i in 0..cur.len() {
            if adj[i].is_empty() || (pin && boundary[i]) {
                continue;
            }
            let mut avg = [0.0f32; 3];
            for &n in &adj[i] {
                for c in 0..3 {
                    avg[c] += prev[n as usize][c];
                }
            }
            let inv = 1.0 / adj[i].len() as f32;
            for c in 0..3 {
                cur[i][c] = prev[i][c] + (avg[c] * inv - prev[i][c]) * s;
            }
        }
    }
    let mut g = src.clone();
    *g.p_mut() = cur;
    g
}

fn peak(src: &Geometry, p: &Params) -> Geometry {
    let n = crate::point_normals(src);
    let a = p.f("amount");
    let mut g = src.clone();
    for (q, n) in g.p_mut().iter_mut().zip(n) {
        *q = [q[0] + n[0] * a, q[1] + n[1] * a, q[2] + n[2] * a];
    }
    g
}

fn polyextrude(src: &Geometry, p: &Params) -> Geometry {
    let (dist, inset) = (p.f("distance"), p.f("inset").clamp(0.0, 1.0));
    let sp = src.p();
    let mut pts = sp.to_vec();
    let mut orig: Vec<u32> = (0..sp.len() as u32).collect();
    let (mut polys, mut from_prim) = (Vec::<Vec<u32>>::new(), Vec::<u32>::new());
    for i in 0..src.n_prims() {
        let pr = src.prim_points(i);
        if pr.len() < 3 {
            polys.push(pr.to_vec());
            from_prim.push(i as u32);
            continue;
        }
        let p0 = sp[pr[0] as usize];
        let mut cr = [0.0f32; 3];
        let mut c = [0.0f32; 3];
        for k in 0..pr.len() {
            let q = sp[pr[k] as usize];
            for a in 0..3 {
                c[a] += q[a];
            }
            if k >= 1 && k + 1 < pr.len() {
                let x = math::cross(math::sub(q, p0), math::sub(sp[pr[k + 1] as usize], p0));
                for a in 0..3 {
                    cr[a] += x[a];
                }
            }
        }
        let n = math::normalize(cr);
        let inv = 1.0 / pr.len() as f32;
        let c = [c[0] * inv, c[1] * inv, c[2] * inv];
        let mut top = Vec::new();
        for &pt in pr {
            let q = sp[pt as usize];
            let np = [0, 1, 2].map(|a| q[a] + (c[a] - q[a]) * inset + n[a] * dist);
            top.push(pts.len() as u32);
            pts.push(np);
            orig.push(pt);
        }
        for k in 0..pr.len() {
            let k1 = (k + 1) % pr.len();
            polys.push(vec![pr[k], pr[k1], top[k1], top[k]]);
            from_prim.push(i as u32);
        }
        polys.push(top);
        from_prim.push(i as u32);
    }
    let mut g = Geometry::from_polys(pts, &polys);
    let new_p = g.p().to_vec();
    g.point = src.point.gather(&orig);
    g.point.set("P", Column::from_v3(new_p));
    g.point_groups = gather_groups(&src.point_groups, &orig);
    g.prim = src.prim.gather(&from_prim);
    g.prim_groups = gather_groups(&src.prim_groups, &from_prim);
    g.detail = src.detail.clone();
    g.instances = src.instances.clone();
    g
}

fn interp_attrs(old: &AttrSet, weights: &[Vec<(u32, f32)>]) -> AttrSet {
    let mut out = AttrSet::default();
    for (name, col) in old.iter() {
        let c = match col {
            Column::F32(v) => Column::from_f32(weights.iter().map(|w| w.iter().map(|&(i, k)| v[i as usize] * k).sum()).collect()),
            Column::V3(v) => Column::from_v3(weights.iter().map(|w| {
                let mut a = [0.0f32; 3];
                for &(i, k) in w {
                    for c in 0..3 {
                        a[c] += v[i as usize][c] * k;
                    }
                }
                a
            }).collect()),
            other => other.gather(&weights.iter().map(|w| w[0].0).collect::<Vec<_>>()),
        };
        out.set(name, c);
    }
    out
}

fn subdivide(src: &Geometry, levels: usize) -> Geometry {
    let mut g = src.clone();
    for _ in 0..levels {
        g = catmull_clark(&g);
    }
    g
}

fn catmull_clark(src: &Geometry) -> Geometry {
    let sp = src.p();
    let n_old = sp.len();
    let faces: Vec<&[u32]> = (0..src.n_prims()).map(|i| src.prim_points(i)).filter(|f| f.len() >= 3).collect();
    // Edges and adjacency.
    let mut edge_id: HashMap<(u32, u32), usize> = HashMap::new();
    let mut edges: Vec<(u32, u32, Vec<usize>)> = Vec::new();
    let mut vert_faces = vec![Vec::<usize>::new(); n_old];
    let mut vert_edges = vec![Vec::<usize>::new(); n_old];
    for (fi, f) in faces.iter().enumerate() {
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            let key = (a.min(b), a.max(b));
            let id = *edge_id.entry(key).or_insert_with(|| {
                edges.push((key.0, key.1, Vec::new()));
                vert_edges[key.0 as usize].push(edges.len() - 1);
                vert_edges[key.1 as usize].push(edges.len() - 1);
                edges.len() - 1
            });
            edges[id].2.push(fi);
            vert_faces[a as usize].push(fi);
        }
    }
    let avg = |ids: &[u32]| -> [f32; 3] {
        let mut a = [0.0f32; 3];
        for &i in ids {
            for c in 0..3 {
                a[c] += sp[i as usize][c];
            }
        }
        let inv = 1.0 / ids.len() as f32;
        [a[0] * inv, a[1] * inv, a[2] * inv]
    };
    let fpts: Vec<[f32; 3]> = faces.iter().map(|f| avg(f)).collect();
    let epts: Vec<[f32; 3]> = edges.iter().map(|(a, b, fs)| {
        let (pa, pb) = (sp[*a as usize], sp[*b as usize]);
        if fs.len() == 2 {
            [0, 1, 2].map(|c| (pa[c] + pb[c] + fpts[fs[0]][c] + fpts[fs[1]][c]) * 0.25)
        } else {
            [0, 1, 2].map(|c| (pa[c] + pb[c]) * 0.5)
        }
    }).collect();
    let mut vpts = Vec::with_capacity(n_old);
    for v in 0..n_old {
        let pv = sp[v];
        let es = &vert_edges[v];
        if es.is_empty() {
            vpts.push(pv);
            continue;
        }
        let bnd: Vec<usize> = es.iter().copied().filter(|&e| edges[e].2.len() == 1).collect();
        if !bnd.is_empty() {
            if bnd.len() == 2 {
                let other = |e: usize| if edges[e].0 as usize == v { edges[e].1 } else { edges[e].0 };
                let (a, b) = (sp[other(bnd[0]) as usize], sp[other(bnd[1]) as usize]);
                vpts.push([0, 1, 2].map(|c| (6.0 * pv[c] + a[c] + b[c]) * 0.125));
            } else {
                vpts.push(pv);
            }
            continue;
        }
        let n = es.len() as f32;
        let fl = &vert_faces[v];
        let mut favg = [0.0f32; 3];
        for &f in fl {
            for c in 0..3 {
                favg[c] += fpts[f][c];
            }
        }
        let mut ravg = [0.0f32; 3];
        for &e in es {
            let (pa, pb) = (sp[edges[e].0 as usize], sp[edges[e].1 as usize]);
            for c in 0..3 {
                ravg[c] += (pa[c] + pb[c]) * 0.5;
            }
        }
        let (fi, ri) = (1.0 / fl.len().max(1) as f32, 1.0 / n);
        vpts.push([0, 1, 2].map(|c| (favg[c] * fi + 2.0 * ravg[c] * ri + (n - 3.0) * pv[c]) / n));
    }
    let mut pts = vpts;
    pts.extend(epts);
    pts.extend(fpts);
    let mut weights: Vec<Vec<(u32, f32)>> = (0..n_old as u32).map(|i| vec![(i, 1.0)]).collect();
    weights.extend(edges.iter().map(|(a, b, _)| vec![(*a, 0.5), (*b, 0.5)]));
    weights.extend(faces.iter().map(|f| f.iter().map(|&i| (i, 1.0 / f.len() as f32)).collect()));
    let eoff = n_old as u32;
    let foff = (n_old + edges.len()) as u32;
    let mut polys = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        let m = f.len();
        let e = |a: u32, b: u32| eoff + edge_id[&(a.min(b), a.max(b))] as u32;
        for k in 0..m {
            let (prev, cur, next) = (f[(k + m - 1) % m], f[k], f[(k + 1) % m]);
            polys.push(vec![cur, e(cur, next), foff + fi as u32, e(prev, cur)]);
        }
    }
    let mut g = Geometry::from_polys(pts, &polys);
    g.point = interp_attrs(&src.point, &weights);
    g.point.set("P", Column::from_v3(g.p().to_vec()));
    g.detail = src.detail.clone();
    g.instances = src.instances.clone();
    g
}

fn scatter_poisson(src: &Geometry, p: &Params) -> Res {
    let count = p.i("count").max(0) as usize;
    let md = p.f("min_dist").max(1e-6);
    let cands = crate::surface_samples(src, count * p.i("tries").clamp(1, 100) as usize, p.i("seed"))?;
    let (lo, hi) = (p.f("pscale_min"), p.f("pscale_max"));
    let key = |q: [f32; 3]| [(q[0] / md).floor() as i32, (q[1] / md).floor() as i32, (q[2] / md).floor() as i32];
    let mut grid: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
    let (mut pos, mut nrm, mut sc) = (Vec::new(), Vec::new(), Vec::new());
    for (q, n, r) in cands {
        if pos.len() >= count {
            break;
        }
        let k = key(q);
        let mut ok = true;
        'outer: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &i in list {
                            let d = math::sub(pos[i], q);
                            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < md * md {
                                ok = false;
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
        if ok {
            grid.entry(k).or_default().push(pos.len());
            pos.push(q);
            nrm.push(n);
            sc.push(lo + (hi - lo) * r[3]);
        }
    }
    let mut g = Geometry::from_points(pos);
    g.point.set("N", Column::from_v3(nrm));
    g.point.set("pscale", Column::from_f32(sc));
    Ok(g)
}
