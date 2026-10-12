//! `pt scorecard`: the AAA Readiness Scorecard (PRO_QUALITY_PLAN B13.1), v0.
//!
//! One build of a world recipe is measured by three independent referees and the result is one page and one JSON file:
//!   * Blender Cycles (how it looks: material coverage, atmosphere),
//!   * Godot (what an engine makes of the delivered glTF: import, triangles in frame, frame rate, navigation mesh),
//!   * the Khronos glTF validator (is the file clean),
//! plus audits of the cooked geometry itself (repeats, LOD chains, asset variety) and of provenance (every external file
//! has a ledger row). Only measures that exist today are listed; a new measure is added the day its referee exists.
//! Every bar is a DRAFT from B13.1 until it is calibrated against reference scenes, and the page says so.

use crate::blender;
use pt_core::Geometry;
use pt_graph::Graph;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub const DEFAULT_VALIDATOR: &str = r"D:\Fekri\tools\gltf_validator\gltf_validator.exe";
pub const DEFAULT_GODOT: &str = r"D:\Fekri\tools\godot\Godot_v4.7.2-stable_win64_console.exe";

fn tool(env: &str, default: &str) -> PathBuf {
    std::env::var(env).map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(default))
}

pub struct Measure {
    pub family: &'static str,
    pub id: &'static str,
    pub label: &'static str,
    pub value: Option<f64>,
    pub shown: String,
    pub bar: String,
    /// None = informational (no bar yet).
    pub pass: Option<bool>,
    /// What we do when it fails (the route, one sentence).
    pub route: &'static str,
}

impl Measure {
    fn to_json(&self) -> Value {
        json!({"family": self.family, "id": self.id, "label": self.label, "value": self.value, "shown": self.shown, "bar": self.bar,
               "status": match self.pass { Some(true) => "pass", Some(false) => "fail", None => "info" }, "route": self.route, "draft_bar": true})
    }
}

#[derive(Default)]
pub struct Audit {
    pub base_triangles: u64,
    pub instances: u64,
    pub total_triangles: u64,
    pub unique_triangles: u64,
    pub external_triangles: u64,
    pub external_unique_triangles: u64,
    pub max_repeats: u64,
    /// Most copies of one LARGE prototype (over 2 m) inside any 50 m x 50 m square: what a viewer can see repeating.
    pub max_local_repeats: u64,
    pub protos: Vec<Value>,
    pub heavy_protos: u64,
    pub heavy_with_lod: u64,
    pub assets: BTreeSet<String>,
    pub area_km2: f64,
    /// (objects floating or hanging in a camera view, objects mostly under water in a camera view, cameras checked); `None` when the
    /// scene has no standing cameras. From `pt ground-check`'s rules.
    pub placement: Option<(u64, u64, u64)>,
}

/// Triangle and repeat statistics of a cooked world (base geometry plus lazy instances).
pub fn audit(geo: &Geometry) -> Audit {
    let mut a = Audit { base_triangles: geo.triangles().len() as u64, ..Default::default() };
    a.total_triangles = a.base_triangles;
    a.unique_triangles = a.base_triangles;
    if let Some((lo, hi)) = geo.bounds() {
        a.area_km2 = ((hi[0] - lo[0]) as f64 * (hi[2] - lo[2]) as f64) / 1.0e6;
    }
    let Some(inst) = &geo.instances else { return a };
    // Per-square-kilometre measures use the DRESSED area (the box around the placed instances), not the whole mesh: a far ring of
    // distant terrain with no instances on it is scenery, not content.
    if inst.len() > 0 {
        let (mut x0, mut x1, mut z0, mut z1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        // (a far stand-in, `lod_hint` with a draw distance, is scenery too: the spikes of a forest kilometres away)
        let scenery: Vec<bool> = inst.protos.iter().map(|p| pt_core::lod::hinted_cull(p).is_some()).collect();
        for (k, xf) in inst.xf.iter().enumerate() {
            if scenery[inst.proto_id[k] as usize] {
                continue;
            }
            x0 = x0.min(xf[3]);
            x1 = x1.max(xf[3]);
            z0 = z0.min(xf[11]);
            z1 = z1.max(xf[11]);
        }
        let dressed = ((x1 - x0) as f64 * (z1 - z0) as f64) / 1.0e6;
        if dressed > 0.0 {
            a.area_km2 = a.area_km2.min(dressed);
        }
    }
    let mut counts = vec![0u64; inst.protos.len()];
    for &p in inst.proto_id.iter() {
        counts[p as usize] += 1;
    }
    a.instances = inst.len() as u64;
    let mut large = vec![false; inst.protos.len()];
    for (i, p) in inst.protos.iter().enumerate() {
        // a far stand-in (`lod_hint` with a draw distance: the spikes of a distant forest) is not a prototype a viewer can see
        // repeating; it is what a forest looks like from a kilometre away
        // Large = stands up: over 2 m in its longest side AND at least 1.5 m tall (a tree, a tall bush, a boulder). A 4 m patch of
        // grass is about a metre tall: it is ground cover, drawn a hundred times in a 50 m square by design, and nobody sees one
        // patch repeat.
        large[i] = pt_core::lod::hinted_cull(p).is_none() && p.bounds().is_some_and(|(lo, hi)| (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]) > 2.0 && hi[1] - lo[1] >= 1.5);
    }
    let mut cells: std::collections::HashMap<(i32, i32, u32), u64> = std::collections::HashMap::new();
    for (k, xf) in inst.xf.iter().enumerate() {
        let p = inst.proto_id[k];
        if large[p as usize] {
            *cells.entry(((xf[3] / 50.0).floor() as i32, (xf[11] / 50.0).floor() as i32, p)).or_default() += 1;
        }
    }
    a.max_local_repeats = cells.values().copied().max().unwrap_or(0);
    for (i, p) in inst.protos.iter().enumerate() {
        if counts[i] == 0 {
            continue; // carried but never placed: not part of the world
        }
        let real = p.realize();
        let tris = real.triangles().len() as u64;
        let sources: BTreeSet<String> = real.detail.iter().filter(|(k, _)| k.starts_with(pt_core::gltf::ASSET_MATERIAL_PREFIX)).filter_map(|(_, v)| v.get("source").and_then(Value::as_str).map(String::from)).collect();
        let external = !sources.is_empty();
        a.assets.extend(sources.iter().cloned());
        a.total_triangles += tris * counts[i];
        a.unique_triangles += tris;
        if external {
            a.external_triangles += tris * counts[i];
            a.external_unique_triangles += tris;
        }
        a.max_repeats = a.max_repeats.max(counts[i]);
        let has_lod = real.detail.get("lod_levels").and_then(Value::as_u64).unwrap_or(1) > 1;
        if tris > 2000 {
            a.heavy_protos += 1;
            if has_lod {
                a.heavy_with_lod += 1;
            }
        }
        a.protos.push(json!({"index": i, "triangles": tris, "instances": counts[i], "source": sources.iter().next(), "lod": has_lod}));
    }
    a
}

/// Every external file the build leans on, checked against the licence ledger: asset glTFs and the PBR sets of the materials in use.
pub fn ledger_trace(a: &Audit, scene: &Value, lib: &Value, geo: &Geometry, ledger: &crate::corpus::Ledger) -> (usize, usize, Vec<String>) {
    let mut wanted: BTreeSet<String> = a.assets.clone();
    let mut names: BTreeSet<String> = geo.material_names().into_iter().collect();
    for p in geo.instances.iter().flat_map(|i| i.protos.iter()) {
        names.extend(p.realize().material_names());
    }
    for roles in blender::layers_of(geo).values() {
        names.extend(roles.iter().cloned());
    }
    if let Some(g) = scene.pointer("/ground/material").and_then(Value::as_str) {
        names.insert(g.to_string());
    }
    for n in names {
        let key = scene.pointer(&format!("/materials/{n}/use")).and_then(Value::as_str).unwrap_or(&n);
        if let Some(id) = lib.pointer(&format!("/materials/{key}/polyhaven")).and_then(Value::as_str) {
            wanted.insert(format!("polyhaven/materials/{id}/{id}_diff_1k.jpg"));
        }
    }
    let missing: Vec<String> = wanted.iter().filter(|w| !ledger.rows.contains_key(w.as_str())).cloned().collect();
    (wanted.len() - missing.len(), wanted.len(), missing)
}

/// Fold the exporter's LOD report (`asset.extras.proteus_lod` of the engine glTF) into the audit: which prototypes got a chain.
pub fn apply_lod_report(a: &mut Audit, report: &Value) {
    let entries = report.as_array().cloned().unwrap_or_default();
    a.heavy_protos = 0;
    a.heavy_with_lod = 0;
    for p in a.protos.iter_mut() {
        let idx = p["index"].as_u64();
        let chain = entries.iter().find(|e| e["proto"].as_u64() == idx).and_then(|e| e["triangles"].as_array().cloned());
        let has = chain.as_ref().is_some_and(|c| c.len() > 1);
        p["lod"] = json!(has);
        if let Some(c) = chain {
            p["lod_triangles"] = json!(c);
        }
        if p["triangles"].as_u64().unwrap_or(0) > 2000 {
            a.heavy_protos += 1;
            a.heavy_with_lod += has as u64;
        }
    }
}

/// Which materials of an engine glTF carry no texture (so the engine would show a flat grey).
pub fn engine_materials(doc: &Value) -> Value {
    let mats = doc.get("materials").and_then(Value::as_array).cloned().unwrap_or_default();
    let untextured: Vec<&str> = mats
        .iter()
        .filter(|m| m.pointer("/pbrMetallicRoughness/baseColorTexture").is_none() && m.pointer("/extras/proteus_flat").and_then(Value::as_bool) != Some(true))
        .filter_map(|m| m.get("name").and_then(Value::as_str))
        .collect();
    json!({"materials": mats.len(), "untextured": untextured})
}

/// Tell the engine file what the style says about every material that is neither authored in an asset nor layered: a library
/// scan becomes `library:<name>` (the colour map tiles through box-projected UVs), a plain colour becomes `flat:<name>`.
/// Without this an engine shows a procedural hut and the water as grey placeholders.
pub fn style_records(g: &mut Geometry, scene: &Value, lib: &Value, corpus: &Path) {
    for name in g.material_names() {
        if g.detail.contains_key(&format!("{}{name}", pt_core::gltf::ASSET_MATERIAL_PREFIX)) || g.detail.contains_key(&format!("layers:{name}")) {
            continue;
        }
        let spec = scene.pointer(&format!("/materials/{name}")).cloned().unwrap_or(json!({}));
        let key = spec.get("use").and_then(Value::as_str).unwrap_or(&name).to_string();
        let entry = lib.pointer(&format!("/materials/{key}")).cloned().unwrap_or(json!({}));
        let pick = |k: &str| spec.get(k).or_else(|| entry.get(k)).cloned();
        let file = entry.get("polyhaven").and_then(Value::as_str).filter(|_| entry.get("kind").is_none()).map(|id| corpus.join("polyhaven/materials").join(id).join(format!("{id}_diff_1k.jpg"))).filter(|p| p.exists());
        match file {
            Some(path) => {
                g.detail.insert(format!("library:{name}"), json!({"diff": path.to_string_lossy(), "scale_m": pick("scale_m").unwrap_or(json!(1.0)), "tint": pick("tint").unwrap_or(json!([1.0, 1.0, 1.0])), "roughness": pick("roughness").unwrap_or(json!(0.85))}));
            }
            None => {
                if let Some(color) = pick("color").or_else(|| pick("fallback")) {
                    g.detail.insert(format!("flat:{name}"), json!({"color": color, "roughness": pick("roughness").unwrap_or(json!(0.8))}));
                }
            }
        }
    }
}

/// The report of a stage that finished earlier, if every file it lists is still there: `list` points at an array of objects or
/// an object of objects, and `key` names the file path inside each. `None` when the stage has to run again.
fn finished_stage(report: &Path, list: &str, key: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(report).ok()?).ok()?;
    let items: Vec<&Value> = match v.pointer(list)? {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => o.values().collect(),
        _ => return None,
    };
    let all_there = !items.is_empty() && items.iter().all(|i| i.get(key).and_then(Value::as_str).is_some_and(|p| Path::new(p).exists()));
    all_there.then_some(v)
}

fn absolute(p: &Path) -> PathBuf {
    std::env::current_dir().unwrap_or_default().join(p)
}

fn run_validator(glb: &Path) -> Value {
    let glb = absolute(glb);
    let exe = tool("PT_GLTF_VALIDATOR", DEFAULT_VALIDATOR);
    if !exe.exists() {
        return json!({"available": false});
    }
    // The validator holds about ten times the file in memory and can take longer than everything else together on a file of
    // several hundred megabytes; it writes to a file and gets a time limit, so a slow validation is reported, not waited for forever.
    let report = std::env::temp_dir().join(format!("pt_validator_{}.txt", std::process::id()));
    let Ok(sink) = std::fs::File::create(&report) else { return json!({"available": false}) };
    let Ok(sink_err) = sink.try_clone() else { return json!({"available": false}) };
    // the summary line ("Errors: 0, Warnings: 0, ...") goes to stderr, the JSON report to stdout: both into the one file
    let Ok(mut child) = crate::lowload::spawn(Command::new(&exe).arg(&glb).arg("-o").stdout(sink).stderr(sink_err)) else { return json!({"available": false}) };
    let limit = std::env::var("PT_VALIDATOR_SECONDS").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(900);
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed().as_secs() < limit => std::thread::sleep(std::time::Duration::from_millis(500)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&report);
                return json!({"available": true, "timed_out": true, "seconds": limit});
            }
        }
    }
    let text = std::fs::read_to_string(&report).unwrap_or_default();
    let _ = std::fs::remove_file(&report);
    let line = text.lines().find(|l| l.starts_with("Errors:")).unwrap_or("");
    let n = |key: &str| line.split(',').find(|p| p.trim().starts_with(key)).and_then(|p| p.split(':').nth(1)).and_then(|v| v.trim().parse::<u64>().ok());
    let mut codes: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(json_start) = text.find('{') {
        if let Ok(v) = serde_json::from_str::<Value>(&text[json_start..]) {
            for m in v.pointer("/issues/messages").and_then(Value::as_array).into_iter().flatten() {
                if let Some(c) = m.get("code").and_then(Value::as_str) {
                    *codes.entry(c.to_string()).or_default() += 1;
                }
            }
        }
    }
    json!({"available": true, "errors": n("Errors"), "warnings": n("Warnings"), "codes": codes})
}

fn run_godot(glb: &Path, extra: &[&str], windowed: bool) -> Value {
    let glb = absolute(glb).to_string_lossy().replace('\\', "/");
    let exe = tool("PT_GODOT", DEFAULT_GODOT);
    if !exe.exists() {
        return json!({"available": false});
    }
    let project = blender::repo_root().join("engine/godot");
    let mut cmd = Command::new(&exe);
    if !windowed {
        cmd.arg("--headless");
    }
    cmd.arg("--path").arg(&project).args(["--script", "referee.gd", "--"]).arg(&glb).args(extra);
    let Ok(out) = crate::lowload::output(&mut cmd) else { return json!({"available": false}) };
    let text = String::from_utf8_lossy(&out.stdout);
    match text.lines().find_map(|l| l.strip_prefix("REFEREE_JSON ")).and_then(|j| serde_json::from_str::<Value>(j).ok()) {
        Some(mut v) => {
            v["available"] = json!(true);
            v
        }
        None => json!({"available": true, "ok": false, "errors": [format!("Godot printed no result (exit {:?})", out.status.code())]}),
    }
}

fn f(v: &Value, p: &str) -> Option<f64> {
    v.pointer(p).and_then(Value::as_f64)
}

fn fmt_big(x: f64) -> String {
    if x >= 1.0e9 {
        format!("{:.2} billion", x / 1.0e9)
    } else if x >= 1.0e6 {
        format!("{:.1} million", x / 1.0e6)
    } else if x >= 1.0e3 {
        format!("{:.1} thousand", x / 1.0e3)
    } else {
        format!("{x:.0}")
    }
}

/// Assemble the measures from everything the referees reported. Pure, so it is unit-tested.
pub fn measures(audit: &Audit, render: &Value, godot: &Value, fps: &Value, validator: &Value, engine: &Value, traced: (usize, usize), scene: &Value, judge: &Value) -> Vec<Measure> {
    let mut m: Vec<Measure> = Vec::new();
    let mut add = |family, id, label, value: Option<f64>, shown: String, bar: &str, pass: Option<bool>, route| {
        m.push(Measure { family, id, label, value, shown, bar: bar.into(), pass, route });
    };
    // Visual
    let cov = f(render, "/material_coverage");
    add("Visual", "v_coverage", "Surfaces with a real material", cov, cov.map_or("not rendered".into(), |c| format!("{:.1}%", c * 100.0)), "at least 99%", cov.map(|c| c >= 0.99), "give every role a library material or an authored asset material");
    let unres = render.get("unresolved").and_then(Value::as_array).map_or(0, Vec::len) + render.get("unknown_materials").and_then(Value::as_array).map_or(0, Vec::len);
    add("Visual", "v_unresolved", "Material names nobody could resolve", cov.map(|_| unres as f64), cov.map_or("not rendered".into(), |_| unres.to_string()), "0", cov.map(|_| unres == 0), "add the name to the library or the style");
    let sky = scene.pointer("/lighting/sky").and_then(Value::as_str).unwrap_or("nishita");
    let fog = scene.pointer("/lighting/fog_density").and_then(Value::as_f64).unwrap_or(0.0);
    let atmo = sky != "flat" && fog > 0.0;
    add("Visual", "v_atmosphere", "Physical sky, shadows and depth haze on", Some(atmo as u8 as f64), format!("sky {sky}, haze {}", if fog > 0.0 { "on" } else { "off" }), "sky and haze on", Some(atmo), "set lighting.sky and lighting.fog_density in the style");
    // The vision judge (pt judge): does the frame read as the brief, and does it read as the brief rather than as a different world?
    let judged = judge.get("frames").and_then(Value::as_array).is_some_and(|f| !f.is_empty());
    let fit = f(judge, "/mean_fit").filter(|_| judged);
    add("Visual", "v_brief_fit", "The judge rates the frames against the brief (0 to 10)", fit, fit.map_or("not judged".into(), |x| format!("{x:.1} of 10")), "at least 7 of 10", fit.map(|x| x >= 7.0), "read the judge's sentences on the page: the missing feature becomes a generator task");
    let (won, of) = (f(judge, "/brief_picked/count"), f(judge, "/brief_picked/of"));
    let share = won.zip(of).filter(|(_, o)| *o > 0.0 && judged).map(|(w, o)| w / o);
    add("Visual", "v_brief_vs_foils", "Frames the judge reads as the brief rather than as three other worlds", share, share.map_or("not judged".into(), |s| format!("{:.0}% ({} of {})", s * 100.0, won.unwrap_or(0.0), of.unwrap_or(0.0))), "at least 90%", share.map(|s| s >= 0.9), "the world does not read as its brief: strengthen the style (palette, shape language) and the typology");
    let blank = f(judge, "/control_blank_fit").filter(|_| judged);
    let contra = f(judge, "/contradictions").zip(of).filter(|_| judged).map(|(c, o)| if o > 0.0 { c / o } else { 0.0 });
    let sane = blank.zip(contra).map(|(b, c)| b <= 2.0 && c <= 0.2);
    add("Visual", "v_judge_sane", "Judge reliability: a blank frame scores 0 to 2 and its answers hold when the order is swapped", blank, sane.map_or("not judged".into(), |_| format!("blank scores {:.0}, {:.0}% of answers flipped with the order", blank.unwrap_or(0.0), contra.unwrap_or(0.0) * 100.0)), "blank at most 2, flips at most 20%", sane, "re-calibrate the judge (pt judge critique, pt judge audit) before trusting the Visual rows");
    // Art coherence
    let style = render.get("style").and_then(Value::as_str).or_else(|| scene.get("style_name").and_then(Value::as_str));
    add("Art coherence", "a_style", "A style drove the look", Some(style.is_some() as u8 as f64), style.unwrap_or("none").to_string(), "a named style", Some(style.is_some()), "give the scene a style file");
    let variants = audit.protos.len() as f64;
    add("Art coherence", "a_variety", "Distinct prototypes in the world", Some(variants), format!("{variants:.0}"), "at least 12", Some(variants >= 12.0), "more prototypes per role (generator families)");
    // World function
    // The intended walkable ground (slope under 25 degrees) that the baked navigation mesh reaches. The first version divided the
    // navmesh area by ALL slope-walkable ground, which is structurally below 100%: the mesh is inset by the agent radius along every
    // cliff edge. B13.1 asks for coverage of the INTENDED walkable area, so that is what is measured now.
    let nav = f(godot, "/navmesh/coverage_intended");
    let hang = audit.placement.map(|(h, _, _)| h);
    add("World function", "w_grounded", "Objects floating or hanging in the air inside a camera view (a stone over the bed, a bush over a bank)", hang.map(|h| h as f64), hang.map_or("no standing camera".into(), |h| format!("{h}")), "0", hang.map(|h| h == 0), "drop_to_surface with a footprint, and `pt ground-check --frames` to see them");
    let shallow = audit.placement.map(|(_, s, _)| s);
    add("World function", "w_shallow", "Stones mostly under water inside a camera view (they read as flat lids)", shallow.map(|s| s as f64), shallow.map_or("no standing camera".into(), |s| format!("{s}")), "at most 10", shallow.map(|s| s <= 10), "place stones out of the water unless they stand clear of it (water depth under 40% of the stone's height)");
    add("World function", "w_nav", "Intended walkable ground (slope under 25 degrees) the navigation mesh reaches", nav, nav.map_or("not measured".into(), |c| format!("{:.1}%", c * 100.0)), "at least 90%", nav.map(|c| c >= 0.90), "explorer bots for stuck points; obstacles (trunks, rocks) in the bake");
    // Performance
    let fps_v = f(fps, "/fps/fps");
    add("Performance", "p_fps", "Frame rate in the engine (RTX 2070 SUPER, 1280x720, eye height)", fps_v, fps_v.map_or("not measured".into(), |x| format!("{x:.1} fps")), "at least 60 fps", fps_v.map(|x| x >= 60.0), "LOD chains and a triangle budget (decimate node, then import with LODs)");
    let prims = f(fps, "/fps/primitives");
    add("Performance", "p_tris", "Triangles in one frame", prims, prims.map_or("not measured".into(), fmt_big), "at most 20 million", prims.map(|x| x <= 2.0e7), "LOD chains and a triangle budget (decimate node, then import with LODs)");
    let dc = f(fps, "/fps/draw_calls");
    add("Performance", "p_draw", "Draw calls in one frame", dc, dc.map_or("not measured".into(), |x| format!("{x:.0}")), "at most 3000", dc.map(|x| x <= 3000.0), "merge small props, GPU instancing");
    let vram = f(fps, "/fps/video_mem_mb");
    add("Performance", "p_vram", "Video memory", vram, vram.map_or("not measured".into(), |x| format!("{x:.0} MB")), "at most 6000 MB", vram.map(|x| x <= 6000.0), "lower texture resolution on distant assets, atlas props");
    // Content breadth
    let per_km2 = if audit.area_km2 > 0.0 { variants / audit.area_km2 } else { 0.0 };
    add("Content breadth", "b_unique", "Distinct prototypes per square kilometre", Some(per_km2), format!("{per_km2:.1}"), "at least 15", Some(per_km2 >= 15.0), "more generator families");
    add("Content breadth", "b_repeat", "Most copies of one large prototype (over 2 m, standing 1.5 m or taller) in a 50 m square", Some(audit.max_local_repeats as f64), audit.max_local_repeats.to_string(), "at most 12", Some(audit.max_local_repeats <= 12), "more tree and rock variants (generator families), scale and tint variation");
    let lod = if audit.heavy_protos == 0 { 1.0 } else { audit.heavy_with_lod as f64 / audit.heavy_protos as f64 };
    add("Content breadth", "b_lod", "Heavy meshes (over 2000 triangles) that have an LOD chain", Some(lod), format!("{:.0}% ({} of {})", lod * 100.0, audit.heavy_with_lod, audit.heavy_protos), "100%", Some(lod >= 1.0), "decimate node and LOD generation on import");
    // Delivery
    let verr = f(validator, "/errors");
    let v_missing = if validator.get("timed_out").is_some() { "not finished in the time limit" } else { "validator missing" };
    add("Delivery", "d_validator", "glTF validator errors", verr, verr.map_or(v_missing.into(), |e| format!("{e:.0} errors, {} warnings", f(validator, "/warnings").unwrap_or(0.0))), "0 errors", verr.map(|e| e == 0.0), "fix the exporter");
    let gerr = godot.get("errors").and_then(Value::as_array).map(Vec::len);
    let gok = godot.get("ok").and_then(Value::as_bool);
    add("Delivery", "d_engine", "Engine import errors", gerr.map(|e| e as f64), gok.map_or("engine missing".into(), |ok| if ok { "imported cleanly".into() } else { "import failed".into() }), "0 errors", gok, "fix the exporter");
    let bare = engine.get("untextured").and_then(Value::as_array).map(Vec::len);
    add("Delivery", "d_textured", "Materials in the engine file with no texture", bare.map(|b| b as f64), bare.map_or("not measured".into(), |b| if b == 0 { "none".to_string() } else { format!("{b}: {}", engine["untextured"].as_array().map_or(String::new(), |a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))) }), "0", bare.map(|b| b == 0), "bake layered materials to textures for the engine export");
    let (ok, total) = traced;
    add("Delivery", "d_ledger", "External files with a licence ledger row", Some(if total == 0 { 1.0 } else { ok as f64 / total as f64 }), format!("{ok} of {total}"), "100%", Some(ok == total), "fetch through `pt corpus fetch` so each file gets a row");
    let share = if audit.unique_triangles > 0 { audit.external_unique_triangles as f64 / audit.unique_triangles as f64 } else { 0.0 };
    add("Delivery", "d_cc0_share", "Unique triangles that come from CC0 files (the rest are ours)", Some(share), format!("{:.1}%", share * 100.0), "reported (falls as generators replace assets)", None, "generator families replace CC0 assets (B15a)");
    m
}

fn page(title: &str, subtitle: &str, ms: &[Measure], frames: &[PathBuf], audit: &Audit, notes: &str, baseline: Option<&Value>, judge: &Value, fragment: bool) -> String {
    use crate::report::{esc, img_tag, CSS};
    let (pass, total) = (ms.iter().filter(|m| m.pass == Some(true)).count(), ms.iter().filter(|m| m.pass.is_some()).count());
    let fails: Vec<&Measure> = ms.iter().filter(|m| m.pass == Some(false)).collect();
    let mut html = format!("<main>\n<h1>{}</h1>\n<p class=\"sub\">{}</p>\n", esc(title), esc(subtitle));
    let state = if fails.is_empty() { "green" } else if pass * 2 >= total { "amber" } else { "red" };
    let was = baseline.map(|b| format!(" (the baseline build passed {} of {})", b["pass"], b["total"])).unwrap_or_default();
    html.push_str(&format!("<div class=\"card\"><span class=\"light {state}\"></span><b>{pass} of {total} measures pass{was}.</b> "));
    if fails.is_empty() {
        html.push_str("Every measure is inside its bar.");
    } else {
        let mut routes: Vec<&str> = fails.iter().map(|m| m.route).collect();
        routes.dedup();
        html.push_str(&format!("Next: {}.", esc(&routes.iter().take(3).copied().collect::<Vec<_>>().join("; "))));
    }
    html.push_str("</div>\n");
    if !notes.is_empty() {
        html.push_str(&format!("<div class=\"card\">{}</div>\n", esc(notes)));
    }
    html.push_str("<h2>The frames</h2>\n<div class=\"gallery\">\n");
    for f in frames {
        // The fragment (what gets published) LINKS the frames by file name: four full-size frames embedded as text are over the
        // 16 MB a published page may be. The standalone page embeds them, so it can be opened from anywhere.
        let name = f.file_name().map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let tag = if fragment { format!("<img alt=\"frame\" style=\"width:100%\" src=\"{}\">", esc(&name)) } else { img_tag(f, "frame", "100%") };
        html.push_str(&format!("<figure class=\"pair\">{tag}<figcaption>{}</figcaption></figure>\n", esc(&f.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned()))));
    }
    html.push_str("</div>\n");
    if let Some(rows) = judge.get("frames").and_then(Value::as_array).filter(|r| !r.is_empty()) {
        html.push_str(&format!("<h2>What the judge saw</h2>\n<p class=\"sub\">Judge {} read each frame against the brief: \"{}\". It also had to choose between the brief and {} other worlds, in both orders.</p>\n<div class=\"tablewrap\"><table>\n<tr><th>Frame</th><th class=\"n\">Fit (0 to 10)</th><th>In its own words</th></tr>\n", esc(judge["judge"].as_str().unwrap_or("?")), esc(judge["brief"].as_str().unwrap_or("")), judge["foils"].as_array().map_or(0, Vec::len)));
        for r in rows {
            let name = Path::new(r["file"].as_str().unwrap_or("")).file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
            html.push_str(&format!("<tr><td>{}</td><td class=\"n\">{}</td><td>{}</td></tr>\n", esc(&name), r["fit"], esc(r["seen"].as_str().unwrap_or(""))));
        }
        html.push_str("</table></div>\n");
    }
    html.push_str(&format!("<h2>Measures</h2>\n<p class=\"sub\">Bars are drafts from the plan (B13.1) until they are calibrated against reference scenes.</p>\n<div class=\"tablewrap\"><table>\n<tr><th>Family</th><th>Measure</th><th class=\"n\">Result</th><th>Bar</th><th></th>{}</tr>\n", if baseline.is_some() { "<th>Baseline build</th>" } else { "" }));
    for m in ms {
        let (cls, word) = match m.pass {
            Some(true) => ("green", "pass"),
            Some(false) => ("red", "fail"),
            None => ("amber", "info"),
        };
        let was_cell = baseline.map_or(String::new(), |b| {
            let old = b["measures"].as_array().and_then(|a| a.iter().find(|x| x["id"] == m.id));
            format!("<td><small>{}</small></td>", old.map_or("not measured then".to_string(), |o| format!("{} ({})", esc(o["shown"].as_str().unwrap_or("")), o["status"].as_str().unwrap_or(""))))
        });
        html.push_str(&format!("<tr><td>{}</td><td>{}{}</td><td class=\"n\">{}</td><td>{}</td><td><span class=\"chip {cls}\">{word}</span></td>{was_cell}</tr>\n", esc(m.family), esc(m.label), if m.pass == Some(false) { format!("<br><small>route: {}</small>", esc(m.route)) } else { String::new() }, esc(&m.shown), esc(&m.bar)));
    }
    html.push_str("</table></div>\n<h2>What is in the world</h2>\n<div class=\"tablewrap\"><table>\n<tr><th>Prototype source</th><th class=\"n\">Triangles each</th><th class=\"n\">Copies</th><th class=\"n\">Triangles in total</th><th>LOD levels in the engine export (triangles)</th></tr>\n");
    html.push_str(&format!("<tr><td>terrain and procedural geometry</td><td class=\"n\">{}</td><td class=\"n\">1</td><td class=\"n\">{}</td><td>single level</td></tr>\n", audit.base_triangles, audit.base_triangles));
    for p in &audit.protos {
        let (t, c) = (p["triangles"].as_u64().unwrap_or(0), p["instances"].as_u64().unwrap_or(0));
        let lods = p["lod_triangles"].as_array().map_or("single level".to_string(), |l| l.iter().filter_map(Value::as_u64).map(|x| x.to_string()).collect::<Vec<_>>().join(" / "));
        html.push_str(&format!("<tr><td>{}</td><td class=\"n\">{t}</td><td class=\"n\">{c}</td><td class=\"n\">{}</td><td>{}</td></tr>\n", esc(p["source"].as_str().unwrap_or("ours (procedural)")), t * c, esc(&lods)));
    }
    html.push_str(&format!("</table></div>\n<p class=\"sub\">Total {} triangles instanced, {} unique.</p>\n</main>", fmt_big(audit.total_triangles as f64), fmt_big(audit.unique_triangles as f64)));
    if fragment {
        // title, style and body content only: what the Artifact publisher wraps in its own document
        return format!("<title>{}</title>\n<style>{CSS}</style>\n{html}\n", esc(title));
    }
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{CSS}</style></head><body>{html}</body></html>\n", esc(title))
}

/// `pt scorecard <recipe> <scene.json> --out DIR [--no-render] [--no-fps] [--quick] [--samples N] [--notes TEXT]`
pub fn scorecard_cmd(args: &[String]) -> Result<(), String> {
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let flag_val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1));
    let (Some(recipe), Some(scene_path)) = (pos.first(), pos.get(1)) else {
        return Err("usage: pt scorecard <recipe.hcs> <scene.json> --out DIR [--no-render] [--no-fps] [--quick] [--samples N] [--notes TEXT] [--reuse] [--bake] [--judge]".into());
    };
    let out_dir = PathBuf::from(flag_val("--out").ok_or("--out DIR is required")?);
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let t_all = Instant::now();

    // 1. cook
    let code = std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?;
    let t = Instant::now();
    let mut g = Graph::new();
    let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let geo = g.cook(rep.outputs.first().ok_or("recipe has no out()")?)?;
    let cook_s = t.elapsed().as_secs_f64();
    drop(g); // the graph keeps every node's result; only the cooked world is needed from here on, and Blender needs the memory

    // 2. scene (style folded in, layers injected) and audits
    let scene_text = std::fs::read_to_string(scene_path.as_str()).map_err(|e| format!("{scene_path}: {e}"))?;
    let mut scene: Value = serde_json::from_str(scene_text.trim_start_matches('\u{feff}')).map_err(|e| format!("{scene_path}: {e}"))?;
    let opts = blender::Opts { quick: args.iter().any(|a| a == "--quick"), samples: flag_val("--samples").and_then(|s| s.parse().ok()), keep: Some(out_dir.clone()), lod: false, only: None, lookdev: false };
    blender::resolve_style(&mut scene)?;
    blender::inject_layers(&mut scene, &geo);
    let root = blender::repo_root();
    let lib: Value = std::fs::read_to_string(root.join("corpus/materials/library.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({"materials": {}}));
    let mut a = audit(&geo);
    a.placement = crate::grounding::view_counts(&geo, &scene, 45.0);
    let ledger = crate::corpus::Ledger::load(&root.join("corpus/LEDGER.csv"))?;
    let (traced_ok, traced_total, missing) = ledger_trace(&a, &scene, &lib, &geo, &ledger);

    // 3. export (and render)
    let render_glb = out_dir.join("world.glb"); // full detail: what the offline render sees
    let glb = out_dir.join("world.engine.glb"); // what an engine gets: levels of detail, capped per level
    let frames_base = out_dir.join("frame.png");
    let mut render = json!({});
    // `--reuse`: a run that was cut off (a closed session, a crash in a later stage) keeps what it finished. The frames and the
    // bake are the long stages; each is reused when its report and every file it names are still in the output folder.
    let reuse = args.iter().any(|a| a == "--reuse");
    let kept_render = if reuse { finished_stage(&out_dir.join("world.report.json"), "/frames", "file") } else { None };
    if args.iter().any(|a| a == "--no-render") {
        let t = Instant::now();
        std::fs::write(&render_glb, pt_core::gltf::to_glb(&geo)).map_err(|e| e.to_string())?;
        render["export_s"] = json!(t.elapsed().as_secs_f64());
    } else if let (Some(r), true) = (kept_render, render_glb.exists()) {
        render = r;
        render["reused"] = json!(true);
    } else {
        render = blender::render_geometry(&geo, "world", scene.clone(), &frames_base, &opts)?;
    }
    // layered materials (terrain blends) have no portable form: bake them to albedo and normal textures for the engine file
    let mut engine_geo: Geometry = (*geo).clone();
    // the layer weights are what the baked textures replace: an engine has no use for them, and they are a third of the terrain's bytes
    engine_geo.point.remove("Wt0");
    engine_geo.point.remove("Wt1");
    style_records(&mut engine_geo, &scene, &lib, &crate::corpus::root());
    if let Some(inst) = engine_geo.instances.as_mut() {
        for p in inst.protos.iter_mut() {
            let plain = p.material_names().iter().any(|n| !p.detail.contains_key(&format!("{}{n}", pt_core::gltf::ASSET_MATERIAL_PREFIX)));
            if plain {
                let mut q = (**p).clone();
                style_records(&mut q, &scene, &lib, &crate::corpus::root());
                *p = std::sync::Arc::new(q);
            }
        }
    }
    let mut baked_report = json!({});
    if args.iter().any(|a| a == "--bake") && !blender::layers_of(&geo).is_empty() {
        let t = Instant::now();
        let kept_bake = if reuse { finished_stage(&out_dir.join("bake.report.json"), "/baked", "albedo") } else { None };
        let baked: BTreeMap<String, Value> = match kept_bake.and_then(|r| r.get("baked").and_then(Value::as_object).cloned()) {
            Some(m) => m.into_iter().collect(),
            None => blender::bake_layers(&render_glb, scene.clone(), &geo, &out_dir, 4096)?,
        };
        for (name, b) in &baked {
            engine_geo.detail.insert(format!("baked:{name}"), json!({"albedo": b["albedo"], "normal": b["normal"]}));
        }
        baked_report = json!({"materials": baked, "seconds": t.elapsed().as_secs_f64()});
    }
    let t = Instant::now();
    let engine_lod = !args.iter().any(|a| a == "--no-lod");
    let bytes = if engine_lod {
        // tiled: one instanced node per ground tile, prototype and level, so the engine imports a forest in seconds (a node per
        // instance and level stopped Godot's importer at a quarter of a million nodes)
        pt_core::gltf::to_glb_tiled(&engine_geo, &pt_core::lod::LodPolicy { cache_dir: Some(root.join("out/lod_cache")), ..pt_core::lod::LodPolicy::engine() })
    } else {
        pt_core::gltf::to_glb(&engine_geo)
    };
    std::fs::write(&glb, bytes).map_err(|e| e.to_string())?;
    let engine_export_s = t.elapsed().as_secs_f64();
    let lod_report = pt_core::gltf::read_json(&glb).ok().and_then(|j| j.pointer("/asset/extras/proteus_lod").cloned()).unwrap_or(json!([]));
    apply_lod_report(&mut a, &lod_report);
    let mb = |p: &Path| std::fs::metadata(p).map_or(0.0, |m| m.len() as f64 / 1048576.0);
    let (glb_mb, render_glb_mb) = (mb(&glb), mb(&render_glb));

    // 4. referees (the engine and the validator see the engine export)
    let validator = run_validator(&glb);
    // the navigation mesh is baked and measured over the playable window (scene "playable": {"size": [w, d]}, centred on the origin) when the
    // world has a far ring; otherwise over the whole terrain
    let nav_box = scene.pointer("/playable/size").and_then(Value::as_array).filter(|s| s.len() == 2).map(|s| {
        let (hw, hd) = (s[0].as_f64().unwrap_or(0.0) / 2.0, s[1].as_f64().unwrap_or(0.0) / 2.0);
        format!("nav_box={},{},{},{}", -hw, -hd, hw, hd)
    });
    let mut nav_args: Vec<&str> = vec!["nav=1"];
    if let Some(b) = &nav_box {
        nav_args.push(b);
    }
    // which materials of the base mesh are ground to walk on (scene "playable": {"materials": [...]}); the base mesh also carries
    // water, a merged distant forest and buildings
    let nav_mats = scene.pointer("/playable/materials").and_then(Value::as_array).map(|a| format!("nav_mats={}", a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(",")));
    if let Some(m) = &nav_mats {
        nav_args.push(m);
    }
    let godot = run_godot(&glb, &nav_args, false);
    // the engine view stands where the scene's first eye-height camera stands, so its picture and frame rate are comparable to the render
    let eye_cam = scene.get("cameras").and_then(Value::as_array).into_iter().flatten().chain(scene.get("camera")).find(|c| c.get("eye").is_some()).cloned();
    let engine_shot = out_dir.join("engine_eye.png");
    let mut fps_args: Vec<String> = vec!["fps=1".into(), "frames=60".into(), format!("shot={}", absolute(&engine_shot).to_string_lossy().replace('\\', "/"))];
    if let Some(c) = &eye_cam {
        let at = c.pointer("/eye/at").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect::<Vec<_>>()).unwrap_or_default();
        if at.len() == 2 {
            fps_args.push(format!("eye_at={},{}", at[0], at[1]));
            fps_args.push(format!("height={}", c.pointer("/eye/height").and_then(Value::as_f64).unwrap_or(1.7)));
            fps_args.push(format!("yaw={}", c.pointer("/eye/yaw").and_then(Value::as_f64).unwrap_or(0.0)));
            fps_args.push(format!("pitch={}", c.pointer("/eye/pitch").and_then(Value::as_f64).unwrap_or(0.0)));
            fps_args.push(format!("lens={}", c.get("lens").and_then(Value::as_f64).unwrap_or(28.0)));
        }
    }
    let fps_refs: Vec<&str> = fps_args.iter().map(String::as_str).collect();
    let fps = if args.iter().any(|a| a == "--no-fps") { json!({}) } else { run_godot(&glb, &fps_refs, true) };

    // 5. the vision judge reads the rendered frames (last, so it never shares the GPU with the frame-rate measurement)
    let frame_files: Vec<PathBuf> = render.get("frames").and_then(Value::as_array).into_iter().flatten().filter_map(|f| f.get("file").and_then(Value::as_str).map(PathBuf::from)).collect();
    let brief = flag_val("--brief").cloned().or_else(|| scene.get("brief").and_then(Value::as_str).map(String::from));
    let mut foils: Vec<String> = args.windows(2).filter(|w| w[0] == "--foil").map(|w| w[1].clone()).collect();
    if foils.is_empty() {
        foils = scene.get("foils").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
    }
    if foils.is_empty() {
        foils = crate::judge_bench::DEFAULT_FOILS.iter().map(|s| s.to_string()).collect();
    }
    let judge_report = match (&brief, !args.iter().any(|a| a == "--judge") || args.iter().any(|a| a == "--no-render"), frame_files.is_empty()) {
        (Some(b), false, false) => {
            let t = Instant::now();
            match crate::judge_bench::judge_frames(&crate::judge::Judge::local(), &frame_files, b, &foils) {
                Ok(mut r) => {
                    r["seconds"] = json!(t.elapsed().as_secs_f64());
                    r
                }
                Err(e) => json!({"error": e}),
            }
        }
        _ => json!({}),
    };

    // 6. measures, JSON, page
    let engine_doc = pt_core::gltf::read_json(&glb).unwrap_or(json!({}));
    let engine = engine_materials(&engine_doc);
    let ms = measures(&a, &render, &godot, &fps, &validator, &engine, (traced_ok, traced_total), &scene, &judge_report);
    let mut frames: Vec<PathBuf> = render.get("frames").and_then(Value::as_array).into_iter().flatten().filter_map(|f| f.get("file").and_then(Value::as_str).map(PathBuf::from)).collect();
    if engine_shot.exists() {
        frames.push(engine_shot.clone()); // what the engine makes of the delivered file, next to the render
    }
    let style = render.get("style").and_then(Value::as_str).or_else(|| scene.get("style_name").and_then(Value::as_str)).unwrap_or("no style");
    let title = "Readiness Scorecard".to_string();
    let subtitle = format!("Style {style} · {recipe} with {scene_path} · measured by Blender (look), Godot (engine) and the glTF validator (file) · {}", crate::corpus::now_iso());
    let notes = flag_val("--notes").map_or("", |s| s.as_str());
    let baseline: Option<Value> = flag_val("--baseline").and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok());
    std::fs::write(out_dir.join("scorecard.html"), page(&title, &subtitle, &ms, &frames, &a, notes, baseline.as_ref(), &judge_report, false)).map_err(|e| e.to_string())?;
    std::fs::write(out_dir.join("scorecard_fragment.html"), page(&title, &subtitle, &ms, &frames, &a, notes, baseline.as_ref(), &judge_report, true)).map_err(|e| e.to_string())?;
    let (pass, total) = (ms.iter().filter(|m| m.pass == Some(true)).count(), ms.iter().filter(|m| m.pass.is_some()).count());
    let summary = json!({
        "title": title, "recipe": recipe, "scene": scene_path, "style": style, "when": crate::corpus::now_iso(),
        "pass": pass, "total": total, "measures": ms.iter().map(Measure::to_json).collect::<Vec<_>>(),
        "audit": {"base_triangles": a.base_triangles, "instances": a.instances, "total_triangles": a.total_triangles, "unique_triangles": a.unique_triangles,
                  "external_triangles": a.external_triangles, "external_unique_triangles": a.external_unique_triangles, "max_repeats": a.max_repeats, "max_local_repeats": a.max_local_repeats, "area_km2": a.area_km2, "protos": a.protos, "ledger_missing": missing},
        "render": render, "judge": judge_report, "godot": godot, "fps": fps, "validator": validator, "baked": baked_report, "engine_materials": engine,
        "timings_s": {"cook": cook_s, "total": t_all.elapsed().as_secs_f64()}, "glb_mb": {"engine": glb_mb, "render": render_glb_mb}, "engine_export_s": engine_export_s, "engine_lod": engine_lod,
    });
    std::fs::write(out_dir.join("scorecard.json"), serde_json::to_string_pretty(&summary).unwrap_or_default()).map_err(|e| e.to_string())?;
    println!("scorecard: {pass} of {total} measures pass -> {}", out_dir.join("scorecard.html").display());
    for m in &ms {
        println!("  {:<6} {:<15} {:<52} {}", match m.pass { Some(true) => "pass", Some(false) => "FAIL", None => "info" }, m.family, m.label, m.shown);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(n: usize) -> Geometry {
        let pts: Vec<[f32; 3]> = (0..n + 2).map(|i| [i as f32, (i % 2) as f32, 0.0]).collect();
        let polys: Vec<Vec<u32>> = (0..n).map(|i| vec![i as u32, i as u32 + 1, i as u32 + 2]).collect();
        Geometry::from_polys(pts, &polys)
    }

    #[test]
    fn audit_counts_instanced_triangles_repeats_and_lod_need() {
        let mut proto = tri(3000);
        for q in proto.p_mut() {
            q[1] *= 2.0; // 2 m tall: it stands up, so it counts as a large prototype
        }
        proto.detail.insert("material:tree__bark".into(), json!({"source": "polyhaven/models/x/x_1k.gltf", "index": 0}));
        let mut world = tri(10);
        world.instances = Some(pt_core::Instances { protos: vec![std::sync::Arc::new(proto), std::sync::Arc::new(tri(100))], proto_id: std::sync::Arc::new(vec![0, 0, 1]), xf: std::sync::Arc::new(vec![pt_core::math::IDENTITY; 3]), attrs: Default::default() });
        let a = audit(&world);
        assert_eq!((a.base_triangles, a.instances, a.max_repeats), (10, 3, 2));
        assert_eq!(a.max_local_repeats, 2, "the 3000-triangle strip is over 2 m long and sits twice in one cell");
        assert_eq!(a.total_triangles, 10 + 3000 * 2 + 100);
        assert_eq!(a.unique_triangles, 10 + 3000 + 100);
        assert_eq!(a.external_triangles, 6000, "only the prototype with a recorded source counts as CC0");
        assert_eq!((a.heavy_protos, a.heavy_with_lod), (1, 0));
        assert!(a.assets.contains("polyhaven/models/x/x_1k.gltf"));
    }

    #[test]
    fn measures_pass_and_fail_on_their_bars() {
        let a = Audit { base_triangles: 10, total_triangles: 1000, unique_triangles: 100, max_repeats: 461, max_local_repeats: 9, heavy_protos: 2, heavy_with_lod: 0, area_km2: 0.81, protos: vec![json!({}); 16], ..Default::default() };
        let render = json!({"material_coverage": 1.0, "unresolved": [], "unknown_materials": [], "style": "alpine"});
        let godot = json!({"ok": true, "errors": [], "navmesh": {"coverage": 0.745, "coverage_intended": 0.954}});
        let fps = json!({"fps": {"fps": 2.1, "primitives": 1.4e9, "draw_calls": 336, "video_mem_mb": 380.0}});
        let validator = json!({"available": true, "errors": 0, "warnings": 19});
        let scene = json!({"lighting": {"sky": "nishita", "fog_density": 0.0002}});
        let engine = json!({"untextured": []});
        let ms = measures(&a, &render, &godot, &fps, &validator, &engine, (5, 5), &scene, &json!({}));
        let get = |id: &str| ms.iter().find(|m| m.id == id).unwrap().pass;
        for ok in ["v_coverage", "v_atmosphere", "a_style", "a_variety", "w_nav", "p_draw", "p_vram", "b_unique", "b_repeat", "d_validator", "d_engine", "d_ledger"] {
            assert_eq!(get(ok), Some(true), "{ok}");
        }
        for bad in ["p_fps", "p_tris", "b_lod"] {
            assert_eq!(get(bad), Some(false), "{bad}");
        }
        assert_eq!(get("d_cc0_share"), None, "informational");
        for unjudged in ["v_brief_fit", "v_brief_vs_foils", "v_judge_sane"] {
            assert_eq!(get(unjudged), None, "no judge ran, so {unjudged} is not invented");
        }
        // nothing measured -> no pass or fail is invented
        let none = measures(&a, &json!({}), &json!({"available": false}), &json!({}), &json!({"available": false}), &json!({}), (0, 0), &json!({}), &json!({}));
        assert!(none.iter().find(|m| m.id == "p_fps").unwrap().pass.is_none() && none.iter().find(|m| m.id == "v_coverage").unwrap().pass.is_none());
    }

    #[test]
    fn judge_rows_pass_on_a_good_report_and_fail_on_a_weak_or_unreliable_one() {
        let a = Audit::default();
        let run = |judge: Value| measures(&a, &json!({}), &json!({}), &json!({}), &json!({}), &json!({}), (0, 0), &json!({}), &judge);
        let good = json!({"frames": [{"file": "a.png", "fit": 8, "seen": "x"}], "mean_fit": 8.0, "brief_picked": {"count": 9, "of": 9}, "contradictions": 0, "control_blank_fit": 0.0});
        let ms = run(good);
        assert!(["v_brief_fit", "v_brief_vs_foils", "v_judge_sane"].iter().all(|id| ms.iter().find(|m| m.id == *id).unwrap().pass == Some(true)));
        // a world that reads as something else, from a judge that flips with the order and rates a blank frame highly
        let bad = json!({"frames": [{"file": "a.png", "fit": 3, "seen": "x"}], "mean_fit": 3.0, "brief_picked": {"count": 4, "of": 9}, "contradictions": 4, "control_blank_fit": 6.0});
        let ms = run(bad);
        assert!(["v_brief_fit", "v_brief_vs_foils", "v_judge_sane"].iter().all(|id| ms.iter().find(|m| m.id == *id).unwrap().pass == Some(false)));
    }

    #[test]
    fn page_leads_with_the_result_and_the_route() {
        let a = Audit::default();
        let ms = measures(&a, &json!({"material_coverage": 1.0}), &json!({}), &json!({"fps": {"fps": 2.0}}), &json!({}), &json!({}), (1, 1), &json!({}), &json!({}));
        let judge = json!({"judge": "j", "brief": "a valley", "foils": ["x"], "frames": [{"file": "d/frame_eye.png", "fit": 8, "seen": "a green valley"}]});
        let html = page("T", "s", &ms, &[], &a, "", None, &judge, false);
        assert!(html.contains("measures pass.") && html.contains("Next:") && html.contains("route:"), "{}", &html[..400.min(html.len())]);
        assert!(html.contains("What the judge saw") && html.contains("a green valley") && html.contains("frame_eye"));
        assert!(!page("T", "s", &ms, &[], &a, "", None, &json!({}), false).contains("What the judge saw"));
    }
}
