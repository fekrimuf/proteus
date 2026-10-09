//! Attribute, group and wrangle operators.

use crate::flow::{col_ty, AttrTys, GeoEnv};
use crate::{ps, sch};
use pt_core::math;
use pt_core::rng::rand4;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use pt_vex::{AttrView, Inputs as VInputs, Program, Ty};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("wrangle", "Run a VEX-style snippet per point (class=point) or per instance (class=instance). Read/write attributes with @name; tunables via ch(\"name\") defined in `channels`. Example: @P.y += noise(@P * ch(\"f\")) * ch(\"a\");", 1, D::Piecewise, G::PerPoint, vec![
            ps("code", json!(""), "Snippet (see pt-vex): float/vec3 vars, @attrs, if/else, bounded for, functions."),
            ps("class", json!("point"), "point | instance."),
            ps("channels", json!({}), "Named numbers/arrays for ch()/chv(), e.g. {\"amp\": 0.5}."),
        ]),
        sch("attrib_create", "Create a constant attribute.", 1, D::Smooth, G::PerPoint, vec![
            ps("name", json!("attr"), "Attribute name."),
            ps("class", json!("point"), "point | prim."),
            ps("type", json!("float"), "float | vec3."),
            ps("value", json!([0.0, 0.0, 0.0]), "Value (first component for float)."),
        ]),
        sch("attrib_delete", "Delete attributes by name (space/comma separated). P cannot be deleted.", 1, D::Smooth, G::PerPoint, vec![ps("names", json!(""), "Names to delete.")]),
        sch("attrib_rename", "Rename an attribute.", 1, D::Smooth, G::PerPoint, vec![
            ps("from", json!(""), "Existing name."),
            ps("to", json!(""), "New name."),
            ps("class", json!("point"), "point | prim."),
        ]),
        sch("randomize", "Fill an attribute with deterministic random values (Philox, keyed by element index).", 1, D::None, G::PerPoint, vec![
            ps("name", json!("rand"), "Attribute name."),
            ps("class", json!("point"), "point | prim."),
            ps("type", json!("float"), "float | vec3."),
            ps("min", json!(0.0), "Minimum."),
            ps("max", json!(1.0), "Maximum."),
            ps("seed", json!(1), "Seed."),
        ]),
        sch("attrib_remap", "Remap a float point attribute from [in_min,in_max] to [out_min,out_max].", 1, D::Smooth, G::PerPoint, vec![
            ps("name", json!("pscale"), "Attribute."),
            ps("in_min", json!(0.0), "Input min."),
            ps("in_max", json!(1.0), "Input max."),
            ps("out_min", json!(0.0), "Output min."),
            ps("out_max", json!(1.0), "Output max."),
            ps("clamp", json!(1), "1 = clamp to the output range."),
        ]),
        sch("group_by_range", "Group points or prims by index range [start, end).", 1, D::None, G::PerPoint, vec![
            ps("name", json!("group1"), "Group name."),
            ps("class", json!("point"), "point | prim."),
            ps("start", json!(0), "First index."),
            ps("end", json!(100), "End index (exclusive)."),
            ps("invert", json!(0), "1 = invert."),
        ]),
        sch("group_by_expression", "Group points where a VEX expression is non-zero, e.g. `@P.y > 1.0 && @pscale < 0.5`.", 1, D::None, G::PerPoint, vec![
            ps("name", json!("group1"), "Group name."),
            ps("expression", json!("@P.y > 0.0"), "Expression evaluated per point."),
            ps("channels", json!({}), "Named numbers for ch()."),
        ]),
        sch("blast", "Delete a group of points or prims (or everything else with keep_selected=1).", 1, D::None, G::Whole, vec![
            ps("group", json!("group1"), "Group name."),
            ps("class", json!("point"), "point | prim."),
            ps("keep_selected", json!(0), "1 = delete everything not in the group."),
        ]),
        sch("measure", "Write prim attrs `area` and `perimeter`.", 1, D::Piecewise, G::PerPrim, vec![]),
        sch("attribute_transfer", "Copy point attributes from input 1 to the nearest point (deterministic ties) of input 0.", 2, D::None, G::PerPoint, vec![
            ps("attribs", json!(""), "Names to copy (empty = all float/vec3 except P)."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "wrangle" => inp(0).and_then(|g| wrangle(g, p)),
        "attrib_create" => inp(0).and_then(|g| attrib_create(g, p)),
        "attrib_delete" => inp(0).map(|g| attrib_delete(g, p)),
        "attrib_rename" => inp(0).and_then(|g| attrib_rename(g, p)),
        "randomize" => inp(0).and_then(|g| randomize(g, p)),
        "attrib_remap" => inp(0).and_then(|g| attrib_remap(g, p)),
        "group_by_range" => inp(0).and_then(|g| group_by_range(g, p)),
        "group_by_expression" => inp(0).and_then(|g| group_by_expression(g, p)),
        "blast" => inp(0).and_then(|g| blast(g, p)),
        "measure" => inp(0).map(measure),
        "attribute_transfer" => inp(0).and_then(|a| inp(1).and_then(|b| attribute_transfer(a, b, p))),
        _ => return None,
    })
}

// ---- wrangle ----

fn env_of(s: &pt_core::AttrSet) -> AttrTys {
    s.iter().filter(|(k, _)| !k.starts_with("__")).filter_map(|(k, c)| col_ty(c).map(|t| (k.clone(), t))).collect()
}

fn diags_to_string(prefix: &str, ds: &[pt_vex::Diag]) -> String {
    format!("{prefix}:\n  {}", ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n  "))
}

fn channel_values(prog: &Program, p: &Params) -> Result<Vec<[f32; 3]>, String> {
    let ch = p.get("channels").and_then(Value::as_object);
    prog.chans
        .iter()
        .map(|(name, ty)| {
            let v = ch.and_then(|m| m.get(name)).ok_or_else(|| format!("channel '{name}' is used in code but not defined; add it to `channels`, e.g. {{\"{name}\": 1.0}}"))?;
            match (ty, v) {
                (Ty::Float, Value::Number(n)) => Ok([n.as_f64().unwrap_or(0.0) as f32; 3]),
                (Ty::Vec3, Value::Array(a)) if a.len() == 3 => Ok([0, 1, 2].map(|i| a[i].as_f64().unwrap_or(0.0) as f32)),
                (Ty::Float, _) => Err(format!("channel '{name}' must be a number (used with ch())")),
                (Ty::Vec3, _) => Err(format!("channel '{name}' must be a 3-array (used with chv())")),
            }
        })
        .collect()
}

fn views<'a>(prog: &Program, set: &'a pt_core::AttrSet, p_override: Option<&'a [[f32; 3]]>) -> Vec<Option<AttrView<'a>>> {
    prog.attrs
        .iter()
        .map(|a| {
            if a.name == "P" {
                if let Some(o) = p_override {
                    return Some(AttrView::V(o));
                }
            }
            match set.get(&a.name) {
                Some(Column::F32(v)) => Some(AttrView::F(v.as_slice())),
                Some(Column::V3(v)) => Some(AttrView::V(v.as_slice())),
                _ => None,
            }
        })
        .collect()
}

fn column_for(ty: Ty, vals: &[[f32; 3]]) -> Column {
    match ty {
        Ty::Float => Column::from_f32(vals.iter().map(|v| v[0]).collect()),
        Ty::Vec3 => Column::from_v3(vals.to_vec()),
    }
}

fn wrangle(src: &Geometry, p: &Params) -> Res {
    let code = p.s("code");
    let frame = p.f("__frame");
    let frame = if frame == 0.0 { 1.0 } else { frame };
    match p.s("class").as_str() {
        "point" => {
            let prog = pt_vex::analyze(&code, &env_of(&src.point)).map_err(|d| diags_to_string("wrangle code", &d))?;
            let inp = VInputs { n: src.n_points(), attrs: views(&prog, &src.point, None), chans: channel_values(&prog, p)?, frame };
            let out = prog.run::<0>(&inp);
            let mut g = src.clone();
            for (ai, vals, _) in &out.written {
                let a = &prog.attrs[*ai];
                g.point.set(&a.name, column_for(a.ty, vals));
            }
            Ok(g)
        }
        "instance" => {
            let inst = src.instances.as_ref().ok_or("wrangle class=instance but input has no instances")?;
            let mut env = env_of(&inst.attrs);
            env.insert("P".into(), Ty::Vec3);
            let prog = pt_vex::analyze(&code, &env).map_err(|d| diags_to_string("wrangle code", &d))?;
            let ps: Vec<[f32; 3]> = inst.xf.iter().map(|m| [m[3], m[7], m[11]]).collect();
            let inp = VInputs { n: inst.len(), attrs: views(&prog, &inst.attrs, Some(&ps)), chans: channel_values(&prog, p)?, frame };
            let out = prog.run::<0>(&inp);
            let mut ni = inst.clone();
            for (ai, vals, _) in &out.written {
                let a = &prog.attrs[*ai];
                if a.name == "P" {
                    let xf = Arc::make_mut(&mut ni.xf);
                    for (m, v) in xf.iter_mut().zip(vals) {
                        m[3] = v[0];
                        m[7] = v[1];
                        m[11] = v[2];
                    }
                } else {
                    ni.attrs.set(&a.name, column_for(a.ty, vals));
                }
            }
            let mut g = src.clone();
            g.instances = Some(ni);
            Ok(g)
        }
        o => Err(format!("wrangle class must be 'point' or 'instance', got '{o}'")),
    }
}

// ---- simple attribute ops ----

fn class_is_prim(p: &Params) -> Result<bool, String> {
    match p.s("class").as_str() {
        "point" => Ok(false),
        "prim" => Ok(true),
        o => Err(format!("class must be 'point' or 'prim', got '{o}'")),
    }
}

fn attrib_create(src: &Geometry, p: &Params) -> Res {
    let prim = class_is_prim(p)?;
    let name = p.s("name");
    if name.is_empty() {
        return Err("attrib_create: name is empty".into());
    }
    let n = if prim { src.n_prims() } else { src.n_points() };
    let v = p.v3("value");
    let col = match p.s("type").as_str() {
        "float" => Column::from_f32(vec![v[0]; n]),
        "vec3" => Column::from_v3(vec![v; n]),
        o => return Err(format!("type must be float or vec3, got '{o}'")),
    };
    let mut g = src.clone();
    if prim { g.prim.set(&name, col) } else { g.point.set(&name, col) }
    Ok(g)
}

fn attrib_delete(src: &Geometry, p: &Params) -> Geometry {
    let mut g = src.clone();
    for n in p.s("names").split([',', ' ']).filter(|s| !s.is_empty() && *s != "P") {
        g.point.remove(n);
        g.prim.remove(n);
    }
    g
}

fn attrib_rename(src: &Geometry, p: &Params) -> Res {
    let prim = class_is_prim(p)?;
    let (from, to) = (p.s("from"), p.s("to"));
    let mut g = src.clone();
    let set = if prim { &mut g.prim } else { &mut g.point };
    let c = set.remove(&from).ok_or_else(|| format!("attribute '{from}' does not exist"))?;
    set.set(&to, c);
    Ok(g)
}

fn randomize(src: &Geometry, p: &Params) -> Res {
    let prim = class_is_prim(p)?;
    let n = if prim { src.n_prims() } else { src.n_points() };
    let (lo, hi, seed) = (p.f("min"), p.f("max"), p.i("seed"));
    let r = |i: usize| rand4(seed, 2, i as u64);
    let col = match p.s("type").as_str() {
        "float" => Column::from_f32((0..n).map(|i| lo + (hi - lo) * r(i)[0]).collect()),
        "vec3" => Column::from_v3((0..n).map(|i| {
            let x = r(i);
            [lo + (hi - lo) * x[0], lo + (hi - lo) * x[1], lo + (hi - lo) * x[2]]
        }).collect()),
        o => return Err(format!("type must be float or vec3, got '{o}'")),
    };
    let mut g = src.clone();
    if prim { g.prim.set(&p.s("name"), col) } else { g.point.set(&p.s("name"), col) }
    Ok(g)
}

fn attrib_remap(src: &Geometry, p: &Params) -> Res {
    let name = p.s("name");
    let Some(Column::F32(v)) = src.point.get(&name) else {
        return Err(format!("point float attribute '{name}' does not exist"));
    };
    let (i0, i1, o0, o1) = (p.f("in_min"), p.f("in_max"), p.f("out_min"), p.f("out_max"));
    let clamp = p.i("clamp") != 0;
    let span = if i1 != i0 { i1 - i0 } else { 1.0 };
    let out: Vec<f32> = v.iter().map(|&x| {
        let t = (x - i0) / span;
        let t = if clamp { t.clamp(0.0, 1.0) } else { t };
        o0 + (o1 - o0) * t
    }).collect();
    let mut g = src.clone();
    g.point.set(&name, Column::from_f32(out));
    Ok(g)
}

fn group_by_range(src: &Geometry, p: &Params) -> Res {
    let prim = class_is_prim(p)?;
    let n = if prim { src.n_prims() } else { src.n_points() };
    let (s, e) = (p.i("start").max(0) as usize, p.i("end").max(0) as usize);
    let inv = p.i("invert") != 0;
    let bits: Vec<bool> = (0..n).map(|i| (i >= s && i < e) != inv).collect();
    let mut g = src.clone();
    if prim { g.prim_groups.insert(p.s("name"), Arc::new(bits)); } else { g.point_groups.insert(p.s("name"), Arc::new(bits)); }
    Ok(g)
}

fn group_by_expression(src: &Geometry, p: &Params) -> Res {
    let code = format!("@__sel = ({});", p.s("expression"));
    let prog = pt_vex::analyze(&code, &env_of(&src.point)).map_err(|d| diags_to_string("group expression", &d))?;
    let frame = if p.f("__frame") == 0.0 { 1.0 } else { p.f("__frame") };
    let inp = VInputs { n: src.n_points(), attrs: views(&prog, &src.point, None), chans: channel_values(&prog, p)?, frame };
    let out = prog.run::<0>(&inp);
    let bits: Vec<bool> = out.written.first().map(|w| w.1.iter().map(|v| v[0] != 0.0).collect()).unwrap_or_default();
    let mut g = src.clone();
    g.point_groups.insert(p.s("name"), Arc::new(bits));
    Ok(g)
}

fn blast(src: &Geometry, p: &Params) -> Res {
    let prim = class_is_prim(p)?;
    let name = p.s("group");
    let keep_sel = p.i("keep_selected") != 0;
    let (np, nr) = (src.n_points(), src.n_prims());
    if prim {
        let bits = src.prim_groups.get(&name).ok_or_else(|| format!("prim group '{name}' does not exist (available: {:?})", src.prim_groups.keys().collect::<Vec<_>>()))?;
        let keep: Vec<bool> = bits.iter().map(|&b| b == keep_sel).collect();
        Ok(src.filter(&vec![true; np], &keep))
    } else {
        let bits = src.point_groups.get(&name).ok_or_else(|| format!("point group '{name}' does not exist (available: {:?})", src.point_groups.keys().collect::<Vec<_>>()))?;
        let keep: Vec<bool> = bits.iter().map(|&b| b == keep_sel).collect();
        Ok(src.filter(&keep, &vec![true; nr]))
    }
}

fn measure(src: &Geometry) -> Geometry {
    let sp = src.p();
    let (mut area, mut perim) = (Vec::new(), Vec::new());
    for i in 0..src.n_prims() {
        let pr = src.prim_points(i);
        let p0 = sp[pr[0] as usize];
        let mut cr = [0.0f32; 3];
        for k in 1..pr.len().saturating_sub(1) {
            let c = math::cross(math::sub(sp[pr[k] as usize], p0), math::sub(sp[pr[k + 1] as usize], p0));
            for j in 0..3 {
                cr[j] += c[j];
            }
        }
        area.push(0.5 * (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt());
        let mut l = 0.0f32;
        for k in 0..pr.len() {
            let d = math::sub(sp[pr[(k + 1) % pr.len()] as usize], sp[pr[k] as usize]);
            l += (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        }
        perim.push(if pr.len() == 2 { l * 0.5 } else { l });
    }
    let mut g = src.clone();
    g.prim.set("area", Column::from_f32(area));
    g.prim.set("perimeter", Column::from_f32(perim));
    g
}

// ---- attribute transfer (nearest point via uniform hash grid) ----

fn attribute_transfer(dst: &Geometry, src: &Geometry, p: &Params) -> Res {
    let sp = src.p();
    if sp.is_empty() {
        return Err("attribute_transfer: input 1 has no points".into());
    }
    let names: Vec<String> = {
        let n = p.s("attribs");
        if n.trim().is_empty() {
            src.point.iter().filter(|(k, c)| *k != "P" && !k.starts_with("__") && col_ty(c).is_some()).map(|(k, _)| k.clone()).collect()
        } else {
            n.split([',', ' ']).filter(|s| !s.is_empty()).map(String::from).collect()
        }
    };
    for n in &names {
        if src.point.get(n).is_none() {
            return Err(format!("input 1 has no point attribute '{n}'"));
        }
    }
    let (lo, hi) = src.base_bounds().unwrap();
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let vol = (ext[0].max(1e-3)) * (ext[1].max(1e-3)) * (ext[2].max(1e-3));
    let cell = ((vol / sp.len() as f32).powf_det(1.0 / 3.0) * 1.5).max(1e-4);
    let key = |q: [f32; 3]| [((q[0] - lo[0]) / cell).floor() as i32, ((q[1] - lo[1]) / cell).floor() as i32, ((q[2] - lo[2]) / cell).floor() as i32];
    let mut grid: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
    for (i, q) in sp.iter().enumerate() {
        grid.entry(key(*q)).or_default().push(i as u32);
    }
    let maxr = (ext[0].max(ext[1]).max(ext[2]) / cell).ceil() as i32 + 2;
    let nearest = |q: [f32; 3]| -> u32 {
        let k = key(q);
        let (mut best, mut bd) = (0u32, f32::INFINITY);
        for r in 0..=maxr {
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                            continue;
                        }
                        if let Some(list) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                            for &i in list {
                                let d = math::sub(sp[i as usize], q);
                                let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
                                if dd < bd || (dd == bd && i < best) {
                                    bd = dd;
                                    best = i;
                                }
                            }
                        }
                    }
                }
            }
            // Everything in ring r+1 is at least r*cell away.
            if bd.is_finite() && bd.sqrt() <= r as f32 * cell {
                break;
            }
        }
        best
    };
    let idx: Vec<u32> = dst.p().iter().map(|q| nearest(*q)).collect();
    let mut g = dst.clone();
    for n in names {
        g.point.set(&n, src.point.get(&n).unwrap().gather(&idx));
    }
    Ok(g)
}

/// Deterministic x^y (no std powf): used only for non-hashed sizing heuristics.
trait PowfDet {
    fn powf_det(self, y: f32) -> f32;
}
impl PowfDet for f32 {
    fn powf_det(self, y: f32) -> f32 {
        math::pow(self, y)
    }
}

#[allow(dead_code)]
fn _env(_: &GeoEnv) {}
