//! Heightfield tools. These operate on `grid` geometry (row-major points, heights in P.y) and
//! are deterministic: the hydraulic eroder runs droplets sequentially with counter-based
//! random starts, so results never depend on thread count.

use crate::{ps, sch};
use pt_core::math;
use pt_core::rng::rand4;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("hf_slope", "Write point attrs `slope` (steepness = |grad h|) and `slope_deg` (degrees) from a grid heightfield.", 1, D::Piecewise, G::Whole, vec![]),
        sch("hf_erode_thermal", "Thermal erosion: slopes steeper than the talus angle shed material downhill (Jacobi, deterministic).", 1, D::None, G::Whole, vec![
            ps("iterations", json!(30), "Iterations."),
            ps("talus", json!(35.0), "Talus angle in degrees."),
            ps("strength", json!(0.5), "Fraction of excess moved per iteration (0..1)."),
        ]),
        sch("hf_erode_hydraulic", "Hydraulic erosion by simulated droplets (deterministic, sequential). Writes point attrs `erosion` (material removed), `deposit` (material added), `water` (droplet visits) and `flow` (water / mean, so 1 = average).", 1, D::None, G::Whole, vec![
            ps("droplets", json!(50000), "Number of droplets."),
            ps("steps", json!(40), "Max steps per droplet."),
            ps("inertia", json!(0.05), "Direction inertia (0..1)."),
            ps("capacity", json!(4.0), "Sediment capacity factor."),
            ps("erosion", json!(0.3), "Erosion rate."),
            ps("deposition", json!(0.3), "Deposition rate."),
            ps("evaporation", json!(0.02), "Water lost per step."),
            ps("gravity", json!(4.0), "Gravity (speed gain on descent)."),
            ps("radius", json!(2), "Erosion brush radius in cells (1-6); larger = smoother channels."),
            ps("seed", json!(1), "Seed for droplet starts."),
        ]),
        sch("hf_terrace", "Quantize heights into terraces (blend 0 = off, 1 = hard steps).", 1, D::Piecewise, G::PerPoint, vec![
            ps("step", json!(1.0), "Terrace height in meters."),
            ps("blend", json!(0.7), "Blend toward the stepped height (0..1)."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "hf_slope" => inp(0).and_then(slope),
        "hf_erode_thermal" => inp(0).and_then(|g| thermal(g, p)),
        "hf_erode_hydraulic" => inp(0).and_then(|g| hydraulic(g, p)),
        "hf_terrace" => inp(0).map(|g| terrace(g, p)),
        _ => return None,
    })
}

/// (rows, cols, dx, dz) of a grid heightfield, validating the point layout.
fn dims(g: &Geometry, op: &str) -> Result<(usize, usize, f32, f32), String> {
    let d = g.detail.get("grid").ok_or_else(|| format!("{op}: input is not a grid heightfield (no grid layout recorded)"))?;
    let (rows, cols) = (d["rows"].as_u64().unwrap_or(0) as usize, d["cols"].as_u64().unwrap_or(0) as usize);
    if rows == 0 || cols == 0 || g.n_points() != (rows + 1) * (cols + 1) {
        return Err(format!("{op}: grid layout is inconsistent ({} points for {rows}x{cols}); point count must not change before heightfield ops", g.n_points()));
    }
    let p = g.p();
    Ok((rows, cols, (p[1][0] - p[0][0]).abs().max(1e-6), (p[cols + 1][2] - p[0][2]).abs().max(1e-6)))
}

fn heights(g: &Geometry) -> Vec<f32> {
    g.p().iter().map(|p| p[1]).collect()
}

fn with_heights(g: &Geometry, h: &[f32]) -> Geometry {
    let mut out = g.clone();
    for (p, h) in out.p_mut().iter_mut().zip(h) {
        p[1] = *h;
    }
    out
}

fn slope(g: &Geometry) -> Res {
    let (rows, cols, dx, dz) = dims(g, "hf_slope")?;
    let h = heights(g);
    let w = cols + 1;
    let at = |i: usize, j: usize| h[j * w + i];
    let (mut s, mut deg) = (Vec::with_capacity(h.len()), Vec::with_capacity(h.len()));
    for j in 0..=rows {
        for i in 0..=cols {
            let (i0, i1) = (i.saturating_sub(1), (i + 1).min(cols));
            let (j0, j1) = (j.saturating_sub(1), (j + 1).min(rows));
            let gx = (at(i1, j) - at(i0, j)) / ((i1 - i0) as f32 * dx);
            let gz = (at(i, j1) - at(i, j0)) / ((j1 - j0) as f32 * dz);
            let m = (gx * gx + gz * gz).sqrt();
            s.push(m);
            deg.push((math::atan64(m as f64) * (180.0 / std::f64::consts::PI)) as f32);
        }
    }
    let mut out = g.clone();
    out.point.set("slope", Column::from_f32(s));
    out.point.set("slope_deg", Column::from_f32(deg));
    Ok(out)
}

fn thermal(g: &Geometry, p: &Params) -> Res {
    let (rows, cols, dx, dz) = dims(g, "hf_erode_thermal")?;
    let w = cols + 1;
    let mut h = heights(g);
    let tan_t = math::sin_cos(math::to_radians(p.f("talus").clamp(1.0, 89.0))).0 / math::sin_cos(math::to_radians(p.f("talus").clamp(1.0, 89.0))).1;
    let strength = p.f("strength").clamp(0.0, 1.0);
    let nbrs: [(i32, i32, f32); 8] = {
        let dd = (dx * dx + dz * dz).sqrt();
        [(-1, 0, dx), (1, 0, dx), (0, -1, dz), (0, 1, dz), (-1, -1, dd), (1, -1, dd), (-1, 1, dd), (1, 1, dd)]
    };
    for _ in 0..p.i("iterations").clamp(0, 500) {
        let mut delta = vec![0.0f32; h.len()];
        for j in 0..=rows {
            for i in 0..=cols {
                let idx = j * w + i;
                let mut exc = [0.0f32; 8];
                let (mut total, mut maxd) = (0.0f32, 0.0f32);
                for (k, (di, dj, dist)) in nbrs.iter().enumerate() {
                    let (ni, nj) = (i as i32 + di, j as i32 + dj);
                    if ni < 0 || nj < 0 || ni > cols as i32 || nj > rows as i32 {
                        continue;
                    }
                    let diff = h[idx] - h[nj as usize * w + ni as usize];
                    let e = diff - tan_t * dist;
                    if e > 0.0 {
                        exc[k] = e;
                        total += e;
                        maxd = maxd.max(diff);
                    }
                }
                if total > 0.0 {
                    let moved = strength * 0.5 * total / 8.0 * 2.0f32.min(8.0);
                    let moved = moved.min(maxd * 0.5);
                    delta[idx] -= moved;
                    for (k, (di, dj, _)) in nbrs.iter().enumerate() {
                        if exc[k] > 0.0 {
                            let n = (j as i32 + dj) as usize * w + (i as i32 + di) as usize;
                            delta[n] += moved * exc[k] / total;
                        }
                    }
                }
            }
        }
        for (a, d) in h.iter_mut().zip(&delta) {
            *a += d;
        }
    }
    Ok(with_heights(g, &h))
}

fn hydraulic(g: &Geometry, p: &Params) -> Res {
    let (rows, cols, dx, _dz) = dims(g, "hf_erode_hydraulic")?;
    let w = cols + 1;
    let mut h = heights(g);
    let n = h.len();
    let (mut eroded, mut deposited, mut water) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    let (inertia, cap_k, er, dep, evap, grav) = (p.f("inertia").clamp(0.0, 0.99), p.f("capacity"), p.f("erosion").clamp(0.0, 1.0), p.f("deposition").clamp(0.0, 1.0), p.f("evaporation").clamp(0.0, 1.0), p.f("gravity"));
    let (steps, seed) = (p.i("steps").clamp(1, 500), p.i("seed"));
    let (maxx, maxz) = ((cols - 1) as f32 - 0.001, (rows - 1) as f32 - 0.001);
    let rad = p.i("radius").clamp(1, 6) as i32;
    let mut brush: Vec<(i32, i32, f32)> = Vec::new();
    for dj in -rad..=rad {
        for di in -rad..=rad {
            let d = ((di * di + dj * dj) as f32).sqrt();
            if d <= rad as f32 {
                brush.push((di, dj, rad as f32 + 1.0 - d));
            }
        }
    }
    let wsum: f32 = brush.iter().map(|b| b.2).sum();
    for b in brush.iter_mut() {
        b.2 /= wsum;
    }
    let _ = dx;
    for d in 0..p.i("droplets").clamp(0, 5_000_000) {
        let r = rand4(seed, 3, d as u64);
        let (mut x, mut z) = (1.0 + r[0] * (maxx - 1.0).max(0.0), 1.0 + r[1] * (maxz - 1.0).max(0.0));
        let (mut dirx, mut dirz, mut speed, mut vol, mut sed) = (0.0f32, 0.0f32, 1.0f32, 1.0f32, 0.0f32);
        for _ in 0..steps {
            let (ci, cj) = (x as usize, z as usize);
            let (fx, fz) = (x - ci as f32, z - cj as f32);
            let idx = cj * w + ci;
            let (h00, h10, h01, h11) = (h[idx], h[idx + 1], h[idx + w], h[idx + w + 1]);
            let gx = (h10 - h00) * (1.0 - fz) + (h11 - h01) * fz;
            let gz = (h01 - h00) * (1.0 - fx) + (h11 - h10) * fx;
            let height = h00 * (1.0 - fx) * (1.0 - fz) + h10 * fx * (1.0 - fz) + h01 * (1.0 - fx) * fz + h11 * fx * fz;
            dirx = dirx * inertia - gx * (1.0 - inertia);
            dirz = dirz * inertia - gz * (1.0 - inertia);
            let len = (dirx * dirx + dirz * dirz).sqrt();
            if len < 1e-6 {
                break;
            }
            dirx /= len;
            dirz /= len;
            let (nx, nz) = (x + dirx, z + dirz);
            if nx < 0.0 || nz < 0.0 || nx >= (cols - 1) as f32 || nz >= (rows - 1) as f32 {
                break;
            }
            let (ni, nj) = (nx as usize, nz as usize);
            let (nfx, nfz) = (nx - ni as f32, nz - nj as f32);
            let nidx = nj * w + ni;
            let nh = h[nidx] * (1.0 - nfx) * (1.0 - nfz) + h[nidx + 1] * nfx * (1.0 - nfz) + h[nidx + w] * (1.0 - nfx) * nfz + h[nidx + w + 1] * nfx * nfz;
            let dh = nh - height;
            let capacity = (-dh).max(0.01) * speed * vol * cap_k;
            let weights = [(1.0 - fx) * (1.0 - fz), fx * (1.0 - fz), (1.0 - fx) * fz, fx * fz];
            let cells = [idx, idx + 1, idx + w, idx + w + 1];
            if sed > capacity || dh > 0.0 {
                let amount = if dh > 0.0 { dh.min(sed) } else { (sed - capacity) * dep };
                sed -= amount;
                for (c, wt) in cells.iter().zip(weights) {
                    h[*c] += amount * wt;
                    deposited[*c] += amount * wt;
                }
            } else {
                let amount = ((capacity - sed) * er).min(-dh);
                // Erode through a smooth brush so single cells never become pits or spikes.
                let (bi, bj) = (x.round() as i32, z.round() as i32);
                let mut removed = 0.0f32;
                for (di, dj, wt) in &brush {
                    let (ci, cj) = (bi + di, bj + dj);
                    if ci < 0 || cj < 0 || ci > cols as i32 || cj > rows as i32 {
                        continue;
                    }
                    let c = cj as usize * w + ci as usize;
                    let take = (amount * wt).min(h[c].max(0.0));
                    h[c] -= take;
                    eroded[c] += take;
                    removed += take;
                }
                sed += removed;
            }
            for (c, wt) in cells.iter().zip(weights) {
                water[*c] += wt;
            }
            speed = (speed * speed - dh * grav).max(0.0).sqrt();
            vol *= 1.0 - evap;
            x = nx;
            z = nz;
            if vol < 0.01 {
                break;
            }
        }
    }
    let mut out = with_heights(g, &h);
    out.point.set("erosion", Column::from_f32(eroded));
    out.point.set("deposit", Column::from_f32(deposited));
    let mean = (water.iter().map(|x| *x as f64).sum::<f64>() / n.max(1) as f64).max(1e-9) as f32;
    out.point.set("flow", Column::from_f32(water.iter().map(|x| x / mean).collect()));
    out.point.set("water", Column::from_f32(water));
    Ok(out)
}

fn terrace(g: &Geometry, p: &Params) -> Geometry {
    let (step, blend) = (p.f("step").max(1e-4), p.f("blend").clamp(0.0, 1.0));
    let mut out = g.clone();
    for q in out.p_mut() {
        let stepped = (q[1] / step).floor() * step;
        q[1] += (stepped - q[1]) * blend;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain() -> Geometry {
        let mut p = Params::new();
        for s in crate::schema("grid").unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        p.insert("rows".into(), json!(64));
        p.insert("cols".into(), json!(64));
        p.insert("size".into(), json!([64.0, 64.0]));
        let g = crate::cook("grid", &p, &[]).unwrap();
        let mut np = Params::new();
        for s in crate::schema("noise").unwrap().params {
            np.insert(s.name.into(), s.default);
        }
        np.insert("amp".into(), json!(12.0));
        np.insert("freq".into(), json!(0.05));
        np.insert("octaves".into(), json!(4));
        crate::cook("noise", &np, &[Some(Arc::new(g))]).unwrap()
    }

    fn run(kind: &str, set: &[(&str, serde_json::Value)], g: &Geometry) -> Geometry {
        let mut p = Params::new();
        for s in crate::schema(kind).unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        for (k, v) in set {
            p.insert((*k).into(), v.clone());
        }
        crate::cook(kind, &p, &[Some(Arc::new(g.clone()))]).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    fn max_slope(g: &Geometry) -> f32 {
        run("hf_slope", &[], g).point.get("slope_deg").unwrap().as_f32().unwrap().iter().cloned().fold(0.0, f32::max)
    }

    #[test]
    fn slope_of_a_tilted_plane() {
        let flat = terrain();
        let mut plane = flat.clone();
        for q in plane.p_mut() {
            q[1] = q[0] * 0.5; // slope 0.5 along x
        }
        let s = run("hf_slope", &[], &plane);
        let deg = s.point.get("slope_deg").unwrap().as_f32().unwrap();
        let want = (0.5f64).atan().to_degrees() as f32;
        let mid = deg[32 * 65 + 32];
        assert!((mid - want).abs() < 0.01, "slope {mid} vs {want}");
    }

    #[test]
    fn thermal_erosion_flattens_steep_slopes() {
        let g = terrain();
        let before = max_slope(&g);
        let e = run("hf_erode_thermal", &[("iterations", json!(60)), ("talus", json!(30.0))], &g);
        let after = max_slope(&e);
        assert!(after < before, "max slope {before} -> {after}");
        let vol = |g: &Geometry| g.p().iter().map(|p| p[1] as f64).sum::<f64>();
        assert!(((vol(&e) - vol(&g)).abs() / (g.n_points() as f64)) < 1e-3, "mass is conserved");
    }

    #[test]
    fn hydraulic_erosion_carves_and_is_deterministic() {
        let g = terrain();
        let a = run("hf_erode_hydraulic", &[("droplets", json!(20000))], &g);
        let b = run("hf_erode_hydraulic", &[("droplets", json!(20000))], &g);
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), g.content_hash());
        let eroded: f32 = a.point.get("erosion").unwrap().as_f32().unwrap().iter().sum();
        let deposit: f32 = a.point.get("deposit").unwrap().as_f32().unwrap().iter().sum();
        assert!(eroded > 0.0 && deposit > 0.0, "eroded {eroded} deposited {deposit}");
        let water = a.point.get("water").unwrap().as_f32().unwrap();
        assert!(water.iter().cloned().fold(0.0, f32::max) > 5.0, "channels concentrate flow");
        assert!(a.p().iter().all(|p| p[1].is_finite()));
    }

    #[test]
    fn terrace_quantizes() {
        let g = terrain();
        let t = run("hf_terrace", &[("step", json!(2.0)), ("blend", json!(1.0))], &g);
        assert!(t.p().iter().all(|p| ((p[1] / 2.0).round() * 2.0 - p[1]).abs() < 1e-4));
    }

    #[test]
    fn non_grid_input_is_rejected_with_a_clear_message() {
        let mut p = Params::new();
        p.insert("iterations".into(), json!(1));
        let b = crate::cook("box", &Params::new(), &[]).unwrap();
        let e = crate::cook("hf_erode_thermal", &p, &[Some(Arc::new(b))]).unwrap_err();
        assert!(e.contains("not a grid heightfield"));
    }
}
