//! pt-vex: a small typed, deterministic, differentiable per-element expression language
//! (VEX-style snippets: `@P.y += noise(@P * ch("freq")) * ch("amp");`).
//!
//! Types: `float`, `vec3`. Attributes are `@name`. Tunable parameters are `ch("name")` /
//! `chv("name")`. Loops are bounded (`for i in 0..8 { }`) so programs stay differentiable.
//! The same interpreter runs plain (G=0) or with forward-mode gradients w.r.t. `ch()` values.

pub mod check;
pub mod fit;
mod interp;
mod lex;
mod parse;

pub use check::analyze;
pub use interp::{AttrView, Inputs, Outputs};

use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Float,
    Vec3,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Float => "f32",
            Ty::Vec3 => "v3",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub line: u32,
    pub col: u32,
    pub msg: String,
    pub fix: String,
}

impl Diag {
    pub fn new(line: u32, col: u32, msg: String, fix: impl Into<String>) -> Self {
        Diag { line, col, msg, fix: fix.into() }
    }
    pub fn to_json(&self) -> Value {
        json!({"line": self.line, "col": self.col, "message": self.msg, "fix": self.fix})
    }
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}: {} (fix: {})", self.line, self.col, self.msg, self.fix)
    }
}

#[derive(Clone, Debug)]
pub struct AttrSlot {
    pub name: String,
    pub ty: Ty,
    pub slot: usize,
    pub read: bool,
    pub written: bool,
    /// Present on the input geometry (vs created by this program).
    pub exists: bool,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub stmts: Vec<check::IStmt>,
    pub nslots: usize,
    pub attrs: Vec<AttrSlot>,
    pub chans: Vec<(String, Ty)>,
}

/// Number of gradient channels available to `sum_grad`.
pub const MAX_GRAD: usize = 8;

impl Program {
    /// Gradient of `loss = sum_over_points(attr[comp])` with respect to each channel
    /// (first `MAX_GRAD` channels), by forward-mode autodiff. Returns (loss, grads).
    pub fn sum_grad(&self, inp: &Inputs, attr: &str, comp: usize) -> Option<(f32, Vec<f32>)> {
        let ai = self.attrs.iter().position(|a| a.name == attr && a.written)?;
        let out = self.run::<MAX_GRAD>(inp);
        let (_, vals, tans) = out.written.iter().find(|w| w.0 == ai)?;
        let c = if self.attrs[ai].ty == Ty::Float { 0 } else { comp.min(2) };
        let loss: f64 = vals.iter().map(|v| v[c] as f64).sum();
        let mut g = vec![0.0f64; MAX_GRAD];
        for t in tans {
            for (k, gk) in g.iter_mut().enumerate() {
                *gk += t[k][c] as f64;
            }
        }
        Some((loss as f32, g.into_iter().take(self.chans.len().min(MAX_GRAD)).map(|x| x as f32).collect()))
    }
}

/// Closest candidate by edit distance (for "did you mean" hints).
pub fn suggest<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    fn lev(a: &str, b: &str) -> usize {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut cur = vec![i];
            for j in 1..=b.len() {
                let cost = usize::from(a[i - 1] != b[j - 1]);
                cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
            }
            prev = cur;
        }
        prev[b.len()]
    }
    candidates.iter().map(|c| (lev(&name.to_lowercase(), &c.to_lowercase()), *c)).filter(|(d, _)| *d <= 2.max(name.len() / 3)).min_by_key(|x| x.0).map(|x| x.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env() -> BTreeMap<String, Ty> {
        BTreeMap::from([("P".to_string(), Ty::Vec3), ("pscale".to_string(), Ty::Float)])
    }

    fn pts() -> Vec<[f32; 3]> {
        (0..200).map(|i| [i as f32 * 0.137 % 3.0, (i as f32 * 0.071) % 2.0, (i as f32 * 0.31) % 5.0]).collect()
    }

    fn run_sum(code: &str, chans: &[f32], attr: &str, comp: usize) -> (f32, Vec<f32>) {
        let prog = analyze(code, &env()).unwrap_or_else(|e| panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")));
        let p = pts();
        let ps: Vec<f32> = (0..p.len()).map(|i| 0.5 + (i % 7) as f32 * 0.1).collect();
        let attrs = prog.attrs.iter().map(|a| match a.name.as_str() {
            "P" => Some(AttrView::V(&p)),
            "pscale" => Some(AttrView::F(&ps)),
            _ => None,
        }).collect();
        let inp = Inputs { n: p.len(), attrs, chans: chans.iter().map(|&c| [c; 3]).collect(), frame: 1.0 };
        prog.sum_grad(&inp, attr, comp).unwrap()
    }

    #[test]
    fn evaluates_simple_program() {
        let (loss, _) = run_sum("@P.y = @P.x * 2.0 + 1.0;", &[], "P", 1);
        let want: f32 = pts().iter().map(|p| p[0] * 2.0 + 1.0).sum();
        assert!((loss - want).abs() < 1e-2, "{loss} vs {want}");
    }

    /// Forward-mode gradients must match central finite differences for smooth programs.
    #[test]
    fn autodiff_matches_finite_differences() {
        let programs = [
            ("@P.y = ch(\"a\") * @P.x + sin(ch(\"b\") * @P.z);", vec![1.3, 0.7]),
            ("float k = ch(\"a\"); @P.y = sqrt(@P.x * k + 1.0) * exp(-0.3 * ch(\"b\"));", vec![0.9, 1.1]),
            ("@P.y = pow(@P.x + 1.0, ch(\"a\")) + ln(@P.z + 2.0) * ch(\"b\");", vec![1.4, 0.6]),
            ("vec3 q = @P * ch(\"a\"); @P.y = length(q) + dot(q, {1.0, 2.0, 3.0}) * ch(\"b\");", vec![0.8, 0.2]),
            ("@P.y = mix(@P.x, @P.z, smoothstep(0.0, 5.0, ch(\"a\") * @P.z)) * ch(\"b\");", vec![1.2, 0.9]),
            ("float s = 0.0; for i in 0..4 { s += sin(ch(\"a\") * @P.x + i) * ch(\"b\"); } @P.y = s;", vec![1.1, 0.4]),
            ("if (@P.x > 1.0) { @P.y = ch(\"a\") * @P.x; } else { @P.y = ch(\"b\") * @P.z; }", vec![0.5, 1.5]),
            ("@P.y = fit(@P.x, 0.0, 3.0, ch(\"a\"), ch(\"b\")) + normalize(@P + {1.0,1.0,1.0}).y * ch(\"a\");", vec![2.0, 4.0]),
            ("@P.y = atan2(@P.z * ch(\"a\") + 0.5, @P.x + ch(\"b\") + 2.0);", vec![1.2, 0.3]),
            ("@P.y = asin(clamp(@P.x * 0.2 * ch(\"a\"), -0.9, 0.9)) + acos(clamp(@P.z * 0.1 * ch(\"b\"), -0.9, 0.9)) + atan(ch(\"a\") * @P.x);", vec![1.1, 0.8]),
            ("@P.y = radians(ch(\"a\")) * @P.x + degrees(ch(\"b\")) * 0.01 * @P.z;", vec![0.9, 1.3]),
        ];
        for (code, ch) in programs {
            let (_, g) = run_sum(code, &ch, "P", 1);
            for k in 0..ch.len() {
                let h = 1e-2f32;
                let (mut hi, mut lo) = (ch.clone(), ch.clone());
                hi[k] += h;
                lo[k] -= h;
                let fd = (run_sum(code, &hi, "P", 1).0 - run_sum(code, &lo, "P", 1).0) / (2.0 * h);
                let tol = 2e-2 * fd.abs().max(1.0);
                assert!((g[k] - fd).abs() < tol, "grad mismatch ch{k} in `{code}`: autodiff {} vs fd {fd}", g[k]);
            }
        }
    }


    #[test]
    fn mod_and_inverse_trig_values() {
        let (loss, _) = run_sum("@P.y = mod(@P.x - 4.0, 1.5);", &[], "P", 1);
        let want: f32 = pts().iter().map(|p| { let a = p[0] - 4.0; a - 1.5 * (a / 1.5).floor() }).sum();
        assert!((loss - want).abs() < 1e-3, "mod {loss} vs {want}");
        let (loss, _) = run_sum("@P.y = atan2(@P.z, @P.x) + degrees(acos(0.5)) * 0.0 + asin(0.5);", &[], "P", 1);
        let want: f32 = pts().iter().map(|p| p[2].atan2(p[0]) + 0.5f32.asin()).sum();
        assert!((loss - want).abs() < 1e-3, "atan2 {loss} vs {want}");
    }
    #[test]
    fn noise_gradient_is_piecewise_but_close() {
        let code = "@P.y = noise(@P * ch(\"f\")) * ch(\"a\");";
        let ch = vec![0.7, 1.5];
        let (_, g) = run_sum(code, &ch, "P", 1);
        let h = 1e-2f32;
        for k in 0..2 {
            let (mut hi, mut lo) = (ch.clone(), ch.clone());
            hi[k] += h;
            lo[k] -= h;
            let fd = (run_sum(code, &hi, "P", 1).0 - run_sum(code, &lo, "P", 1).0) / (2.0 * h);
            assert!((g[k] - fd).abs() < 0.05 * fd.abs().max(1.0), "noise grad ch{k}: {} vs {fd}", g[k]);
        }
    }

    #[test]
    fn diagnostics_have_positions_and_fixes() {
        let e = analyze("@P.y = @Nn.x;", &env()).unwrap_err();
        assert!(e[0].msg.contains("@Nn") && e[0].line == 1);
        let e = analyze("@P.y = nois(@P);", &env()).unwrap_err();
        assert!(e[0].fix.contains("noise"));
        let e = analyze("@P = 1.0;", &env()).unwrap_err();
        assert!(e[0].fix.contains("vec3"));
        let e = analyze("float x = 1.0;\nx = {1,2,3};", &env()).unwrap_err();
        assert_eq!(e[0].line, 2);
        let e = analyze("for i in 0..1000 { }", &env()).unwrap_err();
        assert!(e[0].msg.contains("loop"));
        let e = analyze("@P.y = ;", &env()).unwrap_err();
        assert!(e[0].msg.contains("expected a value"));
    }

    #[test]
    fn reads_and_writes_are_tracked() {
        let p = analyze("@Cd = vec3(@pscale); @pscale *= 2.0;", &env()).unwrap();
        let cd = p.attrs.iter().find(|a| a.name == "Cd").unwrap();
        assert!(cd.written && !cd.exists);
        let ps = p.attrs.iter().find(|a| a.name == "pscale").unwrap();
        assert!(ps.read && ps.written && ps.exists);
    }
}
