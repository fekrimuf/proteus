//! Simulation nodes. Each is a pure function of (inputs, params, `__frame`): the solver runs
//! from the start frame up to the current frame, so scrubbing or cooking any frame is
//! reproducible, and results are content-addressed in the graph cache like every other node.

use crate::{ps, sch};
use pt_core::math;
use pt_core::rng::rand4;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params, Volume};
use pt_sim::{cloth, pop, rbd, smoke};
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

const MAX_SIM_FRAMES: usize = 2000;
const MAX_SMOKE_CELLS: usize = 3_000_000;

pub fn schemas() -> Vec<NodeSchema> {
    let start = || ps("start_frame", json!(1), "Frame at which the simulation starts (state at this frame = initial).");
    vec![
        sch("pop_sim", "Particles from the points of input 0 (direction = point N, else +Y). Optional input 1: an 'sdf' collider. Simulated up to the current frame; writes P, v, age, id, pscale.", 2, D::None, G::Whole, vec![
            start(),
            ps("fps", json!(24.0), "Frames per second."),
            ps("substeps", json!(2), "Solver substeps per frame."),
            ps("gravity", json!([0.0, -9.81, 0.0]), "Gravity (m/s^2)."),
            ps("drag", json!(0.1), "Drag toward the wind velocity."),
            ps("wind", json!([0.0, 0.0, 0.0]), "Wind velocity."),
            ps("noise_amp", json!(0.0), "Turbulence acceleration amplitude."),
            ps("noise_freq", json!(1.0), "Turbulence spatial frequency."),
            ps("speed", json!(6.0), "Emission speed (m/s)."),
            ps("spread", json!(0.15), "0 = along the emitter direction, 1 = random."),
            ps("life", json!(0.0), "Lifetime in seconds (0 = forever)."),
            ps("rate", json!(4.0), "New particles per emitter point per frame."),
            ps("bounce", json!(0.4), "Restitution on collisions."),
            ps("friction", json!(0.1), "Tangential damping on collisions."),
            ps("ground_y", json!(0.0), "Ground plane height."),
            ps("max_particles", json!(200000), "Hard cap on particle count."),
            ps("pscale_min", json!(1.0), "Min random pscale."),
            ps("pscale_max", json!(1.0), "Max random pscale."),
            ps("seed", json!(1), "Seed."),
        ]),
        sch("cloth_sim", "Position-based cloth on the mesh of input 0. Optional input 1: an 'sdf' collider. Pin points via `pin_group` or `pin_top`. Writes updated P.", 2, D::None, G::Whole, vec![
            start(),
            ps("fps", json!(24.0), "Frames per second."),
            ps("substeps", json!(6), "Substeps per frame."),
            ps("iterations", json!(8), "Constraint iterations per substep."),
            ps("gravity", json!([0.0, -9.81, 0.0]), "Gravity."),
            ps("damping", json!(0.02), "Velocity damping per substep."),
            ps("stiffness", json!(0.9), "Stretch stiffness (0..1)."),
            ps("bend_stiffness", json!(0.15), "Bending stiffness (0..1)."),
            ps("wind", json!([0.0, 0.0, 0.0]), "Wind velocity."),
            ps("wind_strength", json!(0.0), "Wind pressure coefficient."),
            ps("thickness", json!(0.02), "Collision offset (m)."),
            ps("friction", json!(0.3), "Ground friction."),
            ps("ground_y", json!(-1000.0), "Ground plane height (very low = none)."),
            ps("pin_group", json!(""), "Point group whose points are pinned."),
            ps("pin_top", json!(0), "1 = also pin the highest points of the mesh."),
            ps("collider_center", json!([0.0, 0.0, 0.0]), "Sphere collider center."),
            ps("collider_radius", json!(0.0), "Sphere collider radius (0 = none)."),
        ]),
        sch("smoke_sim", "Smoke on a dense grid, emitted from the points of input 0. Writes volumes 'density' and 'temperature' (meshable via volume_to_sdf + sdf_to_mesh).", 1, D::None, G::Whole, vec![
            start(),
            ps("fps", json!(24.0), "Frames per second."),
            ps("size", json!([3.0, 4.0, 3.0]), "Domain size in meters."),
            ps("center", json!([0.0, 0.0, 0.0]), "Domain center in x/z; y = domain bottom."),
            ps("voxel", json!(0.08), "Voxel size (m)."),
            ps("buoyancy", json!(4.0), "Upward acceleration per unit temperature."),
            ps("weight", json!(0.2), "Downward acceleration per unit density."),
            ps("dissipation", json!(0.15), "Density fade rate."),
            ps("cooling", json!(0.6), "Temperature fade rate."),
            ps("vorticity", json!(0.5), "Vorticity confinement strength."),
            ps("source_radius", json!(0.25), "Emission radius (m)."),
            ps("source_rate", json!(14.0), "Density added per second at the source."),
            ps("source_temp", json!(1.0), "Source temperature."),
            ps("source_vel", json!([0.0, 0.6, 0.0]), "Source velocity."),
            ps("iterations", json!(40), "Pressure solver iterations."),
        ]),
        sch("volume_to_sdf", "Turn a scalar volume (e.g. smoke density) into an 'sdf' by thresholding, ready for sdf_to_mesh.", 1, D::Piecewise, G::Whole, vec![
            ps("volume", json!("density"), "Source volume name."),
            ps("threshold", json!(0.3), "Iso level: inside where the volume exceeds it."),
        ]),
        sch("rbd_shatter", "Voronoi-fracture a closed mesh into `pieces` closed pieces (via SDFs). Writes prim attr `piece` (index).", 1, D::None, G::Whole, vec![
            ps("pieces", json!(24), "Number of pieces (2-200)."),
            ps("seed", json!(1), "Seed for fracture sites."),
            ps("voxel", json!(0.06), "Voxel size (m); smaller = finer fracture surfaces."),
            ps("padding", json!(0.1), "Margin around the mesh (m)."),
        ]),
        sch("rbd_settle", "SET DRESSING: lets the instances of input 0 (copy_to_points, array...) fall onto the surface of input 1 (the terrain, a floor, a table: any mesh) and come to rest, so piles of logs, stones, barrels and crates lie the way real things lie, on slopes, against each other, never floating. Each instance becomes a rigid body (convex hull of its prototype) dropped from where it is (at least `gap` metres above the ground under it: raise instances above the ground to drop a pile from heights); the result keeps the instances, with new positions AND turns, plus instance attributes `settle_move` (metres its origin moved) and `settled` (1 = at rest). Not simulated per frame: it returns the resting poses, deterministic. Only the ground near the bodies is used as the collider.", 2, D::None, G::Whole, vec![
            ps("gap", json!(0.12), "Metres above the ground under each body that it starts from (it falls this far)."),
            ps("frames", json!(300), "Longest simulation (frames at 60 per second); it stops earlier when everything sleeps."),
            ps("rolling", json!(6.0), "Resistance to rolling: 0 lets a log roll down any slope like a real one; 6 keeps dressing where it landed."),
            ps("friction", json!(0.8), "Friction of bodies and ground."),
            ps("restitution", json!(0.05), "Bounciness."),
            ps("density", json!(1500.0), "Density (kg/m^3): only matters between bodies that touch."),
            ps("hull_points", json!(160), "Points per prototype hull (the prototype's points are sampled; more = rounder, slower)."),
            ps("margin", json!(4.0), "Ground triangles within this many metres of a body are the collider."),
            ps("drop_lost", json!(1), "1 = remove bodies that did not come to rest or have no ground under them."),
        ]),
        sch("rbd_sim", "Rigid-body simulation of the disconnected pieces of input 0 (convex hulls, ground plane, optional radial impact).", 1, D::None, G::Whole, vec![
            start(),
            ps("fps", json!(24.0), "Frames per second."),
            ps("substeps", json!(4), "Substeps per frame."),
            ps("gravity", json!([0.0, -9.81, 0.0]), "Gravity."),
            ps("ground_y", json!(0.0), "Ground height."),
            ps("friction", json!(0.7), "Friction."),
            ps("restitution", json!(0.15), "Bounciness."),
            ps("density", json!(1000.0), "Density (kg/m^3)."),
            ps("impact_center", json!([0.0, 0.0, 0.0]), "Impact point."),
            ps("impact_radius", json!(0.0), "Impact radius (0 = none)."),
            ps("impact_strength", json!(0.0), "Launch speed at the impact center (m/s)."),
            ps("spin", json!(2.0), "Random initial spin."),
            ps("seed", json!(1), "Seed."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "pop_sim" => inp(0).and_then(|e| pop_sim(e, inputs.get(1).and_then(|g| g.as_deref()), p)),
        "cloth_sim" => inp(0).and_then(|m| cloth_sim(m, inputs.get(1).and_then(|g| g.as_deref()), p)),
        "smoke_sim" => inp(0).and_then(|s| smoke_sim(s, p)),
        "volume_to_sdf" => inp(0).and_then(|g| volume_to_sdf(g, p)),
        "rbd_shatter" => inp(0).and_then(|g| shatter(g, p)),
        "rbd_sim" => inp(0).and_then(|g| rbd_sim(g, p)),
        "rbd_settle" => inp(0).and_then(|g| inp(1).and_then(|ground| rbd_settle(g, ground, p))),
        _ => return None,
    })
}

fn elapsed_frames(p: &Params) -> usize {
    let f = p.f("__frame");
    let f = if f == 0.0 { 1.0 } else { f };
    ((f.round() - p.f("start_frame").round()).max(0.0) as usize).min(MAX_SIM_FRAMES)
}

fn collider_of(g: Option<&Geometry>) -> Result<Option<Arc<Volume>>, String> {
    match g {
        None => Ok(None),
        Some(g) => g.volumes.get("sdf").cloned().map(Some).ok_or_else(|| "collider input has no 'sdf' volume (build one with sdf_from_mesh or sdf_shape)".to_string()),
    }
}

fn pop_sim(em: &Geometry, collider: Option<&Geometry>, p: &Params) -> Res {
    let pos = em.p().to_vec();
    if pos.is_empty() {
        return Err("pop_sim: emitter input has no points".into());
    }
    let dir: Vec<[f32; 3]> = match em.point.get("N").and_then(Column::as_v3) {
        Some(n) => n.iter().map(|v| math::normalize(*v)).collect(),
        None => vec![[0.0, 1.0, 0.0]; pos.len()],
    };
    let coll = collider_of(collider)?;
    let prm = pop::PopParams {
        frame: elapsed_frames(p),
        fps: p.f("fps"),
        substeps: p.i("substeps").clamp(1, 32) as usize,
        gravity: p.v3("gravity"),
        drag: p.f("drag"),
        wind: p.v3("wind"),
        noise_amp: p.f("noise_amp"),
        noise_freq: p.f("noise_freq"),
        speed: p.f("speed"),
        spread: p.f("spread").clamp(0.0, 1.0),
        life: p.f("life"),
        rate: p.f("rate").max(0.0),
        bounce: p.f("bounce").clamp(0.0, 1.0),
        friction: p.f("friction").clamp(0.0, 1.0),
        ground_y: p.f("ground_y"),
        max_particles: p.i("max_particles").clamp(1, 2_000_000) as usize,
        seed: p.i("seed"),
    };
    let s = pop::simulate(&pop::Emitter { pos, dir }, coll.as_deref(), &prm);
    let (lo, hi) = (p.f("pscale_min"), p.f("pscale_max"));
    let mut g = Geometry::from_points(s.p);
    g.point.set("v", Column::from_v3(s.v));
    g.point.set("age", Column::from_f32(s.age));
    g.point.set("pscale", Column::from_f32(s.id.iter().map(|i| lo + (hi - lo) * rand4(prm.seed, 33, *i as u64)[0]).collect()));
    g.point.set("id", Column::from_f32(s.id.iter().map(|i| *i as f32).collect()));
    Ok(g)
}

fn cloth_sim(src: &Geometry, collider: Option<&Geometry>, p: &Params) -> Res {
    let tris = src.triangles();
    if tris.is_empty() {
        return Err("cloth_sim: input has no polygons".into());
    }
    let pts = src.p().to_vec();
    let mut pinned = vec![false; pts.len()];
    let group = p.s("pin_group");
    if !group.is_empty() {
        let bits = src.point_groups.get(&group).ok_or_else(|| format!("cloth_sim: point group '{group}' does not exist (available: {:?})", src.point_groups.keys().collect::<Vec<_>>()))?;
        for (i, b) in bits.iter().enumerate() {
            pinned[i] |= *b;
        }
    }
    if p.i("pin_top") != 0 {
        let ymax = pts.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max);
        for (i, q) in pts.iter().enumerate() {
            pinned[i] |= q[1] >= ymax - 1e-4;
        }
    }
    if pinned.iter().all(|b| *b) {
        return Err("cloth_sim: every point is pinned, so nothing can move. pin_top pins the highest points: use a hanging (vertical) sheet, or pin a subset with pin_group".into());
    }
    if !pinned.iter().any(|b| *b) && p.f("ground_y") < -100.0 && p.f("collider_radius") <= 0.0 && collider.is_none() {
        // Nothing holds or catches it: it will free-fall forever. Still valid, so just proceed.
    }
    let coll = collider_of(collider)?;
    let mut cols: Vec<cloth::Collider> = Vec::new();
    if p.f("collider_radius") > 0.0 {
        cols.push(cloth::Collider::Sphere { center: p.v3("collider_center"), radius: p.f("collider_radius") });
    }
    if let Some(v) = &coll {
        cols.push(cloth::Collider::Sdf(v));
    }
    let prm = cloth::ClothParams {
        frame: elapsed_frames(p),
        fps: p.f("fps"),
        substeps: p.i("substeps").clamp(1, 64) as usize,
        iterations: p.i("iterations").clamp(1, 64) as usize,
        gravity: p.v3("gravity"),
        damping: p.f("damping").clamp(0.0, 1.0),
        stiffness: p.f("stiffness"),
        bend_stiffness: p.f("bend_stiffness"),
        wind: p.v3("wind"),
        wind_strength: p.f("wind_strength"),
        thickness: p.f("thickness").max(0.0),
        friction: p.f("friction").clamp(0.0, 1.0),
        ground_y: p.f("ground_y"),
    };
    let out = cloth::simulate(&cloth::ClothMesh { p: pts, tris, pinned }, &cols, &prm);
    let mut g = src.clone();
    *g.p_mut() = out;
    Ok(g)
}

fn smoke_sim(src: &Geometry, p: &Params) -> Res {
    let sources = src.p().to_vec();
    if sources.is_empty() {
        return Err("smoke_sim: source input has no points".into());
    }
    let (size, c, voxel) = (p.v3("size"), p.v3("center"), p.f("voxel").max(1e-3));
    let dims = [0, 1, 2].map(|a| (size[a] / voxel).ceil().max(4.0) as usize + 1);
    let cells = dims[0] * dims[1] * dims[2];
    if cells > MAX_SMOKE_CELLS {
        return Err(format!("smoke_sim: {}x{}x{} = {cells} cells exceeds {MAX_SMOKE_CELLS}; increase `voxel` or shrink `size`", dims[0], dims[1], dims[2]));
    }
    let origin = [c[0] - size[0] * 0.5, c[1], c[2] - size[2] * 0.5];
    let prm = smoke::SmokeParams {
        dims,
        origin,
        voxel,
        frame: elapsed_frames(p),
        fps: p.f("fps"),
        substeps: 1,
        buoyancy: p.f("buoyancy"),
        weight: p.f("weight"),
        dissipation: p.f("dissipation"),
        cooling: p.f("cooling"),
        vorticity: p.f("vorticity"),
        source_radius: p.f("source_radius"),
        source_rate: p.f("source_rate"),
        source_temp: p.f("source_temp"),
        source_vel: p.v3("source_vel"),
        iterations: p.i("iterations").clamp(1, 200) as usize,
    };
    let s = smoke::simulate(&sources, &prm);
    let mut g = Geometry::new();
    g.volumes.insert("density".into(), Arc::new(Volume { dims, origin, voxel, data: Arc::new(s.density) }));
    g.volumes.insert("temperature".into(), Arc::new(Volume { dims, origin, voxel, data: Arc::new(s.temp) }));
    Ok(g)
}

fn volume_to_sdf(g: &Geometry, p: &Params) -> Res {
    let name = p.s("volume");
    let v = g.volumes.get(&name).ok_or_else(|| format!("volume_to_sdf: no volume '{name}' (available: {:?})", g.volumes.keys().collect::<Vec<_>>()))?;
    let t = p.f("threshold");
    let mut out = g.clone();
    out.volumes.insert("sdf".into(), Arc::new(Volume { dims: v.dims, origin: v.origin, voxel: v.voxel, data: Arc::new(v.data.iter().map(|d| (t - d) * v.voxel).collect()) }));
    Ok(out)
}

fn shatter(src: &Geometry, p: &Params) -> Res {
    let n = p.i("pieces").clamp(2, 200) as usize;
    let (voxel, seed) = (p.f("voxel").max(0.01), p.i("seed"));
    let mut fp = Params::new();
    fp.insert("voxel".into(), json!(voxel));
    fp.insert("padding".into(), json!(p.f("padding").max(0.0) + 3.0 * voxel));
    fp.insert("band".into(), json!(3));
    let sdf_geo = crate::sdf::from_mesh_pub(src, &fp)?;
    let vol = sdf_geo.volumes.get("sdf").unwrap().clone();
    let (lo, hi) = src.base_bounds().ok_or("rbd_shatter: empty input")?;
    // Fracture sites: random points inside the solid.
    let mut sites: Vec<[f32; 3]> = Vec::new();
    for t in 0..(n * 400) as u64 {
        if sites.len() >= n {
            break;
        }
        let r = rand4(seed, 41, t);
        let q = [lo[0] + (hi[0] - lo[0]) * r[0], lo[1] + (hi[1] - lo[1]) * r[1], lo[2] + (hi[2] - lo[2]) * r[2]];
        if vol.sample(q) < -voxel * 0.5 {
            sites.push(q);
        }
    }
    if sites.len() < 2 {
        return Err("rbd_shatter: could not place fracture sites inside the mesh; is it a closed, watertight solid?".into());
    }
    let [nx, ny, nz] = vol.dims;
    let total = nx * ny * nz;
    let mut nearest = vec![(0u32, f32::INFINITY, f32::INFINITY); total];
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let q = vol.pos(i, j, k);
                let (mut b1, mut d1, mut d2) = (0u32, f32::INFINITY, f32::INFINITY);
                for (si, s) in sites.iter().enumerate() {
                    let d = math::sub(q, *s);
                    let dd = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if dd < d1 {
                        d2 = d1;
                        d1 = dd;
                        b1 = si as u32;
                    } else if dd < d2 {
                        d2 = dd;
                    }
                }
                nearest[(k * ny + j) * nx + i] = (b1, d1, d2);
            }
        }
    }
    let mut out = Geometry::new();
    for (si, site) in sites.iter().enumerate() {
        // Bounding box of the voxels owned by this site, plus a safety margin.
        let (mut mn, mut mx) = ([usize::MAX; 3], [0usize; 3]);
        let mut any = false;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let idx = (k * ny + j) * nx + i;
                    if nearest[idx].0 as usize == si && vol.data[idx] < 0.0 {
                        any = true;
                        for (a, v) in [i, j, k].iter().enumerate() {
                            mn[a] = mn[a].min(*v);
                            mx[a] = mx[a].max(*v);
                        }
                    }
                }
            }
        }
        if !any {
            continue;
        }
        let m = 3usize;
        let lo_i = [0, 1, 2].map(|a| mn[a].saturating_sub(m));
        let hi_i = [0, 1, 2].map(|a| (mx[a] + m).min(vol.dims[a] - 1));
        let sub_dims = [0, 1, 2].map(|a| hi_i[a] - lo_i[a] + 1);
        let mut sub = Volume::new(sub_dims, vol.pos(lo_i[0], lo_i[1], lo_i[2]), vol.voxel, 1.0);
        let data = Arc::make_mut(&mut sub.data);
        for k in 0..sub_dims[2] {
            for j in 0..sub_dims[1] {
                for i in 0..sub_dims[0] {
                    let (gi, gj, gk) = (lo_i[0] + i, lo_i[1] + j, lo_i[2] + k);
                    let gidx = (gk * ny + gj) * nx + gi;
                    let (b1, d1, d2) = nearest[gidx];
                    let q = vol.pos(gi, gj, gk);
                    let ds = math::sub(q, *site);
                    let di = (ds[0] * ds[0] + ds[1] * ds[1] + ds[2] * ds[2]).sqrt();
                    let other = if b1 as usize == si { d2 } else { d1 };
                    let cell = 0.5 * (di - other);
                    data[(k * sub_dims[1] + j) * sub_dims[0] + i] = vol.data[gidx].max(cell);
                }
            }
        }
        let mut holder = Geometry::new();
        holder.volumes.insert("sdf".into(), Arc::new(sub));
        let mut tp = Params::new();
        tp.insert("iso".into(), json!(0.0));
        tp.insert("method".into(), json!("auto"));
        let mut piece = crate::sdf::to_mesh_pub(&holder, &tp)?;
        let np = piece.n_prims();
        if np == 0 {
            continue;
        }
        piece.prim.set("piece", Column::from_f32(vec![si as f32; np]));
        out.append(&piece)?;
    }
    if out.n_prims() == 0 {
        return Err("rbd_shatter: fracture produced no pieces; try a smaller voxel".into());
    }
    Ok(out)
}

/// The convex shape of a prototype for the physics: its points sampled evenly (and always the extreme ones on every axis, so the
/// hull keeps its size), at most `n`.
fn hull_sample(proto: &Geometry, n: usize) -> Vec<[f32; 3]> {
    let p = proto.p();
    if p.len() <= n {
        return p.to_vec();
    }
    let mut keep: Vec<usize> = (0..n).map(|i| i * p.len() / n).collect();
    for a in 0..3 {
        let (mut lo, mut hi) = (0usize, 0usize);
        for (i, q) in p.iter().enumerate() {
            if q[a] < p[lo][a] {
                lo = i;
            }
            if q[a] > p[hi][a] {
                hi = i;
            }
        }
        keep.push(lo);
        keep.push(hi);
    }
    keep.sort_unstable();
    keep.dedup();
    keep.into_iter().map(|i| p[i]).collect()
}

fn rbd_settle(src: &Geometry, ground: &Geometry, p: &Params) -> Res {
    const MAX_BODIES: usize = 6000;
    let inst = src.instances.as_ref().ok_or("rbd_settle: input 0 has no instances (put copy_to_points or array upstream, or realize the objects you want settled into instances)")?;
    let n = inst.len();
    if n == 0 {
        return Ok(src.clone());
    }
    if n > MAX_BODIES {
        return Err(format!("rbd_settle: {n} bodies exceeds the limit of {MAX_BODIES}; settle the hero piles (a group or a region) and scatter the rest"));
    }
    let tree = crate::refindex::surface_of(ground).ok_or("rbd_settle: input 1 has no surface to land on")?;
    let (gap, margin) = (p.f("gap").max(0.0), p.f("margin").max(0.5));
    let top = ground.base_bounds().map_or(1000.0, |(_, hi)| hi[1] + 10.0);
    let hulls: Vec<Vec<[f32; 3]>> = inst.protos.iter().map(|g| hull_sample(g, p.i("hull_points").clamp(8, 1000) as usize)).collect();

    // each body: the hull as it stands (scale and turn of the instance applied), and its origin lifted to `gap` above the ground
    let lin = |m: &math::Mat34, v: [f32; 3]| [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[4] * v[0] + m[5] * v[1] + m[6] * v[2], m[8] * v[0] + m[9] * v[1] + m[10] * v[2]];
    let mut bodies = Vec::with_capacity(n);
    let mut has_ground = vec![true; n];
    for k in 0..n {
        let xf = inst.xf[k];
        let hull: Vec<[f32; 3]> = hulls[inst.proto_id[k] as usize].iter().map(|v| lin(&xf, *v)).collect();
        let o = [xf[3], xf[7], xf[11]];
        // the highest ground under the footprint (centre and a ring at half the extent), so no side starts inside a slope
        let (mut lo_y, mut hx, mut hz) = (f32::INFINITY, 0.0f32, 0.0f32);
        for h in &hull {
            lo_y = lo_y.min(h[1]);
            hx = hx.max(h[0].abs());
            hz = hz.max(h[2].abs());
        }
        let mut gy = f32::NEG_INFINITY;
        for (sx, sz) in [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5), (0.35, 0.35), (-0.35, 0.35), (0.35, -0.35), (-0.35, -0.35)] {
            if let Some(h) = tree.ray([o[0] + sx * hx, top, o[2] + sz * hz], [0.0, -1.0, 0.0]) {
                gy = gy.max(h.pos[1]);
            } else if sx == 0.0 && sz == 0.0 {
                break;
            }
        }
        // never lower than where it was put (a pile is dropped from heights), never inside the ground: `gap` above it at least
        let origin = if gy.is_finite() {
            [o[0], (gy + gap - lo_y).max(o[1]), o[2]]
        } else {
            has_ground[k] = false;
            o
        };
        bodies.push(rbd::Body { hull, origin });
    }

    // the collider: the ground triangles near the bodies (cells of `margin` metres around every body's origin)
    let gp = ground.p();
    let cell = margin.max(2.0);
    let key = |x: f32, z: f32| ((x / cell).floor() as i32, (z / cell).floor() as i32);
    let mut wanted: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
    for b in bodies.iter() {
        let reach = (b.hull.iter().fold(0.0f32, |m, h| m.max(h[0].abs()).max(h[2].abs())) + margin).max(margin);
        let (c0, c1) = (key(b.origin[0] - reach, b.origin[2] - reach), key(b.origin[0] + reach, b.origin[2] + reach));
        for cx in c0.0..=c1.0 {
            for cz in c0.1..=c1.1 {
                wanted.insert((cx, cz));
            }
        }
    }
    let mut remap = vec![u32::MAX; gp.len()];
    let (mut pts, mut tris) = (Vec::new(), Vec::new());
    for t in ground.triangles() {
        let hit = t.iter().any(|&i| wanted.contains(&key(gp[i as usize][0], gp[i as usize][2])));
        if !hit {
            continue;
        }
        let mut idx = [0u32; 3];
        for (j, &i) in t.iter().enumerate() {
            if remap[i as usize] == u32::MAX {
                remap[i as usize] = pts.len() as u32;
                pts.push(gp[i as usize]);
            }
            idx[j] = remap[i as usize];
        }
        tris.push(idx);
    }
    if tris.is_empty() {
        return Err("rbd_settle: no ground near the bodies".into());
    }
    let prm = rbd::SettleParams {
        frames: p.i("frames").clamp(1, 1200) as usize,
        friction: p.f("friction").clamp(0.0, 2.0),
        restitution: p.f("restitution").clamp(0.0, 1.0),
        density: p.f("density").max(1.0),
        rolling: p.f("rolling"),
        ..Default::default()
    };
    let poses = rbd::settle(&bodies, &pts, &tris, &prm)?;

    // the new transform of instance k: the settled turn applied to the instance's own scale and turn, at the settled position
    let mut xf = (*inst.xf).clone();
    let (mut moved, mut rest) = (Vec::with_capacity(n), Vec::with_capacity(n));
    let top_y = ground.base_bounds().map_or(f32::INFINITY, |(_, hi)| hi[1]);
    let low_y = ground.base_bounds().map_or(f32::NEG_INFINITY, |(lo, _)| lo[1]);
    for k in 0..n {
        let (m, pose) = (xf[k], poses[k]);
        let l = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
        let r = pose.rot;
        let mut nm = [0.0f32; 12];
        for i in 0..3 {
            for j in 0..3 {
                nm[i * 4 + j] = r[i][0] * l[0][j] + r[i][1] * l[1][j] + r[i][2] * l[2][j];
            }
        }
        nm[3] = pose.pos[0];
        nm[7] = pose.pos[1];
        nm[11] = pose.pos[2];
        let d = [pose.pos[0] - m[3], pose.pos[1] - m[7], pose.pos[2] - m[11]];
        moved.push((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
        let ok = has_ground[k] && pose.rest && pose.pos[1] > low_y - 5.0 && pose.pos[1] < top_y + 50.0 && nm.iter().all(|v| v.is_finite());
        rest.push(if ok { 1.0f32 } else { 0.0 });
        if has_ground[k] && ok {
            xf[k] = nm;
        }
    }
    let mut ni = inst.clone();
    ni.xf = Arc::new(xf);
    ni.attrs.set("settle_move", Column::from_f32(moved));
    ni.attrs.set("settled", Column::from_f32(rest.clone()));
    if p.i("drop_lost") != 0 && rest.iter().any(|r| *r == 0.0) {
        let keep: Vec<u32> = (0..n as u32).filter(|&k| rest[k as usize] == 1.0).collect();
        ni.attrs = ni.attrs.gather(&keep);
        ni.proto_id = Arc::new(keep.iter().map(|&k| inst.proto_id[k as usize]).collect());
        ni.xf = Arc::new(keep.iter().map(|&k| ni.xf[k as usize]).collect());
    }
    let mut g = src.clone();
    g.instances = Some(ni);
    Ok(g)
}

fn rbd_sim(src: &Geometry, p: &Params) -> Res {
    if src.n_prims() == 0 {
        return Err("rbd_sim: input has no polygons".into());
    }
    // Pieces = connected components of the primitives (shared points).
    let np = src.n_points();
    let mut parent: Vec<usize> = (0..np).collect();
    fn find(parent: &mut Vec<usize>, mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for i in 0..src.n_prims() {
        let pr = src.prim_points(i);
        for w in pr.windows(2) {
            let (a, b) = (find(&mut parent, w[0] as usize), find(&mut parent, w[1] as usize));
            if a != b {
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut piece_of: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut members: Vec<Vec<u32>> = Vec::new();
    let mut used = vec![false; np];
    for i in 0..src.n_prims() {
        for &pt in src.prim_points(i) {
            used[pt as usize] = true;
        }
    }
    for pt in 0..np {
        if !used[pt] {
            continue;
        }
        let root = find(&mut parent, pt);
        let id = *piece_of.entry(root).or_insert_with(|| {
            members.push(Vec::new());
            members.len() - 1
        });
        members[id].push(pt as u32);
    }
    if members.len() > 5000 {
        return Err(format!("rbd_sim: {} pieces exceeds the limit of 5000", members.len()));
    }
    let rest: Vec<Vec<[f32; 3]>> = members.iter().map(|m| m.iter().map(|&i| src.p()[i as usize]).collect()).collect();
    let prm = rbd::RbdParams {
        frame: elapsed_frames(p),
        fps: p.f("fps"),
        substeps: p.i("substeps").clamp(1, 16) as usize,
        gravity: p.v3("gravity"),
        ground_y: p.f("ground_y"),
        friction: p.f("friction").clamp(0.0, 2.0),
        restitution: p.f("restitution").clamp(0.0, 1.0),
        density: p.f("density").max(1.0),
        impact_center: p.v3("impact_center"),
        impact_radius: p.f("impact_radius"),
        impact_strength: p.f("impact_strength"),
        spin: p.f("spin"),
        seed: p.i("seed"),
    };
    let xf = rbd::simulate(&rest, &prm);
    let mut g = src.clone();
    let pts = g.p_mut();
    for (m, ids) in xf.iter().zip(&members) {
        for &i in ids {
            pts[i as usize] = math::xform_point(m, pts[i as usize]);
        }
    }
    Ok(g)
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

    fn make(kind: &str, set: &[(&str, serde_json::Value)]) -> Geometry {
        run(kind, set, &[])
    }

    fn volume(g: &Geometry) -> f32 {
        let p = g.p();
        g.triangles().iter().map(|t| {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let cr = math::cross(b, c);
            (a[0] * cr[0] + a[1] * cr[1] + a[2] * cr[2]) / 6.0
        }).sum()
    }

    #[test]
    fn particle_node_follows_the_frame_and_is_reproducible() {
        let em = Geometry::from_points(vec![[0.0, 0.2, 0.0]]);
        let a = run("pop_sim", &[("__frame", json!(1.0))], &[&em]);
        assert_eq!(a.n_points(), 0, "frame 1 is the initial state");
        let b = run("pop_sim", &[("__frame", json!(30.0))], &[&em]);
        let c = run("pop_sim", &[("__frame", json!(30.0))], &[&em]);
        assert!(b.n_points() > 50);
        assert_eq!(b.content_hash(), c.content_hash());
        let later = run("pop_sim", &[("__frame", json!(60.0))], &[&em]);
        assert!(later.n_points() > b.n_points());
        assert!(b.point.get("v").is_some() && b.point.get("age").is_some());
    }

    #[test]
    fn cloth_node_pins_and_sags() {
        let grid = make("grid", &[("size", json!([2.0, 2.0])), ("rows", json!(12)), ("cols", json!(12)), ("center", json!([0.0, 2.0, 0.0]))]);
        assert!(crate::cook("cloth_sim", &{ let mut q = Params::new(); for s in crate::schema("cloth_sim").unwrap().params { q.insert(s.name.into(), s.default); } q.insert("pin_top".into(), json!(1)); q }, &[Some(Arc::new(grid.clone()))]).unwrap_err().contains("every point is pinned"));
        let grid = run("transform", &[("r", json!([90.0, 0.0, 0.0])), ("t", json!([0.0, 3.0, 0.0]))], &[&grid]);
        let c = run("cloth_sim", &[("__frame", json!(40.0)), ("pin_top", json!(1)), ("collider_radius", json!(0.0))], &[&grid]);
        let n = grid.n_points();
        assert_eq!(c.n_points(), n);
        let ymax_before = grid.p().iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max);
        let ymin_after = c.p().iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
        assert!(ymin_after < ymax_before - 0.5, "unpinned cloth hangs: min y {ymin_after}");
        // Pinned (top) row did not move.
        for (a, b) in grid.p().iter().zip(c.p()) {
            if a[1] >= ymax_before - 1e-4 {
                assert_eq!(a, b);
            }
        }
    }

    #[test]
    fn smoke_node_makes_a_meshable_plume() {
        let src = Geometry::from_points(vec![[0.0, 0.3, 0.0]]);
        let s = run("smoke_sim", &[("__frame", json!(30.0)), ("size", json!([2.4, 3.2, 2.4])), ("center", json!([0.0, 0.0, 0.0])), ("voxel", json!(0.1)), ("iterations", json!(30))], &[&src]);
        let d = &s.volumes["density"];
        assert!(d.data.iter().sum::<f32>() > 1.0 && d.data.iter().all(|x| x.is_finite()));
        let sdf = run("volume_to_sdf", &[("threshold", json!(0.15))], &[&s]);
        let mesh = run("sdf_to_mesh", &[], &[&sdf]);
        assert!(mesh.n_prims() > 50, "plume surface has {} prims", mesh.n_prims());
        assert!(volume(&mesh).abs() > 1e-3);
    }

    #[test]
    fn shatter_makes_closed_pieces_that_tile_the_solid() {
        let b = make("box", &[("size", json!([2.0, 1.0, 0.5]))]);
        let pieces = run("rbd_shatter", &[("pieces", json!(8)), ("voxel", json!(0.05)), ("seed", json!(4))], &[&b]);
        let ids = pieces.prim.get("piece").unwrap().as_f32().unwrap();
        let count = ids.iter().map(|x| *x as i64).collect::<std::collections::BTreeSet<_>>().len();
        assert!(count >= 6, "{count} pieces");
        let total = volume(&pieces);
        assert!((total / 1.0 - 1.0).abs() < 0.12, "pieces fill the box: volume {total} vs 1.0");
        let iss = pt_core::validate::validate(&pieces);
        assert!(!iss.iter().any(|i| i.severity == "error"), "{:?}", iss.iter().map(|i| i.code).collect::<Vec<_>>());
    }

    #[test]
    fn rbd_node_drops_pieces_to_the_ground() {
        let b = make("box", &[("size", json!([1.0, 1.0, 1.0])), ("center", json!([0.0, 2.0, 0.0]))]);
        let pieces = run("rbd_shatter", &[("pieces", json!(5)), ("voxel", json!(0.08))], &[&b]);
        let before_min = pieces.p().iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
        let after = run("rbd_sim", &[("__frame", json!(72.0))], &[&pieces]);
        let after_min = after.p().iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
        assert!(before_min > 1.0 && after_min > -0.05 && after_min < 0.5, "min y {before_min} -> {after_min}");
        assert_eq!(after.n_prims(), pieces.n_prims());
        assert!(after.p().iter().all(|q| q.iter().all(|c| c.is_finite())));
    }

    /// A bumpy hill with logs scattered over it: after settling, every body lies on the ground (its lowest point touches the
    /// surface, none is inside it), and a tilted body is turned, not just dropped.
    #[test]
    fn rbd_settle_lays_logs_on_a_hill_and_is_deterministic() {
        let flat = make("grid", &[("size", json!([30.0, 30.0])), ("rows", json!(60)), ("cols", json!(60))]);
        let hill = run("wrangle", &[("class", json!("point")), ("code", json!("@P.y = 3.0 * exp(-(@P.x * @P.x + @P.z * @P.z) / 60.0) + 0.3 * sin(@P.x * 1.7) * cos(@P.z * 1.3);"))], &[&flat]);
        let spots = run("scatter_poisson", &[("count", json!(40)), ("min_dist", json!(1.6)), ("seed", json!(4))], &[&make("grid", &[("size", json!([16.0, 16.0]))])]);
        let spots = run("wrangle", &[("class", json!("point")), ("code", json!("@P.y = 4.0 + @P.x * 0.1;"))], &[&spots]);
        let log = make("tube", &[("radius", json!(0.18)), ("height", json!(1.8)), ("rows", json!(9)), ("cols", json!(10))]);
        let log = run("transform", &[("r", json!([0.0, 0.0, 90.0]))], &[&log]);
        let logs = run("copy_to_points", &[("rot_y_random", json!(360.0)), ("seed", json!(2))], &[&log, &spots]);
        let all = run("rbd_settle", &[("drop_lost", json!(0))], &[&logs, &hill]);
        let flags = all.instances.as_ref().unwrap().attrs.get("settled").unwrap().as_f32().unwrap().to_vec();
        let at_rest = flags.iter().filter(|f| **f == 1.0).count();
        eprintln!("{} bodies, {} at rest, moves {:?}", flags.len(), at_rest, all.instances.as_ref().unwrap().attrs.get("settle_move").unwrap().as_f32().unwrap().iter().map(|m| (m * 10.0).round() / 10.0).collect::<Vec<_>>());
        let settled = run("rbd_settle", &[], &[&logs, &hill]);
        let inst = settled.instances.as_ref().unwrap();
        assert!(inst.len() >= 30, "most logs come to rest ({at_rest} of {}): {}", flags.len(), inst.len());
        let ground = pt_core::spatial::SurfaceTree::new(&hill).unwrap();
        let proto = &inst.protos[0];
        let mut tilted = 0;
        for (k, m) in inst.xf.iter().enumerate() {
            let world: Vec<[f32; 3]> = proto.p().iter().map(|q| math::xform_point(m, *q)).collect();
            // distance of every point above the ground right under it: nothing deeper than a hair, the lowest touching
            let gaps: Vec<f32> = world.iter().filter_map(|q| ground.ray([q[0], 50.0, q[2]], [0.0, -1.0, 0.0]).map(|h| q[1] - h.pos[1])).collect();
            let lowest = gaps.iter().cloned().fold(f32::INFINITY, f32::min);
            assert!(lowest > -0.06, "log {k} sinks {} into the hill", -lowest);
            assert!(lowest < 0.12, "log {k} hovers {lowest} above the hill");
            // the log's axis (its local x) leans with the slope: not everything stays level
            if m[4].abs() > 0.05 {
                tilted += 1;
            }
            let at = settled.instances.as_ref().unwrap().attrs.get("settled").and_then(|c| c.as_f32().map(|v| v[k]));
            assert_eq!(at, Some(1.0));
        }
        assert!(tilted > 3, "settling turns bodies to lie on the slope ({tilted} tilted)");
        let again = run("rbd_settle", &[], &[&logs, &hill]);
        assert_eq!(settled.content_hash(), again.content_hash(), "the same logs settle the same way");
        // it explains itself when it cannot work
        let mut p = Params::new();
        for s in crate::schema("rbd_settle").unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        let err = crate::cook("rbd_settle", &p, &[Some(Arc::new(hill.clone())), Some(Arc::new(hill))]).unwrap_err();
        assert!(err.contains("no instances"), "{err}");
    }
}
