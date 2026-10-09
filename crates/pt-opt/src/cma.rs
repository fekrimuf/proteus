//! CMA-ES (Hansen's (mu/mu_w, lambda) variant) with a deterministic counter-based RNG and a
//! Jacobi eigen-solver, so a given (seed, objective) always yields the same optimum.

use pt_core::math;
use pt_core::rng::rand4;

pub struct Cma {
    n: usize,
    lambda: usize,
    mu: usize,
    weights: Vec<f64>,
    mueff: f64,
    cc: f64,
    cs: f64,
    c1: f64,
    cmu: f64,
    damps: f64,
    chi_n: f64,
    pub mean: Vec<f64>,
    pub sigma: f64,
    c: Vec<f64>,
    pc: Vec<f64>,
    ps: Vec<f64>,
    b: Vec<f64>,
    d: Vec<f64>,
    seed: i64,
    counter: u64,
    gen: usize,
}

fn exp64(x: f64) -> f64 {
    math::pow64(std::f64::consts::E, x.clamp(-50.0, 50.0))
}

impl Cma {
    pub fn new(x0: &[f64], sigma0: f64, seed: i64) -> Cma {
        let n = x0.len();
        let nf = n as f64;
        let lambda = 4 + (3.0 * math::ln(nf as f32) as f64).floor() as usize;
        let mu = lambda / 2;
        let mut w: Vec<f64> = (1..=mu).map(|i| (mu as f64 + 0.5).ln_det() - (i as f64).ln_det()).collect();
        let s: f64 = w.iter().sum();
        w.iter_mut().for_each(|x| *x /= s);
        let mueff = 1.0 / w.iter().map(|x| x * x).sum::<f64>();
        let cc = (4.0 + mueff / nf) / (nf + 4.0 + 2.0 * mueff / nf);
        let cs = (mueff + 2.0) / (nf + mueff + 5.0);
        let c1 = 2.0 / ((nf + 1.3) * (nf + 1.3) + mueff);
        let cmu = (1.0 - c1).min(2.0 * (mueff - 2.0 + 1.0 / mueff) / ((nf + 2.0) * (nf + 2.0) + mueff));
        let damps = 1.0 + 2.0 * (((mueff - 1.0) / (nf + 1.0)).sqrt() - 1.0).max(0.0) + cs;
        let chi_n = nf.sqrt() * (1.0 - 1.0 / (4.0 * nf) + 1.0 / (21.0 * nf * nf));
        let mut c = vec![0.0; n * n];
        let mut b = vec![0.0; n * n];
        for i in 0..n {
            c[i * n + i] = 1.0;
            b[i * n + i] = 1.0;
        }
        Cma { n, lambda, mu, weights: w, mueff, cc, cs, c1, cmu, damps, chi_n, mean: x0.to_vec(), sigma: sigma0, c, pc: vec![0.0; n], ps: vec![0.0; n], b, d: vec![1.0; n], seed, counter: 0, gen: 0 }
    }

    pub fn lambda(&self) -> usize {
        self.lambda
    }

    fn normal(&mut self) -> f64 {
        let r = rand4(self.seed, 7, self.counter);
        self.counter += 1;
        let u1 = (r[0] as f64).max(1e-7);
        let (_, c) = math::sin_cos(2.0 * math::PI * r[1]);
        ((-2.0 * math::ln(u1 as f32) as f64).sqrt()) * c as f64
    }

    /// Sample lambda candidates (unbounded; the caller clamps).
    pub fn ask(&mut self) -> Vec<Vec<f64>> {
        let n = self.n;
        (0..self.lambda)
            .map(|_| {
                let z: Vec<f64> = (0..n).map(|_| self.normal()).collect();
                (0..n)
                    .map(|i| {
                        let mut y = 0.0;
                        for j in 0..n {
                            y += self.b[i * n + j] * self.d[j] * z[j];
                        }
                        self.mean[i] + self.sigma * y
                    })
                    .collect()
            })
            .collect()
    }

    /// Update from evaluated candidates (lower f is better).
    pub fn tell(&mut self, xs: &[Vec<f64>], fs: &[f64]) {
        let n = self.n;
        let mut order: Vec<usize> = (0..xs.len()).collect();
        order.sort_by(|&a, &b| fs[a].partial_cmp(&fs[b]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        let old = self.mean.clone();
        let mut mean = vec![0.0; n];
        for (r, &k) in order.iter().take(self.mu).enumerate() {
            for i in 0..n {
                mean[i] += self.weights[r] * xs[k][i];
            }
        }
        self.mean = mean;
        let yw: Vec<f64> = (0..n).map(|i| (self.mean[i] - old[i]) / self.sigma).collect();
        // C^{-1/2} yw = B diag(1/D) B^T yw
        let mut t = vec![0.0; n];
        for j in 0..n {
            for i in 0..n {
                t[j] += self.b[i * n + j] * yw[i];
            }
            t[j] /= self.d[j].max(1e-12);
        }
        let mut inv = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                inv[i] += self.b[i * n + j] * t[j];
            }
        }
        let k = (self.cs * (2.0 - self.cs) * self.mueff).sqrt();
        for i in 0..n {
            self.ps[i] = (1.0 - self.cs) * self.ps[i] + k * inv[i];
        }
        let psn = self.ps.iter().map(|x| x * x).sum::<f64>().sqrt();
        let hsig = psn / (1.0 - math::pow64(1.0 - self.cs, 2.0 * (self.gen as f64 + 1.0))).sqrt() / self.chi_n < 1.4 + 2.0 / (n as f64 + 1.0);
        let hs = if hsig { 1.0 } else { 0.0 };
        let kc = (self.cc * (2.0 - self.cc) * self.mueff).sqrt();
        for i in 0..n {
            self.pc[i] = (1.0 - self.cc) * self.pc[i] + hs * kc * yw[i];
        }
        let old_c = self.c.clone();
        for i in 0..n {
            for j in 0..n {
                let mut rank_mu = 0.0;
                for (r, &k) in order.iter().take(self.mu).enumerate() {
                    rank_mu += self.weights[r] * ((xs[k][i] - old[i]) / self.sigma) * ((xs[k][j] - old[j]) / self.sigma);
                }
                self.c[i * n + j] = (1.0 - self.c1 - self.cmu) * old_c[i * n + j] + self.c1 * (self.pc[i] * self.pc[j] + (1.0 - hs) * self.cc * (2.0 - self.cc) * old_c[i * n + j]) + self.cmu * rank_mu;
            }
        }
        self.sigma *= exp64((self.cs / self.damps) * (psn / self.chi_n - 1.0)).min(2.0);
        self.gen += 1;
        // Symmetrize and eigen-decompose.
        for i in 0..n {
            for j in 0..i {
                let m = 0.5 * (self.c[i * n + j] + self.c[j * n + i]);
                self.c[i * n + j] = m;
                self.c[j * n + i] = m;
            }
        }
        let (vals, vecs) = jacobi_eigen(&self.c, n);
        self.d = vals.iter().map(|v| v.max(1e-20).sqrt()).collect();
        self.b = vecs;
    }
}

trait LnDet {
    fn ln_det(self) -> f64;
}
impl LnDet for f64 {
    fn ln_det(self) -> f64 {
        math::ln(self as f32) as f64
    }
}

/// Cyclic Jacobi for a symmetric matrix: returns (eigenvalues, eigenvectors as columns, row-major).
fn jacobi_eigen(a: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut a = a.to_vec();
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _ in 0..60 {
        let mut off = 0.0;
        for i in 0..n {
            for j in 0..i {
                off += a[i * n + j] * a[i * n + j];
            }
        }
        if off < 1e-22 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq.abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (akp, akq) = (a[k * n + p], a[k * n + q]);
                    a[k * n + p] = c * akp - s * akq;
                    a[k * n + q] = s * akp + c * akq;
                }
                for k in 0..n {
                    let (apk, aqk) = (a[p * n + k], a[q * n + k]);
                    a[p * n + k] = c * apk - s * aqk;
                    a[q * n + k] = s * apk + c * aqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl Fn(&[f64]) -> f64, x0: &[f64], gens: usize, seed: i64) -> (Vec<f64>, f64) {
        let mut c = Cma::new(x0, 0.3, seed);
        let mut best = (x0.to_vec(), f(x0));
        for _ in 0..gens {
            let xs = c.ask();
            let fs: Vec<f64> = xs.iter().map(|x| f(x)).collect();
            for (x, v) in xs.iter().zip(&fs) {
                if *v < best.1 {
                    best = (x.clone(), *v);
                }
            }
            c.tell(&xs, &fs);
        }
        best
    }

    #[test]
    fn minimizes_sphere_and_rosenbrock_deterministically() {
        let sphere = |x: &[f64]| x.iter().map(|v| (v - 0.3) * (v - 0.3)).sum::<f64>();
        let (x, f) = run(sphere, &[0.9; 5], 80, 1);
        assert!(f < 1e-8, "sphere {f} at {x:?}");
        let rosen = |x: &[f64]| (0..x.len() - 1).map(|i| 100.0 * (x[i + 1] - x[i] * x[i]).powi(2) + (1.0 - x[i]).powi(2)).sum::<f64>();
        let (x, f) = run(rosen, &[-0.5, 0.5, -0.5], 400, 2);
        assert!(f < 1e-3, "rosenbrock {f} at {x:?}");
        let (x2, f2) = run(rosen, &[-0.5, 0.5, -0.5], 400, 2);
        assert_eq!((x, f), (x2, f2), "same seed -> identical run");
    }

    #[test]
    fn jacobi_recovers_eigenvalues() {
        let a = [2.0, 1.0, 0.0, 1.0, 2.0, 0.0, 0.0, 0.0, 5.0];
        let (mut vals, _) = jacobi_eigen(&a, 3);
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((vals[0] - 1.0).abs() < 1e-9 && (vals[1] - 3.0).abs() < 1e-9 && (vals[2] - 5.0).abs() < 1e-9, "{vals:?}");
    }
}
