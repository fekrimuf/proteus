//! glTF 2.0 / GLB reader (geometry only: positions, triangle indices, node transforms) and a GLB writer.
//!
//! The reader walks the default scene and returns one *part* per node that carries a mesh, with the node's
//! world transform applied. Material *names* become prim groups `mat:<name>`; textures, animation and skins are
//! ignored. Compressed meshes (Draco, meshopt) and sparse accessors are rejected with a clear message instead of
//! being half-read.
//!
//! The writer ([`to_glb`], [`build`]) keeps lazy instances as instances, writes one primitive per material
//! with a named glTF material (`extras.proteus_material`), and the point colour `Cd` as `COLOR_0`.

use crate::column::Column;
use crate::geo::{Geometry, MATERIAL_PREFIX};
use crate::math::{self, Mat34};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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

/// A foreign material name ("Wood Floor.001", "lambert1") as a valid material identifier.
pub fn material_identifier(n: &str) -> String {
    let mut s: String = n.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if !s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        s.insert_str(0, "m_");
    }
    s.truncate(64);
    s
}

/// Detail key prefix for "this material was authored in that file": `detail["material:<name>"] = {"source": <corpus-relative
/// glTF path>, "index": <material index>}`. The writer copies the material and its textures from the source.
pub const ASSET_MATERIAL_PREFIX: &str = "material:";

/// Rows of the 3x3 normal matrix (the inverse transpose of the upper 3x3, sign-corrected so mirrored nodes keep outward normals).
fn normal_matrix(m: &Mat34) -> [[f32; 3]; 3] {
    let a = |r: usize, c: usize| m[r * 4 + c];
    let cof = |r: usize, c: usize| {
        let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
        a(r1, c1) * a(r2, c2) - a(r1, c2) * a(r2, c1)
    };
    let s = if det3(m) < 0.0 { -1.0 } else { 1.0 };
    [[s * cof(0, 0), s * cof(0, 1), s * cof(0, 2)], [s * cof(1, 0), s * cof(1, 1), s * cof(1, 2)], [s * cof(2, 0), s * cof(2, 1), s * cof(2, 2)]]
}

fn mat3_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
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

/// Parse GLB or `.gltf` bytes into the JSON document and its resolved buffers.
fn open(bytes: &[u8], resolve: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<Doc, String> {
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
    Ok(Doc { j, bufs })
}

/// How to name what is read. With a non-empty `prefix` (an imported asset), material `m` becomes `<prefix>__m` and the part's
/// detail records where the authored material lives (`source`, a corpus-relative path), so the writer can copy its textures.
#[derive(Default, Clone, Copy)]
pub struct LoadOpts<'a> {
    pub prefix: &'a str,
    pub source: &'a str,
}

/// Parse GLB or `.gltf` bytes. `resolve` loads external buffers by (relative) URI.
pub fn load(bytes: &[u8], resolve: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<GltfScene, String> {
    load_with(bytes, resolve, &LoadOpts::default())
}

/// [`load`] with asset naming (see [`LoadOpts`]); UVs (`uv`, point domain) and normals (`N`) are read when present.
pub fn load_with(bytes: &[u8], resolve: &dyn Fn(&str) -> Result<Vec<u8>, String>, opts: &LoadOpts) -> Result<GltfScene, String> {
    let doc = open(bytes, resolve)?;
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
        if node.pointer("/extensions/EXT_mesh_gpu_instancing").is_some() {
            scene.warnings.push(format!("node {ni}: EXT_mesh_gpu_instancing is not expanded (the mesh is read once)"));
        }
        let mesh = doc.arr("meshes").get(mi as usize).ok_or_else(|| format!("mesh {mi} does not exist"))?;
        let flip = det3(&world) < 0.0;
        let (mut pts, mut polys): (Vec<[f32; 3]>, Vec<Vec<u32>>) = (vec![], vec![]);
        let mut poly_mat: Vec<Option<String>> = Vec::new();
        let (mut uvs, mut nrm): (Vec<[f32; 2]>, Vec<[f32; 3]>) = (vec![], vec![]);
        let (mut any_uv, mut any_n) = (false, false);
        let mut asset_materials: BTreeMap<String, Value> = BTreeMap::new();
        let ncof = normal_matrix(&world);
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
            // Optional attributes: a primitive without them gets zeros, and the column exists only if some primitive had it.
            let uv_acc = prim.pointer("/attributes/TEXCOORD_0").and_then(Value::as_u64).map(|a| doc.read_acc(a as usize)).transpose()?.filter(|a| a.nc == 2 && a.n == pos.n);
            let n_acc = prim.pointer("/attributes/NORMAL").and_then(Value::as_u64).map(|a| doc.read_acc(a as usize)).transpose()?.filter(|a| a.nc == 3 && a.n == pos.n);
            any_uv |= uv_acc.is_some();
            any_n |= n_acc.is_some();
            for i in 0..pos.n {
                uvs.push(uv_acc.as_ref().map_or([0.0; 2], |a| [a.v[i * 2] as f32, a.v[i * 2 + 1] as f32]));
                nrm.push(n_acc.as_ref().map_or([0.0; 3], |a| math::normalize(mat3_mul(&ncof, [a.v[i * 3] as f32, a.v[i * 3 + 1] as f32, a.v[i * 3 + 2] as f32]))));
            }
            let mat_index = prim.get("material").and_then(Value::as_u64);
            let material = mat_index.and_then(|m| doc.arr("materials").get(m as usize).map(|mm| (m, mm))).map(|(mi, m)| {
                let orig = m.pointer("/extras/proteus_material").or_else(|| m.get("name")).and_then(Value::as_str).map_or_else(|| format!("material{mi}"), String::from);
                if opts.prefix.is_empty() {
                    material_identifier(&orig)
                } else {
                    let name = material_identifier(&format!("{}__{orig}", opts.prefix));
                    let cutout = m.get("alphaMode").and_then(Value::as_str).is_some_and(|a| a != "OPAQUE");
                    asset_materials.insert(name.clone(), json!({"source": opts.source, "index": mi, "cutout": cutout}));
                    name
                }
            });
            for t in tris {
                if t.iter().any(|&i| i as usize >= pos.n) {
                    return Err(format!("node {ni}: index out of range"));
                }
                let (a, b, c) = (t[0] + base, t[1] + base, t[2] + base);
                polys.push(if flip { vec![a, c, b] } else { vec![a, b, c] });
                poly_mat.push(material.clone());
            }
        }
        if polys.is_empty() {
            continue;
        }
        let name = node.get("name").and_then(Value::as_str).map_or_else(|| format!("node{ni}"), String::from);
        let mut geo = Geometry::from_polys(pts, &polys);
        if any_uv {
            geo.point.set("uv", Column::V2(Arc::new(uvs)));
        }
        if any_n {
            geo.point.set("N", Column::from_v3(nrm));
        }
        let names: BTreeSet<&String> = poly_mat.iter().flatten().collect();
        for n in names {
            geo.prim_groups.insert(format!("{MATERIAL_PREFIX}{n}"), Arc::new(poly_mat.iter().map(|m| m.as_ref() == Some(n)).collect()));
        }
        for (n, rec) in asset_materials {
            geo.detail.insert(format!("{ASSET_MATERIAL_PREFIX}{n}"), rec);
        }
        scene.parts.push(GltfPart { name, geo });
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

/// Instance counts up to this are written as one glTF node per instance (every importer reads that);
/// above it, as one `EXT_mesh_gpu_instancing` node per prototype.
pub const INSTANCE_NODE_LIMIT: usize = 20_000;

/// Where the raw corpus lives (`PT_CORPUS`, else `D:\Fekri\corpus`, else `corpus/raw`); asset paths are relative to it.
pub fn corpus_root() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PT_CORPUS") {
        return p.into();
    }
    let d = std::path::PathBuf::from(r"D:\Fekri\corpus");
    if d.exists() { d } else { "corpus/raw".into() }
}

/// A source glTF that authored materials are copied from: its parsed document and its folder (for external images).
struct Source {
    doc: Doc,
    dir: std::path::PathBuf,
}

struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accs: Vec<Value>,
    materials: Vec<Value>,
    material_ids: BTreeMap<String, usize>,
    meshes: Vec<Value>,
    images: Vec<Value>,
    textures: Vec<Value>,
    samplers: Vec<Value>,
    extensions_used: BTreeSet<String>,
    /// corpus-relative path -> parsed source (None = unreadable, reported once as a warning)
    sources: BTreeMap<String, Option<Source>>,
    image_ids: BTreeMap<(String, usize), usize>,
    texture_ids: BTreeMap<(String, usize), usize>,
    sampler_ids: BTreeMap<(String, usize), usize>,
    warnings: Vec<String>,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            bin: vec![],
            views: vec![],
            accs: vec![],
            materials: vec![],
            material_ids: BTreeMap::new(),
            meshes: vec![],
            images: vec![],
            textures: vec![],
            samplers: vec![],
            extensions_used: BTreeSet::new(),
            sources: BTreeMap::new(),
            image_ids: BTreeMap::new(),
            texture_ids: BTreeMap::new(),
            sampler_ids: BTreeMap::new(),
            warnings: vec![],
        }
    }

    fn source(&mut self, rel: &str) -> Option<&Source> {
        if !self.sources.contains_key(rel) {
            let path = corpus_root().join(rel);
            let loaded = (|| -> Result<Source, String> {
                if rel.contains("..") {
                    return Err("refusing a path with '..'".into());
                }
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let dir = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                let d2 = dir.clone();
                let doc = open(&bytes, &move |uri| if uri.contains("..") { Err(format!("refusing '{uri}'")) } else { std::fs::read(d2.join(uri)).map_err(|e| format!("{uri}: {e}")) })?;
                Ok(Source { doc, dir })
            })();
            if let Err(e) = &loaded {
                self.warnings.push(format!("asset material source {rel}: {e}; its materials are written without textures"));
            }
            self.sources.insert(rel.to_string(), loaded.ok());
        }
        self.sources.get(rel).and_then(Option::as_ref)
    }

    /// Embed one image of a source file; returns its index in `images`.
    fn copy_image(&mut self, rel: &str, idx: usize) -> Option<usize> {
        if let Some(&i) = self.image_ids.get(&(rel.to_string(), idx)) {
            return Some(i);
        }
        let (bytes, mime) = {
            let src = self.source(rel)?;
            let img = src.doc.arr("images").get(idx)?;
            if let Some(uri) = img.get("uri").and_then(Value::as_str) {
                if let Some(rest) = uri.strip_prefix("data:") {
                    let (head, data) = rest.split_once(',')?;
                    (base64_decode(data).ok()?, if head.starts_with("image/png") { "image/png" } else { "image/jpeg" })
                } else {
                    let clean = percent_decode(uri);
                    if clean.contains("..") {
                        return None;
                    }
                    let mime = if clean.to_ascii_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" };
                    (std::fs::read(src.dir.join(&clean)).ok()?, mime)
                }
            } else {
                let bv = src.doc.arr("bufferViews").get(img.get("bufferView")?.as_u64()? as usize)?;
                let buf = src.doc.bufs.get(bv.get("buffer")?.as_u64()? as usize)?;
                let off = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
                let len = bv.get("byteLength")?.as_u64()? as usize;
                let mime = if img.get("mimeType").and_then(Value::as_str) == Some("image/png") { "image/png" } else { "image/jpeg" };
                (buf.get(off..off + len)?.to_vec(), mime)
            }
        };
        let v = self.view(&bytes, None);
        self.images.push(json!({"bufferView": v, "mimeType": mime}));
        self.image_ids.insert((rel.to_string(), idx), self.images.len() - 1);
        Some(self.images.len() - 1)
    }

    /// Copy one texture (image + sampler) of a source file; returns its index in `textures`.
    fn copy_texture(&mut self, rel: &str, idx: usize) -> Option<usize> {
        if let Some(&i) = self.texture_ids.get(&(rel.to_string(), idx)) {
            return Some(i);
        }
        let (img_idx, sampler) = {
            let src = self.source(rel)?;
            let t = src.doc.arr("textures").get(idx)?;
            (t.get("source").and_then(Value::as_u64)? as usize, t.get("sampler").and_then(Value::as_u64).map(|s| (s as usize, src.doc.arr("samplers").get(s as usize).cloned())))
        };
        let image = self.copy_image(rel, img_idx)?;
        let mut tex = json!({"source": image});
        if let Some((si, Some(sj))) = sampler {
            let new = match self.sampler_ids.get(&(rel.to_string(), si)) {
                Some(&n) => n,
                None => {
                    self.samplers.push(sj);
                    self.sampler_ids.insert((rel.to_string(), si), self.samplers.len() - 1);
                    self.samplers.len() - 1
                }
            };
            tex["sampler"] = json!(new);
        }
        self.textures.push(tex);
        self.texture_ids.insert((rel.to_string(), idx), self.textures.len() - 1);
        Some(self.textures.len() - 1)
    }

    /// Rewrite texture references inside a copied material JSON; a reference that cannot be copied is removed.
    fn remap_textures(&mut self, rel: &str, v: &mut Value) {
        let Value::Object(map) = v else { return };
        let keys: Vec<String> = map.keys().cloned().collect();
        for k in keys {
            if k == "extensions" {
                if let Some(Value::Object(e)) = map.get(&k) {
                    self.extensions_used.extend(e.keys().cloned());
                }
            }
            let is_tex = k.ends_with("Texture") && map.get(&k).is_some_and(|t| t.get("index").is_some());
            if is_tex {
                let old = map[&k]["index"].as_u64().unwrap_or(u64::MAX) as usize;
                match self.copy_texture(rel, old) {
                    Some(n) => {
                        map.get_mut(&k).unwrap()["index"] = json!(n);
                        self.remap_textures(rel, map.get_mut(&k).unwrap());
                    }
                    None => {
                        map.remove(&k);
                    }
                }
            } else if let Some(child) = map.get_mut(&k) {
                if child.is_object() {
                    self.remap_textures(rel, child);
                }
            }
        }
    }

    /// The authored material `index` of `rel`, with its textures embedded, under our name.
    fn copy_material(&mut self, rel: &str, index: usize, name: &str) -> Option<Value> {
        let mut m = self.source(rel)?.doc.arr("materials").get(index)?.clone();
        self.remap_textures(rel, &mut m);
        m["name"] = json!(name);
        m["extras"] = json!({"proteus_material": name, "proteus_source": rel});
        Some(m)
    }

    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        let off = self.bin.len();
        self.bin.extend_from_slice(bytes);
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": off, "byteLength": bytes.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.views.push(v);
        self.views.len() - 1
    }

    fn acc(&mut self, view: usize, ctype: u32, count: usize, ty: &str, minmax: Option<([f32; 3], [f32; 3])>) -> usize {
        let mut a = json!({"bufferView": view, "componentType": ctype, "count": count, "type": ty});
        if let Some((lo, hi)) = minmax {
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accs.push(a);
        self.accs.len() - 1
    }

    fn f32_acc(&mut self, data: &[f32], ty: &str, n: usize, target: Option<u32>, minmax: Option<([f32; 3], [f32; 3])>) -> usize {
        let v = self.view(bytemuck::cast_slice(data), target);
        self.acc(v, 5126, n, ty, minmax)
    }

    /// Embed a PNG file as an image + texture (clamped, mipmapped); the file's bytes go into the buffer unchanged.
    fn texture_from_file(&mut self, path: &str) -> Option<usize> {
        let bytes = std::fs::read(path).ok()?;
        let v = self.view(&bytes, None);
        self.images.push(json!({"bufferView": v, "mimeType": "image/png"}));
        self.samplers.push(json!({"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}));
        self.textures.push(json!({"source": self.images.len() - 1, "sampler": self.samplers.len() - 1}));
        Some(self.textures.len() - 1)
    }

    /// A layered material baked to textures (`detail["baked:<name>"] = {"albedo": path, "normal": path}`): what an engine can run.
    fn baked_material(&mut self, name: &str, layers: Option<&Value>, baked: Option<&Value>) -> Option<Value> {
        let b = baked?;
        let albedo = self.texture_from_file(b.get("albedo")?.as_str()?)?;
        let mut m = json!({
            "name": name,
            "pbrMetallicRoughness": {"baseColorTexture": {"index": albedo}, "metallicFactor": 0.0, "roughnessFactor": 0.9},
            "extras": {"proteus_material": name, "proteus_baked": true, "proteus_layers": layers.cloned().unwrap_or(Value::Null)},
        });
        if let Some(normal) = b.get("normal").and_then(Value::as_str).and_then(|p| self.texture_from_file(p)) {
            m["normalTexture"] = json!({"index": normal});
        }
        Some(m)
    }

    /// A named material: the authored one (textures embedded) when the geometry records where it came from, else a plain
    /// grey placeholder whose NAME the renderer binds to a library material.
    fn material(&mut self, name: &str, record: Option<&Value>, layers: Option<&Value>, baked: Option<&Value>) -> usize {
        if let Some(&i) = self.material_ids.get(name) {
            return i;
        }
        let authored = record.and_then(|r| Some((r.get("source")?.as_str()?.to_string(), r.get("index")?.as_u64()? as usize))).and_then(|(src, idx)| self.copy_material(&src, idx, name));
        let baked_material = if authored.is_none() { self.baked_material(name, layers, baked) } else { None };
        self.materials.push(authored.or(baked_material).unwrap_or_else(|| {
            json!({
                "name": name,
                "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.8, 0.8, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.8},
                "extras": layers.map_or_else(|| json!({"proteus_material": name}), |l| json!({"proteus_material": name, "proteus_layers": l})),
            })
        }));
        self.material_ids.insert(name.to_string(), self.materials.len() - 1);
        self.materials.len() - 1
    }

    /// One mesh with a primitive per material (unassigned triangles first); `None` if there is no surface.
    fn mesh(&mut self, g: &Geometry) -> Option<usize> {
        let tris_of: Vec<Vec<[u32; 3]>> = (0..g.n_prims())
            .map(|i| {
                let pr = g.prim_points(i);
                (1..pr.len().saturating_sub(1)).map(|k| [pr[0], pr[k], pr[k + 1]]).collect()
            })
            .collect();
        let mats = g.prim_materials();
        let mut by_mat: BTreeMap<Option<&str>, Vec<u32>> = BTreeMap::new();
        for (tris, m) in tris_of.iter().zip(&mats) {
            if !tris.is_empty() {
                by_mat.entry(*m).or_default().extend(tris.iter().flatten().copied());
            }
        }
        if by_mat.is_empty() || g.n_points() == 0 {
            return None;
        }
        let flat: Vec<f32> = g.p().iter().flatten().copied().collect();
        let (lo, hi) = g.base_bounds().unwrap_or(([0.0; 3], [0.0; 3]));
        let pos = self.f32_acc(&flat, "VEC3", g.n_points(), Some(34962), Some((lo, hi)));
        let mut attributes = json!({"POSITION": pos});
        if let Some(cd) = g.point.get("Cd").and_then(Column::as_v3).filter(|c| c.len() == g.n_points()) {
            let col: Vec<f32> = cd.iter().flatten().map(|v| v.clamp(0.0, 1.0)).collect();
            attributes["COLOR_0"] = json!(self.f32_acc(&col, "VEC3", cd.len(), Some(34962), None));
        }
        if let Some(n) = g.point.get("N").and_then(Column::as_v3).filter(|c| c.len() == g.n_points()) {
            let flat: Vec<f32> = n.iter().flatten().copied().collect();
            attributes["NORMAL"] = json!(self.f32_acc(&flat, "VEC3", n.len(), Some(34962), None));
        }
        if let Some(Column::V2(uv)) = g.point.get("uv").filter(|c| c.len() == g.n_points()) {
            let flat: Vec<f32> = uv.iter().flatten().copied().collect();
            attributes["TEXCOORD_0"] = json!(self.f32_acc(&flat, "VEC2", uv.len(), Some(34962), None));
        }
        let authored = g.detail.keys().any(|k| k.starts_with(ASSET_MATERIAL_PREFIX) || k.starts_with("baked:"));
        if let (true, Some(nv), Some(Column::V2(uv))) = (authored, g.point.get("N").and_then(Column::as_v3).filter(|c| c.len() == g.n_points()), g.point.get("uv").filter(|c| c.len() == g.n_points())) {
            let all: Vec<u32> = tris_of.iter().flatten().flatten().copied().collect();
            let flat: Vec<f32> = tangents(g.p(), nv, uv, &all).iter().flatten().copied().collect();
            attributes["TANGENT"] = json!(self.f32_acc(&flat, "VEC4", g.n_points(), Some(34962), None));
        }
        for (col, attr) in [("Wt0", "_WEIGHTS_0"), ("Wt1", "_WEIGHTS_1")] {
            if let Some(Column::V4(w)) = g.point.get(col).filter(|c| c.len() == g.n_points()) {
                let flat: Vec<f32> = w.iter().flatten().copied().collect();
                attributes[attr] = json!(self.f32_acc(&flat, "VEC4", w.len(), Some(34962), None));
            }
        }
        let mut prims = Vec::new();
        for (m, idx) in by_mat {
            let v = self.view(bytemuck::cast_slice(&idx), Some(34963));
            let ia = self.acc(v, 5125, idx.len(), "SCALAR", None);
            let mut p = json!({"attributes": attributes, "indices": ia});
            if let Some(name) = m {
                let record = g.detail.get(&format!("{ASSET_MATERIAL_PREFIX}{name}"));
                let layers = g.detail.get(&format!("layers:{name}"));
                let baked = g.detail.get(&format!("baked:{name}"));
                p["material"] = json!(self.material(name, record, layers, baked));
            }
            prims.push(p);
        }
        self.meshes.push(json!({"primitives": prims}));
        Some(self.meshes.len() - 1)
    }
}

/// Per-vertex tangents (xyz, handedness w) from positions, normals and UVs, as glTF wants them for normal-mapped
/// materials: accumulated per triangle, orthogonalised against the normal. UV v is flipped because glTF's origin is the
/// top-left corner while normal maps are green-up.
pub fn tangents(pos: &[[f32; 3]], nrm: &[[f32; 3]], uv: &[[f32; 2]], tri_indices: &[u32]) -> Vec<[f32; 4]> {
    let n = pos.len();
    let (mut t1, mut t2) = (vec![[0.0f32; 3]; n], vec![[0.0f32; 3]; n]);
    for t in tri_indices.chunks_exact(3) {
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let e1 = math::sub(pos[i1], pos[i0]);
        let e2 = math::sub(pos[i2], pos[i0]);
        let (du1, dv1) = (uv[i1][0] - uv[i0][0], -(uv[i1][1] - uv[i0][1]));
        let (du2, dv2) = (uv[i2][0] - uv[i0][0], -(uv[i2][1] - uv[i0][1]));
        let r = du1 * dv2 - du2 * dv1;
        if r.abs() < 1e-14 {
            continue;
        }
        let f = 1.0 / r;
        for a in 0..3 {
            let tan = (e1[a] * dv2 - e2[a] * dv1) * f;
            let bit = (e2[a] * du1 - e1[a] * du2) * f;
            for &i in &[i0, i1, i2] {
                t1[i][a] += tan;
                t2[i][a] += bit;
            }
        }
    }
    (0..n)
        .map(|i| {
            let nv = math::normalize(nrm[i]);
            let d = nv[0] * t1[i][0] + nv[1] * t1[i][1] + nv[2] * t1[i][2];
            let mut t = [t1[i][0] - nv[0] * d, t1[i][1] - nv[1] * d, t1[i][2] - nv[2] * d];
            if t[0] * t[0] + t[1] * t[1] + t[2] * t[2] < 1e-18 {
                t = math::cross(nv, if nv[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] });
            }
            let t = math::normalize(t);
            let c = math::cross(nv, t);
            let w = if c[0] * t2[i][0] + c[1] * t2[i][1] + c[2] * t2[i][2] < 0.0 { -1.0 } else { 1.0 };
            [t[0], t[1], t[2], w]
        })
        .collect()
}

/// Column-major glTF matrix from our row-major 3x4.
fn gltf_matrix(m: &Mat34) -> [f32; 16] {
    let mut o = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..3 {
            o[c * 4 + r] = m[r * 4 + c];
        }
    }
    o[15] = 1.0;
    o
}

/// glTF document and binary buffer for the realized-or-instanced geometry: one mesh per prototype with a
/// primitive per material, lazy instances as nodes (`node_limit` or fewer) or `EXT_mesh_gpu_instancing`.
pub fn build(g: &Geometry, node_limit: usize) -> (Value, Vec<u8>) {
    build_lod(g, node_limit, None)
}

/// LOD exports write one node per instance and level; above this many nodes the exporter falls back to a single level.
pub const LOD_NODE_LIMIT: usize = 80_000;

/// [`build`] with levels of detail: every prototype becomes a chain of meshes (see `lod`), every instance one node per
/// level named `inst_<i>__lod<l>` whose extras carry `proteus_lod` and `proteus_lod_range` (metres, scaled by the instance).
/// The engine bridge turns the ranges into visibility ranges; a renderer that ignores them draws level 0 by dropping `__lod1+`.
pub fn build_lod(g: &Geometry, node_limit: usize, lod: Option<&crate::lod::LodPolicy>) -> (Value, Vec<u8>) {
    let mut b = Builder::new();
    let mut nodes: Vec<Value> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut used_ext = false;
    let mut lod_report: Vec<Value> = Vec::new();
    let lod = match (lod, &g.instances) {
        (Some(p), Some(i)) if i.len() * p.ratios.len() > LOD_NODE_LIMIT => {
            b.warnings.push(format!("LOD skipped: {} instances x {} levels exceeds {LOD_NODE_LIMIT} nodes", i.len(), p.ratios.len()));
            None
        }
        (l, _) => l,
    };
    let node_limit = if lod.is_some() { node_limit.max(LOD_NODE_LIMIT) } else { node_limit };
    if let Some(m) = b.mesh(g) {
        nodes.push(json!({"name": "geometry", "mesh": m}));
        roots.push(nodes.len() - 1);
    }
    if let Some(inst) = &g.instances {
        // levels[p] = (mesh, begin, end) per level of prototype p; only prototypes that are placed are written
        let mut counts = vec![0usize; inst.protos.len()];
        for &p in inst.proto_id.iter() {
            counts[p as usize] += 1;
        }
        let mut levels: Vec<Vec<(usize, f32, f32)>> = vec![vec![]; inst.protos.len()];
        for p in (0..inst.protos.len()).filter(|&p| counts[p] > 0) {
            match lod {
                Some(policy) => {
                    let chain = crate::lod::lod_chain(&inst.protos[p], policy);
                    lod_report.push(json!({"proto": p, "triangles": chain.iter().map(|l| l.triangles).collect::<Vec<_>>(),
                        "begin": chain.iter().map(|l| l.begin).collect::<Vec<_>>(), "end": chain.iter().map(|l| l.end).collect::<Vec<_>>(), "instances": counts[p]}));
                    for l in &chain {
                        if let Some(m) = b.mesh(&l.geo) {
                            levels[p].push((m, l.begin, l.end));
                        }
                    }
                }
                None => {
                    if let Some(m) = b.mesh(&inst.protos[p].realize()) {
                        levels[p].push((m, 0.0, f32::INFINITY));
                    }
                }
            }
        }
        if inst.len() <= node_limit {
            for (i, xf) in inst.xf.iter().enumerate() {
                let p = inst.proto_id[i] as usize;
                let (_, sc) = crate::io::quat_from_cols(xf);
                let scale = (sc[0] * sc[1] * sc[2]).abs().cbrt().max(1e-6);
                for (l, &(m, begin, end)) in levels[p].iter().enumerate() {
                    let mut node = json!({"name": if lod.is_some() { format!("inst_{i}__lod{l}") } else { format!("inst_{i}") }, "mesh": m, "matrix": gltf_matrix(xf)});
                    if lod.is_some() {
                        node["extras"] = json!({"proteus_lod": l, "proteus_lod_range": [begin * scale, end * scale]});
                    }
                    nodes.push(node);
                    roots.push(nodes.len() - 1);
                }
            }
        } else {
            used_ext = true;
            for p in 0..inst.protos.len() {
                let ids: Vec<usize> = (0..inst.len()).filter(|&i| inst.proto_id[i] as usize == p).collect();
                let Some(&(m, _, _)) = levels[p].first().filter(|_| !ids.is_empty()) else { continue };
                let (mut t, mut r, mut s) = (Vec::with_capacity(ids.len() * 3), Vec::with_capacity(ids.len() * 4), Vec::with_capacity(ids.len() * 3));
                for &i in &ids {
                    let xf = &inst.xf[i];
                    let ([w, x, y, z], sc) = crate::io::quat_from_cols(xf);
                    t.extend_from_slice(&[xf[3], xf[7], xf[11]]);
                    r.extend_from_slice(&[x, y, z, w]);
                    s.extend_from_slice(&sc);
                }
                let (ta, ra, sa) = (b.f32_acc(&t, "VEC3", ids.len(), None, None), b.f32_acc(&r, "VEC4", ids.len(), None, None), b.f32_acc(&s, "VEC3", ids.len(), None, None));
                nodes.push(json!({"name": format!("proto_{p}"), "mesh": m,
                    "extensions": {"EXT_mesh_gpu_instancing": {"attributes": {"TRANSLATION": ta, "ROTATION": ra, "SCALE": sa}}}}));
                roots.push(nodes.len() - 1);
            }
        }
    }
    let mut doc = json!({"asset": {"version": "2.0", "generator": "proteus"}, "scene": 0, "scenes": [{"nodes": roots}]});
    if !nodes.is_empty() {
        doc["nodes"] = json!(nodes);
        doc["meshes"] = json!(b.meshes);
        doc["bufferViews"] = json!(b.views);
        doc["accessors"] = json!(b.accs);
        doc["buffers"] = json!([{"byteLength": b.bin.len()}]);
        if !b.materials.is_empty() {
            doc["materials"] = json!(b.materials);
        }
        if !b.textures.is_empty() {
            doc["textures"] = json!(b.textures);
            doc["images"] = json!(b.images);
        }
        if !b.samplers.is_empty() {
            doc["samplers"] = json!(b.samplers);
        }
        if used_ext {
            b.extensions_used.insert("EXT_mesh_gpu_instancing".into());
        }
        if !b.extensions_used.is_empty() {
            doc["extensionsUsed"] = json!(b.extensions_used);
        }
        let mut extras = serde_json::Map::new();
        if !b.warnings.is_empty() {
            extras.insert("proteus_warnings".into(), json!(b.warnings));
        }
        if !lod_report.is_empty() {
            extras.insert("proteus_lod".into(), json!(lod_report));
        }
        if !extras.is_empty() {
            doc["asset"]["extras"] = Value::Object(extras);
        }
        doc["buffers"] = json!([{"byteLength": b.bin.len()}]);
    }
    (doc, b.bin)
}

/// Binary glTF: named materials as primitives, vertex colour `Cd` as COLOR_0, instances kept as instances.
pub fn to_glb(g: &Geometry) -> Vec<u8> {
    glb_bytes(build(g, INSTANCE_NODE_LIMIT))
}

/// [`to_glb`] with levels of detail for every placed prototype (see [`build_lod`]).
pub fn to_glb_lod(g: &Geometry, policy: &crate::lod::LodPolicy) -> Vec<u8> {
    glb_bytes(build_lod(g, INSTANCE_NODE_LIMIT, Some(policy)))
}

/// The JSON chunk of a `.glb` file only (cheap even for a gigabyte file).
pub fn read_json(path: &std::path::Path) -> Result<Value, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut head = [0u8; 20];
    f.read_exact(&mut head).map_err(|e| e.to_string())?;
    if le_u32(&head, 0)? != GLB_MAGIC || le_u32(&head, 16)? != CHUNK_JSON {
        return Err("not a GLB with a leading JSON chunk".into());
    }
    let mut json = vec![0u8; le_u32(&head, 12)? as usize];
    f.read_exact(&mut json).map_err(|e| e.to_string())?;
    serde_json::from_slice(&json).map_err(|e| e.to_string())
}

fn glb_bytes((doc, mut bin): (Value, Vec<u8>)) -> Vec<u8> {
    let mut json_bytes = doc.to_string().into_bytes();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let has_bin = !bin.is_empty();
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

    fn two_materials() -> Geometry {
        let mut g = cube();
        g.assign_material("stone", &[true, true, false, false, false, false]);
        g.assign_material("wood", &[false, false, false, true, true, false]);
        g
    }

    #[test]
    fn named_materials_become_primitives_and_come_back_as_groups() {
        let (doc, _) = build(&two_materials(), INSTANCE_NODE_LIMIT);
        let names: Vec<&str> = doc["materials"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["stone", "wood"].as_slice());
        // unassigned faces form a third primitive without a material
        let prims = doc["meshes"][0]["primitives"].as_array().unwrap();
        assert_eq!(prims.len(), 3);
        assert_eq!(prims.iter().filter(|p| p.get("material").is_none()).count(), 1);

        let scene = load(&to_glb(&two_materials()), &no_external).unwrap();
        let back = &scene.parts[0].geo;
        assert_eq!(back.material_names(), ["stone", "wood"]);
        let count = |m: &str| back.prim_materials().iter().filter(|x| **x == Some(m)).count();
        assert_eq!((count("stone"), count("wood")), (4, 4), "2 quads = 4 triangles each");
        assert_eq!(back.prim_materials().iter().filter(|x| x.is_none()).count(), 4);
    }

    #[test]
    fn foreign_material_names_become_identifiers() {
        assert_eq!(material_identifier("Wood Floor.001"), "Wood_Floor_001");
        assert_eq!(material_identifier("2k_brick"), "m_2k_brick");
        assert!(crate::geo::valid_material_name(&material_identifier("lambert1")));
    }

    fn instanced(n: usize) -> Geometry {
        let tri = Geometry::from_polys(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[vec![0, 1, 2]]);
        let mut g = Geometry::new();
        let xf: Vec<Mat34> = (0..n).map(|i| math::trs([i as f32, 0.0, 0.0], [0.0, 30.0, 0.0], [1.0, 1.0, 1.0])).collect();
        g.instances = Some(crate::geo::Instances { protos: vec![Arc::new(tri)], proto_id: Arc::new(vec![0; n]), xf: Arc::new(xf), attrs: Default::default() });
        g
    }

    #[test]
    fn instances_stay_instances() {
        let (doc, bin) = build(&instanced(3), INSTANCE_NODE_LIMIT);
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 1, "one prototype mesh shared by every node");
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(doc["nodes"][2]["matrix"][12].as_f64(), Some(2.0), "translation sits in the last column");
        assert!(bin.len() < 100);
        // above the node limit: one node per prototype carrying the extension
        let (doc, _) = build(&instanced(3), 2);
        assert_eq!(doc["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(doc["extensionsUsed"][0], "EXT_mesh_gpu_instancing");
        let attrs = &doc["nodes"][0]["extensions"]["EXT_mesh_gpu_instancing"]["attributes"];
        assert_eq!(doc["accessors"][attrs["TRANSLATION"].as_u64().unwrap() as usize]["count"], 3);
    }

    #[test]
    fn a_baked_layered_material_becomes_a_textured_gltf_material() {
        let dir = std::env::temp_dir().join(format!("pt_bake_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, n) = (dir.join("albedo.png"), dir.join("normal.png"));
        std::fs::write(&a, b"\x89PNG albedo").unwrap();
        std::fs::write(&n, b"\x89PNG normal").unwrap();
        let mut g = cube();
        g.assign_material("terrain_blend", &[true; 6]);
        g.detail.insert("layers:terrain_blend".into(), json!(["ground_flat", "ground_rock"]));
        g.detail.insert("baked:terrain_blend".into(), json!({"albedo": a.to_string_lossy(), "normal": n.to_string_lossy()}));
        g.point.set("N", Column::from_v3(g.p().to_vec()));
        g.point.set("uv", Column::V2(Arc::new(g.p().iter().map(|p| [p[0], p[2]]).collect())));
        let (doc, bin) = build(&g, 10);
        let m = &doc["materials"][0];
        assert_eq!(m["pbrMetallicRoughness"]["baseColorTexture"]["index"], 0);
        assert_eq!(m["normalTexture"]["index"], 1);
        assert_eq!(m["extras"]["proteus_baked"], true);
        assert_eq!(doc["images"].as_array().unwrap().len(), 2);
        let bv = &doc["bufferViews"][doc["images"][1]["bufferView"].as_u64().unwrap() as usize];
        let (off, len) = (bv["byteOffset"].as_u64().unwrap() as usize, bv["byteLength"].as_u64().unwrap() as usize);
        assert_eq!(&bin[off..off + len], b"\x89PNG normal");
        assert!(doc["meshes"][0]["primitives"][0]["attributes"].get("TANGENT").is_some(), "a baked normal map needs tangents");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tangents_follow_u_and_the_green_up_convention() {
        // a quad in the XY plane facing +Z, u along +X, glTF v running DOWN the image (v = 1 - y): tangent +X, bitangent +Y => w = +1
        let pos = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let nrm = [[0.0, 0.0, 1.0]; 4];
        let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let t = tangents(&pos, &nrm, &uv, &[0, 1, 2, 0, 2, 3]);
        for v in t {
            assert!((v[0] - 1.0).abs() < 1e-5 && v[1].abs() < 1e-5 && v[2].abs() < 1e-5 && v[3] == 1.0, "{v:?}");
        }
        // mirrored u flips the handedness
        let mirrored = [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]];
        assert_eq!(tangents(&pos, &nrm, &mirrored, &[0, 1, 2, 0, 2, 3])[0][3], -1.0);
    }

    #[test]
    fn lod_export_writes_a_node_per_instance_and_level_with_scaled_ranges() {
        // a heavy prototype (a 60 x 60 quad grid = 7200 triangles) placed twice, once at double scale, plus a tiny one
        let n = 60u32;
        let pts: Vec<[f32; 3]> = (0..=n).flat_map(|j| (0..=n).map(move |i| [i as f32 * 0.1, (i as f32 * 0.5).sin() * 0.05, j as f32 * 0.1])).collect();
        let polys: Vec<Vec<u32>> = (0..n).flat_map(|j| (0..n).map(move |i| vec![j * (n + 1) + i, (j + 1) * (n + 1) + i, (j + 1) * (n + 1) + i + 1, j * (n + 1) + i + 1])).collect();
        let heavy = Geometry::from_polys(pts, &polys);
        let tiny = Geometry::from_polys(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[vec![0, 1, 2]]);
        let mut g = Geometry::new();
        let xf = vec![math::trs([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]), math::trs([10.0, 0.0, 0.0], [0.0, 0.0, 0.0], [2.0, 2.0, 2.0]), math::trs([20.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])];
        g.instances = Some(crate::geo::Instances { protos: vec![Arc::new(heavy), Arc::new(tiny)], proto_id: Arc::new(vec![0, 0, 1]), xf: Arc::new(xf), attrs: Default::default() });
        let policy = crate::lod::LodPolicy::default();
        let (doc, _) = build_lod(&g, INSTANCE_NODE_LIMIT, Some(&policy));
        let nodes = doc["nodes"].as_array().unwrap();
        let named = |prefix: &str| nodes.iter().filter(|n| n["name"].as_str().is_some_and(|s| s.starts_with(prefix))).count();
        assert_eq!((named("inst_0__lod"), named("inst_1__lod"), named("inst_2__lod")), (4, 4, 1), "the heavy prototype has 4 levels, the tiny one 1");
        let range = |name: &str| {
            let n = nodes.iter().find(|n| n["name"] == name).unwrap();
            (n["extras"]["proteus_lod_range"][0].as_f64().unwrap(), n["extras"]["proteus_lod_range"][1].as_f64().unwrap())
        };
        let (b1, e1) = range("inst_0__lod1");
        let (b1x2, e1x2) = range("inst_1__lod1");
        assert!(b1 > 0.0 && e1 > b1);
        assert!((b1x2 - 2.0 * b1).abs() < 1e-3 && (e1x2 - 2.0 * e1).abs() < 1e-3, "ranges scale with the instance");
        assert_eq!(range("inst_0__lod0").0, 0.0);
        let rep = &doc["asset"]["extras"]["proteus_lod"];
        assert_eq!(rep.as_array().unwrap().len(), 2, "both placed prototypes are reported");
        assert_eq!(rep[0]["triangles"].as_array().unwrap().len(), 4);
        // without a policy the same export has one plain node per instance and no extras
        let (plain, _) = build(&g, INSTANCE_NODE_LIMIT);
        assert_eq!(plain["nodes"].as_array().unwrap().len(), 3);
        assert!(plain["nodes"][0].get("extras").is_none());
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
