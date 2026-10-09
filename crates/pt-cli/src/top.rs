//! TOP-style task graph runner. A task file describes tasks (run a recipe or inline hcs, path
//! trace a still) with dependencies and parameter *wedges* (one task expanded into many
//! variations). Independent tasks run in parallel; every task result is cached on disk by a
//! content hash of (task description, dependency results, recipe source), so re-running a farm
//! only executes what actually changed.
//!
//! ```json
//! {"tasks": [
//!   {"id": "rock", "type": "recipe", "recipe": "03_rock_generator", "export": "out/top/rock.ptgeo"},
//!   {"id": "rock_shot", "type": "shot", "from": "rock", "out": "out/top/rock.png", "spp": 16, "depends": ["rock"]},
//!   {"id": "freq", "type": "recipe", "recipe": "03_rock_generator", "export": "out/top/rock_{i}.obj",
//!    "wedge": {"node": "rough", "param": "channels", "key": "freq", "values": [1.0, 2.0, 3.0]}}
//! ]}
//! ```

use crate::recipes::recipes_dir;
use pt_graph::{hex, Graph};
use pt_opt::{write_values, ParamRef};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Ran,
    Cached,
    Failed(String),
    Skipped(String),
}

#[derive(Clone, Debug)]
pub struct TaskResult {
    pub id: String,
    pub status: Status,
    pub hash: String,
    pub outputs: Vec<PathBuf>,
    pub ms: f64,
}

#[derive(Clone, Debug)]
struct Task {
    id: String,
    spec: Value,
    deps: Vec<String>,
}

fn expand(tasks: &[Value]) -> Result<Vec<Task>, String> {
    let mut out = Vec::new();
    for t in tasks {
        let id = t["id"].as_str().ok_or("every task needs an 'id'")?.to_string();
        let deps: Vec<String> = t["depends"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        match t.get("wedge") {
            Some(w) => {
                let values = w["values"].as_array().ok_or_else(|| format!("task '{id}': wedge needs 'values'"))?;
                for (i, v) in values.iter().enumerate() {
                    let mut spec = t.clone();
                    spec["wedge_value"] = v.clone();
                    if let Some(e) = spec["export"].as_str() {
                        spec["export"] = json!(e.replace("{i}", &i.to_string()));
                    }
                    if let Some(e) = spec["out"].as_str() {
                        spec["out"] = json!(e.replace("{i}", &i.to_string()));
                    }
                    out.push(Task { id: format!("{id}[{i}]"), spec, deps: deps.clone() });
                }
            }
            None => out.push(Task { id, spec: t.clone(), deps }),
        }
    }
    // A dependency on a wedged task means "all of its variations".
    let ids: Vec<String> = out.iter().map(|t| t.id.clone()).collect();
    for t in &mut out {
        let mut resolved = Vec::new();
        for d in &t.deps {
            let prefix = format!("{d}[");
            let matches: Vec<&String> = ids.iter().filter(|i| *i == d || i.starts_with(&prefix)).collect();
            if matches.is_empty() {
                return Err(format!("task '{}' depends on unknown task '{d}'", t.id));
            }
            resolved.extend(matches.into_iter().cloned());
        }
        t.deps = resolved;
    }
    Ok(out)
}

fn task_source(spec: &Value) -> Result<String, String> {
    if let Some(code) = spec["code"].as_str() {
        return Ok(code.to_string());
    }
    let name = spec["recipe"].as_str().ok_or("task needs 'recipe' or 'code'")?;
    let path = if Path::new(name).exists() { PathBuf::from(name) } else { recipes_dir().join(format!("{name}.hcs")) };
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

fn exec(task: &Task, dep_results: &HashMap<String, TaskResult>) -> Result<(String, Vec<PathBuf>), String> {
    let spec = &task.spec;
    match spec["type"].as_str().unwrap_or("recipe") {
        "recipe" | "hcs" => {
            let code = task_source(spec)?;
            let mut g = Graph::new();
            let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
            if let Some(w) = spec.get("wedge") {
                let r = ParamRef { node: w["node"].as_str().unwrap_or("").into(), param: w["param"].as_str().unwrap_or("").into(), index: w["index"].as_u64().map(|i| i as usize), key: w["key"].as_str().map(String::from), min: 0.0, max: 1.0 };
                let v = spec["wedge_value"].as_f64().ok_or("wedge values must be numbers")?;
                write_values(&mut g, &[r], &[v])?;
            }
            let out = rep.outputs.first().ok_or("no out(...) in the task's program")?;
            let e = g.cook_entry(out)?;
            let mut outputs = Vec::new();
            if let Some(path) = spec["export"].as_str() {
                if let Some(parent) = Path::new(path).parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let data: Vec<u8> = match Path::new(path).extension().and_then(|e| e.to_str()) {
                    Some("obj") => pt_core::io::to_obj(&e.geo).into_bytes(),
                    Some("gltf") => pt_core::io::to_gltf(&e.geo).into_bytes(),
                    Some("usda") | Some("usd") => pt_core::io::to_usda(&e.geo).into_bytes(),
                    Some("ptgeo") => pt_core::io::to_bytes(&e.geo),
                    _ => return Err(format!("unknown export extension in '{path}' (obj, gltf, usda, ptgeo)")),
                };
                std::fs::write(path, data).map_err(|e| e.to_string())?;
                outputs.push(PathBuf::from(path));
            }
            Ok((hex(&e.hash), outputs))
        }
        "shot" => {
            let geo = if let Some(from) = spec["from"].as_str() {
                let dep = dep_results.get(from).ok_or_else(|| format!("'from' task '{from}' is not a dependency"))?;
                let f = dep.outputs.iter().find(|p| p.extension().is_some_and(|e| e == "ptgeo")).ok_or_else(|| format!("task '{from}' did not export a .ptgeo file"))?;
                pt_core::io::from_bytes(&std::fs::read(f).map_err(|e| e.to_string())?)?
            } else {
                let code = task_source(spec)?;
                let mut g = Graph::new();
                let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
                (*g.cook(rep.outputs.first().ok_or("no out(...)")?)?).clone()
            };
            let path = spec["out"].as_str().ok_or("shot needs 'out'")?;
            if let Some(parent) = Path::new(path).parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let o = pt_render::pathtrace::PtOpts { width: spec["width"].as_u64().unwrap_or(480) as usize, height: spec["height"].as_u64().unwrap_or(360) as usize, spp: spec["spp"].as_u64().unwrap_or(16) as usize, azimuth_deg: spec["azimuth"].as_f64().unwrap_or(35.0) as f32, ..Default::default() };
            let (img, _) = pt_render::pathtrace::pathtrace(&geo, &o);
            std::fs::write(path, pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
            Ok((blake3::hash(&img.rgb).to_hex().to_string(), vec![PathBuf::from(path)]))
        }
        other => Err(format!("unknown task type '{other}' (recipe | hcs | shot)")),
    }
}

/// Run a task file's tasks. `cache_dir` holds one small JSON per finished task.
pub fn run(tasks: &Value, cache_dir: &Path) -> Result<Vec<TaskResult>, String> {
    let list = tasks["tasks"].as_array().ok_or("task file needs a 'tasks' array")?;
    let tasks = expand(list)?;
    std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let by_id: HashMap<String, &Task> = tasks.iter().map(|t| (t.id.clone(), t)).collect();
    // Topological levels (cycle = error).
    let mut level: BTreeMap<String, usize> = BTreeMap::new();
    fn depth(id: &str, by_id: &HashMap<String, &Task>, memo: &mut BTreeMap<String, usize>, stack: &mut Vec<String>) -> Result<usize, String> {
        if let Some(l) = memo.get(id) {
            return Ok(*l);
        }
        if stack.iter().any(|s| s == id) {
            return Err(format!("dependency cycle through '{id}'"));
        }
        stack.push(id.to_string());
        let mut l = 0;
        for d in &by_id[id].deps {
            l = l.max(depth(d, by_id, memo, stack)? + 1);
        }
        stack.pop();
        memo.insert(id.to_string(), l);
        Ok(l)
    }
    for t in &tasks {
        depth(&t.id, &by_id, &mut level, &mut vec![])?;
    }
    let max_level = level.values().copied().max().unwrap_or(0);
    let mut results: HashMap<String, TaskResult> = HashMap::new();
    for l in 0..=max_level {
        let batch: Vec<&Task> = tasks.iter().filter(|t| level[&t.id] == l).collect();
        let done: Vec<TaskResult> = batch
            .par_iter()
            .map(|t| {
                let started = std::time::Instant::now();
                let fail = |s: Status| TaskResult { id: t.id.clone(), status: s, hash: String::new(), outputs: vec![], ms: started.elapsed().as_secs_f64() * 1000.0 };
                if let Some(bad) = t.deps.iter().find(|d| matches!(results[*d].status, Status::Failed(_) | Status::Skipped(_))) {
                    return fail(Status::Skipped(format!("dependency '{bad}' did not succeed")));
                }
                let dep_results: HashMap<String, TaskResult> = t.deps.iter().map(|d| (d.clone(), results[d].clone())).collect();
                let src = match task_source(&t.spec) {
                    Ok(s) => s,
                    Err(_) => String::new(),
                };
                let mut h = blake3::Hasher::new();
                h.update(b"pttop/1");
                h.update(t.spec.to_string().as_bytes());
                h.update(src.as_bytes());
                for d in &t.deps {
                    h.update(results[d].hash.as_bytes());
                }
                let key = h.finalize().to_hex().to_string();
                let cache_file = cache_dir.join(format!("{key}.json"));
                if let Ok(txt) = std::fs::read_to_string(&cache_file) {
                    if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                        let outputs: Vec<PathBuf> = v["outputs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(PathBuf::from)).collect()).unwrap_or_default();
                        if outputs.iter().all(|p| p.exists()) {
                            return TaskResult { id: t.id.clone(), status: Status::Cached, hash: v["hash"].as_str().unwrap_or("").into(), outputs, ms: started.elapsed().as_secs_f64() * 1000.0 };
                        }
                    }
                }
                match exec(t, &dep_results) {
                    Ok((hash, outputs)) => {
                        let _ = std::fs::write(&cache_file, json!({"hash": hash, "outputs": outputs.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>()}).to_string());
                        TaskResult { id: t.id.clone(), status: Status::Ran, hash, outputs, ms: started.elapsed().as_secs_f64() * 1000.0 }
                    }
                    Err(e) => fail(Status::Failed(e)),
                }
            })
            .collect();
        for r in done {
            results.insert(r.id.clone(), r);
        }
    }
    Ok(tasks.iter().map(|t| results[&t.id].clone()).collect())
}

pub fn run_file(path: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?).map_err(|e| e.to_string())?;
    let t = std::time::Instant::now();
    let results = run(&v, Path::new("out/top_cache"))?;
    println!("{:<20} {:<10} {:>9}  {}", "task", "status", "ms", "hash / detail");
    let mut failed = 0;
    for r in &results {
        let (s, detail) = match &r.status {
            Status::Ran => ("ran", r.hash.chars().take(16).collect::<String>()),
            Status::Cached => ("cached", r.hash.chars().take(16).collect::<String>()),
            Status::Failed(e) => {
                failed += 1;
                ("FAILED", e.clone())
            }
            Status::Skipped(e) => {
                failed += 1;
                ("skipped", e.clone())
            }
        };
        println!("{:<20} {:<10} {:>9.1}  {}", r.id, s, r.ms, detail);
    }
    println!("{} tasks in {:.2} s, {} not successful", results.len(), t.elapsed().as_secs_f64(), failed);
    if failed > 0 { Err("some tasks failed".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pt_top_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn farm(dir: &Path) -> Value {
        let p = |f: &str| dir.join(f).to_string_lossy().replace('\\', "/");
        json!({"tasks": [
            {"id": "base", "type": "hcs", "code": "b = box(size=[1,2,3])\nout(b)\n", "export": p("base.ptgeo")},
            {"id": "shot", "type": "shot", "from": "base", "depends": ["base"], "out": p("base.png"), "spp": 2, "width": 64, "height": 48},
            {"id": "w", "type": "hcs", "code": "b = box(size=[1,1,1])\nt = transform(b, s=[1,1,1])\nout(t)\n", "export": p("w_{i}.obj"),
             "wedge": {"node": "t", "param": "s", "index": 0, "values": [1.0, 2.0, 3.0]}},
            {"id": "after", "type": "hcs", "code": "out(box())\n", "depends": ["w"]},
        ]})
    }

    #[test]
    fn runs_in_dependency_order_expands_wedges_and_caches() {
        let dir = tmp("a");
        let cache = dir.join("cache");
        let f = farm(&dir);
        let r1 = run(&f, &cache).unwrap();
        assert_eq!(r1.len(), 1 + 1 + 3 + 1, "{:?}", r1.iter().map(|r| &r.id).collect::<Vec<_>>());
        assert!(r1.iter().all(|r| r.status == Status::Ran), "{:?}", r1.iter().map(|r| (&r.id, &r.status)).collect::<Vec<_>>());
        let hashes: std::collections::BTreeSet<_> = r1.iter().filter(|r| r.id.starts_with("w[")).map(|r| r.hash.clone()).collect();
        assert_eq!(hashes.len(), 3, "wedge variations differ");
        assert!(dir.join("base.png").exists() && dir.join("w_2.obj").exists());
        let r2 = run(&f, &cache).unwrap();
        assert!(r2.iter().all(|r| r.status == Status::Cached), "second run is fully cached: {:?}", r2.iter().map(|r| (&r.id, &r.status)).collect::<Vec<_>>());
        assert_eq!(r1.iter().map(|r| &r.hash).collect::<Vec<_>>(), r2.iter().map(|r| &r.hash).collect::<Vec<_>>());
        // Changing one task re-runs it and its dependents only.
        let mut f2 = f.clone();
        f2["tasks"][0]["code"] = json!("b = box(size=[1,2,4])\nout(b)\n");
        let r3 = run(&f2, &cache).unwrap();
        let st = |id: &str| r3.iter().find(|r| r.id == id).unwrap().status.clone();
        assert_eq!(st("base"), Status::Ran);
        assert_eq!(st("shot"), Status::Ran);
        assert_eq!(st("w[1]"), Status::Cached);
        assert_eq!(st("after"), Status::Cached);
    }

    #[test]
    fn failures_skip_dependents_and_cycles_are_rejected() {
        let dir = tmp("b");
        let f = json!({"tasks": [
            {"id": "bad", "type": "hcs", "code": "x = nois(1)\n"},
            {"id": "child", "type": "hcs", "code": "out(box())\n", "depends": ["bad"]},
            {"id": "ok", "type": "hcs", "code": "out(box())\n"},
        ]});
        let r = run(&f, &dir.join("c")).unwrap();
        assert!(matches!(r[0].status, Status::Failed(_)));
        assert!(matches!(r[1].status, Status::Skipped(_)));
        assert_eq!(r[2].status, Status::Ran);
        let cyc = json!({"tasks": [
            {"id": "a", "type": "hcs", "code": "out(box())\n", "depends": ["b"]},
            {"id": "b", "type": "hcs", "code": "out(box())\n", "depends": ["a"]},
        ]});
        assert!(run(&cyc, &dir.join("d")).unwrap_err().contains("cycle"));
        assert!(run(&json!({"tasks": [{"id": "x", "type": "hcs", "code": "out(box())", "depends": ["nope"]}]}), &dir.join("e")).unwrap_err().contains("unknown task"));
    }
}
