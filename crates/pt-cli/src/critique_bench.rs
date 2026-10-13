//! Critique experiment: does an automatic judge agree with the spec-based ground truth about which
//! candidate scene better satisfies a brief?
//!
//! `pt critique-bench prepare [dir]` builds, per task, four anonymous candidates of graded quality
//! (the oracle plus numerically perturbed variants), renders them with the SAME camera (framed on the
//! oracle) into one 2x2 sheet (slots A B / C D, shuffled), and writes `blind.json` (what a judge sees) and
//! `key.json` (hidden: reward, generic rubric score, SSIM-to-oracle per slot).
//! `pt critique-bench score <judgments.json> [dir]` compares a judge's 0-10 scores per slot against the key.

use crate::critique::critique_json;
use crate::gym::{tasks, Env, Task};
use pt_core::rng::rand4;
use pt_graph::Graph;
use pt_render::{Image, Mode, Opts};
use serde_json::{json, Value};
use std::path::Path;

const TASKS: [&str; 12] = [
    "01_fence", "02_spiral_staircase", "03_rock_generator", "04_tree", "05_building", "06_pipe_network", "07_terrain_valley", "09_colonnade", "10_scifi_panel", "12_mechanical_bracket", "18_bolt", "21_city_block",
];
const SLOTS: [&str; 4] = ["A", "B", "C", "D"];

fn cook_code(code: &str) -> Option<pt_core::Geometry> {
    let mut g = Graph::new();
    let rep = pt_hcs::run(code, &mut g, true).ok()?;
    g.cook(rep.outputs.first()?).ok().map(|a| (*a).clone())
}

fn framed_render(geo: &pt_core::Geometry, bounds: ([f32; 3], [f32; 3])) -> Image {
    let o = Opts { width: 400, height: 300, mode: Mode::Shaded, fit_bounds: Some(bounds), ..Opts::default() };
    pt_render::render(geo, &o).0
}


struct Cand {
    kind: String,
    reward: f64,
    code: String,
}

fn candidates(t: &Task) -> Vec<Cand> {
    // Perturb the oracle at several strengths, keep programs that build, then pick three spread over the reward range.
    let mut pool: Vec<Cand> = Vec::new();
    for seed in 0..24u64 {
        let amp = [0.25, 0.6, 0.9][(seed / 8) as usize % 3];
        let code = crate::gym::perturb(&t.reference, 500 + seed, amp);
        let r = Env::new(t.clone()).step(&code).reward;
        if r >= 0.2 && r < 0.999 && cook_code(&code).is_some() {
            pool.push(Cand { kind: format!("perturbed(seed {seed}, amp {amp})"), reward: r, code });
        }
    }
    pool.sort_by(|a, b| a.reward.partial_cmp(&b.reward).unwrap_or(std::cmp::Ordering::Equal).then(a.code.cmp(&b.code)));
    pool.dedup_by(|a, b| (a.reward - b.reward).abs() < 0.02);
    let mut picks: Vec<Cand> = Vec::new();
    if !pool.is_empty() {
        let n = pool.len();
        let mut idx = vec![0, n / 2, n - 1];
        idx.dedup();
        for i in idx {
            picks.push(Cand { kind: pool[i].kind.clone(), reward: pool[i].reward, code: pool[i].code.clone() });
        }
    }
    picks.push(Cand { kind: "oracle".into(), reward: 1.0, code: t.reference.clone() });
    picks
}

pub fn prepare(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let all = tasks();
    let (mut blind, mut key) = (Vec::new(), serde_json::Map::new());
    for name in TASKS {
        let Some(t) = all.iter().find(|t| t.id == name) else { continue };
        let Some(oracle_geo) = cook_code(&t.reference) else { continue };
        let Some(bounds) = oracle_geo.bounds() else { continue };
        let oracle_img = framed_render(&oracle_geo, bounds);
        let mut cands = candidates(t);
        if cands.len() < 3 {
            println!("skip {name}: only {} candidates", cands.len());
            continue;
        }
        cands.truncate(4);
        // Deterministic shuffle into slots.
        let mut order: Vec<usize> = (0..cands.len()).collect();
        for i in (1..order.len()).rev() {
            let r = (rand4(31, name.len() as u32 + i as u32, name.bytes().map(|b| b as u64).sum())[0] * (i + 1) as f32) as usize;
            order.swap(i, r.min(i));
        }
        let mut imgs = Vec::new();
        let mut slots = serde_json::Map::new();
        let mut files = serde_json::Map::new();
        for (slot, &ci) in order.iter().enumerate() {
            let c = &cands[ci];
            let geo = cook_code(&c.code).ok_or("candidate stopped building")?;
            let img = framed_render(&geo, bounds);
            let generic = critique_json(&geo, &Opts { width: 480, height: 360, ..Opts::default() }, None)["score"].as_f64().unwrap_or(0.0);
            let ssim = pt_render::metrics::compare(&img, &oracle_img).ssim;
            slots.insert(SLOTS[slot].into(), json!({"kind": c.kind, "reward": c.reward, "generic_critique": generic, "ssim_to_oracle": ssim}));
            // one file per candidate, for judges that take images one at a time (`pt judge critique`)
            let one = dir.join(format!("cand_{name}_{}.png", SLOTS[slot]));
            std::fs::write(&one, pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
            files.insert(SLOTS[slot].into(), json!(one.display().to_string()));
            imgs.push(img);
        }
        while imgs.len() < 4 {
            imgs.push(Image { w: imgs[0].w, h: imgs[0].h, rgb: vec![255; imgs[0].rgb.len()] });
        }
        let png = dir.join(format!("sheet_{name}.png"));
        std::fs::write(&png, pt_render::encode_png(&pt_render::grid(&imgs, 2, 4))?).map_err(|e| e.to_string())?;
        blind.push(json!({"task": name, "sheet": png.display().to_string(), "layout": "A B / C D (top row A B, bottom row C D)", "brief": t.brief, "images": Value::Object(files)}));
        key.insert(name.into(), Value::Object(slots));
    }
    std::fs::write(dir.join("blind.json"), serde_json::to_string_pretty(&blind).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("key.json"), serde_json::to_string_pretty(&key).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("prepared {} tasks in {} (judge sees blind.json + sheet_*.png; key.json stays hidden)", blind.len(), dir.display());
    Ok(())
}

fn ranks(v: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut r = vec![0.0; v.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for &k in &idx[i..=j] {
            r[k] = avg;
        }
        i = j + 1;
    }
    r
}

/// Spearman rank correlation (NaN-free: returns 0 when a series is constant).
pub fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let (ra, rb) = (ranks(a), ranks(b));
    let n = a.len() as f64;
    let (ma, mb) = (ra.iter().sum::<f64>() / n, rb.iter().sum::<f64>() / n);
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        c += (ra[i] - ma) * (rb[i] - mb);
        va += (ra[i] - ma).powi(2);
        vb += (rb[i] - mb).powi(2);
    }
    if va == 0.0 || vb == 0.0 { 0.0 } else { c / (va * vb).sqrt() }
}

/// Fraction of within-task pairs (with different true reward) that the judge orders correctly; ties count half.
pub fn pairwise_accuracy(groups: &[(Vec<f64>, Vec<f64>)]) -> f64 {
    let (mut hit, mut total) = (0.0, 0.0);
    for (truth, judge) in groups {
        for i in 0..truth.len() {
            for j in i + 1..truth.len() {
                if truth[i] == truth[j] {
                    continue;
                }
                total += 1.0;
                let (dt, dj) = (truth[i] - truth[j], judge[i] - judge[j]);
                if dj == 0.0 {
                    hit += 0.5;
                } else if dt * dj > 0.0 {
                    hit += 1.0;
                }
            }
        }
    }
    if total == 0.0 { 0.0 } else { hit / total }
}

pub fn score(judgments: &Path, dir: &Path) -> Result<(), String> {
    score_to(judgments, dir, Path::new("gym/critique_experiment.json"))
}

/// As `score`, recording the result in `out_path` (one file per judge, so results can be compared side by side).
pub fn score_to(judgments: &Path, dir: &Path, out_path: &Path) -> Result<(), String> {
    let key: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("key.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let j: Value = serde_json::from_str(&std::fs::read_to_string(judgments).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let judge_name = j["judge"].as_str().unwrap_or("unnamed judge").to_string();
    let scores = &j["scores"];
    // Per judge kind: pooled series and per-task groups.
    let kinds = ["judge", "generic_critique", "ssim_to_oracle"];
    let mut pooled: Vec<(Vec<f64>, Vec<f64>)> = kinds.iter().map(|_| (vec![], vec![])).collect();
    let mut groups: Vec<Vec<(Vec<f64>, Vec<f64>)>> = kinds.iter().map(|_| vec![]).collect();
    let mut best_hits = [0.0f64; 3];
    let mut n_tasks = 0.0;
    for (task, slots) in key.as_object().ok_or("bad key")? {
        let Some(js) = scores.get(task) else { continue };
        n_tasks += 1.0;
        let mut per: Vec<(Vec<f64>, Vec<f64>)> = kinds.iter().map(|_| (vec![], vec![])).collect();
        for (slot, meta) in slots.as_object().ok_or("bad key: a task's slots are not an object")? {
            let Some(js) = js.get(slot).and_then(Value::as_f64) else { continue };
            let truth = meta["reward"].as_f64().ok_or("bad key: a slot has no numeric reward")?;
            for (k, kind) in kinds.iter().enumerate() {
                let v = if *kind == "judge" { js } else { meta[*kind].as_f64().unwrap_or(0.0) };
                per[k].0.push(truth);
                per[k].1.push(v);
            }
        }
        for k in 0..kinds.len() {
            pooled[k].0.extend(&per[k].0);
            pooled[k].1.extend(&per[k].1);
            // Does the judge's top pick (ties split) coincide with the truly best candidate?
            let top = per[k].1.iter().cloned().fold(f64::MIN, f64::max);
            let best_truth = per[k].0.iter().cloned().fold(f64::MIN, f64::max);
            let tied: Vec<usize> = (0..per[k].1.len()).filter(|&i| per[k].1[i] == top).collect();
            best_hits[k] += tied.iter().filter(|&&i| per[k].0[i] == best_truth).count() as f64 / tied.len() as f64;
            groups[k].push(per[k].clone());
        }
    }
    if n_tasks == 0.0 {
        return Err("no matching tasks in the judgments file".into());
    }
    println!("judge: {judge_name}   tasks: {n_tasks}");
    println!("{:<18} {:>10} {:>14} {:>14}", "scorer", "spearman", "pairwise acc", "picks best");
    let mut report = serde_json::Map::new();
    for (k, kind) in kinds.iter().enumerate() {
        let rho = spearman(&pooled[k].0, &pooled[k].1);
        let acc = pairwise_accuracy(&groups[k].iter().map(|(t, s)| (t.clone(), s.clone())).collect::<Vec<_>>());
        let best = best_hits[k] / n_tasks;
        println!("{:<18} {:>10.3} {:>14.3} {:>14.3}", if *kind == "judge" { judge_name.as_str() } else { kind }, rho, acc, best);
        report.insert((*kind).into(), json!({"spearman": rho, "pairwise_accuracy": acc, "picks_best": best}));
    }
    let out = json!({"judge": judge_name, "tasks": n_tasks, "candidates_per_task": 4, "results": report});
    if let Some(dir) = out_path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(out_path, serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("recorded {}", out_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spearman_and_pairwise_behave() {
        assert!((spearman(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]) - 1.0).abs() < 1e-12);
        assert!((spearman(&[1.0, 2.0, 3.0, 4.0], &[4.0, 3.0, 2.0, 1.0]) + 1.0).abs() < 1e-12);
        assert_eq!(spearman(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), 0.0);
        let g = vec![(vec![0.2, 0.5, 1.0], vec![1.0, 2.0, 3.0]), (vec![0.2, 0.5], vec![2.0, 2.0])];
        // 3 pairs fully right + 1 pair tied (half) over 4 pairs.
        assert!((pairwise_accuracy(&g) - 3.5 / 4.0).abs() < 1e-12);
    }

    #[test]
    fn candidate_sets_span_reward_range_and_include_oracle() {
        let t = tasks().into_iter().find(|t| t.id == "01_fence").unwrap();
        let c = candidates(&t);
        assert!(c.len() >= 3 && c.iter().any(|c| c.kind == "oracle" && c.reward == 1.0));
        assert!(c.iter().any(|c| c.reward < 0.95), "need at least one clearly worse candidate");
    }
}

#[cfg(test)]
mod leak_tests {
    use super::*;

    #[test]
    fn blind_file_does_not_leak_ground_truth() {
        let dir = std::env::temp_dir().join("pt_critique_blind_test");
        let _ = std::fs::remove_dir_all(&dir);
        prepare(&dir).unwrap();
        let blind = std::fs::read_to_string(dir.join("blind.json")).unwrap();
        for forbidden in ["reward", "oracle", "perturbed", "ssim", "generic_critique", "seed"] {
            assert!(!blind.contains(forbidden), "blind.json leaks '{forbidden}'");
        }
        let key: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("key.json")).unwrap()).unwrap();
        // Every task has exactly one oracle slot with reward 1.
        for (task, slots) in key.as_object().unwrap() {
            let oracles = slots.as_object().unwrap().values().filter(|m| m["kind"] == "oracle" && m["reward"] == 1.0).count();
            assert_eq!(oracles, 1, "{task}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
