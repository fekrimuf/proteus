//! Attribute-flow analysis (PLAN Bet 4): predict, without cooking, which attributes exist
//! after each node, and report precise errors (with fixes) for reads of missing attributes,
//! type mismatches, and ops that need a surface or points.

use pt_core::{Column, Geometry, Params};
use pt_vex::Ty;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub type AttrTys = BTreeMap<String, Ty>;

#[derive(Clone, Debug, Default)]
pub struct GeoEnv {
    pub point: AttrTys,
    pub prim: AttrTys,
    /// Some(..) when the geometry carries an instance table with these attributes.
    pub inst: Option<AttrTys>,
    pub has_surface: bool,
    pub has_points: bool,
    /// Still a regular grid heightfield (needed by hf_* ops).
    pub is_grid: bool,
    /// Names of volumes present (e.g. "sdf").
    pub volumes: std::collections::BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct FlowDiag {
    pub message: String,
    pub fix: String,
}

fn fd(message: impl Into<String>, fix: impl Into<String>) -> FlowDiag {
    FlowDiag { message: message.into(), fix: fix.into() }
}

pub fn col_ty(c: &Column) -> Option<Ty> {
    match c {
        Column::F32(_) => Some(Ty::Float),
        Column::V3(_) => Some(Ty::Vec3),
        _ => None,
    }
}

fn set_env(s: &pt_core::AttrSet) -> AttrTys {
    s.iter().filter(|(k, _)| !k.starts_with("__")).filter_map(|(k, c)| col_ty(c).map(|t| (k.clone(), t))).collect()
}

impl GeoEnv {
    pub fn from_geo(g: &Geometry) -> GeoEnv {
        GeoEnv {
            point: set_env(&g.point),
            prim: set_env(&g.prim),
            inst: g.instances.as_ref().map(|i| set_env(&i.attrs)),
            has_surface: g.triangles().first().is_some() || g.instances.is_some(),
            has_points: g.n_points() > 0,
            is_grid: g.detail.contains_key("grid"),
            volumes: g.volumes.keys().cloned().collect(),
        }
    }

    pub fn to_json(&self) -> Value {
        let m = |a: &AttrTys| json!(a.iter().map(|(k, t)| (k.clone(), t.name())).collect::<BTreeMap<_, _>>());
        json!({"point": m(&self.point), "primitive": m(&self.prim), "instance": self.inst.as_ref().map(m),
               "has_surface": self.has_surface, "has_points": self.has_points})
    }

    fn base_points(extra: &[(&str, Ty)], surface: bool) -> GeoEnv {
        let mut point: AttrTys = BTreeMap::from([("P".to_string(), Ty::Vec3)]);
        for (k, t) in extra {
            point.insert(k.to_string(), *t);
        }
        GeoEnv { point, has_surface: surface, has_points: true, ..Default::default() }
    }
}

fn fmt_names(a: &AttrTys) -> String {
    format!("{:?}", a.keys().collect::<Vec<_>>())
}

/// Inputs absent (unconnected) are reported by the graph checker, not here.
pub fn effect(kind: &str, p: &Params, ins: &[Option<GeoEnv>]) -> Result<GeoEnv, Vec<FlowDiag>> {
    use pt_core::ParamExt;
    let in0 = || ins.first().cloned().flatten().unwrap_or_default();
    let class_set = |e: &mut GeoEnv, class: &str| -> Result<(), FlowDiag> {
        match class {
            "point" | "prim" => Ok(()),
            o => Err(fd(format!("class must be 'point' or 'prim', got '{o}'"), "set class to point or prim")),
        }
        .map(|_| {
            let _ = e;
        })
    };
    match kind {
        "grid" => {
            let mut e = GeoEnv::base_points(&[], true);
            e.is_grid = true;
            Ok(e)
        }
        "hf_slope" | "hf_erode_thermal" | "hf_erode_hydraulic" | "hf_terrace" | "hf_flow" => {
            let mut e = in0();
            if !e.is_grid {
                return Err(vec![fd(format!("{kind} needs a grid heightfield but its input is not a regular grid"), "feed it from `grid` (optionally through noise/transform/wrangle/smooth) without merge, blast, fuse or subdivide in between")]);
            }
            let add: &[&str] = match kind {
                "hf_slope" => &["slope", "slope_deg"],
                "hf_erode_hydraulic" => &["erosion", "deposit", "water", "flow"],
                "hf_flow" => &["filled", "fill_depth", "flow_acc", "twi", "stream", "lake"],
                _ => &[],
            };
            for a in add {
                e.point.insert((*a).to_string(), Ty::Float);
            }
            if kind == "hf_flow" && p.f("river_width") > 0.0 {
                for a in ["river", "river_sd", "river_w", "water_y"] {
                    e.point.insert(a.to_string(), Ty::Float);
                }
            }
            Ok(e)
        }
        "hf_fit" | "hf_hole" | "hf_refine" => {
            let mut e = in0();
            let needs_grid = if kind == "hf_fit" { ins.get(1).cloned().flatten().unwrap_or_default().is_grid } else { e.is_grid };
            if !needs_grid {
                let which = if kind == "hf_fit" { "input 1 (the outer level)" } else { "its input" };
                return Err(vec![fd(format!("{kind} needs a grid heightfield on {which}"), "feed it from `grid`, `dem` or `hf_refine` before any hf_hole, merge, blast or fuse")]);
            }
            match kind {
                "hf_hole" => e.is_grid = false,
                "hf_refine" => {
                    e.point.insert("edge".into(), Ty::Float);
                }
                _ => {}
            }
            Ok(e)
        }
        "pop_sim" => {
            if !in0().has_points {
                return Err(vec![fd("pop_sim needs emitter points on input 0", "connect scatter, a grid, or any geometry with points")]);
            }
            if let Some(Some(c)) = ins.get(1) {
                if !c.volumes.contains("sdf") {
                    return Err(vec![fd("pop_sim collider (input 1) has no 'sdf' volume", "build the collider with sdf_from_mesh or sdf_shape")]);
                }
            }
            Ok(GeoEnv::base_points(&[("v", Ty::Vec3), ("age", Ty::Float), ("pscale", Ty::Float), ("id", Ty::Float)], false))
        }
        "cloth_sim" => {
            let e = in0();
            if !e.has_surface {
                return Err(vec![fd("cloth_sim needs a polygon mesh on input 0", "connect a grid or any surface")]);
            }
            if let Some(Some(c)) = ins.get(1) {
                if !c.volumes.contains("sdf") {
                    return Err(vec![fd("cloth_sim collider (input 1) has no 'sdf' volume", "build the collider with sdf_from_mesh or sdf_shape")]);
                }
            }
            Ok(e)
        }
        "smoke_sim" => {
            if !in0().has_points {
                return Err(vec![fd("smoke_sim needs source points on input 0", "connect a point/scatter/geometry source")]);
            }
            Ok(GeoEnv { volumes: ["density".to_string(), "temperature".to_string()].into(), ..Default::default() })
        }
        "volume_to_sdf" => {
            let mut e = in0();
            let name = p.s("volume");
            if !e.volumes.contains(&name) {
                return Err(vec![fd(format!("volume_to_sdf: input has no volume '{name}' (has {:?})", e.volumes), "set `volume` to an existing volume name, e.g. density from smoke_sim")]);
            }
            e.volumes.insert("sdf".into());
            Ok(e)
        }
        "rbd_shatter" => {
            if !in0().has_surface {
                return Err(vec![fd("rbd_shatter needs a closed polygon mesh", "connect a box/sphere/tube/sdf_to_mesh result")]);
            }
            let mut e = GeoEnv::base_points(&[("N", Ty::Vec3)], true);
            e.prim.insert("piece".into(), Ty::Float);
            Ok(e)
        }
        "rbd_sim" => {
            let e = in0();
            if !e.has_surface {
                return Err(vec![fd("rbd_sim needs the pieces mesh (polygons) on input 0", "connect rbd_shatter or any mesh made of separate pieces")]);
            }
            Ok(e)
        }
        "rbd_settle" => {
            let mut e = in0();
            let ground = ins.get(1).cloned().flatten().unwrap_or_default();
            if !ground.has_surface {
                return Err(vec![fd("rbd_settle needs a surface to land on (polygons) on input 1", "connect the terrain or floor the objects should rest on")]);
            }
            match &mut e.inst {
                Some(i) => {
                    i.insert("settle_move".into(), Ty::Float);
                    i.insert("settled".into(), Ty::Float);
                }
                None => return Err(vec![fd("rbd_settle needs instances on input 0", "put copy_to_points or array upstream of it")]),
            }
            Ok(e)
        }
        "seat" => {
            let mut e = in0();
            if !ins.get(1).cloned().flatten().is_some_and(|s| s.has_surface) {
                return Err(vec![fd("seat needs the ground (polygons) on input 1", "connect the terrain the objects stand on")]);
            }
            match &mut e.inst {
                Some(i) => {
                    i.insert("seat_move".into(), Ty::Float);
                    i.insert("seat_soft".into(), Ty::Float);
                    i.insert("seat_float".into(), Ty::Float);
                }
                None => return Err(vec![fd("seat needs instances on input 0", "put copy_to_points or copy_variants upstream of it")]),
            }
            Ok(e)
        }
        "sdf_shape" => Ok(GeoEnv { volumes: ["sdf".to_string()].into(), ..Default::default() }),
        "sdf_from_mesh" => {
            let e = in0();
            if !e.has_surface {
                return Err(vec![fd("sdf_from_mesh needs a closed polygon mesh", "connect a box/sphere/tube/... (use `realize` first if the input is instances)")]);
            }
            Ok(GeoEnv { volumes: ["sdf".to_string()].into(), ..Default::default() })
        }
        "sdf_combine" => {
            let (a, b) = (in0(), ins.get(1).cloned().flatten().unwrap_or_default());
            for (slot, e) in [(0, &a), (1, &b)] {
                if !e.volumes.contains("sdf") {
                    return Err(vec![fd(format!("sdf_combine input {slot} has no 'sdf' volume"), "feed it from sdf_shape or sdf_from_mesh (not directly from a mesh)")]);
                }
            }
            Ok(GeoEnv { volumes: ["sdf".to_string()].into(), ..Default::default() })
        }
        "sdf_offset" | "sdf_smooth" | "sdf_displace" => {
            let e = in0();
            if !e.volumes.contains("sdf") {
                return Err(vec![fd(format!("{kind} needs an 'sdf' volume on its input"), "feed it from sdf_shape/sdf_from_mesh/sdf_combine")]);
            }
            Ok(e)
        }
        "sparse_remesh" | "sparse_boolean" => {
            let e = in0();
            if !e.has_surface {
                return Err(vec![fd(format!("{kind} needs closed polygon meshes"), "connect a closed mesh (box, sphere, sdf_to_mesh output...)")]);
            }
            Ok(GeoEnv::base_points(&[("N", Ty::Vec3)], true))
        }
        "sdf_to_mesh" => {
            let e = in0();
            if !e.volumes.contains("sdf") {
                return Err(vec![fd("sdf_to_mesh needs an 'sdf' volume on its input", "feed it from sdf_shape/sdf_from_mesh/sdf_combine")]);
            }
            Ok(GeoEnv::base_points(&[("N", Ty::Vec3)], true))
        }
        "fuse" | "blast" | "facet" | "polyextrude" => {
            let mut e = in0();
            e.is_grid = false;
            Ok(e)
        }
        "box" | "sphere" | "tube" | "torus" | "circle" | "import_asset" => Ok(GeoEnv::base_points(&[], true)),
        "dem" => {
            let mut e = GeoEnv::base_points(&[("elev", Ty::Float)], true);
            e.is_grid = true; // a far ring with a hole is not one; the heightfield tools check at cook time
            Ok(e)
        }
        "building" => {
            let mut e = GeoEnv::base_points(&[], true);
            let attrs: &[&str] = match p.s("mode").as_str() {
                "openings" => &["opening", "kind", "wall", "room_from", "room_to", "width", "height", "sill"],
                "rooms" => &["room", "row", "col", "area", "height"],
                _ => &["room", "part", "opening"],
            };
            for a in attrs {
                e.prim.insert((*a).into(), Ty::Float);
            }
            Ok(e)
        }
        "line" => Ok(GeoEnv::base_points(&[], false)),
        "curve" | "helix" | "resample" => Ok(GeoEnv::base_points(&[], false)),
        "sweep" | "revolve" => Ok(GeoEnv::base_points(&[], true)),
        "drop_to_surface" => {
            let mut e = in0();
            if !ins.get(1).cloned().flatten().is_some_and(|s| s.has_surface) {
                return Err(vec![fd("drop_to_surface input 1 has no polygon surface", "connect a terrain/grid/mesh to input 1")]);
            }
            if p.i("set_normal") != 0 {
                e.point.insert("N".into(), Ty::Vec3);
            }
            if p.f("footprint") > 0.0 {
                e.point.insert("settle_drop".into(), Ty::Float);
            }
            Ok(e)
        }
        "switch" => Ok(ins.get(p.i("index").clamp(0, 3) as usize).cloned().flatten().unwrap_or_default()),
        "bound" => Ok(GeoEnv::base_points(&[], true)),
        "merge" => {
            let mut out = GeoEnv::default();
            for e in ins.iter().flatten() {
                for (src, dst) in [(&e.point, &mut out.point), (&e.prim, &mut out.prim)] {
                    for (k, t) in src {
                        if let Some(old) = dst.get(k) {
                            if old != t {
                                return Err(vec![fd(format!("attribute '{k}' is {} on one input and {} on another", old.name(), t.name()), "rename or convert one of them before merging")]);
                            }
                        }
                        dst.insert(k.clone(), *t);
                    }
                }
                if let Some(i) = &e.inst {
                    out.inst.get_or_insert_with(Default::default).extend(i.clone());
                }
                out.has_surface |= e.has_surface;
                out.has_points |= e.has_points;
                out.volumes.extend(e.volumes.iter().cloned());
            }
            Ok(out)
        }
        "scatter" | "scatter_poisson" | "scatter_stable" => {
            let e = in0();
            if !e.has_surface {
                return Err(vec![fd(format!("{kind} needs a polygon surface but its input has none"), "scatter onto a grid/box/sphere, or put `realize` before it if the input is instances")]);
            }
            if kind == "scatter_stable" {
                return Ok(GeoEnv::base_points(&[("N", Ty::Vec3), ("pscale", Ty::Float), ("u", Ty::Float), ("v", Ty::Float)], false));
            }
            Ok(GeoEnv::base_points(&[("N", Ty::Vec3), ("pscale", Ty::Float)], false))
        }
        "copy_to_points" => {
            let t = ins.get(1).cloned().flatten().unwrap_or_default();
            if !t.has_points {
                return Err(vec![fd("copy_to_points input 1 has no points to copy onto", "connect scatter, a point wrangle result, or any geometry with points to input 1")]);
            }
            if p.i("orient_to_normal") != 0 && t.point.get("N") != Some(&Ty::Vec3) {
                return Err(vec![fd("orient_to_normal=1 but input 1 has no vec3 point attribute N", "put `normals` (or scatter) before input 1, or set orient_to_normal=0")]);
            }
            let mut a = t.point.clone();
            a.remove("P");
            Ok(GeoEnv { inst: Some(a), ..Default::default() })
        }
        "copy_variants" => {
            let t = in0();
            if !t.has_points {
                return Err(vec![fd("copy_variants input 0 has no points to copy onto", "connect scatter or scatter_poisson (then filter with blast) to input 0")]);
            }
            if ins.iter().skip(1).all(Option::is_none) {
                return Err(vec![fd("copy_variants has no prototype", "connect at least one prototype (a tree, rock, plant) to input 1")]);
            }
            if p.i("orient_to_normal") != 0 && t.point.get("N") != Some(&Ty::Vec3) {
                return Err(vec![fd("orient_to_normal=1 but input 0 has no vec3 point attribute N", "put `normals` (or scatter) before input 0, or set orient_to_normal=0")]);
            }
            let mut a = t.point.clone();
            a.remove("P");
            Ok(GeoEnv { inst: Some(a), ..Default::default() })
        }
        "array" | "radial_array" => Ok(GeoEnv { inst: Some(Default::default()), ..Default::default() }),
        "realize" => {
            let mut e = in0();
            e.inst = None;
            Ok(e)
        }
        "normals" => {
            let mut e = in0();
            if !e.has_surface {
                return Err(vec![fd("normals needs polygons", "connect a surface")]);
            }
            e.point.insert("N".into(), Ty::Vec3);
            Ok(e)
        }
        "peak" | "smooth" => {
            let e = in0();
            if !e.has_surface {
                return Err(vec![fd(format!("{kind} needs polygons"), "connect a surface")]);
            }
            Ok(e)
        }
        "color_by_height" => {
            let mut e = in0();
            e.point.insert("Cd".into(), Ty::Vec3);
            Ok(e)
        }
        "measure" => {
            let mut e = in0();
            e.prim.insert("area".into(), Ty::Float);
            e.prim.insert("perimeter".into(), Ty::Float);
            Ok(e)
        }
        "triangulate" | "subdivide" => {
            let mut e = in0();
            e.prim.clear();
            e.is_grid = false;
            Ok(e)
        }
        "attrib_create" => {
            let mut e = in0();
            class_set(&mut e, &p.s("class")).map_err(|d| vec![d])?;
            let ty = match p.s("type").as_str() {
                "float" => Ty::Float,
                "vec3" => Ty::Vec3,
                o => return Err(vec![fd(format!("type must be float or vec3, got '{o}'"), "set type")]),
            };
            let name = p.s("name");
            if name.is_empty() {
                return Err(vec![fd("name is empty", "set the attribute name")]);
            }
            if p.s("class") == "prim" { e.prim.insert(name, ty); } else { e.point.insert(name, ty); }
            Ok(e)
        }
        "randomize" => {
            let mut e = in0();
            let ty = if p.s("type") == "vec3" { Ty::Vec3 } else { Ty::Float };
            if p.s("class") == "prim" { e.prim.insert(p.s("name"), ty); } else { e.point.insert(p.s("name"), ty); }
            Ok(e)
        }
        "attrib_delete" => {
            let mut e = in0();
            for n in p.s("names").split([',', ' ']).filter(|s| !s.is_empty()) {
                if n == "P" {
                    return Err(vec![fd("cannot delete P", "remove 'P' from names")]);
                }
                e.point.remove(n);
                e.prim.remove(n);
            }
            Ok(e)
        }
        "attrib_rename" => {
            let mut e = in0();
            let (from, to) = (p.s("from"), p.s("to"));
            let set = if p.s("class") == "prim" { &mut e.prim } else { &mut e.point };
            match set.remove(&from) {
                Some(t) => {
                    set.insert(to, t);
                    Ok(e)
                }
                None => Err(vec![fd(format!("attribute '{from}' does not exist (available: {})", fmt_names(set)), "check the name and class")]),
            }
        }
        "attrib_remap" => {
            let e = in0();
            match e.point.get(&p.s("name")) {
                Some(Ty::Float) => Ok(e),
                Some(_) => Err(vec![fd("attrib_remap works on float attributes", "pick a float attribute")]),
                None => Err(vec![fd(format!("point attribute '{}' does not exist (available: {})", p.s("name"), fmt_names(&e.point)), "create it upstream")]),
            }
        }
        "attribute_transfer" => {
            let mut e = in0();
            let src = ins.get(1).cloned().flatten().unwrap_or_default();
            let names: Vec<String> = {
                let n = p.s("attribs");
                if n.trim().is_empty() {
                    src.point.keys().filter(|k| *k != "P").cloned().collect()
                } else {
                    n.split([',', ' ']).filter(|s| !s.is_empty()).map(String::from).collect()
                }
            };
            for n in names {
                match src.point.get(&n) {
                    Some(t) => {
                        e.point.insert(n, *t);
                    }
                    None => return Err(vec![fd(format!("input 1 has no point attribute '{n}' (available: {})", fmt_names(&src.point)), "fix the attribute list")]),
                }
            }
            Ok(e)
        }
        "wrangle" => wrangle_effect(p, &in0(), ins.get(1).cloned().flatten().as_ref()),
        "group_by_expression" => {
            let e = in0();
            let code = format!("@__sel = ({});", p.s("expression"));
            let env = e.point.clone();
            match pt_vex::analyze(&code, &env) {
                Ok(_) => Ok(e),
                Err(ds) => Err(ds.iter().map(|d| fd(format!("expression: {}", d.msg), d.fix.clone())).collect()),
            }
        }
        // Everything else preserves the attribute environment of input 0.
        _ => Ok(in0()),
    }
}

fn wrangle_effect(p: &Params, e: &GeoEnv, refe: Option<&GeoEnv>) -> Result<GeoEnv, Vec<FlowDiag>> {
    use pt_core::ParamExt;
    let class = p.s("class");
    // the second input as the program sees it (see `RefSide` in attrs.rs): points or instance origins for near_*, base points for surf_*/ray_*
    let ref_envs: Option<(AttrTys, AttrTys)> = match refe {
        None => None,
        Some(r) => Some(match p.s("ref_class").as_str() {
            "instance" => match &r.inst {
                Some(i) => {
                    let mut near = i.clone();
                    near.insert("P".into(), Ty::Vec3);
                    (near, r.point.clone())
                }
                None => return Err(vec![fd("ref_class=instance but the second input has no instances", "put copy_to_points/array upstream of the second input, or use ref_class=point")]),
            },
            _ => (r.point.clone(), r.point.clone()),
        }),
    };
    let ref_env = ref_envs.as_ref().map(|(near, surf)| pt_vex::RefEnv { near, surf });
    let mut out = e.clone();
    let mut env = match class.as_str() {
        "point" => e.point.clone(),
        "instance" => match &e.inst {
            Some(i) => {
                let mut m = i.clone();
                m.insert("P".into(), Ty::Vec3);
                m
            }
            None => return Err(vec![fd("class=instance but the input has no instances", "put copy_to_points/array upstream, or use class=point")]),
        },
        o => return Err(vec![fd(format!("class must be 'point' or 'instance', got '{o}'"), "set class")]),
    };
    let prog = pt_vex::analyze_ref(&p.s("code"), &env, ref_env.as_ref()).map_err(|ds| ds.iter().map(|d| fd(format!("code {}:{}: {}", d.line, d.col, d.msg), d.fix.clone())).collect::<Vec<_>>())?;
    let chans = p.get("channels").and_then(Value::as_object);
    let missing: Vec<&str> = prog.chans.iter().map(|c| c.0.as_str()).filter(|n| chans.is_none_or(|m| !m.contains_key(*n))).collect();
    if !missing.is_empty() {
        return Err(vec![fd(format!("channel(s) {missing:?} used in code but not defined"), format!("add them to the `channels` parameter, e.g. {{\"{}\": 1.0}}", missing[0]))]);
    }
    for a in prog.attrs.iter().filter(|a| a.written) {
        env.insert(a.name.clone(), a.ty);
    }
    match class.as_str() {
        "point" => out.point = env,
        _ => {
            env.remove("P");
            out.inst = Some(env);
        }
    }
    Ok(out)
}
