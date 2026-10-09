//! ProteusGym: a Gymnasium-style environment for procedural-modeling agents.
//!
//! * observation: task brief + verifiable constraints + feedback from the last step
//! * action: a complete hcs program (the whole scene; each step replaces the last)
//! * reward in [0, 1]: 0.05 builds, 0.05 attribute-flow clean, 0.05 output cooks and validates,
//!   0.85 fraction of the task's specs satisfied. Done at reward 1.0 or max_steps.
//!
//! Driven over stdin/stdout JSON lines (`pt gym`) so any language/agent framework can use it:
//! `{"cmd":"tasks"}`, `{"cmd":"reset","task":"t1_box_sized"}`, `{"cmd":"step","code":"..."}`,
//! `{"cmd":"render"}`, `{"cmd":"trajectory"}`.

use crate::recipes;
use pt_graph::Graph;
use pt_hcs::SpecDef;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

#[derive(Clone)]
pub struct Task {
    pub id: String,
    pub tier: &'static str,
    pub brief: String,
    pub specs: Vec<SpecDef>,
    pub max_steps: usize,
    /// Reference solution (used only for oracle baselines, never shown to the agent).
    pub reference: String,
}

fn spec(kind: &str, min: Option<f64>, max: Option<f64>) -> SpecDef {
    SpecDef { node: String::new(), kind: kind.into(), min, max, name: None, line: 0 }
}

fn describe_spec(s: &SpecDef) -> String {
    let range = match (s.min, s.max) {
        (Some(a), Some(b)) => format!("between {a} and {b}"),
        (Some(a), None) => format!("at least {a}"),
        (None, Some(b)) => format!("at most {b}"),
        (None, None) => String::new(),
    };
    match s.kind.as_str() {
        "height" => format!("height (m) {range}"),
        "width" => format!("width, x extent (m) {range}"),
        "depth" => format!("depth, z extent (m) {range}"),
        "points" => format!("point count {range}"),
        "prims" => format!("primitive count {range}"),
        "instances" => format!("instance count {range}"),
        "grounded" => "rests on the ground (lowest point y within tolerance of 0)".into(),
        "watertight" => "watertight (closed, manifold)".into(),
        "finite" => "contains no NaN/inf".into(),
        "attribute" => format!("has the point/primitive/instance attribute '{}'", s.name.clone().unwrap_or_default()),
        k => k.into(),
    }
}

fn tier_of(name: &str) -> &'static str {
    match name.get(..2).and_then(|n| n.parse::<u32>().ok()) {
        Some(1..=2) => "T1",
        Some(3..=6) | Some(8..=10) | Some(18) => "T2",
        Some(_) => "T3",
        None => "T1",
    }
}

pub fn tasks() -> Vec<Task> {
    let t1 = |id: &str, brief: &str, specs: Vec<SpecDef>, reference: &str| Task { id: id.into(), tier: "T1", brief: brief.into(), specs, max_steps: 6, reference: reference.into() };
    let mut v = vec![
        t1(
            "t1_box_sized",
            "Make a box 2 m wide (x), 0.5 m tall (y) and 1 m deep (z) that rests on the ground.",
            vec![spec("width", Some(1.95), Some(2.05)), spec("height", Some(0.45), Some(0.55)), spec("depth", Some(0.95), Some(1.05)), spec("grounded", None, None), spec("watertight", None, None)],
            "b = center(box(size=[2, 0.5, 1]), mode=\"ground\")\nout(b)\n",
        ),
        t1(
            "t1_post_row",
            "Build a row of 6 posts, each 1.2 m tall, spaced 0.8 m apart along x, standing on the ground. Use instancing, not 6 separate nodes.",
            vec![spec("instances", Some(6.0), Some(6.0)), spec("height", Some(1.1), Some(1.3)), spec("width", Some(4.0), Some(4.3)), spec("grounded", None, None)],
            "post = tube(radius=0.05, height=1.2, cols=8, center=[0, 0.6, 0])\nposts = array(post, count=6, offset=[0.8, 0, 0])\nout(posts)\n",
        ),
        t1(
            "t1_rock_field",
            "A flat 20 m x 20 m ground with exactly 150 rocks (small spheres) scattered on its surface, rocks as instances.",
            vec![spec("width", Some(19.5), Some(21.0)), spec("instances", Some(150.0), Some(150.0)), spec("finite", None, None)],
            "g = grid(size=[20, 20], rows=20, cols=20)\ns = scatter(g, count=150, seed=3)\nrock = sphere(radius=0.3)\nr = copy_to_points(rock, s)\nout(merge(g, r))\n",
        ),
        t1(
            "t1_ball_on_ground",
            "A sphere of radius 0.75 m resting on the ground, closed mesh.",
            vec![spec("height", Some(1.45), Some(1.55)), spec("grounded", None, None), spec("watertight", None, None)],
            "s = center(sphere(radius=0.75, rows=24, cols=48), mode=\"ground\")\nout(s)\n",
        ),
        t1(
            "t1_pillar_ring",
            "Eight pillars (2.5 m tall) arranged in a circle of radius 3 m around the origin, standing on the ground.",
            vec![spec("instances", Some(8.0), Some(8.0)), spec("height", Some(2.4), Some(2.6)), spec("width", Some(6.0), Some(6.8)), spec("grounded", None, None)],
            "p = tube(radius=0.25, height=2.5, cols=12, center=[0, 1.25, 0])\nring = radial_array(p, count=8, radius=3.0)\nout(ring)\n",
        ),
    ];
    for path in recipes::list() {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let Ok(code) = std::fs::read_to_string(&path) else { continue };
        let mut g = Graph::new();
        let Ok(rep) = pt_hcs::run(&code, &mut g, true) else { continue };
        let first = code.lines().next().unwrap_or("").trim_start_matches('#').trim().to_string();
        let specs: Vec<SpecDef> = rep.specs.clone();
        let tier = tier_of(&name);
        let list = specs.iter().map(|s| format!("- {}", describe_spec(s))).collect::<Vec<_>>().join("\n");
        v.push(Task {
            id: name.to_lowercase(),
            tier: if tier == "T1" { "T2" } else { tier },
            brief: format!("{first}\nBuild this procedurally (hcs). Units are meters, +Y up. Your result must satisfy:\n{list}"),
            specs,
            max_steps: 8,
            reference: code,
        });
    }
    v
}

pub struct Env {
    pub task: Task,
    graph: Graph,
    step: usize,
    pub history: Vec<Value>,
    last_code: String,
}

pub struct StepOut {
    pub reward: f64,
    pub done: bool,
    pub info: Value,
}

impl Env {
    pub fn new(task: Task) -> Env {
        Env { task, graph: Graph::new(), step: 0, history: vec![], last_code: String::new() }
    }

    pub fn observation(&self, feedback: Value) -> Value {
        json!({
            "task": self.task.id, "tier": self.task.tier, "brief": self.task.brief,
            "constraints": self.task.specs.iter().map(describe_spec).collect::<Vec<_>>(),
            "step": self.step, "max_steps": self.task.max_steps, "feedback": feedback,
            "tools": "submit a COMPLETE hcs program with step(code); see docs/AGENT_GUIDE.md for the language",
        })
    }

    pub fn step(&mut self, code: &str) -> StepOut {
        self.step += 1;
        self.last_code = code.to_string();
        let mut reward = 0.0;
        let mut info = json!({"build": false});
        match pt_hcs::run(code, &mut self.graph, true) {
            Err(ds) => {
                info["errors"] = json!(ds.iter().map(|d| d.to_json()).collect::<Vec<_>>());
            }
            Ok(rep) => {
                reward += 0.05;
                info["build"] = json!(true);
                let flow = self.graph.check();
                if flow.diags.is_empty() {
                    reward += 0.05;
                } else {
                    info["flow"] = json!(flow.diags.iter().map(|d| d.to_json()).collect::<Vec<_>>());
                }
                match rep.outputs.first() {
                    None => info["output"] = json!("no out(...) call: mark your result with out(node)"),
                    Some(out) => match self.graph.cook(out) {
                        Err(e) => info["cook_error"] = json!(e),
                        Ok(geo) => {
                            let errors: Vec<_> = pt_core::validate::validate(&geo.realize()).into_iter().filter(|i| i.severity == "error").map(|i| format!("{}: {}", i.code, i.message)).collect();
                            if errors.is_empty() {
                                reward += 0.05;
                            } else {
                                info["validate"] = json!(errors);
                            }
                            let specs: Vec<SpecDef> = self.task.specs.iter().cloned().map(|mut s| {
                                s.node = out.clone();
                                s
                            }).collect();
                            let results = pt_hcs::evaluate_specs(&mut self.graph, &specs);
                            let ok = results.iter().filter(|r| r.ok).count();
                            reward += 0.85 * ok as f64 / results.len().max(1) as f64;
                            info["specs"] = json!(results.iter().map(|r| json!({"spec": r.kind, "ok": r.ok, "message": r.message})).collect::<Vec<_>>());
                            info["specs_passed"] = json!(format!("{ok}/{}", results.len()));
                        }
                    },
                }
            }
        }
        let reward = (reward * 1000.0).round() / 1000.0;
        let done = reward >= 0.999 || self.step >= self.task.max_steps;
        self.history.push(json!({"step": self.step, "reward": reward, "code": code, "info": info}));
        StepOut { reward, done, info }
    }

    pub fn render(&mut self, w: usize, h: usize) -> Result<Vec<u8>, String> {
        let rep = pt_hcs::run(&self.last_code, &mut self.graph, true).map_err(|d| d[0].to_string())?;
        let geo = self.graph.cook(rep.outputs.first().ok_or("no output")?)?;
        let (img, _) = pt_render::render(&geo, &pt_render::Opts { width: w, height: h, ..Default::default() });
        pt_render::encode_png(&img)
    }
}

/// JSON-lines server for external agent harnesses.
pub fn serve() {
    let all = tasks();
    let mut env: Option<Env> = None;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let line = line.trim_start_matches('\u{feff}');
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            let _ = writeln!(out, "{}", json!({"error": "invalid JSON"}));
            continue;
        };
        let resp = match req["cmd"].as_str().unwrap_or("") {
            "tasks" => json!({"tasks": all.iter().map(|t| json!({"id": t.id, "tier": t.tier, "max_steps": t.max_steps, "specs": t.specs.len()})).collect::<Vec<_>>()}),
            "reset" => match all.iter().find(|t| Some(t.id.as_str()) == req["task"].as_str()) {
                Some(t) => {
                    let e = Env::new(t.clone());
                    let obs = e.observation(json!(null));
                    env = Some(e);
                    json!({"observation": obs})
                }
                None => json!({"error": format!("unknown task; have {:?}", all.iter().map(|t| &t.id).collect::<Vec<_>>())}),
            },
            "step" => match (&mut env, req["code"].as_str()) {
                (Some(e), Some(code)) => {
                    let r = e.step(code);
                    json!({"observation": e.observation(r.info.clone()), "reward": r.reward, "done": r.done, "info": r.info})
                }
                (None, _) => json!({"error": "call reset first"}),
                (_, None) => json!({"error": "step needs 'code'"}),
            },
            "render" => match &mut env {
                Some(e) => match e.render(req["width"].as_u64().unwrap_or(480) as usize, req["height"].as_u64().unwrap_or(360) as usize) {
                    Ok(png) => json!({"image_png_base64": pt_core::io::base64_encode(&png)}),
                    Err(err) => json!({"error": err}),
                },
                None => json!({"error": "call reset first"}),
            },
            "trajectory" => match &env {
                Some(e) => json!({"task": e.task.id, "steps": e.history}),
                None => json!({"error": "call reset first"}),
            },
            c => json!({"error": format!("unknown cmd '{c}' (tasks | reset | step | render | trajectory)")}),
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}

// ---- baselines ----

/// Multiply every numeric literal in `code` by a deterministic factor in [1-amp, 1+amp].
fn perturb(code: &str, seed: u64, amp: f64) -> String {
    let mut out = String::new();
    let b: Vec<char> = code.chars().collect();
    let (mut i, mut k) = (0, 0u64);
    while i < b.len() {
        if b[i] == '#' {
            while i < b.len() && b[i] != '\n' {
                out.push(b[i]);
                i += 1;
            }
        } else if b[i] == '"' {
            out.push(b[i]);
            i += 1;
            while i < b.len() && b[i] != '"' {
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push(b[i]);
                i += 1;
            }
        } else if b[i].is_ascii_digit() && (i == 0 || !(b[i - 1].is_alphanumeric() || b[i - 1] == '_')) {
            let s = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                i += 1;
            }
            let text: String = b[s..i].iter().collect();
            let v: f64 = text.parse().unwrap_or(0.0);
            let r = pt_core::rng::rand4(seed as i64, 77, k)[0] as f64;
            k += 1;
            let scaled = v * (1.0 + (r * 2.0 - 1.0) * amp);
            // Keep integers integral (counts, resolutions).
            out.push_str(&if text.contains('.') { format!("{scaled:.3}") } else { format!("{}", scaled.round().max(if v >= 1.0 { 1.0 } else { 0.0 })) });
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

pub fn baseline() -> Result<(), String> {
    let all = tasks();
    println!("{:<26} {:>4} | {:>7} {:>7} {:>9}", "task", "tier", "oracle", "null", "perturbed");
    let mut rows = Vec::new();
    for t in &all {
        let oracle = Env::new(t.clone()).step(&t.reference).reward;
        let nullr = Env::new(t.clone()).step("out(box())\n").reward;
        // Best of 3 perturbation attempts: models an agent that has the right structure but guesses numbers.
        let mut perturbed = 0.0f64;
        for attempt in 0..3 {
            perturbed = perturbed.max(Env::new(t.clone()).step(&perturb(&t.reference, 100 + attempt, 0.35)).reward);
        }
        println!("{:<26} {:>4} | {:>7.3} {:>7.3} {:>9.3}", t.id, t.tier, oracle, nullr, perturbed);
        rows.push(json!({"task": t.id, "tier": t.tier, "oracle": oracle, "null_agent": nullr, "perturbed_best_of_3": perturbed}));
    }
    let mean = |k: &str| rows.iter().map(|r| r[k].as_f64().unwrap_or(0.0)).sum::<f64>() / rows.len().max(1) as f64;
    println!("{:<26} {:>4} | {:>7.3} {:>7.3} {:>9.3}", "MEAN", "", mean("oracle"), mean("null_agent"), mean("perturbed_best_of_3"));
    std::fs::create_dir_all("gym").ok();
    std::fs::write("gym/baseline.json", serde_json::to_string_pretty(&json!({"tasks": rows, "mean": {"oracle": mean("oracle"), "null_agent": mean("null_agent"), "perturbed": mean("perturbed_best_of_3")}})).unwrap()).map_err(|e| e.to_string())?;
    println!("recorded gym/baseline.json");
    if mean("oracle") < 0.999 {
        return Err("oracle must score 1.0 on every task (a task or spec is broken)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_solves_every_task_and_null_agent_does_not() {
        let all = tasks();
        assert!(all.len() >= 18, "{} tasks", all.len());
        for t in &all {
            let r = Env::new(t.clone()).step(&t.reference);
            assert!(r.reward >= 0.999, "{}: oracle reward {} info {}", t.id, r.reward, r.info);
            let n = Env::new(t.clone()).step("out(box())\n");
            assert!(n.reward < 0.8, "{}: null agent scores {}", t.id, n.reward);
        }
    }

    #[test]
    fn rewards_are_shaped_and_feedback_is_actionable() {
        let t = tasks().into_iter().find(|t| t.id == "t1_box_sized").unwrap();
        let mut env = Env::new(t);
        let bad = env.step("b = bx(size=[2, 0.5, 1])\n");
        assert_eq!(bad.reward, 0.0);
        assert!(bad.info["errors"][0]["fix"].as_str().unwrap().contains("box"));
        let partial = env.step("out(box(size=[2, 0.5, 1]))\n");
        assert!(partial.reward > 0.4 && partial.reward < 0.99, "builds but is not grounded: {}", partial.reward);
        assert!(partial.info["specs"].as_array().unwrap().iter().any(|s| s["ok"] == false));
        let full = env.step("out(center(box(size=[2, 0.5, 1]), mode=\"ground\"))\n");
        assert!(full.reward >= 0.999 && full.done);
        assert_eq!(env.history.len(), 3);
    }

    #[test]
    fn perturbation_changes_numbers_but_not_structure() {
        let p = perturb("a = box(size=[2, 0.5, 1])  # keep 3 here\nb = tube(cols=8)\n", 1, 0.3);
        assert!(p.contains("# keep 3 here") && p.contains("box(size=[") && p.contains("tube(cols="));
        assert_ne!(p, "a = box(size=[2, 0.5, 1])  # keep 3 here\nb = tube(cols=8)\n");
    }

    #[test]
    fn episodes_end_at_max_steps() {
        let t = tasks().into_iter().find(|t| t.id == "t1_ball_on_ground").unwrap();
        let mut env = Env::new(t.clone());
        let mut done = false;
        for _ in 0..t.max_steps {
            done = env.step("out(box())\n").done;
        }
        assert!(done);
    }
}
