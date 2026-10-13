//! Deformers, curve ops and placement helpers: bend, twist, taper, helix, resample, sweep, revolve,
//! drop_to_surface, point_jitter, switch, fit_size.
//!
//! Curves are polylines stored as 2-point segment primitives (like `line`) or one n-point primitive.
//! Deformers move base points only (instances are untouched: put `realize` before them if needed).
//! All trigonometry goes through `pt_core::math` so results are bit-identical across platforms.

use crate::{ps, sch};
use pt_core::math::{self, cross, normalize};
use pt_core::{Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("bend", "Bend geometry along `axis` into an arc: `angle` degrees over `length` meters, curving toward the next axis (y bends toward x, x toward z, z toward y). Moves base points only.", 1, D::Smooth, G::PerPoint, vec![
            ps("axis", json!("y"), "Axis the bend runs along: x | y | z."),
            ps("angle", json!(90.0), "Total bend angle in degrees over `length`."),
            ps("length", json!(1.0), "Length along the axis that produces `angle` (m)."),
            ps("origin", json!(0.0), "Coordinate on the axis where the bend starts (m)."),
        ]),
        sch("twist", "Rotate cross-sections around `axis` proportionally to their position along it. Moves base points only.", 1, D::Smooth, G::PerPoint, vec![
            ps("axis", json!("y"), "x | y | z."),
            ps("angle", json!(90.0), "Degrees of twist per meter along the axis."),
            ps("origin", json!(0.0), "Coordinate on the axis with zero twist (m)."),
        ]),
        sch("taper", "Scale cross-sections along `axis` linearly from `scale_start` (at the bottom of the bounds) to `scale_end` (at the top). Moves base points only.", 1, D::Smooth, G::PerPoint, vec![
            ps("axis", json!("y"), "x | y | z."),
            ps("scale_start", json!(1.0), "Cross-section scale at the min end."),
            ps("scale_end", json!(0.5), "Cross-section scale at the max end."),
        ]),
        sch("curve", "Polyline through a list of points, e.g. points=[[0,0,0],[1,1,0],[2,0,0]]. Feed it to sweep (pipes), revolve (lathe profile: x = radius, y = height) or resample.", 0, D::Smooth, G::Whole, vec![
            ps("points", json!([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]), "List of [x,y,z] points (at least 2)."),
            ps("closed", json!(0), "1 = connect the last point back to the first."),
        ]),
        sch("helix", "Helical polyline around Y (springs, stairs rails, vines). Radius can change along the helix.", 0, D::Smooth, G::Whole, vec![
            ps("radius", json!(1.0), "Start radius (m)."),
            ps("radius_end", json!(-1.0), "End radius (<0 = same as start)."),
            ps("pitch", json!(0.5), "Rise per turn (m)."),
            ps("turns", json!(3.0), "Number of turns."),
            ps("points_per_turn", json!(16), "Points per turn (>=3)."),
            ps("center", json!([0.0, 0.0, 0.0]), "Position of the start of the helix axis."),
        ]),
        sch("resample", "Resample a polyline into `count` equally spaced points (by arc length). Drops attributes. Closed loops stay closed.", 1, D::Smooth, G::Whole, vec![
            ps("count", json!(16), "Output point count (>=2)."),
        ]),
        sch("sweep", "Extrude a circular profile along a polyline using parallel-transport frames (pipes, cables, rails). Radius blends from `radius` to `radius_end`.", 1, D::Smooth, G::Whole, vec![
            ps("radius", json!(0.1), "Radius at the start (m)."),
            ps("radius_end", json!(-1.0), "Radius at the end (<0 = same as start)."),
            ps("cols", json!(8), "Points around the profile (>=3)."),
            ps("caps", json!(1), "1 = close the ends of an open curve."),
        ]),
        sch("revolve", "Lathe: spin a profile polyline (x = radius, y = height) around the Y axis. Points on the axis collapse to a pole.", 1, D::Smooth, G::Whole, vec![
            ps("divisions", json!(24), "Steps around the axis (>=3)."),
            ps("angle", json!(360.0), "Sweep in degrees; 360 closes the surface."),
        ]),
        sch("drop_to_surface", "Move input 0's points straight down/up onto the surface in input 1 (XZ lookup, highest hit wins). Writes the surface normal to `N`. Use it to place buildings, props or scatter points on terrain.", 2, D::None, G::Whole, vec![
            ps("offset", json!(0.0), "Lift above the surface (m)."),
            ps("set_normal", json!(1), "1 = write the surface normal into point attribute N."),
            ps("footprint", json!(0.0), "If > 0, settle each point onto the LOWEST ground under a ring of this radius (metres, times the point's pscale), so an object placed there has no side hanging in the air on a slope or a bank. Writes point attribute `settle_drop` (how far it was lowered, m): reject points with a large drop."),
        ]),
        sch("seat", "GROUNDING: seats the instances of input 0 (copy_to_points, copy_variants) on the ground of input 1 by their REAL shape, so nothing floats: a standing object is lowered until its whole base is in the ground, however steep the slope, and that includes the slab of soil a scanned tree stands on; a lying object (a log, a branch, a grass tile) is turned to lie along the ground under its whole footprint and comes down until its body rests on it, so it never stands in the air on a stub; and with input 2 (the water surface) and `draft`, wood lies in the water line where the water is deep enough and on the bed where it is not. An instance that would have to be lowered too far (the ground falls away under it: the lip of a bank, a ledge) is removed. Writes instance attribute `seat_move` (metres moved in y). `pt ground-check` measures the same thing, so what this node seats passes it.", 3, D::None, G::Whole, vec![
            ps("limit", json!(0.5), "Remove an instance that must be lowered by more than this share of its size (the smaller of its height and of what it stands on); 0 keeps all."),
            ps("draft", json!(0.0), "For things that float (wood): the share of the body under the water line, e.g. 0.55; 0 = it does not float and rests on the bed. Needs the water surface on input 2."),
            ps("bury", json!(0.02), "Metres the rim of the base with the most air under it goes below the ground: 0.2 for trees, whose roots go into the soil."),
            ps("sink", json!(0.45), "Most of an object's height that may lie under the ground (a share, 0.45 = under half). A kind buried deeper is raised to it, and removed where raising leaves its base in the air; a rock may be given 0.45, a fern 0.25. 1 = no limit. `pt ground-check` fails a world on more than half."),
            ps("wade", json!(0.45), "Most of an object's height that may lie under the creek's water (needs the water on input 2). Land plants and stones that would stand deeper are removed; wood that floats (`draft`) is not counted. 1 = no limit. `pt ground-check` fails a world on more than half."),
            ps("cameras", json!([]), "Standing cameras as [[x, z, eye_height], ...]: the recipe gets them from `knob(\"cameras\", [])`. An object whose nearest vertex comes within `clearance` metres of a camera's eye is removed (a sapling's branches in the lens)."),
            ps("clearance", json!(pt_core::seat::CAMERA_CLEARANCE), "Metres of clear air around every camera's eye, measured to the object's real extent, not its pivot."),
            ps("soft", json!(0), "1 = ground cover (grass tiles, flowers): exempt from `clearance`, since it is the ground the camera stands in. Recorded per instance as `seat_soft`, which `pt ground-check` reads. Wood put in the water line is recorded as `seat_float`: the check does not call it drowned."),
        ]),
        sch("point_jitter", "Random per-point displacement in [-amount, amount] per axis (scaled by `scale`).", 1, D::None, G::PerPoint, vec![
            ps("amount", json!(0.1), "Maximum displacement (m)."),
            ps("scale", json!([1.0, 1.0, 1.0]), "Per-axis multiplier (use [1,0,1] to keep Y)."),
            ps("seed", json!(1), "Seed."),
        ]),
        sch("switch", "Pass through input number `index` (0-3). Use it for variants and toggles.", 4, D::Smooth, G::Whole, vec![
            ps("index", json!(0), "Which input to output (0-3)."),
        ]),
        sch("fit_size", "Scale geometry to a target size. Axes with target 0 are free; `uniform` = 1 keeps proportions (smallest factor wins). Anchor: ground | center | none.", 1, D::Smooth, G::PerPoint, vec![
            ps("size", json!([0.0, 1.0, 0.0]), "Target extents (x,y,z); 0 = do not constrain this axis."),
            ps("uniform", json!(1), "1 = same scale on all axes."),
            ps("anchor", json!("ground"), "ground (center XZ, sit on y=0) | center | none."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "bend" => inp(0).and_then(|g| bend(g, p)),
        "twist" => inp(0).and_then(|g| twist(g, p)),
        "taper" => inp(0).and_then(|g| taper(g, p)),
        "helix" => Ok(helix(p)),
        "curve" => curve(p),
        "resample" => inp(0).and_then(|g| resample(g, p)),
        "sweep" => inp(0).and_then(|g| sweep(g, p)),
        "revolve" => inp(0).and_then(|g| revolve(g, p)),
        "drop_to_surface" => inp(0).and_then(|g| inp(1).and_then(|s| drop_to_surface(g, s, p))),
        "seat" => inp(0).and_then(|g| inp(1).and_then(|s| seat(g, s, inputs.get(2).and_then(|w| w.as_deref()), p))),
        "point_jitter" => inp(0).map(|g| point_jitter(g, p)),
        "switch" => {
            let i = p.i("index").clamp(0, 3) as usize;
            inp(i).cloned()
        }
        "fit_size" => inp(0).map(|g| fit_size(g, p)),
        _ => return None,
    })
}

fn axis_index(s: &str) -> Result<usize, String> {
    match s {
        "x" => Ok(0),
        "y" => Ok(1),
        "z" => Ok(2),
        o => Err(format!("axis must be x, y or z, got '{o}'")),
    }
}

fn bend(src: &Geometry, p: &Params) -> Res {
    let u = axis_index(&p.s("axis"))?;
    let v = (u + 2) % 3;
    let (angle, len, o) = (math::to_radians(p.f("angle")), p.f("length"), p.f("origin"));
    if len.abs() < 1e-9 {
        return Err("bend: length must be non-zero".into());
    }
    let mut g = src.clone();
    let k = angle / len;
    if k.abs() < 1e-7 {
        return Ok(g);
    }
    let r = 1.0 / k;
    for q in g.p_mut() {
        let t = k * (q[u] - o);
        let (s, c) = math::sin_cos(t);
        let d = r - q[v];
        q[u] = o + d * s;
        q[v] = r - d * c;
    }
    Ok(g)
}

fn twist(src: &Geometry, p: &Params) -> Res {
    let u = axis_index(&p.s("axis"))?;
    let (a, b) = ((u + 1) % 3, (u + 2) % 3);
    let (per_m, o) = (math::to_radians(p.f("angle")), p.f("origin"));
    let mut g = src.clone();
    for q in g.p_mut() {
        let (s, c) = math::sin_cos(per_m * (q[u] - o));
        let (x, y) = (q[a], q[b]);
        q[a] = x * c - y * s;
        q[b] = x * s + y * c;
    }
    Ok(g)
}

fn taper(src: &Geometry, p: &Params) -> Res {
    let u = axis_index(&p.s("axis"))?;
    let (a, b) = ((u + 1) % 3, (u + 2) % 3);
    let Some((lo, hi)) = src.base_bounds() else { return Ok(src.clone()) };
    let (s0, s1) = (p.f("scale_start"), p.f("scale_end"));
    let span = (hi[u] - lo[u]).max(1e-9);
    let mut g = src.clone();
    for q in g.p_mut() {
        let t = (q[u] - lo[u]) / span;
        let s = s0 + (s1 - s0) * t;
        q[a] *= s;
        q[b] *= s;
    }
    Ok(g)
}

fn helix(p: &Params) -> Geometry {
    let r0 = p.f("radius");
    let r1 = if p.f("radius_end") < 0.0 { r0 } else { p.f("radius_end") };
    let (pitch, turns) = (p.f("pitch"), p.f("turns").max(0.0));
    let per = p.i("points_per_turn").max(3) as usize;
    let c = p.v3("center");
    let n = ((turns * per as f32).round() as usize).max(1);
    let pts: Vec<[f32; 3]> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let ang = 2.0 * math::PI * turns * t;
            let (s, co) = math::sin_cos(ang);
            let r = r0 + (r1 - r0) * t;
            [c[0] + r * co, c[1] + pitch * turns * t, c[2] + r * s]
        })
        .collect();
    let polys: Vec<Vec<u32>> = (0..n as u32).map(|i| vec![i, i + 1]).collect();
    Geometry::from_polys(pts, &polys)
}

/// Ordered point chain of a polyline plus whether it is closed.
fn chain(g: &Geometry) -> Result<(Vec<u32>, bool), String> {
    let np = g.n_prims();
    if np == 0 {
        return Err("needs a polyline (segment primitives), but the input has no primitives".into());
    }
    if np == 1 {
        let pts = g.prim_points(0).to_vec();
        if pts.len() < 2 {
            return Err("the polyline primitive has fewer than 2 points".into());
        }
        return Ok((pts.clone(), pts.len() >= 3));
    }
    let n = g.n_points();
    let mut adj: Vec<Vec<u32>> = vec![vec![]; n];
    for i in 0..np {
        let pr = g.prim_points(i);
        if pr.len() != 2 {
            return Err(format!("expected 2-point segments, primitive {i} has {} points", pr.len()));
        }
        adj[pr[0] as usize].push(pr[1]);
        adj[pr[1] as usize].push(pr[0]);
    }
    if adj.iter().any(|a| a.len() > 2) {
        return Err("the curve branches (a point has more than 2 segments); split it into separate curves first".into());
    }
    let start = (0..n).find(|&i| adj[i].len() == 1).or_else(|| (0..n).find(|&i| adj[i].len() == 2));
    let Some(start) = start else { return Err("no curve points found".into()) };
    let closed = adj[start].len() == 2;
    let mut out = vec![start as u32];
    let mut prev = u32::MAX;
    let mut cur = start as u32;
    loop {
        let next = adj[cur as usize].iter().copied().find(|&x| x != prev && (closed || x != cur));
        let Some(nx) = next else { break };
        if nx as usize == start || out.contains(&nx) {
            break;
        }
        out.push(nx);
        prev = cur;
        cur = nx;
    }
    if out.len() < np && !closed || out.len() + 1 < np {
        return Err("the input holds several disconnected curves; process them one at a time".into());
    }
    Ok((out, closed))
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = math::sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn resample(src: &Geometry, p: &Params) -> Res {
    let (ids, closed) = chain(src).map_err(|e| format!("resample: {e}"))?;
    let n = p.i("count").max(2) as usize;
    let mut pts: Vec<[f32; 3]> = ids.iter().map(|&i| src.p()[i as usize]).collect();
    if closed {
        pts.push(pts[0]);
    }
    let mut cum = vec![0.0f32];
    for w in pts.windows(2) {
        let so_far = cum.last().copied().unwrap_or(0.0);
        cum.push(so_far + dist(w[0], w[1]));
    }
    let total = cum.last().copied().unwrap_or(0.0);
    let steps = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(n);
    let mut seg = 0;
    for i in 0..n {
        let d = total * i as f32 / steps as f32;
        while seg + 2 < cum.len() && cum[seg + 1] < d {
            seg += 1;
        }
        let span = (cum[seg + 1] - cum[seg]).max(1e-12);
        let t = ((d - cum[seg]) / span).clamp(0.0, 1.0);
        let (a, b) = (pts[seg], pts[seg + 1]);
        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
    }
    let mut polys: Vec<Vec<u32>> = (0..n as u32 - 1).map(|i| vec![i, i + 1]).collect();
    if closed {
        polys.push(vec![n as u32 - 1, 0]);
    }
    Ok(Geometry::from_polys(out, &polys))
}

fn sweep(src: &Geometry, p: &Params) -> Res {
    let (ids, closed) = chain(src).map_err(|e| format!("sweep: {e}"))?;
    let pts: Vec<[f32; 3]> = ids.iter().map(|&i| src.p()[i as usize]).collect();
    let m = pts.len();
    let cols = p.i("cols").max(3) as usize;
    let r0 = p.f("radius");
    let r1 = if p.f("radius_end") < 0.0 { r0 } else { p.f("radius_end") };
    // Tangents (central differences, wrapped when closed).
    let tan: Vec<[f32; 3]> = (0..m)
        .map(|i| {
            let (a, b) = if closed { (pts[(i + m - 1) % m], pts[(i + 1) % m]) } else { (pts[i.saturating_sub(1)], pts[(i + 1).min(m - 1)]) };
            normalize(math::sub(b, a))
        })
        .collect();
    // Parallel-transport frames starting from a stable reference vector.
    let t0 = tan[0];
    let refv = if t0[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let mut nrm = normalize(cross(t0, refv));
    let mut ring_pts: Vec<[f32; 3]> = Vec::with_capacity(m * cols);
    let total: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum::<f32>().max(1e-12);
    let mut acc = 0.0f32;
    for i in 0..m {
        if i > 0 {
            acc += dist(pts[i - 1], pts[i]);
            let t = tan[i];
            let d = nrm[0] * t[0] + nrm[1] * t[1] + nrm[2] * t[2];
            nrm = normalize([nrm[0] - t[0] * d, nrm[1] - t[1] * d, nrm[2] - t[2] * d]);
        }
        let bin = cross(nrm, tan[i]);
        let r = r0 + (r1 - r0) * (acc / total);
        for k in 0..cols {
            let (s, c) = math::sin_cos(2.0 * math::PI * k as f32 / cols as f32);
            ring_pts.push([
                pts[i][0] + r * (c * nrm[0] + s * bin[0]),
                pts[i][1] + r * (c * nrm[1] + s * bin[1]),
                pts[i][2] + r * (c * nrm[2] + s * bin[2]),
            ]);
        }
    }
    let at = |j: usize, k: usize| (j * cols + k % cols) as u32;
    let mut polys: Vec<Vec<u32>> = Vec::new();
    let rings = if closed { m } else { m - 1 };
    for j in 0..rings {
        let j1 = (j + 1) % m;
        for k in 0..cols {
            polys.push(vec![at(j, k), at(j1, k), at(j1, k + 1), at(j, k + 1)]);
        }
    }
    if !closed && p.i("caps") != 0 {
        polys.push((0..cols).map(|k| at(0, k)).collect());
        polys.push((0..cols).rev().map(|k| at(m - 1, k)).collect());
    }
    Ok(Geometry::from_polys(ring_pts, &polys))
}

fn revolve(src: &Geometry, p: &Params) -> Res {
    let (ids, _) = chain(src).map_err(|e| format!("revolve: {e}"))?;
    let prof: Vec<[f32; 3]> = ids.iter().map(|&i| src.p()[i as usize]).collect();
    let div = p.i("divisions").max(3) as usize;
    let sweep_deg = p.f("angle").clamp(-360.0, 360.0);
    let full = (sweep_deg.abs() - 360.0).abs() < 1e-3;
    let cols = if full { div } else { div + 1 };
    let mut pts: Vec<[f32; 3]> = Vec::new();
    // idx[j][i]: point id for profile point j at step i (poles share one id).
    let mut idx: Vec<Vec<u32>> = Vec::new();
    for q in &prof {
        let r = q[0];
        if r.abs() < 1e-6 {
            pts.push([0.0, q[1], 0.0]);
            idx.push(vec![pts.len() as u32 - 1; cols]);
        } else {
            let mut row = Vec::with_capacity(cols);
            for i in 0..cols {
                let a = math::to_radians(sweep_deg) * i as f32 / div as f32;
                let (s, c) = math::sin_cos(a);
                pts.push([r * c, q[1], r * s]);
                row.push(pts.len() as u32 - 1);
            }
            idx.push(row);
        }
    }
    let mut polys: Vec<Vec<u32>> = Vec::new();
    let steps = if full { div } else { div };
    for j in 0..prof.len() - 1 {
        for i in 0..steps {
            let i1 = if full { (i + 1) % div } else { i + 1 };
            let mut quad = vec![idx[j][i], idx[j + 1][i], idx[j + 1][i1], idx[j][i1]];
            quad.dedup();
            if quad.len() > 1 && quad.first() == quad.last() {
                quad.pop();
            }
            if quad.len() >= 3 {
                polys.push(quad);
            }
        }
    }
    Ok(Geometry::from_polys(pts, &polys))
}

fn drop_to_surface(src: &Geometry, surf: &Geometry, p: &Params) -> Res {
    let tris = surf.triangles();
    if tris.is_empty() {
        return Err("drop_to_surface: input 1 has no polygons to drop onto".into());
    }
    let sp = surf.p();
    let (lo, hi) = surf.base_bounds().ok_or("drop_to_surface: input 1 is empty")?;
    let cells = ((tris.len() as f32).sqrt().ceil() as usize).clamp(1, 512);
    let (sx, sz) = ((hi[0] - lo[0]).max(1e-6) / cells as f32, (hi[2] - lo[2]).max(1e-6) / cells as f32);
    let cell_of = |x: f32, z: f32| -> (usize, usize) { (((x - lo[0]) / sx).floor().clamp(0.0, cells as f32 - 1.0) as usize, ((z - lo[2]) / sz).floor().clamp(0.0, cells as f32 - 1.0) as usize) };
    let mut grid: Vec<Vec<u32>> = vec![vec![]; cells * cells];
    for (ti, t) in tris.iter().enumerate() {
        let v = [sp[t[0] as usize], sp[t[1] as usize], sp[t[2] as usize]];
        let (mnx, mxx) = (v.iter().map(|q| q[0]).fold(f32::MAX, f32::min), v.iter().map(|q| q[0]).fold(f32::MIN, f32::max));
        let (mnz, mxz) = (v.iter().map(|q| q[2]).fold(f32::MAX, f32::min), v.iter().map(|q| q[2]).fold(f32::MIN, f32::max));
        let (c0, r0) = cell_of(mnx, mnz);
        let (c1, r1) = cell_of(mxx, mxz);
        for r in r0..=r1 {
            for c in c0..=c1 {
                grid[r * cells + c].push(ti as u32);
            }
        }
    }
    // height and normal of the surface under (x, z); the highest hit wins
    let hit = |x: f32, z: f32| -> Option<(f32, [f32; 3])> {
        let (c, r) = cell_of(x, z);
        let mut best: Option<(f32, [f32; 3])> = None;
        for &ti in &grid[r * cells + c] {
            let t = tris[ti as usize];
            let (a, b, cc) = (sp[t[0] as usize], sp[t[1] as usize], sp[t[2] as usize]);
            // Barycentric coordinates in the XZ plane.
            let d = (b[2] - cc[2]) * (a[0] - cc[0]) + (cc[0] - b[0]) * (a[2] - cc[2]);
            if d.abs() < 1e-12 {
                continue;
            }
            let w0 = ((b[2] - cc[2]) * (x - cc[0]) + (cc[0] - b[0]) * (z - cc[2])) / d;
            let w1 = ((cc[2] - a[2]) * (x - cc[0]) + (a[0] - cc[0]) * (z - cc[2])) / d;
            let w2 = 1.0 - w0 - w1;
            let eps = -1e-5;
            if w0 >= eps && w1 >= eps && w2 >= eps {
                let y = w0 * a[1] + w1 * b[1] + w2 * cc[1];
                if best.is_none_or(|(by, _)| y > by) {
                    let mut n = normalize(cross(math::sub(b, a), math::sub(cc, a)));
                    if n[1] < 0.0 {
                        n = [-n[0], -n[1], -n[2]];
                    }
                    best = Some((y, n));
                }
            }
        }
        best
    };
    let off = p.f("offset");
    let set_n = p.i("set_normal") != 0;
    let footprint = p.f("footprint").max(0.0);
    let mut g = src.clone();
    let pscale: Option<Vec<f32>> = g.point.get("pscale").and_then(pt_core::Column::as_f32).map(|s| s.to_vec());
    let mut normals: Vec<[f32; 3]> = vec![[0.0, 1.0, 0.0]; g.n_points()];
    let mut drops: Vec<f32> = vec![0.0; g.n_points()];
    for (pi, q) in g.p_mut().iter_mut().enumerate() {
        if let Some((y, n)) = hit(q[0], q[2]) {
            let mut y_set = y;
            if footprint > 0.0 {
                // settle onto the lowest ground under the object's footprint (a ring of eight points at `footprint` x pscale metres), so
                // no side of it hangs in the air on a slope or a bank; the high side is buried instead
                let r = footprint * pscale.as_ref().and_then(|s| s.get(pi)).copied().unwrap_or(1.0);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4;
                    let (sn, cs) = math::sin_cos(a);
                    if let Some((yy, _)) = hit(q[0] + r * cs, q[2] + r * sn) {
                        y_set = y_set.min(yy);
                    }
                }
                drops[pi] = y - y_set;
            }
            q[1] = y_set + off;
            normals[pi] = n;
        }
    }
    if set_n {
        g.point.set("N", pt_core::Column::from_v3(normals));
    }
    if footprint > 0.0 {
        g.point.set("settle_drop", pt_core::Column::from_f32(drops));
    }
    Ok(g)
}

/// Seat instances on the ground by their real shape (`pt_core::seat`): the same measure `pt ground-check` uses.
fn seat(src: &Geometry, ground: &Geometry, water: Option<&Geometry>, p: &Params) -> Res {
    use pt_core::seat::{Heights, Sole};
    let inst = src.instances.as_ref().ok_or("seat: input 0 has no instances (put copy_to_points or copy_variants upstream)")?;
    let land = Heights::new(ground).ok_or("seat: input 1 has no polygons to stand on")?;
    let wet = water.and_then(Heights::new);
    let (limit, draft, bury) = (p.f("limit").max(0.0), p.f("draft").clamp(0.0, 1.0), p.f("bury").max(0.0));
    let soles: Vec<Option<Sole>> = inst.protos.iter().map(|g| Sole::of(g)).collect();
    let on_land = |x: f32, z: f32| land.at(x, z);
    let on_water = |x: f32, z: f32| wet.as_ref().and_then(|w| w.at(x, z));
    let kind = pt_core::seat::Keep { sink: p.f("sink").clamp(0.0, 1.0), wade: p.f("wade").clamp(0.0, 1.0) };
    // the eye of every standing camera, and the prototypes' boxes for the nearest-vertex test
    let soft = p.i("soft") != 0;
    let clearance = p.f("clearance").max(0.0);
    let eyes: Vec<[f32; 3]> = match p.get("cameras").and_then(|v| v.as_array()) {
        Some(list) if !soft && clearance > 0.0 => list
            .iter()
            .filter_map(|c| {
                let c = c.as_array()?;
                let (x, z, h) = (c.first()?.as_f64()? as f32, c.get(1)?.as_f64()? as f32, c.get(2)?.as_f64()? as f32);
                Some([x, land.at(x, z).unwrap_or(0.0) + h, z])
            })
            .collect(),
        _ => Vec::new(),
    };
    let bounds: Vec<Option<([f32; 3], [f32; 3])>> = inst.protos.iter().map(|g| g.bounds()).collect();
    let (mut keep, mut xf, mut moved, mut floating) = (Vec::with_capacity(inst.len()), Vec::with_capacity(inst.len()), Vec::with_capacity(inst.len()), Vec::with_capacity(inst.len()));
    for (k, m) in inst.xf.iter().enumerate() {
        let pid = inst.proto_id[k] as usize;
        let seated = match &soles[pid] {
            Some(sole) => pt_core::seat::place(sole, m, &on_land, &on_water, limit, draft, bury, &kind).ok(),
            None => Some((*m, false)),
        };
        let Some((to, floated)) = seated else { continue };
        let in_face = bounds[pid].is_some_and(|b| eyes.iter().any(|e| pt_core::seat::nearest_vertex(&inst.protos[pid], b, &to, *e, clearance).is_some()));
        if in_face {
            continue;
        }
        keep.push(k as u32);
        moved.push(to[7] - m[7]);
        floating.push(if floated { 1.0 } else { 0.0 });
        xf.push(to);
    }
    let mut out = src.clone();
    let mut attrs = inst.attrs.gather(&keep);
    attrs.set("seat_soft", pt_core::Column::from_f32(vec![if soft { 1.0 } else { 0.0 }; keep.len()]));
    attrs.set("seat_float", pt_core::Column::from_f32(floating));
    attrs.set("seat_move", pt_core::Column::from_f32(moved));
    out.instances = Some(pt_core::geo::Instances { protos: inst.protos.clone(), proto_id: Arc::new(keep.iter().map(|&k| inst.proto_id[k as usize]).collect()), xf: Arc::new(xf), attrs });
    Ok(out)
}

fn point_jitter(src: &Geometry, p: &Params) -> Geometry {
    let (amt, sc, seed) = (p.f("amount"), p.v3("scale"), p.i("seed"));
    let mut g = src.clone();
    for (i, q) in g.p_mut().iter_mut().enumerate() {
        let r = pt_core::rng::rand4(seed, 17, i as u64);
        for a in 0..3 {
            q[a] += (r[a] * 2.0 - 1.0) * amt * sc[a];
        }
    }
    g
}

fn fit_size(src: &Geometry, p: &Params) -> Geometry {
    let Some((lo, hi)) = src.bounds() else { return src.clone() };
    let target = p.v3("size");
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let mut f = [1.0f32; 3];
    let mut best = f32::MAX;
    for a in 0..3 {
        if target[a] > 0.0 && ext[a] > 1e-9 {
            f[a] = target[a] / ext[a];
            best = best.min(f[a]);
        }
    }
    if p.i("uniform") != 0 && best < f32::MAX {
        f = [best; 3];
    }
    let anchor = p.s("anchor");
    let (clo, chi) = ([lo[0] * f[0], lo[1] * f[1], lo[2] * f[2]], [hi[0] * f[0], hi[1] * f[1], hi[2] * f[2]]);
    let t = match anchor.as_str() {
        "none" => [0.0; 3],
        "center" => [-(clo[0] + chi[0]) * 0.5, -(clo[1] + chi[1]) * 0.5, -(clo[2] + chi[2]) * 0.5],
        _ => [-(clo[0] + chi[0]) * 0.5, -clo[1], -(clo[2] + chi[2]) * 0.5],
    };
    crate::kernel::apply(src, &[crate::kernel::PointOp::Affine(math::trs(t, [0.0; 3], f))])
}

fn curve(p: &Params) -> Res {
    let arr = p.get("points").and_then(|v| v.as_array()).ok_or("curve: `points` must be a list of [x,y,z]")?;
    let mut pts = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| format!("curve: point {i} must be [x,y,z]"))?;
        let c = |k: usize| a[k].as_f64().ok_or_else(|| format!("curve: point {i} has a non-numeric coordinate"));
        pts.push([c(0)? as f32, c(1)? as f32, c(2)? as f32]);
    }
    if pts.len() < 2 {
        return Err("curve: needs at least 2 points".into());
    }
    let n = pts.len() as u32;
    let mut polys: Vec<Vec<u32>> = (0..n - 1).map(|i| vec![i, i + 1]).collect();
    if p.i("closed") != 0 && n >= 3 {
        polys.push(vec![n - 1, 0]);
    }
    Ok(Geometry::from_polys(pts, &polys))
}
