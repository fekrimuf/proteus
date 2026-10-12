//! `pt edit`: record a decision about one placed object in the world's edits file, keyed by its stable id.
//!
//! The owner looks at a frame ("that rock goes", "move the log to the bank", "that fir is too big"); `pt pick` names the object and
//! its `pid`; this writes the decision where the recipe replays it (`apply_edits(points, edits=edits_of(file, scope))`). The world
//! rebuilds with every other object exactly where it was.

use serde_json::{json, Value};
use std::path::Path;

/// Adds (or replaces) one edit in `file` under `scope`. `op` is remove | move | scale | add; `nums` are its numbers: move takes
/// dx dy dz (or `to` with --to), scale one factor, add x y z [pscale]. Returns the edit written.
pub fn record(file: &Path, scope: &str, id: Option<i64>, op: &str, nums: &[f64], absolute: bool) -> Result<Value, String> {
    let mut root: Value = match std::fs::read_to_string(file) {
        Ok(t) => serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", file.display()))?,
        Err(_) => json!({}),
    };
    let edit = match (op, nums) {
        ("remove", []) => json!({"id": id.ok_or("remove needs an id")?, "op": "remove"}),
        ("move", [x, y, z]) => json!({"id": id.ok_or("move needs an id")?, "op": "move", if absolute { "to" } else { "by" }: [x, y, z]}),
        ("scale", [f]) => json!({"id": id.ok_or("scale needs an id")?, "op": "scale", "by": f}),
        ("add", [x, y, z]) => json!({"op": "add", "at": [x, y, z], "id": id.unwrap_or(0)}),
        ("add", [x, y, z, s]) => json!({"op": "add", "at": [x, y, z], "pscale": s, "id": id.unwrap_or(0)}),
        ("remove", _) => return Err("remove takes no numbers".into()),
        ("move", _) => return Err("move takes three numbers: dx dy dz (with --to: the new x y z)".into()),
        ("scale", _) => return Err("scale takes one number: the factor".into()),
        ("add", _) => return Err("add takes x y z and optionally pscale".into()),
        (o, _) => return Err(format!("unknown op '{o}' (remove | move | scale | add)")),
    };
    let obj = root.as_object_mut().ok_or("the edits file must be a JSON object: {\"scope\": [edits]}")?;
    let list = obj.entry(scope.to_string()).or_insert_with(|| json!([]));
    let list = list.as_array_mut().ok_or_else(|| format!("scope '{scope}' in the edits file is not a list"))?;
    // a later decision about the same object and the same kind of change replaces the earlier one
    list.retain(|e| !(op != "add" && e.get("id") == edit.get("id") && e.get("op") == edit.get("op")) && !(op == "remove" && e.get("id") == edit.get("id")));
    list.push(edit.clone());
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, serde_json::to_string_pretty(&root).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(edit)
}

/// `pt edit <edits.json> <scope> <id> remove | move DX DY DZ [--to] | scale F | add X Y Z [PSCALE]`
pub fn edit_cmd(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: pt edit <edits.json> <scope> <id> remove | move DX DY DZ [--to] | scale FACTOR   or   pt edit <edits.json> <scope> add X Y Z [PSCALE]";
    let [file, scope, rest @ ..] = args else { return Err(USAGE.into()) };
    let absolute = rest.iter().any(|a| a == "--to");
    let rest: Vec<&String> = rest.iter().filter(|a| *a != "--to").collect();
    let (id, op, nums_from) = match rest.as_slice() {
        [op, ..] if op.as_str() == "add" => (None, op.as_str(), 1),
        [id, op, ..] => (Some(id.parse::<i64>().map_err(|_| USAGE.to_string())?), op.as_str(), 2),
        _ => return Err(USAGE.into()),
    };
    let nums: Vec<f64> = rest[nums_from..].iter().map(|s| s.parse::<f64>().map_err(|_| USAGE.to_string())).collect::<Result<_, _>>()?;
    let e = record(Path::new(file), scope, id, op, &nums, absolute)?;
    println!("{file} [{scope}]: {}", e);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_are_recorded_by_id_and_replaced_by_later_ones() {
        let dir = std::env::temp_dir().join(format!("pt_edits_test_{}", std::process::id()));
        let f = dir.join("world.json");
        let _ = std::fs::remove_dir_all(&dir);
        record(&f, "rocks", Some(77), "scale", &[1.5], false).unwrap();
        record(&f, "rocks", Some(77), "scale", &[2.0], false).unwrap();
        record(&f, "rocks", Some(78), "remove", &[], false).unwrap();
        record(&f, "trees", Some(5), "move", &[1.0, 0.0, -2.0], false).unwrap();
        record(&f, "trees", None, "add", &[3.0, 0.0, 4.0, 1.2], false).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["rocks"].as_array().unwrap().len(), 2, "the second scale replaced the first: {v}");
        assert_eq!(v["rocks"][0]["by"], 2.0);
        assert_eq!(v["trees"][0]["by"], json!([1.0, 0.0, -2.0]));
        assert_eq!(v["trees"][1]["op"], "add");
        assert!(record(&f, "rocks", Some(1), "teleport", &[], false).unwrap_err().contains("unknown op"));
        assert!(record(&f, "rocks", Some(1), "move", &[1.0], false).unwrap_err().contains("three numbers"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
