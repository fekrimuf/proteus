//! Recipe runner: executes every `recipes/*.hcs`, checks specs and the static flow check,
//! records deterministic content hashes (golden), and renders a preview image per recipe.

use pt_graph::{hex, Graph};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct RecipeResult {
    pub name: String,
    pub hash: String,
    pub specs_ok: usize,
    pub specs_total: usize,
    pub failures: Vec<String>,
    pub ms: f64,
    pub points: usize,
    pub instances: usize,
    pub png: Option<PathBuf>,
}

pub fn recipes_dir() -> PathBuf {
    // Works from the workspace root and from `cargo test` (crate dir).
    for c in ["recipes", "../../recipes"] {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../recipes")
}

pub fn list() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(recipes_dir()).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "hcs")).collect()).unwrap_or_default();
    v.sort();
    v
}

pub fn run_one(path: &Path, render_to: Option<&Path>) -> RecipeResult {
    let name = path.file_stem().unwrap().to_string_lossy().to_string();
    let mut r = RecipeResult { name, hash: String::new(), specs_ok: 0, specs_total: 0, failures: vec![], ms: 0.0, points: 0, instances: 0, png: None };
    let code = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            r.failures.push(format!("cannot read: {e}"));
            return r;
        }
    };
    let t = std::time::Instant::now();
    let mut g = Graph::new();
    let report = match pt_hcs::run(&code, &mut g, true) {
        Ok(rep) => rep,
        Err(ds) => {
            r.failures.extend(ds.iter().map(|d| format!("script: {d}")));
            return r;
        }
    };
    for d in g.check().diags {
        r.failures.push(format!("flow: {} ({}): {}", d.node, d.message, d.fix));
    }
    let Some(out_id) = report.outputs.first() else {
        r.failures.push("no out(...) in recipe".into());
        return r;
    };
    match g.cook_entry(out_id) {
        Ok(e) => {
            r.hash = hex(&e.hash);
            r.points = e.geo.n_points();
            r.instances = e.geo.n_instances();
            let results = pt_hcs::evaluate_specs(&mut g, &report.specs);
            r.specs_total = results.len();
            for s in &results {
                if s.ok {
                    r.specs_ok += 1;
                } else {
                    r.failures.push(format!("spec line {} ({}): {}", s.line, s.kind, s.message));
                }
            }
            for i in pt_core::validate::validate(&e.geo.realize()) {
                if i.severity == "error" {
                    r.failures.push(format!("validate: {}: {}", i.code, i.message));
                }
            }
            if let Some(dir) = render_to {
                let mode = if e.geo.point.get("Cd").is_some() { pt_render::Mode::Attr("Cd".into()) } else { pt_render::Mode::Shaded };
                let (img, _) = pt_render::render(&e.geo, &pt_render::Opts { width: 640, height: 480, mode, ..Default::default() });
                if let Ok(png) = pt_render::encode_png(&img) {
                    let p = dir.join(format!("{}.png", r.name));
                    if std::fs::write(&p, png).is_ok() {
                        r.png = Some(p);
                    }
                }
            }
        }
        Err(e) => r.failures.push(format!("cook failed: {e}")),
    }
    r.ms = t.elapsed().as_secs_f64() * 1000.0;
    r
}

/// Run all recipes; compare (or with `update`, write) `recipes/golden.json`.
pub fn run_all(update: bool, render_to: Option<&Path>) -> Result<Vec<RecipeResult>, String> {
    if let Some(d) = render_to {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let golden_path = recipes_dir().join("golden.json");
    let golden: Value = std::fs::read_to_string(&golden_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
    let mut results: Vec<RecipeResult> = list().iter().map(|p| run_one(p, render_to)).collect();
    if update {
        let m: serde_json::Map<String, Value> = results.iter().filter(|r| r.failures.is_empty()).map(|r| (r.name.clone(), json!(r.hash))).collect();
        std::fs::write(&golden_path, serde_json::to_string_pretty(&Value::Object(m)).unwrap()).map_err(|e| e.to_string())?;
    } else if golden.as_object().is_some_and(|m| !m.is_empty()) {
        for r in results.iter_mut() {
            match golden.get(&r.name).and_then(Value::as_str) {
                Some(h) if h != r.hash && !r.hash.is_empty() => r.failures.push(format!("golden hash mismatch: expected {}..., got {}... (re-run with --update if the change is intended)", &h[..16], &r.hash[..16])),
                None if !r.hash.is_empty() => r.failures.push("no golden hash recorded (run `pt recipes --update`)".into()),
                _ => {}
            }
        }
    }
    Ok(results)
}

/// Agents propose recipes into `recipes/proposed/`; a human accepts them with `pt recipes-accept`.
pub fn proposed_dir() -> PathBuf {
    recipes_dir().join("proposed")
}

fn safe_name(name: &str) -> Result<String, String> {
    let n = name.trim().trim_end_matches(".hcs");
    if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("recipe name must be [A-Za-z0-9_-]+".into());
    }
    Ok(n.to_string())
}

/// Validate `code` as a recipe (script, flow check, specs, validation) and, if it passes, store it as a proposal.
pub fn propose(name: &str, code: &str) -> Result<RecipeResult, String> {
    let name = safe_name(name)?;
    let dir = proposed_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.hcs"));
    std::fs::write(&path, code).map_err(|e| e.to_string())?;
    let r = run_one(&path, None);
    if r.failures.is_empty() && r.specs_total == 0 {
        let _ = std::fs::remove_file(&path);
        return Err("a recipe needs at least one spec(...) so it can be verified".into());
    }
    if !r.failures.is_empty() {
        let _ = std::fs::remove_file(&path);
    }
    Ok(r)
}

/// Move a proposal into `recipes/` with the next number prefix and refresh the golden hashes.
pub fn accept(name: &str) -> Result<PathBuf, String> {
    let name = safe_name(name)?;
    let src = proposed_dir().join(format!("{name}.hcs"));
    let r = run_one(&src, None);
    if !r.failures.is_empty() {
        return Err(format!("proposal does not pass: {}", r.failures.join("; ")));
    }
    let next = list().iter().filter_map(|p| p.file_name()?.to_string_lossy().split('_').next()?.parse::<u32>().ok()).max().unwrap_or(0) + 1;
    let dst = recipes_dir().join(format!("{next:02}_{name}.hcs"));
    std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
    run_all(true, None)?;
    Ok(dst)
}

pub fn list_proposed() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(proposed_dir()).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "hcs")).collect()).unwrap_or_default();
    v.sort();
    v
}
