//! Measured demos for the Phase 3 gates (run with `pt optimize-demo` / `pt vary-bench`).

use crate::recipes::recipes_dir;
use pt_graph::Graph;
use pt_opt::{optimize, vary, write_values, Objective, OptOptions, ParamRef};
use pt_render::Mode;
use serde_json::json;
use std::time::Instant;

fn pr(node: &str, param: &str, index: Option<usize>, key: Option<&str>, min: f64, max: f64) -> ParamRef {
    ParamRef { node: node.into(), param: param.into(), index, key: key.map(String::from), min, max }
}

fn rock_space() -> Vec<ParamRef> {
    vec![
        pr("rough", "channels", None, Some("freq"), 0.8, 3.0),
        pr("rough", "channels", None, Some("amp"), 0.1, 0.6),
        pr("rough", "channels", None, Some("ridge"), 0.0, 0.4),
        pr("rough", "channels", None, Some("cut"), 0.0, 0.25),
        pr("squashed", "s", Some(0), None, 0.7, 1.8),
        pr("squashed", "s", Some(1), None, 0.5, 1.2),
        pr("squashed", "s", Some(2), None, 0.7, 1.5),
    ]
}

fn rock_graph() -> Result<Graph, String> {
    let code = std::fs::read_to_string(recipes_dir().join("03_rock_generator.hcs")).map_err(|e| e.to_string())?;
    let mut g = Graph::new();
    pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    Ok(g)
}

/// Phase 3 gate: fit a procedural rock to scans (noisy point clouds) it has never seen.
pub fn optimize_demo() -> Result<(), String> {
    let hidden: [[f64; 7]; 3] = [
        [2.4, 0.5, 0.22, 0.15, 1.5, 0.7, 1.1],
        [1.2, 0.28, 0.1, 0.05, 1.0, 1.0, 1.0],
        [2.9, 0.42, 0.3, 0.2, 1.7, 0.6, 1.4],
    ];
    println!("== Phase 3 gate: fit a procedural rock to held-out scans (7 parameters) ==");
    println!("budget 1500 evaluations, 5 restarts, scans = noisy point clouds of rocks with hidden parameters\n");
    let space = rock_space();
    let mut all_pass = true;
    for (n, truth) in hidden.iter().enumerate() {
        // Build the hidden target and its "scan": surface samples with sensor noise.
        let mut tg = rock_graph()?;
        write_values(&mut tg, &space, truth)?;
        let geo = tg.cook("rock")?;
        let mut pts = pt_sop::sample_surface(&geo, 4000, 99)?;
        for (i, p) in pts.iter_mut().enumerate() {
            let r = pt_core::rng::rand4(7, 5, i as u64);
            for a in 0..3 {
                p[a] += (r[a] - 0.5) * 0.006;
            }
        }
        let scan = pt_core::Geometry::from_points(pts);
        let obj = Objective::chamfer(&scan, 1500)?;
        let mut g = rock_graph()?;
        let t = Instant::now();
        let rep = optimize(&mut g, &space, &obj, "rock", &OptOptions { budget: 1500, seed: 11 + n as i64, restarts: 5, sigma0: 0.3 })?;
        let secs = t.elapsed().as_secs_f64();
        // Held-out check: a different noisy sampling of the SAME target shape.
        let mut hpts = pt_sop::sample_surface(&geo, 4000, 12345)?;
        for (i, p) in hpts.iter_mut().enumerate() {
            let r = pt_core::rng::rand4(8, 6, i as u64);
            for a in 0..3 {
                p[a] += (r[a] - 0.5) * 0.006;
            }
        }
        let held = Objective::chamfer(&pt_core::Geometry::from_points(hpts), 1500)?;
        let held_loss = held.eval(&mut g, "rock")?;
        // The scan has sensor noise and finite sampling, so the loss cannot reach 0: the floor is
        // the loss of the TRUE parameters. Score how much of the achievable improvement we got.
        let mut oracle_graph = rock_graph()?;
        write_values(&mut oracle_graph, &space, truth)?;
        let oracle = obj.eval(&mut oracle_graph, "rock")?;
        let reduction = (rep.initial_loss - rep.best_loss) / (rep.initial_loss - oracle).max(1e-12);
        let ok = reduction >= 0.90 && rep.best_loss <= oracle * 1.10;
        all_pass &= ok;
        println!("target {} : loss {:.5} -> {:.5}  (floor at true params {:.5}; {:.0}% of achievable gain)  held-out {:.5}  {} evals in {:.1}s  [{}]", n + 1, rep.initial_loss, rep.best_loss, oracle, reduction * 100.0, held_loss, rep.evals, secs, if ok { "PASS" } else { "FAIL" });
        for ((label, t), b) in rep.labels.iter().zip(truth).zip(&rep.best) {
            println!("    {label:<22} truth {t:>6.3}   found {b:>6.3}");
        }
        std::fs::create_dir_all("out/optimize").ok();
        let (img, _) = pt_render::render(&*g.cook("rock")?, &pt_render::Opts { width: 480, height: 360, ..Default::default() });
        std::fs::write(format!("out/optimize/fit_{}.png", n + 1), pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
        let (img, _) = pt_render::render(&geo, &pt_render::Opts { width: 480, height: 360, ..Default::default() });
        std::fs::write(format!("out/optimize/target_{}.png", n + 1), pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
    }
    println!("\nGATE (>=90% of the achievable gain, within 10% of the noise floor, on all held-out targets): {}", if all_pass { "PASS" } else { "FAIL" });
    if all_pass { Ok(()) } else { Err("optimize gate failed".into()) }
}

/// Phase 3 gate: variant exploration shares the content-addressed cache.
pub fn vary_bench() -> Result<(), String> {
    let code = std::fs::read_to_string(recipes_dir().join("07_terrain_valley.hcs")).map_err(|e| e.to_string())?;
    let mut g = Graph::new();
    pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let space = vec![
        pr("conifer", "radius", None, None, 0.3, 0.9),
        pr("conifer", "height", None, None, 1.5, 4.5),
        pr("trees", "rot_y_random", None, None, 0.0, 360.0),
        pr("trees", "seed", None, None, 1.0, 50.0),
    ];
    let n = 64;
    println!("== Phase 3 gate: 64-variant exploration of the terrain-valley recipe ==");
    println!("varying tree shape and placement only (the 160x160 terrain, erosion-free noise stack and Poisson scatter are unchanged)\n");
    let samples = pt_opt::spread_samples(space.len(), n, 3);
    // Warm: one graph, shared cache.
    let mut warm = g_clone(&g)?;
    warm.cook("valley")?;
    let t = Instant::now();
    for u in &samples {
        let x: Vec<f64> = space.iter().zip(u).map(|(p, u)| p.min + u * (p.max - p.min)).collect();
        write_values(&mut warm, &space, &x)?;
        warm.cook("valley")?;
    }
    let shared = t.elapsed().as_secs_f64();
    let shared_computed = warm.stats.computed;
    // Cold: clear the cache before every variant (what a cache-less system does).
    let mut cold = g_clone(&g)?;
    let t = Instant::now();
    for u in &samples {
        let x: Vec<f64> = space.iter().zip(u).map(|(p, u)| p.min + u * (p.max - p.min)).collect();
        write_values(&mut cold, &space, &x)?;
        cold.clear_cache();
        cold.cook("valley")?;
    }
    let naive = t.elapsed().as_secs_f64();
    println!("shared cache : {:.3} s for {n} variants  ({} node computations)", shared, shared_computed);
    println!("no cache     : {:.3} s for {n} variants  ({} node computations)", naive, cold.stats.computed);
    println!("speedup      : {:.1}x", naive / shared.max(1e-9));
    let v = vary(&mut g, &space, "valley", 12, 3, (192, 144), Mode::Shaded, None)?;
    std::fs::create_dir_all("out/optimize").ok();
    std::fs::write("out/optimize/variants.png", pt_render::encode_png(&v.sheet)?).map_err(|e| e.to_string())?;
    println!("contact sheet of 12 diverse variants: out/optimize/variants.png");
    println!("first variant parameters: {}", json!(space.iter().map(ParamRef::label).zip(v.values[0].iter().copied()).collect::<std::collections::BTreeMap<_, _>>()));
    Ok(())
}

fn g_clone(g: &Graph) -> Result<Graph, String> {
    Graph::from_json(&g.to_json())
}

/// Phase 6 gate: recover scene parameters from images of our own scenes (silhouettes from three
/// views are the only information the optimizer gets about the target).
pub fn recover_demo() -> Result<(), String> {
    let hidden: [[f64; 7]; 3] = [
        [2.4, 0.5, 0.22, 0.15, 1.5, 0.7, 1.1],
        [1.3, 0.3, 0.12, 0.08, 1.2, 0.9, 0.8],
        [2.8, 0.4, 0.28, 0.18, 0.9, 1.0, 1.4],
    ];
    println!("== Phase 6 gate: recover parameters from synthetic renders (3 silhouette views) ==");
    let space = rock_space();
    let mut all_pass = true;
    for (n, truth) in hidden.iter().enumerate() {
        let mut tg = rock_graph()?;
        write_values(&mut tg, &space, truth)?;
        let target = tg.cook("rock")?;
        // Coarse-to-fine: global multi-start search on 64px silhouettes, then refine on 160px ones.
        let coarse = Objective::silhouette(&target, 64)?;
        let fine = Objective::silhouette(&target, 160)?;
        let mut g = rock_graph()?;
        let first = optimize(&mut g, &space, &coarse, "rock", &OptOptions { budget: 2500, seed: 21 + n as i64, restarts: 7, sigma0: 0.3 })?;
        let mut rep = optimize(&mut g, &space, &fine, "rock", &OptOptions { budget: 700, seed: 5 + n as i64, restarts: 0, sigma0: 0.06 })?;
        rep.initial_loss = fine.eval(&mut { let mut t = rock_graph()?; write_values(&mut t, &space, &first.initial)?; t }, "rock")?;
        let iou = 1.0 - rep.best_loss;
        let ok = iou >= 0.95;
        all_pass &= ok;
        println!("target {}: silhouette IoU {:.3} -> {:.3}  [{}]", n + 1, 1.0 - rep.initial_loss, iou, if ok { "PASS" } else { "FAIL" });
        let shape: Vec<String> = rep.labels.iter().zip(truth).zip(&rep.best).filter(|((l, _), _)| l.starts_with("squashed")).map(|((l, t), b)| format!("{l} {t:.2}->{b:.2}")).collect();
        println!("    scale parameters (identifiable from silhouettes): {}", shape.join("  "));
    }
    println!("\nGATE (mean silhouette IoU >= 0.95 on every target): {}", if all_pass { "PASS" } else { "FAIL" });
    if all_pass { Ok(()) } else { Err("recover gate failed".into()) }
}
