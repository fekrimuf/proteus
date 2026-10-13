//! Geometry v2: columnar copy-on-write attributes over point / vertex / primitive / detail
//! domains, CSR polygon topology, groups, and lazy instance tables.

use crate::column::{AttrSet, Column};
use crate::math::{self, Mat34};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Topology {
    /// `offsets[i]..offsets[i+1]` indexes `indices` for primitive `i`; `indices` are point ids.
    pub offsets: Vec<u32>,
    pub indices: Vec<u32>,
}

impl Default for Topology {
    fn default() -> Self {
        Topology { offsets: vec![0], indices: vec![] }
    }
}

/// Lazy instances: a prototype reference plus a transform and attributes per instance.
/// Nothing is copied until `Geometry::realize` (or an exporter) asks for it.
#[derive(Clone, Debug, Default)]
pub struct Instances {
    pub protos: Vec<Arc<Geometry>>,
    pub proto_id: Arc<Vec<u32>>,
    pub xf: Arc<Vec<Mat34>>,
    pub attrs: AttrSet,
}

impl Instances {
    pub fn len(&self) -> usize {
        self.xf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.xf.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Geometry {
    pub point: AttrSet,
    pub vertex: AttrSet,
    pub prim: AttrSet,
    pub detail: BTreeMap<String, Value>,
    pub topo: Arc<Topology>,
    pub point_groups: BTreeMap<String, Arc<Vec<bool>>>,
    pub prim_groups: BTreeMap<String, Arc<Vec<bool>>>,
    pub instances: Option<Instances>,
    /// Named dense scalar fields (e.g. "sdf", "density").
    pub volumes: BTreeMap<String, Arc<Volume>>,
}

impl Geometry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_polys(points: Vec<[f32; 3]>, polys: &[Vec<u32>]) -> Self {
        let mut g = Geometry::new();
        g.point.set("P", Column::from_v3(points));
        let mut t = Topology::default();
        for p in polys {
            t.indices.extend_from_slice(p);
            t.offsets.push(t.indices.len() as u32);
        }
        g.topo = Arc::new(t);
        g
    }

    pub fn from_points(points: Vec<[f32; 3]>) -> Self {
        Self::from_polys(points, &[])
    }

    pub fn n_points(&self) -> usize {
        self.point.get("P").map_or(0, Column::len)
    }
    pub fn n_prims(&self) -> usize {
        self.topo.offsets.len() - 1
    }
    pub fn n_vertices(&self) -> usize {
        self.topo.indices.len()
    }
    pub fn p(&self) -> &[[f32; 3]] {
        self.point.get("P").and_then(Column::as_v3).unwrap_or(&[])
    }
    /// Mutable positions; copies the column only if it is shared (copy-on-write).
    pub fn p_mut(&mut self) -> &mut Vec<[f32; 3]> {
        if !matches!(self.point.get("P"), Some(Column::V3(_))) {
            self.point.set("P", Column::from_v3(vec![]));
        }
        match self.point.0.get_mut("P") {
            Some(Column::V3(a)) => Arc::make_mut(a),
            // not reachable: `set` above made P a V3 column. Loud on purpose (a silent scratch buffer would lose writes); the tool guard turns it into an error.
            #[allow(clippy::unreachable)]
            _ => unreachable!("P was just set to a V3 column"),
        }
    }
    pub fn prim_points(&self, i: usize) -> &[u32] {
        &self.topo.indices[self.topo.offsets[i] as usize..self.topo.offsets[i + 1] as usize]
    }
    pub fn n_instances(&self) -> usize {
        self.instances.as_ref().map_or(0, Instances::len)
    }

    /// Fan-triangulated point-index triples.
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        let mut t = Vec::new();
        for i in 0..self.n_prims() {
            let pr = self.prim_points(i);
            for k in 1..pr.len().saturating_sub(1) {
                t.push([pr[0], pr[k], pr[k + 1]]);
            }
        }
        t
    }

    /// Bounds of the base geometry only (instances excluded).
    pub fn base_bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let first = *self.p().first()?;
        let (mut lo, mut hi) = (first, first);
        for q in self.p() {
            for a in 0..3 {
                lo[a] = lo[a].min(q[a]);
                hi[a] = hi[a].max(q[a]);
            }
        }
        Some((lo, hi))
    }

    /// Bounds including instances (prototype box corners pushed through each transform).
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut acc = self.base_bounds();
        if let Some(inst) = &self.instances {
            let pb: Vec<_> = inst.protos.iter().map(|p| p.bounds()).collect();
            for (i, xf) in inst.xf.iter().enumerate() {
                let Some((lo, hi)) = pb[inst.proto_id[i] as usize] else { continue };
                for c in 0..8 {
                    let corner = [
                        if c & 1 == 0 { lo[0] } else { hi[0] },
                        if c & 2 == 0 { lo[1] } else { hi[1] },
                        if c & 4 == 0 { lo[2] } else { hi[2] },
                    ];
                    let q = math::xform_point(xf, corner);
                    acc = Some(match acc {
                        None => (q, q),
                        Some((mut l, mut h)) => {
                            for a in 0..3 {
                                l[a] = l[a].min(q[a]);
                                h[a] = h[a].max(q[a]);
                            }
                            (l, h)
                        }
                    });
                }
            }
        }
        acc
    }

    /// Append `other`, offsetting indices. Missing attributes are zero-filled.
    pub fn append(&mut self, other: &Geometry) -> Result<(), String> {
        let (np, nv, nr) = (self.n_points(), self.n_vertices(), self.n_prims());
        let (op, ov, or) = (other.n_points(), other.n_vertices(), other.n_prims());
        self.point.append(np, &other.point, op)?;
        self.vertex.append(nv, &other.vertex, ov)?;
        self.prim.append(nr, &other.prim, or)?;
        if or > 0 || ov > 0 {
            let t = Arc::make_mut(&mut self.topo);
            let base = t.indices.len() as u32;
            t.indices.extend(other.topo.indices.iter().map(|i| i + np as u32));
            t.offsets.extend(other.topo.offsets[1..].iter().map(|o| o + base));
        }
        merge_groups(&mut self.point_groups, np, &other.point_groups, op);
        merge_groups(&mut self.prim_groups, nr, &other.prim_groups, or);
        for (k, v) in &other.detail {
            self.detail.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.volumes {
            self.volumes.entry(k.clone()).or_insert_with(|| v.clone());
        }
        if let Some(oi) = &other.instances {
            match &mut self.instances {
                None => self.instances = Some(oi.clone()),
                Some(si) => {
                    let (n_s, n_o) = (si.len(), oi.len());
                    let shift = si.protos.len() as u32;
                    si.protos.extend(oi.protos.iter().cloned());
                    Arc::make_mut(&mut si.proto_id).extend(oi.proto_id.iter().map(|i| i + shift));
                    Arc::make_mut(&mut si.xf).extend_from_slice(&oi.xf);
                    si.attrs.append(n_s, &oi.attrs, n_o)?;
                }
            }
        }
        Ok(())
    }

    /// Turn instances into real geometry (recursively for nested instancing).
    pub fn realize(&self) -> Geometry {
        let mut out = self.clone();
        out.instances = None;
        let Some(inst) = &self.instances else { return out };
        let protos: Vec<Geometry> = inst.protos.iter().map(|p| p.realize()).collect();
        for (i, xf) in inst.xf.iter().enumerate() {
            let mut g = protos[inst.proto_id[i] as usize].clone();
            for q in g.p_mut() {
                *q = math::xform_point(xf, *q);
            }
            // Realized geometry cannot fail to append: attributes come from the same protos.
            let _ = out.append(&g);
        }
        out
    }

    /// Merkle content hash over all columns, topology, groups, detail and instances.
    pub fn content_hash(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(b"ptgeo/1");
        self.hash_into(&mut h);
        *h.finalize().as_bytes()
    }

    fn hash_into(&self, h: &mut blake3::Hasher) {
        for (dom, set) in [("point", &self.point), ("vertex", &self.vertex), ("prim", &self.prim)] {
            h.update(dom.as_bytes());
            hash_attrs(set, h);
        }
        h.update(bytemuck::cast_slice(&self.topo.offsets));
        h.update(bytemuck::cast_slice(&self.topo.indices));
        for (dom, gs) in [("pg", &self.point_groups), ("rg", &self.prim_groups)] {
            for (name, bits) in gs {
                h.update(dom.as_bytes());
                h.update(name.as_bytes());
                let bytes: Vec<u8> = bits.iter().map(|&b| b as u8).collect();
                h.update(blake3::hash(&bytes).as_bytes());
            }
        }
        for (k, v) in &self.detail {
            h.update(k.as_bytes());
            h.update(v.to_string().as_bytes());
        }
        if let Some(i) = &self.instances {
            h.update(b"inst");
            for p in &i.protos {
                h.update(&p.content_hash());
            }
            h.update(bytemuck::cast_slice(i.proto_id.as_slice()));
            h.update(bytemuck::cast_slice(i.xf.as_slice()));
            hash_attrs(&i.attrs, h);
        }
        for (name, v) in &self.volumes {
            h.update(b"vol");
            h.update(name.as_bytes());
            h.update(bytemuck::cast_slice(&[v.dims[0] as u64, v.dims[1] as u64, v.dims[2] as u64]));
            h.update(bytemuck::cast_slice(&v.origin));
            h.update(&v.voxel.to_le_bytes());
            Column::F32(v.data.clone()).hash_into(h);
        }
    }

    /// Compact summary an agent can read to sanity-check a cook result.
    pub fn stats(&self) -> Value {
        let b = self.bounds();
        let summ = |s: &AttrSet| -> Value {
            Value::Object(s.iter().map(|(k, c)| (k.clone(), c.summary())).collect())
        };
        json!({
            "points": self.n_points(),
            "vertices": self.n_vertices(),
            "primitives": self.n_prims(),
            "triangles": self.triangles().len(),
            "instances": self.n_instances(),
            "bounds_min": b.map(|b| b.0),
            "bounds_max": b.map(|b| b.1),
            "size": b.map(|b| [b.1[0]-b.0[0], b.1[1]-b.0[1], b.1[2]-b.0[2]]),
            "point_attributes": summ(&self.point),
            "primitive_attributes": summ(&self.prim),
            "instance_attributes": self.instances.as_ref().map(|i| summ(&i.attrs)),
            "volumes": self.volumes.iter().map(|(k, v)| (k.clone(), json!({"dims": v.dims, "voxel": v.voxel, "origin": v.origin, "range": Column::F32(v.data.clone()).summary()}))).collect::<BTreeMap<_, _>>(),
            "point_groups": self.point_groups.iter().map(|(k, v)| (k.clone(), v.iter().filter(|b| **b).count())).collect::<BTreeMap<_, _>>(),
            "primitive_groups": self.prim_groups.iter().map(|(k, v)| (k.clone(), v.iter().filter(|b| **b).count())).collect::<BTreeMap<_, _>>(),
            "detail": self.detail,
        })
    }
}

fn hash_attrs(set: &AttrSet, h: &mut blake3::Hasher) {
    for (name, col) in set.iter() {
        h.update(name.as_bytes());
        col.hash_into(h);
    }
}

fn merge_groups(
    a: &mut BTreeMap<String, Arc<Vec<bool>>>,
    n_a: usize,
    b: &BTreeMap<String, Arc<Vec<bool>>>,
    n_b: usize,
) {
    for (k, bits) in a.iter_mut() {
        let v = Arc::make_mut(bits);
        match b.get(k) {
            Some(o) => v.extend_from_slice(o),
            None => v.resize(n_a + n_b, false),
        }
    }
    for (k, o) in b {
        if !a.contains_key(k) {
            let mut v = vec![false; n_a];
            v.extend_from_slice(o);
            a.insert(k.clone(), Arc::new(v));
        }
    }
}

pub fn gather_groups(g: &BTreeMap<String, Arc<Vec<bool>>>, idx: &[u32]) -> BTreeMap<String, Arc<Vec<bool>>> {
    g.iter().map(|(k, v)| (k.clone(), Arc::new(idx.iter().map(|&i| v[i as usize]).collect()))).collect()
}

/// Prefix of the primitive groups that carry material names: a primitive in group `mat:rock_granite` is
/// bound to the material `rock_granite`. Groups survive merge, filter, blast and realize, so materials
/// need no attribute type of their own.
pub const MATERIAL_PREFIX: &str = "mat:";

/// A material name is a plain identifier, valid unchanged in glTF, USD, OBJ and Blender.
pub fn valid_material_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic()) && c.all(|x| x.is_ascii_alphanumeric() || x == '_') && n.len() <= 64
}

impl Geometry {
    /// Sorted names of the materials present on the base geometry (instance prototypes not included).
    pub fn material_names(&self) -> Vec<String> {
        self.prim_groups.keys().filter_map(|k| k.strip_prefix(MATERIAL_PREFIX)).map(String::from).collect()
    }

    /// Material of every primitive: the first `mat:` group (alphabetical) that holds it, `None` if unassigned.
    pub fn prim_materials(&self) -> Vec<Option<&str>> {
        let mut out: Vec<Option<&str>> = vec![None; self.n_prims()];
        for (k, bits) in &self.prim_groups {
            let Some(name) = k.strip_prefix(MATERIAL_PREFIX) else { continue };
            for (slot, &b) in out.iter_mut().zip(bits.iter()) {
                if b && slot.is_none() {
                    *slot = Some(name);
                }
            }
        }
        out
    }

    /// Bind `name` to the selected primitives, taking them out of every other material (a primitive has one material).
    pub fn assign_material(&mut self, name: &str, sel: &[bool]) {
        let key = format!("{MATERIAL_PREFIX}{name}");
        let mut bits: Vec<bool> = self.prim_groups.get(&key).map_or_else(|| vec![false; sel.len()], |g| g.to_vec());
        bits.resize(sel.len(), false);
        for (k, g) in self.prim_groups.iter_mut() {
            if k.starts_with(MATERIAL_PREFIX) && *k != key && g.iter().zip(sel).any(|(&a, &s)| a && s) {
                for (a, &s) in Arc::make_mut(g).iter_mut().zip(sel) {
                    *a &= !s;
                }
            }
        }
        for (b, &s) in bits.iter_mut().zip(sel) {
            *b |= s;
        }
        self.prim_groups.insert(key, Arc::new(bits));
        self.prim_groups.retain(|k, g| !k.starts_with(MATERIAL_PREFIX) || g.iter().any(|&b| b));
    }

    /// Keep selected points and primitives (a primitive also needs all of its points kept).
    /// Attributes and groups are gathered; instances and detail are preserved.
    pub fn filter(&self, point_keep: &[bool], prim_keep: &[bool]) -> Geometry {
        let mut remap = vec![u32::MAX; self.n_points()];
        let mut pidx: Vec<u32> = Vec::new();
        for (i, &k) in point_keep.iter().enumerate() {
            if k {
                remap[i] = pidx.len() as u32;
                pidx.push(i as u32);
            }
        }
        let mut topo = Topology::default();
        let (mut ridx, mut vidx): (Vec<u32>, Vec<u32>) = (vec![], vec![]);
        for i in 0..self.n_prims() {
            let pts = self.prim_points(i);
            if !prim_keep[i] || pts.iter().any(|&p| remap[p as usize] == u32::MAX) {
                continue;
            }
            ridx.push(i as u32);
            for (k, &p) in pts.iter().enumerate() {
                topo.indices.push(remap[p as usize]);
                vidx.push(self.topo.offsets[i] + k as u32);
            }
            topo.offsets.push(topo.indices.len() as u32);
        }
        Geometry {
            point: self.point.gather(&pidx),
            vertex: self.vertex.gather(&vidx),
            prim: self.prim.gather(&ridx),
            detail: self.detail.clone(),
            topo: Arc::new(topo),
            point_groups: gather_groups(&self.point_groups, &pidx),
            prim_groups: gather_groups(&self.prim_groups, &ridx),
            instances: self.instances.clone(),
            volumes: self.volumes.clone(),
        }
    }

    /// Replace the polygon list (point set unchanged). Prim/vertex attributes and prim
    /// groups are dropped because the primitives change.
    pub fn with_polys(&self, polys: &[Vec<u32>]) -> Geometry {
        let mut g = Geometry::from_polys(self.p().to_vec(), polys);
        g.point = self.point.clone();
        g.point_groups = self.point_groups.clone();
        g.detail = self.detail.clone();
        g.instances = self.instances.clone();
        g.volumes = self.volumes.clone();
        g
    }
}

/// Dense scalar field on a regular grid: sample (i,j,k) sits at `origin + (i,j,k) * voxel`.
/// For signed distance fields, negative = inside.
#[derive(Clone, Debug)]
pub struct Volume {
    pub dims: [usize; 3],
    pub origin: [f32; 3],
    pub voxel: f32,
    pub data: Arc<Vec<f32>>,
}

impl Volume {
    pub fn new(dims: [usize; 3], origin: [f32; 3], voxel: f32, fill: f32) -> Volume {
        Volume { dims, origin, voxel, data: Arc::new(vec![fill; dims[0] * dims[1] * dims[2]]) }
    }
    #[inline]
    pub fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.dims[1] + j) * self.dims[0] + i
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn pos(&self, i: usize, j: usize, k: usize) -> [f32; 3] {
        [self.origin[0] + i as f32 * self.voxel, self.origin[1] + j as f32 * self.voxel, self.origin[2] + k as f32 * self.voxel]
    }
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let hi = self.pos(self.dims[0].saturating_sub(1), self.dims[1].saturating_sub(1), self.dims[2].saturating_sub(1));
        (self.origin, hi)
    }
    /// Trilinear sample; outside the grid, extends the nearest value plus the distance to the
    /// grid box (a conservative signed-distance continuation).
    pub fn sample(&self, p: [f32; 3]) -> f32 {
        let (lo, hi) = self.bounds();
        let mut outside = 0.0f32;
        let mut q = p;
        for a in 0..3 {
            if p[a] < lo[a] {
                outside += (lo[a] - p[a]) * (lo[a] - p[a]);
                q[a] = lo[a];
            } else if p[a] > hi[a] {
                outside += (p[a] - hi[a]) * (p[a] - hi[a]);
                q[a] = hi[a];
            }
        }
        let g = [(q[0] - lo[0]) / self.voxel, (q[1] - lo[1]) / self.voxel, (q[2] - lo[2]) / self.voxel];
        let base = [g[0].floor(), g[1].floor(), g[2].floor()];
        let mut ij = [0usize; 3];
        let mut f = [0.0f32; 3];
        for a in 0..3 {
            let b = (base[a] as i64).clamp(0, self.dims[a] as i64 - 1) as usize;
            ij[a] = b.min(self.dims[a].saturating_sub(2));
            f[a] = (g[a] - ij[a] as f32).clamp(0.0, 1.0);
        }
        let at = |di: usize, dj: usize, dk: usize| {
            let (i, j, k) = ((ij[0] + di).min(self.dims[0] - 1), (ij[1] + dj).min(self.dims[1] - 1), (ij[2] + dk).min(self.dims[2] - 1));
            self.data[self.idx(i, j, k)]
        };
        let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let v = l(
            l(l(at(0, 0, 0), at(1, 0, 0), f[0]), l(at(0, 1, 0), at(1, 1, 0), f[0]), f[1]),
            l(l(at(0, 0, 1), at(1, 0, 1), f[0]), l(at(0, 1, 1), at(1, 1, 1), f[0]), f[1]),
            f[2],
        );
        v + outside.sqrt()
    }
}
