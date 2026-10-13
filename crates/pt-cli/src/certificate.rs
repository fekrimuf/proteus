//! `pt certificate`: the clean-title certificate of a world (ROUTE.md row 15).
//!
//! It reads the recipe text and `corpus/LEDGER.csv` and nothing else: the recipe is run into a graph (no cooking, so no asset file,
//! no GPU and no Blender is needed), every `import_asset` file and every `dem` elevation tile the graph names is looked up in the
//! ledger, and a Markdown report lists each with its source, licence and hash. An asset the ledger does not hold fails the command
//! (exit code 1) and is named; no report is written then, so a certificate that exists is a certificate that is complete.
//! The report has no date or machine detail, so the same recipe and ledger always give the same file.

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
        summary: "Write the clean-title certificate of a world: every asset file and elevation tile its recipe uses, with source, licence and hash from corpus/LEDGER.csv, and a line stating that everything else is made by the recipe's rules. Works from the recipe text and the ledger alone (no asset files, GPU or Blender). Fails with exit code 1, naming them, when an asset is missing from the ledger.",
        heavy: false,
        params: &[
            Param::pos("recipe", Kind::Str, "the world's recipe .hcs file"),
            Param::req_flag("out", Kind::Str, "the Markdown file to write"),
            Param::flag("ledger", Kind::Str, "the licence ledger (default corpus/LEDGER.csv of this repository)"),
        ],
        run: certificate_cmd,
    }
};

/// One asset the recipe uses, with the first node that names it.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Used {
    pub id: String,
    pub kind: &'static str,
}

/// The ledger ids of everything a recipe reads from the corpus, found without cooking: the files of its `import_asset` nodes and
/// the elevation tiles each `dem` node reads (the same tile list `pt dem-manifest` writes).
pub fn assets_used(code: &str) -> Result<Vec<Used>, String> {
    let mut g = Graph::new();
    pt_hcs::run(code, &mut g, true).map_err(|d| d.first().map_or_else(|| "recipe failed".to_string(), |d| d.to_string()))?;
    let mut set: BTreeSet<Used> = BTreeSet::new();
    for (name, n) in &g.nodes {
        match n.kind.as_str() {
            "import_asset" => {
                let path = n.params.s("path").replace('\\', "/");
                if path.is_empty() {
                    return Err(format!("node {name}: import_asset has no `path`, so its source cannot be certified"));
                }
                set.insert(Used { id: path, kind: "model" });
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
                            set.insert(Used { id: format!("{tiles}/{z}/{x}/{y}.png"), kind: "elevation tile" });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(set.into_iter().collect())
}

/// The certificate text, or the ids missing from the ledger.
pub fn certificate(recipe_name: &str, code: &str, ledger: &Ledger) -> Result<String, String> {
    let used = assets_used(code)?;
    let missing: Vec<&str> = used.iter().filter(|u| !ledger.rows.contains_key(&u.id)).map(|u| u.id.as_str()).collect();
    if !missing.is_empty() {
        let shown = missing.iter().take(12).copied().collect::<Vec<_>>().join(", ");
        let more = if missing.len() > 12 { format!(" and {} more", missing.len() - 12) } else { String::new() };
        return Err(format!("certificate: {} asset(s) used by {recipe_name} are missing from the licence ledger: {shown}{more}", missing.len()));
    }
    let mut s = format!("# Clean-title certificate\n\nWorld recipe: `{recipe_name}` (sha256 `{}`)\n\n", sha256_hex(code.as_bytes()));
    s.push_str(&format!("Assets used: {} (every one is listed in the licence ledger, `corpus/LEDGER.csv`).\n\n", used.len()));
    if !used.is_empty() {
        s.push_str("| Asset | Kind | Source | Licence | sha256 | Origin |\n|---|---|---|---|---|---|\n");
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
    s.push_str("Everything else in this world is made by the recipe's rules: no other model, texture, picture or elevation file is read.\n");
    Ok(s)
}

/// `pt certificate <recipe.hcs> --out FILE [--ledger FILE]`
pub fn certificate_cmd(args: &[String]) -> Result<(), String> {
    let [recipe, rest @ ..] = args else { return Err("usage: pt certificate <recipe.hcs> --out FILE [--ledger FILE]".into()) };
    let flag = |name: &str| rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned();
    let out = flag("--out").ok_or("usage: pt certificate <recipe.hcs> --out FILE [--ledger FILE]")?;
    let ledger_path = flag("--ledger").map_or_else(|| crate::blender::repo_root().join("corpus/LEDGER.csv"), std::path::PathBuf::from);
    if !ledger_path.exists() {
        return Err(format!("certificate: licence ledger {} not found", ledger_path.display()));
    }
    let ledger = Ledger::load(&ledger_path)?;
    let code = std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?;
    let text = certificate(&recipe.replace('\\', "/"), code.trim_start_matches('\u{feff}'), &ledger)?;
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

    const RECIPE: &str = "M = \"polyhaven/models/\"\nrock = import_asset(path=M + \"rock_09/rock_09_1k.gltf\")\nground = grid(size=[10, 10])\nout(merge(rock, ground))\n";

    fn row(id: &str) -> Vec<String> {
        [id, "polyhaven", "https://example.org/x", "CC0-1.0", "https://creativecommons.org/publicdomain/zero/1.0/", "abc123", "2026-10-10T00:00:00Z", ""].iter().map(|s| s.to_string()).collect()
    }

    fn ledger(ids: &[&str]) -> Ledger {
        Ledger { rows: ids.iter().map(|i| (i.to_string(), row(i))).collect() }
    }

    #[test]
    fn a_world_whose_assets_are_all_in_the_ledger_gets_a_certificate() {
        let text = certificate("w.hcs", RECIPE, &ledger(&["polyhaven/models/rock_09/rock_09_1k.gltf", "unrelated/other.png"])).unwrap();
        assert!(text.contains("`polyhaven/models/rock_09/rock_09_1k.gltf`") && text.contains("CC0-1.0") && text.contains("`abc123`"), "{text}");
        assert!(!text.contains("unrelated/other.png"), "an asset the recipe does not use is listed");
        assert!(text.contains("Everything else in this world is made by the recipe's rules"));
        assert_eq!(text, certificate("w.hcs", RECIPE, &ledger(&["polyhaven/models/rock_09/rock_09_1k.gltf"])).unwrap(), "not deterministic");
    }

    #[test]
    fn an_asset_missing_from_the_ledger_fails_and_is_named() {
        let e = certificate("w.hcs", RECIPE, &ledger(&["polyhaven/models/other/other_1k.gltf"])).unwrap_err();
        assert!(e.contains("polyhaven/models/rock_09/rock_09_1k.gltf"), "{e}");
    }

    #[test]
    fn a_recipe_with_no_assets_says_so_and_a_dem_lists_its_tiles() {
        let none = certificate("w.hcs", "out(grid())\n", &ledger(&[])).unwrap();
        assert!(none.contains("Assets used: 0"));
        let used = assets_used("out(dem(zoom=11, lat=43.7767, lon=-110.9028, size=[2000.0, 2000.0], rows=8, cols=8))\n").unwrap();
        assert!(!used.is_empty() && used.iter().all(|u| u.id.starts_with("dem/terrarium/11/") && u.kind == "elevation tile"), "{used:?}");
    }
}
