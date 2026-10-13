//! Exporters (OBJ, glTF) and the native binary cache format (`ptgeo`).

use crate::column::{AttrSet, Column};
use crate::geo::{Geometry, Instances, Topology};
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

pub fn to_obj(g: &Geometry) -> String {
    let g = g.realize();
    let mut s = String::new();
    for p in g.p() {
        let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
    }
    let mats = g.prim_materials();
    let mut current: Option<&str> = None;
    for (i, m) in mats.iter().enumerate() {
        if *m != current {
            current = *m;
            let _ = writeln!(s, "usemtl {}", current.unwrap_or("none"));
        }
        let _ = write!(s, "f");
        for v in g.prim_points(i) {
            let _ = write!(s, " {}", v + 1);
        }
        let _ = writeln!(s);
    }
    s
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_encode(data: &[u8]) -> String {
    base64(data)
}

/// `.gltf` text with the buffer embedded as a data URI (same content as [`crate::gltf::to_glb`]).
pub fn to_gltf(g: &Geometry) -> String {
    let (mut doc, bin) = crate::gltf::build(g, crate::gltf::INSTANCE_NODE_LIMIT);
    if let Some(b) = doc.get_mut("buffers").and_then(|b| b.get_mut(0)) {
        b["uri"] = json!(format!("data:application/octet-stream;base64,{}", base64(&bin)));
    }
    doc.to_string()
}

// ---- native binary format (used by the persistent cook cache) ----

struct W(Vec<u8>);
impl W {
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.0.extend_from_slice(b);
    }
    fn attrs(&mut self, s: &AttrSet) {
        self.u64(s.0.len() as u64);
        for (k, c) in &s.0 {
            self.bytes(k.as_bytes());
            self.bytes(c.tag().as_bytes());
            self.bytes(c.as_bytes());
        }
    }
    fn groups(&mut self, g: &std::collections::BTreeMap<String, Arc<Vec<bool>>>) {
        self.u64(g.len() as u64);
        for (k, v) in g {
            self.bytes(k.as_bytes());
            self.bytes(&v.iter().map(|&b| b as u8).collect::<Vec<_>>());
        }
    }
}

struct R<'a>(&'a [u8], usize);
impl<'a> R<'a> {
    fn u64(&mut self) -> Result<u64, String> {
        let b = self.0.get(self.1..self.1 + 8).ok_or("truncated")?;
        self.1 += 8;
        let arr: [u8; 8] = b.try_into().map_err(|_| "truncated")?;
        Ok(u64::from_le_bytes(arr))
    }
    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let n = self.u64()? as usize;
        let b = self.0.get(self.1..self.1 + n).ok_or("truncated")?;
        self.1 += n;
        Ok(b)
    }
    fn string(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|e| e.to_string())
    }
    fn attrs(&mut self) -> Result<AttrSet, String> {
        let mut s = AttrSet::default();
        for _ in 0..self.u64()? {
            let k = self.string()?;
            let tag = self.string()?;
            s.set(&k, Column::from_bytes(&tag, self.bytes()?)?);
        }
        Ok(s)
    }
    fn groups(&mut self) -> Result<std::collections::BTreeMap<String, Arc<Vec<bool>>>, String> {
        let mut m = std::collections::BTreeMap::new();
        for _ in 0..self.u64()? {
            let k = self.string()?;
            m.insert(k, Arc::new(self.bytes()?.iter().map(|&b| b != 0).collect()));
        }
        Ok(m)
    }
}

pub fn to_bytes(g: &Geometry) -> Vec<u8> {
    let mut w = W(b"PTG2".to_vec());
    w.attrs(&g.point);
    w.attrs(&g.vertex);
    w.attrs(&g.prim);
    w.bytes(bytemuck::cast_slice(&g.topo.offsets));
    w.bytes(bytemuck::cast_slice(&g.topo.indices));
    w.groups(&g.point_groups);
    w.groups(&g.prim_groups);
    w.bytes(serde_json::to_string(&g.detail).unwrap_or_else(|_| "{}".to_string()).as_bytes());
    match &g.instances {
        None => w.u64(0),
        Some(i) => {
            w.u64(1);
            w.u64(i.protos.len() as u64);
            for p in &i.protos {
                w.bytes(&to_bytes(p));
            }
            w.bytes(bytemuck::cast_slice(i.proto_id.as_slice()));
            w.bytes(bytemuck::cast_slice(i.xf.as_slice()));
            w.attrs(&i.attrs);
        }
    }
    w.u64(g.volumes.len() as u64);
    for (name, v) in &g.volumes {
        w.bytes(name.as_bytes());
        for d in v.dims {
            w.u64(d as u64);
        }
        w.bytes(bytemuck::cast_slice(&v.origin));
        w.bytes(&v.voxel.to_le_bytes());
        w.bytes(bytemuck::cast_slice(v.data.as_slice()));
    }
    w.0
}

pub fn from_bytes(b: &[u8]) -> Result<Geometry, String> {
    if b.get(..4) != Some(b"PTG2") {
        return Err("not a ptgeo file".into());
    }
    let mut r = R(b, 4);
    let mut g = Geometry::new();
    g.point = r.attrs()?;
    g.vertex = r.attrs()?;
    g.prim = r.attrs()?;
    let offsets = crate::column::collect::<u32>(r.bytes()?)?;
    let indices = crate::column::collect::<u32>(r.bytes()?)?;
    g.topo = Arc::new(Topology { offsets, indices });
    g.point_groups = r.groups()?;
    g.prim_groups = r.groups()?;
    g.detail = serde_json::from_slice(r.bytes()?).map_err(|e| e.to_string())?;
    if r.u64()? == 1 {
        let mut inst = Instances::default();
        for _ in 0..r.u64()? {
            inst.protos.push(Arc::new(from_bytes(r.bytes()?)?));
        }
        inst.proto_id = Arc::new(crate::column::collect(r.bytes()?)?);
        inst.xf = Arc::new(crate::column::collect(r.bytes()?)?);
        inst.attrs = r.attrs()?;
        g.instances = Some(inst);
    }
    for _ in 0..r.u64()? {
        let name = r.string()?;
        let dims = [r.u64()? as usize, r.u64()? as usize, r.u64()? as usize];
        let origin: Vec<f32> = crate::column::collect(r.bytes()?)?;
        let voxel: Vec<f32> = crate::column::collect(r.bytes()?)?;
        let data: Vec<f32> = crate::column::collect(r.bytes()?)?;
        if origin.len() != 3 || voxel.len() != 1 || data.len() != dims[0] * dims[1] * dims[2] {
            return Err("corrupt volume".into());
        }
        g.volumes.insert(name, Arc::new(crate::geo::Volume { dims, origin: [origin[0], origin[1], origin[2]], voxel: voxel[0], data: Arc::new(data) }));
    }
    Ok(g)
}

/// Parse a Wavefront OBJ (vertices and faces; texture/normal indices are ignored).
pub fn load_obj(text: &str) -> Result<Geometry, String> {
    let (mut pts, mut polys): (Vec<[f32; 3]>, Vec<Vec<u32>>) = (vec![], vec![]);
    let (mut current, mut poly_mat): (Option<String>, Vec<Option<String>>) = (None, vec![]);
    for (ln, line) in text.lines().enumerate() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("usemtl") => current = it.next().filter(|n| *n != "none").map(crate::gltf::material_identifier),
            Some("v") => {
                let v: Vec<f32> = it.take(3).map(|s| s.parse::<f32>().map_err(|_| format!("line {}: bad vertex", ln + 1))).collect::<Result<_, _>>()?;
                if v.len() != 3 {
                    return Err(format!("line {}: vertex needs 3 numbers", ln + 1));
                }
                pts.push([v[0], v[1], v[2]]);
            }
            Some("f") => {
                let mut poly = Vec::new();
                for tok in it {
                    let idx: i64 = tok.split('/').next().unwrap_or("").parse().map_err(|_| format!("line {}: bad face index", ln + 1))?;
                    let i = if idx < 0 { pts.len() as i64 + idx } else { idx - 1 };
                    if i < 0 || i as usize >= pts.len() {
                        return Err(format!("line {}: face index {idx} out of range", ln + 1));
                    }
                    poly.push(i as u32);
                }
                if poly.len() >= 3 {
                    polys.push(poly);
                    poly_mat.push(current.clone());
                }
            }
            _ => {}
        }
    }
    let mut g = Geometry::from_polys(pts, &polys);
    let names: std::collections::BTreeSet<&String> = poly_mat.iter().flatten().collect();
    for n in names {
        g.prim_groups.insert(format!("{}{n}", crate::geo::MATERIAL_PREFIX), Arc::new(poly_mat.iter().map(|m| m.as_ref() == Some(n)).collect()));
    }
    Ok(g)
}

// ---- USD (ASCII .usda) ----

pub(crate) fn quat_from_cols(m: &crate::math::Mat34) -> ([f32; 4], [f32; 3]) {
    // Columns are the scaled basis vectors; remove scale, then convert the rotation to a quaternion.
    let col = |c: usize| [m[c], m[4 + c], m[8 + c]];
    let len = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let s = [len(col(0)), len(col(1)), len(col(2))];
    let r = |row: usize, c: usize| m[row * 4 + c] / if s[c] > 1e-12 { s[c] } else { 1.0 };
    let (r00, r01, r02, r10, r11, r12, r20, r21, r22) = (r(0, 0), r(0, 1), r(0, 2), r(1, 0), r(1, 1), r(1, 2), r(2, 0), r(2, 1), r(2, 2));
    let tr = r00 + r11 + r22;
    let q = if tr > 0.0 {
        let k = (tr + 1.0).sqrt() * 2.0;
        [0.25 * k, (r21 - r12) / k, (r02 - r20) / k, (r10 - r01) / k]
    } else if r00 > r11 && r00 > r22 {
        let k = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
        [(r21 - r12) / k, 0.25 * k, (r01 + r10) / k, (r02 + r20) / k]
    } else if r11 > r22 {
        let k = (1.0 + r11 - r00 - r22).sqrt() * 2.0;
        [(r02 - r20) / k, (r01 + r10) / k, 0.25 * k, (r12 + r21) / k]
    } else {
        let k = (1.0 + r22 - r00 - r11).sqrt() * 2.0;
        [(r10 - r01) / k, (r02 + r20) / k, (r12 + r21) / k, 0.25 * k]
    };
    (q, s) // q = (w, x, y, z), the order USD's quatf uses
}

fn usd_mesh(out: &mut String, indent: &str, name: &str, g: &Geometry) {
    let _ = writeln!(out, "{indent}def Mesh \"{name}\"\n{indent}{{");
    let pts: Vec<String> = g.p().iter().map(|p| format!("({}, {}, {})", p[0], p[1], p[2])).collect();
    let counts: Vec<String> = (0..g.n_prims()).map(|i| g.prim_points(i).len().to_string()).collect();
    let idx: Vec<String> = g.topo.indices.iter().map(|i| i.to_string()).collect();
    let _ = writeln!(out, "{indent}    int[] faceVertexCounts = [{}]", counts.join(", "));
    let _ = writeln!(out, "{indent}    int[] faceVertexIndices = [{}]", idx.join(", "));
    let _ = writeln!(out, "{indent}    point3f[] points = [{}]", pts.join(", "));
    if let Some(n) = g.point.get("N").and_then(Column::as_v3) {
        let ns: Vec<String> = n.iter().map(|p| format!("({}, {}, {})", p[0], p[1], p[2])).collect();
        let _ = writeln!(out, "{indent}    normal3f[] normals = [{}] (interpolation = \"vertex\")", ns.join(", "));
    }
    if let Some(c) = g.point.get("Cd").and_then(Column::as_v3) {
        let cs: Vec<String> = c.iter().map(|p| format!("({}, {}, {})", p[0], p[1], p[2])).collect();
        let _ = writeln!(out, "{indent}    color3f[] primvars:displayColor = [{}] (interpolation = \"vertex\")", cs.join(", "));
    }
    let _ = writeln!(out, "{indent}    uniform token subdivisionScheme = \"none\"");
    let mut by_mat: std::collections::BTreeMap<&str, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, m) in g.prim_materials().into_iter().enumerate() {
        if let Some(m) = m {
            by_mat.entry(m).or_default().push(i);
        }
    }
    if !by_mat.is_empty() {
        let _ = writeln!(out, "{indent}    uniform token subsetFamily:materialBind:type = \"nonOverlapping\"");
    }
    for (m, faces) in by_mat {
        let ids: Vec<String> = faces.iter().map(usize::to_string).collect();
        let _ = writeln!(out, "{indent}    def GeomSubset \"mat_{m}\" (\n{indent}        prepend apiSchemas = [\"MaterialBindingAPI\"]\n{indent}    )\n{indent}    {{");
        let _ = writeln!(out, "{indent}        uniform token elementType = \"face\"\n{indent}        uniform token familyName = \"materialBind\"");
        let _ = writeln!(out, "{indent}        int[] indices = [{}]", ids.join(", "));
        let _ = writeln!(out, "{indent}        rel material:binding = </World/Materials/{m}>\n{indent}    }}");
    }
    let _ = writeln!(out, "{indent}}}");
}

/// A `Materials` scope with one UsdPreviewSurface per material name (grey placeholder; the renderer binds the real set by name).
fn usd_materials(out: &mut String, names: &std::collections::BTreeSet<String>) {
    if names.is_empty() {
        return;
    }
    let _ = writeln!(out, "    def Scope \"Materials\"\n    {{");
    for n in names {
        let _ = writeln!(out, "        def Material \"{n}\"\n        {{");
        let _ = writeln!(out, "            custom string proteus:material = \"{n}\"");
        let _ = writeln!(out, "            token outputs:surface.connect = </World/Materials/{n}/Preview.outputs:surface>");
        let _ = writeln!(out, "            def Shader \"Preview\"\n            {{");
        let _ = writeln!(out, "                uniform token info:id = \"UsdPreviewSurface\"");
        let _ = writeln!(out, "                color3f inputs:diffuseColor = (0.8, 0.8, 0.8)");
        let _ = writeln!(out, "                float inputs:roughness = 0.8");
        let _ = writeln!(out, "                token outputs:surface\n            }}\n        }}");
    }
    let _ = writeln!(out, "    }}");
}

/// ASCII USD: the base mesh as a `Mesh`, lazy instances as a native `PointInstancer`
/// (positions/orientations/scales per instance + prototype meshes), so instancing survives export.
pub fn to_usda(g: &Geometry) -> String {
    let mut s = String::from("#usda 1.0\n(\n    defaultPrim = \"World\"\n    metersPerUnit = 1\n    upAxis = \"Y\"\n    doc = \"exported by proteus\"\n)\n\ndef Xform \"World\"\n{\n");
    let mut names: std::collections::BTreeSet<String> = g.material_names().into_iter().collect();
    for p in g.instances.iter().flat_map(|i| i.protos.iter()) {
        names.extend(p.realize().material_names());
    }
    usd_materials(&mut s, &names);
    if g.n_prims() > 0 {
        usd_mesh(&mut s, "    ", "Geometry", g);
    }
    if let Some(inst) = &g.instances {
        let _ = writeln!(s, "    def PointInstancer \"Instances\"\n    {{");
        let protos: Vec<Geometry> = inst.protos.iter().map(|p| p.realize()).collect();
        let targets: Vec<String> = (0..protos.len()).map(|i| format!("</World/Instances/Prototypes/Proto{i}>")).collect();
        let _ = writeln!(s, "        rel prototypes = [{}]", targets.join(", "));
        let ids: Vec<String> = inst.proto_id.iter().map(|i| i.to_string()).collect();
        let _ = writeln!(s, "        int[] protoIndices = [{}]", ids.join(", "));
        let mut pos = Vec::with_capacity(inst.len());
        let mut ori = Vec::with_capacity(inst.len());
        let mut scl = Vec::with_capacity(inst.len());
        for m in inst.xf.iter() {
            let (q, sc) = quat_from_cols(m);
            pos.push(format!("({}, {}, {})", m[3], m[7], m[11]));
            ori.push(format!("({}, {}, {}, {})", q[0], q[1], q[2], q[3]));
            scl.push(format!("({}, {}, {})", sc[0], sc[1], sc[2]));
        }
        let _ = writeln!(s, "        point3f[] positions = [{}]", pos.join(", "));
        let _ = writeln!(s, "        quatf[] orientations = [{}]", ori.join(", "));
        let _ = writeln!(s, "        float3[] scales = [{}]", scl.join(", "));
        let _ = writeln!(s, "        def Scope \"Prototypes\"\n        {{");
        for (i, p) in protos.iter().enumerate() {
            usd_mesh(&mut s, "            ", &format!("Proto{i}"), p);
        }
        let _ = writeln!(s, "        }}\n    }}");
    }
    s.push_str("}\n");
    s
}

#[cfg(test)]
mod usd_tests {
    use super::*;
    use crate::{math, Instances};

    #[test]
    fn quaternion_round_trips_rotation_and_scale() {
        let m = math::trs([1.0, 2.0, 3.0], [20.0, 55.0, -80.0], [2.0, 2.0, 2.0]);
        let ([w, x, y, z], s) = quat_from_cols(&m);
        assert!((w * w + x * x + y * y + z * z - 1.0).abs() < 1e-5);
        assert!(s.iter().all(|v| (v - 2.0).abs() < 1e-4));
        // Rebuild the rotation from the quaternion and compare with the normalized matrix.
        let rot = [[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)], [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)], [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)]];
        for r in 0..3 {
            for c in 0..3 {
                assert!((rot[r][c] - m[r * 4 + c] / 2.0).abs() < 1e-4, "R[{r}][{c}]");
            }
        }
    }

    fn painted_quad_pair() -> Geometry {
        let mut g = Geometry::from_polys(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0]], &[vec![0, 1, 2, 3], vec![1, 4, 5, 2]]);
        g.assign_material("stone", &[true, false]);
        g.assign_material("grass", &[false, true]);
        g
    }

    #[test]
    fn usda_binds_materials_by_name_with_geom_subsets() {
        let u = to_usda(&painted_quad_pair());
        assert!(u.contains("def Material \"stone\"") && u.contains("def Material \"grass\""));
        assert_eq!(u.matches("def GeomSubset").count(), 2);
        assert!(u.contains("rel material:binding = </World/Materials/stone>") && u.contains("int[] indices = [1]"));
        assert!(u.contains("subsetFamily:materialBind:type = \"nonOverlapping\""));
        assert_eq!(u.matches('{').count(), u.matches('}').count(), "balanced braces");
        assert!(!to_usda(&Geometry::new()).contains("Materials"), "no materials, no scope");
    }

    #[test]
    fn obj_writes_usemtl_and_reads_it_back() {
        let o = to_obj(&painted_quad_pair());
        assert_eq!(o.matches("usemtl").count(), 2);
        let back = load_obj(&o).unwrap();
        assert_eq!(back.material_names(), ["grass", "stone"]);
        assert_eq!(back.prim_materials(), vec![Some("stone"), Some("grass")]);
    }

    #[test]
    fn usda_keeps_instances_as_a_point_instancer() {
        let tri = Geometry::from_polys(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[vec![0, 1, 2]]);
        let mut g = tri.clone();
        g.instances = Some(Instances { protos: vec![std::sync::Arc::new(tri)], proto_id: std::sync::Arc::new(vec![0, 0, 0]), xf: std::sync::Arc::new(vec![math::IDENTITY; 3]), attrs: Default::default() });
        let u = to_usda(&g);
        assert!(u.starts_with("#usda 1.0") && u.contains("upAxis = \"Y\"") && u.contains("def PointInstancer"));
        assert!(u.contains("int[] protoIndices = [0, 0, 0]") && u.contains("Proto0"));
        assert_eq!(u.matches("def Mesh").count(), 2, "base mesh + one prototype");
        assert_eq!(u.matches('{').count(), u.matches('}').count(), "balanced braces");
    }
}
