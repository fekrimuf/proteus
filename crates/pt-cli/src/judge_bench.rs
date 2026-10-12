//! Calibration and use of the vision judge (`pt judge ...`). A judge enters a gate only after it has been scored against answers
//! that are known by construction:
//!   * `critique`: 10 tasks x 4 candidates of graded quality, truth = the task specs (the experiment the author model also took),
//!   * `audit`: the 24 solved rebuilds of Checkpoint 1, 6 of which were flagged by eye as missing a feature, plus two controls
//!     (an original against itself must be faithful; an original against a different object must not be),
//!   * `frames`: world frames against their own brief and against foils, with a blank control.
//! Results are written to `corpus/results/judge/` and `gym/`, always under the judge's name.

use crate::judge::{borda, split_pair, Judge, JUDGE_NAME};
use crate::report::load;
use pt_render::Image;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// Positional arguments: everything that is not a flag or a flag's value.
fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(a.as_str());
        }
    }
    out
}

fn results_dir() -> PathBuf {
    let d = crate::blender::repo_root().join("corpus").join("results").join("judge");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The critique experiment with this judge: every pair of candidates of a task, both orders, scored by pairwise wins, then the
/// same scoring as the author-model judgments (`pt critique-bench score`).
pub fn run_critique(j: &Judge, dir: &Path, method: &str) -> Result<Value, String> {
    let blind: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("blind.json")).map_err(|e| format!("{}: run `pt critique-bench prepare` first ({e})", dir.display()))?).map_err(|e| e.to_string())?;
    let mut scores = serde_json::Map::new();
    let (mut pairs_total, mut contradictory) = (0usize, 0usize);
    for task in blind.as_array().ok_or("blind.json is not a list")? {
        let name = task["task"].as_str().unwrap_or("?");
        let brief = task["brief"].as_str().unwrap_or("");
        let Some(images) = task["images"].as_object() else { return Err("blind.json has no per-candidate images: run `pt critique-bench prepare` again".into()) };
        let slots: Vec<&String> = images.keys().collect();
        let pngs: Vec<Vec<u8>> = slots.iter().map(|k| read(Path::new(images[*k].as_str().unwrap_or("")))).collect::<Result<_, _>>()?;
        let imgs: Vec<Image> = pngs.iter().map(|p| pt_render::decode_png(p)).collect::<Result<_, _>>()?;
        let sc = if method == "sheet" {
            j.rank_sheet(brief, &imgs)?
        } else {
            let mut pairs = Vec::new();
            for a in 0..slots.len() {
                for b in a + 1..slots.len() {
                    let v = if method == "sbs" { j.compare_side_by_side(brief, &imgs[a], &imgs[b])? } else { j.compare(brief, &pngs[a], &pngs[b])? };
                    pairs_total += 1;
                    contradictory += v.contradictory as usize;
                    pairs.push((a, b, v.margin));
                }
            }
            borda(slots.len(), &pairs)
        };
        scores.insert(name.into(), Value::Object(slots.iter().zip(&sc).map(|(k, v)| ((*k).clone(), json!(v))).collect()));
        eprintln!("  {name}: {}", slots.iter().zip(&sc).map(|(k, v)| format!("{k}={v:.2}")).collect::<Vec<_>>().join(" "));
    }
    let judgments = dir.join(format!("judgments_{JUDGE_NAME}_{method}.json"));
    let doc = json!({"judge": format!("{JUDGE_NAME} {method}"), "scores": scores, "pairs": pairs_total, "contradictory_pairs": contradictory});
    std::fs::write(&judgments, serde_json::to_string_pretty(&doc).unwrap()).map_err(|e| e.to_string())?;
    let out = PathBuf::from(format!("gym/critique_experiment_{JUDGE_NAME}_{method}.json"));
    crate::critique_bench::score_to(&judgments, dir, &out)?;
    if pairs_total > 0 {
        println!("order contradictions: {contradictory} of {pairs_total} pairs ({:.0}%)", 100.0 * contradictory as f64 / pairs_total as f64);
    }
    let mut result: Value = serde_json::from_str(&std::fs::read_to_string(&out).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    result["order_contradictions"] = json!({"pairs": pairs_total, "contradictory": contradictory});
    std::fs::write(&out, serde_json::to_string_pretty(&result).unwrap()).map_err(|e| e.to_string())?;
    Ok(result)
}

/// One solved rebuild of Checkpoint 1: where its pair picture is and whether it was flagged as missing a feature.
pub struct AuditItem {
    pub id: String,
    pub pair: PathBuf,
    pub flagged: Option<String>,
}

/// The calibration set, frozen on 2026-10-10 when the judge was measured: the 24 rebuilds that Checkpoint 1 called solved, their pair
/// pictures as they were then, and the nine that miss a feature (`corpus/results/judge/audit_set/`). It does not follow later repairs, so
/// `pt judge audit` always measures the judge on the same pictures.
pub fn audit_items() -> Result<Vec<AuditItem>, String> {
    let dir = crate::blender::repo_root().join("corpus").join("results").join("judge").join("audit_set");
    let labels = load(&dir.join("labels.json"))?;
    Ok(labels
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| AuditItem { id: o["id"].as_str().unwrap_or("").to_string(), pair: dir.join(o["file"].as_str().unwrap_or("")), flagged: o["flagged"].as_str().map(String::from) })
        .collect())
}

fn confusion(rows: &[(bool, bool)]) -> (usize, usize, usize, usize) {
    let (mut tp, mut fp, mut fneg, mut tn) = (0, 0, 0, 0);
    for &(truth, said) in rows {
        match (truth, said) {
            (true, true) => tp += 1,
            (false, true) => fp += 1,
            (true, false) => fneg += 1,
            (false, false) => tn += 1,
        }
    }
    (tp, fp, fneg, tn)
}

/// Score the part-counting judge on the 24 solved rebuilds of Checkpoint 1, with an identity control (an original against itself)
/// and a different-object control. The judge only counts what it sees in each column; `structure_diff` decides what differs.
/// The question format was chosen after a leading question ("does the copy miss a feature?") flagged every item, including 22 of 24
/// pictures compared with themselves (`audit_qwen3-vl-8b-q4_verdict.json`). The schema was written after seeing the first six defects, so
/// these numbers are development numbers; the clean test is the fresh 50 assets of Phase 1.
pub fn run_audit(j: &Judge) -> Result<Value, String> {
    let items = audit_items()?;
    let mut cols: Vec<(Image, Image)> = Vec::new();
    for it in &items {
        let img: Image = pt_render::decode_png(&read(&it.pair)?)?;
        cols.push(split_pair(&img));
    }
    let n = items.len();
    // one question about an (original, copy) pair of columns: do the counted parts differ, and how
    let ask = |orig: &Image, copy: &Image| -> Result<(bool, Vec<String>), String> {
        let (diff, _) = j.structure(&pt_render::grid(&[orig.clone(), copy.clone()], 2, 4))?;
        Ok((!diff.is_empty(), diff))
    };
    let (mut main, mut ident, mut other) = (Vec::new(), Vec::new(), Vec::new());
    let mut rows = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let (said, named) = ask(&cols[i].0, &cols[i].1)?;
        main.push((it.flagged.is_some(), said));
        let (said_same, _) = ask(&cols[i].0, &cols[i].0)?;
        ident.push((false, said_same));
        let (said_other, _) = ask(&cols[i].0, &cols[(i + 7) % n].1)?; // 7 is coprime with 24: a different object every time
        other.push((true, said_other));
        eprintln!("  {:<40} flagged={:<5} judge={:<5} identity={:<5} other_object={:<5} {}", it.id, it.flagged.is_some(), said, said_same, said_other, named.join("; "));
        rows.push(json!({"id": it.id, "flagged_by_eye": it.flagged, "judge_says_missing": said, "named": named, "identity_control_false_alarm": said_same, "other_object_detected": said_other}));
    }
    let (tp, fp, fneg, tn) = confusion(&main);
    let (_, ident_fp, _, ident_tn) = confusion(&ident);
    let (other_tp, _, other_fn, _) = confusion(&other);
    let pos = tp + fneg;
    let neg = fp + tn;
    // the same two halves as the checkpoint (by hash of the id), for comparison with its numbers
    let half = |dev: bool| -> Vec<(bool, bool)> {
        items.iter().zip(&main).filter(|(it, _)| (blake3::hash(it.id.as_bytes()).as_bytes()[0] < 128) == dev).map(|(_, m)| *m).collect()
    };
    let (dtp, dfp, dfn, dtn) = confusion(&half(true));
    let (htp, hfp, hfn, htn) = confusion(&half(false));
    let result = json!({
        "judge": JUDGE_NAME, "method": "structure", "items": n,
        "flagged_recall": {"found": tp, "of": pos},
        "false_alarms_on_unflagged": {"count": fp, "of": neg},
        "identity_control_false_alarms": {"count": ident_fp, "of": ident_fp + ident_tn},
        "other_object_detected": {"count": other_tp, "of": other_tp + other_fn},
        "development_half": {"tp": dtp, "fp": dfp, "fn": dfn, "tn": dtn},
        "hold_out_half": {"tp": htp, "fp": hfp, "fn": hfn, "tn": htn},
        "rows": rows,
    });
    println!("flagged defects found: {tp} of {pos}   false alarms on the other {neg}: {fp}   identity control false alarms: {ident_fp} of {n}   different object detected: {other_tp} of {n}");
    println!("development half tp/fp/fn/tn {dtp}/{dfp}/{dfn}/{dtn}   hold-out half {htp}/{hfp}/{hfn}/{htn}");
    std::fs::write(results_dir().join(format!("audit_{JUDGE_NAME}_structure.json")), serde_json::to_string_pretty(&result).unwrap()).map_err(|e| e.to_string())?;
    Ok(result)
}

/// Foils for the brief-fit gate when the caller names none: three very different kinds of world.
pub const DEFAULT_FOILS: [&str; 3] = [
    "a sci-fi village of metal domes, glowing panels and landing pads",
    "a hot desert with sand dunes and no plants",
    "the inside of a furnished apartment",
];

fn grey_control() -> Result<Vec<u8>, String> {
    pt_render::encode_png(&Image { w: 640, h: 360, rgb: vec![128; 640 * 360 * 3] })
}

/// Judge the frames of one world against its brief: a 0-10 fit, and a forced choice between the brief and each foil (both orders).
pub fn judge_frames(j: &Judge, frames: &[PathBuf], brief: &str, foils: &[String]) -> Result<Value, String> {
    let mut rows = Vec::new();
    let (mut fit_sum, mut wins, mut total, mut contradictions) = (0.0, 0usize, 0usize, 0usize);
    for f in frames {
        let png = read(f)?;
        let (fit, seen) = j.rate(brief, &png)?;
        fit_sum += fit;
        let mut vs = Vec::new();
        for foil in foils {
            let v = j.which_brief(&png, brief, foil)?;
            total += 1;
            wins += (v.margin > 0.0) as usize;
            contradictions += v.contradictory as usize;
            vs.push(json!({"foil": foil, "margin": v.margin, "contradictory": v.contradictory}));
        }
        rows.push(json!({"file": f.display().to_string(), "fit": fit, "seen": seen, "versus": vs}));
    }
    let (grey_fit, _) = j.rate(brief, &grey_control()?)?;
    Ok(json!({
        "judge": JUDGE_NAME, "brief": brief, "foils": foils,
        "frames": rows,
        "mean_fit": if frames.is_empty() { 0.0 } else { fit_sum / frames.len() as f64 },
        "brief_picked": {"count": wins, "of": total},
        "contradictions": contradictions,
        "control_blank_fit": grey_fit,
    }))
}

/// `pt judge status | compare A B --brief T | rate IMG --brief T | which IMG --a T --b T | parts PAIR.png | critique [dir] | audit | frames IMG... --brief T [--foil T]...`
pub fn judge_cmd(args: &[String]) -> Result<(), String> {
    let pos = positional(&args[1.min(args.len())..]);
    match args.first().map(String::as_str) {
        Some("status") => {
            let j = Judge::local();
            for (what, p) in [("server", std::env::var("PT_JUDGE_SERVER").unwrap_or(crate::judge::DEFAULT_SERVER.into())), ("model", std::env::var("PT_JUDGE_MODEL").unwrap_or(crate::judge::DEFAULT_MODEL.into())), ("projector", std::env::var("PT_JUDGE_MMPROJ").unwrap_or(crate::judge::DEFAULT_MMPROJ.into()))] {
                println!("{what:<10} {} {p}", if Path::new(&p).exists() { "found  " } else { "MISSING" });
            }
            j.ensure_server()?;
            println!("judge {JUDGE_NAME} answers");
            Ok(())
        }
        Some("compare") => {
            let (a, b) = (pos.first().ok_or("usage: pt judge compare A.png B.png --brief TEXT")?, pos.get(1).ok_or("two images")?);
            let brief = flag(args, "--brief").ok_or("--brief TEXT")?;
            let img = |p: &str| pt_render::decode_png(&read(Path::new(p))?);
            let v = Judge::local().compare_side_by_side(brief, &img(a)?, &img(b)?)?;
            println!("{}", serde_json::to_string_pretty(&json!({"margin_toward_first": v.margin, "contradictory": v.contradictory, "reasons": v.reasons})).unwrap());
            Ok(())
        }
        Some("parts") => {
            let p = pos.first().ok_or("usage: pt judge parts PAIR.png (a `pt match --report` pair picture: original left, rebuild right)")?;
            let (diff, counted) = Judge::local().structure(&pt_render::decode_png(&read(Path::new(p))?)?)?;
            println!("{}", serde_json::to_string_pretty(&json!({"verdict": if diff.is_empty() { "parts match" } else { "parts differ" }, "differences": diff, "counted": counted})).unwrap());
            Ok(())
        }
        Some("rate") => {
            let a = pos.first().ok_or("usage: pt judge rate IMG.png --brief TEXT")?;
            let (score, seen) = Judge::local().rate(flag(args, "--brief").ok_or("--brief TEXT")?, &read(Path::new(a))?)?;
            println!("{score}/10  ({seen})");
            Ok(())
        }
        Some("which") => {
            let a = pos.first().ok_or("usage: pt judge which IMG.png --a TEXT --b TEXT")?;
            let v = Judge::local().which_brief(&read(Path::new(a))?, flag(args, "--a").ok_or("--a TEXT")?, flag(args, "--b").ok_or("--b TEXT")?)?;
            println!("{}", serde_json::to_string_pretty(&json!({"margin_toward_a": v.margin, "contradictory": v.contradictory, "seen": v.reasons})).unwrap());
            Ok(())
        }
        Some("critique") => {
            let dir = PathBuf::from(pos.first().copied().unwrap_or("out/critique"));
            run_critique(&Judge::local(), &dir, flag(args, "--method").unwrap_or("sbs")).map(|_| ())
        }
        Some("audit") => run_audit(&Judge::local()).map(|_| ()),
        Some("frames") => {
            let brief = flag(args, "--brief").ok_or("usage: pt judge frames IMG.png... --brief TEXT [--foil TEXT]...")?;
            let mut foils: Vec<String> = args.windows(2).filter(|w| w[0] == "--foil").map(|w| w[1].clone()).collect();
            if foils.is_empty() {
                foils = DEFAULT_FOILS.iter().map(|s| s.to_string()).collect();
            }
            let frames: Vec<PathBuf> = pos.iter().map(PathBuf::from).collect();
            let r = judge_frames(&Judge::local(), &frames, brief, &foils)?;
            println!("{}", serde_json::to_string_pretty(&r).unwrap());
            Ok(())
        }
        _ => Err("usage: pt judge status | compare A B --brief T | rate IMG --brief T | which IMG --a T --b T | parts PAIR.png | critique [dir] [--method sheet|sbs|pairs] | audit | frames IMG... --brief T [--foil T]...".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confusion_counts_each_cell() {
        assert_eq!(confusion(&[(true, true), (true, false), (false, true), (false, false), (false, false)]), (1, 1, 1, 2));
    }

    #[test]
    fn positional_skips_flag_values() {
        let a: Vec<String> = ["frames", "a.png", "--brief", "x y", "b.png", "--foil", "z"].iter().map(|s| s.to_string()).collect();
        assert_eq!(positional(&a[1..]), vec!["a.png", "b.png"]);
        assert_eq!(flag(&a, "--brief"), Some("x y"));
    }

    #[test]
    fn the_audit_set_is_the_24_solved_results_with_nine_flags() {
        let items = audit_items().unwrap();
        assert_eq!(items.len(), 24, "Checkpoint 1: 24 solved results");
        assert_eq!(items.iter().filter(|i| i.flagged.is_some()).count(), 9, "six from the first eye read, three the judge found and the eye confirmed");
        assert!(items.iter().all(|i| !i.id.is_empty()));
    }
}
