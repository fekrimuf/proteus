//! `pt pick`: what is at this pixel of a frame, and what made it?
//!
//! The owner points at a frame ("that moss floats") and the build answers in one query: the prototype and instance number under the
//! pixel, the node and rule that placed it (provenance), where it stands relative to the ground, and whether `ground-check` flags it.
//! It casts the camera's ray into the cooked world on the CPU (no render, no GPU): the base surfaces (terrain, water, buildings) and
//! every instance through a box tree over instance bounds and a triangle tree per prototype. A small radius turns the point into a
//! region: the classes of object under it, counted, so "that patch of moss" names its node and not one stone.

use crate::grounding::{cameras_of, check, mark, Camera, Finding, Surface};
use pt_core::math::{cross, normalize, sub, xform_point, Mat34};
use pt_core::spatial::{BoxTree, SurfHit, SurfaceTree};
use pt_core::{Column, Geometry};
use pt_graph::Graph;
use serde_json::{json, Value};
use std::cell::OnceCell;
use std::collections::BTreeMap;

type V3 = [f32; 3];

/// What a ray met first.
#[derive(Clone, Copy, Debug)]
pub enum Hit {
    /// Base geometry (terrain, water, a building): the primitive of the cooked geometry.
    Base { prim: u32, dist: f32, pos: V3, n: V3 },
    /// An instance: its index in the instance table, its prototype, and the primitive hit inside the prototype.
    Instance { k: u32, proto: u32, prim: u32, dist: f32, pos: V3, n: V3 },
}

impl Hit {
    pub fn dist(&self) -> f32 {
        match self {
            Hit::Base { dist, .. } | Hit::Instance { dist, .. } => *dist,
        }
    }
}

/// A cooked world indexed for ray casts.
pub struct World<'a> {
    geo: &'a Geometry,
    base: Option<SurfaceTree>,
    boxes: BoxTree,
    inst_of_box: Vec<u32>,
    protos: Vec<OnceCell<Option<SurfaceTree>>>,
}

fn inverse(m: &Mat34) -> Option<Mat34> {
    let (a, b, c, d, e, f, g, h, i) = (m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-20 {
        return None;
    }
    let r = [
        (e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det,
        (f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det,
        (d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det,
    ];
    let t = [m[3], m[7], m[11]];
    let tt = [
        -(r[0] * t[0] + r[1] * t[1] + r[2] * t[2]),
        -(r[3] * t[0] + r[4] * t[1] + r[5] * t[2]),
        -(r[6] * t[0] + r[7] * t[1] + r[8] * t[2]),
    ];
    Some([r[0], r[1], r[2], tt[0], r[3], r[4], r[5], tt[1], r[6], r[7], r[8], tt[2]])
}

fn linear(m: &Mat34, v: V3) -> V3 {
    [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[4] * v[0] + m[5] * v[1] + m[6] * v[2], m[8] * v[0] + m[9] * v[1] + m[10] * v[2]]
}

impl<'a> World<'a> {
    pub fn new(geo: &'a Geometry) -> World<'a> {
        let mut boxes = Vec::new();
        let mut inst_of_box = Vec::new();
        let mut protos = Vec::new();
        if let Some(inst) = &geo.instances {
            let bounds: Vec<Option<(V3, V3)>> = inst.protos.iter().map(|p| p.base_bounds()).collect();
            for (k, xf) in inst.xf.iter().enumerate() {
                let Some((lo, hi)) = bounds[inst.proto_id[k] as usize] else { continue };
                let (mut blo, mut bhi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
                for c in 0..8 {
                    let q = xform_point(xf, [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }]);
                    for a in 0..3 {
                        blo[a] = blo[a].min(q[a]);
                        bhi[a] = bhi[a].max(q[a]);
                    }
                }
                boxes.push((blo, bhi));
                inst_of_box.push(k as u32);
            }
            protos = inst.protos.iter().map(|_| OnceCell::new()).collect();
        }
        World { geo, base: SurfaceTree::new(geo), boxes: BoxTree::new(&boxes), inst_of_box, protos }
    }

    /// The ray's hit on instance `k`, as (distance along the ray, hit in the prototype).
    fn hit_instance(&self, k: u32, o: V3, d: V3) -> Option<(f32, SurfHit, u32)> {
        let inst = self.geo.instances.as_ref()?;
        let pid = inst.proto_id[k as usize];
        let tree = self.protos[pid as usize].get_or_init(|| SurfaceTree::new(&inst.protos[pid as usize])).as_ref()?;
        let inv = inverse(&inst.xf[k as usize])?;
        let (po, pd) = (xform_point(&inv, o), linear(&inv, d));
        let scale = (pd[0] * pd[0] + pd[1] * pd[1] + pd[2] * pd[2]).sqrt();
        let h = tree.ray(po, pd)?;
        Some((h.dist / scale, h, pid))
    }

    /// The first thing the ray from `o` along `d` meets.
    pub fn cast(&self, o: V3, d: V3) -> Option<Hit> {
        let d = normalize(d);
        let base = self.base.as_ref().and_then(|b| b.ray(o, d));
        let tmax = base.map_or(f32::INFINITY, |h| h.dist);
        let found = self.boxes.ray(o, d, tmax, &mut |bi, _| self.hit_instance(self.inst_of_box[bi as usize], o, d).map(|h| h.0));
        if let Some((bi, t)) = found {
            if t < tmax {
                let k = self.inst_of_box[bi as usize];
                let (_, h, pid) = self.hit_instance(k, o, d)?;
                let inst = self.geo.instances.as_ref()?;
                let mut n = normalize(linear(&inst.xf[k as usize], h.n));
                if n[0] * d[0] + n[1] * d[1] + n[2] * d[2] > 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                return Some(Hit::Instance { k, proto: pid, prim: h.prim, dist: t, pos: [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t], n });
            }
        }
        base.map(|h| Hit::Base { prim: h.prim, dist: h.dist, pos: h.pos, n: h.n })
    }
}

impl Camera {
    /// The ray through pixel (`x`, `y`) of a `w` x `h` frame: (origin, unit direction). The inverse of `project`.
    pub fn ray(&self, eye_y: f32, w: usize, h: usize, x: f32, y: f32) -> (V3, V3) {
        let (yaw, pitch) = (self.yaw_deg.to_radians(), self.pitch_deg.to_radians());
        let fwd = [yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos()];
        let right = [-yaw.sin(), 0.0, yaw.cos()];
        let up = cross(right, fwd);
        let f = self.lens / self.sensor * w as f32;
        let (u, v) = ((x - w as f32 / 2.0) / f, (h as f32 / 2.0 - y) / f);
        let dir = normalize([fwd[0] + right[0] * u + up[0] * v, fwd[1] + right[1] * u + up[1] * v, fwd[2] + right[2] * u + up[2] * v]);
        ([self.x, eye_y, self.z], dir)
    }
}

fn r3(v: V3) -> Value {
    json!([(v[0] * 1000.0).round() / 1000.0, (v[1] * 1000.0).round() / 1000.0, (v[2] * 1000.0).round() / 1000.0])
}

/// A prototype in words: its material names, its triangle count, and whether it is a far stand-in.
fn proto_label(geo: &Geometry, p: u32) -> (String, Value) {
    let Some(inst) = &geo.instances else { return (String::new(), Value::Null) };
    let g = &inst.protos[p as usize];
    let name = g.material_names().first().cloned().unwrap_or_else(|| format!("prototype {p}"));
    (name.clone(), json!({"id": p, "name": name, "materials": g.material_names(), "triangles": g.triangles().len(), "far_stand_in": pt_core::lod::hinted_cull(g).is_some()}))
}

/// Everything known about one pixel: the hit, who made it, where it stands.
struct Described {
    value: Value,
    /// Grouping key for regions: the creating node and the prototype.
    class: String,
    flagged: Vec<&'static str>,
}

struct Ctx<'a> {
    geo: &'a Geometry,
    graph: &'a mut Graph,
    node: &'a str,
    terrain: &'a Surface,
    findings: BTreeMap<usize, Vec<Finding>>,
}

impl Ctx<'_> {
    fn describe(&mut self, hit: &Hit) -> Described {
        match *hit {
            Hit::Instance { k, proto, prim, dist, pos, n } => {
                let inst = self.geo.instances.as_ref().expect("instance hit");
                let xf = inst.xf[k as usize];
                let origin = [xf[3], xf[7], xf[11]];
                let (label, proto_json) = proto_label(self.geo, proto);
                let creator = self.graph.explain(self.node, "instance", k as usize).unwrap_or_else(|e| json!({"error": e}));
                let proto_tag = inst.protos[proto as usize].prim.get("__src").and_then(|c| if let Column::I32(v) = c { v.first().copied() } else { None });
                let proto_made_by = proto_tag.and_then(|t| self.graph.creator_json(t, None)).map(|c| c["created_by"].clone());
                let mut attrs = serde_json::Map::new();
                for (name, col) in inst.attrs.iter().filter(|(n, _)| !n.starts_with("__")) {
                    match col {
                        Column::F32(v) if (k as usize) < v.len() => {
                            attrs.insert(name.clone(), json!((v[k as usize] * 1000.0).round() / 1000.0));
                        }
                        Column::V3(v) if (k as usize) < v.len() => {
                            attrs.insert(name.clone(), r3(v[k as usize]));
                        }
                        Column::I32(v) if (k as usize) < v.len() => {
                            attrs.insert(name.clone(), json!(v[k as usize]));
                        }
                        _ => {}
                    }
                }
                let ground = self.terrain.height(origin[0], origin[2]);
                let fs = self.findings.get(&(k as usize)).cloned().unwrap_or_default();
                let flagged: Vec<&'static str> = fs.iter().map(|f| f.kind).collect();
                let creator_name = creator["created_by"].as_str().unwrap_or("?").to_string();
                let value = json!({
                    "kind": "instance",
                    "instance": k,
                    "prototype": proto_json,
                    "prototype_made_by": proto_made_by,
                    "created_by": creator,
                    "hit": {"distance_m": (dist * 100.0).round() / 100.0, "position": r3(pos), "normal": r3(n), "primitive_in_prototype": prim},
                    "origin": r3(origin),
                    "origin_above_ground_m": ground.map(|g| ((origin[1] - g) * 1000.0).round() / 1000.0),
                    "scale": [(linear(&xf, [1.0, 0.0, 0.0]).iter().map(|c| c * c).sum::<f32>().sqrt() * 1000.0).round() / 1000.0],
                    "attributes": attrs,
                    "ground_check": fs.iter().map(|f| json!({"kind": f.kind, "amount_m": (f.amount * 100.0).round() / 100.0, "object_height_m": (f.height * 100.0).round() / 100.0})).collect::<Vec<_>>(),
                });
                Described { value, class: format!("{creator_name} / {label}"), flagged }
            }
            Hit::Base { prim, dist, pos, n } => {
                let mat = self.geo.prim_materials().get(prim as usize).copied().flatten().unwrap_or("(none)").to_string();
                let creator = self.graph.explain(self.node, "primitive", prim as usize).unwrap_or_else(|e| json!({"error": e}));
                let ground = self.terrain.height(pos[0], pos[2]);
                let value = json!({
                    "kind": "base surface",
                    "material": mat,
                    "primitive": prim,
                    "created_by": creator,
                    "hit": {"distance_m": (dist * 100.0).round() / 100.0, "position": r3(pos), "normal": r3(n), "slope_deg": (n[1].clamp(-1.0, 1.0).acos().to_degrees() * 10.0).round() / 10.0},
                    "terrain_height_there_m": ground.map(|g| (g * 1000.0).round() / 1000.0),
                });
                let by = value["created_by"]["created_by"].as_str().unwrap_or("?").to_string();
                Described { value, class: format!("{by} / {mat}"), flagged: vec![] }
            }
        }
    }
}

/// What the options of one pick say.
pub struct PickOpts<'a> {
    pub camera: &'a str,
    pub x: f32,
    pub y: f32,
    /// Frame size in pixels the coordinates refer to.
    pub size: (usize, usize),
    /// 0 = the one pixel; more = also the classes of object within this many pixels.
    pub radius_px: f32,
}

/// Casts the ray through a pixel (and a disc of pixels) into the cooked output of `node`.
pub fn pick(graph: &mut Graph, node: &str, scene: &Value, o: &PickOpts) -> Result<Value, String> {
    let geo = graph.cook(node)?;
    let cams = cameras_of(scene);
    let cam = cams.iter().find(|c| c.name == o.camera).ok_or_else(|| format!("no standing camera '{}' in the scene (have: {:?})", o.camera, cams.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()))?;
    let terrain = Surface::from_geometry(&geo, |m| m.starts_with("terrain"));
    let water = Surface::from_geometry(&geo, |m| m.contains("water"));
    let mut findings: BTreeMap<usize, Vec<Finding>> = BTreeMap::new();
    if !terrain.is_empty() {
        for f in check(&geo, &terrain, &water).0 {
            findings.entry(f.index).or_default().push(f);
        }
    }
    let eye_y = terrain.height(cam.x, cam.z).unwrap_or(0.0) + cam.eye_height;
    let world = World::new(&geo);
    let (w, h) = o.size;
    let mut ctx = Ctx { geo: &geo, graph, node, terrain: &terrain, findings };
    let cast = |x: f32, y: f32| {
        let (origin, dir) = cam.ray(eye_y, w, h, x, y);
        world.cast(origin, dir)
    };
    let centre = cast(o.x, o.y);
    let pixel = match &centre {
        Some(hit) => ctx.describe(hit).value,
        None => json!({"kind": "sky (nothing hit)"}),
    };
    let mut out = json!({"camera": o.camera, "frame": [w, h], "pixel": [o.x, o.y], "eye": r3([cam.x, eye_y, cam.z]), "result": pixel});
    if o.radius_px > 0.0 {
        // a grid of rays over the disc: the classes of object in the region, counted
        let step = (o.radius_px / 7.0).max(1.0);
        let n = (o.radius_px / step).floor() as i32;
        let mut classes: BTreeMap<String, (usize, f32, Value, Vec<&'static str>)> = BTreeMap::new();
        let mut rays = 0usize;
        for iy in -n..=n {
            for ix in -n..=n {
                let (dx, dy) = (ix as f32 * step, iy as f32 * step);
                let r = (dx * dx + dy * dy).sqrt();
                if r > o.radius_px {
                    continue;
                }
                let (x, y) = (o.x + dx, o.y + dy);
                if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                    continue;
                }
                rays += 1;
                let Some(hit) = cast(x, y) else {
                    classes.entry("sky".into()).or_insert((0, f32::MAX, Value::Null, vec![])).0 += 1;
                    continue;
                };
                let d = ctx.describe(&hit);
                let e = classes.entry(d.class.clone()).or_insert((0, f32::MAX, Value::Null, vec![]));
                e.0 += 1;
                if r < e.1 {
                    e.1 = r;
                    e.2 = json!({"pixel": [x, y], "instance": d.value.get("instance"), "origin": d.value.get("origin"), "origin_above_ground_m": d.value.get("origin_above_ground_m"), "ground_check": d.value.get("ground_check")});
                    e.3 = d.flagged;
                }
            }
        }
        let mut rows: Vec<_> = classes.into_iter().collect();
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        out["region"] = json!({
            "radius_px": o.radius_px,
            "rays": rays,
            "classes": rows.into_iter().map(|(k, (n, _, ex, fl))| json!({"class": k, "rays": n, "share": ((n as f32 / rays.max(1) as f32) * 100.0).round() / 100.0, "nearest_to_centre": ex, "flagged": fl})).collect::<Vec<_>>(),
        });
    }
    out["summary"] = json!(summary(&out));
    Ok(out)
}

/// One plain sentence for the result.
fn summary(v: &Value) -> String {
    let r = &v["result"];
    let at = format!("pixel ({}, {}) of {}", v["pixel"][0], v["pixel"][1], v["camera"].as_str().unwrap_or("?"));
    match r["kind"].as_str() {
        Some("instance") => {
            let flags: Vec<String> = r["ground_check"].as_array().map(|a| a.iter().map(|f| format!("{} {} m", f["kind"].as_str().unwrap_or("?"), f["amount_m"])).collect()).unwrap_or_default();
            let pid = r["attributes"]["pid"].as_i64().map_or(String::new(), |p| format!(", pid {p}"));
            format!(
                "{at}: {} (instance {}{pid}), placed by node `{}` ({}), {} m from the camera, origin {} m above the ground{}",
                r["prototype"]["name"].as_str().unwrap_or("?"),
                r["instance"],
                r["created_by"]["created_by"].as_str().unwrap_or("?"),
                r["created_by"]["kind"].as_str().unwrap_or("?"),
                r["hit"]["distance_m"],
                r["origin_above_ground_m"],
                if flags.is_empty() { "; ground-check flags nothing".to_string() } else { format!("; ground-check flags: {}", flags.join(", ")) }
            )
        }
        Some("base surface") => format!(
            "{at}: base surface `{}` made by node `{}`, {} m from the camera, slope {} degrees",
            r["material"].as_str().unwrap_or("?"),
            r["created_by"]["created_by"].as_str().unwrap_or("?"),
            r["hit"]["distance_m"],
            r["hit"]["slope_deg"]
        ),
        _ => format!("{at}: nothing hit (sky)"),
    }
}

fn frame_size(scene: &Value, frame: Option<&str>, size: Option<&str>) -> Result<(usize, usize), String> {
    if let Some(f) = frame {
        let img = pt_render::decode_png(&std::fs::read(f).map_err(|e| format!("{f}: {e}"))?)?;
        return Ok((img.w, img.h));
    }
    if let Some(s) = size {
        let (w, h) = s.split_once('x').ok_or("--size is WIDTHxHEIGHT, e.g. 1920x1080")?;
        return Ok((w.parse().map_err(|_| "--size width")?, h.parse().map_err(|_| "--size height")?));
    }
    let r = scene.get("render").and_then(|r| r.get("resolution")).and_then(Value::as_array);
    Ok(match r {
        Some(a) if a.len() == 2 => (a[0].as_u64().unwrap_or(1920) as usize, a[1].as_u64().unwrap_or(1080) as usize),
        _ => (1920, 1080),
    })
}

/// `pt pick <recipe.hcs> <scene.json> <camera> <x> <y> [--also X,Y]... [--radius PX] [--frame FRAME.png] [--size WxH] [--annotate OUT.png]`
pub fn pick_cmd(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: pt pick <recipe.hcs> <scene.json> <camera> <x> <y> [--also X,Y]... [--radius PX] [--frame FRAME.png] [--size WxH] [--annotate OUT.png]";
    let [recipe, scene_path, camera, x, y, rest @ ..] = args else { return Err(USAGE.into()) };
    let flag = |name: &str| rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned();
    let (x, y): (f32, f32) = (x.parse().map_err(|_| USAGE.to_string())?, y.parse().map_err(|_| USAGE.to_string())?);
    // more pixels of the same frame (`--also X,Y`, repeatable): the world is cooked once for all of them
    let mut points = vec![(x, y)];
    for (i, a) in rest.iter().enumerate() {
        if a == "--also" {
            let (px, py) = rest.get(i + 1).and_then(|s| s.split_once(',')).ok_or("--also takes X,Y")?;
            points.push((px.parse().map_err(|_| "--also X,Y")?, py.parse().map_err(|_| "--also X,Y")?));
        }
    }
    let radius: f32 = flag("--radius").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let code = std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?;
    let scene: Value = serde_json::from_str(std::fs::read_to_string(scene_path).map_err(|e| format!("{scene_path}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{scene_path}: {e}"))?;
    let size = frame_size(&scene, flag("--frame").as_deref(), flag("--size").as_deref())?;
    let mut g = Graph::new();
    g.provenance = true;
    g.knobs = crate::blender::knobs_of(&scene)?;
    g.lookdev = rest.iter().any(|a| a == "--lookdev"); // pick in the same tier the frame was rendered in
    let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let node = rep.outputs.first().ok_or("recipe has no out()")?.clone();
    let mut all = Vec::new();
    // `camera` may be a comma-separated list: the same pixels asked of several cameras in one cook (to compare candidate viewpoints)
    for cam in camera.split(',') {
        for &(px, py) in &points {
            let r = pick(&mut g, &node, &scene, &PickOpts { camera: cam, x: px, y: py, size, radius_px: radius })?;
            println!("{}", r["summary"].as_str().unwrap_or(""));
            all.push(r);
        }
    }
    let shown = if all.len() == 1 { all[0].clone() } else { Value::Array(all) };
    println!("{}", serde_json::to_string_pretty(&shown).map_err(|e| e.to_string())?);
    if let (Some(frame), Some(out)) = (flag("--frame"), flag("--annotate")) {
        let mut img = pt_render::decode_png(&std::fs::read(&frame).map_err(|e| format!("{frame}: {e}"))?)?;
        for &(px, py) in &points {
            mark(&mut img, px, py, 10.0, [0, 255, 255]);
            if radius > 0.0 {
                mark(&mut img, px, py, radius, [255, 255, 255]);
            }
        }
        std::fs::write(&out, pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
        eprintln!("wrote {out}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pt_core::Instances;
    use std::sync::Arc;

    fn camera() -> Camera {
        Camera { name: "c".into(), x: 0.0, z: 0.0, yaw_deg: 90.0, half_fov_deg: 32.0, pitch_deg: 0.0, eye_height: 1.7, lens: 28.0, sensor: 36.0 }
    }

    /// A flat ground at y = 0 and two boxes standing on it, 10 m and 20 m in front of a camera that looks along +z.
    fn world() -> Geometry {
        let ground = Geometry::from_polys(vec![[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]], &[vec![0, 1, 2, 3]]);
        let b = [[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [1.0, 2.0, -1.0], [-1.0, 2.0, -1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 2.0, 1.0], [-1.0, 2.0, 1.0]];
        let quads = vec![vec![0, 1, 2, 3], vec![5, 4, 7, 6], vec![4, 0, 3, 7], vec![1, 5, 6, 2], vec![3, 2, 6, 7], vec![4, 5, 1, 0]];
        let proto = Geometry::from_polys(b.to_vec(), &quads);
        let mut g = ground;
        g.instances = Some(Instances {
            protos: vec![Arc::new(proto)],
            proto_id: Arc::new(vec![0, 0]),
            xf: Arc::new(vec![[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 10.0], [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 20.0]]),
            attrs: Default::default(),
        });
        g
    }

    #[test]
    fn rays_through_pixels_are_the_inverse_of_projection() {
        let c = camera();
        for (x, y) in [(960.0f32, 540.0f32), (100.0, 80.0), (1800.0, 1000.0), (400.0, 700.0)] {
            let (o, d) = c.ray(1.7, 1920, 1080, x, y);
            let p = [o[0] + d[0] * 25.0, o[1] + d[1] * 25.0, o[2] + d[2] * 25.0];
            let back = c.project(1.7, p, 1920, 1080).unwrap();
            assert!((back[0] - x).abs() < 0.05 && (back[1] - y).abs() < 0.05, "({x}, {y}) came back as {back:?}");
        }
    }

    #[test]
    fn the_first_thing_along_a_ray_is_found_and_named() {
        let g = world();
        let w = World::new(&g);
        let c = camera();
        // the centre pixel looks along +z at eye height 1.7: it meets the front face of the box 10 m away (z = 9)
        let (o, d) = c.ray(1.7, 1920, 1080, 960.0, 540.0);
        let Some(Hit::Instance { k, dist, n, .. }) = w.cast(o, d) else { panic!("expected an instance") };
        assert_eq!(k, 0);
        assert!((dist - 9.0).abs() < 1e-3, "{dist}");
        assert!(n[2] < -0.9, "the normal faces the camera: {n:?}");
        // a ray aimed lower meets the ground in front of the first box
        let (o, d) = c.ray(1.7, 1920, 1080, 960.0, 900.0);
        let Some(Hit::Base { pos, .. }) = w.cast(o, d) else { panic!("expected the ground") };
        assert!(pos[1].abs() < 1e-3 && pos[2] < 9.0);
        // a ray above the boxes meets nothing but the sky
        let (o, d) = c.ray(1.7, 1920, 1080, 960.0, 100.0);
        assert!(w.cast(o, d).is_none());
        // a ray off to the side of both boxes goes on to the ground far ahead, not the boxes
        let (o, d) = c.ray(1.7, 1920, 1080, 200.0, 700.0);
        assert!(matches!(w.cast(o, d), Some(Hit::Base { .. })));
    }

    #[test]
    fn a_pick_names_the_node_that_placed_the_object() {
        let code = "ground = set_material(grid(size=[100, 100], rows=4, cols=4), name=\"terrain_x\")\n\
                    spot = curve(points=[[0.0, 0.0, 10.0], [0.0, 0.0, 30.0]])\n\
                    rock = box(size=[2.0, 2.0, 2.0], center=[0, 1.0, 0])\n\
                    rocks = copy_to_points(rock, spot)\n\
                    world = merge(ground, rocks)\n\
                    out(world)\n";
        let mut g = Graph::new();
        g.provenance = true;
        let rep = pt_hcs::run(code, &mut g, true).unwrap_or_else(|d| panic!("{}", d[0]));
        let scene = json!({"cameras": [{"name": "c", "eye": {"at": [0.0, 0.0], "height": 1.7, "yaw": 90.0, "pitch": 0.0}, "lens": 28}]});
        let o = PickOpts { camera: "c", x: 960.0, y: 540.0, size: (1920, 1080), radius_px: 30.0 };
        let r = pick(&mut g, &rep.outputs[0], &scene, &o).unwrap();
        assert_eq!(r["result"]["kind"], "instance");
        assert_eq!(r["result"]["instance"], 0);
        let by = r["result"]["created_by"]["created_by"].as_str().unwrap();
        assert!(by == "spot" || by == "rocks", "created by {by}");
        assert!((r["result"]["hit"]["distance_m"].as_f64().unwrap() - 9.0).abs() < 0.05);
        assert!(r["result"]["origin_above_ground_m"].as_f64().unwrap().abs() < 0.01);
        assert!(r["summary"].as_str().unwrap().contains("instance 0"), "{}", r["summary"]);
        let region = r["region"]["classes"].as_array().unwrap();
        assert!(region.iter().any(|c| c["class"].as_str().unwrap().contains(by)), "{region:?}");
        // the ground below the rock, and the sky above it, are named too
        let low = pick(&mut g, &rep.outputs[0], &scene, &PickOpts { y: 900.0, radius_px: 0.0, ..o }).unwrap();
        assert_eq!(low["result"]["material"], "terrain_x");
        let sky = pick(&mut g, &rep.outputs[0], &scene, &PickOpts { y: 60.0, radius_px: 0.0, ..o }).unwrap();
        assert!(sky["summary"].as_str().unwrap().contains("sky"));
        assert!(pick(&mut g, &rep.outputs[0], &scene, &PickOpts { camera: "nope", ..o }).unwrap_err().contains("no standing camera"));
    }

    #[test]
    fn the_nearer_instance_hides_the_farther_one() {
        let g = world();
        let w = World::new(&g);
        let c = camera();
        // pixels a little above the centre line still hit the near box first: at 9 m ahead these rays are 1.7, 1.82 and 1.94 m
        // high, under the box's 2 m top
        for y in [540.0f32, 520.0, 500.0] {
            let (o, d) = c.ray(1.7, 1920, 1080, 960.0, y);
            match w.cast(o, d) {
                Some(Hit::Instance { k, .. }) => assert_eq!(k, 0, "pixel row {y}"),
                other => panic!("row {y}: {other:?}"),
            }
        }
    }
}
