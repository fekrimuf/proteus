//! Recipe-level tools: count the knobs in a recipe's source, and tune its numbers against a reference.
//!
//! A *knob* is a numeric literal a person could change: every non-zero number in the source except structural integers
//! (`rows`, `cols`, `caps`, `seed`, `octaves`, `divisions`, `levels`, `iterations`), arguments of `range(...)`, and anything
//! inside strings or comments. `count=` literals are knobs but are never tuned automatically. Because the count is taken on
//! the source, a loop or a variable that is used five times counts once: this is what makes a parametric recipe
//! "editable" in the sense of PRO_QUALITY_PLAN B3 and what separates it from a pile of boxes.
//!
//! `refine_source` runs CMA-ES over the tunable literals and rewrites only those numbers, so the recipe's structure
//! (loops, mirrors, arrays, expressions) is preserved.

use crate::cma::Cma;
use crate::matching::*;
use pt_graph::Graph;
use rayon::prelude::*;
use std::time::Instant;

const STRUCTURAL: [&str; 8] = ["rows", "cols", "caps", "seed", "octaves", "divisions", "levels", "iterations"];

#[derive(Clone, Debug, PartialEq)]
pub struct Literal {
    pub start: usize,
    pub end: usize,
    pub value: f64,
    /// `count=<n>`: counted as a knob, never tuned.
    pub is_count: bool,
    /// Written with a decimal point or exponent. Whole numbers are treated as structure (counts, quarter turns)
    /// and are only tuned when the caller asks, so a recipe can say `SHELVES = 3` and `90.0` deliberately.
    pub decimal: bool,
}

/// Every knob in the source, in order of appearance.
pub fn literals(code: &str) -> Vec<Literal> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // For each open parenthesis: was it `range(`?
    let mut parens: Vec<bool> = Vec::new();
    let mut last_ident = String::new();
    let mut prev_sig = b' '; // previous significant (non-space) byte
    while i < b.len() {
        let c = b[i];
        match c {
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' | b'\'' => {
                let triple = b.get(i..i + 3).is_some_and(|s| s == [c, c, c]);
                let q = if triple { 3 } else { 1 };
                i += q;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b.get(i..i + q).is_some_and(|s| s.iter().all(|&x| x == c)) {
                        i += q;
                        break;
                    }
                    i += 1;
                }
                prev_sig = b'"';
                continue;
            }
            b'(' => {
                parens.push(last_ident == "range");
                last_ident.clear();
                prev_sig = c;
            }
            b')' => {
                parens.pop();
                prev_sig = c;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                last_ident = code[s..i].to_string();
                prev_sig = b'a';
                continue;
            }
            c if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) => {
                let s = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < b.len() && b[j].is_ascii_digit() {
                        while j < b.len() && b[j].is_ascii_digit() {
                            j += 1;
                        }
                        i = j;
                    }
                }
                let mut value: f64 = code[s..i].parse().unwrap_or(0.0);
                let mut start = s;
                // A unary minus belongs to the number.
                let mut k = s;
                while k > 0 && b[k - 1] == b' ' {
                    k -= 1;
                }
                if k > 0 && b[k - 1] == b'-' {
                    let mut m = k - 1;
                    while m > 0 && b[m - 1] == b' ' {
                        m -= 1;
                    }
                    let before = if m == 0 { b'(' } else { b[m - 1] };
                    if matches!(before, b'(' | b',' | b'[' | b'=' | b':' | b'{' | b'\n') {
                        value = -value;
                        start = k - 1;
                    }
                }
                // `name = number` with a structural name, or `count = number`.
                let mut eq = s;
                while eq > 0 && b[eq - 1] == b' ' {
                    eq -= 1;
                }
                let prev_is_eq = eq > 0 && b[eq - 1] == b'=' && !(eq > 1 && matches!(b[eq - 2], b'=' | b'<' | b'>' | b'!'));
                let kw = if prev_is_eq {
                    let mut e = eq - 1;
                    while e > 0 && b[e - 1] == b' ' {
                        e -= 1;
                    }
                    let mut st = e;
                    while st > 0 && (b[st - 1].is_ascii_alphanumeric() || b[st - 1] == b'_') {
                        st -= 1;
                    }
                    &code[st..e]
                } else {
                    ""
                };
                let in_range = parens.last().copied().unwrap_or(false);
                // `W / 2` and `3 * x` express a geometric relation, not a number somebody would tune.
                let is_small_int = !code[start..i].contains(['.', 'e', 'E']) && value.abs() <= 10.0;
                let mut nx = i;
                while nx < b.len() && b[nx] == b' ' {
                    nx += 1;
                }
                let next = b.get(nx).copied().unwrap_or(b' ');
                let relation = is_small_int && (matches!(next, b'*' | b'/') || matches!(prev_sig, b'*' | b'/'));
                if !in_range && !relation && !STRUCTURAL.contains(&kw) && value != 0.0 {
                    out.push(Literal { start, end: i, value, is_count: kw == "count", decimal: code[s..i].contains(['.', 'e', 'E']) });
                }
                prev_sig = b'0';
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => prev_sig = c,
        }
        i += 1;
    }
    out
}

/// Knobs in the source (see the module comment).
pub fn count_source_params(code: &str) -> usize {
    literals(code).len()
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.5}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

fn substitute(code: &str, lits: &[&Literal], values: &[f64]) -> String {
    let mut out = String::with_capacity(code.len());
    let mut at = 0;
    for (l, v) in lits.iter().zip(values) {
        out.push_str(&code[at..l.start]);
        out.push_str(&fmt_num(*v));
        at = l.end;
    }
    out.push_str(&code[at..]);
    out
}

/// Run a recipe and return its first output's triangles.
pub fn cook_source(code: &str) -> Result<Vec<Tri>, String> {
    let mut g = Graph::new();
    let rep = pt_hcs::run(code, &mut g, true).map_err(|d| d.first().map_or("recipe error".to_string(), |x| x.to_string()))?;
    let out = rep.outputs.first().ok_or("the recipe has no out(...)")?;
    let geo = g.cook(out)?;
    Ok(tris_of(&geo))
}

pub struct RefineInfo {
    pub tuned: usize,
    pub evals: usize,
    pub loss_before: f64,
    pub loss_after: f64,
}

/// Silhouette masks of the reference from a few views, rendered once; the refinement compares candidates to them.
struct SilTarget {
    bounds: ([f32; 3], [f32; 3]),
    masks: Vec<Vec<bool>>,
}

/// The same ten views the official score uses, at a lower resolution to keep each evaluation cheap.
const REFINE_VIEWS: [(f32, f32); 10] = MATCH_VIEWS;
const REFINE_RES: usize = 160;

fn refine_opts(v: (f32, f32), bounds: ([f32; 3], [f32; 3])) -> pt_render::Opts {
    pt_render::Opts { width: REFINE_RES, height: REFINE_RES, azimuth_deg: v.0, elevation_deg: v.1, fit_bounds: Some(bounds), mode: pt_render::Mode::Shaded, ..Default::default() }
}

impl SilTarget {
    fn new(reference_tris: &[Tri]) -> Option<SilTarget> {
        let (lo, hi) = bounds_of_tris(reference_tris)?;
        let pad = pt_core::math::sub(hi, lo);
        let bounds = ([0, 1, 2].map(|a| lo[a] - pad[a] * 0.25), [0, 1, 2].map(|a| hi[a] + pad[a] * 0.25));
        let g = geometry_from_tris(reference_tris);
        let masks = REFINE_VIEWS.iter().map(|v| pt_render::silhouette(&g, &refine_opts(*v, bounds))).collect();
        Some(SilTarget { bounds, masks })
    }

    /// 1 - mean IoU over the views.
    fn loss(&self, tris: &[Tri]) -> f64 {
        let g = geometry_from_tris(tris);
        let mut sum = 0.0;
        for (v, m) in REFINE_VIEWS.iter().zip(&self.masks) {
            let r = pt_render::silhouette(&g, &refine_opts(*v, self.bounds));
            let (mut i, mut u) = (0usize, 0usize);
            for (a, b) in m.iter().zip(&r) {
                i += (*a && *b) as usize;
                u += (*a || *b) as usize;
            }
            sum += if u == 0 { 1.0 } else { i as f64 / u as f64 };
        }
        1.0 - sum / REFINE_VIEWS.len() as f64
    }
}

fn loss_of(tris: &[Tri], ref_bvh: &TriBvh, ref_samples: &[([f32; 3], [f32; 3])], sil: Option<&SilTarget>, tau: f32, diag: f32, ref_tris: usize) -> f64 {
    let Ok(cb) = TriBvh::new(tris) else { return 1.0e3 };
    let cs = sample_tris(tris, 2500, 31);
    let a = distances_to(ref_bvh, &cs);
    let b = distances_to(&cb, ref_samples);
    let (f, _, _) = f_from(&a, &b, tau);
    let chamfer = (mean_dist(&a) + mean_dist(&b)) * 0.5 / f64::from(diag);
    let over = if tris.len() > 2 * ref_tris { 0.5 } else { 0.0 };
    // Surface agreement and silhouette agreement are both part of every tier, so both are optimised.
    (1.0 - f) + 10.0 * chamfer + sil.map_or(0.0, |s| s.loss(tris)) + over
}

/// Tune the numeric literals of `code` against `reference` (original units). Returns the rewritten source.
pub fn refine_source(code: &str, reference: &pt_core::Geometry, evals: usize, seed: i64, time_budget_s: f64, tune_integers: bool) -> Result<(String, RefineInfo), String> {
    let t0 = Instant::now();
    let raw = tris_of(reference);
    let (lo, hi) = bounds_of_tris(&raw).ok_or("reference has no triangles")?;
    let diag = diagonal(lo, hi); // raw units: sizes the search windows
    // The official score normalises each shape by its own bounding box; optimise exactly that.
    let tris = Norm::of_tris(&raw)?.tris(&raw);
    let tau = F_THRESHOLD;
    let ref_bvh = TriBvh::new(&tris)?;
    let ref_samples = sample_tris(&tris, 4000, 3);
    let sil = SilTarget::new(&tris);
    let all = literals(code);
    let tunable: Vec<&Literal> = all.iter().filter(|l| !l.is_count && (l.decimal || tune_integers)).collect();
    let eval = |c: &str| -> f64 {
        match cook_source(c) {
            Ok(t) if !t.is_empty() => match Norm::of_tris(&t) {
                Ok(n) => {
                    // The bounding-box normalisation hides absolute size, so keep the size honest separately.
                    let size = bounds_of_tris(&t).map_or(0.0, |(l, h)| diagonal(l, h)) / diag;
                    loss_of(&n.tris(&t), &ref_bvh, &ref_samples, sil.as_ref(), tau, 1.0, raw.len()) + 3.0 * f64::from((size - 1.0).abs())
                }
                Err(_) => 1.0e3,
            },
            _ => 1.0e3,
        }
    };
    let loss_before = eval(code);
    if tunable.is_empty() || evals == 0 {
        return Ok((code.to_string(), RefineInfo { tuned: 0, evals: 1, loss_before, loss_after: loss_before }));
    }
    let d = f64::from(diag);
    let bounds: Vec<(f64, f64)> = tunable
        .iter()
        .map(|l| {
            let span = (0.3 * l.value.abs()).max(0.03 * d);
            // A number never changes sign or collapses to nothing: a box with a negative size is not a recipe.
            if l.value > 0.0 {
                ((l.value - span).max(0.2 * l.value), l.value + span)
            } else {
                (l.value - span, (l.value + span).min(0.2 * l.value))
            }
        })
        .collect();
    let start: Vec<f64> = vec![0.5; tunable.len()];
    let value_of = |u: &[f64]| -> Vec<f64> { u.iter().zip(&bounds).map(|(u, b)| b.0 + u.clamp(0.0, 1.0) * (b.1 - b.0)).collect() };
    let mut best = (start.clone(), loss_before);
    let mut used = 1usize;
    // Restarts around the best point with shrinking steps: thin structures have narrow basins, so one pass with one
    // step size stalls. Each round stops on stagnation; the whole search stops when two rounds in a row gain nothing.
    let sigmas = [0.25, 0.12, 0.06, 0.12, 0.03, 0.2, 0.06, 0.02];
    let mut dry_rounds = 0;
    'rounds: for (round, &sigma) in sigmas.iter().enumerate() {
        let before_round = best.1;
        let mut cma = Cma::new(&best.0, sigma, seed.wrapping_add(round as i64 * 101));
        let mut stagnant = 0;
        while used < evals && t0.elapsed().as_secs_f64() < time_budget_s {
            let xs = cma.ask();
            let fs: Vec<f64> = xs
                .par_iter()
                .map(|x| {
                    let c: Vec<f64> = x.iter().map(|v| v.clamp(0.0, 1.0)).collect();
                    let pen: f64 = x.iter().zip(&c).map(|(a, b)| (a - b) * (a - b)).sum();
                    eval(&substitute(code, &tunable, &value_of(&c))) + 1e-3 * pen
                })
                .collect();
            used += xs.len();
            let mut improved = false;
            for (x, &f) in xs.iter().zip(&fs) {
                if f < best.1 - 1e-6 {
                    best = (x.iter().map(|v| v.clamp(0.0, 1.0)).collect(), f);
                    improved = true;
                }
            }
            stagnant = if improved { 0 } else { stagnant + 1 };
            cma.tell(&xs, &fs);
            if cma.sigma < 1e-3 || stagnant > 15 {
                break;
            }
        }
        if used >= evals || t0.elapsed().as_secs_f64() >= time_budget_s {
            break 'rounds;
        }
        dry_rounds = if best.1 < before_round - 1e-4 { 0 } else { dry_rounds + 1 };
        if dry_rounds >= 2 {
            break 'rounds;
        }
    }
    let refined = substitute(code, &tunable, &value_of(&best.0));
    let loss_after = eval(&refined);
    if loss_after < loss_before {
        Ok((refined, RefineInfo { tuned: tunable.len(), evals: used, loss_before, loss_after }))
    } else {
        Ok((code.to_string(), RefineInfo { tuned: tunable.len(), evals: used, loss_before, loss_after: loss_before }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_knobs_and_skips_structure() {
        let code = r#"
# a comment with 3.5 in it
w = 2.0
leg = box(size=[0.1, 0.7, 0.1], center=[0, 0.35, 0])      # 0 is neutral
legs = array(leg, count=4, offset=[1.4, 0, 0])
post = tube(radius=0.05, height=1.2, cols=8, caps=1)
for i in range(6):
    p = transform(post, t=[i * 0.5, 0, 0], r=[0, -45, 0])
s = sphere(radius=0.3, rows=8, cols=16)
w = wrangle(s, code="@Cd = vec3(0.1, 0.2, 0.3);", channels={"amp": 1.5})
h = W / 2 + 3 * D + 0.5 * W
"#;
        let l = literals(code);
        let vals: Vec<f64> = l.iter().map(|x| x.value).collect();
        // w=2.0, size 0.1 0.7 0.1, center 0.35, count 4 (knob), offset 1.4, radius 0.05, height 1.2, t: 0.5 (i * 0.5), r: -45, radius 0.3, amp 1.5
        assert_eq!(vals, vec![2.0, 0.1, 0.7, 0.1, 0.35, 4.0, 1.4, 0.05, 1.2, 0.5, -45.0, 0.3, 1.5, 0.5]);
        assert_eq!(l.iter().filter(|x| x.is_count).count(), 1);
        assert_eq!(count_source_params(code), 14, "W / 2 and 3 * D are relations, 0.5 * W is a knob");
    }

    #[test]
    fn substitution_keeps_the_text_around_the_numbers() {
        let code = "a = box(size=[1.0, 2.0, 3.0])\nb = transform(a, t=[-0.5, 0, 0])\nout(b)\n";
        let l = literals(code);
        let refs: Vec<&Literal> = l.iter().collect();
        let out = substitute(code, &refs, &[1.5, 2.0, 3.25, -0.75]);
        assert_eq!(out, "a = box(size=[1.5, 2, 3.25])\nb = transform(a, t=[-0.75, 0, 0])\nout(b)\n");
    }

    #[test]
    fn refine_recovers_a_hidden_size_without_touching_structure() {
        // Reference: a 2 x 1 x 1 box. Start: 1.4 x 0.7 x 1.3, inside the +-30% search window.
        let target = "out(box(size=[2.0, 1.0, 1.0]))\n";
        let mut g = Graph::new();
        let rep = pt_hcs::run(target, &mut g, true).unwrap();
        let reference = (*g.cook(&rep.outputs[0]).unwrap()).clone();
        let code = "legs = [i for i in range(1)]\nout(box(size=[1.7, 0.85, 1.2]))\n";
        let (fixed, info) = refine_source(code, &reference, 400, 5, 60.0, false).unwrap();
        assert!(info.loss_after < info.loss_before * 0.2, "loss {} -> {}", info.loss_before, info.loss_after);
        assert!(fixed.starts_with("legs = [i for i in range(1)]"), "{fixed}");
        let m = Reference::new(&reference).unwrap().score(&reference_from(&fixed), Align::Reference).unwrap();
        assert!(m.f_score > 0.98, "F {}", m.f_score);
    }

    fn reference_from(code: &str) -> pt_core::Geometry {
        geometry_from_tris(&cook_source(code).unwrap())
    }
}
