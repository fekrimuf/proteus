//! Geometry validation: machine-readable issues with fix hints, so agents can self-correct.

use crate::geo::Geometry;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct Issue {
    pub severity: &'static str, // "error" | "warning" | "info"
    pub code: &'static str,
    pub message: String,
    pub fix: &'static str,
}

impl Issue {
    fn new(severity: &'static str, code: &'static str, message: String, fix: &'static str) -> Self {
        Issue { severity, code, message, fix }
    }
    pub fn to_json(&self) -> Value {
        json!({"severity": self.severity, "code": self.code, "message": self.message, "fix": self.fix})
    }
}

pub fn validate(g: &Geometry) -> Vec<Issue> {
    let mut out = Vec::new();
    let np = g.n_points();

    for (dom, set, n) in [("point", &g.point, np), ("vertex", &g.vertex, g.n_vertices()), ("primitive", &g.prim, g.n_prims())] {
        for (name, c) in set.iter() {
            if c.len() != n {
                out.push(Issue::new("error", "attr_length", format!("{dom} attribute '{name}' has {} rows, expected {n}", c.len()), "recreate the attribute with one value per element"));
            }
        }
    }

    let bad = g.p().iter().filter(|p| p.iter().any(|c| !c.is_finite())).count();
    if bad > 0 {
        out.push(Issue::new("error", "non_finite_points", format!("{bad} point(s) have NaN/inf positions"), "check upstream divisions by zero, huge noise amplitude or scale 0"));
    }

    let oob = g.topo.indices.iter().filter(|&&i| i as usize >= np).count();
    if oob > 0 {
        out.push(Issue::new("error", "index_out_of_range", format!("{oob} vertex index(es) reference missing points"), "a node produced inconsistent topology; report the node via explain()"));
    }

    if oob == 0 && bad == 0 {
        let (mut degenerate, mut dup) = (0usize, 0usize);
        let mut used = vec![false; np];
        for i in 0..g.n_prims() {
            let pr = g.prim_points(i);
            for &v in pr {
                used[v as usize] = true;
            }
            let mut s = pr.to_vec();
            s.sort_unstable();
            s.dedup();
            if s.len() != pr.len() {
                dup += 1;
            }
            let p0 = g.p()[pr[0] as usize];
            let mut area = [0.0f32; 3];
            for k in 1..pr.len().saturating_sub(1) {
                let (a, b) = (g.p()[pr[k] as usize], g.p()[pr[k + 1] as usize]);
                let c = crate::math::cross(crate::math::sub(a, p0), crate::math::sub(b, p0));
                for j in 0..3 {
                    area[j] += c[j];
                }
            }
            if (area[0] * area[0] + area[1] * area[1] + area[2] * area[2]) < 1e-18 {
                degenerate += 1;
            }
        }
        if dup > 0 {
            out.push(Issue::new("warning", "repeated_vertex", format!("{dup} primitive(s) reference the same point twice"), "run a fuse/clean step or check merge/copy logic"));
        }
        if degenerate > 0 {
            out.push(Issue::new("warning", "degenerate_faces", format!("{degenerate} primitive(s) have ~zero area"), "remove zero-area faces; check scale-to-zero or collapsed noise"));
        }
        if g.n_prims() > 0 {
            let unused = used.iter().filter(|u| !**u).count();
            if unused > 0 {
                out.push(Issue::new("info", "unreferenced_points", format!("{unused} point(s) belong to no primitive"), "delete them if unintended"));
            }
        }
        if g.n_prims() > 0 && g.n_prims() <= 2_000_000 {
            let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
            for i in 0..g.n_prims() {
                let pr = g.prim_points(i);
                for k in 0..pr.len() {
                    let (a, b) = (pr[k], pr[(k + 1) % pr.len()]);
                    *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                }
            }
            let nonman = edges.values().filter(|&&c| c > 2).count();
            let boundary = edges.values().filter(|&&c| c == 1).count();
            if nonman > 0 {
                out.push(Issue::new("warning", "non_manifold_edges", format!("{nonman} edge(s) shared by more than 2 faces"), "split or fuse the offending geometry before booleans/sim"));
            }
            out.push(Issue::new("info", "boundary_edges", format!("{boundary} open boundary edge(s); watertight = 0"), "expected for grids/terrain; close the mesh if a solid is required"));
        }
    }

    if let Some((lo, hi)) = g.bounds() {
        let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        let big = size.iter().cloned().fold(0.0f32, f32::max);
        if big > 1.0e5 {
            out.push(Issue::new("warning", "scale_huge", format!("bounding size {big} m is huge"), "units are meters; check scale parameters"));
        } else if big > 0.0 && big < 1.0e-3 {
            out.push(Issue::new("warning", "scale_tiny", format!("bounding size {big} m is tiny"), "units are meters; check scale parameters"));
        }
    } else {
        out.push(Issue::new("warning", "empty", "geometry is empty".into(), "check that inputs are connected and counts are > 0"));
    }

    for (name, v) in &g.volumes {
        if v.data.iter().any(|x| !x.is_finite()) {
            out.push(Issue::new("error", "non_finite_volume", format!("volume '{name}' contains NaN/inf"), "check noise/offset parameters upstream"));
        }
    }
    if let Some(inst) = &g.instances {
        let np = inst.protos.len() as u32;
        if inst.proto_id.iter().any(|&i| i >= np) {
            out.push(Issue::new("error", "bad_proto_id", "instance references a missing prototype".into(), "rebuild the instance table"));
        }
        if inst.xf.iter().any(|m| m.iter().any(|c| !c.is_finite())) {
            out.push(Issue::new("error", "non_finite_instance_xf", "instance transform contains NaN/inf".into(), "check pscale/rotation inputs of copy_to_points"));
        }
    }
    out
}
