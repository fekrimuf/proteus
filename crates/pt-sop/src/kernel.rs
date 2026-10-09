//! Point-kernel IR: a tiny register machine over one vec3 position. Consecutive pointwise
//! nodes are concatenated into one op list and executed in a single pass over memory.
//! The interpreter is the reference backend; every element is independent, so results do
//! not depend on thread count.

use pt_core::math::{self, Mat34};
use pt_core::noise::fbm;
use pt_core::{Geometry, Instances};
use rayon::prelude::*;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum PointOp {
    /// p = M * p (also composed onto instance transforms).
    Affine(Mat34),
    /// p += dir * amp * fbm(freq * (p + offset))
    Noise { amp: f32, freq: f32, octaves: u32, dir: [f32; 3], off: [f32; 3], seed: u32 },
}

#[inline]
fn run_point(ops: &[PointOp], mut p: [f32; 3]) -> [f32; 3] {
    for op in ops {
        match op {
            PointOp::Affine(m) => p = math::xform_point(m, p),
            PointOp::Noise { amp, freq, octaves, dir, off, seed } => {
                let d = amp * fbm([p[0] + off[0], p[1] + off[1], p[2] + off[2]], *freq, *octaves, *seed);
                p = [p[0] + dir[0] * d, p[1] + dir[1] * d, p[2] + dir[2] * d];
            }
        }
    }
    p
}

const CHUNK: usize = 8192;

/// Apply a fused op list to a geometry in one pass. Instance transforms receive the affine
/// ops only (noise is not defined on instance origins).
pub fn apply(g: &Geometry, ops: &[PointOp]) -> Geometry {
    let mut out = g.clone();
    if !ops.is_empty() {
        out.p_mut().par_chunks_mut(CHUNK).for_each(|c| {
            for p in c {
                *p = run_point(ops, *p);
            }
        });
        if let Some(inst) = &g.instances {
            let mats: Vec<Mat34> = ops.iter().filter_map(|o| if let PointOp::Affine(m) = o { Some(*m) } else { None }).collect();
            if !mats.is_empty() {
                let mut xf = (*inst.xf).clone();
                for m in &mut xf {
                    for a in &mats {
                        *m = math::mul34(a, m);
                    }
                }
                out.instances = Some(Instances { xf: Arc::new(xf), ..inst.clone() });
            }
        }
    }
    out
}
