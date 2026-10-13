//! Gradient-based parameter fitting through the autodiff interpreter: recover `ch()` values
//! of a program so that one output attribute component matches target data (Adam on MSE).
//! Forward-mode tangents make this exact for smooth programs with up to MAX_GRAD channels.

use crate::{Inputs, Program, Ty, MAX_GRAD};

pub struct FitResult {
    pub channels: Vec<f32>,
    pub loss_history: Vec<f32>,
}

/// Fit `prog`'s channels (starting at `init`, clamped to `bounds` if given) so that
/// `attr[comp]` matches `target` per element. `make` builds the interpreter inputs for a
/// candidate channel vector (the caller owns the attribute data the inputs borrow).
pub fn fit_channels<'a>(
    prog: &Program,
    make: impl Fn(&[f32]) -> Inputs<'a>,
    attr: &str,
    comp: usize,
    target: &[f32],
    init: &[f32],
    bounds: Option<&[(f32, f32)]>,
    steps: usize,
    lr: f32,
) -> Option<FitResult> {
    let ai = prog.attrs.iter().position(|a| a.name == attr && a.written)?;
    let c = if prog.attrs[ai].ty == Ty::Float { 0 } else { comp.min(2) };
    let nch = prog.chans.len().min(MAX_GRAD);
    let mut x: Vec<f32> = init.iter().copied().take(nch).collect();
    x.resize(nch, 0.0);
    let (mut m, mut v) = (vec![0.0f32; nch], vec![0.0f32; nch]);
    let (b1, b2, eps) = (0.9f32, 0.999f32, 1e-8f32);
    let mut hist = Vec::with_capacity(steps);
    for step in 1..=steps {
        let out = prog.run::<MAX_GRAD>(&make(&x));
        let (_, vals, tans) = out.written.iter().find(|w| w.0 == ai)?;
        let n = target.len().min(vals.len()).max(1) as f32;
        let mut grad = vec![0.0f32; nch];
        let mut loss = 0.0f32;
        for i in 0..target.len().min(vals.len()) {
            let e = vals[i][c] - target[i];
            loss += e * e / n;
            for (k, g) in grad.iter_mut().enumerate() {
                *g += 2.0 * e * tans[i][k][c] / n;
            }
        }
        hist.push(loss);
        for k in 0..nch {
            m[k] = b1 * m[k] + (1.0 - b1) * grad[k];
            v[k] = b2 * v[k] + (1.0 - b2) * grad[k] * grad[k];
            let (mh, vh) = (m[k] / (1.0 - pt_core::math::powi(b1, step as i32)), v[k] / (1.0 - pt_core::math::powi(b2, step as i32)));
            x[k] -= lr * mh / (vh.sqrt() + eps);
            if let Some(b) = bounds {
                x[k] = x[k].clamp(b[k].0, b[k].1);
            }
        }
    }
    Some(FitResult { channels: x, loss_history: hist })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyze, AttrView};
    use std::collections::BTreeMap;

    #[test]
    fn recovers_channels_from_synthetic_data() {
        let code = "@P.y = ch(\"a\") * @P.x + sin(ch(\"b\") * @P.z) + ch(\"c\");";
        let env = BTreeMap::from([("P".to_string(), Ty::Vec3)]);
        let prog = analyze(code, &env).unwrap();
        let pts: Vec<[f32; 3]> = (0..400).map(|i| [(i % 20) as f32 * 0.15, 0.0, (i / 20) as f32 * 0.15]).collect();
        let make = |ch: &[f32]| Inputs { n: pts.len(), attrs: vec![Some(AttrView::V(&pts))], chans: ch.iter().map(|&c| [c; 3]).collect(), frame: 1.0 };
        let truth = [1.7f32, 0.9, -0.4];
        let out = prog.run::<0>(&make(&truth));
        let target: Vec<f32> = out.written[0].1.iter().map(|v| v[1]).collect();
        let r = fit_channels(&prog, make, "P", 1, &target, &[0.5, 0.6, 0.3], None, 600, 0.03).unwrap();
        let first = r.loss_history[0];
        let last = *r.loss_history.last().unwrap();
        assert!(last < first * 1e-4, "loss {first} -> {last}");
        for (got, want) in r.channels.iter().zip(truth) {
            assert!((got - want).abs() < 0.02, "recovered {:?} vs {truth:?}", r.channels);
        }
    }
}
