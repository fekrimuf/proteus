//! Objectives: scalar losses over a cooked node (lower is better).

use pt_core::math;
use pt_graph::Graph;
use pt_hcs::SpecDef;
use pt_render::{Image, Mode, Opts};
use std::collections::HashMap;

/// Nearest-neighbour search over a point set (uniform hash grid).
pub struct Nn {
    pts: Vec<[f32; 3]>,
    cell: f32,
    lo: [f32; 3],
    grid: HashMap<[i32; 3], Vec<u32>>,
    reach: i32,
}

impl Nn {
    pub fn new(pts: Vec<[f32; 3]>) -> Nn {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &pts {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        if pts.is_empty() {
            lo = [0.0; 3];
            hi = [1.0; 3];
        }
        let ext = [0, 1, 2].map(|a| (hi[a] - lo[a]).max(1e-3));
        // Cell size so each cell holds a few points on a surface-like distribution.
        let cell = ((ext[0] * ext[1] + ext[1] * ext[2] + ext[0] * ext[2]) * 2.0 / pts.len().max(1) as f32 * 4.0).sqrt().max(1e-4);
        let mut grid: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (i, p) in pts.iter().enumerate() {
            grid.entry([0, 1, 2].map(|a| ((p[a] - lo[a]) / cell).floor() as i32)).or_default().push(i as u32);
        }
        let reach = (ext[0].max(ext[1]).max(ext[2]) / cell).ceil() as i32 + 2;
        Nn { pts, cell, lo, grid, reach }
    }

    pub fn nearest_dist(&self, q: [f32; 3]) -> f32 {
        let k = [0, 1, 2].map(|a| ((q[a] - self.lo[a]) / self.cell).floor() as i32);
        let mut best = f32::INFINITY;
        for r in 0..=self.reach {
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                            continue;
                        }
                        if let Some(l) = self.grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                            for &i in l {
                                let d = math::sub(self.pts[i as usize], q);
                                best = best.min(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
                            }
                        }
                    }
                }
            }
            if best.is_finite() && best.sqrt() <= r as f32 * self.cell {
                break;
            }
        }
        best.sqrt()
    }
}

pub enum Objective {
    /// Symmetric mean Chamfer distance to a target point cloud, normalized by its bbox diagonal.
    Chamfer { target: Nn, target_pts: Vec<[f32; 3]>, diag: f32, samples: usize },
    /// Match the bounding-box size (relative error).
    Size { target: [f32; 3] },
    /// 1 - mean IoU of silhouettes from front/side/top views framed on `bounds`.
    Silhouette { masks: Vec<Vec<bool>>, bounds: ([f32; 3], [f32; 3]), res: usize },
    /// Mean squared luminance error against a reference image rendered with the same framing.
    Image { reference: Image, opts: Opts },
    /// Violation distance of declared specs (0 = all pass).
    Specs(Vec<SpecDef>),
    /// Weighted sum.
    Sum(Vec<(f64, Objective)>),
}

pub const SIL_VIEWS: [(f32, f32); 3] = [(0.0, 0.0), (90.0, 0.0), (0.0, 89.0)];

impl Objective {
    pub fn chamfer(target_geo: &pt_core::Geometry, samples: usize) -> Result<Objective, String> {
        let g = target_geo.realize();
        let pts = if g.triangles().is_empty() { g.p().to_vec() } else { pt_sop::sample_surface(&g, samples.max(500) * 2, 17)? };
        let (lo, hi) = g.bounds().ok_or("target is empty")?;
        let diag = math::sub(hi, lo);
        let diag = (diag[0] * diag[0] + diag[1] * diag[1] + diag[2] * diag[2]).sqrt().max(1e-6);
        Ok(Objective::Chamfer { target: Nn::new(pts.clone()), target_pts: pts, diag, samples })
    }

    pub fn silhouette(target_geo: &pt_core::Geometry, res: usize) -> Result<Objective, String> {
        let g = target_geo.realize();
        let (lo, hi) = g.bounds().ok_or("target is empty")?;
        let pad = math::sub(hi, lo);
        let (lo, hi) = ([0, 1, 2].map(|a| lo[a] - pad[a] * 0.25), [0, 1, 2].map(|a| hi[a] + pad[a] * 0.25));
        let masks = SIL_VIEWS.iter().map(|v| pt_render::silhouette(&g, &Opts { width: res, height: res, azimuth_deg: v.0, elevation_deg: v.1, fit_bounds: Some((lo, hi)), mode: Mode::Shaded, ..Default::default() })).collect();
        Ok(Objective::Silhouette { masks, bounds: (lo, hi), res })
    }

    pub fn eval(&self, g: &mut Graph, node: &str) -> Result<f64, String> {
        let geo = g.cook(node)?;
        Ok(match self {
            Objective::Chamfer { target, target_pts, diag, samples } => {
                let real = geo.realize();
                let pts = if real.triangles().is_empty() { real.p().to_vec() } else { pt_sop::sample_surface(&real, *samples, 23)? };
                if pts.is_empty() {
                    return Ok(1.0e3);
                }
                let a: f32 = pts.iter().map(|p| target.nearest_dist(*p)).sum::<f32>() / pts.len() as f32;
                let nn = Nn::new(pts);
                let b: f32 = target_pts.iter().map(|p| nn.nearest_dist(*p)).sum::<f32>() / target_pts.len() as f32;
                ((a + b) * 0.5 / diag) as f64
            }
            Objective::Size { target } => {
                let (lo, hi) = geo.bounds().ok_or("result is empty")?;
                (0..3).map(|a| (((hi[a] - lo[a]) - target[a]) / target[a].max(1e-6)).powi(2) as f64).sum::<f64>() / 3.0
            }
            Objective::Silhouette { masks, bounds, res } => {
                let real = geo.realize();
                let mut iou = 0.0;
                for (m, v) in masks.iter().zip(SIL_VIEWS) {
                    let r = pt_render::silhouette(&real, &Opts { width: *res, height: *res, azimuth_deg: v.0, elevation_deg: v.1, fit_bounds: Some(*bounds), mode: Mode::Shaded, ..Default::default() });
                    let (mut i, mut u) = (0usize, 0usize);
                    for (a, b) in m.iter().zip(&r) {
                        i += (*a && *b) as usize;
                        u += (*a || *b) as usize;
                    }
                    iou += if u == 0 { 1.0 } else { i as f64 / u as f64 };
                }
                1.0 - iou / masks.len() as f64
            }
            Objective::Image { reference, opts } => {
                let (img, _) = pt_render::render(&geo, opts);
                if img.w != reference.w || img.h != reference.h {
                    return Err("image objective: render size differs from the reference".into());
                }
                let lum = |p: &[u8]| (0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64) / 255.0;
                img.rgb.chunks(3).zip(reference.rgb.chunks(3)).map(|(a, b)| (lum(a) - lum(b)).powi(2)).sum::<f64>() / (img.w * img.h) as f64
            }
            Objective::Specs(specs) => {
                let results = pt_hcs::evaluate_specs(g, specs);
                let mut loss = 0.0;
                for (s, r) in specs.iter().zip(&results) {
                    if r.ok {
                        continue;
                    }
                    loss += match r.measured {
                        Some(m) => {
                            let below = s.min.map_or(0.0, |lo| (lo - m).max(0.0) / lo.abs().max(1.0));
                            let above = s.max.map_or(0.0, |hi| (m - hi).max(0.0) / hi.abs().max(1.0));
                            (below + above).max(1e-3)
                        }
                        None => 1.0,
                    };
                }
                loss
            }
            Objective::Sum(parts) => {
                let mut t = 0.0;
                for (w, o) in parts {
                    t += w * o.eval(g, node)?;
                }
                t
            }
        })
    }
}
