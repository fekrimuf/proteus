//! pt-opt: inverse design for procedural graphs. Because every cook is a deterministic,
//! content-addressed pure function, thousands of evaluations are cheap (upstream nodes whose
//! inputs did not change come straight from the cache) and every run is reproducible.
//!
//! * `optimize`: CMA-ES with restarts and a pattern-search polish over declared parameters.
//! * `vary`: diverse variants (Latin hypercube + greedy maximin) with a contact sheet.
//! * objectives: Chamfer to a scan/mesh, size, silhouette IoU, image match, spec violations.

pub mod cma;
pub mod matching;
pub mod objective;
pub mod recipe;
pub mod solve;

pub use objective::{Nn, Objective};

use pt_graph::Graph;
use pt_render::{Image, Mode, Opts};
use serde_json::{json, Value};

/// A tunable scalar inside a node parameter: a number, a vector component (`index`) or a
/// key of an object parameter such as a wrangle's `channels` (`key`).
#[derive(Clone, Debug)]
pub struct ParamRef {
    pub node: String,
    pub param: String,
    pub index: Option<usize>,
    pub key: Option<String>,
    pub min: f64,
    pub max: f64,
}

impl ParamRef {
    pub fn label(&self) -> String {
        match (&self.index, &self.key) {
            (Some(i), _) => format!("{}.{}[{i}]", self.node, self.param),
            (_, Some(k)) => format!("{}.{}.{k}", self.node, self.param),
            _ => format!("{}.{}", self.node, self.param),
        }
    }

    /// Parse `{node, param, index?, key?, min, max}`.
    pub fn from_json(v: &Value) -> Result<ParamRef, String> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from).ok_or_else(|| format!("parameter spec needs string '{k}'"));
        let (min, max) = (v.get("min").and_then(Value::as_f64), v.get("max").and_then(Value::as_f64));
        let (Some(min), Some(max)) = (min, max) else { return Err("parameter spec needs numeric 'min' and 'max'".into()) };
        if min >= max {
            return Err(format!("parameter spec min {min} must be below max {max}"));
        }
        Ok(ParamRef { node: s("node")?, param: s("param")?, index: v.get("index").and_then(Value::as_u64).map(|i| i as usize), key: v.get("key").and_then(Value::as_str).map(String::from), min, max })
    }
}

pub fn read_value(g: &Graph, p: &ParamRef) -> Result<f64, String> {
    let params = pt_graph::resolved_params(g, &p.node).ok_or_else(|| format!("no node '{}'", p.node))??;
    let v = params.get(&p.param).ok_or_else(|| format!("node '{}' has no parameter '{}'", p.node, p.param))?;
    let x = match (v, p.index, &p.key) {
        (Value::Number(n), None, None) => n.as_f64(),
        (Value::Array(a), Some(i), _) => a.get(i).and_then(Value::as_f64),
        (Value::Object(o), _, Some(k)) => o.get(k).and_then(Value::as_f64),
        _ => None,
    };
    x.ok_or_else(|| format!("{} is not a number (use index for vectors, key for objects)", p.label()))
}

pub fn write_values(g: &mut Graph, space: &[ParamRef], xs: &[f64]) -> Result<(), String> {
    // Group writes per (node, param) so vectors/objects are updated once.
    let mut done: Vec<(String, String)> = Vec::new();
    for p in space {
        if done.iter().any(|d| d.0 == p.node && d.1 == p.param) {
            continue;
        }
        done.push((p.node.clone(), p.param.clone()));
        let params = pt_graph::resolved_params(g, &p.node).ok_or_else(|| format!("no node '{}'", p.node))??;
        let mut cur = params.get(&p.param).cloned().ok_or_else(|| format!("node '{}' has no parameter '{}'", p.node, p.param))?;
        for (q, x) in space.iter().zip(xs) {
            if q.node != p.node || q.param != p.param {
                continue;
            }
            match (&mut cur, q.index, &q.key) {
                (Value::Number(_), None, None) => cur = json!(x),
                (Value::Array(a), Some(i), _) if i < a.len() => a[i] = json!(x),
                (Value::Object(o), _, Some(k)) => {
                    o.insert(k.clone(), json!(x));
                }
                _ => return Err(format!("cannot write {}: parameter shape does not match", q.label())),
            }
        }
        g.set_param(&p.node, &p.param, cur)?;
    }
    Ok(())
}

fn denorm(space: &[ParamRef], u: &[f64]) -> Vec<f64> {
    space.iter().zip(u).map(|(p, u)| p.min + u.clamp(0.0, 1.0) * (p.max - p.min)).collect()
}

fn norm(space: &[ParamRef], x: &[f64]) -> Vec<f64> {
    space.iter().zip(x).map(|(p, x)| ((x - p.min) / (p.max - p.min)).clamp(0.0, 1.0)).collect()
}

pub struct OptOptions {
    /// Maximum objective evaluations (cooks) across all restarts and the polish.
    pub budget: usize,
    pub seed: i64,
    /// Extra random restarts after the first run.
    pub restarts: usize,
    /// Initial CMA step in the unit box.
    pub sigma0: f64,
}

impl Default for OptOptions {
    fn default() -> Self {
        OptOptions { budget: 400, seed: 1, restarts: 2, sigma0: 0.3 }
    }
}

pub struct OptReport {
    pub labels: Vec<String>,
    pub initial: Vec<f64>,
    pub best: Vec<f64>,
    pub initial_loss: f64,
    pub best_loss: f64,
    pub evals: usize,
    pub restarts_used: usize,
    /// Best-so-far loss after each evaluation batch.
    pub history: Vec<f64>,
    pub first_error: Option<String>,
}

impl OptReport {
    pub fn to_json(&self) -> Value {
        let m = |v: &[f64]| json!(self.labels.iter().cloned().zip(v.iter().copied()).collect::<std::collections::BTreeMap<_, _>>());
        let reduction = if self.initial_loss > 0.0 { 1.0 - self.best_loss / self.initial_loss } else { 0.0 };
        json!({"initial_loss": self.initial_loss, "best_loss": self.best_loss, "loss_reduction": reduction, "evals": self.evals,
               "restarts_used": self.restarts_used, "initial": m(&self.initial), "best": m(&self.best),
               "history": self.history.iter().step_by((self.history.len() / 40).max(1)).collect::<Vec<_>>(), "first_error": self.first_error})
    }
}

/// Minimize `obj` over `space` for the node `node`. On return the best parameters are applied.
pub fn optimize(g: &mut Graph, space: &[ParamRef], obj: &Objective, node: &str, opt: &OptOptions) -> Result<OptReport, String> {
    if space.is_empty() {
        return Err("optimize needs at least one parameter".into());
    }
    let labels: Vec<String> = space.iter().map(ParamRef::label).collect();
    let initial: Vec<f64> = space.iter().map(|p| read_value(g, p)).collect::<Result<_, _>>()?;
    let mut evals = 0usize;
    let mut first_error: Option<String> = None;
    let mut eval = |g: &mut Graph, u: &[f64], first_error: &mut Option<String>| -> f64 {
        let x = denorm(space, u);
        let r = write_values(g, space, &x).and_then(|_| obj.eval(g, node));
        match r {
            Ok(v) if v.is_finite() => v,
            Ok(_) => 1.0e6,
            Err(e) => {
                first_error.get_or_insert(e);
                1.0e6
            }
        }
    };
    let u0 = norm(space, &initial);
    let initial_loss = eval(g, &u0, &mut first_error);
    evals += 1;
    let mut best = (u0.clone(), initial_loss);
    let mut history = vec![initial_loss];
    let polish_budget = opt.budget / 10;
    let search_budget = opt.budget.saturating_sub(polish_budget + 1);
    let runs = opt.restarts + 1;
    let per_run = (search_budget / runs).max(space.len() * 4);
    let mut restarts_used = 0;
    for run in 0..runs {
        if evals >= 1 + search_budget {
            break;
        }
        let start: Vec<f64> = if run == 0 {
            u0.clone()
        } else {
            restarts_used += 1;
            (0..space.len()).map(|i| pt_core::rng::rand4(opt.seed, 11 + run as u32, i as u64)[0] as f64).collect()
        };
        let mut cma = cma::Cma::new(&start, opt.sigma0, opt.seed.wrapping_add(run as i64 * 7919));
        let run_end = (evals + per_run).min(1 + search_budget);
        let mut stagnant = 0;
        while evals < run_end {
            let xs = cma.ask();
            let mut fs = Vec::with_capacity(xs.len());
            let mut clamped = Vec::with_capacity(xs.len());
            for x in &xs {
                let c: Vec<f64> = x.iter().map(|v| v.clamp(0.0, 1.0)).collect();
                // Penalize leaving the box slightly so CMA stays inside without a hard wall.
                let pen: f64 = x.iter().zip(&c).map(|(a, b)| (a - b) * (a - b)).sum();
                let f = eval(g, &c, &mut first_error) + 1e-3 * pen;
                evals += 1;
                if f < best.1 {
                    best = (c.clone(), f);
                    stagnant = 0;
                }
                fs.push(f);
                clamped.push(c);
                if evals >= run_end {
                    break;
                }
            }
            history.push(best.1);
            if fs.len() == xs.len() {
                cma.tell(&xs, &fs);
            }
            stagnant += 1;
            if cma.sigma < 1e-4 || stagnant > 40 {
                break;
            }
        }
    }
    // Polish: coordinate pattern search with shrinking steps.
    let mut step = 0.04;
    let end = (evals + polish_budget).min(opt.budget);
    'polish: while step > 0.001 {
        let mut improved = false;
        for i in 0..space.len() {
            for dir in [1.0, -1.0] {
                if evals >= end {
                    break 'polish;
                }
                let mut c = best.0.clone();
                c[i] = (c[i] + dir * step).clamp(0.0, 1.0);
                if c[i] == best.0[i] {
                    continue;
                }
                let f = eval(g, &c, &mut first_error);
                evals += 1;
                if f < best.1 {
                    best = (c, f);
                    improved = true;
                }
            }
        }
        history.push(best.1);
        if !improved {
            step *= 0.5;
        }
    }
    let best_x = denorm(space, &best.0);
    write_values(g, space, &best_x)?;
    let confirm = obj.eval(g, node)?;
    Ok(OptReport { labels, initial, best: best_x, initial_loss, best_loss: confirm, evals, restarts_used, history, first_error })
}

// ---- variation engine ----

/// `count` well-spread points in the unit box: Latin-hypercube pool, greedy maximin selection.
pub fn spread_samples(dims: usize, count: usize, seed: i64) -> Vec<Vec<f64>> {
    let pool_n = (count * 8).max(count + 1);
    let mut pool: Vec<Vec<f64>> = vec![vec![0.0; dims]; pool_n];
    for d in 0..dims {
        // Per-dimension stratified permutation (deterministic shuffle by random keys).
        let mut order: Vec<usize> = (0..pool_n).collect();
        order.sort_by(|&a, &b| {
            let (ka, kb) = (pt_core::rng::rand4(seed, 21 + d as u32, a as u64)[0], pt_core::rng::rand4(seed, 21 + d as u32, b as u64)[0]);
            ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
        });
        for (rank, &i) in order.iter().enumerate() {
            let jitter = pt_core::rng::rand4(seed, 90 + d as u32, i as u64)[1] as f64;
            pool[i][d] = (rank as f64 + jitter) / pool_n as f64;
        }
    }
    let dist = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>();
    let mut chosen = vec![0usize];
    let mut mind: Vec<f64> = pool.iter().map(|p| dist(p, &pool[0])).collect();
    while chosen.len() < count.min(pool_n) {
        let (best, _) = mind.iter().enumerate().fold((0, -1.0), |acc, (i, &d)| if d > acc.1 { (i, d) } else { acc });
        chosen.push(best);
        for (i, m) in mind.iter_mut().enumerate() {
            *m = m.min(dist(&pool[i], &pool[best]));
        }
    }
    chosen.into_iter().map(|i| pool[i].clone()).collect()
}

pub struct Variants {
    pub values: Vec<Vec<f64>>,
    pub sheet: Image,
    pub losses: Vec<Option<f64>>,
}

/// Render `count` diverse variants of `node` into a labeled contact sheet. The graph is
/// restored to its original parameters afterwards.
pub fn vary(g: &mut Graph, space: &[ParamRef], node: &str, count: usize, seed: i64, thumb: (usize, usize), mode: Mode, score: Option<&Objective>) -> Result<Variants, String> {
    if space.is_empty() || count == 0 {
        return Err("vary needs at least one parameter and count >= 1".into());
    }
    for p in space {
        read_value(g, p)?; // validate the space up front
    }
    let snapshot = g.snapshot();
    let samples = spread_samples(space.len(), count.min(64), seed);
    let cols = (samples.len() as f64).sqrt().ceil() as usize;
    let rows = samples.len().div_ceil(cols);
    let mut sheet = Image { w: cols * thumb.0, h: rows * thumb.1, rgb: vec![30; cols * thumb.0 * rows * thumb.1 * 3] };
    let (mut values, mut losses) = (Vec::new(), Vec::new());
    let result = (|| -> Result<(), String> {
        for (n, u) in samples.iter().enumerate() {
            let x = denorm(space, u);
            write_values(g, space, &x)?;
            let geo = g.cook(node)?;
            let (img, _) = pt_render::render(&geo, &Opts { width: thumb.0, height: thumb.1, mode: mode.clone(), ..Default::default() });
            let (ox, oy) = ((n % cols) * thumb.0, (n / cols) * thumb.1);
            for y in 0..thumb.1 {
                let dst = ((oy + y) * sheet.w + ox) * 3;
                sheet.rgb[dst..dst + thumb.0 * 3].copy_from_slice(&img.rgb[y * thumb.0 * 3..(y + 1) * thumb.0 * 3]);
            }
            losses.push(match score {
                Some(o) => Some(o.eval(g, node)?),
                None => None,
            });
            values.push(x);
        }
        Ok(())
    })();
    g.restore(&snapshot)?;
    result?;
    Ok(Variants { values, sheet, losses })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scene() -> Graph {
        let mut g = Graph::new();
        g.create("box", Some("b")).unwrap();
        g.create("transform", Some("t")).unwrap();
        g.connect("b", "t", 0).unwrap();
        g
    }

    fn space() -> Vec<ParamRef> {
        vec![
            ParamRef { node: "t".into(), param: "s".into(), index: Some(0), key: None, min: 0.2, max: 4.0 },
            ParamRef { node: "t".into(), param: "s".into(), index: Some(1), key: None, min: 0.2, max: 4.0 },
            ParamRef { node: "t".into(), param: "uniform_scale".into(), index: None, key: None, min: 0.5, max: 2.0 },
        ]
    }

    #[test]
    fn recovers_a_hidden_size_and_is_reproducible() {
        let mut target = scene();
        target.set_param("t", "s", json!([2.2, 1.4, 1.0])).unwrap();
        target.set_param("t", "uniform_scale", json!(1.3)).unwrap();
        let tgeo = target.cook("t").unwrap();
        let (lo, hi) = tgeo.bounds().unwrap();
        let obj = Objective::Size { target: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]] };
        let opt = OptOptions { budget: 1200, seed: 3, restarts: 1, sigma0: 0.3 };
        let mut g = scene();
        let r = optimize(&mut g, &space(), &obj, "t", &opt).unwrap();
        assert!(r.best_loss < 1e-3, "loss {} -> {}", r.initial_loss, r.best_loss);
        assert!(r.evals <= 1200);
        let g2 = &mut scene();
        let r2 = optimize(g2, &space(), &obj, "t", &opt).unwrap();
        assert_eq!(r.best, r2.best, "same seed, same optimum");
    }

    #[test]
    fn chamfer_fits_a_point_cloud_scan() {
        let mut target = scene();
        target.set_param("t", "s", json!([1.8, 0.9, 1.2])).unwrap();
        target.set_param("t", "uniform_scale", json!(1.2)).unwrap();
        let obj = Objective::chamfer(&target.cook("t").unwrap(), 800).unwrap();
        let mut g = scene();
        let r = optimize(&mut g, &space(), &obj, "t", &OptOptions { budget: 400, seed: 5, restarts: 1, sigma0: 0.3 }).unwrap();
        assert!(r.best_loss < 0.3 * r.initial_loss, "chamfer {} -> {} (floor is surface sampling noise)", r.initial_loss, r.best_loss);
    }

    #[test]
    fn silhouette_objective_prefers_the_right_shape() {
        let mut target = scene();
        target.set_param("t", "s", json!([2.0, 1.0, 1.0])).unwrap();
        let obj = Objective::silhouette(&target.cook("t").unwrap(), 48).unwrap();
        let mut good = scene();
        good.set_param("t", "s", json!([2.0, 1.0, 1.0])).unwrap();
        let mut bad = scene();
        bad.set_param("t", "s", json!([0.5, 2.0, 1.0])).unwrap();
        let (lg, lb) = (obj.eval(&mut good, "t").unwrap(), obj.eval(&mut bad, "t").unwrap());
        assert!(lg < 0.02 && lb > 0.3, "good {lg} bad {lb}");
    }

    #[test]
    fn spec_objective_pushes_values_into_range() {
        let mut g = scene();
        let specs = vec![pt_hcs::SpecDef { node: "t".into(), kind: "height".into(), min: Some(2.9), max: Some(3.1), name: None, line: 1 }];
        let r = optimize(&mut g, &space(), &Objective::Specs(specs.clone()), "t", &OptOptions { budget: 200, seed: 1, restarts: 0, sigma0: 0.3 }).unwrap();
        assert!(r.best_loss == 0.0, "specs satisfied: loss {}", r.best_loss);
        assert!(pt_hcs::evaluate_specs(&mut g, &specs)[0].ok);
    }

    #[test]
    fn variants_are_diverse_and_restore_the_graph() {
        let pts = spread_samples(3, 12, 4);
        assert_eq!(pts.len(), 12);
        let min_d = (0..pts.len()).flat_map(|i| (i + 1..pts.len()).map(move |j| (i, j))).map(|(i, j)| pts[i].iter().zip(&pts[j]).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt()).fold(f64::INFINITY, f64::min);
        assert!(min_d > 0.15, "maximin spread {min_d}");
        let mut g = scene();
        g.set_param("t", "s", json!([1.0, 1.0, 1.0])).unwrap();
        let before = g.snapshot();
        let v = vary(&mut g, &space(), "t", 6, 2, (64, 48), Mode::Shaded, None).unwrap();
        assert_eq!(v.values.len(), 6);
        assert_eq!(before, g.snapshot(), "parameters restored");
        assert!(v.sheet.w >= 64 * 2);
    }
}
