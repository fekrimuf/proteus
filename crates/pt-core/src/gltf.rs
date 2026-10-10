//! glTF 2.0 / GLB reader (geometry only: positions, triangle indices, node transforms) and a GLB writer.
//!
//! The reader walks the default scene and returns one *part* per node that carries a mesh, with the node's
//! world transform applied. Materials, textures, animation and skins are ignored. Compressed meshes (Draco,
//! meshopt) and sparse accessors are rejected with a clear message instead of being half-read.

use crate::geo::Geometry;
use crate::math::{self, Mat34};
use serde_json::{json, Value};

#[derive(Debug)]
pub struct GltfPart {
    /// Node name (or `node<N>` when the node is unnamed).
    pub name: String,
    pub geo: Geometry,
}

#[derive(Default, Debug)]
pub struct GltfScene {
    pub parts: Vec<GltfPart>,
    /// Non-fatal notes (skipped primitive modes, missing scene, ...).
    pub warnings: Vec<String>,
}

impl GltfScene {
    /// All parts merged into one geometry.
    pub fn merged(&self) -> Geometry {
        let mut g = Geometry::new();
        for p in &self.parts {
            // Parts built here share the same attribute layout (only `P`), so appending cannot fail.
            let _ = g.append(&p.geo);
        }
        g
    }
}

const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

fn le_u32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "truncated GLB".to_string())
}

/// Decode standard base64 (padding optional, whitespace ignored).
pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err(format!("bad base64 character '{}'", c as char)),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Doc {
    j: Value,
    bufs: Vec<Vec<u8>>,
}

struct Acc {
    nc: usize,
    n: usize,
    v: Vec<f64>,
}

fn comp_size(ct: u64) -> Result<usize, String> {
    match ct {
        5120 | 5121 => Ok(1),
        5122 | 5123 => Ok(2),
        5125 | 5126 => Ok(4),
        o => Err(format!("unknown accessor componentType {o}")),
    }
}

fn n_components(t: &str) -> Result<usize, String> {
    Ok(match t {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        o => return Err(format!("unknown accessor type {o}")),
    })
}

impl Doc {
    fn arr(&self, key: &str) -> &[Value] {
        self.j.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }

    fn read_acc(&self, idx: usize) -> Result<Acc, String> {
        let a = self.arr("accessors").get(idx).ok_or_else(|| format!("accessor {idx} does not exist"))?;
        if a.get("sparse").is_some() {
            return Err(format!("accessor {idx} is sparse (not supported)"));
        }
        let ct = a.get("componentType").and_then(Value::as_u64).ok_or("accessor without componentType")?;
        let n = a.get("count").and_then(Value::as_u64).ok_or("accessor without count")? as usize;
        let nc = n_components(a.get("type").and_then(Value::as_str).ok_or("accessor without type")?)?;
        let norm = a.get("normalized").and_then(Value::as_bool).unwrap_or(false);
        let cs = comp_size(ct)?;
        let mut v = vec![0.0f64; n * nc];
        let Some(bv_idx) = a.get("bufferView").and_then(Value::as_u64) else { return Ok(Acc { nc, n, v }) };
        let bv = self.arr("bufferViews").get(bv_idx as usize).ok_or_else(|| format!("bufferView {bv_idx} does not exist"))?;
        let buf = bv.get("buffer").and_then(Value::as_u64).ok_or("bufferView without buffer")? as usize;
        let data = self.bufs.get(buf).ok_or_else(|| format!("buffer {buf} does not exist"))?;
        let base = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize + a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let elem = cs * nc;
        let stride = bv.get("byteStride").and_then(Value::as_u64).map_or(elem, |s| s as usize).max(elem);
        for i in 0..n {
            for k in 0..nc {
                let at = base + i * stride + k * cs;
                let b = data.get(at..at + cs).ok_or_else(|| format!("accessor {idx} reads past the end of its buffer"))?;
                v[i * nc + k] = match ct {
                    5120 => {
                        let x = b[0] as i8 as f64;
                        if norm { (x / 127.0).max(-1.0) } else { x }
                    }
                    5121 => {
                        let x = b[0] as f64;
                        if norm { x / 255.0 } else { x }
                    }
                    5122 => {
                        let x = i16::from_le_bytes([b[0], b[1]]) as f64;
                        if norm { (x / 32767.0).max(-1.0) } else { x }
                    }
                    5123 => {
                        let x = u16::from_le_bytes([b[0], b[1]]) as f64;
                        if norm { x / 65535.0 } else { x }
                    }
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                    _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                };
            }
        }
        Ok(Acc { nc, n, v })
    }
}

fn node_matrix(n: &Value) -> Mat34 {
    let f = |v: &Value, d: f64| v.as_f64().unwrap_or(d) as f32;
    if let Some(m) = n.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16) {
        let mut o = math::IDENTITY;
        for r in 0..3 {
            for c in 0..4 {
                o[r * 4 + c] = f(&m[c * 4 + r], 0.0);
            }
        }
        return o;
    }
    let t = n.get("translation").and_then(Value::as_array).filter(|a| a.len() == 3).map_or([0.0; 3], |a| [f(&a[0], 0.0), f(&a[1], 0.0), f(&a[2], 0.0)]);
    let s = n.get("scale").and_then(Value::as_array).filter(|a| a.len() == 3).map_or([1.0; 3], |a| [f(&a[0], 1.0), f(&a[1], 1.0), f(&a[2], 1.0)]);
    let q = n.get("rotation").and_then(Value::as_array).filter(|a| a.len() == 4).map_or([0.0, 0.0, 0.0, 1.0], |a| [f(&a[0], 0.0), f(&a[1], 0.0), f(&a[2], 0.0), f(&a[3], 1.0)]);
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let (x, y, z, w) = if len > 0.0 { (q[0] / len, q[1] / len, q[2] / len, q[3] / len) } else { (0.0, 0.0, 0.0, 1.0) };
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut o = math::IDENTITY;
    for row in 0..3 {
        for col in 0..3 {
            o[row * 4 + col] = r[row][col] * s[col];
        }
        o[row * 4 + 3] = t[row];
    }
    o
}

fn det3(m: &Mat34) -> f32 {
    m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8]) + m[2] * (m[4] * m[9] - m[5] * m[8])
}

/// Triangles of one primitive according to its draw mode (4 = triangles, 5 = strip, 6 = fan).
fn mode_triangles(mode: u64, idx: &[u32]) -> Option<Vec<[u32; 3]>> {
    Some(match mode {
        4 => idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        5 => (0..idx.len().saturating_sub(2)).map(|i| if i % 2 == 0 { [idx[i], idx[i + 1], idx[i + 2]] } else { [idx[i + 1], idx[i], idx[i + 2]] }).collect(),
        6 => (1..idx.len().saturating_sub(1)).map(|i| [idx[0], idx[i], idx[i + 1]]).collect(),
        _ => return None,
    })
}

/// Parse GLB or `.gltf` bytes. `resolve` loads external buffers by (relative) URI.
pub fn load(bytes: &[u8], resolve: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<GltfScene, String> {
    let (json_bytes, bin): (&[u8], Option<&[u8]>) = if le_u32(bytes, 0).ok() == Some(GLB_MAGIC) {
        if le_u32(bytes, 4)? != 2 {
            return Err("only GLB version 2 is supported".into());
        }
        let total = (le_u32(bytes, 8)? as usize).min(bytes.len());
        let (mut at, mut json, mut bin) = (12usize, None, None);
        while at + 8 <= total {
            let len = le_u32(bytes, at)? as usize;
            let kind = le_u32(bytes, at + 4)?;
            let body = bytes.get(at + 8..at + 8 + len).ok_or("truncated GLB chunk")?;
            match kind {
                CHUNK_JSON if json.is_none() => json = Some(body),
                CHUNK_BIN if bin.is_none() => bin = Some(body),
                _ => {}
            }
            at += 8 + ((len + 3) & !3);
        }
        (json.ok_or("GLB has no JSON chunk")?, bin)
    } else {
        (bytes, None)
    };
    let j: Value = serde_json::from_slice(json_bytes).map_err(|e| format!("glTF JSON: {e}"))?;
    if j.pointer("/asset/version").and_then(Value::as_str).is_some_and(|v| !v.starts_with('2')) {
        return Err("only glTF 2.x is supported".into());
    }
    for ext in j.get("extensionsRequired").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        if ext == "KHR_draco_mesh_compression" || ext == "EXT_meshopt_compression" || ext == "KHR_meshopt_compression" {
            return Err(format!("mesh compression '{ext}' is not supported"));
        }
    }
    let mut bufs = Vec::new();
    for (i, b) in j.get("buffers").and_then(Value::as_array).map_or(&[][..], Vec::as_slice).iter().enumerate() {
        match b.get("uri").and_then(Value::as_str) {
            Some(u) if u.starts_with("data:") => bufs.push(base64_decode(u.split_once(',').ok_or("malformed data URI")?.1)?),
            Some(u) => bufs.push(resolve(&percent_decode(u))?),
            None if i == 0 => bufs.push(bin.ok_or("buffer 0 has no uri and the file has no BIN chunk")?.to_vec()),
            None => return Err(format!("buffer {i} has no uri")),
        }
    }
    let doc = Doc { j, bufs };
    let nodes = doc.arr("nodes");
    let mut scene = GltfScene::default();

    let roots: Vec<usize> = {
        let sc = doc.j.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
        match doc.arr("scenes").get(sc).and_then(|s| s.get("nodes")).and_then(Value::as_array) {
            Some(r) => r.iter().filter_map(Value::as_u64).map(|v| v as usize).collect(),
            None => {
                scene.warnings.push("no scene: using every node that is not a child".into());
                let mut is_child = vec![false; nodes.len()];
                for n in nodes {
                    for c in n.get("children").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64) {
                        if let Some(f) = is_child.get_mut(c as usize) {
                            *f = true;
                        }
                    }
                }
                (0..nodes.len()).filter(|&i| !is_child[i]).collect()
            }
        }
    };

    // Depth-first walk with an explicit stack (parent matrix carried along).
    let mut stack: Vec<(usize, Mat34, usize)> = roots.iter().rev().map(|&r| (r, math::IDENTITY, 0)).collect();
    let mut visited = 0usize;
    while let Some((ni, parent, depth)) = stack.pop() {
        visited += 1;
        if depth > 64 || visited > 1_000_000 {
            return Err("node hierarchy too deep or cyclic".into());
        }
        let Some(node) = nodes.get(ni) else { return Err(format!("node {ni} does not exist")) };
        let world = math::mul34(&parent, &node_matrix(node));
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            for c in children.iter().rev().filter_map(Value::as_u64) {
                stack.push((c as usize, world, depth + 1));
            }
        }
        let Some(mi) = node.get("mesh").and_then(Value::as_u64) else { continue };
        let mesh = doc.arr("meshes").get(mi as usize).ok_or_else(|| format!("mesh {mi} does not exist"))?;
        let flip = det3(&world) < 0.0;
        let (mut pts, mut polys): (Vec<[f32; 3]>, Vec<Vec<u32>>) = (vec![], vec![]);
        for prim in mesh.get("primitives").and_then(Value::as_array).into_iter().flatten() {
            if prim.pointer("/extensions/KHR_draco_mesh_compression").is_some() {
                return Err("mesh compression 'KHR_draco_mesh_compression' is not supported".into());
            }
            let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
            let Some(pa) = prim.pointer("/attributes/POSITION").and_then(Value::as_u64) else {
                scene.warnings.push(format!("node {ni}: primitive without POSITION skipped"));
                continue;
            };
            let pos = doc.read_acc(pa as usize)?;
            if pos.nc != 3 {
                return Err("POSITION accessor must be VEC3".into());
            }
            let idx: Vec<u32> = match prim.get("indices").and_then(Value::as_u64) {
                Some(ia) => {
                    let a = doc.read_acc(ia as usize)?;
                    a.v.iter().map(|&x| x as u32).collect()
                }
                None => (0..pos.n as u32).collect(),
            };
            let Some(tris) = mode_triangles(mode, &idx) else {
                scene.warnings.push(format!("node {ni}: primitive mode {mode} (points/lines) skipped"));
                continue;
            };
            let base = pts.len() as u32;
            for i in 0..pos.n {
                pts.push(math::xform_point(&world, [pos.v[i * 3] as f32, pos.v[i * 3 + 1] as f32, pos.v[i * 3 + 2] as f32]));
            }
            for t in tris {
                if t.iter().any(|&i| i as usize >= pos.n) {
                    return Err(format!("node {ni}: index out of range"));
                }
                let (a, b, c) = (t[0] + base, t[1] + base, t[2] + base);
                polys.push(if flip { vec![a, c, b] } else { vec![a, b, c] });
            }
        }
        if polys.is_empty() {
            continue;
        }
        let name = node.get("name").and_then(Value::as_str).map_or_else(|| format!("node{ni}"), String::from);
        scene.parts.push(GltfPart { name, geo: Geometry::from_polys(pts, &polys) });
    }
    Ok(scene)
}

/// Read a `.glb` / `.gltf` file; external buffers are resolved relative to the file.
pub fn load_file(path: &std::path::Path) -> Result<GltfScene, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    load(&bytes, &|uri| {
        if uri.contains("..") {
            return Err(format!("refusing external buffer path '{uri}'"));
        }
        std::fs::read(dir.join(uri)).map_err(|e| format!("{uri}: {e}"))
    })
}

/// Binary glTF: positions + triangle indices of the realized geometry (no materials).
pub fn to_glb(g: &Geometry) -> Vec<u8> {
    let g = g.realize();
    let tris = g.triangles();
    let mut bin: Vec<u8> = Vec::new();
    for p in g.p() {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    for t in &tris {
        for i in t {
            bin.extend_from_slice(&i.to_le_bytes());
        }
    }
    let idx_len = bin.len() - pos_len;
    let (lo, hi) = g.base_bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    let doc = if tris.is_empty() || g.n_points() == 0 {
        json!({"asset": {"version": "2.0", "generator": "proteus"}, "scene": 0, "scenes": [{"nodes": []}]})
    } else {
        json!({
            "asset": {"version": "2.0", "generator": "proteus"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
            "buffers": [{"byteLength": bin.len()}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": pos_len, "target": 34962},
                {"buffer": 0, "byteOffset": pos_len, "byteLength": idx_len, "target": 34963}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": g.n_points(), "type": "VEC3", "min": lo, "max": hi},
                {"bufferView": 1, "componentType": 5125, "count": tris.len() * 3, "type": "SCALAR"}],
        })
    };
    let mut json_bytes = doc.to_string().into_bytes();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let has_bin = !bin.is_empty() && !tris.is_empty();
    let total = 12 + 8 + json_bytes.len() + if has_bin { 8 + bin.len() } else { 0 };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    if has_bin {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
        out.extend_from_slice(&bin);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_external(_: &str) -> Result<Vec<u8>, String> {
        Err("no external buffers in this test".into())
    }

    fn cube() -> Geometry {
        let p = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]];
        let q = vec![vec![0, 3, 2, 1], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![2, 3, 7, 6], vec![1, 2, 6, 5], vec![0, 4, 7, 3]];
        Geometry::from_polys(p, &q)
    }

    #[test]
    fn glb_round_trip_keeps_triangles_bit_exact() {
        let g = cube();
        let scene = load(&to_glb(&g), &no_external).unwrap();
        assert_eq!(scene.parts.len(), 1);
        let back = &scene.parts[0].geo;
        let expect = Geometry::from_polys(g.p().to_vec(), &g.triangles().iter().map(|t| t.to_vec()).collect::<Vec<_>>());
        assert_eq!(back.content_hash(), expect.content_hash(), "round trip changed the geometry");
    }

    #[test]
    fn empty_geometry_round_trips_to_no_parts() {
        let scene = load(&to_glb(&Geometry::new()), &no_external).unwrap();
        assert!(scene.parts.is_empty());
    }

    /// A hand-written glTF: one triangle mesh used by two nodes (translated, and scaled by -2 on x which flips winding).
    fn tiny_gltf() -> String {
        let mut buf: Vec<u8> = Vec::new();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
        for i in [0u16, 1, 2] {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        buf.extend_from_slice(&[0, 0]); // pad
        let uri = format!("data:application/octet-stream;base64,{}", crate::io::base64_encode(&buf));
        json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0, 2]}],
            "nodes": [
                {"name": "parent", "translation": [10.0, 0.0, 0.0], "children": [1]},
                {"name": "child", "mesh": 0, "translation": [0.0, 5.0, 0.0]},
                {"name": "mirrored", "mesh": 0, "scale": [-2.0, 1.0, 1.0]}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
            "buffers": [{"byteLength": buf.len(), "uri": uri}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 6}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"},
                {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}],
        })
        .to_string()
    }

    #[test]
    fn node_transforms_apply_and_mirroring_flips_winding() {
        let scene = load(tiny_gltf().as_bytes(), &no_external).unwrap();
        assert_eq!(scene.parts.len(), 2);
        let child = scene.parts.iter().find(|p| p.name == "child").unwrap();
        assert_eq!(child.geo.p()[0], [10.0, 5.0, 0.0], "parent translation composes with the child's");
        let mirrored = scene.parts.iter().find(|p| p.name == "mirrored").unwrap();
        assert_eq!(mirrored.geo.p()[1], [-2.0, 0.0, 0.0]);
        assert_eq!(mirrored.geo.prim_points(0), &[0, 2, 1], "negative determinant flips the triangle");
        assert_eq!(scene.merged().n_prims(), 2);
    }

    #[test]
    fn unsupported_files_fail_loudly() {
        let draco = json!({"asset": {"version": "2.0"}, "extensionsRequired": ["KHR_draco_mesh_compression"]}).to_string();
        assert!(load(draco.as_bytes(), &no_external).unwrap_err().contains("compression"));
        assert!(load(b"glTF\x02\x00\x00\x00", &no_external).is_err());
        assert!(load(b"not json", &no_external).is_err());
    }

    #[test]
    fn base64_and_percent_decoding() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode(&crate::io::base64_encode(&[0, 255, 1, 2, 3])).unwrap(), vec![0, 255, 1, 2, 3]);
        assert_eq!(percent_decode("a%20b.bin"), "a b.bin");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
