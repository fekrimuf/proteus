//! `pt ground-check`: does every placed object sit on the ground, and does it read as a solid thing?
//!
//! The eye finds these defects in a frame (a stone hovering over the bed, moss in the air, a rock that is mostly under water and
//! shows only a flat lid), so the build must find them first, from the geometry alone and on the CPU: no renderer, no GPU.
//! For every instance the check reads the cooked terrain and water surfaces under the object's footprint and reports
//!
//! * `float`: the lowest point of the object is above the ground under every part of its footprint (nothing touches);
//! * `overhang`: one side hangs in the air by more than a third of the object's height (reads as hovering on a slope);
//! * `shallow`: more than 60% of the object's height is under water, so it shows only a flat lid;
//! * `buried`: more than 60% of the object's height is under the ground;
//! * `card`: the prototype is flat (thickness under 12% of its widest side), a leaf card or a sheet that reads as 2D close up.
//!
//! It then lists, per scene camera, the flagged objects inside that camera's view cone within a radius, because those are the
//! ones an eye will see. A `float` or `overhang` inside a view fails the check.

use pt_core::math::xform_point;
use pt_core::Geometry;
use serde_json::{json, Value};
use std::collections::HashMap;

const CELL: f32 = 8.0;

/// Triangles of one surface (terrain or water) with a grid over x and z, so a height lookup tests a handful of triangles.
pub struct Surface {
    tris: Vec<[[f32; 3]; 3]>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

impl Surface {
    pub fn from_geometry(geo: &Geometry, keep: impl Fn(&str) -> bool) -> Surface {
        let mats = geo.prim_materials();
        let p = geo.p();
        let mut tris = Vec::new();
        for i in 0..geo.n_prims() {
            if !mats[i].is_some_and(&keep) {
                continue;
            }
            let ids = geo.prim_points(i);
            for k in 1..ids.len().saturating_sub(1) {
                tris.push([p[ids[0] as usize], p[ids[k] as usize], p[ids[k + 1] as usize]]);
            }
        }
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (t, tri) in tris.iter().enumerate() {
            let (x0, x1) = (tri[0][0].min(tri[1][0]).min(tri[2][0]), tri[0][0].max(tri[1][0]).max(tri[2][0]));
            let (z0, z1) = (tri[0][2].min(tri[1][2]).min(tri[2][2]), tri[0][2].max(tri[1][2]).max(tri[2][2]));
            for cx in (x0 / CELL).floor() as i32..=(x1 / CELL).floor() as i32 {
                for cz in (z0 / CELL).floor() as i32..=(z1 / CELL).floor() as i32 {
                    grid.entry((cx, cz)).or_default().push(t as u32);
                }
            }
        }
        Surface { tris, grid }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Height of the surface at (x, z). Where levels overlap, the finest (smallest) triangle wins.
    pub fn height(&self, x: f32, z: f32) -> Option<f32> {
        let cell = self.grid.get(&((x / CELL).floor() as i32, (z / CELL).floor() as i32))?;
        let mut best: Option<(f32, f32)> = None; // (area, y)
        for &t in cell {
            let [a, b, c] = self.tris[t as usize];
            let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if d.abs() < 1e-12 {
                continue;
            }
            let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
            let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
            let l3 = 1.0 - l1 - l2;
            let e = -1e-4;
            if l1 >= e && l2 >= e && l3 >= e {
                let y = l1 * a[1] + l2 * b[1] + l3 * c[1];
                let area = d.abs();
                if best.is_none_or(|(ba, _)| area < ba) {
                    best = Some((area, y));
                }
            }
        }
        best.map(|(_, y)| y)
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: &'static str,
    pub proto: u32,
    pub index: usize,
    pub pos: [f32; 3],
    pub height: f32,
    /// Metres: for `float` and `overhang` the air gap, for `shallow` and `buried` the depth under water or ground.
    pub amount: f32,
}

pub struct Camera {
    pub name: String,
    pub x: f32,
    pub z: f32,
    pub yaw_deg: f32,
    pub half_fov_deg: f32,
    pub pitch_deg: f32,
    pub eye_height: f32,
    pub lens: f32,
    pub sensor: f32,
}

impl Camera {
    /// Where a world point lands in a frame of `w` x `h` pixels, or `None` behind the camera or outside the frame.
    /// `eye_y` is the camera's world height. Convention of the scene file: forward = (cos yaw cos pitch, sin pitch, sin yaw cos pitch),
    /// right = (-sin yaw, 0, cos yaw), as in `render_scene.py`; the lens is horizontal-sensor based.
    pub fn project(&self, eye_y: f32, p: [f32; 3], w: usize, h: usize) -> Option<[f32; 2]> {
        let (yaw, pitch) = (self.yaw_deg.to_radians(), self.pitch_deg.to_radians());
        let fwd = [yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos()];
        let right = [-yaw.sin(), 0.0, yaw.cos()];
        let up = [right[1] * fwd[2] - right[2] * fwd[1], right[2] * fwd[0] - right[0] * fwd[2], right[0] * fwd[1] - right[1] * fwd[0]];
        let d = [p[0] - self.x, p[1] - eye_y, p[2] - self.z];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let depth = dot(d, fwd);
        if depth < 0.3 {
            return None;
        }
        let f = self.lens / self.sensor * w as f32;
        let (x, y) = (w as f32 / 2.0 + f * dot(d, right) / depth, h as f32 / 2.0 - f * dot(d, up) / depth);
        (x >= 0.0 && y >= 0.0 && x < w as f32 && y < h as f32).then_some([x, y])
    }
}

/// Cameras of a scene file that stand on the ground (`eye`); a 28 mm lens on a 36 mm sensor has a half field of view of 32.8 degrees.
pub fn cameras_of(scene: &Value) -> Vec<Camera> {
    let list: Vec<&Value> = match scene.get("cameras").and_then(Value::as_array) {
        Some(a) => a.iter().collect(),
        None => scene.get("camera").into_iter().collect(),
    };
    list.iter()
        .filter_map(|c| {
            let e = c.get("eye")?;
            let at = e.get("at")?.as_array()?;
            let lens = c.get("lens").and_then(Value::as_f64).unwrap_or(35.0);
            let sensor = c.get("sensor").and_then(Value::as_f64).unwrap_or(36.0);
            Some(Camera {
                name: c.get("name").and_then(Value::as_str).unwrap_or("camera").to_string(),
                x: at.first()?.as_f64()? as f32,
                z: at.get(1)?.as_f64()? as f32,
                yaw_deg: e.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                half_fov_deg: ((sensor / 2.0 / lens).atan().to_degrees()) as f32,
                pitch_deg: e.get("pitch").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                eye_height: e.get("height").and_then(Value::as_f64).unwrap_or(1.7) as f32,
                lens: lens as f32,
                sensor: sensor as f32,
            })
        })
        .collect()
}

/// Every flagged instance of the cooked world.
pub fn check(geo: &Geometry, terrain: &Surface, water: &Surface) -> (Vec<Finding>, Vec<(u32, &'static str)>) {
    let mut out = Vec::new();
    let mut flat_protos = Vec::new();
    let Some(inst) = &geo.instances else { return (out, flat_protos) };
    // per prototype: bounds, and whether it is a flat card
    let bounds: Vec<Option<([f32; 3], [f32; 3])>> = inst.protos.iter().map(|p| p.bounds()).collect();
    for (i, b) in bounds.iter().enumerate() {
        if let Some((lo, hi)) = b {
            let s = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            let widest = s.iter().cloned().fold(0.0f32, f32::max);
            let thinnest = s.iter().cloned().fold(f32::MAX, f32::min);
            if widest > 0.0 && thinnest / widest < 0.12 && pt_core::lod::hinted_cull(&inst.protos[i]).is_none() {
                flat_protos.push((i as u32, "card"));
            }
        }
    }
    for (k, xf) in inst.xf.iter().enumerate() {
        let pid = inst.proto_id[k];
        let Some((lo, hi)) = bounds[pid as usize] else { continue };
        if pt_core::lod::hinted_cull(&inst.protos[pid as usize]).is_some() {
            continue; // distant stand-ins are not seen up close
        }
        // world-space extent of the box: lowest and highest point, and the footprint ring at half the box's half-width
        let mut y_lo = f32::MAX;
        let mut y_hi = f32::MIN;
        for c in 0..8 {
            let q = xform_point(xf, [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }]);
            y_lo = y_lo.min(q[1]);
            y_hi = y_hi.max(q[1]);
        }
        let height = y_hi - y_lo;
        if height < 0.05 {
            continue;
        }
        let (cx, cz) = ((lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5);
        let (hx, hz) = ((hi[0] - lo[0]) * 0.25, (hi[2] - lo[2]) * 0.25);
        let mut samples = vec![xform_point(xf, [cx, lo[1], cz])];
        for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            samples.push(xform_point(xf, [cx + sx * hx, lo[1], cz + sz * hz]));
        }
        let origin = [xf[3], xf[7], xf[11]];
        let gaps: Vec<f32> = samples.iter().filter_map(|s| terrain.height(s[0], s[2]).map(|g| y_lo - g)).collect();
        if gaps.is_empty() {
            continue; // off the terrain (on a building roof, say)
        }
        let min_gap = gaps.iter().cloned().fold(f32::MAX, f32::min);
        let max_gap = gaps.iter().cloned().fold(f32::MIN, f32::max);
        let tol = (0.04 * height).max(0.08);
        let mut push = |kind: &'static str, amount: f32| out.push(Finding { kind, proto: pid, index: k, pos: origin, height, amount });
        if min_gap > tol {
            push("float", min_gap);
        } else if max_gap > height / 3.0 && max_gap > 0.2 {
            push("overhang", max_gap);
        }
        let ground_c = gaps.first().map_or(0.0, |g| y_lo - g);
        let buried = ground_c - y_lo;
        if buried > 0.6 * height {
            push("buried", buried);
        }
        if let Some(w) = water.height(origin[0], origin[2]) {
            let under = (w - y_lo).min(height);
            if under > 0.6 * height {
                push("shallow", under);
            }
        }
    }
    (out, flat_protos)
}

fn in_view(c: &Camera, pos: [f32; 3], radius: f32) -> bool {
    let (dx, dz) = (pos[0] - c.x, pos[2] - c.z);
    let d = (dx * dx + dz * dz).sqrt();
    if d > radius {
        return false;
    }
    if d < 0.5 {
        return true;
    }
    let yaw = c.yaw_deg.to_radians();
    let cos = (dx * yaw.cos() + dz * yaw.sin()) / d;
    cos >= (c.half_fov_deg + 8.0).to_radians().cos()
}

/// Paint one marker: a square outline of `half` pixels around (cx, cy).
pub(crate) fn mark(img: &mut pt_render::Image, cx: f32, cy: f32, half: f32, rgb: [u8; 3]) {
    let (x0, x1, y0, y1) = ((cx - half) as i64, (cx + half) as i64, (cy - half) as i64, (cy + half) as i64);
    let mut put = |x: i64, y: i64| {
        for t in 0..3i64 {
            for (px, py) in [(x, y + t), (x + t, y)] {
                if px >= 0 && py >= 0 && (px as usize) < img.w && (py as usize) < img.h {
                    let o = (py as usize * img.w + px as usize) * 3;
                    img.rgb[o..o + 3].copy_from_slice(&rgb);
                }
            }
        }
    };
    for x in x0..=x1 {
        put(x, y0);
        put(x, y1);
    }
    for y in y0..=y1 {
        put(x0, y);
        put(x1, y);
    }
}

fn kind_colour(kind: &str) -> [u8; 3] {
    match kind {
        "float" => [255, 0, 0],
        "overhang" => [255, 140, 0],
        "shallow" => [255, 255, 0],
        "buried" => [255, 0, 255],
        _ => [0, 255, 255],
    }
}

/// Report as JSON: totals per kind, per camera the flagged objects in view (with their pixel position in a 1920 x 1080 frame), and
/// the failing verdict. With `frames`, a copy of every camera's `frame_<camera>.png` with the flagged objects boxed is written
/// to `annotate_to` (red float, orange overhang, yellow shallow, magenta buried).
pub fn report(geo: &Geometry, scene: &Value, radius: f32, frames: Option<&std::path::Path>, annotate_to: Option<&std::path::Path>) -> Result<Value, String> {
    let terrain = Surface::from_geometry(geo, |m| m.starts_with("terrain"));
    if terrain.is_empty() {
        return Err("no terrain material (a name starting with 'terrain') in the cooked world".into());
    }
    let water = Surface::from_geometry(geo, |m| m.contains("water"));
    let (findings, flat) = check(geo, &terrain, &water);
    let inst = geo.instances.as_ref();
    let label = |p: u32| -> String {
        inst.and_then(|i| i.protos.get(p as usize)).map(|g| g.material_names().first().cloned().unwrap_or_else(|| format!("proto {p}"))).unwrap_or_default()
    };
    let mut by_kind: std::collections::BTreeMap<&str, usize> = Default::default();
    for f in &findings {
        *by_kind.entry(f.kind).or_default() += 1;
    }
    let cams = cameras_of(scene);
    let mut cam_json = vec![];
    let mut failing = false;
    for c in &cams {
        let eye_y = terrain.height(c.x, c.z).unwrap_or(0.0) + c.eye_height;
        let mut seen: Vec<&Finding> = findings.iter().filter(|f| in_view(c, f.pos, radius)).collect();
        seen.sort_by(|a, b| b.amount.partial_cmp(&a.amount).unwrap_or(std::cmp::Ordering::Equal));
        let bad = seen.iter().filter(|f| f.kind == "float" || f.kind == "overhang").count();
        failing |= bad > 0;
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
        let mut by_what: std::collections::BTreeMap<String, usize> = Default::default();
        for f in &seen {
            *counts.entry(f.kind).or_default() += 1;
            *by_what.entry(format!("{} {}", f.kind, label(f.proto))).or_default() += 1;
        }
        let (w, h) = (1920usize, 1080usize);
        let px = |f: &Finding| c.project(eye_y, [f.pos[0], f.pos[1] + f.height * 0.5, f.pos[2]], w, h);
        if let (Some(dir), Some(out)) = (frames, annotate_to) {
            let file = dir.join(format!("frame_{}.png", c.name));
            if let Ok(bytes) = std::fs::read(&file) {
                let mut img = pt_render::decode_png(&bytes)?;
                let fpx = c.lens / c.sensor * img.w as f32;
                for f in &seen {
                    let depth = ((f.pos[0] - c.x).powi(2) + (f.pos[2] - c.z).powi(2)).sqrt().max(0.5);
                    if let Some(p) = c.project(eye_y, [f.pos[0], f.pos[1] + f.height * 0.5, f.pos[2]], img.w, img.h) {
                        let half = (fpx * f.height / depth * 0.6).clamp(10.0, 70.0);
                        mark(&mut img, p[0], p[1], half, kind_colour(f.kind));
                    }
                }
                std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
                std::fs::write(out.join(format!("ground_{}.png", c.name)), pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
            }
        }
        cam_json.push(json!({
            "camera": c.name,
            "counts": counts,
            "by_prototype": by_what,
            "worst": seen.iter().take(8).map(|f| json!({"kind": f.kind, "what": label(f.proto), "at": [f.pos[0], f.pos[1], f.pos[2]], "height": f.height, "amount": f.amount, "pixel": px(f)})).collect::<Vec<_>>(),
        }));
    }
    // where the world's triangles are: per material, triangles, points used and the horizontal extent in metres
    let mut mats: std::collections::BTreeMap<String, (usize, f32, f32, f32, f32)> = Default::default();
    {
        let names = geo.prim_materials();
        let p = geo.p();
        for i in 0..geo.n_prims() {
            let ids = geo.prim_points(i);
            let e = mats.entry(names[i].unwrap_or("(none)").to_string()).or_insert((0, f32::MAX, f32::MAX, f32::MIN, f32::MIN));
            e.0 += ids.len().saturating_sub(2);
            for &v in ids {
                let q = p[v as usize];
                *e = (e.0, e.1.min(q[0]), e.2.min(q[2]), e.3.max(q[0]), e.4.max(q[2]));
            }
        }
    }
    Ok(json!({
        "base_points": geo.n_points(),
        "base_materials": mats.iter().map(|(k, v)| json!({"material": k, "triangles": v.0, "x": [v.1, v.3], "z": [v.2, v.4]})).collect::<Vec<_>>(),
        "instances": inst.map_or(0, |i| i.len()),
        "findings": by_kind,
        "flat_prototypes": flat.iter().map(|(p, k)| json!({"proto": label(*p), "kind": k})).collect::<Vec<_>>(),
        "cameras": cam_json,
        "radius_m": radius,
        "fail": failing,
    }))
}

/// `pt ground-check <recipe.hcs> <scene.json> [--radius M] [--frames DIR --annotate DIR]`
pub fn ground_check_cmd(args: &[String]) -> Result<(), String> {
    let [recipe, scene_path, rest @ ..] = args else { return Err("usage: pt ground-check <recipe.hcs> <scene.json> [--radius M] [--frames DIR --annotate DIR]".into()) };
    let flag = |name: &str| rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).cloned();
    let radius = flag("--radius").and_then(|s| s.parse().ok()).unwrap_or(45.0);
    let code = std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?;
    let scene: Value = serde_json::from_str(std::fs::read_to_string(scene_path).map_err(|e| format!("{scene_path}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{scene_path}: {e}"))?;
    let mut g = pt_graph::Graph::new();
    g.knobs = crate::blender::knobs_of(&scene)?;
    g.lookdev = rest.iter().any(|a| a == "--lookdev");
    let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let geo = g.cook(rep.outputs.first().ok_or("recipe has no out()")?)?;
    drop(g);
    let (frames, out) = (flag("--frames").map(std::path::PathBuf::from), flag("--annotate").map(std::path::PathBuf::from));
    let r = report(&geo, &scene, radius, frames.as_deref(), out.as_deref())?;
    println!("{}", serde_json::to_string_pretty(&r).map_err(|e| e.to_string())?);
    if r["fail"] == true {
        Err("ground-check: floating or overhanging objects inside a camera view".into())
    } else {
        Ok(())
    }
}

/// `pt review-tiles <frame.png> <out_dir>`: cut a frame into 3 x 2 tiles, each enlarged 2x, so a frame is reviewed at full
/// resolution tile by tile instead of in one downscaled glance (the way two defects were missed in `frame_east.png`).
pub fn review_tiles_cmd(args: &[String]) -> Result<(), String> {
    let [frame, out] = args else { return Err("usage: pt review-tiles <frame.png> <out_dir>".into()) };
    let img = pt_render::decode_png(&std::fs::read(frame).map_err(|e| format!("{frame}: {e}"))?)?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let (cols, rows, scale) = (3usize, 2usize, 2usize);
    let (tw, th) = (img.w / cols, img.h / rows);
    for r in 0..rows {
        for c in 0..cols {
            let mut t = pt_render::Image { w: tw * scale, h: th * scale, rgb: vec![0; tw * scale * th * scale * 3] };
            for y in 0..t.h {
                for x in 0..t.w {
                    let (sx, sy) = (c * tw + x / scale, r * th + y / scale);
                    let (o, s) = ((y * t.w + x) * 3, (sy * img.w + sx) * 3);
                    t.rgb[o..o + 3].copy_from_slice(&img.rgb[s..s + 3]);
                }
            }
            let name = format!("tile_r{}c{}.png", r + 1, c + 1);
            std::fs::write(std::path::Path::new(out).join(&name), pt_render::encode_png(&t)?).map_err(|e| e.to_string())?;
        }
    }
    println!("wrote {} tiles to {out}", cols * rows);
    Ok(())
}

/// How far a far stand-in can matter to a frame: the world's skyline is about this far.
const FAR_RANGE_M: f32 = 30000.0;

/// Terrain cells (not the far level, which is the skyline and the shadows of the peaks) that no chosen camera can see are dropped
/// from a look-development render: within `radius` metres of a camera, or inside the view cone at any distance, a cell stays.
/// Returns (cells kept, cells before) of the terrain that was considered.
fn crop_terrain(geo: &mut Geometry, cams: &[Camera], radius: f32) -> (usize, usize) {
    let mats: Vec<bool> = geo.prim_materials().iter().map(|m| m.is_some_and(|m| m.starts_with("terrain") && !m.contains("far"))).collect();
    let p = geo.p();
    let near = |q: [f32; 3]| cams.iter().any(|c| ((q[0] - c.x).powi(2) + (q[2] - c.z).powi(2)).sqrt() <= radius || in_view(c, q, FAR_RANGE_M));
    let keep: Vec<bool> = (0..geo.n_prims()).map(|i| !mats[i] || geo.prim_points(i).iter().any(|&q| near(p[q as usize]))).collect();
    let (considered, kept) = (mats.iter().filter(|m| **m).count(), (0..mats.len()).filter(|&i| mats[i] && keep[i]).count());
    if kept == considered {
        return (kept, considered);
    }
    // a point stays when a kept primitive uses it, or when no primitive uses it at all (loose points belong to something else)
    let mut used = vec![false; geo.n_points()];
    let mut used_by_kept = vec![false; geo.n_points()];
    for i in 0..geo.n_prims() {
        for &q in geo.prim_points(i) {
            used[q as usize] = true;
            used_by_kept[q as usize] |= keep[i];
        }
    }
    let point_keep: Vec<bool> = (0..geo.n_points()).map(|q| used_by_kept[q] || !used[q]).collect();
    *geo = geo.filter(&point_keep, &keep);
    (kept, considered)
}

/// Look development: drop every instance the chosen cameras cannot see, so a one-view render of a big world fits in the memory of
/// a desktop. Kept: everything within `radius` metres of a camera (it can cast shadows into the view), everything inside the view
/// cone out to four times that, and the far stand-ins (`lod_hint`, a few triangles each) inside the view cone. Returns (kept, total).
pub fn cull_to_view(geo: &mut Geometry, cams: &[Camera], radius: f32) -> (usize, usize) {
    let (cells, before) = crop_terrain(geo, cams, radius);
    eprintln!("  terrain cells in view: {cells} of {before}");
    let Some(inst) = geo.instances.as_ref() else { return (0, 0) };
    let total = inst.len();
    let far_standin: Vec<bool> = inst.protos.iter().map(|p| pt_core::lod::hinted_cull(p).is_some()).collect();
    let keep: Vec<u32> = (0..total as u32)
        .filter(|&k| {
            let k = k as usize;
            let pos = [inst.xf[k][3], inst.xf[k][7], inst.xf[k][11]];
            if far_standin[inst.proto_id[k] as usize] {
                // a far stand-in matters only where the camera looks, at any distance (200 000 of them are 4 million triangles)
                return cams.iter().any(|c| in_view(c, pos, FAR_RANGE_M));
            }
            cams.iter().any(|c| {
                let d = ((pos[0] - c.x).powi(2) + (pos[2] - c.z).powi(2)).sqrt();
                d <= radius || in_view(c, pos, radius * 4.0)
            })
        })
        .collect();
    let kept = keep.len();
    // what is left, by prototype: the render's memory goes where the instance count and the triangles per instance multiply
    let mut per = vec![0usize; inst.protos.len()];
    for &k in &keep {
        per[inst.proto_id[k as usize] as usize] += 1;
    }
    let mut rows: Vec<(usize, usize)> = per.iter().enumerate().map(|(p, n)| (*n, p)).filter(|r| r.0 > 0).collect();
    rows.sort_by(|a, b| b.cmp(a));
    for (n, p) in rows.iter().take(6) {
        let g = &inst.protos[*p];
        let name = g.material_names().first().cloned().unwrap_or_else(|| format!("proto {p}"));
        eprintln!("  {n} x {name}: {} triangles each{}", g.triangles().len(), if far_standin[*p] { " (far stand-in)" } else { "" });
    }
    if kept < total {
        let inst = geo.instances.as_mut().unwrap();
        let proto_id: Vec<u32> = keep.iter().map(|&k| inst.proto_id[k as usize]).collect();
        let xf: Vec<[f32; 12]> = keep.iter().map(|&k| inst.xf[k as usize]).collect();
        inst.attrs = inst.attrs.gather(&keep);
        inst.proto_id = std::sync::Arc::new(proto_id);
        inst.xf = std::sync::Arc::new(xf);
    }
    (kept, total)
}

/// For the scorecard: (objects floating or hanging inside a camera view, objects mostly under water inside a camera view, cameras
/// checked), counted over every standing camera of the scene; `None` when there is none or no terrain.
pub fn view_counts(geo: &Geometry, scene: &Value, radius: f32) -> Option<(u64, u64, u64)> {
    let cams = cameras_of(scene);
    let terrain = Surface::from_geometry(geo, |m| m.starts_with("terrain"));
    if cams.is_empty() || terrain.is_empty() {
        return None;
    }
    let water = Surface::from_geometry(geo, |m| m.contains("water"));
    let (findings, _) = check(geo, &terrain, &water);
    let (mut hang, mut shallow) = (0u64, 0u64);
    for f in &findings {
        let seen = cams.iter().any(|c| in_view(c, f.pos, radius));
        if seen {
            match f.kind {
                "float" | "overhang" => hang += 1,
                "shallow" => shallow += 1,
                _ => {}
            }
        }
    }
    Some((hang, shallow, cams.len() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pt_core::column::AttrSet;
    use pt_core::geo::Instances;
    use std::sync::Arc;

    /// A 100 m square of ground at height `y` named `terrain_t`, plus a proto box of 1 x 1 x 1 m with its lowest point at y = 0.
    fn world(instances: Vec<[f32; 3]>, ground_y: f32, water_y: Option<f32>) -> Geometry {
        let mut pts = vec![[-50.0, ground_y, -50.0], [50.0, ground_y, -50.0], [50.0, ground_y, 50.0], [-50.0, ground_y, 50.0]];
        let mut polys = vec![vec![0u32, 1, 2, 3]];
        if let Some(w) = water_y {
            pts.extend([[-50.0, w, -50.0], [50.0, w, -50.0], [50.0, w, 50.0], [-50.0, w, 50.0]]);
            polys.push(vec![4, 5, 6, 7]);
        }
        let mut g = Geometry::from_polys(pts, &polys);
        let mut land = vec![false; polys.len()];
        land[0] = true;
        g.assign_material("terrain_t", &land);
        if water_y.is_some() {
            g.assign_material("water_w", &[false, true]);
        }
        let proto = Geometry::from_polys(
            vec![[-0.5, 0.0, -0.5], [0.5, 0.0, -0.5], [0.5, 0.0, 0.5], [-0.5, 0.0, 0.5], [-0.5, 1.0, -0.5], [0.5, 1.0, -0.5], [0.5, 1.0, 0.5], [-0.5, 1.0, 0.5]],
            &[vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7]],
        );
        let xf: Vec<[f32; 12]> = instances.iter().map(|p| [1.0, 0.0, 0.0, p[0], 0.0, 1.0, 0.0, p[1], 0.0, 0.0, 1.0, p[2]]).collect();
        g.instances = Some(Instances { protos: vec![Arc::new(proto)], proto_id: Arc::new(vec![0; xf.len()]), xf: Arc::new(xf), attrs: AttrSet::default() });
        g
    }

    fn kinds(geo: &Geometry) -> Vec<&'static str> {
        let t = Surface::from_geometry(geo, |m| m.starts_with("terrain"));
        let w = Surface::from_geometry(geo, |m| m.contains("water"));
        check(geo, &t, &w).0.iter().map(|f| f.kind).collect()
    }

    #[test]
    fn a_stone_on_the_ground_is_fine() {
        assert!(kinds(&world(vec![[0.0, 0.0, 0.0], [5.0, 0.02, 5.0]], 0.0, None)).is_empty());
    }

    #[test]
    fn a_stone_in_the_air_floats() {
        assert_eq!(kinds(&world(vec![[0.0, 0.5, 0.0]], 0.0, None)), vec!["float"]);
    }

    #[test]
    fn a_stone_mostly_under_water_is_shallow() {
        // ground at -1, water at 0.7 above it: the box (y from -0.9 to 0.1) is almost fully under the water
        assert!(kinds(&world(vec![[0.0, -0.9, 0.0]], -1.0, Some(0.0))).contains(&"shallow"));
    }

    #[test]
    fn a_stone_sunk_into_the_ground_is_buried() {
        assert!(kinds(&world(vec![[0.0, -0.8, 0.0]], 0.0, None)).contains(&"buried"));
    }

    #[test]
    fn a_point_straight_ahead_lands_in_the_middle_of_the_frame() {
        let c = Camera { name: "c".into(), x: 0.0, z: 0.0, yaw_deg: 90.0, half_fov_deg: 33.0, pitch_deg: 0.0, eye_height: 1.7, lens: 28.0, sensor: 36.0 };
        let p = c.project(1.7, [0.0, 1.7, 10.0], 1920, 1080).unwrap();
        assert!((p[0] - 960.0).abs() < 0.5 && (p[1] - 540.0).abs() < 0.5);
        // to the camera's right is further right in the frame (yaw 90 looks along +z, so right is -x), and lower points are lower
        let right = c.project(1.7, [-2.0, 1.7, 10.0], 1920, 1080).unwrap();
        assert!(right[0] > 960.0);
        let low = c.project(1.7, [0.0, 0.0, 10.0], 1920, 1080).unwrap();
        assert!(low[1] > 540.0);
        assert!(c.project(1.7, [0.0, 1.7, -5.0], 1920, 1080).is_none());
    }

    #[test]
    fn camera_view_cone_keeps_what_is_ahead() {
        let c = Camera { name: "c".into(), x: 0.0, z: 0.0, yaw_deg: 0.0, half_fov_deg: 33.0, pitch_deg: 0.0, eye_height: 1.7, lens: 28.0, sensor: 36.0 };
        assert!(in_view(&c, [10.0, 0.0, 1.0], 45.0));
        assert!(!in_view(&c, [-10.0, 0.0, 0.0], 45.0));
        assert!(!in_view(&c, [60.0, 0.0, 0.0], 45.0));
    }
}
