//! Smoke / fire gas on a dense grid: Stam-style stable fluids (semi-Lagrangian advection with
//! midpoint backtrace, buoyancy from a temperature field, vorticity confinement, Jacobi pressure
//! projection). Parallel passes read the previous buffer and write a new one, so results are
//! independent of thread count.

use rayon::prelude::*;

#[derive(Clone, Debug)]
pub struct SmokeParams {
    pub dims: [usize; 3],
    pub origin: [f32; 3],
    pub voxel: f32,
    pub frame: usize,
    pub fps: f32,
    pub substeps: usize,
    /// Upward acceleration per unit temperature.
    pub buoyancy: f32,
    /// Downward acceleration per unit density (smoke is slightly heavier than air).
    pub weight: f32,
    pub dissipation: f32,
    pub cooling: f32,
    pub vorticity: f32,
    pub source_radius: f32,
    pub source_rate: f32,
    pub source_temp: f32,
    pub source_vel: [f32; 3],
    pub iterations: usize,
}

impl Default for SmokeParams {
    fn default() -> Self {
        SmokeParams { dims: [48, 64, 48], origin: [-1.5, 0.0, -1.5], voxel: 0.06, frame: 48, fps: 24.0, substeps: 1, buoyancy: 4.0, weight: 0.2, dissipation: 0.15, cooling: 0.6, vorticity: 0.5, source_radius: 0.25, source_rate: 14.0, source_temp: 1.0, source_vel: [0.0, 0.6, 0.0], iterations: 40 }
    }
}

pub struct SmokeState {
    pub dims: [usize; 3],
    pub density: Vec<f32>,
    pub temp: Vec<f32>,
    pub vel: [Vec<f32>; 3],
}

#[inline]
fn at(dims: [usize; 3], i: i64, j: i64, k: i64) -> usize {
    let c = |v: i64, n: usize| v.clamp(0, n as i64 - 1) as usize;
    (c(k, dims[2]) * dims[1] + c(j, dims[1])) * dims[0] + c(i, dims[0])
}

fn sample(f: &[f32], dims: [usize; 3], g: [f32; 3]) -> f32 {
    let p = [g[0].clamp(0.0, dims[0] as f32 - 1.001), g[1].clamp(0.0, dims[1] as f32 - 1.001), g[2].clamp(0.0, dims[2] as f32 - 1.001)];
    let (i, j, k) = (p[0] as i64, p[1] as i64, p[2] as i64);
    let (fx, fy, fz) = (p[0] - i as f32, p[1] - j as f32, p[2] - k as f32);
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let s = |di, dj, dk| f[at(dims, i + di, j + dj, k + dk)];
    l(l(l(s(0, 0, 0), s(1, 0, 0), fx), l(s(0, 1, 0), s(1, 1, 0), fx), fy), l(l(s(0, 0, 1), s(1, 0, 1), fx), l(s(0, 1, 1), s(1, 1, 1), fx), fy), fz)
}

/// Semi-Lagrangian advection of `src` through velocity `vel` over `dt` (grid units/second).
fn advect(src: &[f32], vel: &[Vec<f32>; 3], dims: [usize; 3], dt: f32) -> Vec<f32> {
    let [nx, ny, _] = dims;
    let mut out = vec![0.0f32; src.len()];
    out.par_chunks_mut(nx * ny).enumerate().for_each(|(k, slab)| {
        for j in 0..ny {
            for i in 0..nx {
                let idx = (k * ny + j) * nx + i;
                let g = [i as f32, j as f32, k as f32];
                let v0 = [vel[0][idx], vel[1][idx], vel[2][idx]];
                let mid = [g[0] - 0.5 * dt * v0[0], g[1] - 0.5 * dt * v0[1], g[2] - 0.5 * dt * v0[2]];
                let vm = [sample(&vel[0], dims, mid), sample(&vel[1], dims, mid), sample(&vel[2], dims, mid)];
                let back = [g[0] - dt * vm[0], g[1] - dt * vm[1], g[2] - dt * vm[2]];
                slab[j * nx + i] = sample(src, dims, back);
            }
        }
    });
    out
}

fn enforce_walls(vel: &mut [Vec<f32>; 3], dims: [usize; 3]) {
    let [nx, ny, nz] = dims;
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let idx = (k * ny + j) * nx + i;
                if i == 0 || i == nx - 1 {
                    vel[0][idx] = 0.0;
                }
                if j == 0 {
                    vel[1][idx] = vel[1][idx].max(0.0);
                }
                if j == ny - 1 {
                    vel[1][idx] = vel[1][idx].min(0.0);
                }
                if k == 0 || k == nz - 1 {
                    vel[2][idx] = 0.0;
                }
            }
        }
    }
}

pub fn divergence(vel: &[Vec<f32>; 3], dims: [usize; 3], voxel: f32) -> Vec<f32> {
    let [nx, ny, nz] = dims;
    let mut div = vec![0.0f32; nx * ny * nz];
    for k in 0..nz as i64 {
        for j in 0..ny as i64 {
            for i in 0..nx as i64 {
                let d = (vel[0][at(dims, i + 1, j, k)] - vel[0][at(dims, i - 1, j, k)] + vel[1][at(dims, i, j + 1, k)] - vel[1][at(dims, i, j - 1, k)] + vel[2][at(dims, i, j, k + 1)] - vel[2][at(dims, i, j, k - 1)]) / (2.0 * voxel);
                div[at(dims, i, j, k)] = d;
            }
        }
    }
    div
}

fn project(vel: &mut [Vec<f32>; 3], dims: [usize; 3], voxel: f32, iters: usize) {
    let [nx, ny, nz] = dims;
    let div = divergence(vel, dims, voxel);
    let mut p = vec![0.0f32; nx * ny * nz];
    for _ in 0..iters {
        let mut next = vec![0.0f32; p.len()];
        next.par_chunks_mut(nx * ny).enumerate().for_each(|(k, slab)| {
            for j in 0..ny {
                for i in 0..nx {
                    let (ii, jj, kk) = (i as i64, j as i64, k as i64);
                    let s = p[at(dims, ii + 1, jj, kk)] + p[at(dims, ii - 1, jj, kk)] + p[at(dims, ii, jj + 1, kk)] + p[at(dims, ii, jj - 1, kk)] + p[at(dims, ii, jj, kk + 1)] + p[at(dims, ii, jj, kk - 1)];
                    slab[j * nx + i] = (s - voxel * voxel * div[(k * ny + j) * nx + i]) / 6.0;
                }
            }
        });
        p = next;
    }
    for k in 0..nz as i64 {
        for j in 0..ny as i64 {
            for i in 0..nx as i64 {
                let idx = at(dims, i, j, k);
                vel[0][idx] -= (p[at(dims, i + 1, j, k)] - p[at(dims, i - 1, j, k)]) / (2.0 * voxel);
                vel[1][idx] -= (p[at(dims, i, j + 1, k)] - p[at(dims, i, j - 1, k)]) / (2.0 * voxel);
                vel[2][idx] -= (p[at(dims, i, j, k + 1)] - p[at(dims, i, j, k - 1)]) / (2.0 * voxel);
            }
        }
    }
}

pub fn simulate(sources: &[[f32; 3]], prm: &SmokeParams) -> SmokeState {
    let dims = prm.dims;
    let n = dims[0] * dims[1] * dims[2];
    let mut density = vec![0.0f32; n];
    let mut temp = vec![0.0f32; n];
    let mut vel = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    let dt = 1.0 / prm.fps.max(1.0) / prm.substeps.max(1) as f32;
    let h = prm.voxel;
    // Source cells (positions in grid units) with smooth falloff weights, computed once.
    let r_cells = (prm.source_radius / h).max(1.0);
    let mut src_cells: Vec<(usize, f32)> = Vec::new();
    for s in sources {
        let g = [(s[0] - prm.origin[0]) / h, (s[1] - prm.origin[1]) / h, (s[2] - prm.origin[2]) / h];
        let lo = [0, 1, 2].map(|a| ((g[a] - r_cells).floor() as i64).max(0));
        let hi = [0, 1, 2].map(|a| ((g[a] + r_cells).ceil() as i64).min(dims[a] as i64 - 1));
        for k in lo[2]..=hi[2] {
            for j in lo[1]..=hi[1] {
                for i in lo[0]..=hi[0] {
                    let d = (pt_core::math::sq(i as f32 - g[0]) + pt_core::math::sq(j as f32 - g[1]) + pt_core::math::sq(k as f32 - g[2])).sqrt() / r_cells;
                    if d < 1.0 {
                        let w = 1.0 - d;
                        src_cells.push((at(dims, i, j, k), w * w * (3.0 - 2.0 * w)));
                    }
                }
            }
        }
    }
    for _ in 0..prm.frame * prm.substeps.max(1) {
        for &(idx, w) in &src_cells {
            density[idx] = (density[idx] + prm.source_rate * w * dt).min(4.0);
            temp[idx] = temp[idx].max(prm.source_temp * w);
            for a in 0..3 {
                vel[a][idx] += (prm.source_vel[a] - vel[a][idx]) * w * 0.5;
            }
        }
        // Forces: buoyancy, weight.
        for i in 0..n {
            vel[1][i] += dt * (prm.buoyancy * temp[i] - prm.weight * density[i]);
        }
        // Vorticity confinement.
        if prm.vorticity > 0.0 {
            let (nx, ny, nz) = (dims[0] as i64, dims[1] as i64, dims[2] as i64);
            let mut curl = vec![[0.0f32; 3]; n];
            let mut mag = vec![0.0f32; n];
            for k in 0..nz {
                for j in 0..ny {
                    for i in 0..nx {
                        let c = [
                            (vel[2][at(dims, i, j + 1, k)] - vel[2][at(dims, i, j - 1, k)] - vel[1][at(dims, i, j, k + 1)] + vel[1][at(dims, i, j, k - 1)]) / (2.0 * h),
                            (vel[0][at(dims, i, j, k + 1)] - vel[0][at(dims, i, j, k - 1)] - vel[2][at(dims, i + 1, j, k)] + vel[2][at(dims, i - 1, j, k)]) / (2.0 * h),
                            (vel[1][at(dims, i + 1, j, k)] - vel[1][at(dims, i - 1, j, k)] - vel[0][at(dims, i, j + 1, k)] + vel[0][at(dims, i, j - 1, k)]) / (2.0 * h),
                        ];
                        let idx = at(dims, i, j, k);
                        curl[idx] = c;
                        mag[idx] = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
                    }
                }
            }
            for k in 1..nz - 1 {
                for j in 1..ny - 1 {
                    for i in 1..nx - 1 {
                        let eta = [
                            (mag[at(dims, i + 1, j, k)] - mag[at(dims, i - 1, j, k)]) / (2.0 * h),
                            (mag[at(dims, i, j + 1, k)] - mag[at(dims, i, j - 1, k)]) / (2.0 * h),
                            (mag[at(dims, i, j, k + 1)] - mag[at(dims, i, j, k - 1)]) / (2.0 * h),
                        ];
                        let l = (eta[0] * eta[0] + eta[1] * eta[1] + eta[2] * eta[2]).sqrt();
                        if l < 1e-6 {
                            continue;
                        }
                        let nrm = [eta[0] / l, eta[1] / l, eta[2] / l];
                        let w = curl[at(dims, i, j, k)];
                        let f = [nrm[1] * w[2] - nrm[2] * w[1], nrm[2] * w[0] - nrm[0] * w[2], nrm[0] * w[1] - nrm[1] * w[0]];
                        let idx = at(dims, i, j, k);
                        for a in 0..3 {
                            vel[a][idx] += dt * prm.vorticity * h * f[a];
                        }
                    }
                }
            }
        }
        // Advect velocity, project, advect scalars.
        let g_dt = dt / h;
        let adv = [advect(&vel[0], &vel, dims, g_dt), advect(&vel[1], &vel, dims, g_dt), advect(&vel[2], &vel, dims, g_dt)];
        vel = adv;
        enforce_walls(&mut vel, dims);
        project(&mut vel, dims, h, prm.iterations);
        enforce_walls(&mut vel, dims);
        density = advect(&density, &vel, dims, g_dt);
        temp = advect(&temp, &vel, dims, g_dt);
        let (dis, cool) = ((1.0 - prm.dissipation * dt).max(0.0), (1.0 - prm.cooling * dt).max(0.0));
        density.iter_mut().for_each(|d| *d *= dis);
        temp.iter_mut().for_each(|t| *t *= cool);
    }
    SmokeState { dims, density, temp, vel }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centroid_y(s: &SmokeState, prm: &SmokeParams) -> (f32, f32) {
        let [nx, ny, nz] = s.dims;
        let (mut sum, mut wy) = (0.0f64, 0.0f64);
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let d = s.density[(k * ny + j) * nx + i] as f64;
                    sum += d;
                    wy += d * (prm.origin[1] + j as f32 * prm.voxel) as f64;
                }
            }
        }
        ((wy / sum.max(1e-9)) as f32, sum as f32)
    }

    #[test]
    fn plume_rises_stays_bounded_and_is_incompressible() {
        let prm = SmokeParams { dims: [24, 40, 24], voxel: 0.1, origin: [-1.2, 0.0, -1.2], frame: 40, iterations: 60, ..Default::default() };
        let src = [[0.0, 0.3, 0.0]];
        let s = simulate(&src, &prm);
        assert!(s.density.iter().all(|d| d.is_finite() && *d >= -1e-4 && *d < 6.0));
        let (cy, mass) = centroid_y(&s, &prm);
        assert!(mass > 1.0, "smoke exists: {mass}");
        assert!(cy > 0.8, "smoke rose above the source (centroid y = {cy})");
        // After projection the field is nearly divergence-free in the interior.
        let div = divergence(&s.vel, s.dims, prm.voxel);
        let [nx, ny, nz] = s.dims;
        let mut max_interior = 0.0f32;
        let mut max_speed = 0.0f32;
        for k in 2..nz - 2 {
            for j in 2..ny - 2 {
                for i in 2..nx - 2 {
                    let idx = (k * ny + j) * nx + i;
                    max_interior = max_interior.max(div[idx].abs());
                    max_speed = max_speed.max(s.vel[1][idx].abs());
                }
            }
        }
        assert!(max_speed > 0.2, "there is flow: {max_speed}");
        assert!(max_interior < 0.6 * max_speed / prm.voxel, "divergence {max_interior} vs scale {}", max_speed / prm.voxel);
    }

    #[test]
    fn deterministic_and_thread_independent() {
        let prm = SmokeParams { dims: [16, 24, 16], voxel: 0.12, origin: [-1.0, 0.0, -1.0], frame: 12, ..Default::default() };
        let src = [[0.0, 0.3, 0.0]];
        let run = |threads| rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap().install(|| simulate(&src, &prm).density);
        assert_eq!(run(1), run(6));
    }

    #[test]
    fn no_source_means_no_smoke() {
        let s = simulate(&[], &SmokeParams { dims: [8, 8, 8], frame: 5, ..Default::default() });
        assert!(s.density.iter().all(|d| *d == 0.0));
    }
}
