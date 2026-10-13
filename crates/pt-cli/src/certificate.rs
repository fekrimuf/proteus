//! `pt certificate`: the clean-title certificate of a world (ROUTE.md row 15).
//!
//! It reads the recipe text, the scene or style file (for the materials) and `corpus/LEDGER.csv`; no asset file, GPU or Blender is
//! needed. The recipe is run into a graph (no cooking). Everything the world reads from the corpus when it is rendered is looked up
//! in the ledger: every file in the folder of each `import_asset` model (the .gltf, its .bin, its textures), every `dem` elevation
//! tile, the Poly Haven material sets the style's materials resolve to (by the render's own lookup, `blender::polyhaven_set`) and
//! the sky picture. A Markdown report lists each with its source, licence and hash. A file the ledger does not hold fails the
//! command (exit code 1) and is named; no report is written then, so a certificate that exists is a certificate that is complete.
//! The report has no date or machine detail and does not depend on which asset files are on this machine, so the same recipe,
//! ledger and style always give the same file.
//!
//! Other corpus reads, and why they need no row: the LOD cache (`<corpus>/_cache/lod`) holds only data derived from a model that is
//! listed; the engine bridge receives the exported glTF and reads no corpus file itself; the foliage texture in the library
//! (`corpus/generated/*.png`) is drawn by this program, lives in the repository and is listed as program-made.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::corpus::{sha256_hex, Ledger};
use pt_core::ParamExt;
use pt_graph::Graph;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

pub const CERTIFICATE: crate::registry::Command = {
    use crate::registry::{Kind, Param};
    crate::registry::Command {
        name: "certificate",
        tool: "certificate",
        summary: "Write the clean-title certificate of a world: every file its recipe reads from the corpus, with source, licence and hash from corpus/LEDGER.csv, and a closing line that claims only what was checked. Models: every ledger file in the model's folder (.gltf, .bin, textures; the .gltf's own buffer and image names are checked when the file is present). Elevation tiles of dem nodes. With --scene or --style: the scanned material sets the style's materials use (the render's own lookup) and the sky picture. Without them the report says materials were not certified and claims no completeness. Works from the texts and the ledger alone (no asset files, GPU or Blender). Fails with exit code 1, naming them, when a file or a material set is missing from the ledger.",
        heavy: false,
        params: &[
            Param::pos("recipe", Kind::Str, "the world's recipe .hcs file"),
            Param::req_flag("out", Kind::Str, "the Markdown file to write"),
            Param::flag("scene", Kind::Str, "the world's scene .json (it names its style); certifies the materials and the sky picture"),
            Param::flag("style", Kind::Str, "a style .json on its own (or replacing the scene's style); certifies the materials"),
            Param::flag("ledger", Kind::Str, "the licence ledger (default corpus/LEDGER.csv of this repository)"),
        ],
        run: certificate_cmd,
    }
};

/// One file the world reads, with what kind of file it is.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Used {
    pub id: String,
    pub kind: &'static str,
}

/// What a recipe names, found without cooking: the `import_asset` paths, the elevation tile ids of its `dem` nodes (the same tile
/// list `pt dem-manifest` writes) and every material name it sets or layers.
#[derive(Debug, Default)]
pub struct RecipeReads {
    pub models: BTreeSet<String>,
    pub tiles: BTreeSet<String>,
    pub materials: BTreeSet<String>,
}

pub fn recipe_reads(code: &str) -> Result<RecipeReads, String> {
    let mut g = Graph::new();
    pt_hcs::run(code, &mut g, true).map_err(|d| d.first().map_or_else(|| "recipe failed".to_string(), |d| d.to_string()))?;
    let mut r = RecipeReads::default();
    for (name, n) in &g.nodes {
        match n.kind.as_str() {
            "import_asset" => {
                let path = n.params.s("path").replace('\\', "/");
                if path.is_empty() {
                    return Err(format!("node {name}: import_asset has no `path`, so its source cannot be certified"));
                }
                r.models.insert(path);
            }
            "set_material" => {
                r.materials.insert(n.params.s("name"));
            }
            "blend_materials" => {
                r.materials.insert(n.params.s("name"));
                r.materials.extend(n.params.s("layers").split([',', ' ']).filter(|s| !s.is_empty()).map(String::from));
            }
            "dem" => {
                let num = |k: &str, d: f64| n.params.get(k).and_then(Value::as_f64).unwrap_or(d);
                let tiles = Some(n.params.s("tiles")).filter(|t| !t.is_empty()).unwrap_or_else(|| "dem/terrarium".into()).replace('\\', "/");
                let size = if n.params.contains_key("size") { n.params.v3("size") } else { [1200.0, 1200.0, 0.0] };
                let zoom = (num("zoom", 14.0).round() as i64).clamp(0, 18); // the language stores every number as a float
                let far = (num("far_zoom", 0.0).round() as i64).clamp(0, 18);
                for z in [zoom, far].into_iter().filter(|&z| z > 0 || z == zoom) {
                    let (x0, x1, y0, y1) = pt_sop::dem::tile_range(num("lat", 43.7767), num("lon", -110.9028), z as u32, size[0] as f64, size[1] as f64);
                    for x in x0..=x1 {
                        for y in y0..=y1 {
                            r.tiles.insert(format!("{tiles}/{z}/{x}/{y}.png"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    r.materials.retain(|m| !m.is_empty());
    Ok(r)
}

/// The scene (style already folded in, `blender::resolve_style`) and the material library the materials are certified from.
pub struct Materials {
    pub scene: Value,
    pub lib: Value,
}

/// A uri inside a glTF, as a path relative to the model's folder with `%20`-style escapes undone.
fn decode_uri(uri: &str) -> String {
    let b = uri.as_bytes();
    let hex = |i: usize| b.get(i).and_then(|c| (*c as char).to_digit(16));
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let (b'%', Some(h), Some(l)) = (b[i], hex(i + 1), hex(i + 2)) {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).replace('\\', "/")
}

/// `dir/uri` with `.` and `..` folded away; `None` when it climbs out of the corpus root.
fn join_uri(dir: &str, uri: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in uri.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// Buffer and image files a .gltf names that the ledger has no row for.
fn gltf_unledgered(rel: &str, text: &[u8], ledger: &Ledger) -> Vec<String> {
    let Ok(doc) = serde_json::from_slice::<Value>(text) else { return vec![format!("{rel} (not valid glTF JSON, so its buffers and images cannot be checked)")] };
    let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
    let mut out = Vec::new();
    for key in ["buffers", "images"] {
        for uri in doc.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(|e| e.get("uri").and_then(Value::as_str)).filter(|u| !u.starts_with("data:")) {
            match join_uri(dir, &decode_uri(uri)) {
                Some(id) if ledger.rows.contains_key(&id) => {}
                Some(id) => out.push(format!("{id} (named by {rel})")),
                None => out.push(format!("{uri} (named by {rel}, outside the corpus)")),
            }
        }
    }
    out
}

/// Every ledger row under `prefix` (a folder, ending in `/`).
fn rows_under(ledger: &Ledger, prefix: &str) -> Vec<String> {
    ledger.rows.range(prefix.to_string()..).map(|(k, _)| k).take_while(|k| k.starts_with(prefix)).cloned().collect()
}

/// The certificate text, or the files and material sets missing from the ledger. `corpus` is where asset files would be on this
/// machine: a model's .gltf found there has its buffer and image names checked as well, and the report is the same either way.
pub fn certificate(recipe_name: &str, code: &str, ledger: &Ledger, materials: Option<&Materials>, corpus: &Path) -> Result<String, String> {
    let reads = recipe_reads(code)?;
    let mut used: BTreeSet<Used> = BTreeSet::new();
    let mut missing: Vec<String> = Vec::new();
    for path in &reads.models {
        if !ledger.rows.contains_key(path) {
            missing.push(path.clone());
        }
        let ids: Vec<String> = match path.rsplit_once('/') {
            Some((d, _)) => rows_under(ledger, &format!("{d}/")),
            None => ledger.rows.contains_key(path).then(|| path.clone()).into_iter().collect(),
        };
        used.extend(ids.into_iter().map(|id| Used { id, kind: "model file" }));
        if path.ends_with(".gltf") && !path.contains("..") {
            if let Ok(bytes) = std::fs::read(corpus.join(path)) {
                missing.extend(gltf_unledgered(path, &bytes, ledger));
            }
        }
    }
    for t in &reads.tiles {
        if !ledger.rows.contains_key(t) {
            missing.push(t.clone());
        }
        used.insert(Used { id: t.clone(), kind: "elevation tile" });
    }
    let mut drawn: BTreeSet<String> = BTreeSet::new();
    let mut style_name = String::new();
    if let Some(m) = materials {
        style_name = m.scene.get("style_name").and_then(Value::as_str).unwrap_or("").to_string();
        let mut names: BTreeSet<String> = reads.materials.clone();
        names.extend(m.scene.get("materials").and_then(Value::as_object).into_iter().flat_map(|o| o.keys().cloned()));
        names.extend(m.scene.pointer("/ground/material").and_then(Value::as_str).map(String::from));
        let mut reached = crate::blender::Reached::default(); // sets: set -> the first material name that leads to it
        for n in &names {
            crate::blender::reach_material(&m.scene, &m.lib, n, &mut reached);
        }
        // a picture under corpus/generated is drawn by this program; any other picture a material names must be in the ledger
        for t in &reached.textures {
            if t.starts_with("corpus/generated/") {
                drawn.insert(t.clone());
            } else if ledger.rows.contains_key(t) {
                used.insert(Used { id: t.clone(), kind: "material file" });
            } else {
                missing.push(format!("material picture {t}"));
            }
        }
        for (set, name) in &reached.sets {
            let folder = format!("polyhaven/materials/{set}/");
            let rows = rows_under(ledger, &folder);
            if rows.is_empty() {
                missing.push(format!("material set {set} (used by material {name}; no ledger rows under {folder})"));
            }
            used.extend(rows.into_iter().map(|id| Used { id, kind: "material file" }));
        }
        if m.scene.pointer("/lighting/sky").and_then(Value::as_str) == Some("hdri") {
            let hdri = m.scene.pointer("/lighting/hdri").and_then(Value::as_str).unwrap_or("").replace('\\', "/");
            if ledger.rows.contains_key(&hdri) {
                used.insert(Used { id: hdri, kind: "sky picture" });
            } else {
                missing.push(format!("sky picture {hdri}"));
            }
        }
    }
    if !missing.is_empty() {
        let shown = missing.iter().take(12).map(String::as_str).collect::<Vec<_>>().join(", ");
        let more = if missing.len() > 12 { format!(" and {} more", missing.len() - 12) } else { String::new() };
        return Err(format!("certificate: {} file(s) or material set(s) used by {recipe_name} are missing from the licence ledger: {shown}{more}", missing.len()));
    }
    let mut s = format!("# Clean-title certificate\n\nWorld recipe: `{recipe_name}` (sha256 `{}`)\n", sha256_hex(code.as_bytes()));
    if materials.is_some() {
        let style = if style_name.is_empty() { "the scene's own materials".to_string() } else { format!("style `{style_name}`") };
        s.push_str(&format!("Materials certified from {style}.\n"));
    }
    s.push_str(&format!("\nFiles listed: {} (every one is in the licence ledger, `corpus/LEDGER.csv`).\n\n", used.len()));
    if !used.is_empty() {
        s.push_str("| File | Kind | Source | Licence | sha256 | Origin |\n|---|---|---|---|---|---|\n");
        for u in &used {
            let r = &ledger.rows[&u.id]; // id, source, url, license, license_url, sha256, fetched_at, attribution
            s.push_str(&format!("| `{}` | {} | {} | [{}]({}) | `{}` | <{}> |\n", u.id, u.kind, r[1], r[3], r[4], r[5], r[2]));
        }
        s.push('\n');
        let mut notes: BTreeSet<(&str, &str)> = BTreeSet::new();
        for u in &used {
            let r = &ledger.rows[&u.id];
            if !r[7].is_empty() {
                notes.insert((r[1].as_str(), r[7].as_str()));
            }
        }
        if !notes.is_empty() {
            s.push_str("Credit lines the sources ask for:\n\n");
            for (src, text) in notes {
                s.push_str(&format!("* {src}: {text}\n"));
            }
            s.push('\n');
        }
    }
    if !drawn.is_empty() {
        s.push_str("Pictures drawn by this program (kept in the repository, no outside source):\n\n");
        for d in &drawn {
            s.push_str(&format!("* `{d}`\n"));
        }
        s.push('\n');
    }
    if materials.is_some() {
        s.push_str("Every file in the folder of each model, every elevation tile, every scanned material set and the sky picture this world reads are listed above. Everything else in this world is made by the recipe's rules.\n");
    } else {
        s.push_str("Materials were not certified: no scene or style was given, so the scanned material pictures a render binds are not listed. This report lists the model files and elevation tiles the recipe reads and makes no claim that it lists every file the world reads.\n");
    }
    Ok(s)
}

/// `pt certificate <recipe.hcs> --out FILE [--scene FILE] [--style FILE] [--ledger FILE]`
pub fn certificate_cmd(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: pt certificate <recipe.hcs> --out FILE [--scene FILE] [--style FILE] [--ledger FILE]";
    let [recipe, rest @ ..] = args else { return Err(USAGE.into()) };
    let flag = |name: &str| rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned();
    let out = flag("--out").ok_or(USAGE)?;
    let root = crate::blender::repo_root();
    let ledger_path = flag("--ledger").map_or_else(|| root.join("corpus/LEDGER.csv"), std::path::PathBuf::from);
    if !ledger_path.exists() {
        return Err(format!("certificate: licence ledger {} not found", ledger_path.display()));
    }
    let ledger = Ledger::load(&ledger_path)?;
    let code = std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?;
    let scene = match (flag("--scene"), flag("--style")) {
        (None, None) => None,
        (scene, style) => {
            let mut v: Value = match scene {
                Some(p) => serde_json::from_str(std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}"))?,
                None => serde_json::json!({}),
            };
            if let Some(st) = style {
                v["style"] = Value::String(st);
            }
            crate::blender::resolve_style(&mut v)?;
            Some(v)
        }
    };
    let materials = scene.map(|scene| Materials { scene, lib: crate::blender::load_library(&root) });
    let text = certificate(&recipe.replace('\\', "/"), code.trim_start_matches('\u{feff}'), &ledger, materials.as_ref(), &crate::corpus::root())?;
    if let Some(dir) = Path::new(&out).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&out, &text).map_err(|e| format!("{out}: {e}"))?;
    println!("certificate: {recipe} -> {out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RECIPE: &str = "M = \"polyhaven/models/\"\nrock = import_asset(path=M + \"rock_09/rock_09_1k.gltf\")\nground = set_material(grid(size=[10, 10]), name=\"ground_flat\")\nout(merge(rock, ground))\n";
    const ROCK: [&str; 4] = ["polyhaven/models/rock_09/rock_09_1k.gltf", "polyhaven/models/rock_09/rock_09.bin", "polyhaven/models/rock_09/textures/rock_09_diff_1k.jpg", "polyhaven/models/rock_09/textures/rock_09_nor_gl_1k.jpg"];
    const GLTF: &str = r#"{"buffers":[{"uri":"rock_09.bin"}],"images":[{"uri":"textures/rock_09_diff_1k.jpg"},{"uri":"textures/rock_09_nor_gl_1k.jpg"}]}"#;

    fn row(id: &str) -> Vec<String> {
        [id, "polyhaven", "https://example.org/x", "CC0-1.0", "https://creativecommons.org/publicdomain/zero/1.0/", "abc123", "2026-10-10T00:00:00Z", ""].iter().map(|s| s.to_string()).collect()
    }

    fn ledger(ids: &[&str]) -> Ledger {
        Ledger { rows: ids.iter().map(|i| (i.to_string(), row(i))).collect() }
    }

    fn no_corpus() -> std::path::PathBuf {
        std::path::PathBuf::from("this/folder/does/not/exist")
    }

    /// A scene as `resolve_style` leaves it, and a library with one scan, one water and one drawn foliage material.
    fn materials(sets_used: &[(&str, &str)]) -> Materials {
        let roles: serde_json::Map<String, Value> = sets_used.iter().map(|(role, lib)| (role.to_string(), json!({"use": lib}))).collect();
        let lib = json!({"materials": {
            "forest_floor": {"polyhaven": "forest_floor_01"},
            "lost_set": {"polyhaven": "lost_set_77"},
            "water_clear": {"kind": "water", "fallback": [0.0, 0.1, 0.1]},
            "conifer_needles": {"kind": "foliage", "texture": "corpus/generated/needles_a.png", "fallback": [0.0, 0.1, 0.0]}}});
        Materials { scene: json!({"style_name": "test_style", "materials": roles}), lib }
    }

    #[test]
    fn the_report_lists_a_models_bin_and_textures_and_not_other_folders() {
        let all: Vec<&str> = ROCK.iter().copied().chain(["polyhaven/models/other/other.gltf"]).collect();
        let text = certificate("w.hcs", RECIPE, &ledger(&all), None, &no_corpus()).unwrap();
        for f in ROCK {
            assert!(text.contains(&format!("`{f}`")), "{f} is not listed: {text}");
        }
        assert!(!text.contains("other.gltf"), "a file of another model is listed");
        assert_eq!(text, certificate("w.hcs", RECIPE, &ledger(&all), None, &no_corpus()).unwrap(), "not deterministic");
    }

    #[test]
    fn a_gltf_on_disk_that_names_a_file_without_a_row_fails_and_the_report_does_not_depend_on_the_file() {
        let dir = std::env::temp_dir().join("pt_cert_gltf_check");
        let model = dir.join("polyhaven/models/rock_09");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("rock_09_1k.gltf"), GLTF).unwrap();
        // the textures are in the folder's ledger rows except one the gltf names
        let partial = ledger(&[ROCK[0], ROCK[1], ROCK[2]]);
        let e = certificate("w.hcs", RECIPE, &partial, None, &dir).unwrap_err();
        assert!(e.contains("rock_09_nor_gl_1k.jpg"), "{e}");
        let full = ledger(&ROCK);
        let with_file = certificate("w.hcs", RECIPE, &full, None, &dir).unwrap();
        assert_eq!(with_file, certificate("w.hcs", RECIPE, &full, None, &no_corpus()).unwrap(), "the report depends on the file being present");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_asset_missing_from_the_ledger_fails_and_is_named() {
        let e = certificate("w.hcs", RECIPE, &ledger(&["polyhaven/models/other/other_1k.gltf"]), None, &no_corpus()).unwrap_err();
        assert!(e.contains("polyhaven/models/rock_09/rock_09_1k.gltf"), "{e}");
    }

    #[test]
    fn a_style_material_set_missing_from_the_ledger_fails_and_is_named() {
        let m = materials(&[("ground_flat", "forest_floor"), ("ground_lost", "lost_set")]);
        let l = ledger(&[ROCK[0], "polyhaven/materials/forest_floor_01/forest_floor_01_diff_1k.jpg"]);
        let e = certificate("w.hcs", RECIPE, &l, Some(&m), &no_corpus()).unwrap_err();
        assert!(e.contains("lost_set_77") && !e.contains("forest_floor_01"), "{e}");
        // the same style with the set in the ledger passes
        let l = ledger(&[ROCK[0], "polyhaven/materials/forest_floor_01/forest_floor_01_diff_1k.jpg", "polyhaven/materials/lost_set_77/lost_set_77_diff_1k.jpg"]);
        assert!(certificate("w.hcs", RECIPE, &l, Some(&m), &no_corpus()).is_ok());
    }

    #[test]
    fn a_world_whose_files_are_all_in_the_ledger_passes_with_every_material_file_listed() {
        let m = materials(&[("ground_flat", "forest_floor"), ("stream_water", "water_clear"), ("needles", "conifer_needles")]);
        let mut ids: Vec<&str> = ROCK.to_vec();
        ids.extend(["polyhaven/materials/forest_floor_01/forest_floor_01_diff_1k.jpg", "polyhaven/materials/forest_floor_01/forest_floor_01_nor_gl_1k.jpg", "polyhaven/materials/unused_02/unused_02_diff_1k.jpg"]);
        let text = certificate("w.hcs", RECIPE, &ledger(&ids), Some(&m), &no_corpus()).unwrap();
        assert!(text.contains("forest_floor_01_diff_1k.jpg") && text.contains("forest_floor_01_nor_gl_1k.jpg") && text.contains("material file"), "{text}");
        assert!(!text.contains("unused_02"), "a set no material uses is listed");
        assert!(text.contains("style `test_style`") && text.contains("needles_a.png") && text.contains("Everything else in this world is made by the recipe's rules"), "{text}");
    }

    fn world_with(scene_materials: Value, lib_materials: Value) -> Materials {
        Materials { scene: json!({"materials": scene_materials}), lib: json!({"materials": lib_materials}) }
    }

    #[test]
    fn a_style_entry_with_its_own_polyhaven_set_is_followed_and_wins_over_the_library() {
        // one entry overrides the library's set, one has no library match at all; neither is reached by a library-only lookup
        let m = world_with(json!({"ground_flat": {"use": "forest_floor", "polyhaven": "own_set_01"}, "ground_lost": {"polyhaven": "lost_set_77"}}), json!({"forest_floor": {"polyhaven": "forest_floor_01"}}));
        let l = ledger(&[ROCK[0], "polyhaven/materials/own_set_01/own_set_01_diff_1k.jpg"]);
        let e = certificate("w.hcs", RECIPE, &l, Some(&m), &no_corpus()).unwrap_err();
        assert!(e.contains("lost_set_77") && !e.contains("own_set_01") && !e.contains("forest_floor_01"), "{e}");
        let l = ledger(&[ROCK[0], "polyhaven/materials/own_set_01/own_set_01_diff_1k.jpg", "polyhaven/materials/lost_set_77/lost_set_77_diff_1k.jpg", "polyhaven/materials/forest_floor_01/forest_floor_01_diff_1k.jpg"]);
        let text = certificate("w.hcs", RECIPE, &l, Some(&m), &no_corpus()).unwrap();
        assert!(text.contains("own_set_01_diff_1k.jpg") && text.contains("lost_set_77_diff_1k.jpg"), "{text}");
        assert!(!text.contains("forest_floor_01"), "the library's set is overridden, the render does not read it: {text}");
    }

    #[test]
    fn layers_are_followed_one_level_and_nested_and_a_cycle_ends() {
        let m = world_with(
            json!({"ground_flat": {"layers": ["a", "b"]}, "b": {"layers": ["c"]}, "loop_a": {"layers": ["loop_b"]}, "loop_b": {"layers": ["loop_a", "a"]}}),
            json!({"a": {"polyhaven": "set_a"}, "c": {"polyhaven": "set_c"}}),
        );
        let l = ledger(&[ROCK[0], "polyhaven/materials/set_a/set_a_diff_1k.jpg", "polyhaven/materials/set_c/set_c_diff_1k.jpg"]);
        let text = certificate("w.hcs", RECIPE, &l, Some(&m), &no_corpus()).unwrap();
        assert!(text.contains("set_a_diff_1k.jpg") && text.contains("set_c_diff_1k.jpg"), "{text}");
        let e = certificate("w.hcs", RECIPE, &ledger(&[ROCK[0], "polyhaven/materials/set_a/set_a_diff_1k.jpg"]), Some(&m), &no_corpus()).unwrap_err();
        assert!(e.contains("set_c"), "a nested layer's set must be named: {e}");
    }

    #[test]
    fn a_foliage_picture_outside_the_generated_folder_needs_a_row() {
        let m = world_with(json!({}), json!({"leaf": {"kind": "foliage", "texture": "polyhaven/textures/leaf.png", "fallback": [0.0, 0.1, 0.0]}}));
        let code = "out(set_material(grid(), name=\"leaf\"))\n";
        assert!(certificate("w.hcs", code, &ledger(&[]), Some(&m), &no_corpus()).unwrap_err().contains("polyhaven/textures/leaf.png"));
        assert!(certificate("w.hcs", code, &ledger(&["polyhaven/textures/leaf.png"]), Some(&m), &no_corpus()).unwrap().contains("leaf.png"));
    }

    #[test]
    fn without_a_scene_or_style_materials_are_not_certified_and_no_completeness_is_claimed() {
        let text = certificate("w.hcs", RECIPE, &ledger(&ROCK), None, &no_corpus()).unwrap();
        assert!(text.contains("Materials were not certified") && text.contains("makes no claim"), "{text}");
        assert!(!text.contains("Everything else in this world is made by the recipe's rules"), "{text}");
    }

    #[test]
    fn a_dem_lists_its_tiles_and_a_recipe_with_no_assets_says_so() {
        let none = certificate("w.hcs", "out(grid())\n", &ledger(&[]), None, &no_corpus()).unwrap();
        assert!(none.contains("Files listed: 0"));
        let r = recipe_reads("out(dem(zoom=11, lat=43.7767, lon=-110.9028, size=[2000.0, 2000.0], rows=8, cols=8))\n").unwrap();
        assert!(!r.tiles.is_empty() && r.tiles.iter().all(|t| t.starts_with("dem/terrarium/11/")), "{r:?}");
    }

    #[test]
    fn gltf_names_are_decoded_and_kept_inside_the_corpus() {
        assert_eq!(join_uri("a/b", &decode_uri("my%20tex/x.jpg")).as_deref(), Some("a/b/my tex/x.jpg"));
        assert_eq!(join_uri("a/b", "../c.bin").as_deref(), Some("a/c.bin"));
        assert_eq!(join_uri("a", "../../c.bin"), None);
    }
}
