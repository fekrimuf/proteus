//! `pt match`, `pt export` and `pt match-bench`: measure how well a recipe reproduces a reference model
//! (PRO_QUALITY_PLAN B4 steps 3, 5 and 6).

use crate::corpus;
use pt_core::Geometry;
use pt_graph::Graph;
use pt_opt::matching::*;
use pt_opt::solve::{solve, SolverOptions};
use pt_render::{Image, Mode, Opts};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const SOLVER_VERSION: &str = "primitive-v0a.5";

pub struct Loaded {
    /// glTF nodes (or one group for OBJ), as triangle lists in the file's own units.
    pub groups: Vec<(String, Vec<Tri>)>,
    pub geo: Geometry,
    pub warnings: Vec<String>,
}

/// Load a reference: `.glb`/`.gltf` (one group per node with a mesh) or `.obj`.
pub fn load_reference(path: &Path) -> Result<Loaded, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "glb" | "gltf" => {
            let scene = pt_core::gltf::load_file(path)?;
            if scene.parts.is_empty() {
                return Err("the file contains no triangle meshes".into());
            }
            let groups = scene.parts.iter().map(|p| (p.name.clone(), tris_of(&p.geo))).collect();
            Ok(Loaded { groups, geo: scene.merged(), warnings: scene.warnings })
        }
        "obj" => {
            let geo = pt_core::io::load_obj(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)?;
            Ok(Loaded { groups: vec![("model".into(), tris_of(&geo))], geo, warnings: vec![] })
        }
        o => Err(format!("unsupported reference format '.{o}' (use .glb, .gltf or .obj)")),
    }
}

/// Run an `.hcs` recipe and cook its first output.
pub fn cook_recipe(code: &str) -> Result<(Graph, String, std::sync::Arc<Geometry>), String> {
    let mut g = Graph::new();
    let rep = pt_hcs::run(code, &mut g, true).map_err(|d| d.first().map_or("recipe error".to_string(), |x| x.to_string()))?;
    let out = rep.outputs.first().cloned().ok_or("the recipe has no out(...)")?;
    let geo = g.cook(&out)?;
    Ok((g, out, geo))
}

/// A candidate is either an `.hcs` recipe or a geometry file.
pub fn load_candidate(path: &Path) -> Result<(Geometry, Option<usize>), String> {
    if path.extension().and_then(|e| e.to_str()) == Some("hcs") {
        let code = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (_g, _, geo) = cook_recipe(&code)?;
        Ok(((*geo).clone(), Some(pt_opt::recipe::count_source_params(&code))))
    } else {
        Ok((load_reference(path)?.geo, None))
    }
}

const PAIR_VIEWS: [(f32, f32); 2] = [(35.0, 25.0), (215.0, 20.0)];

/// Reference (left) and candidate (right) from two camera angles, one shared camera per row.
pub fn pair_image(reference: &Geometry, ours: &Geometry) -> Image {
    let (lo, hi) = reference.realize().bounds().unwrap_or(([-1.0; 3], [1.0; 3]));
    let pad = pt_core::math::sub(hi, lo);
    let bounds = ([0, 1, 2].map(|a| lo[a] - pad[a] * 0.08), [0, 1, 2].map(|a| hi[a] + pad[a] * 0.08));
    let mut cells = Vec::new();
    for (az, el) in PAIR_VIEWS {
        for g in [reference, ours] {
            let o = Opts { width: 384, height: 288, azimuth_deg: az, elevation_deg: el, fit_bounds: Some(bounds), mode: Mode::Shaded, ..Default::default() };
            cells.push(pt_render::render(g, &o).0);
        }
    }
    pt_render::grid(&cells, 2, 4)
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = !matches!(a.as_str(), "--resume" | "--no-pairs" | "--only-recipes" | "--brief" | "--tune-integers");
        } else {
            out.push(a.as_str());
        }
    }
    out
}

/// `pt match <reference> <candidate> [--report DIR] [--align bbox|reference]`
pub fn match_cmd(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let [refp, candp] = pos[..] else { return Err("usage: pt match <reference.glb|obj> <recipe.hcs|geometry> [--report DIR] [--align bbox|reference]".into()) };
    let align = if flag(args, "--align") == Some("reference") { Align::Reference } else { Align::Bbox };
    let reference = load_reference(Path::new(refp))?;
    let (cand, params) = load_candidate(Path::new(candp))?;
    let r = Reference::new(&reference.geo)?;
    let m = r.score(&cand, align)?;
    let v = validity(&cand, &reference.geo);
    let out = json!({"reference": refp, "candidate": candp, "align": format!("{align:?}"), "metrics": m.to_json(), "validity": v.to_json(),
                     "free_parameters": params, "tier": params.map(|p| tier(&m, v.ok, p))});
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    if let Some(dir) = flag(args, "--report") {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(Path::new(dir).join("match.json"), serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::write(Path::new(dir).join("match.png"), pt_render::encode_png(&pair_image(&reference.geo, &cand))?).map_err(|e| e.to_string())?;
        println!("wrote {dir}/match.json and {dir}/match.png (original left, ours right, same camera)");
    }
    Ok(())
}

/// `pt parts <reference>`: the model's overall size and its parts (glTF nodes, then connected pieces) with bounds.
pub fn parts_cmd(args: &[String]) -> Result<(), String> {
    let [path] = args else { return Err("usage: pt parts <reference.glb|gltf|obj>".into()) };
    let l = load_reference(Path::new(path))?;
    let all: Vec<Tri> = l.groups.iter().flat_map(|g| g.1.iter().copied()).collect();
    let (lo, hi) = bounds_of_tris(&all).ok_or("empty model")?;
    let d = diagonal(lo, hi);
    println!("{path}: {} triangles, {} node(s), watertight {}", all.len(), l.groups.len(), welded_watertight(&all, d * 1e-4));
    println!("bounds x [{:.4}, {:.4}]  y [{:.4}, {:.4}]  z [{:.4}, {:.4}]   size {:.4} x {:.4} x {:.4}   diagonal {d:.4}", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2], hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
    let total: f32 = all.iter().map(tri_area).sum();
    println!("{:<18} {:>5} {:>6}   {:<27} {:<27}", "part", "tris", "area%", "min x, y, z", "max x, y, z");
    for p in split_parts(&l.groups, 0.0) {
        println!("{:<18} {:>5} {:>5.1}%   {:>8.4} {:>8.4} {:>8.4}   {:>8.4} {:>8.4} {:>8.4}", p.name, p.tris.len(), 100.0 * p.area / total, p.lo[0], p.lo[1], p.lo[2], p.hi[0], p.hi[1], p.hi[2]);
    }
    Ok(())
}

/// `pt describe <reference>`: cross-section islands along Y, X and Z, a text view of what the model is made of.
/// Each slab lists the separate islands of surface found in it (bounding box and share of the slab's surface).
pub fn describe_cmd(args: &[String]) -> Result<(), String> {
    let brief = args.iter().any(|a| a == "--brief");
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [path] = pos[..] else { return Err("usage: pt describe <reference.glb|gltf|obj> [--brief]".into()) };
    let l = load_reference(Path::new(path))?;
    let all: Vec<Tri> = l.groups.iter().flat_map(|g| g.1.iter().copied()).collect();
    let (lo, hi) = bounds_of_tris(&all).ok_or("empty model")?;
    println!("{path}: size {:.3} x {:.3} x {:.3} (x, y, z), y is up, diagonal {:.3}", hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2], diagonal(lo, hi));
    let samples = sample_tris(&all, 60_000, 5);
    for (axis, name, slabs) in [(1usize, "Y (up)", 12usize), (0, "X", 8), (2, "Z", 8)] {
        if brief && axis != 1 {
            continue;
        }
        let (a, b) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        println!("-- slabs along {name}: [from, to]  islands as {}-range x {}-range (share of the slab's surface)", ["x", "y", "z"][a], ["x", "y", "z"][b]);
        let n = 48usize;
        let (ea, eb) = ((hi[a] - lo[a]).max(1e-6), (hi[b] - lo[b]).max(1e-6));
        for s in 0..slabs {
            let (s0, s1) = (lo[axis] + (hi[axis] - lo[axis]) * s as f32 / slabs as f32, lo[axis] + (hi[axis] - lo[axis]) * (s + 1) as f32 / slabs as f32);
            let pts: Vec<&([f32; 3], [f32; 3])> = samples.iter().filter(|p| p.0[axis] >= s0 && (p.0[axis] < s1 || (s + 1 == slabs && p.0[axis] <= s1))).collect();
            if pts.is_empty() {
                println!("  [{s0:7.3}, {s1:7.3}]  empty");
                continue;
            }
            let mut grid = vec![0u32; n * n];
            for p in &pts {
                let (ia, ib) = ((((p.0[a] - lo[a]) / ea) * (n as f32 - 1.0)).round() as usize, (((p.0[b] - lo[b]) / eb) * (n as f32 - 1.0)).round() as usize);
                grid[ia * n + ib] += 1;
            }
            // 8-neighbour flood fill over occupied cells.
            let mut seen = vec![false; n * n];
            let mut islands: Vec<(usize, [usize; 4])> = Vec::new();
            for start in 0..n * n {
                if grid[start] == 0 || seen[start] {
                    continue;
                }
                let (mut count, mut bb) = (0usize, [n, 0, n, 0]);
                let mut stack = vec![start];
                seen[start] = true;
                while let Some(c) = stack.pop() {
                    count += grid[c] as usize;
                    let (ca, cb) = (c / n, c % n);
                    bb = [bb[0].min(ca), bb[1].max(ca), bb[2].min(cb), bb[3].max(cb)];
                    for da in -1i32..=1 {
                        for db in -1i32..=1 {
                            let (na, nb) = (ca as i32 + da, cb as i32 + db);
                            if na < 0 || nb < 0 || na >= n as i32 || nb >= n as i32 {
                                continue;
                            }
                            let nc = na as usize * n + nb as usize;
                            if grid[nc] > 0 && !seen[nc] {
                                seen[nc] = true;
                                stack.push(nc);
                            }
                        }
                    }
                }
                islands.push((count, bb));
            }
            islands.sort_by(|x, y| y.0.cmp(&x.0));
            let world = |i: usize, e: f32, l0: f32, hi_edge: bool| l0 + e * (i as f32 + if hi_edge { 0.5 } else { -0.5 }) / (n as f32 - 1.0);
            let txt: Vec<String> = islands
                .iter()
                .filter(|i| i.0 * 100 >= pts.len().max(1))
                .take(6)
                .map(|(c, bb)| format!("{:.2}..{:.2} x {:.2}..{:.2} ({:.0}%)", world(bb[0], ea, lo[a], false), world(bb[1], ea, lo[a], true), world(bb[2], eb, lo[b], false), world(bb[3], eb, lo[b], true), 100.0 * *c as f64 / pts.len() as f64))
                .collect();
            println!("  [{s0:7.3}, {s1:7.3}]  {} island(s): {}", islands.len(), txt.join(" | "));
        }
    }
    Ok(())
}

/// `pt refine <reference> <recipe.hcs> [--evals N] [--out FILE]`: tune the numbers of a hand-written recipe against a reference.
pub fn refine_cmd(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let [refp, recp] = pos[..] else { return Err("usage: pt refine <reference> <recipe.hcs> [--evals N] [--out FILE]".into()) };
    let evals = flag(args, "--evals").and_then(|v| v.parse().ok()).unwrap_or(800);
    let reference = load_reference(Path::new(refp))?;
    let code = std::fs::read_to_string(recp).map_err(|e| format!("{recp}: {e}"))?;
    let t = Instant::now();
    let (fixed, info) = pt_opt::recipe::refine_source(&code, &reference.geo, evals, 1, 600.0, args.iter().any(|a| a == "--tune-integers"))?;
    let out = flag(args, "--out").unwrap_or(recp);
    std::fs::write(out, &fixed).map_err(|e| format!("{out}: {e}"))?;
    println!("refined {} numbers in {:.1}s ({} evaluations): loss {:.4} -> {:.4}; wrote {out}", info.tuned, t.elapsed().as_secs_f64(), info.evals, info.loss_before, info.loss_after);
    Ok(())
}

/// `pt export <recipe-name-or-file> <out.obj|glb|gltf|usda>`
pub fn export_cmd(args: &[String]) -> Result<(), String> {
    let lod = args.iter().any(|a| a == "--lod");
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [name, out] = pos[..] else { return Err("usage: pt export <recipe-name-or-file> <out.obj|out.glb|out.gltf|out.usda> [--lod]".into()) };
    let path = if Path::new(name).exists() {
        PathBuf::from(name)
    } else {
        crate::recipes::list().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(name.as_str()))).ok_or_else(|| format!("no recipe matching '{name}'"))?
    };
    let (_, _, geo) = cook_recipe(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)?;
    let ext = Path::new(out).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "obj" => std::fs::write(out, pt_core::io::to_obj(&geo)).map_err(|e| e.to_string())?,
        "glb" if lod => std::fs::write(out, pt_core::gltf::to_glb_lod(&geo, &pt_core::lod::LodPolicy { cache_dir: Some("out/lod_cache".into()), ..Default::default() })).map_err(|e| e.to_string())?,
        "glb" => std::fs::write(out, pt_core::gltf::to_glb(&geo)).map_err(|e| e.to_string())?,
        "gltf" => std::fs::write(out, pt_core::io::to_gltf(&geo)).map_err(|e| e.to_string())?,
        "usda" | "usd" => std::fs::write(out, pt_core::io::to_usda(&geo)).map_err(|e| e.to_string())?,
        o => return Err(format!("unknown export format '.{o}'")),
    }
    println!("{} -> {out} ({} points, {} instances)", path.display(), geo.n_points(), geo.n_instances());
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// match-bench
// ---------------------------------------------------------------------------------------------

pub struct BenchOpts {
    pub run: String,
    pub set: String,
    pub limit: Option<usize>,
    /// Only assets whose id contains one of these substrings (comma separated on the command line).
    pub only: Vec<String>,
    pub resume: bool,
    pub evals: usize,
    /// CMA-ES evaluations spent tuning the numbers of a hand-written recipe.
    pub refine_evals: usize,
    pub max_prims: usize,
    pub time_budget_s: f64,
    pub pairs: bool,
    pub results_dir: PathBuf,
    pub recipes_dir: Option<PathBuf>,
    /// Where the final (tuned) recipes are written, one folder per run.
    pub recipes_out: PathBuf,
}

fn parse_bench_opts(args: &[String]) -> Result<(PathBuf, BenchOpts), String> {
    let pos = positional(args);
    let manifest = PathBuf::from(pos.first().copied().unwrap_or("corpus/manifest.json"));
    let num = |n: &str, d: usize| flag(args, n).and_then(|v| v.parse().ok()).unwrap_or(d);
    Ok((
        manifest,
        BenchOpts {
            run: flag(args, "--run").unwrap_or("run1").to_string(),
            set: flag(args, "--set").unwrap_or("trial").to_string(),
            limit: flag(args, "--limit").and_then(|v| v.parse().ok()),
            only: flag(args, "--only").map(|v| v.split(',').map(String::from).collect()).unwrap_or_default(),
            resume: args.iter().any(|a| a == "--resume"),
            evals: num("--evals", 900),
            refine_evals: num("--refine-evals", 6000),
            max_prims: num("--max-prims", 12),
            time_budget_s: flag(args, "--time-budget").and_then(|v| v.parse().ok()).unwrap_or(600.0),
            pairs: !args.iter().any(|a| a == "--no-pairs"),
            results_dir: PathBuf::from(flag(args, "--results").unwrap_or("corpus/results")),
            recipes_dir: flag(args, "--recipes").map(PathBuf::from),
            recipes_out: PathBuf::from(flag(args, "--recipes-out").unwrap_or("corpus/recipes/tuned")),
        },
    ))
}

/// The 20% hold-out: the assets whose id hashes lowest. Chosen by hash so nobody (including us) picks them.
pub fn holdout_ids(ids: &[String]) -> Vec<String> {
    let mut v: Vec<(&String, [u8; 32])> = ids.iter().map(|i| (i, *blake3::hash(i.as_bytes()).as_bytes())).collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v.iter().take(ids.len().div_ceil(5)).map(|x| x.0.clone()).collect()
}

fn short_name(id: &str) -> String {
    id.replace(['/', '\\'], "_")
}

/// Why a rebuild is not solved, in the plan's failure taxonomy.
fn failure_reasons(m: &Metrics, v: &Validity, n_params: usize, timed_out: bool) -> Vec<&'static str> {
    let mut why = Vec::new();
    if timed_out {
        why.push("search_timeout");
    }
    if v.errors > 0 || (v.ref_watertight && !v.watertight) {
        why.push("wrong_topology");
    }
    if !v.within_budget {
        why.push("triangle_budget");
    }
    if n_params > 40 {
        why.push("too_many_parameters");
    }
    if m.f_score < 0.9 && m.recall < 0.9 {
        why.push("missing_detail");
    }
    if m.f_score < 0.9 && m.precision < 0.9 {
        why.push("extra_material");
    }
    if m.silhouette_iou < 0.9 && (m.f_score >= 0.9 || (m.size_ratio - 1.0).abs() > 0.1) {
        why.push("proportion_error");
    }
    why
}

#[allow(clippy::too_many_lines)]
fn bench_one(a: &corpus::AssetEntry, root: &Path, o: &BenchOpts, holdout: bool, pair_dir: &Path, recipe_dir: &Path) -> Value {
    let name = short_name(&a.id);
    let base = json!({"id": a.id, "source": a.source, "category": a.category, "difficulty": a.difficulty, "holdout": holdout});
    let mut entry = base.as_object().cloned().unwrap_or_default();
    let fail = |mut e: serde_json::Map<String, Value>, why: &str, msg: String| {
        e.insert("tier".into(), json!("none"));
        e.insert("solved".into(), json!(false));
        e.insert("failure".into(), json!([why]));
        e.insert("error".into(), json!(msg));
        Value::Object(e)
    };
    let main = root.join(&a.main);
    let loaded = match load_reference(&main) {
        Ok(l) => l,
        Err(e) => return fail(entry, "import_error", e),
    };
    let parts = split_parts(&loaded.groups, 0.003);
    let reference = match Reference::new(&loaded.geo) {
        Ok(r) => r,
        Err(e) => return fail(entry, "import_error", e),
    };
    entry.insert("import".into(), json!({"nodes": loaded.groups.len(), "parts": parts.len(), "triangles": reference.tris.len(), "watertight": welded_watertight(&reference.tris, 1e-4), "diagonal": reference.raw_diag, "warnings": loaded.warnings}));

    let t0 = Instant::now();
    // Hand-written (LLM) recipe if one exists for this asset, otherwise the primitive baseline.
    let hand = o.recipes_dir.as_ref().map(|d| d.join(format!("{name}.hcs"))).filter(|p| p.exists());
    let (cand, hcs, n_params, evals, notes, source, llm_turns): (std::sync::Arc<Geometry>, String, usize, usize, Vec<String>, &str, u64) = if let Some(p) = &hand {
        let written = std::fs::read_to_string(p).unwrap_or_default();
        let meta = std::fs::read_to_string(p.with_extension("meta.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let turns = meta.as_ref().and_then(|v| v.get("llm_turns").and_then(Value::as_u64)).unwrap_or(1);
        // The language model supplies the structure; CMA-ES then tunes only the numbers (plan step c).
        let (code, evals, note) = match pt_opt::recipe::refine_source(&written, &loaded.geo, o.refine_evals, 1, o.time_budget_s, false) {
            Ok((c, info)) => (c, info.evals, format!("refined {} numbers: loss {:.4} -> {:.4}", info.tuned, info.loss_before, info.loss_after)),
            Err(e) => (written.clone(), 0, format!("refinement skipped: {e}")),
        };
        match cook_recipe(&code) {
            Ok((_, _, geo)) => (geo, code.clone(), pt_opt::recipe::count_source_params(&code), evals, vec![note], "llm-recipe", turns),
            Err(e) => return fail(entry, "wrong_topology", format!("hand-written recipe failed to run: {e}")),
        }
    } else {
        let sopts = SolverOptions { seed: 1, max_prims: o.max_prims, max_params: 40, evals: o.evals, time_budget_s: o.time_budget_s };
        match solve(&loaded.geo, &sopts) {
            Ok(mut s) => match s.geometry() {
                Ok(geo) => {
                    // The solver optimised F-score and Chamfer; polish the emitted numbers against the official
                    // objective too (silhouettes included). Same refiner the language-model recipes get.
                    let mut notes = s.notes.clone();
                    let (code, extra, geo) = match pt_opt::recipe::refine_source(&s.hcs, &loaded.geo, o.refine_evals, 1, o.time_budget_s, true) {
                        Ok((c, info)) if info.loss_after < info.loss_before => match cook_recipe(&c) {
                            Ok((_, _, g2)) => {
                                notes.push(format!("polish: loss {:.4} -> {:.4}", info.loss_before, info.loss_after));
                                (c, info.evals, g2)
                            }
                            Err(_) => (s.hcs.clone(), 0, geo),
                        },
                        _ => (s.hcs.clone(), 0, geo),
                    };
                    let knobs = pt_opt::recipe::count_source_params(&code);
                    (geo, code, knobs, s.evals + extra, notes, "primitive-baseline", 0)
                }
                Err(e) => return fail(entry, "wrong_topology", e),
            },
            Err(e) => return fail(entry, "missing_detail", e),
        }
    };
    let secs = t0.elapsed().as_secs_f64();
    let timed_out = hand.is_none() && secs >= o.time_budget_s;
    let (m, mr) = match (reference.score(&cand, Align::Bbox), reference.score(&cand, Align::Reference)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return fail(entry, "wrong_topology", e),
    };
    let v = validity(&cand, &loaded.geo);
    // The bounding-box normalisation hides absolute size, so a rebuild of the wrong overall size cannot be solved.
    let size_ok = (m.size_ratio - 1.0).abs() <= 0.1;
    let t = match tier(&m, v.ok, n_params) {
        "silver" | "gold" if !size_ok => "bronze",
        t => t,
    };
    let solved = is_solved(t);
    let why = if solved { vec![] } else { failure_reasons(&m, &v, n_params, timed_out) };

    let _ = std::fs::create_dir_all(recipe_dir);
    let recipe_path = recipe_dir.join(format!("{name}.hcs"));
    let _ = std::fs::write(&recipe_path, format!("# {} ({source}, run {})\n{hcs}", a.id, o.run));
    if o.pairs {
        let _ = std::fs::create_dir_all(pair_dir);
        if let Ok(png) = pt_render::encode_png(&pair_image(&loaded.geo, &cand)) {
            let _ = std::fs::write(pair_dir.join(format!("{name}.png")), png);
        }
    }
    let key = bench_key(a, root, o);
    entry.insert("metrics".into(), m.to_json());
    entry.insert("metrics_strict".into(), mr.to_json());
    entry.insert("validity".into(), v.to_json());
    entry.insert("free_parameters".into(), json!(n_params));
    entry.insert("tier".into(), json!(t));
    entry.insert("solved".into(), json!(solved));
    entry.insert("failure".into(), json!(why));
    entry.insert("source".into(), json!(source));
    entry.insert("cost".into(), json!({"cpu_seconds": secs, "evals": evals, "llm_turns": llm_turns, "tokens": 0}));
    entry.insert("notes".into(), json!(notes));
    entry.insert("recipe".into(), json!(recipe_path.to_string_lossy()));
    entry.insert("key".into(), json!(key));
    Value::Object(entry)
}

/// Content key for resume: the reference bytes, the solver version and the options that change the answer.
fn bench_key(a: &corpus::AssetEntry, root: &Path, o: &BenchOpts) -> String {
    let mut h = blake3::Hasher::new();
    for f in &a.files {
        if let Ok(b) = std::fs::read(root.join(&f.path)) {
            h.update(&b);
        }
    }
    h.update(SOLVER_VERSION.as_bytes());
    h.update(format!("{}:{}", o.evals, o.max_prims).as_bytes());
    if let Some(d) = &o.recipes_dir {
        if let Ok(b) = std::fs::read(d.join(format!("{}.hcs", short_name(&a.id)))) {
            h.update(&b);
        }
    }
    h.finalize().to_hex().to_string()
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 { "-".into() } else { format!("{:.0}% ({n}/{d})", 100.0 * n as f64 / d as f64) }
}

/// `pt match-bench [manifest] [--run NAME] [--set trial] [--limit N] [--resume] [--evals N] [--max-prims N]
/// [--time-budget S] [--no-pairs] [--results DIR] [--recipes DIR]`
pub fn bench_cmd(args: &[String]) -> Result<(), String> {
    let (manifest, o) = parse_bench_opts(args)?;
    let root = corpus::root();
    let mut assets: Vec<corpus::AssetEntry> = corpus::load_manifest(&manifest)?.into_iter().filter(|a| a.set == o.set).collect();
    let rank = |d: &str| ["primitive", "compound", "building"].iter().position(|x| *x == d).unwrap_or(9);
    assets.sort_by(|a, b| rank(&a.difficulty).cmp(&rank(&b.difficulty)).then(a.id.cmp(&b.id)));
    let ids: Vec<String> = assets.iter().map(|a| a.id.clone()).collect();
    let hold = holdout_ids(&ids);
    if args.iter().any(|a| a == "--only-recipes") {
        if let Some(d) = &o.recipes_dir {
            assets.retain(|a| d.join(format!("{}.hcs", short_name(&a.id))).exists());
        }
    }
    if !o.only.is_empty() {
        assets.retain(|a| o.only.iter().any(|s| a.id.contains(s.as_str())));
    }
    if let Some(n) = o.limit {
        assets.truncate(n);
    }
    std::fs::create_dir_all(&o.results_dir).map_err(|e| e.to_string())?;
    let results_path = o.results_dir.join(format!("{}.json", o.run));
    let mut done: Vec<Value> = Vec::new();
    if o.resume {
        if let Some(v) = std::fs::read_to_string(&results_path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            done = v.get("assets").and_then(Value::as_array).cloned().unwrap_or_default();
        }
    }
    let pair_dir = root.join("out").join(&o.run).join("pairs");
    let recipe_dir = o.recipes_out.join(&o.run);
    let write = |done: &[Value]| -> Result<(), String> {
        let doc = json!({"run": o.run, "solver": SOLVER_VERSION, "set": o.set, "options": {"evals": o.evals, "max_prims": o.max_prims, "time_budget_s": o.time_budget_s},
                         "holdout": hold, "assets": done, "summary": summarize(done)});
        std::fs::write(&results_path, serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    };
    let started = Instant::now();
    for (i, a) in assets.iter().enumerate() {
        let key = bench_key(a, &root, &o);
        if done.iter().any(|d| d.get("id").and_then(Value::as_str) == Some(a.id.as_str()) && d.get("key").and_then(Value::as_str) == Some(key.as_str())) {
            println!("[{}/{}] {:<44} resumed (already finished)", i + 1, assets.len(), a.id);
            continue;
        }
        done.retain(|d| d.get("id").and_then(Value::as_str) != Some(a.id.as_str()));
        let e = bench_one(a, &root, &o, hold.contains(&a.id), &pair_dir, &recipe_dir);
        let g = |p: &str| e.pointer(p).and_then(Value::as_f64).unwrap_or(0.0);
        println!(
            "[{}/{}] {:<44} {:<7} F {:.3} IoU {:.3} params {:>3} {:>6.1}s {}{}",
            i + 1,
            assets.len(),
            a.id,
            e.get("tier").and_then(Value::as_str).unwrap_or("?"),
            g("/metrics/f_score"),
            g("/metrics/silhouette_iou"),
            e.get("free_parameters").and_then(Value::as_u64).unwrap_or(0),
            g("/cost/cpu_seconds"),
            if e.get("holdout") == Some(&json!(true)) { "[hold-out] " } else { "" },
            e.get("failure").and_then(Value::as_array).map(|f| f.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(",")).unwrap_or_default()
        );
        done.push(e);
        write(&done)?;
    }
    write(&done)?;
    let s = summarize(&done);
    println!("\n== {} ({} assets, {:.0}s wall) ==", o.run, done.len(), started.elapsed().as_secs_f64());
    println!("solved (silver+): all {}  mining set {}  hold-out {}", pct(s["solved"].as_u64().unwrap_or(0) as usize, s["total"].as_u64().unwrap_or(0) as usize), pct(s["solved_mining"].as_u64().unwrap_or(0) as usize, s["total_mining"].as_u64().unwrap_or(0) as usize), pct(s["solved_holdout"].as_u64().unwrap_or(0) as usize, s["total_holdout"].as_u64().unwrap_or(0) as usize));
    println!("tiers: {}", s["tiers"]);
    println!("failures: {}", s["failures"]);
    println!("results: {}", results_path.display());
    Ok(())
}

pub fn summarize(done: &[Value]) -> Value {
    let count = |f: &dyn Fn(&Value) -> bool| done.iter().filter(|d| f(d)).count();
    let solved = |d: &Value| d.get("solved") == Some(&json!(true));
    let hold = |d: &Value| d.get("holdout") == Some(&json!(true));
    let mut tiers = serde_json::Map::new();
    let mut failures = serde_json::Map::new();
    for d in done {
        let t = d.get("tier").and_then(Value::as_str).unwrap_or("none").to_string();
        let n = tiers.get(&t).and_then(Value::as_u64).unwrap_or(0);
        tiers.insert(t, json!(n + 1));
        for f in d.get("failure").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            let n = failures.get(f).and_then(Value::as_u64).unwrap_or(0);
            failures.insert(f.to_string(), json!(n + 1));
        }
    }
    let mean = |p: &str| -> f64 {
        let v: Vec<f64> = done.iter().filter_map(|d| d.pointer(p).and_then(Value::as_f64)).collect();
        if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
    };
    json!({
        "total": done.len(), "solved": count(&solved),
        "total_holdout": count(&hold), "solved_holdout": count(&|d| hold(d) && solved(d)),
        "total_mining": count(&|d| !hold(d)), "solved_mining": count(&|d| !hold(d) && solved(d)),
        "tiers": tiers, "failures": failures,
        "mean_f_score": mean("/metrics/f_score"), "mean_silhouette_iou": mean("/metrics/silhouette_iou"),
        "mean_cpu_seconds": mean("/cost/cpu_seconds"), "mean_llm_turns": mean("/cost/llm_turns"), "mean_free_parameters": mean("/free_parameters"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holdout_is_twenty_percent_and_stable() {
        let ids: Vec<String> = (0..50).map(|i| format!("asset/{i}")).collect();
        let a = holdout_ids(&ids);
        assert_eq!(a.len(), 10);
        let mut shuffled = ids.clone();
        shuffled.reverse();
        let mut b = holdout_ids(&shuffled);
        let mut a2 = a.clone();
        a2.sort();
        b.sort();
        assert_eq!(a2, b, "the hold-out depends only on the ids, not on their order");
    }

    #[test]
    fn failure_taxonomy_names_the_dominant_problem() {
        let v = Validity { errors: 0, watertight: true, ref_watertight: true, triangles: 10, ref_triangles: 10, within_budget: true, ok: true };
        let m = Metrics { f_score: 0.7, precision: 0.95, recall: 0.55, silhouette_iou: 0.6, size_ratio: 1.0, ..Default::default() };
        let why = failure_reasons(&m, &v, 55, false);
        assert!(why.contains(&"missing_detail") && why.contains(&"too_many_parameters") && !why.contains(&"extra_material"));
        let bad = Validity { errors: 2, ok: false, ..v };
        assert!(failure_reasons(&m, &bad, 5, true).contains(&"wrong_topology"));
    }

    #[test]
    fn summary_counts_hold_out_separately() {
        let mk = |solved: bool, hold: bool| json!({"solved": solved, "holdout": hold, "tier": if solved { "silver" } else { "none" }});
        let s = summarize(&[mk(true, false), mk(false, false), mk(true, true), mk(false, true)]);
        assert_eq!((s["solved_mining"].as_u64(), s["total_mining"].as_u64(), s["solved_holdout"].as_u64(), s["total_holdout"].as_u64()), (Some(1), Some(2), Some(1), Some(2)));
    }
}

#[cfg(test)]
mod acceptance {
    //! Phase 0 acceptance tests that need the whole stack: recipe perturbation and kill/restart resume.
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn scores_decrease_as_a_recipe_is_perturbed_with_gym_perturb() {
        // A small recipe with real structure: a table. The reference is its own unperturbed cook.
        let code = "top = box(size=[1.6, 0.1, 0.9], center=[0.0, 0.75, 0.0])\nleg = box(size=[0.1, 0.7, 0.1], center=[0.7, 0.35, 0.35])\nlegs = mirror(mirror(leg, axis=\"x\", offset=0.0), axis=\"z\", offset=0.0)\nout(merge(top, legs))\n";
        let (_, _, reference) = cook_recipe(code).unwrap();
        let r = Reference::new(&reference).unwrap();
        let mean_f = |amp: f64| -> f64 {
            let scores: Vec<f64> = (0..8u64)
                .filter_map(|seed| {
                    let (_, _, g) = cook_recipe(&crate::gym::perturb(code, seed, amp)).ok()?;
                    Some(r.score(&g, Align::Reference).ok()?.f_score)
                })
                .collect();
            assert!(scores.len() >= 5, "most perturbed recipes must still run ({} of 8)", scores.len());
            scores.iter().sum::<f64>() / scores.len() as f64
        };
        let (a, b, c) = (mean_f(0.02), mean_f(0.12), mean_f(0.4));
        assert!(a > b && b > c, "F-score should fall with the size of the perturbation: {a:.3} > {b:.3} > {c:.3}");
        assert!(a > 0.9, "a 2% nudge barely matters: {a:.3}");
    }

    #[test]
    fn bench_resumes_without_redoing_finished_assets() {
        let _guard = crate::corpus::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("pt_bench_resume_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("raw")).unwrap();
        let cube = |s: f32| format!("v 0 0 0\nv {s} 0 0\nv {s} {s} 0\nv 0 {s} 0\nv 0 0 {s}\nv {s} 0 {s}\nv {s} {s} {s}\nv 0 {s} {s}\nf 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 3 4 8 7\nf 2 3 7 6\nf 1 5 8 4\n");
        write(&dir.join("raw"), "a.obj", &cube(1.0));
        write(&dir.join("raw"), "b.obj", &cube(2.0));
        let entry = |id: &str, f: &str| format!("{{\"id\":\"{id}\",\"source\":\"t\",\"set\":\"trial\",\"category\":\"box\",\"difficulty\":\"primitive\",\"license\":\"CC0-1.0\",\"license_url\":\"u\",\"attribution\":\"a\",\"main\":\"raw/{f}\",\"files\":[{{\"path\":\"raw/{f}\",\"url\":\"http://unused\"}}]}}");
        let manifest = dir.join("m.json");
        write(&dir, "m.json", &format!("{{\"assets\":[{},{}]}}", entry("t/a", "a.obj"), entry("t/b", "b.obj")));
        std::env::set_var("PT_CORPUS", &dir);
        let args = |extra: &[&str]| -> Vec<String> {
            let mut v: Vec<String> = vec![manifest.to_string_lossy().into(), "--run".into(), "resume_test".into(), "--evals".into(), "40".into(), "--refine-evals".into(), "40".into(), "--no-pairs".into(), "--results".into(), dir.join("results").to_string_lossy().into(), "--recipes-out".into(), dir.join("recipes").to_string_lossy().into()];
            v.extend(extra.iter().map(|s| s.to_string()));
            v
        };
        // First process "dies" after one asset (--limit 1), second one resumes and finishes both.
        bench_cmd(&args(&["--limit", "1"])).unwrap();
        let path = dir.join("results").join("resume_test.json");
        let first: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let first_assets = first["assets"].as_array().unwrap().clone();
        assert_eq!(first_assets.len(), 1);
        bench_cmd(&args(&["--resume"])).unwrap();
        let second: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let all = second["assets"].as_array().unwrap();
        assert_eq!(all.len(), 2);
        let kept = all.iter().find(|a| a["id"] == first_assets[0]["id"]).unwrap();
        assert_eq!(kept["cost"]["cpu_seconds"], first_assets[0]["cost"]["cpu_seconds"], "the finished asset must be carried over, not recomputed");
        assert_eq!(kept["key"], first_assets[0]["key"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `pt audit <manifest> <results.json>... --out audit.json`: the second reading (largest uncovered patch) for every
/// recipe in the given result files, so a silver that is missing a visible part can be told apart from a clean one.
pub fn audit_cmd(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let (manifest, files) = pos.split_first().ok_or("usage: pt audit <manifest> <results.json>... --out audit.json")?;
    let out = flag(args, "--out").unwrap_or("corpus/results/audit.json");
    let root = corpus::root();
    let assets = corpus::load_manifest(Path::new(manifest))?;
    let mut doc = serde_json::Map::new();
    for f in files {
        let res: Value = serde_json::from_str(&std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?).map_err(|e| format!("{f}: {e}"))?;
        let run = res.get("run").and_then(Value::as_str).unwrap_or("run").to_string();
        let mut per = serde_json::Map::new();
        for a in res.get("assets").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), Some(recipe)) = (a.get("id").and_then(Value::as_str), a.get("recipe").and_then(Value::as_str)) else { continue };
            let Some(entry) = assets.iter().find(|x| x.id == id) else { continue };
            let reference = load_reference(&root.join(&entry.main))?;
            let r = Reference::new(&reference.geo)?;
            let (cand, _) = load_candidate(Path::new(recipe))?;
            let (share, big) = uncovered_patches(&r, &cand)?;
            per.insert(id.to_string(), json!({"largest_patch": share, "patches_over_2pct": big}));
        }
        doc.insert(run, Value::Object(per));
    }
    std::fs::write(out, serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}
