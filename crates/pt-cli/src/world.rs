//! `pt world`: one file that names a world's layers, and variants that change its look and its layout without touching the recipe.
//!
//! A world is made of layers that used to be spread over four command lines: the BASE (a recipe), the SCENE (cameras, light), a
//! STYLE (materials, light, render settings and KNOBS the recipe asks for with `knob(name, default)`) and the OWNER layer (the
//! decisions about single objects, `edits/*.json`, replayed by `apply_edits`). A variant picks a style, adds knobs and can point
//! at another owner layer, so one recipe serves many looks and many studios: the same creek on a grey day with a thicker forest
//! and more fallen wood is a variant, not a copy of the recipe.
//!
//! ```json
//! { "name": "cascade", "recipe": "scenes/cascade.hcs", "scene": "scenes/cascade.scene.json", "default_variant": "evening",
//!   "variants": { "evening": {"style": "styles/cascade_evening.json"},
//!                 "overcast": {"style": "styles/cascade_overcast.json", "knobs": {"log_density": 3.0}, "edits": "edits/cascade_grey.json"} } }
//! ```
//!
//! `pt world <manifest> info|scene|render|check|pick [--variant NAME] ...` expands the variant into a scene file under `out/world/`
//! and runs the usual command on it, so every tool sees the same world.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A world manifest read from disk.
pub struct Manifest {
    pub name: String,
    pub recipe: String,
    pub scene: String,
    pub default_variant: Option<String>,
    pub variants: Value,
}

pub fn read_manifest(path: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display()))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(String::from).ok_or_else(|| format!("{}: missing \"{k}\" (a path as a string)", path.display()));
    let name = v.get("name").and_then(Value::as_str).map(String::from).or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "world".into());
    let allowed = ["name", "note", "recipe", "scene", "default_variant", "variants"];
    if let Some(bad) = v.as_object().and_then(|o| o.keys().find(|k| !allowed.contains(&k.as_str()))) {
        return Err(format!("{}: unknown key \"{bad}\" (allowed: {allowed:?})", path.display()));
    }
    Ok(Manifest { name, recipe: s("recipe")?, scene: s("scene")?, default_variant: v.get("default_variant").and_then(Value::as_str).map(String::from), variants: v.get("variants").cloned().unwrap_or_else(|| json!({})) })
}

/// The scene a variant makes: the base scene file with the variant's style, its knobs on top of the scene's own, and its owner layer
/// (as the knob `edits`). Pure: the file is written by the caller.
pub fn expand(base_scene: &Value, manifest: &Manifest, variant: Option<&str>) -> Result<(String, Value), String> {
    let name = variant.map(String::from).or_else(|| manifest.default_variant.clone());
    let mut scene = base_scene.clone();
    let Some(name) = name else { return Ok(("base".into(), scene)) };
    let Some(v) = manifest.variants.get(&name) else {
        let have: Vec<&String> = manifest.variants.as_object().map(|o| o.keys().collect()).unwrap_or_default();
        return Err(format!("world '{}' has no variant '{name}' (have: {have:?})", manifest.name));
    };
    let obj = scene.as_object_mut().ok_or("the scene file must be a JSON object")?;
    if let Some(style) = v.get("style") {
        obj.insert("style".into(), style.clone());
    }
    let mut knobs = obj.get("knobs").and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(k) = v.get("knobs").and_then(Value::as_object) {
        knobs.extend(k.iter().map(|(a, b)| (a.clone(), b.clone())));
    }
    if let Some(e) = v.get("edits") {
        knobs.insert("edits".into(), e.clone());
    }
    if !knobs.is_empty() {
        obj.insert("knobs".into(), Value::Object(knobs));
    }
    Ok((name, scene))
}

/// `pt world <manifest> info | scene | render OUT.png [flags] | check [flags] | pick CAMERA X Y [flags]` with `--variant NAME` anywhere.
pub fn world_cmd(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: pt world <manifest.json> info|scene|render OUT.png [render flags]|check [flags]|pick CAMERA X Y [flags] [--variant NAME]";
    let [manifest_path, command, rest @ ..] = args else { return Err(USAGE.into()) };
    let mut rest: Vec<String> = rest.to_vec();
    let variant = rest.iter().position(|a| a == "--variant").map(|i| {
        let v = rest.get(i + 1).cloned();
        rest.drain(i..(i + 2).min(rest.len()));
        v
    });
    let variant = variant.flatten();
    let m = read_manifest(Path::new(manifest_path))?;
    let base: Value = serde_json::from_str(std::fs::read_to_string(&m.scene).map_err(|e| format!("{}: {e}", m.scene))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", m.scene))?;
    let (vname, scene) = expand(&base, &m, variant.as_deref())?;
    let dir = PathBuf::from("out/world");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let scene_path = dir.join(format!("{}.{vname}.scene.json", m.name));
    std::fs::write(&scene_path, serde_json::to_string_pretty(&scene).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let scene_arg = scene_path.to_string_lossy().into_owned();
    match command.as_str() {
        "info" => {
            println!("{}", serde_json::to_string_pretty(&json!({"world": m.name, "variant": vname, "recipe": m.recipe, "scene_file": scene_arg, "variants": m.variants.as_object().map(|o| o.keys().collect::<Vec<_>>()), "style": scene.get("style"), "knobs": crate::blender::knobs_of(&scene)?})).map_err(|e| e.to_string())?);
            Ok(())
        }
        "scene" => {
            println!("{scene_arg}");
            Ok(())
        }
        "render" => crate::blender::render_scene_cmd(&[vec![m.recipe.clone(), scene_arg], rest].concat()),
        "check" => crate::grounding::ground_check_cmd(&[vec![m.recipe.clone(), scene_arg], rest].concat()),
        "pick" => {
            let (camera_xy, flags) = rest.split_at(rest.len().min(3));
            crate::pick::pick_cmd(&[vec![m.recipe.clone(), scene_arg], camera_xy.to_vec(), flags.to_vec()].concat())
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            name: "w".into(),
            recipe: "r.hcs".into(),
            scene: "s.json".into(),
            default_variant: Some("evening".into()),
            variants: json!({"evening": {"style": "styles/a.json"}, "grey": {"style": "styles/b.json", "knobs": {"forest_density": 1.3}, "edits": "edits/grey.json"}}),
        }
    }

    #[test]
    fn a_variant_swaps_the_style_adds_knobs_and_points_at_its_owner_layer() {
        let base = json!({"style": "styles/a.json", "knobs": {"forest_density": 1.0, "keep": 5}, "cameras": []});
        let (name, s) = expand(&base, &manifest(), Some("grey")).unwrap();
        assert_eq!(name, "grey");
        assert_eq!(s["style"], "styles/b.json");
        assert_eq!(s["knobs"], json!({"forest_density": 1.3, "keep": 5, "edits": "edits/grey.json"}));
        assert_eq!(s["cameras"], json!([]), "everything else of the scene is kept");
        let (dflt, s) = expand(&base, &manifest(), None).unwrap();
        assert_eq!((dflt.as_str(), s["style"].as_str()), ("evening", Some("styles/a.json")));
        let err = expand(&base, &manifest(), Some("night")).unwrap_err();
        assert!(err.contains("no variant 'night'") && err.contains("grey"), "{err}");
    }

    #[test]
    fn the_manifest_is_checked_for_typos_and_missing_paths() {
        let dir = std::env::temp_dir().join(format!("pt_world_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.world.json");
        std::fs::write(&ok, r#"{"recipe": "a.hcs", "scene": "b.json", "variants": {}}"#).unwrap();
        assert_eq!(read_manifest(&ok).unwrap().recipe, "a.hcs");
        let bad = dir.join("bad.world.json");
        std::fs::write(&bad, r#"{"recipe": "a.hcs", "scene": "b.json", "varients": {}}"#).unwrap();
        assert!(read_manifest(&bad).err().unwrap().contains("unknown key \"varients\""));
        let none = dir.join("none.world.json");
        std::fs::write(&none, r#"{"recipe": "a.hcs"}"#).unwrap();
        assert!(read_manifest(&none).err().unwrap().contains("missing \"scene\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
