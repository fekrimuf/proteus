//! `pt render-scene`: cook a recipe (or read a glTF), check the scene description, and render it with Blender Cycles.
//!
//! Blender is only ever started as a separate program (`corpus/tools/render_scene.py`); the recipe's material
//! names reach it through the glTF (`set_material`) and the scene file says the rest (sky, camera, quality).
//! The scene file is checked here first so a typo costs milliseconds, not a render.

use pt_core::Geometry;
use pt_graph::Graph;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DEFAULT_BLENDER: &str = r"D:\Fekri\tools\blender\blender-4.5.14-windows-x64\blender.exe";

pub fn blender_exe() -> PathBuf {
    std::env::var("PT_BLENDER").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(DEFAULT_BLENDER))
}

/// The repo root: `PT_ROOT`, else the nearest ancestor of the working directory or the executable that has `corpus/tools/render_scene.py`.
pub fn repo_root() -> PathBuf {
    if let Ok(r) = std::env::var("PT_ROOT") {
        return PathBuf::from(r);
    }
    let mut starts = vec![std::env::current_dir().unwrap_or_default()];
    if let Ok(e) = std::env::current_exe() {
        starts.push(e);
    }
    for s in starts {
        for a in s.ancestors() {
            if a.join("corpus/tools/render_scene.py").exists() {
                return a.to_path_buf();
            }
        }
    }
    PathBuf::from(".")
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn vec_n(v: &Value, n: usize) -> bool {
    v.as_array().is_some_and(|a| a.len() == n && a.iter().all(|x| x.is_number()))
}

fn unknown_keys(o: &serde_json::Map<String, Value>, known: &[&str], section: &str, out: &mut Vec<String>) {
    for k in o.keys() {
        if !known.contains(&k.as_str()) {
            out.push(format!("{section}: unknown key '{k}' (known: {})", known.join(", ")));
        }
    }
}

fn range(o: &Value, key: &str, lo: f64, hi: f64, section: &str, out: &mut Vec<String>) {
    if let Some(v) = o.get(key) {
        match num(v) {
            Some(x) if x >= lo && x <= hi => {}
            _ => out.push(format!("{section}.{key} must be a number in [{lo}, {hi}], got {v}")),
        }
    }
}

/// Problems in a scene description, each with its fix; empty = good. Unknown keys are reported too (typos).
pub fn check_scene(s: &Value) -> Vec<String> {
    let mut e = Vec::new();
    let Some(top) = s.as_object() else { return vec!["scene file must be a JSON object".into()] };
    unknown_keys(top, &["version", "render", "lighting", "camera", "cameras", "ground", "materials", "default_material", "style", "style_name", "note", "brief", "foils", "playable", "knobs"], "scene", &mut e);
    if let Some(r) = s.get("render") {
        if let Some(o) = r.as_object() {
            unknown_keys(o, &["samples", "resolution", "device", "seed", "denoise", "noise_threshold", "clamp_indirect", "bounces", "transparent_bounces", "volume_bounces", "view_transform", "look", "exposure", "white_balance", "glare", "glare_threshold", "glare_size", "foliage_translucency", "smooth_angle"], "render", &mut e);
        }
        range(r, "samples", 1.0, 4096.0, "render", &mut e);
        range(r, "exposure", -10.0, 10.0, "render", &mut e);
        range(r, "smooth_angle", 0.0, 180.0, "render", &mut e);
        if let Some(res) = r.get("resolution") {
            let ok = vec_n(res, 2) && res.as_array().is_some_and(|a| a.iter().all(|x| x.as_f64().is_some_and(|v| (64.0..=8192.0).contains(&v))));
            if !ok {
                e.push(format!("render.resolution must be [width, height] with each in 64..8192, got {res}"));
            }
        }
        if let Some(d) = r.get("device").and_then(Value::as_str) {
            if !["auto", "cpu", "gpu"].contains(&d) {
                e.push(format!("render.device must be auto, cpu or gpu, got '{d}'"));
            }
        }
    }
    if let Some(l) = s.get("lighting") {
        if let Some(o) = l.as_object() {
            unknown_keys(o, &["sky", "strength", "sun_elevation", "sun_azimuth", "sun_size", "sun_intensity", "air", "dust", "ozone", "altitude", "hdri", "rotation", "color", "fog_density", "fog_height", "fog_extent", "fog_anisotropy", "fog_color", "fog_base", "mist_density", "mist_height", "mist_extent", "mist_step"], "lighting", &mut e);
        }
        match l.get("sky").and_then(Value::as_str).unwrap_or("nishita") {
            "nishita" | "flat" => {}
            "hdri" => {
                if l.get("hdri").and_then(Value::as_str).is_none() {
                    e.push("lighting.sky is 'hdri' but lighting.hdri (a path under the corpus root, or absolute) is missing".into());
                }
            }
            o => e.push(format!("lighting.sky must be nishita, hdri or flat, got '{o}'")),
        }
        range(l, "sun_elevation", -10.0, 90.0, "lighting", &mut e);
        range(l, "sun_azimuth", -3600.0, 3600.0, "lighting", &mut e);
        range(l, "fog_density", 0.0, 10.0, "lighting", &mut e);
        range(l, "mist_density", 0.0, 10.0, "lighting", &mut e);
        range(l, "mist_height", 0.5, 5000.0, "lighting", &mut e);
        range(l, "fog_height", 1.0, 100000.0, "lighting", &mut e);
        range(l, "strength", 0.0, 1000.0, "lighting", &mut e);
    }
    let mut cams: Vec<(&str, &Value)> = Vec::new();
    if let Some(c) = s.get("camera") {
        cams.push(("camera", c));
    }
    if let Some(a) = s.get("cameras").and_then(Value::as_array) {
        cams.extend(a.iter().map(|c| ("cameras[]", c)));
        if s.get("camera").is_some() {
            e.push("give either `camera` or `cameras`, not both".into());
        }
    }
    for (sec, c) in cams {
        if let Some(o) = c.as_object() {
            unknown_keys(o, &["name", "location", "look_at", "eye", "lens", "sensor", "clip_end", "dof"], sec, &mut e);
        }
        range(c, "lens", 8.0, 300.0, sec, &mut e);
        if c.get("location").is_some_and(|v| !vec_n(v, 3)) || c.get("look_at").is_some_and(|v| !vec_n(v, 3)) {
            e.push(format!("{sec}: location and look_at must be [x, y, z] (metres, Y up)"));
        }
        if c.get("location").is_some() && c.get("eye").is_some() {
            e.push(format!("{sec}: give either location+look_at or eye, not both"));
        }
        if let Some(eye) = c.get("eye") {
            if !eye.get("at").is_some_and(|v| vec_n(v, 2)) {
                e.push(format!("{sec}.eye.at must be [x, z] (the camera stands on whatever the model has there)"));
            }
        }
    }
    if let Some(m) = s.get("materials") {
        if !m.as_object().is_some_and(|o| o.values().all(Value::is_object)) {
            e.push("materials must be an object of objects: {\"rock_cliff\": {\"tint\": [r,g,b]}}".into());
        }
    }
    e
}

/// The knobs a scene gives its recipe: the style's `knobs`, overridden key by key by the scene's own. A recipe reads them with
/// `knob(name, default)`; every command that cooks a scene (render, ground-check, pick, scorecard) sets them on the graph first,
/// so all of them see the same world.
pub fn knobs_of(scene: &Value) -> Result<BTreeMap<String, Value>, String> {
    let mut s = scene.clone();
    resolve_style(&mut s)?;
    let mut knobs: BTreeMap<String, Value> = s.get("knobs").and_then(Value::as_object).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    // the standing cameras go to the recipe as [[x, z, eye_height], ...], so placement keeps objects out of arm's reach of every
    // lens (`knob("cameras", [])` and the `seat` node's `cameras`); a style or world variant may set the knob itself
    let cams = crate::grounding::cameras_of(&s);
    if !cams.is_empty() {
        knobs.entry("cameras".to_string()).or_insert_with(|| json!(cams.iter().map(|c| json!([c.x, c.z, c.eye_height])).collect::<Vec<_>>()));
    }
    Ok(knobs)
}

/// Layered materials (`blend_materials`): material name -> its layer roles, from the geometry and its instance prototypes.
pub fn layers_of(geo: &Geometry) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out = std::collections::BTreeMap::new();
    let mut take = |g: &Geometry| {
        for (k, v) in &g.detail {
            if let (Some(name), Some(arr)) = (k.strip_prefix("layers:"), v.as_array()) {
                out.insert(name.to_string(), arr.iter().filter_map(|x| x.as_str().map(String::from)).collect());
            }
        }
    };
    take(geo);
    for p in geo.instances.iter().flat_map(|i| i.protos.iter()) {
        take(&p.realize());
    }
    out
}

/// Tell the render which roles each layered material mixes (the scene file may already say, and then wins).
pub fn inject_layers(scene: &mut Value, geo: &Geometry) {
    inject_layers_from(scene, &layers_of(geo));
}

fn inject_layers_from(scene: &mut Value, layers: &std::collections::BTreeMap<String, Vec<String>>) {
    for (name, roles) in layers {
        let entry = &mut scene["materials"][&name];
        if !entry.is_object() {
            *entry = json!({});
        }
        if entry.get("layers").is_none() {
            entry["layers"] = json!(roles);
        }
    }
}

/// What a render needs to know about a cooked world besides its model file: the layer roles of layered materials, every material
/// name it uses, and the names authored inside imported assets. Kept next to a cached model so the world is not cooked again.
#[derive(Clone, Debug, Default)]
pub struct WorldFacts {
    pub layers: std::collections::BTreeMap<String, Vec<String>>,
    pub names: std::collections::BTreeSet<String>,
    pub authored: std::collections::BTreeSet<String>,
    /// The ground check's counts for the world (`None`: no standing camera or no terrain).
    pub ungrounded: Option<crate::grounding::Counts>,
}

pub fn world_facts(geo: &Geometry) -> WorldFacts {
    let mut f = WorldFacts { layers: layers_of(geo), names: geo.material_names().into_iter().collect(), authored: Default::default(), ungrounded: None };
    let note = |g: &Geometry, authored: &mut std::collections::BTreeSet<String>| {
        authored.extend(g.detail.keys().filter_map(|k| k.strip_prefix(pt_core::gltf::ASSET_MATERIAL_PREFIX)).map(String::from));
    };
    note(geo, &mut f.authored);
    for p in geo.instances.iter().flat_map(|i| i.protos.iter()) {
        let r = p.realize();
        f.names.extend(r.material_names());
        note(&r, &mut f.authored);
    }
    f
}

/// Material names of a world that neither the library nor the scene file knows.
pub fn unknown_from(f: &WorldFacts, lib: &Value, scene: &Value) -> Vec<String> {
    let mut names = f.names.clone();
    for roles in f.layers.values() {
        names.extend(roles.iter().cloned());
    }
    // known = in the library, described by the scene, or authored in an imported asset (its textures travel with the glTF)
    let known = |n: &str| f.authored.contains(n) || lib.pointer(&format!("/materials/{n}")).is_some() || scene.pointer(&format!("/materials/{n}")).is_some();
    names.into_iter().filter(|n| !known(n)).collect()
}

/// Material names used by the geometry (base and prototypes) that neither the library nor the scene file knows.
pub fn unknown_materials(geo: &Geometry, lib: &Value, scene: &Value) -> Vec<String> {
    unknown_from(&world_facts(geo), lib, scene)
}

fn write_sidecar(path: &Path, key: &str, f: &WorldFacts) {
    let placement = f.ungrounded.map(|c| json!([c.hang, c.in_face, c.buried, c.drowned]));
    let v = json!({"key": key, "layers": f.layers, "names": f.names, "authored": f.authored, "placement": placement});
    let _ = std::fs::write(path, v.to_string());
}

fn read_sidecar(path: &Path, key: &str) -> Option<WorldFacts> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    if v.get("key")?.as_str()? != key {
        return None;
    }
    let strs = |x: &Value| -> std::collections::BTreeSet<String> { x.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default() };
    Some(WorldFacts {
        layers: v.get("layers")?.as_object()?.iter().map(|(k, r)| (k.clone(), strs(r).into_iter().collect::<Vec<_>>())).collect(),
        names: strs(v.get("names")?),
        authored: strs(v.get("authored")?),
        ungrounded: v.get("placement").and_then(Value::as_array).and_then(|a| {
            let n = |i: usize| a.get(i).and_then(Value::as_u64);
            Some(crate::grounding::Counts { hang: n(0)?, in_face: n(1)?, buried: n(2)?, drowned: n(3)?, ..Default::default() })
        }),
    })
}

/// Everything that decides the exported world: the recipe, the program that cooks it (its own bytes), the tier, the crop and the
/// cameras the crop looks through. A model written for one key is rendered again without cooking while the key holds.
fn world_key(code: &str, scene: &Value, o: &Opts, cull: Option<f32>) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"world/1");
    h.update(code.as_bytes());
    if let Some(Ok(hex)) = std::env::current_exe().ok().map(|e| file_hash(&e)) {
        h.update(hex.as_bytes());
    }
    h.update(&[o.lookdev as u8, o.lod as u8]);
    h.update(format!("{cull:?}|{:?}", o.only).as_bytes());
    h.update(serde_json::to_string(&knobs_of(scene).unwrap_or_default()).unwrap_or_default().as_bytes());
    // the owner's decisions are part of the world (recipes read them with `edits_of`): a recorded edit must show up in the next render
    let mut edit_files: Vec<PathBuf> = std::fs::read_dir(repo_root().join("edits")).into_iter().flatten().flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    edit_files.sort();
    for f in edit_files {
        if let Ok(t) = std::fs::read(&f) {
            h.update(f.file_name().map_or(&[][..], |n| n.as_encoded_bytes()));
            h.update(&t);
        }
    }
    if let (Some(_), Some(only)) = (cull, o.only.as_deref()) {
        let names: Vec<&str> = only.split(',').map(str::trim).collect();
        for c in scene.get("cameras").and_then(Value::as_array).into_iter().flatten().filter(|c| c.get("name").and_then(Value::as_str).is_some_and(|n| names.contains(&n))) {
            h.update(c.to_string().as_bytes());
        }
    }
    h.finalize().to_hex()[..16].to_string()
}

fn find_recipe(name: &str) -> Result<PathBuf, String> {
    if Path::new(name).exists() {
        return Ok(PathBuf::from(name));
    }
    crate::recipes::list().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(name))).ok_or_else(|| format!("no recipe matching '{name}'"))
}

pub struct Opts {
    pub quick: bool,
    pub samples: Option<u32>,
    pub keep: Option<PathBuf>,
    /// Export levels of detail (nodes `inst_<i>__lod<l>` with distance ranges); the render draws level 0 only.
    pub lod: bool,
    /// Render only these cameras of the scene (names, comma separated): look development on one view.
    pub only: Option<String>,
    /// Look-development tier: recipes choose lighter assets through `lookdev(full, light)`, and with `only` the world is cropped to
    /// what those cameras can see (`cull_m`, 60 m unless `--cull` says otherwise).
    pub lookdev: bool,
}

fn load_library(root: &Path) -> Value {
    std::fs::read_to_string(root.join("corpus/materials/library.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({"materials": {}}))
}

/// Fold the scene's `style` (an object, or a path to a style file) into the scene: the style supplies defaults for
/// `lighting`, `render`, `ground`, `default_material` and the material ROLES (`materials`: role name -> library material
/// plus tint), and anything the scene file states itself wins, key by key. The style's name is kept as `style_name`.
pub fn resolve_style(scene: &mut Value) -> Result<(), String> {
    let Some(style) = scene.get("style").cloned() else { return Ok(()) };
    let style: Value = match style {
        Value::String(p) => {
            let root = repo_root();
            let path = [PathBuf::from(&p), root.join(&p)].into_iter().find(|c| c.exists()).ok_or_else(|| format!("style file '{p}' not found (looked in the working directory and {})", root.display()))?;
            serde_json::from_str(std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display()))?
        }
        v if v.is_object() => v,
        _ => return Err("scene.style must be an object or a path to a style .json file".into()),
    };
    let sobj = style.as_object().ok_or("the style file must be a JSON object")?;
    let known = ["name", "version", "note", "lighting", "render", "ground", "materials", "default_material", "knobs"];
    let mut problems: Vec<String> = Vec::new();
    unknown_keys(sobj, &known, "style", &mut problems);
    if !problems.is_empty() {
        return Err(problems.join("\n  "));
    }
    let obj = scene.as_object_mut().ok_or("scene must be an object")?;
    for key in ["lighting", "render", "default_material", "materials", "knobs"] {
        if let Some(Value::Object(from_style)) = sobj.get(key) {
            let mut merged = from_style.clone();
            if let Some(Value::Object(mine)) = obj.get(key) {
                merged.extend(mine.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            obj.insert(key.to_string(), Value::Object(merged));
        }
    }
    if let (Some(g), false) = (sobj.get("ground"), obj.contains_key("ground")) {
        obj.insert("ground".into(), g.clone());
    }
    if let Some(n) = sobj.get("name") {
        obj.insert("style_name".into(), n.clone());
    }
    obj.remove("style");
    Ok(())
}

/// Check a scene value and apply the `samples` override; the error lists every problem with its fix.
fn prepare_scene(mut scene: Value, what: &str, o: &Opts) -> Result<Value, String> {
    resolve_style(&mut scene).map_err(|e| format!("scene {what}: {e}"))?;
    let problems = check_scene(&scene);
    if !problems.is_empty() {
        return Err(format!("scene {what}:\n  {}", problems.join("\n  ")));
    }
    if let Some(n) = o.samples {
        scene["render"]["samples"] = json!(n);
    }
    Ok(scene)
}

/// Render an already cooked geometry (what the MCP server and `render` use). `stem` names the work files.
pub fn render_geometry(geo: &Geometry, stem: &str, scene: Value, out: &Path, o: &Opts) -> Result<Value, String> {
    let (glb, scene, unknown) = export_for_render(geo, stem, scene, o)?;
    run_blender(&glb, &scene, unknown, stem, out, o, &[])
}

/// Write the model Blender will read and finish the scene description for it (layer roles, unknown material names).
fn export_for_render(geo: &Geometry, stem: &str, scene: Value, o: &Opts) -> Result<(PathBuf, Value, Vec<String>), String> {
    let mut scene = prepare_scene(scene, "description", o)?;
    inject_layers(&mut scene, geo);
    let root = repo_root();
    let unknown = unknown_materials(geo, &load_library(&root), &scene);
    let work = o.keep.clone().unwrap_or_else(|| root.join("out/render"));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let glb = work.join(format!("{stem}.glb"));
    let bytes = if o.lod {
        pt_core::gltf::to_glb_lod(geo, &pt_core::lod::LodPolicy { cache_dir: Some(root.join("out/lod_cache")), ..Default::default() })
    } else {
        pt_core::gltf::to_glb(geo)
    };
    std::fs::write(&glb, bytes).map_err(|e| e.to_string())?;
    Ok((glb, scene, unknown))
}

/// Render `input` (a recipe file or name, or a `.glb`/`.gltf`) with `scene_path` to `out`. Returns the Blender-side report
/// (frames, objects, triangles, material coverage, unresolved materials, warnings) plus `unknown_materials`.
pub fn render(input: &str, scene_path: &Path, out: &Path, o: &Opts) -> Result<Value, String> {
    let scene_text = std::fs::read_to_string(scene_path).map_err(|e| format!("{}: {e}", scene_path.display()))?;
    let scene: Value = serde_json::from_str(scene_text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", scene_path.display()))?;
    let stem = Path::new(input).file_stem().map_or_else(|| "scene".to_string(), |s| s.to_string_lossy().into_owned());
    if input.ends_with(".glb") || input.ends_with(".gltf") {
        let scene = prepare_scene(scene, &scene_path.display().to_string(), o)?;
        return run_blender(Path::new(input), &scene, vec![], &stem, out, o, &[]);
    }
    let path = find_recipe(input)?;
    let code = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    // The crop (look development, with `--only`): drop what the chosen cameras cannot see, so a one-view render fits in memory.
    // `--cull METRES` sets the radius; `--lookdev` alone means 60 m.
    let cull = std::env::var("PT_VIEW_CULL_M").ok().and_then(|s| s.parse::<f32>().ok()).filter(|r| *r > 0.0).or_else(|| o.lookdev.then_some(60.0)).filter(|_| o.only.is_some());
    // The cooked world is kept next to its model file under a key; a render that changes only the scene file, a style or the render
    // script (materials, light, camera) opens the cached model and goes straight to Blender instead of cooking for minutes.
    let key = world_key(&code, &scene, o, cull);
    let work = o.keep.clone().unwrap_or_else(|| repo_root().join("out/render"));
    let (cached_glb, side) = (work.join(format!("{stem}.glb")), work.join(format!("{stem}.world.json")));
    if std::env::var("PT_FRESH_WORLD").is_err() && cached_glb.exists() {
        if let Some(facts) = read_sidecar(&side, &key) {
            eprintln!("cooked world cache hit ({key}): not cooking again (--fresh forces it)");
            let mut sc = prepare_scene(scene, "description", o)?;
            inject_layers_from(&mut sc, &facts.layers);
            let unknown = unknown_from(&facts, &load_library(&repo_root()), &sc);
            return run_blender(&cached_glb, &sc, unknown, &stem, out, o, &[]).map(|r| with_grounding(r, facts.ungrounded));
        }
    }
    // The graph and the cooked world are dropped before Blender starts: a large world is gigabytes here and Blender needs
    // several times the model's size to import it, and the two together do not fit in the memory of an ordinary desktop.
    let (glb, scene, unknown, ungrounded) = {
        let mut g = Graph::new();
        g.lookdev = o.lookdev;
        g.knobs = knobs_of(&scene)?;
        let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
        let mut geo = (*g.cook(rep.outputs.first().ok_or("recipe has no out()")?)?).clone();
        // every frame carries the ground check's verdict on the world it shows, so a floating object is never found by eye first
        let ungrounded = crate::grounding::placement_counts(&geo, &scene, 45.0);
        if let (Some(r), Some(only)) = (cull, o.only.as_deref()) {
            let names: Vec<&str> = only.split(',').map(str::trim).collect();
            let cams: Vec<crate::grounding::Camera> = crate::grounding::cameras_of(&scene).into_iter().filter(|c| names.contains(&c.name.as_str())).collect();
            if !cams.is_empty() {
                let (kept, total) = crate::grounding::cull_to_view(&mut geo, &cams, r);
                eprintln!("view cull {r} m: kept {kept} of {total} instances");
            }
        }
        let mut facts = world_facts(&geo);
        facts.ungrounded = ungrounded;
        let (glb, scene, unknown) = export_for_render(&geo, &stem, scene, o)?;
        write_sidecar(&side, &key, &facts);
        (glb, scene, unknown, ungrounded)
    };
    run_blender(&glb, &scene, unknown, &stem, out, o, &[]).map(|r| with_grounding(r, ungrounded))
}

/// Put the ground check's verdict into a render report: the count of objects that float or hang inside a camera view, and a
/// warning when there are any. A frame is looked at through its report, so the report says it before an eye has to find it.
fn with_grounding(mut report: Value, ungrounded: Option<crate::grounding::Counts>) -> Value {
    if let Some(c) = ungrounded {
        report["floating_or_hanging_in_view"] = json!(c.hang);
        report["objects_in_camera_face"] = json!(c.in_face);
        report["buried_anywhere"] = json!(c.buried);
        report["drowned_anywhere"] = json!(c.drowned);
        let mut says = Vec::new();
        if c.hang > 0 {
            says.push(format!("{} objects float or hang inside a camera view: run `pt ground-check` and seat them with the `seat` node before this frame is shown", c.hang));
        }
        if c.in_face > 0 {
            says.push(format!("{} objects stand within arm's reach of a camera's eye: `pt ground-check` names them; the `seat` node's `cameras` keeps them out", c.in_face));
        }
        if c.buried > 0 || c.drowned > 0 {
            says.push(format!("{} objects are more than half under the ground and {} are mostly under water: `seat` with `sink` and `wade` removes or raises them", c.buried, c.drowned));
        }
        for w in says {
            match report.get_mut("warnings").and_then(Value::as_array_mut) {
                Some(list) => list.push(json!(w)),
                None => report["warnings"] = json!([w]),
            }
        }
    }
    report
}

/// Bake the layered materials of an exported model (terrain blends) into an albedo and a tangent-space normal PNG each, through
/// the model's UVs. Returns `{material: {"albedo": path, "normal": path, ...}}`; empty when nothing is layered.
pub fn bake_layers(model: &Path, mut scene: Value, geo: &Geometry, work: &Path, res: u32) -> Result<BTreeMap<String, Value>, String> {
    resolve_style(&mut scene)?;
    inject_layers(&mut scene, geo);
    let o = Opts { quick: false, samples: None, keep: Some(work.to_path_buf()), lod: false, only: None, lookdev: false };
    let dir = work.join("bake");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let extra = vec!["bake=1".to_string(), format!("bake_dir={}", std::env::current_dir().unwrap_or_default().join(&dir).display()), format!("bake_res={res}")];
    let report = run_blender(model, &scene, vec![], "bake", &work.join("bake_unused.png"), &o, &extra)?;
    Ok(report.get("baked").and_then(Value::as_object).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default())
}

/// BLAKE3 of a file's content as hex, read in chunks (a world file can be gigabytes).
pub(crate) fn file_hash(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 22];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

fn run_blender(model: &Path, scene: &Value, unknown: Vec<String>, stem: &str, out: &Path, o: &Opts, extra: &[String]) -> Result<Value, String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let (model, out) = (cwd.join(model), cwd.join(out)); // an absolute path stays as it is
    let (model, out) = (model.as_path(), out.as_path());
    let root = repo_root();
    let lib_path = root.join("corpus/materials/library.json");
    let work = o.keep.clone().unwrap_or_else(|| root.join("out/render"));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let scene_file = work.join(format!("{stem}.scene.json"));
    std::fs::write(&scene_file, scene.to_string()).map_err(|e| e.to_string())?;
    let report_file = work.join(format!("{stem}.report.json"));
    let _ = std::fs::remove_file(&report_file);
    let exe = blender_exe();
    if !exe.exists() {
        return Err(format!("Blender not found at {} (set PT_BLENDER)", exe.display()));
    }
    let script = root.join("corpus/tools/render_scene.py");
    let mut cmd = Command::new(&exe);
    cmd.args(["-b", "--factory-startup", "-t"]).arg(crate::lowload::threads().to_string()).arg("-P").arg(&script).arg("--");
    cmd.arg(format!("model={}", model.display()))
        .arg(format!("scene={}", scene_file.display()))
        .arg(format!("out={}", out.display()))
        .arg(format!("library={}", lib_path.display()))
        .arg(format!("corpus={}", crate::corpus::root().display()))
        .arg(format!("report={}", report_file.display()));
    if o.quick {
        cmd.arg("quick=1");
    }
    if let Some(names) = &o.only {
        cmd.arg(format!("only={names}"));
    }
    // Import cache: Blender needs minutes to import a large world and seconds to open it again. The cache file is named after
    // the model file and its content, so a changed model never meets a stale cache; older caches of the same model are removed.
    // The render and the terrain bake of one world read the same model, so the bake opens what the render imported.
    {
        if let Ok(hash) = file_hash(model) {
            // under out/ (never in a results folder: the cache is as large as the world file)
            let cache_dir = root.join("out/blend_cache");
            let _ = std::fs::create_dir_all(&cache_dir);
            let model_stem = model.file_stem().map_or_else(|| stem.to_string(), |s| s.to_string_lossy().into_owned());
            let cache = cache_dir.join(format!("{model_stem}.{}.blend", &hash[..12]));
            if let Ok(dir) = std::fs::read_dir(&cache_dir) {
                for f in dir.flatten() {
                    let name = f.file_name().to_string_lossy().into_owned();
                    let stale = name.starts_with(&format!("{model_stem}.")) && (name.ends_with(".blend") || name.ends_with(".blend1")) && f.path() != cache;
                    if stale {
                        let _ = std::fs::remove_file(f.path());
                    }
                }
            }
            cmd.arg(format!("blend_cache={}", cwd.join(&cache).display()));
        }
    }
    cmd.args(extra);
    let output = crate::lowload::output(&mut cmd).map_err(|e| format!("could not start Blender: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut report: Value = std::fs::read_to_string(&report_file).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or_else(|| {
        let tail: Vec<&str> = text.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect();
        format!("Blender produced no report (exit {:?}). Last output:\n{}\n{}", output.status.code(), tail.join("\n"), String::from_utf8_lossy(&output.stderr).lines().take(10).collect::<Vec<_>>().join("\n"))
    })?;
    report["unknown_materials"] = json!(unknown);
    if let Some(n) = scene.get("style_name") {
        report["style"] = n.clone();
    }
    report["model"] = json!(model.display().to_string());
    Ok(report)
}

pub const RENDER_SCENE: crate::registry::Command = {
    use crate::registry::{Kind, Param};
    crate::registry::Command {
        name: "render-scene",
        tool: "render_world",
        summary: "Cook a recipe (or read a .glb) and render it with Blender Cycles under a scene file's cameras, sky and sun, with material names bound to the CC0 PBR library. Prints the report: frames, triangles, material coverage, unknown materials, warnings.",
        heavy: true,
        params: &[
            Param::pos("input", Kind::Str, "recipe .hcs or a .glb file"),
            Param::pos("scene", Kind::Str, "scene .json file (cameras, lighting, render settings)"),
            Param::pos("out", Kind::Str, "output .png path (the report names every frame written)"),
            Param::flag("quick", Kind::Bool, "quarter samples and half size"),
            Param::flag("samples", Kind::Int, "Cycles samples"),
            Param::flag("keep", Kind::Str, "folder to keep the exported world and the Blender files in"),
            Param::flag("lod", Kind::Bool, "render with the levels of detail an engine would get"),
            Param::flag("only", Kind::Str, "render only these cameras (comma-separated names)"),
            Param::flag("cull", Kind::Num, "with only: drop what the chosen cameras cannot see beyond this many metres"),
            Param::flag("lookdev", Kind::Bool, "the look-development tier: fits this machine, not the full-quality world"),
            Param::flag("fresh", Kind::Bool, "ignore the cache of the cooked world"),
        ],
        run: render_scene_cmd,
    }
};

/// `pt render-scene <recipe|file.glb> <scene.json> <out.png> [--quick] [--samples N] [--keep DIR] [--lod]`
pub fn render_scene_cmd(args: &[String]) -> Result<(), String> {
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let flag_val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1));
    let (Some(input), Some(scene), Some(out)) = (pos.first(), pos.get(1), pos.get(2)) else {
        return Err("usage: pt render-scene <recipe|file.glb> <scene.json> <out.png> [--quick] [--samples N] [--keep DIR] [--lod] [--only CAMERA,CAMERA [--cull METRES]] [--lookdev] [--fresh]".into());
    };
    let opts = Opts { quick: args.iter().any(|a| a == "--quick"), samples: flag_val("--samples").and_then(|s| s.parse().ok()), keep: flag_val("--keep").map(PathBuf::from), lod: args.iter().any(|a| a == "--lod"), only: flag_val("--only").cloned(), lookdev: args.iter().any(|a| a == "--lookdev") };
    if let Some(r) = flag_val("--cull") {
        std::env::set_var("PT_VIEW_CULL_M", r);
    }
    if args.iter().any(|a| a == "--fresh") {
        std::env::set_var("PT_FRESH_WORLD", "1");
    }
    let report = render(input, Path::new(scene), Path::new(out), &opts)?;
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_scene_passes_and_typos_are_caught_with_the_fix() {
        let good = json!({
            "render": {"samples": 64, "resolution": [1280, 720]},
            "lighting": {"sky": "nishita", "sun_elevation": 30, "sun_azimuth": 140},
            "camera": {"location": [10, 5, 10], "look_at": [0, 0, 0], "lens": 35},
            "materials": {"grass": {"tint": [0.5, 0.8, 0.5]}}
        });
        assert!(check_scene(&good).is_empty(), "{:?}", check_scene(&good));
        let bad = json!({
            "lightng": {},
            "render": {"samples": 0, "resolution": [10, 720], "device": "tpu"},
            "lighting": {"sky": "hdri", "sun_elevation": 120},
            "camera": {"location": [1, 2], "eye": {"at": [1]}},
            "cameras": [{}]
        });
        let e = check_scene(&bad).join("\n");
        for needle in ["unknown key 'lightng'", "render.samples", "render.resolution", "render.device", "lighting.hdri", "lighting.sun_elevation", "location and look_at", "eye.at", "not both"] {
            assert!(e.contains(needle), "missing '{needle}' in:\n{e}");
        }
    }

    #[test]
    fn a_style_supplies_defaults_and_the_scene_wins_key_by_key() {
        let mut scene = json!({
            "lighting": {"sun_elevation": 20},
            "materials": {"role_ground": {"use": "sand"}},
            "style": {"name": "dune", "lighting": {"sun_elevation": 50, "sky": "nishita", "fog_density": 0.0002},
                      "materials": {"role_ground": {"use": "grass"}, "role_rock": {"use": "rock_cliff", "tint": [0.8, 0.7, 0.6]}},
                      "render": {"exposure": 0.5}, "ground": {"material": "role_ground"}}
        });
        resolve_style(&mut scene).unwrap();
        assert_eq!(scene["lighting"]["sun_elevation"], 20, "the scene's own value wins");
        assert_eq!(scene["lighting"]["fog_density"], 0.0002, "the style fills the rest");
        assert_eq!(scene["materials"]["role_ground"]["use"], "sand");
        assert_eq!(scene["materials"]["role_rock"]["tint"][0], 0.8);
        assert_eq!((scene["render"]["exposure"].clone(), scene["ground"]["material"].clone(), scene["style_name"].clone()), (json!(0.5), json!("role_ground"), json!("dune")));
        assert!(scene.get("style").is_none() && check_scene(&scene).is_empty(), "{:?}", check_scene(&scene));
        // a typo in the style is rejected with the known keys
        let mut bad = json!({"style": {"lightning": {}}});
        assert!(resolve_style(&mut bad).unwrap_err().contains("unknown key 'lightning'"));
        let mut missing = json!({"style": "styles/does_not_exist.json"});
        assert!(resolve_style(&mut missing).unwrap_err().contains("not found"));
    }

    #[test]
    fn a_render_report_says_when_objects_float_or_hang_in_view() {
        use crate::grounding::Counts;
        let clean = with_grounding(json!({"warnings": []}), Some(Counts::default()));
        assert_eq!((clean["floating_or_hanging_in_view"].clone(), clean["warnings"].as_array().unwrap().len()), (json!(0), 0));
        let bad = with_grounding(json!({"frames": []}), Some(Counts { hang: 7, ..Default::default() }));
        assert_eq!(bad["floating_or_hanging_in_view"], 7);
        assert!(bad["warnings"][0].as_str().unwrap().contains("7 objects float or hang"));
        // an object in a camera's face, and objects buried or drowned anywhere, speak in the report too
        let more = with_grounding(json!({}), Some(Counts { in_face: 2, buried: 5, drowned: 3, ..Default::default() }));
        assert_eq!((more["objects_in_camera_face"].clone(), more["buried_anywhere"].clone(), more["drowned_anywhere"].clone()), (json!(2), json!(5), json!(3)));
        assert_eq!(more["warnings"].as_array().unwrap().len(), 2);
        // no standing camera or no terrain: nothing to say
        assert!(with_grounding(json!({}), None).get("floating_or_hanging_in_view").is_none());
    }

    #[test]
    fn unknown_materials_are_found_in_base_and_prototypes() {
        let mut g = pt_core::Geometry::from_polys(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[vec![0, 1, 2]]);
        g.assign_material("grass", &[true]);
        let mut proto = g.clone();
        proto.assign_material("mystery_ore", &[true]);
        g.instances = Some(pt_core::Instances { protos: vec![std::sync::Arc::new(proto)], proto_id: std::sync::Arc::new(vec![0]), xf: std::sync::Arc::new(vec![pt_core::math::IDENTITY]), attrs: Default::default() });
        let lib = json!({"materials": {"grass": {}}});
        assert_eq!(unknown_materials(&g, &lib, &json!({})), ["mystery_ore"]);
        assert!(unknown_materials(&g, &lib, &json!({"materials": {"mystery_ore": {"color": [1, 0, 0]}}})).is_empty());
    }
}
