//! Asset import: bring a professional glTF/GLB (a CC0 scan or kit piece from the corpus) into a graph as geometry that
//! keeps its UVs, normals and AUTHORED materials, so it can be scattered like any other prototype and still render
//! and export with its own textures.

use crate::{ps, sch};
use pt_core::gltf::{corpus_root, load_with, material_identifier, LoadOpts};
use pt_core::{Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch(
            "lod_hint",
            "Tell the exporters how a prototype is used at scale (put it on the prototype, before copy_to_points). `cull` (metres, 0 = by size): how far it stays visible in an engine: a stand-in for a distant forest must be seen for kilometres, where a prop of its size would be dropped at a few hundred metres. `merge` = 1: an offline render gets ONE merged mesh of all its instances instead of an object per instance (hundreds of thousands of tiny objects stall a renderer's importer), while an engine still gets instanced tiles it can cull.",
            1,
            D::None,
            G::Whole,
            vec![ps("cull", json!(0.0), "Draw distance in metres in an engine (0 = from the prototype's size)."), ps("merge", json!(0), "1 = offline renders get one merged mesh of all instances.")],
        ),
        sch(
        "import_asset",
        "Read a glTF/GLB from the corpus (a CC0 asset with a ledger row) as geometry with UVs, normals and its authored materials. Materials become prim groups `mat:<name>__<material>` and are written back with their textures on export, so scattering it with copy_to_points keeps the real look. Set `height` to fit it to a size in metres; `ground` puts its lowest point on y=0 and centres it on x/z.",
        0,
        D::None,
        G::Whole,
        vec![
            ps("path", json!(""), "Path relative to the corpus root (PT_CORPUS, default D:\\Fekri\\corpus), e.g. polyhaven/models/rock_09/rock_09_1k.gltf."),
            ps("name", json!(""), "Prefix for the material names (default: the file name without extension)."),
            ps("height", json!(0.0), "If > 0, scale uniformly so the asset is this tall (Y extent) in metres; 0 keeps the file's own scale."),
            ps("part", json!(""), "Only the glTF nodes whose name contains one of these words (comma separated), e.g. \"fern_02_b\". Poly Haven plants and rock sets ship several variants side by side in one file; pick one per import."),
            ps("size", json!(0.0), "If > 0 (and height is 0), scale uniformly so the LARGEST extent is this many metres (for lying logs, wide rocks)."),
            ps("scale", json!(1.0), "Extra uniform scale applied after `height`."),
            ps("ground", json!(1), "1 = lowest point to y=0 and centre on x/z; 0 = keep the file's origin."),
            ps("materials", json!(1), "1 = keep authored materials and UVs; 0 = geometry only."),
            ps("max_triangles", json!(0), "If > 0 and the asset has more triangles, reduce it to about this many: solid parts by edge collapse that respects UVs and normals, leaf and needle cards by keeping a share of them and enlarging the rest so the canopy stays as full. Scanned trees are millions of triangles each; a forest of them does not fit in memory otherwise. The reduced asset is cached on disk."),
        ],
        ),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    match kind {
        "import_asset" => Some(import_asset(p)),
        "lod_hint" => Some(inputs.first().and_then(|g| g.as_deref()).ok_or_else(|| "lod_hint: input 0 is not connected".to_string()).map(|g| {
            let mut out = g.clone();
            out.detail.insert(pt_core::lod::LOD_HINT.into(), json!({"cull_m": p.f("cull").max(0.0), "merge": p.i("merge") != 0}));
            out
        })),
        _ => None,
    }
}

fn import_asset(p: &Params) -> Res {
    let rel = p.s("path");
    if rel.is_empty() {
        return Err("import_asset: set `path` to a file under the corpus root, e.g. polyhaven/models/rock_09/rock_09_1k.gltf".into());
    }
    if rel.contains("..") {
        return Err("import_asset: `path` may not contain '..'".into());
    }
    let full = corpus_root().join(&rel);
    let bytes = std::fs::read(&full).map_err(|e| format!("import_asset: {}: {e} (is the asset fetched? `pt corpus fetch`, root {})", full.display(), corpus_root().display()))?;
    let dir = full.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let mut stem = std::path::Path::new(&rel).file_stem().map_or_else(|| "asset".to_string(), |s| s.to_string_lossy().into_owned());
    for res in ["_1k", "_2k", "_4k", "_8k"] {
        if let Some(s) = stem.strip_suffix(res) {
            stem = s.to_string(); // "rock_09_1k.gltf" -> material prefix "rock_09"
            break;
        }
    }
    let prefix = material_identifier(&if p.s("name").is_empty() { stem } else { p.s("name") });
    let keep_materials = p.i("materials") != 0;
    let scene = load_with(
        &bytes,
        &|uri| if uri.contains("..") { Err(format!("refusing external buffer path '{uri}'")) } else { std::fs::read(dir.join(uri)).map_err(|e| format!("{uri}: {e}")) },
        &LoadOpts { prefix: &prefix, source: &rel.replace('\\', "/") },
    )
    .map_err(|e| format!("import_asset: {rel}: {e}"))?;
    let want: Vec<String> = p.s("part").split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
    let mut g = Geometry::new();
    for part in scene.parts.iter().filter(|part| want.is_empty() || want.iter().any(|w| part.name.contains(w.as_str()))) {
        g.append(&part.geo)?;
    }
    if g.n_prims() == 0 && !scene.parts.is_empty() {
        let names: Vec<&str> = scene.parts.iter().map(|part| part.name.as_str()).collect();
        return Err(format!("import_asset: no part of {rel} matches '{}' (parts: {})", p.s("part"), names.join(", ")));
    }
    if g.n_prims() == 0 {
        return Err(format!("import_asset: {rel} has no triangles"));
    }
    if !keep_materials {
        g.prim_groups.retain(|k, _| !k.starts_with(pt_core::geo::MATERIAL_PREFIX));
        g.detail.retain(|k, _| !k.starts_with(pt_core::gltf::ASSET_MATERIAL_PREFIX));
        g.point.remove("uv");
    }
    let cap = p.i("max_triangles").max(0) as usize;
    if cap > 0 && g.triangles().len() > cap {
        let policy = pt_core::lod::LodPolicy { ratios: vec![1.0], switch_factors: vec![0.0], max_level0_triangles: cap, min_triangles: 0, cache_dir: Some(corpus_root().join("_cache").join("lod")), ..Default::default() };
        if let Some(level) = pt_core::lod::lod_chain(&g, &policy).into_iter().next() {
            g = (*level.geo).clone();
        }
    }
    let (lo, hi) = g.base_bounds().ok_or("import_asset: empty geometry")?;
    let (height, size) = (p.f("height"), p.f("size"));
    let longest = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]);
    let fit = if height > 0.0 && hi[1] > lo[1] {
        height / (hi[1] - lo[1])
    } else if size > 0.0 && longest > 0.0 {
        size / longest
    } else {
        1.0
    };
    let s = fit * if p.f("scale") > 0.0 { p.f("scale") } else { 1.0 };
    let (cx, cz, base) = if p.i("ground") != 0 { ((lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5, lo[1]) } else { (0.0, 0.0, 0.0) };
    if s != 1.0 || cx != 0.0 || cz != 0.0 || base != 0.0 {
        for q in g.p_mut() {
            *q = [(q[0] - cx) * s, (q[1] - base) * s, (q[2] - cz) * s];
        }
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle glTF with UVs, a normal and a named material that has a texture reference, written to a temp corpus.
    fn tiny_asset(dir: &std::path::Path) {
        let mut buf: Vec<u8> = Vec::new();
        for v in [[0.0f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 4.0, 0.0]] {
            buf.extend(v.iter().flat_map(|c| c.to_le_bytes()));
        }
        for v in [[0.0f32, 0.0, 1.0]; 3] {
            buf.extend(v.iter().flat_map(|c| c.to_le_bytes()));
        }
        for v in [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]] {
            buf.extend(v.iter().flat_map(|c| c.to_le_bytes()));
        }
        for i in [0u16, 1, 2] {
            buf.extend(i.to_le_bytes());
        }
        buf.extend([0, 0]);
        std::fs::create_dir_all(dir.join("tex")).unwrap();
        std::fs::write(dir.join("tex/c.png"), b"\x89PNG fake image bytes").unwrap();
        let uri = format!("data:application/octet-stream;base64,{}", pt_core::io::base64_encode(&buf));
        let doc = json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"name": "t", "mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3, "material": 0}]}],
            "materials": [{"name": "Moss Rock", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "roughnessFactor": 0.9}, "alphaMode": "MASK", "doubleSided": true}],
            "textures": [{"source": 0, "sampler": 0}], "images": [{"uri": "tex/c.png"}], "samplers": [{"magFilter": 9729, "minFilter": 9987}],
            "buffers": [{"byteLength": buf.len(), "uri": uri}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 36}, {"buffer": 0, "byteOffset": 72, "byteLength": 24}, {"buffer": 0, "byteOffset": 96, "byteLength": 6}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}, {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2"}, {"bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR"}],
        });
        std::fs::write(dir.join("asset.gltf"), doc.to_string()).unwrap();
    }

    #[test]
    fn imports_with_uv_normals_and_authored_material_and_writes_the_texture_back() {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("pt_asset_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        tiny_asset(&dir);
        std::env::set_var("PT_CORPUS", dir.parent().unwrap());
        let rel = format!("{}/asset.gltf", dir.file_name().unwrap().to_string_lossy());
        let mut params = Params::new();
        params.insert("path".into(), json!(rel));
        params.insert("height".into(), json!(2.0));
        params.insert("ground".into(), json!(1));
        params.insert("materials".into(), json!(1));
        params.insert("name".into(), json!(""));
        params.insert("scale".into(), json!(1.0));
        let g = import_asset(&params).unwrap();
        // height 4 -> 2: scale 0.5, grounded and centred on x
        let (lo, hi) = g.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 2.0).abs() < 1e-5 && lo[1].abs() < 1e-6 && (lo[0] + hi[0]).abs() < 1e-5, "{lo:?} {hi:?}");
        assert_eq!(g.point.get("uv").map(|c| c.len()), Some(3));
        assert_eq!(g.point.get("N").and_then(|c| c.as_v3()).unwrap()[0], [0.0, 0.0, 1.0]);
        assert_eq!(g.material_names(), ["asset__Moss_Rock"], "prefix = the file stem, then the authored name");
        assert!(g.detail.contains_key("material:asset__Moss_Rock"));
        // exporting copies the authored material: texture, sampler, alpha mode
        let (doc, bin) = pt_core::gltf::build(&g, 10);
        let m = &doc["materials"][0];
        assert_eq!(m["name"], "asset__Moss_Rock");
        assert_eq!(m["alphaMode"], "MASK");
        assert_eq!(m["doubleSided"], true);
        assert_eq!(m["pbrMetallicRoughness"]["baseColorTexture"]["index"], 0);
        assert_eq!(doc["images"][0]["mimeType"], "image/png");
        assert_eq!(doc["samplers"][0]["minFilter"], 9987);
        let bv = &doc["bufferViews"][doc["images"][0]["bufferView"].as_u64().unwrap() as usize];
        let (off, len) = (bv["byteOffset"].as_u64().unwrap() as usize, bv["byteLength"].as_u64().unwrap() as usize);
        assert_eq!(&bin[off..off + len], b"\x89PNG fake image bytes");
        let prim = &doc["meshes"][0]["primitives"][0]["attributes"];
        assert!(prim.get("TEXCOORD_0").is_some() && prim.get("NORMAL").is_some());
        // `part` picks glTF nodes by name; a miss lists the names
        params.insert("part".into(), json!("t"));
        assert!(import_asset(&params).is_ok());
        params.insert("part".into(), json!("zzz"));
        assert!(import_asset(&params).unwrap_err().contains("(parts: t)"));
        params.insert("part".into(), json!(""));
        // `materials = 0` strips materials and UVs
        params.insert("materials".into(), json!(0));
        let bare = import_asset(&params).unwrap();
        assert!(bare.material_names().is_empty() && bare.point.get("uv").is_none());
        // a missing file says how to fix it
        params.insert("path".into(), json!("nope/none.gltf"));
        assert!(import_asset(&params).unwrap_err().contains("pt corpus fetch"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use crate::CORPUS_ENV as ENV;
}
