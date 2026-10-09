//! Position-based cloth (PBD): structural + bending distance constraints, wind, ground,
//! sphere and SDF colliders, pinned vertices. Deterministic Gauss-Seidel in a fixed order.

use pt_core::math;
use pt_core::Volume;

#[derive(Clone, Debug)]
pub struct ClothParams {
    pub frame: usize,
    pub fps: f32,
    pub substeps: usize,
    pub iterations: usize,
    pub gravity: [f32; 3],
    pub damping: f32,
    /// 0..1: how rigidly edges keep their rest length per iteration.
    pub stiffness: f32,
    pub bend_stiffness: f32,
    pub wind: [f32; 3],
    pub wind_strength: f32,
    /// Collision offset from the collider surface (cloth thickness), meters.
    pub thickness: f32,
    pub friction: f32,
    pub ground_y: f32,
}

impl Default for ClothParams {
    fn default() -> Self {
        ClothParams { frame: 48, fps: 24.0, substeps: 6, iterations: 8, gravity: [0.0, -9.81, 0.0], damping: 0.02, stiffness: 0.9, bend_stiffness: 0.15, wind: [0.0; 3], wind_strength: 0.0, thickness: 0.02, friction: 0.3, ground_y: -1.0e9 }
    }
}

pub struct ClothMesh {
    pub p: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub pinned: Vec<bool>,
}

pub enum Collider<'a> {
    Sphere { center: [f32; 3], radius: f32 },
    Sdf(&'a Volume),
}

struct Constraint {
    a: u32,
    b: u32,
    rest: f32,
    k: f32,
}

fn build_constraints(m: &ClothMesh, stiff: f32, bend: f32) -> Vec<Constraint> {
    use std::collections::HashMap;
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for t in &m.tris {
        for k in 0..3 {
            let (a, b, opp) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push(opp);
        }
    }
    let mut keys: Vec<_> = edges.keys().copied().collect();
    keys.sort_unstable();
    let len = |a: u32, b: u32| {
        let d = math::sub(m.p[a as usize], m.p[b as usize]);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let mut cs = Vec::new();
    for (a, b) in &keys {
        cs.push(Constraint { a: *a, b: *b, rest: len(*a, *b), k: stiff });
    }
    for key in &keys {
        let opp = &edges[key];
        if opp.len() == 2 {
            let (a, b) = (opp[0].min(opp[1]), opp[0].max(opp[1]));
            cs.push(Constraint { a, b, rest: len(a, b), k: bend });
        }
    }
    cs
}

fn sdf_grad(vol: &Volume, q: [f32; 3]) -> [f32; 3] {
    let h = vol.voxel * 0.5;
    math::normalize([
        vol.sample([q[0] + h, q[1], q[2]]) - vol.sample([q[0] - h, q[1], q[2]]),
        vol.sample([q[0], q[1] + h, q[2]]) - vol.sample([q[0], q[1] - h, q[2]]),
        vol.sample([q[0], q[1], q[2] + h]) - vol.sample([q[0], q[1], q[2] - h]),
    ])
}

pub fn simulate(m: &ClothMesh, colliders: &[Collider], prm: &ClothParams) -> Vec<[f32; 3]> {
    let n = m.p.len();
    let cs = build_constraints(m, prm.stiffness.clamp(0.0, 1.0), prm.bend_stiffness.clamp(0.0, 1.0));
    let mut x = m.p.clone();
    let mut v = vec![[0.0f32; 3]; n];
    let dt = 1.0 / prm.fps.max(1.0);
    let h = dt / prm.substeps.max(1) as f32;
    for _ in 0..prm.frame {
        for _ in 0..prm.substeps.max(1) {
            // Wind: per-triangle pressure along the normal, split to the triangle's vertices.
            let mut force = vec![[0.0f32; 3]; n];
            let mut count = vec![0.0f32; n];
            if prm.wind_strength != 0.0 {
                for t in &m.tris {
                    let (a, b, c) = (x[t[0] as usize], x[t[1] as usize], x[t[2] as usize]);
                    let cr = math::cross(math::sub(b, a), math::sub(c, a));
                    let area2 = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
                    if area2 < 1e-12 {
                        continue;
                    }
                    let nrm = [cr[0] / area2, cr[1] / area2, cr[2] / area2];
                    let vavg = [0, 1, 2].map(|k| (v[t[0] as usize][k] + v[t[1] as usize][k] + v[t[2] as usize][k]) / 3.0);
                    let rel = [0, 1, 2].map(|k| prm.wind[k] - vavg[k]);
                    let proj = rel[0] * nrm[0] + rel[1] * nrm[1] + rel[2] * nrm[2];
                    let f = [0, 1, 2].map(|k| nrm[k] * proj * prm.wind_strength);
                    for &i in t {
                        for k in 0..3 {
                            force[i as usize][k] += f[k];
                        }
                        count[i as usize] += 1.0;
                    }
                }
            }
            // Wind is an acceleration (per unit relative velocity), averaged over the triangles at a vertex.
            for i in 0..n {
                if count[i] > 0.0 {
                    for k in 0..3 {
                        force[i][k] /= count[i];
                    }
                }
            }
            let mut pred = x.clone();
            for i in 0..n {
                if m.pinned[i] {
                    v[i] = [0.0; 3];
                    continue;
                }
                for k in 0..3 {
                    v[i][k] = v[i][k] * (1.0 - prm.damping) + (prm.gravity[k] + force[i][k]) * h;
                    pred[i][k] = x[i][k] + v[i][k] * h;
                }
            }
            for _ in 0..prm.iterations.max(1) {
                for c in &cs {
                    let (a, b) = (c.a as usize, c.b as usize);
                    let d = math::sub(pred[a], pred[b]);
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if l < 1e-9 {
                        continue;
                    }
                    let (wa, wb) = (if m.pinned[a] { 0.0 } else { 1.0 }, if m.pinned[b] { 0.0 } else { 1.0 });
                    if wa + wb == 0.0 {
                        continue;
                    }
                    let corr = (l - c.rest) / l * c.k / (wa + wb);
                    for k in 0..3 {
                        pred[a][k] -= d[k] * corr * wa;
                        pred[b][k] += d[k] * corr * wb;
                    }
                }
                // Collisions inside the iteration loop keep the cloth from sinking in.
                for i in 0..n {
                    if m.pinned[i] {
                        pred[i] = x[i];
                        continue;
                    }
                    if pred[i][1] < prm.ground_y + prm.thickness {
                        pred[i][1] = prm.ground_y + prm.thickness;
                    }
                    for col in colliders {
                        match col {
                            Collider::Sphere { center, radius } => {
                                let d = math::sub(pred[i], *center);
                                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                                let r = radius + prm.thickness;
                                if l < r && l > 1e-9 {
                                    for k in 0..3 {
                                        pred[i][k] = center[k] + d[k] / l * r;
                                    }
                                }
                            }
                            Collider::Sdf(vol) => {
                                let s = vol.sample(pred[i]) - prm.thickness;
                                if s < 0.0 {
                                    let g = sdf_grad(vol, pred[i]);
                                    for k in 0..3 {
                                        pred[i][k] -= g[k] * s;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for i in 0..n {
                if m.pinned[i] {
                    continue;
                }
                let mut nv = [0.0f32; 3];
                for k in 0..3 {
                    nv[k] = (pred[i][k] - x[i][k]) / h;
                }
                // Cheap friction at the ground contact.
                if pred[i][1] <= prm.ground_y + prm.thickness + 1e-4 {
                    nv[0] *= 1.0 - prm.friction;
                    nv[2] *= 1.0 - prm.friction;
                }
                v[i] = nv;
                x[i] = pred[i];
            }
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(n: usize, size: f32) -> ClothMesh {
        let mut p = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                p.push([i as f32 / n as f32 * size - size / 2.0, 2.0, j as f32 / n as f32 * size - size / 2.0]);
            }
        }
        let w = (n + 1) as u32;
        let mut tris = Vec::new();
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let a = j * w + i;
                tris.push([a, a + w, a + w + 1]);
                tris.push([a, a + w + 1, a + 1]);
            }
        }
        let pinned = (0..p.len()).map(|_| false).collect();
        ClothMesh { p, tris, pinned }
    }

    #[test]
    fn pinned_corners_hold_and_cloth_sags_without_stretching() {
        let n = 12;
        let mut m = sheet(n, 2.0);
        let w = n + 1;
        for corner in [0, n, n * w, n * w + n] {
            m.pinned[corner] = true;
        }
        let rest = m.p.clone();
        let out = simulate(&m, &[], &ClothParams { frame: 60, ..Default::default() });
        for corner in [0, n, n * w, n * w + n] {
            assert_eq!(out[corner], rest[corner], "pinned vertex moved");
        }
        let center = out[(n / 2) * w + n / 2];
        assert!(center[1] < 1.9, "center sags under gravity: {}", center[1]);
        // Edge lengths stay within 15% of rest (no runaway stretching).
        let mut worst = 0.0f32;
        for j in 0..=n {
            for i in 0..n {
                let (a, b) = (j * w + i, j * w + i + 1);
                let (l0, l1) = (len(rest[a], rest[b]), len(out[a], out[b]));
                worst = worst.max((l1 / l0 - 1.0).abs());
            }
        }
        assert!(worst < 0.15, "worst stretch {worst}");
        assert!(out.iter().all(|p| p.iter().all(|c| c.is_finite())));
    }

    fn len(a: [f32; 3], b: [f32; 3]) -> f32 {
        let d = math::sub(a, b);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }

    #[test]
    fn cloth_drapes_over_a_sphere_without_penetrating() {
        let m = sheet(20, 3.0);
        let col = [Collider::Sphere { center: [0.0, 1.0, 0.0], radius: 0.8 }];
        let out = simulate(&m, &col, &ClothParams { frame: 72, ground_y: 0.0, ..Default::default() });
        let min_r = out.iter().map(|p| len(*p, [0.0, 1.0, 0.0])).fold(f32::INFINITY, f32::min);
        assert!(min_r > 0.8 - 1e-3, "penetration: closest vertex at radius {min_r}");
        assert!(out.iter().all(|p| p[1] >= -1e-3), "nothing under the ground");
        // The cloth should have wrapped: some vertices well below the sphere's top.
        assert!(out.iter().any(|p| p[1] < 1.4 && len([p[0], 0.0, p[2]], [0.0; 3]) < 1.0));
    }

    #[test]
    fn deterministic() {
        let mut m = sheet(8, 1.0);
        m.pinned[0] = true;
        let p = ClothParams { frame: 20, wind: [2.0, 0.0, 1.0], wind_strength: 1.0, ..Default::default() };
        assert_eq!(simulate(&m, &[], &p), simulate(&m, &[], &p));
    }
}
