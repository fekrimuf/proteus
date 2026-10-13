//! `pt survey` and `pt walk`: the game engine is the eye that finds flaws; the offline render is for the frames the owner judges.
//!
//! One offline frame costs half a minute to ten minutes and shows one side of the world, so a flaw behind a rock or beside the
//! camera is never seen. The engine loads the same world once (the tiled engine file, each prototype stored once) and draws it at
//! a hundred frames a second, so looking stops being the expensive step:
//!
//! * `pt survey` plans the views by rule and takes them all in one engine launch:
//!   - a WALK: standing points spread over the area the scene's cameras look at, four directions each;
//!   - the area from ABOVE (four sides and a plan), and the scene's own CAMERAS;
//!   - a CLOSE-UP pair for every object the ground check flags (floating, hanging, mostly under water), from the two sides with
//!     the clearest sight, so the defects are looked at and not searched for;
//!   and writes them as contact sheets of twelve labelled views, plus the same views with every flagged object painted in its
//!   colour (`flags`) or every kind of object in a colour of its own (`kinds`). `survey.scene.json` holds every view as a scene
//!   camera, so `pt pick` names what is at a pixel of any of them.
//! * `pt walk` opens the same world in a window to move through, under the same protections as every heavy child (low priority, the
//!   memory cap, stopped when `pt` exits, a time limit on the load). What the owner clicks on is cast into the cooked world when the
//!   window closes and written down with the object's name, its stable id and the node that placed it.
//!
//! What the engine pictures are for: placement, shape, scale, gaps and repeats, which look the same in any renderer. Light, air
//! and surfaces are judged in the offline frames.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::grounding::{cameras_of, check, kind_colour, mark, Camera, Finding, Surface};
use crate::pick::{Hit, World};
use crate::registry::{Command, Kind, Param};
use pt_core::math::{self, xform_point};
use pt_core::{Column, Geometry};
use pt_graph::Graph;
use pt_render::Image;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

type V3 = [f32; 3];

/// Height of a standing eye above the ground, metres.
const EYE: f32 = 1.7;

/// What a close-up is aimed at: one object the ground check flagged.
#[derive(Clone, Debug)]
pub struct Target {
    pub instance: usize,
    pub proto: u32,
    pub kind: &'static str,
    pub what: String,
    pub amount: f32,
    /// The point the camera looks at: the middle of the object's lowest few metres, where it meets (or misses) the ground.
    pub aim: V3,
    /// Size of the part of the object that is looked at, metres.
    pub size: f32,
    /// Size of what the frame must hold: the object and the ground under it, so the air under a floating object is in the picture.
    pub frame: f32,
}

/// One view of the world.
#[derive(Clone, Debug)]
pub struct View {
    pub name: String,
    /// `walk`, `above`, `camera` or `flaw`.
    pub group: &'static str,
    pub why: String,
    pub eye: V3,
    /// Degrees, the scene file's convention: yaw 0 looks along +X and grows toward +Z; pitch up is positive.
    pub yaw: f32,
    pub pitch: f32,
    pub lens: f32,
    pub target: Option<Target>,
}

fn len3(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

impl View {
    /// A view from `eye` that has `aim` in the middle of the frame.
    pub fn looking_at(name: String, group: &'static str, why: String, eye: V3, aim: V3, lens: f32) -> View {
        let d = math::sub(aim, eye);
        let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
        // straight down (or up) has no heading of its own: keep +Z up the picture
        let yaw = if flat < 1e-4 { 90.0 } else { math::atan2(d[2], d[0]).to_degrees() };
        View { name, group, why, eye, yaw, pitch: math::atan2(d[1], flat).to_degrees(), lens, target: None }
    }

    /// The same camera as `pt pick` and the ground check use, so a pixel of this view means the same thing everywhere.
    pub fn camera(&self) -> Camera {
        Camera { name: self.name.clone(), x: self.eye[0], z: self.eye[2], yaw_deg: self.yaw, half_fov_deg: math::atan(18.0 / self.lens).to_degrees(), pitch_deg: self.pitch, eye_height: 0.0, lens: self.lens, sensor: 36.0, subject: None }
    }

    /// Where a world point lands in a `w` x `h` picture of this view; `None` outside it.
    pub fn project(&self, p: V3, w: usize, h: usize) -> Option<[f32; 2]> {
        self.camera().project(self.eye[1], p, w, h)
    }
}

/// The part of the world a survey looks at: a disc on the ground.
#[derive(Clone, Copy, Debug)]
pub struct Focus {
    pub x: f32,
    pub z: f32,
    pub radius: f32,
}

impl Focus {
    fn holds(&self, x: f32, z: f32, share: f32) -> bool {
        math::sq(x - self.x) + math::sq(z - self.z) <= math::sq(self.radius * share)
    }
}

/// Where to look when nobody says: around the scene's standing cameras (that is where the owner looks), else the dressed ground
/// (the box around the placed objects that are not far stand-ins), else the middle of the base geometry.
pub fn focus_of(scene: &Value, geo: &Geometry) -> Focus {
    let standing: Vec<Camera> = cameras_of(scene).into_iter().filter(|c| c.eye_height <= 10.0).collect();
    if !standing.is_empty() {
        let n = standing.len() as f32;
        let (x, z) = (standing.iter().map(|c| c.x).sum::<f32>() / n, standing.iter().map(|c| c.z).sum::<f32>() / n);
        let far = standing.iter().map(|c| (math::sq(c.x - x) + math::sq(c.z - z)).sqrt()).fold(0.0f32, f32::max);
        return Focus { x, z, radius: (far + 40.0).clamp(60.0, 250.0) };
    }
    let around = |lo: V3, hi: V3, share: f32| Focus { x: (lo[0] + hi[0]) * 0.5, z: (lo[2] + hi[2]) * 0.5, radius: (share * (math::sq(hi[0] - lo[0]) + math::sq(hi[2] - lo[2])).sqrt()).clamp(30.0, 150.0) };
    if let Some(inst) = &geo.instances {
        let near: Vec<bool> = inst.protos.iter().map(|p| pt_core::lod::hinted_cull(p).is_none()).collect();
        let (mut lo, mut hi, mut any) = ([f32::MAX; 3], [f32::MIN; 3], false);
        for (k, xf) in inst.xf.iter().enumerate() {
            if !near[inst.proto_id[k] as usize] {
                continue;
            }
            for (a, v) in [xf[3], xf[7], xf[11]].into_iter().enumerate() {
                lo[a] = lo[a].min(v);
                hi[a] = hi[a].max(v);
            }
            any = true;
        }
        if any {
            return around(lo, hi, 0.5);
        }
    }
    match geo.base_bounds() {
        Some((lo, hi)) => around(lo, hi, 0.25),
        None => Focus { x: 0.0, z: 0.0, radius: 60.0 },
    }
}

/// Standing points for the walk: on ground a person can stand on (not steep, not in deep water, nothing within arm's reach of
/// the face), spread as far from each other as the inner part of the area allows. The first is the one nearest the middle.
pub fn stations(terrain: &Surface, water: &Surface, world: &World, f: &Focus, n: usize) -> Vec<V3> {
    let step = (f.radius / 10.0).clamp(2.0, 10.0);
    let k = (f.radius / step).floor() as i32;
    let mut cand: Vec<V3> = Vec::new();
    for ix in -k..=k {
        for iz in -k..=k {
            let (x, z) = (f.x + ix as f32 * step, f.z + iz as f32 * step);
            if !f.holds(x, z, 0.7) {
                continue;
            }
            let Some(g) = terrain.height(x, z) else { continue };
            let d = 1.5f32;
            let slope = [(d, 0.0), (-d, 0.0), (0.0, d), (0.0, -d)].iter().filter_map(|(dx, dz)| terrain.height(x + dx, z + dz)).map(|h| (h - g).abs() / d).fold(0.0f32, f32::max);
            if slope > 0.6 || water.height(x, z).is_some_and(|w| w > g + 0.4) {
                continue;
            }
            let eye = [x, g + EYE, z];
            // nothing under the eye but the ground (not on top of a rock, not inside a bush), and nothing within arm's reach at
            // eye height or at the waist
            let on_ground = !matches!(world.cast_within(eye, [0.0, -1.0, 0.0], EYE - 0.05), Some(Hit::Instance { .. }));
            let clear = on_ground
                && (0..16).all(|a| {
                    let (s, c) = math::sin_cos((a / 2) as f32 * math::PI / 4.0);
                    let from = if a % 2 == 0 { eye } else { [x, g + 0.9, z] };
                    world.cast_within(from, [c, 0.0, s], 1.2).is_none()
                });
            if clear {
                cand.push(eye);
            }
        }
    }
    let mut out: Vec<V3> = Vec::new();
    let d2 = |a: &V3, b: &V3| math::sq(a[0] - b[0]) + math::sq(a[2] - b[2]);
    let centre = [f.x, 0.0, f.z];
    while out.len() < n && !cand.is_empty() {
        let score = |c: &V3| if out.is_empty() { -d2(c, &centre) } else { out.iter().map(|o| d2(o, c)).fold(f32::MAX, f32::min) };
        let best = (0..cand.len()).max_by(|&a, &b| score(&cand[a]).total_cmp(&score(&cand[b])).then(b.cmp(&a))).unwrap_or(0);
        out.push(cand.remove(best));
    }
    out
}

/// The walk: from every station, four views a quarter turn apart, the first toward the middle of the area, tilted a little down
/// (flaws are on the ground). A 20 mm lens sees 84 degrees, so the four cover nearly the whole turn.
pub fn walk_views(stations: &[V3], f: &Focus) -> Vec<View> {
    let mut out = Vec::new();
    for (i, eye) in stations.iter().enumerate() {
        let (dx, dz) = (f.x - eye[0], f.z - eye[2]);
        let base = if dx * dx + dz * dz < 1.0 { 0.0 } else { math::atan2(dz, dx).to_degrees() };
        for (q, turn) in ["a", "b", "c", "d"].iter().enumerate() {
            let yaw = base + 90.0 * q as f32;
            out.push(View { name: format!("walk_{}{turn}", i + 1), group: "walk", why: format!("standing at {:.0}, {:.0}, looking {:.0} degrees", eye[0], eye[2], yaw.rem_euclid(360.0)), eye: *eye, yaw, pitch: -6.0, lens: 20.0, target: None });
        }
    }
    out
}

/// The area from above: from four sides, looking down at its middle, and one plan view that holds the whole disc.
pub fn above_views(terrain: &Surface, f: &Focus) -> Vec<View> {
    let g0 = terrain.height(f.x, f.z).unwrap_or(0.0);
    let aim = [f.x, g0, f.z];
    let mut top = g0;
    for ix in -4..=4 {
        for iz in -4..=4 {
            if let Some(h) = terrain.height(f.x + ix as f32 * f.radius / 4.0, f.z + iz as f32 * f.radius / 4.0) {
                top = top.max(h);
            }
        }
    }
    let mut out = Vec::new();
    for q in 0..4 {
        let az = 45.0 + 90.0 * q as f32;
        let (s, c) = math::sin_cos(math::to_radians(az));
        let mut eye = [f.x + c * f.radius * 0.9, g0 + f.radius * 0.6, f.z + s * f.radius * 0.9];
        if let Some(g) = terrain.height(eye[0], eye[2]) {
            eye[1] = eye[1].max(g + 8.0);
        }
        out.push(View::looking_at(format!("above_{az:.0}"), "above", format!("the area from above, from {az:.0} degrees"), eye, aim, 24.0));
    }
    // an 18 mm lens sees 29.4 degrees up and down from the middle of a 16:9 frame: the disc fits at 1.8 radii above the tree tops
    out.push(View::looking_at("above_plan".into(), "above", "the area in plan (straight down)".into(), [f.x, top + 30.0 + 1.8 * f.radius, f.z], aim, 18.0));
    out
}

/// The scene's own standing cameras, as the offline render sees them.
pub fn camera_views(scene: &Value, terrain: &Surface) -> Vec<View> {
    cameras_of(scene)
        .into_iter()
        .map(|c| View { name: format!("cam_{}", c.name), group: "camera", why: format!("the scene's camera `{}`", c.name), eye: [c.x, terrain.height(c.x, c.z).unwrap_or(0.0) + c.eye_height, c.z], yaw: c.yaw_deg, pitch: c.pitch_deg, lens: c.lens, target: None })
        .collect()
}

/// A prototype in words: its first material name.
pub fn proto_name(geo: &Geometry, p: u32) -> String {
    geo.instances.as_ref().and_then(|i| i.protos.get(p as usize)).and_then(|g| g.material_names().first().cloned()).unwrap_or_else(|| format!("proto {p}"))
}

/// The flagged objects worth a close look: the ones an eye catches (floating, hanging, mostly under water; a buried object is
/// not seen), inside the area, the worst first, at most three of one kind of object with one kind of defect, so twelve close-ups
/// show twelve things and not one class twelve times.
pub fn targets(geo: &Geometry, findings: &[Finding], terrain: &Surface, f: &Focus, max: usize) -> Vec<Target> {
    let Some(inst) = &geo.instances else { return Vec::new() };
    let rank = |k: &str| match k {
        "float" => 0,
        "overhang" => 1,
        "shallow" => 2,
        _ => 9,
    };
    let mut pool: Vec<&Finding> = findings.iter().filter(|x| rank(x.kind) < 9 && f.holds(x.pos[0], x.pos[2], 1.0)).collect();
    pool.sort_by(|a, b| rank(a.kind).cmp(&rank(b.kind)).then(b.amount.total_cmp(&a.amount)).then(a.index.cmp(&b.index)));
    let mut per_class: BTreeMap<(&str, u32), usize> = BTreeMap::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut out = Vec::new();
    for x in pool {
        if out.len() >= max {
            break;
        }
        let n = per_class.entry((x.kind, x.proto)).or_default();
        if *n >= 3 || !seen.insert(x.index) {
            continue;
        }
        let Some((lo, hi)) = inst.protos.get(x.proto as usize).and_then(|p| p.bounds()) else { continue };
        *n += 1;
        let xf = &inst.xf[x.index];
        let (mut wlo, mut whi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for c in 0..8 {
            let q = xform_point(xf, [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }]);
            for a in 0..3 {
                wlo[a] = wlo[a].min(q[a]);
                whi[a] = whi[a].max(q[a]);
            }
        }
        let ext = [whi[0] - wlo[0], whi[1] - wlo[1], whi[2] - wlo[2]];
        let wide = ext[0].max(ext[2]);
        // a tree is looked at where it stands, not as a whole: the lowest four metres at most
        let size = wide.max(ext[1]).clamp(0.3, 4.0);
        let (ax, az) = if wide <= 4.0 { ((wlo[0] + whi[0]) * 0.5, (wlo[2] + whi[2]) * 0.5) } else { (x.pos[0], x.pos[2]) };
        let aim = [ax, wlo[1] + ext[1].min(size) * 0.5, az];
        // a 35 mm lens at 2.5 frame sizes sees 0.72 of a frame size below the middle: the ground under the aim point must be inside
        let frame = terrain.height(ax, az).map_or(size, |g| size.max(1.7 * (aim[1] - g))).min(8.0);
        out.push(Target { instance: x.index, proto: x.proto, kind: x.kind, what: proto_name(geo, x.proto), amount: x.amount, aim, size, frame });
    }
    out
}

/// Two views of every target: one low (it shows the air under a floating object) and one higher from another side, each from the
/// direction with the clearest sight of twelve. Sight is a ray from the eye to the aim point: clear when it meets the object
/// itself, nearly clear when nothing stands in the way before the object's own size.
pub fn closeup_views(targets: &[Target], world: &World, terrain: &Surface, water: &Surface) -> Vec<View> {
    let mut out = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        let d = (2.5 * t.frame).clamp(2.0, 12.0);
        // (score, direction 0..12, eye)
        let mut cands: [Vec<(i32, usize, V3)>; 2] = [Vec::new(), Vec::new()];
        for a in 0..12usize {
            let (s, c) = math::sin_cos(a as f32 * math::PI / 6.0);
            for (level, lift) in [0.12f32, 0.45].into_iter().enumerate() {
                let mut eye = [t.aim[0] + c * d, t.aim[1] + lift * d, t.aim[2] + s * d];
                let Some(g) = terrain.height(eye[0], eye[2]) else { continue };
                let floor = water.height(eye[0], eye[2]).map_or(g, |w| w.max(g));
                eye[1] = eye[1].max(floor + 0.4);
                let dir = math::sub(t.aim, eye);
                let dist = len3(dir);
                let score = match world.cast_within(eye, dir, dist + t.size) {
                    Some(Hit::Instance { k, .. }) if k as usize == t.instance => 2,
                    Some(h) if h.dist() >= dist - 0.75 * t.size => 1,
                    None => 1,
                    _ => 0,
                };
                cands[level].push((score, a, eye));
            }
        }
        let best = |list: &[(i32, usize, V3)], away_from: Option<usize>| -> Option<(i32, usize, V3)> {
            let apart = |a: usize| away_from.is_none_or(|b| (3..=9).contains(&((a + 12 - b) % 12)));
            list.iter().filter(|c| apart(c.1)).max_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1))).copied()
        };
        let first = best(&cands[0], None).or_else(|| best(&cands[1], None));
        let Some(first) = first else { continue };
        let second = best(&cands[1], Some(first.1)).or_else(|| best(&cands[0], Some(first.1)));
        for (tag, pick) in [("a", Some(first)), ("b", second)] {
            let Some((score, _, eye)) = pick else { continue };
            let sight = ["blocked", "nearly clear", "clear"][score.clamp(0, 2) as usize];
            let why = format!("{} {:.2} m: {} (instance {}), sight {sight}", t.kind, t.amount, t.what, t.instance);
            let mut v = View::looking_at(format!("flaw_{:02}{tag}", i + 1), "flaw", why, eye, t.aim, 35.0);
            v.target = Some(t.clone());
            out.push(v);
        }
    }
    out
}

/// Everything a survey looks at, in the order of the sheets: the walk, the area from above and the scene's cameras, the close-ups.
pub fn plan_views(geo: &Geometry, scene: &Value, findings: &[Finding], terrain: &Surface, water: &Surface, world: &World, f: &Focus, n_stations: usize, n_closeups: usize) -> Vec<View> {
    let mut views = walk_views(&stations(terrain, water, world, f, n_stations), f);
    views.extend(above_views(terrain, f));
    views.extend(camera_views(scene, terrain));
    views.extend(closeup_views(&targets(geo, findings, terrain, f, n_closeups), world, terrain, water));
    views
}

/// A colour per number, far from its neighbours' (golden-ratio steps round the colour wheel), as sRGB 0 to 1.
pub fn kind_rgb(i: usize) -> [f32; 3] {
    let h = (i as f32 * 0.618_034).fract() * 6.0;
    let (s, v) = (if i % 2 == 0 { 0.85 } else { 0.6 }, if i % 3 == 0 { 0.75 } else { 1.0 });
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [v * (1.0 - s + s * r), v * (1.0 - s + s * g), v * (1.0 - s + s * b)]
}

/// The plan the engine script reads: the picture size, the sun, every flagged object with its colour, a colour per kind of
/// object, and the shots (a view in a pass, with the file it is written to).
pub fn engine_plan(geo: &Geometry, scene: &Value, findings: &[Finding], shots: &[(usize, &str, PathBuf)], views: &[View], size: (usize, usize)) -> Value {
    let mut flagged: BTreeMap<usize, &Finding> = BTreeMap::new();
    // one colour per object: the defect an eye catches first wins
    for kind in ["float", "overhang", "shallow", "buried"] {
        for f in findings.iter().filter(|f| f.kind == kind) {
            flagged.entry(f.index).or_insert(f);
        }
    }
    let flags: Vec<Value> = flagged
        .values()
        .map(|f| {
            let c = kind_colour(f.kind);
            json!([f.proto, f.pos[0], f.pos[1], f.pos[2], c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0])
        })
        .collect();
    let n_protos = geo.instances.as_ref().map_or(0, |i| i.protos.len());
    let kinds: BTreeMap<String, [f32; 3]> = (0..n_protos).map(|p| (p.to_string(), kind_rgb(p))).collect();
    let base_kinds: BTreeMap<String, [f32; 3]> = geo
        .material_names()
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            let c = if n.starts_with("terrain") {
                [0.5, 0.5, 0.5]
            } else if n.contains("water") {
                [0.15, 0.4, 0.9]
            } else {
                kind_rgb(n_protos + i)
            };
            (n, c)
        })
        .collect();
    json!({
        "size": [size.0, size.1],
        "raw": true,
        "sun": {"azimuth": scene.pointer("/lighting/sun_azimuth").and_then(Value::as_f64).unwrap_or(150.0), "elevation": scene.pointer("/lighting/sun_elevation").and_then(Value::as_f64).unwrap_or(27.0)},
        "flags": flags,
        "kinds": kinds,
        "base_kinds": base_kinds,
        "views": shots.iter().map(|(v, pass, file)| { let v = &views[*v]; json!({"name": v.name, "eye": v.eye, "yaw": v.yaw, "pitch": v.pitch, "lens": v.lens, "pass": pass, "file": crate::engine::engine_path(file)}) }).collect::<Vec<_>>(),
        "cameras": views.iter().filter(|v| v.group == "camera").map(|v| json!({"name": v.name.trim_start_matches("cam_"), "eye": v.eye, "yaw": v.yaw, "pitch": v.pitch, "lens": v.lens})).collect::<Vec<_>>(),
    })
}

/// The scene file with every view as a standing camera (its height is above the terrain under it, as scene cameras are written),
/// so `pt pick <recipe> survey.scene.json <view> X Y` names what is at a pixel of any survey picture.
pub fn scene_with_views(scene: &Value, views: &[View], terrain: &Surface) -> Value {
    let mut s = scene.clone();
    let cams: Vec<Value> = views
        .iter()
        .map(|v| json!({"name": v.name, "eye": {"at": [v.eye[0], v.eye[2]], "height": v.eye[1] - terrain.height(v.eye[0], v.eye[2]).unwrap_or(0.0), "yaw": v.yaw, "pitch": v.pitch}, "lens": v.lens}))
        .collect();
    if let Some(o) = s.as_object_mut() {
        o.remove("camera");
        o.insert("cameras".into(), json!(cams));
    }
    s
}

/// An engine draws a layered ground (`blend_materials`) only after a bake; without one it is a white sheet that reads as snow.
/// For looking at placement the ground gets its layers' colours as point colours: every layer's flat colour (the library's
/// fallback times the style's tint), mixed by the layer weights. Returns the number of points coloured.
pub fn tint_layered(g: &mut Geometry, scene: &Value, lib: &Value) -> usize {
    let colour_of = |role: &str| -> [f32; 3] {
        let spec = scene.pointer(&format!("/materials/{role}")).cloned().unwrap_or(json!({}));
        let key = spec.get("use").and_then(Value::as_str).unwrap_or(role).to_string();
        let entry = lib.pointer(&format!("/materials/{key}")).cloned().unwrap_or(json!({}));
        let v3 = |v: Option<&Value>| v.and_then(Value::as_array).filter(|a| a.len() >= 3).map(|a| [a[0].as_f64().unwrap_or(0.5) as f32, a[1].as_f64().unwrap_or(0.5) as f32, a[2].as_f64().unwrap_or(0.5) as f32]);
        let base = v3(spec.get("color")).or_else(|| v3(spec.get("fallback"))).or_else(|| v3(entry.get("fallback"))).unwrap_or([0.4, 0.4, 0.4]);
        let tint = v3(spec.get("tint")).or_else(|| v3(entry.get("tint"))).unwrap_or([1.0, 1.0, 1.0]);
        [base[0] * tint[0], base[1] * tint[1], base[2] * tint[2]]
    };
    let layers: BTreeMap<String, Vec<[f32; 3]>> = g
        .detail
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix("layers:")?.to_string(), v.as_array()?.iter().filter_map(Value::as_str).map(colour_of).collect())))
        .filter(|(name, _)| !g.detail.contains_key(&format!("baked:{name}")))
        .collect();
    let Some(Column::V4(w0)) = g.point.get("Wt0").cloned() else { return 0 };
    let w1 = match g.point.get("Wt1").cloned() {
        Some(Column::V4(w)) => Some(w),
        _ => None,
    };
    let n = g.n_points();
    if layers.is_empty() || w0.len() != n {
        return 0;
    }
    let mut cd: Vec<[f32; 3]> = g.point.get("Cd").and_then(Column::as_v3).filter(|c| c.len() == n).map_or_else(|| vec![[1.0; 3]; n], <[[f32; 3]]>::to_vec);
    let mut done = vec![false; n];
    let mut coloured = 0usize;
    {
        let mats = g.prim_materials();
        for (i, m) in mats.iter().enumerate() {
            let Some(cols) = m.and_then(|m| layers.get(m)) else { continue };
            for &q in g.prim_points(i) {
                let q = q as usize;
                if std::mem::replace(&mut done[q], true) {
                    continue;
                }
                let (mut sum, mut total) = ([0.0f32; 3], 0.0f32);
                for (l, c) in cols.iter().enumerate().take(8) {
                    let w = if l < 4 { w0[q][l] } else { w1.as_ref().map_or(0.0, |w| w[q][l - 4]) };
                    total += w;
                    for a in 0..3 {
                        sum[a] += w * c[a];
                    }
                }
                if total > 1e-6 {
                    // the engine multiplies the point colour into a 0.8 grey placeholder
                    cd[q] = [sum[0] / total / 0.8, sum[1] / total / 0.8, sum[2] / total / 0.8];
                    coloured += 1;
                }
            }
        }
    }
    g.point.set("Cd", Column::V3(Arc::new(cd)));
    coloured
}

// ---- pictures: shrinking, labels, sheets ----

/// A picture shrunk to `w` x `h` by averaging the pixels each new pixel covers.
pub fn shrink(img: &Image, w: usize, h: usize) -> Image {
    let (w, h) = (w.max(1), h.max(1));
    let mut out = Image { w, h, rgb: vec![0; w * h * 3] };
    if img.w == 0 || img.h == 0 {
        return out;
    }
    for y in 0..h {
        let (y0, y1) = (y * img.h / h, ((y + 1) * img.h).div_ceil(h).clamp(y * img.h / h + 1, img.h));
        for x in 0..w {
            let (x0, x1) = (x * img.w / w, ((x + 1) * img.w).div_ceil(w).clamp(x * img.w / w + 1, img.w));
            let mut sum = [0u32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let o = (sy * img.w + sx) * 3;
                    for a in 0..3 {
                        sum[a] += img.rgb[o + a] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (y * w + x) * 3;
            for a in 0..3 {
                out.rgb[o + a] = (sum[a] / n.max(1)) as u8;
            }
        }
    }
    out
}

/// A 5 x 7 letter: seven rows, the five low bits of each from left to right. Capitals, digits and the marks a label needs.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        ',' => [0, 0, 0, 0, 0b01100, 0b00100, 0b01000],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        ':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        ' ' => [0; 7],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}

/// Write `text` on a picture at (`x`, `y`): white letters on a dark plate, `scale` pixels per dot. What does not fit is cut.
pub fn label(img: &mut Image, x: usize, y: usize, text: &str, scale: usize) {
    let scale = scale.max(1);
    let fits = (img.w.saturating_sub(x + 2 * scale)) / (6 * scale);
    let chars: Vec<char> = text.chars().take(fits).collect();
    let (pw, ph) = (chars.len() * 6 * scale + 2 * scale, 9 * scale);
    let mut put = |px: usize, py: usize, rgb: [u8; 3]| {
        if px < img.w && py < img.h {
            let o = (py * img.w + px) * 3;
            img.rgb[o..o + 3].copy_from_slice(&rgb);
        }
    };
    for py in y..y + ph {
        for px in x..x + pw {
            put(px, py, [16, 16, 16]);
        }
    }
    for (i, c) in chars.iter().enumerate() {
        let rows = glyph(*c);
        for (r, bits) in rows.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            put(x + scale + (i * 6 + col) * scale + dx, y + scale + r * scale + dy, [255, 255, 255]);
                        }
                    }
                }
            }
        }
    }
}

const SHEET_COLS: usize = 4;
const SHEET_CELLS: usize = 12;
const CELL: (usize, usize) = (480, 270);

/// Contact sheets of twelve labelled pictures (4 x 3, each 480 x 270): `sheet_<stem>_<n>.png` in `dir`. Returns, per sheet, its
/// file and what each numbered cell shows.
pub fn sheets(dir: &Path, stem: &str, cells: &[(String, String, PathBuf)]) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for (s, chunk) in cells.chunks(SHEET_CELLS).enumerate() {
        let mut imgs = Vec::new();
        let mut legend = Vec::new();
        for (i, (name, why, file)) in chunk.iter().enumerate() {
            let img = pt_render::decode_png(&std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?)?;
            let mut small = shrink(&img, CELL.0, CELL.1);
            label(&mut small, 4, 4, &format!("{} {name}", i + 1), 2);
            imgs.push(small);
            legend.push(json!({"cell": i + 1, "view": name, "shows": why, "file": file.to_string_lossy()}));
        }
        let file = dir.join(format!("sheet_{stem}_{}.png", s + 1));
        std::fs::write(&file, pt_render::encode_png(&pt_render::grid(&imgs, SHEET_COLS, 4))?).map_err(|e| format!("{}: {e}", file.display()))?;
        out.push(json!({"sheet": file.to_string_lossy(), "cells": legend}));
    }
    Ok(out)
}

/// The PNG of a picture the engine wrote as plain bytes (`<file>.rgb`, three bytes a pixel, `size` pixels); the bytes are removed.
pub fn png_from_raw(file: &Path, size: (usize, usize)) -> Result<(), String> {
    let raw = PathBuf::from(format!("{}.rgb", file.display()));
    let rgb = std::fs::read(&raw).map_err(|e| format!("{}: {e}", raw.display()))?;
    if rgb.len() != size.0 * size.1 * 3 {
        return Err(format!("{}: {} bytes, expected {} x {} x 3", raw.display(), rgb.len(), size.0, size.1));
    }
    std::fs::write(file, pt_render::encode_png(&Image { w: size.0, h: size.1, rgb })?).map_err(|e| format!("{}: {e}", file.display()))?;
    let _ = std::fs::remove_file(&raw);
    Ok(())
}

/// A copy of a close-up with its target boxed in the defect's colour (red floats, orange hangs, yellow is mostly under water).
fn boxed(view: &View, file: &Path, out: &Path) -> Result<(), String> {
    let mut img = pt_render::decode_png(&std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?)?;
    if let Some(t) = &view.target {
        if let Some(p) = view.project(t.aim, img.w, img.h) {
            let dist = len3(math::sub(t.aim, view.eye)).max(0.5);
            let half = (view.lens / 36.0 * img.w as f32 * t.size / dist * 0.75).clamp(14.0, img.h as f32 * 0.45);
            mark(&mut img, p[0], p[1], half, kind_colour(t.kind));
        }
    }
    std::fs::write(out, pt_render::encode_png(&img)?).map_err(|e| format!("{}: {e}", out.display()))
}

// ---- the world, cooked and exported for the engine ----

/// A world cooked from its recipe and scene file.
pub struct Cooked {
    pub graph: Graph,
    pub node: String,
    pub geo: Arc<Geometry>,
    pub hash: [u8; 32],
    /// The scene as written.
    pub raw_scene: Value,
    /// The scene with its style folded in (materials, light).
    pub scene: Value,
}

pub fn cook(recipe: &str, scene_path: &str, lookdev: bool, provenance: bool) -> Result<Cooked, String> {
    let code = std::fs::read_to_string(recipe).map_err(|e| format!("{recipe}: {e}"))?;
    let raw_scene: Value = serde_json::from_str(std::fs::read_to_string(scene_path).map_err(|e| format!("{scene_path}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{scene_path}: {e}"))?;
    let mut scene = raw_scene.clone();
    crate::blender::resolve_style(&mut scene)?;
    let mut graph = Graph::new();
    graph.knobs = crate::blender::knobs_of(&raw_scene)?;
    graph.lookdev = lookdev;
    graph.provenance = provenance;
    let rep = pt_hcs::run(&code, &mut graph, true).map_err(|d| d.first().map_or_else(|| "the recipe did not run".to_string(), |x| x.to_string()))?;
    let node = rep.outputs.first().ok_or("recipe has no out()")?.clone();
    let entry = graph.cook_entry(&node)?;
    Ok(Cooked { graph, node, geo: entry.geo, hash: entry.hash, raw_scene, scene })
}

/// The engine file of a cooked world in `dir` (`world.engine.glb`), written again only when the world, the style's materials or
/// the program changed. Returns (file, was it reused, megabytes).
pub fn engine_file(c: &Cooked, dir: &Path) -> Result<(PathBuf, bool, f64), String> {
    let root = crate::blender::repo_root();
    let (glb, key_file) = (dir.join("world.engine.glb"), dir.join("world.engine.key"));
    let mut h = blake3::Hasher::new();
    h.update(b"survey-engine/3");
    h.update(&c.hash);
    if let Some(Ok(hex)) = std::env::current_exe().ok().map(|e| crate::blender::file_hash(&e)) {
        h.update(hex.as_bytes());
    }
    h.update(c.scene.get("materials").map(Value::to_string).unwrap_or_default().as_bytes());
    let key = h.finalize().to_hex().to_string();
    let mb = |p: &Path| std::fs::metadata(p).map_or(0.0, |m| m.len() as f64 / 1_048_576.0);
    if glb.exists() && std::fs::read_to_string(&key_file).is_ok_and(|k| k.trim() == key) {
        return Ok((glb.clone(), true, mb(&glb)));
    }
    let lib: Value = std::fs::read_to_string(root.join("corpus/materials/library.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({"materials": {}}));
    let mut tinted: Geometry = (*c.geo).clone();
    tint_layered(&mut tinted, &c.scene, &lib);
    let engine_geo = crate::scorecard::engine_geometry(&tinted, &c.scene, &lib);
    drop(tinted);
    // The engine profile swaps a bush for its shell at 16 m, which is right for a game and hides the shapes a survey looks at. The
    // levels are the engine's own; they switch three times further out (a 2 m bush keeps its shape to 20 m, a tall fir to 100 m).
    let policy = pt_core::lod::LodPolicy { cache_dir: Some(root.join("out/lod_cache")), switch_factors: vec![0.0, 4.0, 10.0, 24.0], shell: false, ..pt_core::lod::LodPolicy::engine() };
    let bytes = pt_core::gltf::to_glb_tiled(&engine_geo, &policy);
    std::fs::write(&glb, bytes).map_err(|e| format!("{}: {e}", glb.display()))?;
    std::fs::write(&key_file, &key).map_err(|e| format!("{}: {e}", key_file.display()))?;
    Ok((glb.clone(), false, mb(&glb)))
}

fn flag<'a>(rest: &'a [String], name: &str) -> Option<&'a String> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1))
}

fn pair<T: std::str::FromStr>(s: &str, sep: char, what: &str) -> Result<(T, T), String> {
    let bad = || format!("{what}, got '{s}'");
    let (a, b) = s.split_once(sep).ok_or_else(bad)?;
    Ok((a.trim().parse().map_err(|_| bad())?, b.trim().parse().map_err(|_| bad())?))
}

fn out_dir(rest: &[String], recipe: &str, what: &str) -> Result<PathBuf, String> {
    let stem = Path::new(recipe).file_stem().map_or_else(|| "world".to_string(), |s| s.to_string_lossy().into_owned());
    let dir = flag(rest, "--out").map_or_else(|| crate::blender::repo_root().join("out").join(what).join(stem), PathBuf::from);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub const SURVEY: Command = Command {
    name: "survey",
    tool: "survey",
    summary: "Look at a world from many sides in seconds, in the game engine instead of the offline renderer: a walk (standing points with four views each), the area from above, the scene's cameras, and a close-up pair of every object the ground check flags (floating, hanging, mostly under water), all in one engine launch, written as contact sheets of twelve labelled views. The `flags` pass paints every flagged object in its colour (red floats, orange hangs, yellow under water, magenta buried); `kinds` paints every kind of object in a colour of its own. For placement, shape, scale and gaps; judge light and surfaces in a render. `survey.scene.json` in the output folder holds every view as a camera for `pick`.",
    heavy: true,
    params: &[
        Param::pos("recipe", Kind::Str, "the world's recipe .hcs file"),
        Param::pos("scene", Kind::Str, "the scene .json file (cameras, style)"),
        Param::flag("out", Kind::Str, "folder for the pictures, the sheets and the report (default out/survey/<recipe name>)"),
        Param::flag("at", Kind::Str, "middle of the area to look at, as X,Z in metres (default: where the scene's standing cameras are)"),
        Param::flag("radius", Kind::Num, "radius of that area in metres (default: the cameras' spread plus 40, at least 60)"),
        Param::flag("stations", Kind::Int, "standing points of the walk, four views each (default 6)"),
        Param::flag("closeups", Kind::Int, "flagged objects to look at from two sides each (default 12)"),
        Param::flag("passes", Kind::Str, "pictures per view, comma-separated: lit, flags, kinds (default lit,flags)"),
        Param::flag("size", Kind::Str, "picture size as WIDTHxHEIGHT (default 1280x720)"),
        Param::flag("lookdev", Kind::Bool, "cook in the look-development tier"),
        Param::flag("plan_only", Kind::Bool, "plan the views and write the plan and survey.scene.json without starting the engine"),
    ],
    run: survey_cmd,
};

/// `pt survey <recipe.hcs> <scene.json> [--out DIR] [--at X,Z] [--radius M] [--stations N] [--closeups N] [--passes lit,flags,kinds] [--size WxH] [--lookdev] [--plan-only]`
pub fn survey_cmd(args: &[String]) -> Result<(), String> {
    let [recipe, scene_path, rest @ ..] = args else { return Err(format!("usage: {}", SURVEY.usage())) };
    let t_all = Instant::now();
    let dir = out_dir(rest, recipe, "survey")?;
    let size: (usize, usize) = flag(rest, "--size").map_or(Ok((1280, 720)), |s| pair(s, 'x', "--size is WIDTHxHEIGHT, for example 1280x720"))?;
    let passes: Vec<&str> = flag(rest, "--passes").map_or("lit,flags", String::as_str).split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    if let Some(bad) = passes.iter().find(|p| !["lit", "flags", "kinds"].contains(*p)) {
        return Err(format!("unknown pass '{bad}' in --passes; accepted: lit, flags, kinds"));
    }
    let count = |name: &str, default: usize| flag(rest, name).and_then(|s| s.parse::<usize>().ok()).unwrap_or(default);

    // 1. cook, and what the ground check says about every placed object
    let t = Instant::now();
    let c = cook(recipe, scene_path, rest.iter().any(|a| a == "--lookdev"), false)?;
    let cook_s = t.elapsed().as_secs_f64();
    let terrain = Surface::from_geometry(&c.geo, |m| m.starts_with("terrain"));
    if terrain.is_empty() {
        return Err("no terrain material (a name starting with 'terrain') in the cooked world: a survey stands on the ground".into());
    }
    let water = Surface::from_geometry(&c.geo, |m| m.contains("water"));
    let (findings, _) = check(&c.geo, &terrain, &water);

    // 2. where to look, and from where
    let mut focus = focus_of(&c.raw_scene, &c.geo);
    if let Some(at) = flag(rest, "--at") {
        (focus.x, focus.z) = pair(at, ',', "--at is X,Z in metres, for example 60,45")?;
    }
    if let Some(r) = flag(rest, "--radius").and_then(|s| s.parse::<f32>().ok()).filter(|r| *r > 0.0) {
        focus.radius = r;
    }
    let t = Instant::now();
    let views = {
        let world = World::new(&c.geo);
        plan_views(&c.geo, &c.raw_scene, &findings, &terrain, &water, &world, &focus, count("--stations", 6), count("--closeups", 12))
    };
    let plan_s = t.elapsed().as_secs_f64();
    // every view in the lit pass; the walk, the area from above and the cameras again in each diagnostic pass
    let mut shots: Vec<(usize, &str, PathBuf)> = Vec::new();
    for pass in ["lit", "flags", "kinds"].into_iter().filter(|p| passes.contains(p)) {
        for (i, v) in views.iter().enumerate().filter(|(_, v)| pass == "lit" || v.group != "flaw") {
            shots.push((i, pass, dir.join(format!("{pass}_{}.png", v.name))));
        }
    }
    let plan = engine_plan(&c.geo, &c.scene, &findings, &shots, &views, size);
    let plan_file = dir.join("plan.json");
    std::fs::write(&plan_file, serde_json::to_string(&plan).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", plan_file.display()))?;
    let scene_file = dir.join("survey.scene.json");
    std::fs::write(&scene_file, serde_json::to_string_pretty(&scene_with_views(&c.raw_scene, &views, &terrain)).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", scene_file.display()))?;

    let in_focus = |f: &&Finding| focus.holds(f.pos[0], f.pos[2], 1.0);
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for f in findings.iter().filter(in_focus) {
        *by_kind.entry(f.kind).or_default() += 1;
    }
    let groups = |g: &str| views.iter().filter(|v| v.group == g).count();
    let mut report = json!({
        "out": dir.to_string_lossy(),
        "area": {"middle": [focus.x, focus.z], "radius_m": focus.radius},
        "flagged_in_area": by_kind,
        "views": {"walk": groups("walk"), "above": groups("above"), "camera": groups("camera"), "flaw": groups("flaw"), "pictures": shots.len()},
        "closeups": views.iter().filter(|v| v.group == "flaw").map(|v| json!({"view": v.name, "shows": v.why})).collect::<Vec<_>>(),
        "pick": format!("pt pick {recipe} {} <view> <x> <y> --size {}x{}", scene_file.display(), size.0, size.1),
        "seconds": {"cook": r2(cook_s), "plan": r2(plan_s)},
    });
    if rest.iter().any(|a| a == "--plan-only") {
        report["plan"] = json!(plan_file.to_string_lossy());
        println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
        return Ok(());
    }

    // 3. the engine file (kept while the world is the same), then everything cooked is dropped: the engine needs the memory
    if !crate::engine::exe().exists() {
        return Err(format!("the game engine is not installed at {} (set PT_GODOT; --plan-only plans the views without it)", crate::engine::exe().display()));
    }
    let t = Instant::now();
    let (glb, reused, glb_mb) = engine_file(&c, &dir)?;
    let export_s = t.elapsed().as_secs_f64();
    drop(c);

    // 4. one engine launch takes every picture
    let plan_arg = format!("plan={}", crate::engine::engine_path(&plan_file));
    let engine = crate::engine::run(&glb, &[&plan_arg], true);
    if engine["ok"] != true {
        return Err(format!("the engine did not finish the survey: {}", engine.get("errors").map_or_else(|| engine.to_string(), Value::to_string)));
    }

    // 5. the pictures as PNG (the engine wrote plain bytes: its own PNG writer is most of a survey's time), then the sheets: the
    // walk, the overview (above and cameras), the close-ups with their target boxed, per pass
    let t = Instant::now();
    let got = (engine.pointer("/survey/size/0").and_then(Value::as_u64).unwrap_or(size.0 as u64) as usize, engine.pointer("/survey/size/1").and_then(Value::as_u64).unwrap_or(size.1 as u64) as usize);
    {
        use rayon::prelude::*;
        shots.par_iter().map(|(_, _, file)| png_from_raw(file, got)).collect::<Result<Vec<()>, String>>()?;
    }
    let mut all_sheets = Vec::new();
    for pass in passes.iter().copied() {
        let of = |groups: &[&str]| -> Vec<(String, String, PathBuf)> { shots.iter().filter(|(i, p, _)| *p == pass && groups.contains(&views[*i].group)).map(|(i, _, f)| (views[*i].name.clone(), views[*i].why.clone(), f.clone())).collect() };
        let tag = if pass == "lit" { String::new() } else { format!("_{pass}") };
        all_sheets.extend(sheets(&dir, &format!("walk{tag}"), &of(&["walk"]))?);
        all_sheets.extend(sheets(&dir, &format!("overview{tag}"), &of(&["above", "camera"]))?);
        if pass == "lit" {
            let mut cells = Vec::new();
            for (i, _, file) in shots.iter().filter(|(i, p, _)| *p == "lit" && views[*i].group == "flaw") {
                let v = &views[*i];
                let marked = dir.join(format!("boxed_{}.png", v.name));
                boxed(v, file, &marked)?;
                let short = v.target.as_ref().map_or_else(|| v.name.clone(), |t| format!("{} {} {:.2}M {}", v.name.trim_start_matches("flaw_"), t.kind, t.amount, t.what));
                cells.push((short, v.why.clone(), marked));
            }
            all_sheets.extend(sheets(&dir, "flaws", &cells)?);
        }
    }
    report["sheets"] = json!(all_sheets);
    report["engine"] = json!({
        "file_mb": r2(glb_mb), "file_reused": reused, "load_s": engine["scene_ms"].as_f64().map(|ms| r2(ms / 1000.0)), "ready_s": engine["ready_ms"].as_f64().map(|ms| r2(ms / 1000.0)),
        "pictures": engine.pointer("/survey/views"), "ms_per_picture": engine.pointer("/survey/ms_per_view"), "launch_s": engine["launch_s"], "peak_memory_mb": engine["peak_mem_mb"],
        "video_memory_mb": engine.pointer("/survey/video_mem_mb"), "objects_painted_as_flagged": engine.pointer("/instancing/nodes_with_flagged"),
    });
    report["seconds"] = json!({"cook": r2(cook_s), "plan": r2(plan_s), "engine_file": r2(export_s), "engine": engine["launch_s"], "sheets": r2(t.elapsed().as_secs_f64()), "total": r2(t_all.elapsed().as_secs_f64())});
    std::fs::write(dir.join("survey.json"), serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    Ok(())
}

pub const WALK: Command = Command {
    name: "walk",
    tool: "walk",
    summary: "Open a world in a window to walk or fly through, in the game engine, under the same protections as every heavy job (low priority, the memory cap, stopped when `pt` exits, a time limit on the load): the safe way to open a world by hand. Right mouse button to look, W A S D to move, G to walk on the ground, 1 to 9 for the scene's cameras, F to show flagged objects. A left click notes whatever looks wrong (X notes \"remove this\"); when the window closes each note is named (object, stable id, placing node, ground-check verdict) in notes.json. Returns when the window is closed.",
    heavy: true,
    params: &[
        Param::pos("recipe", Kind::Str, "the world's recipe .hcs file"),
        Param::pos("scene", Kind::Str, "the scene .json file (cameras, style)"),
        Param::flag("out", Kind::Str, "folder for the engine file, the notes and their pictures (default out/walk/<recipe name>)"),
        Param::flag("size", Kind::Str, "window size as WIDTHxHEIGHT (default 1600x900)"),
        Param::flag("lookdev", Kind::Bool, "cook in the look-development tier"),
        Param::flag("check", Kind::Bool, "nobody at the keyboard: open the window, note what is in the middle of the first camera's view, and close (proves the walk-through works on this machine)"),
    ],
    run: walk_cmd,
};

/// The notes of a walk, one JSON object per line as the engine wrote them.
pub fn read_notes(file: &Path) -> Vec<Value> {
    std::fs::read_to_string(file).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

fn v3_of(v: &Value) -> Option<V3> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
}

/// Name what every note points at: its ray is cast into the cooked world, like a pick. Each result carries the note's number, what
/// was asked (`flag`: it looks wrong; `remove`), one sentence, the full description and the picture taken at the click.
pub fn resolve_notes(graph: &mut Graph, node: &str, notes: &[Value]) -> Result<Vec<Value>, String> {
    let rays: Vec<(V3, V3)> = notes.iter().filter_map(|n| Some((v3_of(n.get("origin")?)?, v3_of(n.get("dir")?)?))).collect();
    if rays.len() != notes.len() {
        return Err("a note has no ray (origin and dir)".into());
    }
    let described = crate::pick::describe_rays(graph, node, &rays)?;
    Ok(notes
        .iter()
        .zip(described)
        .map(|(n, d)| {
            let sentence = crate::pick::summary(&json!({"camera": "the walk", "pixel": n.get("pixel").cloned().unwrap_or(json!([0, 0])), "result": d}));
            json!({"n": n["n"], "asked": if n["what"] == "remove" { "remove this" } else { "this looks wrong" }, "summary": sentence, "result": d, "shot": n["shot"], "pixel": n["pixel"], "eye": n["eye"], "yaw": n["yaw"], "pitch": n["pitch"]})
        })
        .collect())
}

/// `pt walk <recipe.hcs> <scene.json> [--out DIR] [--size WxH] [--lookdev] [--check]`
pub fn walk_cmd(args: &[String]) -> Result<(), String> {
    let [recipe, scene_path, rest @ ..] = args else { return Err(format!("usage: {}", WALK.usage())) };
    if !crate::engine::exe().exists() {
        return Err(format!("the game engine is not installed at {} (set PT_GODOT)", crate::engine::exe().display()));
    }
    let dir = out_dir(rest, recipe, "walk")?;
    let size: (usize, usize) = flag(rest, "--size").map_or(Ok((1600, 900)), |s| pair(s, 'x', "--size is WIDTHxHEIGHT, for example 1600x900"))?;
    let lookdev = rest.iter().any(|a| a == "--lookdev");
    let (notes_file, ready_file, plan_file) = (dir.join("notes.jsonl"), dir.join("ready.txt"), dir.join("plan.json"));
    for f in [&notes_file, &ready_file] {
        let _ = std::fs::remove_file(f);
    }

    // the world, its flagged objects and its cameras; then everything cooked is dropped, so the engine has the memory
    eprintln!("cooking the world...");
    let glb = {
        let c = cook(recipe, scene_path, lookdev, false)?;
        let terrain = Surface::from_geometry(&c.geo, |m| m.starts_with("terrain"));
        let water = Surface::from_geometry(&c.geo, |m| m.contains("water"));
        let findings = if terrain.is_empty() { Vec::new() } else { check(&c.geo, &terrain, &water).0 };
        let mut views = camera_views(&c.raw_scene, &terrain);
        if views.is_empty() {
            let f = focus_of(&c.raw_scene, &c.geo);
            views = above_views(&terrain, &f).into_iter().take(1).map(|v| View { group: "camera", ..v }).collect();
        }
        let mut plan = engine_plan(&c.geo, &c.scene, &findings, &[], &views, size);
        plan["notes"] = json!(crate::engine::engine_path(&notes_file));
        plan["ready"] = json!(crate::engine::engine_path(&ready_file));
        plan["shots"] = json!(crate::engine::engine_path(&dir));
        plan["check"] = json!(rest.iter().any(|a| a == "--check"));
        std::fs::write(&plan_file, serde_json::to_string(&plan).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", plan_file.display()))?;
        eprintln!("writing the engine file...");
        engine_file(&c, &dir)?.0
    };

    // the window: stopped if the world is not up within the time limit; open for as long as the owner likes after that
    eprintln!("opening the window (H in the window shows the controls)...");
    let plan_arg = format!("plan={}", crate::engine::engine_path(&plan_file));
    let mut cmd = crate::engine::command(&glb, &[&plan_arg, "walk=1"], true);
    let out_file = crate::lowload::sink_file("walk");
    cmd.stdin(std::process::Stdio::null()).stdout(std::fs::File::create(&out_file).map_err(|e| e.to_string())?).stderr(std::process::Stdio::null());
    let mut child = crate::lowload::spawn(&mut cmd).map_err(|e| format!("could not start the engine: {e}"))?;
    let (started, limit) = (Instant::now(), crate::engine::limit());
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if ready_file.exists() || started.elapsed() < limit => std::thread::sleep(std::time::Duration::from_millis(200)),
            _ => {
                crate::lowload::kill_tree(&mut child);
                let _ = std::fs::remove_file(&out_file);
                return Err(crate::engine::timed_out(started.elapsed().as_secs_f64(), limit)["errors"][0].as_str().unwrap_or("the engine was stopped").to_string());
            }
        }
    };
    let printed = std::fs::read(&out_file).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let _ = std::fs::remove_file(&out_file);
    let engine = crate::engine::result_line(&printed).unwrap_or_else(|| json!({"ok": false, "errors": [format!("the engine printed no result ({status})")]}));
    if engine["ok"] != true {
        return Err(format!("the walk-through ended with an error: {}", engine["errors"]));
    }

    // what the owner pointed at, named
    let notes = read_notes(&notes_file);
    let mut report = json!({"out": dir.to_string_lossy(), "seconds_open": engine.pointer("/walk/seconds"), "load_s": engine["ready_ms"].as_f64().map(|ms| r2(ms / 1000.0)), "peak_memory_mb": crate::lowload::peak_mem_mb().round(), "notes": []});
    if !notes.is_empty() {
        eprintln!("naming the {} noted object(s)...", notes.len());
        let mut c = cook(recipe, scene_path, lookdev, true)?;
        let named = resolve_notes(&mut c.graph, &c.node, &notes)?;
        for n in &named {
            eprintln!("  {}: {}", n["asked"].as_str().unwrap_or(""), n["summary"].as_str().unwrap_or(""));
            // the click, boxed on the picture taken at that moment
            if let (Some(shot), Some(px)) = (n["shot"].as_str(), v3_of(&json!([n["pixel"][0], n["pixel"][1], 0]))) {
                if let Ok(bytes) = std::fs::read(shot) {
                    let mut img = pt_render::decode_png(&bytes)?;
                    mark(&mut img, px[0], px[1], 16.0, [0, 255, 255]);
                    let _ = std::fs::write(shot, pt_render::encode_png(&img)?);
                }
            }
        }
        report["notes"] = json!(named);
    }
    std::fs::write(dir.join("notes.json"), serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pt_core::column::AttrSet;
    use pt_core::geo::Instances;

    /// A 200 m square of flat ground at y = 0 named `terrain_t` and unit boxes (lowest point at y = 0 in the prototype) at `at`.
    fn world(at: &[V3]) -> Geometry {
        let mut g = Geometry::from_polys(vec![[-100.0, 0.0, -100.0], [100.0, 0.0, -100.0], [100.0, 0.0, 100.0], [-100.0, 0.0, 100.0]], &[vec![0, 3, 2, 1]]);
        g.assign_material("terrain_t", &[true]);
        let proto = Geometry::from_polys(
            vec![[-0.5, 0.0, -0.5], [0.5, 0.0, -0.5], [0.5, 0.0, 0.5], [-0.5, 0.0, 0.5], [-0.5, 1.0, -0.5], [0.5, 1.0, -0.5], [0.5, 1.0, 0.5], [-0.5, 1.0, 0.5]],
            &[vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7]],
        );
        let xf: Vec<[f32; 12]> = at.iter().map(|p| [1.0, 0.0, 0.0, p[0], 0.0, 1.0, 0.0, p[1], 0.0, 0.0, 1.0, p[2]]).collect();
        g.instances = Some(Instances { protos: vec![Arc::new(proto)], proto_id: Arc::new(vec![0; xf.len()]), xf: Arc::new(xf), attrs: AttrSet::default() });
        g
    }

    fn surfaces(g: &Geometry) -> (Surface, Surface) {
        (Surface::from_geometry(g, |m| m.starts_with("terrain")), Surface::from_geometry(g, |m| m.contains("water")))
    }

    const SCENE: &str = r#"{"cameras": [{"name": "a", "eye": {"at": [0, 0], "height": 1.7, "yaw": 0}, "lens": 28}, {"name": "b", "eye": {"at": [20, 0], "height": 1.7, "yaw": 90}, "lens": 28}]}"#;

    #[test]
    fn the_area_is_where_the_standing_cameras_are() {
        let scene: Value = serde_json::from_str(SCENE).unwrap();
        let f = focus_of(&scene, &world(&[]));
        assert!((f.x - 10.0).abs() < 1e-4 && f.z.abs() < 1e-4, "{f:?}");
        assert!((f.radius - 60.0).abs() < 1e-4, "ten metres of spread plus forty is under the floor of sixty: {f:?}");
        // without cameras: the box around the placed objects
        let f = focus_of(&json!({}), &world(&[[40.0, 0.0, 40.0], [60.0, 0.0, 80.0]]));
        assert!((f.x - 50.0).abs() < 1e-3 && (f.z - 60.0).abs() < 1e-3, "{f:?}");
    }

    #[test]
    fn stations_stand_clear_of_objects_and_apart_from_each_other() {
        // a wall of boxes through the middle: no station may stand inside or against it
        let boxes: Vec<V3> = (-30..=30).map(|i| [i as f32, 0.0, 0.0]).collect();
        let g = world(&boxes);
        let (t, w) = surfaces(&g);
        let world = World::new(&g);
        let f = Focus { x: 0.0, z: 0.0, radius: 60.0 };
        let st = stations(&t, &w, &world, &f, 6);
        assert_eq!(st.len(), 6);
        for s in &st {
            assert!((s[1] - EYE).abs() < 1e-4, "an eye stands {EYE} m over the ground");
            assert!(s[2].abs() > 1.5 || s[0].abs() > 31.5, "station {s:?} stands in the wall");
            assert!(f.holds(s[0], s[2], 0.7 + 1e-3));
        }
        for (i, a) in st.iter().enumerate() {
            for b in &st[i + 1..] {
                assert!((math::sq(a[0] - b[0]) + math::sq(a[2] - b[2])).sqrt() > 15.0, "{a:?} and {b:?} are the same place");
            }
        }
        assert_eq!(walk_views(&st, &f).len(), 24, "four views from each");
        assert_eq!(st, stations(&t, &w, &world, &f, 6), "the same world gives the same walk");
    }

    /// The class the survey exists for: an object the ground check flags must be put in front of the eye. Every floating box gets a
    /// pair of views; each has the box in the middle of the frame, in sight, and from two different sides.
    #[test]
    fn every_flagged_object_gets_two_views_with_it_in_the_middle_and_in_sight() {
        let g = world(&[[5.0, 0.6, 5.0], [-12.0, 0.0, 3.0], [20.0, 1.5, -8.0], [0.0, 0.0, 30.0]]);
        let (t, w) = surfaces(&g);
        let (findings, _) = check(&g, &t, &w);
        assert_eq!(findings.iter().filter(|f| f.kind == "float").count(), 2, "two of the four boxes hang in the air");
        let f = Focus { x: 0.0, z: 0.0, radius: 60.0 };
        let world = World::new(&g);
        let tg = targets(&g, &findings, &t, &f, 12);
        assert_eq!(tg.iter().map(|t| t.instance).collect::<Vec<_>>(), vec![2, 0], "the worst first: 1.5 m of air, then 0.6 m");
        let views = closeup_views(&tg, &world, &t, &w);
        assert_eq!(views.len(), 4);
        for v in &views {
            let tgt = v.target.as_ref().unwrap();
            let p = v.project(tgt.aim, 1280, 720).expect("the target is in the frame");
            assert!((p[0] - 640.0).abs() < 2.0 && (p[1] - 360.0).abs() < 2.0, "{}: the target lands at {p:?}", v.name);
            assert!(v.why.contains("sight clear"), "{}: {}", v.name, v.why);
            assert!(v.eye[1] >= 0.4 - 1e-4, "the eye is above the ground: {:?}", v.eye);
            // the ground under the object is in the picture, so the air between them is seen
            assert!(v.project([tgt.aim[0], 0.0, tgt.aim[2]], 1280, 720).is_some(), "{}: the ground under the object is out of frame", v.name);
            let d = len3(math::sub(tgt.aim, v.eye));
            assert!((1.9..=12.5).contains(&d), "{}: {d} m away", v.name);
        }
        for pair in views.chunks(2) {
            let side = |v: &View| math::atan2(v.eye[2] - v.target.as_ref().unwrap().aim[2], v.eye[0] - v.target.as_ref().unwrap().aim[0]).to_degrees();
            let apart = (side(&pair[0]) - side(&pair[1])).rem_euclid(360.0);
            assert!((89.0..=271.0).contains(&apart), "the two views of {} are from the same side ({apart} degrees apart)", pair[0].name);
            assert!(pair[0].eye[1] < pair[1].eye[1], "the first is the low view");
        }
        // a world where everything sits on the ground has nothing to look at closely
        let seated = world_of_seated();
        let (t2, w2) = surfaces(&seated);
        assert!(targets(&seated, &check(&seated, &t2, &w2).0, &t2, &f, 12).is_empty());
    }

    fn world_of_seated() -> Geometry {
        world(&[[5.0, 0.0, 5.0], [-12.0, 0.0, 3.0]])
    }

    #[test]
    fn a_view_blocked_by_a_wall_is_not_chosen_while_a_clear_side_exists() {
        // a floating box with a tall wall close on its +X side: no view may look through the wall or stand inside it
        let mut g = world(&[[0.0, 0.8, 0.0]]);
        let slab = Geometry::from_polys(
            vec![[-0.5, 0.0, -7.0], [0.5, 0.0, -7.0], [0.5, 0.0, 7.0], [-0.5, 0.0, 7.0], [-0.5, 9.0, -7.0], [0.5, 9.0, -7.0], [0.5, 9.0, 7.0], [-0.5, 9.0, 7.0]],
            &[vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7]],
        );
        let inst = g.instances.as_mut().unwrap();
        inst.protos.push(Arc::new(slab));
        inst.proto_id = Arc::new(vec![0, 1]);
        inst.xf = Arc::new(vec![inst.xf[0], [1.0, 0.0, 0.0, 1.6, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]]);
        let (t, w) = surfaces(&g);
        let (findings, _) = check(&g, &t, &w);
        let world = World::new(&g);
        let tg = targets(&g, &findings, &t, &Focus { x: 0.0, z: 0.0, radius: 60.0 }, 1);
        assert_eq!(tg[0].instance, 0, "{:?}", check(&g, &t, &w).0);
        let views = closeup_views(&tg, &world, &t, &w);
        assert_eq!(views.len(), 2);
        for v in &views {
            assert!(v.eye[0] < 1.0, "{} looks through the wall from {:?}", v.name, v.eye);
            assert!(v.why.contains("sight clear"), "{}", v.why);
        }
    }

    #[test]
    fn one_class_of_defect_does_not_fill_the_close_ups() {
        let many: Vec<V3> = (0..10).map(|i| [i as f32 * 3.0, 0.5 + i as f32 * 0.1, 0.0]).collect();
        let g = world(&many);
        let (t, w) = surfaces(&g);
        let tg = targets(&g, &check(&g, &t, &w).0, &t, &Focus { x: 15.0, z: 0.0, radius: 60.0 }, 12);
        assert_eq!(tg.len(), 3, "ten floating boxes of one prototype are one class: three examples are enough");
        // outside the area nothing is taken
        assert!(targets(&g, &check(&g, &t, &w).0, &t, &Focus { x: 500.0, z: 500.0, radius: 60.0 }, 12).is_empty());
    }

    #[test]
    fn every_view_is_a_camera_pick_can_use() {
        let scene: Value = serde_json::from_str(SCENE).unwrap();
        let g = world(&[[5.0, 0.6, 5.0]]);
        let (t, w) = surfaces(&g);
        let world = World::new(&g);
        let f = focus_of(&scene, &g);
        let views = plan_views(&g, &scene, &check(&g, &t, &w).0, &t, &w, &world, &f, 2, 4);
        let groups = |n: &str| views.iter().filter(|v| v.group == n).count();
        assert_eq!((groups("walk"), groups("above"), groups("camera"), groups("flaw")), (8, 5, 2, 2));
        let s = scene_with_views(&scene, &views, &t);
        let cams = cameras_of(&s);
        assert_eq!(cams.len(), views.len());
        for (c, v) in cams.iter().zip(&views) {
            // the same eye, heading and lens come back out of the scene file
            let eye_y = t.height(c.x, c.z).unwrap() + c.eye_height;
            assert!((eye_y - v.eye[1]).abs() < 1e-3 && (c.yaw_deg - v.yaw).abs() < 1e-3 && (c.pitch_deg - v.pitch).abs() < 1e-3 && (c.lens - v.lens).abs() < 1e-3, "{}", v.name);
        }
        // the plan view looks straight down and holds the whole area
        let plan = views.iter().find(|v| v.name == "above_plan").unwrap();
        assert!((plan.pitch + 90.0).abs() < 1e-3);
        for (dx, dz) in [(f.radius, 0.0), (-f.radius, 0.0), (0.0, f.radius), (0.0, -f.radius)] {
            assert!(plan.project([f.x + dx, 0.0, f.z + dz], 1280, 720).is_some(), "the rim of the area at ({dx}, {dz}) is outside the plan view");
        }
    }

    #[test]
    fn the_engine_plan_names_every_flagged_object_once_with_its_colour() {
        let g = world(&[[5.0, 0.6, 5.0], [-12.0, 0.0, 3.0], [8.0, -0.9, 8.0]]);
        let (t, w) = surfaces(&g);
        let (findings, _) = check(&g, &t, &w);
        let views = vec![View::looking_at("v".into(), "walk", String::new(), [0.0, 1.7, 0.0], [5.0, 1.0, 5.0], 28.0)];
        let shots = vec![(0usize, "lit", PathBuf::from("lit_v.png")), (0usize, "flags", PathBuf::from("flags_v.png"))];
        let plan = engine_plan(&g, &json!({"lighting": {"sun_azimuth": 232, "sun_elevation": 30}}), &findings, &shots, &views, (1280, 720));
        let flags = plan["flags"].as_array().unwrap();
        assert_eq!(flags.len(), 2, "the floating box and the buried one; the seated one is not flagged");
        let red = flags.iter().find(|f| f[1] == 5.0).unwrap();
        assert_eq!((red[0].as_u64(), red[4].as_f64(), red[5].as_f64()), (Some(0), Some(1.0), Some(0.0)), "floats are red: {red}");
        assert_eq!(plan["sun"]["azimuth"], 232.0);
        assert_eq!(plan["views"].as_array().unwrap().len(), 2);
        assert_eq!(plan["views"][1]["pass"], "flags");
        assert!(plan["kinds"]["0"].is_array() && plan["base_kinds"]["terrain_t"].is_array());
    }

    #[test]
    fn a_layered_ground_gets_the_mix_of_its_layers_colours() {
        let mut g = Geometry::from_polys(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]], &[vec![0, 1, 2, 3]]);
        g.assign_material("terrain_blend", &[true]);
        g.detail.insert("layers:terrain_blend".into(), json!(["ground_a", "ground_b"]));
        g.point.set("Wt0", Column::V4(Arc::new(vec![[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]])));
        let scene = json!({"materials": {"ground_a": {"use": "moss", "tint": [0.5, 0.5, 0.5]}, "ground_b": {"fallback": [0.8, 0.4, 0.0]}}});
        let lib = json!({"materials": {"moss": {"fallback": [0.2, 0.4, 0.2]}}});
        assert_eq!(tint_layered(&mut g, &scene, &lib), 3, "the point without any weight keeps its colour");
        let cd = g.point.get("Cd").and_then(Column::as_v3).unwrap().to_vec();
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k] / 0.8).abs() < 1e-5);
        assert!(close(cd[0], [0.1, 0.2, 0.1]) && close(cd[1], [0.8, 0.4, 0.0]) && close(cd[2], [0.45, 0.3, 0.05]), "{cd:?}");
        assert_eq!(cd[3], [1.0, 1.0, 1.0]);
        // a ground that was baked keeps its textures: nothing is tinted
        g.detail.insert("baked:terrain_blend".into(), json!({"albedo": "a.png"}));
        assert_eq!(tint_layered(&mut g, &scene, &lib), 0);
    }

    #[test]
    fn a_picture_shrinks_by_averaging_and_a_label_is_written_on_it() {
        let mut img = Image { w: 4, h: 2, rgb: vec![0; 24] };
        for x in 0..4 {
            for y in 0..2 {
                img.rgb[(y * 4 + x) * 3] = if x < 2 { 200 } else { 100 };
            }
        }
        let s = shrink(&img, 2, 1);
        assert_eq!((s.w, s.h, s.rgb[0], s.rgb[3]), (2, 1, 200, 100));
        let mut big = Image { w: 200, h: 40, rgb: vec![90; 200 * 40 * 3] };
        label(&mut big, 4, 4, "7 walk_1a", 2);
        let white = big.rgb.chunks(3).filter(|p| *p == [255, 255, 255]).count();
        let dark = big.rgb.chunks(3).filter(|p| *p == [16, 16, 16]).count();
        assert!(white > 100 && dark > white, "letters on a plate: {white} white, {dark} dark");
        // a label longer than the picture is cut, never written outside it
        let mut small = Image { w: 30, h: 12, rgb: vec![90; 30 * 12 * 3] };
        label(&mut small, 4, 4, "a very long label indeed", 2);
        assert_eq!(small.rgb.len(), 30 * 12 * 3);
    }

    #[test]
    fn kind_colours_differ_from_their_neighbours() {
        for i in 0..60 {
            let (a, b) = (kind_rgb(i), kind_rgb(i + 1));
            assert!((0..3).map(|k| (a[k] - b[k]).abs()).sum::<f32>() > 0.25, "kinds {i} and {} look alike: {a:?} {b:?}", i + 1);
            assert!(a.iter().all(|c| (0.0..=1.0).contains(c)));
        }
    }

    #[test]
    fn notes_are_named_by_casting_their_rays_into_the_world() {
        let code = "ground = set_material(grid(size=[100, 100], rows=4, cols=4), name=\"terrain_x\")\n\
                    spot = curve(points=[[0.0, 0.0, 10.0], [0.0, 0.0, 30.0]])\n\
                    rock = box(size=[2.0, 2.0, 2.0], center=[0, 1.0, 0])\n\
                    rocks = copy_to_points(rock, spot)\n\
                    world = merge(ground, rocks)\n\
                    out(world)\n";
        let mut g = Graph::new();
        g.provenance = true;
        let rep = pt_hcs::run(code, &mut g, true).unwrap_or_else(|d| panic!("{}", d[0]));
        // the owner clicked on the near rock, then on the ground beside it, then at the sky
        let notes = vec![
            json!({"n": 1, "what": "flag", "origin": [0.0, 1.7, 0.0], "dir": [0.0, 0.0, 1.0], "pixel": [800, 450], "shot": "note_001.png"}),
            json!({"n": 2, "what": "remove", "origin": [5.0, 1.7, 0.0], "dir": [0.0, -0.5, 1.0], "pixel": [10, 20], "shot": "note_002.png"}),
            json!({"n": 3, "what": "flag", "origin": [0.0, 1.7, 0.0], "dir": [0.0, 1.0, 0.0], "pixel": [1, 1], "shot": "note_003.png"}),
        ];
        let named = resolve_notes(&mut g, &rep.outputs[0], &notes).unwrap();
        assert_eq!(named.len(), 3);
        assert_eq!(named[0]["result"]["kind"], "instance");
        assert_eq!(named[0]["result"]["instance"], 0);
        assert_eq!(named[0]["asked"], "this looks wrong");
        assert!(named[0]["summary"].as_str().unwrap().contains("instance 0"), "{}", named[0]["summary"]);
        assert_eq!(named[1]["result"]["material"], "terrain_x");
        assert_eq!(named[1]["asked"], "remove this");
        assert!(named[2]["summary"].as_str().unwrap().contains("sky"));
        assert!(resolve_notes(&mut g, &rep.outputs[0], &[json!({"n": 1})]).unwrap_err().contains("no ray"));
    }
}
