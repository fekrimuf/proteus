//! Placement that keeps its identity.
//!
//! A scatter that draws N random points in order changes every point when N, the order or anything upstream changes, so a world
//! the owner approved is a different world after the next rebuild. Here a point belongs to a CELL of the world (a square of
//! `min_dist / 1.5` metres) and is decided by the cell and the seed alone: where it stands inside the cell, how it ranks against
//! its neighbours, how big it is, and its id (the cell's coordinates). Change the terrain's height, a mask, the density or what
//! happens elsewhere, and every point that survives is exactly where it was, with the same id. `apply_edits` then replays the
//! owner's decisions (remove this one, move that one, make it bigger, add one here) keyed by those ids.

use crate::refindex::surface_of;
use crate::{ps, sch};
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

/// The point attribute that carries a point's stable identity (the packed coordinates of its cell).
pub const ID: &str = "pid";

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("scatter_stable", "Blue-noise scatter whose points keep their identity. Points are decided by the cell of the world they stand in and the seed, never by a count or an order: change the terrain, a mask, the density or anything elsewhere and every surviving point stays exactly where it was, with the same `pid`. Points are placed on what is seen from above (the first surface under a ray from the sky), spaced at least `min_dist` apart on the map (horizontally: a change of height never decides which points stay). Writes `P`, `N` (up-facing normal), `pscale` and the integer attribute `pid` (the packed cell coordinates, unique per point). Copy nodes key their randomness on `pid`, so a rotation or a variant is also stable.", 1, D::None, G::Whole, vec![
            ps("min_dist", json!(1.0), "Minimum distance between points (m); the cell is min_dist / 1.5."),
            ps("seed", json!(1), "Seed."),
            ps("density", json!(1.0), "Share 0..1 of the possible points kept. Lowering it only removes points (by each point's own hash); the rest do not move."),
            ps("pscale_min", json!(1.0), "Smallest pscale (a per-point hash picks between min and max)."),
            ps("pscale_max", json!(1.0), "Largest pscale."),
        ]),
        sch("apply_edits", "Replays decisions about individual points or instances, keyed by their stable `pid`: [{\"id\": 123, \"op\": \"remove\"}, {\"id\": 123, \"op\": \"move\", \"by\": [dx, dy, dz]} (or \"to\": [x, y, z]), {\"id\": 123, \"op\": \"scale\", \"by\": 1.3}, {\"op\": \"add\", \"id\": 42, \"at\": [x, y, z], \"pscale\": 1.0} (points only)]. Edits whose id no longer exists are reported by `check` and ignored. Optional input 1: a surface; moved and added points are put back on it (a ray from above) when `snap` is 1. Put it AFTER the filters and BEFORE copy_to_points / copy_variants. Typical use: `apply_edits(points, ground, edits=load_json(\"edits/world.json\")[\"rocks\"])`.", 2, D::None, G::Whole, vec![
            ps("edits", json!([]), "List of edit objects (see the description)."),
            ps("snap", json!(1), "1 = moved and added points are placed on the surface of input 1 when it is connected."),
        ]),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> { inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected")) };
    Some(match kind {
        "scatter_stable" => inp(0).and_then(|g| scatter_stable(g, p)),
        "apply_edits" => inp(0).and_then(|g| apply_edits(g, inputs.get(1).and_then(|x| x.as_deref()), p)),
        _ => return None,
    })
}

fn mix(x: u64) -> u64 {
    let mut x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn cell_hash(seed: i64, cx: i32, cz: i32) -> u64 {
    mix(mix(seed as u64) ^ (((cx as u32 as u64) << 32) | cz as u32 as u64))
}

/// A float in [0, 1) from the top 24 bits.
fn unit(bits: u64) -> f32 {
    (bits >> 40) as f32 / 16_777_216.0
}

/// The identity of the point of cell (`cx`, `cz`): the packed coordinates, so ids are unique by construction.
fn pack(cx: i32, cz: i32) -> i32 {
    (((cx + 32768) as u32) << 16 | (cz + 32768) as u32) as i32
}

struct Cand {
    prio: u64,
    h: u64,
    cx: i32,
    cz: i32,
    pos: [f32; 3],
    n: [f32; 3],
}

fn scatter_stable(src: &Geometry, p: &Params) -> Res {
    let tree = surface_of(src).ok_or("scatter_stable: the input has no surface to place points on")?;
    let (lo, hi) = src.base_bounds().ok_or("scatter_stable: the input is empty")?;
    let md = p.f("min_dist").max(0.01);
    let c = md / 1.5;
    let seed = p.i("seed");
    let (cx0, cx1, cz0, cz1) = ((lo[0] / c).floor() as i32, (hi[0] / c).floor() as i32, (lo[2] / c).floor() as i32, (hi[2] / c).floor() as i32);
    if cx0 < -32768 || cz0 < -32768 || cx1 > 32767 || cz1 > 32767 {
        return Err(format!("scatter_stable: the surface is {:.0} x {:.0} m, too large for min_dist {md} (ids pack 65 536 cells per side): raise min_dist or place on parts of it", hi[0] - lo[0], hi[2] - lo[2]));
    }
    if (cx1 - cx0 + 1) as usize * (cz1 - cz0 + 1) as usize > 60_000_000 {
        return Err("scatter_stable: min_dist is too small for this surface (more than 60 million cells)".into());
    }
    let top = hi[1] + 1.0;
    // one candidate per cell: jittered inside the cell, ranked by its own hash, kept only where the sky sees the surface
    let cands: Vec<Cand> = (cz0..=cz1)
        .into_par_iter()
        .flat_map_iter(|cz| {
            let tree = tree.clone();
            (cx0..=cx1).filter_map(move |cx| {
                let h = cell_hash(seed, cx, cz);
                let (x, z) = ((cx as f32 + unit(h)) * c, (cz as f32 + unit(mix(h ^ 1))) * c);
                let hit = tree.ray([x, top, z], [0.0, -1.0, 0.0])?;
                Some(Cand { prio: mix(h ^ 2), h, cx, cz, pos: hit.pos, n: hit.n })
            })
        })
        .collect();
    // dart throwing in order of rank: a candidate is taken when nothing already taken is closer than min_dist
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_unstable_by(|&a, &b| cands[b].prio.cmp(&cands[a].prio).then(cands[a].cz.cmp(&cands[b].cz)).then(cands[a].cx.cmp(&cands[b].cx)));
    let key = |q: [f32; 3]| ((q[0] / md).floor() as i32, (q[2] / md).floor() as i32);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let mut taken: Vec<usize> = Vec::new();
    for &i in &order {
        let q = cands[i].pos;
        let k = key(q);
        let free = (-1..=1).all(|dx| {
            (-1..=1).all(|dz| {
                grid.get(&(k.0 + dx, k.1 + dz)).is_none_or(|l| {
                    l.iter().all(|&j| {
                        // spacing is measured on the map (x and z): a change of height never decides who stays
                        let d = [cands[j].pos[0] - q[0], cands[j].pos[2] - q[2]];
                        d[0] * d[0] + d[1] * d[1] >= md * md
                    })
                })
            })
        });
        if free {
            grid.entry(k).or_default().push(i);
            taken.push(i);
        }
    }
    // thinning after the throwing: a lower density only removes points
    let density = p.f("density").clamp(0.0, 1.0);
    taken.retain(|&i| unit(mix(cands[i].h ^ 3)) < density || density >= 1.0);
    taken.sort_unstable_by(|&a, &b| cands[a].cz.cmp(&cands[b].cz).then(cands[a].cx.cmp(&cands[b].cx)));
    let (smin, smax) = (p.f("pscale_min"), p.f("pscale_max"));
    let mut g = Geometry::from_points(taken.iter().map(|&i| cands[i].pos).collect());
    g.point.set("N", Column::from_v3(taken.iter().map(|&i| cands[i].n).collect()));
    g.point.set("pscale", Column::from_f32(taken.iter().map(|&i| smin + (smax - smin) * unit(mix(cands[i].h ^ 4))).collect()));
    g.point.set(ID, Column::from_i32(taken.iter().map(|&i| pack(cands[i].cx, cands[i].cz)).collect()));
    Ok(g)
}

// ---- edits ----

struct Edit {
    id: i32,
    op: String,
    to: Option<[f32; 3]>,
    by: Option<[f32; 3]>,
    factor: Option<f32>,
    at: Option<[f32; 3]>,
    pscale: f32,
}

fn v3(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    (a.len() == 3).then(|| [0, 1, 2].map(|i| a[i].as_f64().unwrap_or(0.0) as f32))
}

fn parse_edits(p: &Params) -> Result<Vec<Edit>, String> {
    let list = p.get("edits").and_then(Value::as_array).ok_or("apply_edits: `edits` must be a list of edit objects")?;
    list.iter()
        .enumerate()
        .map(|(i, e)| {
            let op = e.get("op").and_then(Value::as_str).ok_or_else(|| format!("apply_edits: edit {i} has no \"op\" (remove | move | scale | add)"))?.to_string();
            if !["remove", "move", "scale", "add"].contains(&op.as_str()) {
                return Err(format!("apply_edits: edit {i}: unknown op '{op}' (remove | move | scale | add)"));
            }
            // the recipe language hands whole numbers over as floats (-2143649776.0); an id is a whole number either way
            let id = e.get("id").and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).map(|v| v as i32);
            if id.is_none() && op != "add" {
                return Err(format!("apply_edits: edit {i} ({op}) needs an \"id\" (the pid that `pt pick` reports)"));
            }
            let by_num = e.get("by").and_then(Value::as_f64).map(|v| v as f32);
            let ed = Edit { id: id.unwrap_or(0), op: op.clone(), to: e.get("to").and_then(v3), by: e.get("by").and_then(v3), factor: by_num, at: e.get("at").and_then(v3), pscale: e.get("pscale").and_then(Value::as_f64).unwrap_or(1.0) as f32 };
            match op.as_str() {
                "move" if ed.to.is_none() && ed.by.is_none() => Err(format!("apply_edits: edit {i}: move needs \"by\": [dx, dy, dz] or \"to\": [x, y, z]")),
                "scale" if ed.factor.is_none() => Err(format!("apply_edits: edit {i}: scale needs \"by\": a number")),
                "add" if ed.at.is_none() => Err(format!("apply_edits: edit {i}: add needs \"at\": [x, y, z]")),
                _ => Ok(ed),
            }
        })
        .collect()
}

fn apply_edits(src: &Geometry, ground: Option<&Geometry>, p: &Params) -> Res {
    let edits = parse_edits(p)?;
    let tree = if p.i("snap") != 0 { ground.and_then(surface_of) } else { None };
    let seat = |q: [f32; 3]| -> [f32; 3] {
        match &tree {
            Some(t) => t.ray([q[0], q[1] + 1000.0, q[2]], [0.0, -1.0, 0.0]).map_or(q, |h| [q[0], h.pos[1], q[2]]),
            None => q,
        }
    };
    let mut by_id: HashMap<i32, Vec<&Edit>> = HashMap::new();
    for e in edits.iter().filter(|e| e.op != "add") {
        by_id.entry(e.id).or_default().push(e);
    }
    let mut seen: HashSet<i32> = HashSet::new();
    if let Some(inst) = &src.instances {
        // instances: remove, move, scale
        let Some(Column::I32(ids)) = inst.attrs.get(ID) else { return Err("apply_edits: the instances carry no `pid` (scatter them with scatter_stable)".into()) };
        let mut xf = (*inst.xf).clone();
        let mut keep: Vec<u32> = Vec::new();
        for (k, m) in xf.iter_mut().enumerate() {
            let list = by_id.get(&ids[k]);
            if list.is_some() {
                seen.insert(ids[k]);
            }
            if list.is_some_and(|l| l.iter().any(|e| e.op == "remove")) {
                continue;
            }
            for e in list.into_iter().flatten() {
                match e.op.as_str() {
                    "move" => {
                        let from = [m[3], m[7], m[11]];
                        let to = seat(e.to.unwrap_or_else(|| e.by.map_or(from, |b| [from[0] + b[0], from[1] + b[1], from[2] + b[2]])));
                        (m[3], m[7], m[11]) = (to[0], to[1], to[2]);
                    }
                    "scale" => {
                        let s = e.factor.unwrap_or(1.0);
                        for row in 0..3 {
                            for col in 0..3 {
                                m[row * 4 + col] *= s;
                            }
                        }
                    }
                    _ => {}
                }
            }
            keep.push(k as u32);
        }
        let mut ni = inst.clone();
        ni.proto_id = Arc::new(keep.iter().map(|&k| inst.proto_id[k as usize]).collect());
        ni.xf = Arc::new(keep.iter().map(|&k| xf[k as usize]).collect());
        ni.attrs = inst.attrs.gather(&keep);
        let mut g = src.clone();
        g.instances = Some(ni);
        report_missing(&by_id, &seen)?;
        return Ok(g);
    }
    // points: remove, move, scale, add
    let Some(Column::I32(ids)) = src.point.get(ID) else { return Err("apply_edits: the points carry no `pid` (scatter them with scatter_stable)".into()) };
    let n = src.n_points();
    let mut pos = src.p().to_vec();
    let mut scale: Vec<f32> = src.point.get("pscale").and_then(Column::as_f32).map_or(vec![1.0; n], <[f32]>::to_vec);
    let mut keep = vec![true; n];
    for k in 0..n {
        let Some(list) = by_id.get(&ids[k]) else { continue };
        seen.insert(ids[k]);
        for e in list {
            match e.op.as_str() {
                "remove" => keep[k] = false,
                "move" => {
                    let to = e.to.unwrap_or_else(|| e.by.map_or(pos[k], |b| [pos[k][0] + b[0], pos[k][1] + b[1], pos[k][2] + b[2]]));
                    pos[k] = seat(to);
                }
                "scale" => scale[k] *= e.factor.unwrap_or(1.0),
                _ => {}
            }
        }
    }
    let mut g = src.clone();
    g.p_mut().copy_from_slice(&pos);
    if g.point.get("pscale").is_some() || scale.iter().any(|s| *s != 1.0) {
        g.point.set("pscale", Column::from_f32(scale));
    }
    let mut g = g.filter(&keep, &[]);
    let adds: Vec<&Edit> = edits.iter().filter(|e| e.op == "add").collect();
    if !adds.is_empty() {
        let mut extra = Geometry::from_points(adds.iter().map(|e| seat(e.at.unwrap_or([0.0; 3]))).collect());
        extra.point.set("N", Column::from_v3(vec![[0.0, 1.0, 0.0]; adds.len()]));
        extra.point.set("pscale", Column::from_f32(adds.iter().map(|e| e.pscale).collect()));
        extra.point.set(ID, Column::from_i32(adds.iter().enumerate().map(|(i, e)| if e.id != 0 { e.id } else { i32::MIN + i as i32 }).collect()));
        g.append(&extra)?;
    }
    report_missing(&by_id, &seen)?;
    Ok(g)
}

/// Edits whose id is not there any more are not an error of the cook (the world moved on), but the agent should know.
fn report_missing(by_id: &HashMap<i32, Vec<&Edit>>, seen: &HashSet<i32>) -> Result<(), String> {
    let gone = by_id.keys().filter(|k| !seen.contains(k)).count();
    if gone > 0 {
        eprintln!("apply_edits: {gone} edit(s) name ids that no longer exist (the points they referred to are gone); they were ignored");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook;

    fn run(kind: &str, set: &[(&str, Value)], ins: &[&Geometry]) -> Geometry {
        let mut p = Params::new();
        for s in crate::schema(kind).unwrap().params {
            p.insert(s.name.into(), s.default);
        }
        for (k, v) in set {
            p.insert((*k).into(), v.clone());
        }
        let inputs: Vec<Option<Arc<Geometry>>> = ins.iter().map(|g| Some(Arc::new((*g).clone()))).collect();
        cook(kind, &p, &inputs).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    fn ground(size: f32, bump: f32) -> Geometry {
        let g = run("grid", &[("size", json!([size, size])), ("rows", json!(40)), ("cols", json!(40))], &[]);
        run("wrangle", &[("class", json!("point")), ("code", json!(format!("@P.y = 0.4 * sin(@P.x * 0.5) + {bump} * exp(-((@P.x - 6.0) * (@P.x - 6.0) + (@P.z - 6.0) * (@P.z - 6.0)) / 8.0);")))], &[&g])
    }

    fn by_id(g: &Geometry) -> HashMap<i32, [f32; 3]> {
        let Some(Column::I32(ids)) = g.point.get(ID) else { panic!("no pid") };
        ids.iter().zip(g.p()).map(|(i, p)| (*i, *p)).collect()
    }

    #[test]
    fn points_keep_their_spacing_ids_and_are_deterministic() {
        let g = ground(30.0, 0.0);
        let s = run("scatter_stable", &[("min_dist", json!(1.4)), ("seed", json!(7))], &[&g]);
        assert!(s.n_points() > 150, "{}", s.n_points());
        let p = s.p();
        for i in 0..p.len() {
            for j in i + 1..p.len() {
                let d = pt_core::math::sub(p[i], p[j]);
                assert!(d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= 1.4 * 1.4 - 1e-4, "points {i} and {j} are too close");
            }
        }
        assert_eq!(by_id(&s).len(), s.n_points(), "ids are unique");
        assert!(s.point.get("N").is_some() && s.point.get("pscale").is_some());
        let again = run("scatter_stable", &[("min_dist", json!(1.4)), ("seed", json!(7))], &[&g]);
        assert_eq!(s.content_hash(), again.content_hash());
        let other = run("scatter_stable", &[("min_dist", json!(1.4)), ("seed", json!(8))], &[&g]);
        assert_ne!(s.content_hash(), other.content_hash());
    }

    #[test]
    fn surviving_points_do_not_move_when_the_world_changes() {
        // the same patch of world twice: once flat, once with a 3 m bump in one corner and a hole cut elsewhere
        let flat = ground(30.0, 0.0);
        let changed = ground(30.0, 3.0);
        let a = run("scatter_stable", &[("min_dist", json!(1.2))], &[&flat]);
        let b = run("scatter_stable", &[("min_dist", json!(1.2))], &[&changed]);
        let (ma, mb) = (by_id(&a), by_id(&b));
        assert_eq!(ma.len(), mb.len(), "a height change moves no point sideways, so none appears or disappears");
        for (id, pa) in &ma {
            let pb = mb[id];
            assert!((pa[0] - pb[0]).abs() < 1e-5 && (pa[2] - pb[2]).abs() < 1e-5, "id {id} moved sideways: {pa:?} -> {pb:?}");
        }
        let lifted = ma.iter().filter(|(id, pa)| (mb[*id][1] - pa[1]).abs() > 0.5).count();
        assert!(lifted > 0, "the points on the bump follow the new height");
        // a hole in the surface removes the points over it and (almost) nobody else
        let keep = run("group_by_expression", &[("name", json!("g")), ("expression", json!("@P.x < 0.0 || @P.x > 6.0 || @P.z < 0.0 || @P.z > 6.0"))], &[&flat]);
        let holed = run("blast", &[("group", json!("g")), ("class", json!("point")), ("keep_selected", json!(1))], &[&keep]);
        let c = by_id(&run("scatter_stable", &[("min_dist", json!(1.2))], &[&holed]));
        let far_same = ma.iter().filter(|(_, p)| p[0] < -4.0 || p[0] > 11.0 || p[2] < -4.0 || p[2] > 11.0).filter(|(id, _)| c.get(*id).is_some_and(|q| (q[0] - ma[*id][0]).abs() < 1e-5)).count();
        let far_total = ma.iter().filter(|(_, p)| p[0] < -4.0 || p[0] > 11.0 || p[2] < -4.0 || p[2] > 11.0).count();
        assert!(far_same as f32 >= far_total as f32 * 0.98, "{far_same} of {far_total} points away from the hole kept their place");
    }

    #[test]
    fn lowering_the_density_only_removes_points() {
        let g = ground(30.0, 0.0);
        let full = by_id(&run("scatter_stable", &[("min_dist", json!(1.2)), ("density", json!(1.0))], &[&g]));
        let half = by_id(&run("scatter_stable", &[("min_dist", json!(1.2)), ("density", json!(0.5))], &[&g]));
        assert!(half.len() < full.len() * 6 / 10 && half.len() > full.len() * 4 / 10, "{} of {}", half.len(), full.len());
        assert!(half.iter().all(|(id, p)| full.get(id) == Some(p)), "every point of the thinner set is in the full set, unmoved");
    }

    #[test]
    fn copies_key_their_randomness_on_the_id_so_removing_a_point_changes_no_other() {
        let g = ground(24.0, 0.0);
        let pts = run("scatter_stable", &[("min_dist", json!(1.5)), ("pscale_min", json!(0.8)), ("pscale_max", json!(1.2))], &[&g]);
        let ids: Vec<i32> = match pts.point.get(ID) {
            Some(Column::I32(v)) => v.to_vec(),
            _ => panic!(),
        };
        let a = run("box", &[], &[]);
        let b = run("sphere", &[], &[]);
        let pose = |g: &Geometry| -> HashMap<i32, ([f32; 12], u32)> {
            let inst = g.instances.as_ref().unwrap();
            let Some(Column::I32(pid)) = inst.attrs.get(ID) else { panic!("instances lost pid") };
            pid.iter().enumerate().map(|(k, id)| (*id, (inst.xf[k], inst.proto_id[k]))).collect()
        };
        let before = pose(&run("copy_variants", &[("rot_y_random", json!(360.0)), ("seed", json!(3))], &[&pts, &a, &b]));
        let edited = run("apply_edits", &[("edits", json!([{"id": ids[3], "op": "remove"}, {"id": ids[10], "op": "remove"}]))], &[&pts]);
        assert_eq!(edited.n_points(), pts.n_points() - 2);
        let after = pose(&run("copy_variants", &[("rot_y_random", json!(360.0)), ("seed", json!(3))], &[&edited, &a, &b]));
        assert_eq!(after.len(), before.len() - 2);
        assert!(after.iter().all(|(id, v)| before.get(id) == Some(v)), "every remaining object keeps its turn, size and variant");
    }

    #[test]
    fn edits_remove_move_scale_and_add_by_id_and_say_what_is_wrong() {
        let g = ground(24.0, 0.0);
        let pts = run("scatter_stable", &[("min_dist", json!(2.0))], &[&g]);
        let ids: Vec<i32> = match pts.point.get(ID) {
            Some(Column::I32(v)) => v.to_vec(),
            _ => panic!(),
        };
        let edits = json!([
            {"id": ids[0], "op": "remove"},
            {"id": ids[1], "op": "move", "by": [0.5, 0.0, 0.0]},
            {"id": ids[2], "op": "scale", "by": 2.0},
            {"op": "add", "id": 777, "at": [1.0, 0.0, 1.0], "pscale": 1.5},
            {"id": 424242, "op": "remove"},
        ]);
        let out = run("apply_edits", &[("edits", edits.clone())], &[&pts, &g]);
        let (before, after) = (by_id(&pts), by_id(&out));
        assert!(!after.contains_key(&ids[0]));
        assert!((after[&ids[1]][0] - before[&ids[1]][0] - 0.5).abs() < 1e-4, "moved 0.5 m east");
        // a moved point is seated on the ground again (the ground rises and falls with sin(0.5 x))
        let want_y = 0.4 * (after[&ids[1]][0] * 0.5).sin();
        assert!((after[&ids[1]][1] - want_y).abs() < 0.05, "seated on the ground: {} vs {want_y}", after[&ids[1]][1]);
        assert!(after.contains_key(&777));
        let ps = out.point.get("pscale").unwrap().as_f32().unwrap();
        let ix = match out.point.get(ID) { Some(Column::I32(v)) => v.iter().position(|i| *i == ids[2]).unwrap(), _ => panic!() };
        assert!((ps[ix] - 2.0 * pts.point.get("pscale").unwrap().as_f32().unwrap()[2]).abs() < 1e-5);
        assert_eq!(out.n_points(), pts.n_points() - 1 + 1);
        // instances: the same edits by pid
        let inst = run("copy_to_points", &[], &[&run("box", &[], &[]), &pts]);
        let io = run("apply_edits", &[("edits", json!([{"id": ids[0], "op": "remove"}, {"id": ids[2], "op": "scale", "by": 2.0}]))], &[&inst]);
        assert_eq!(io.n_instances(), pts.n_points() - 1);
        // wrong input says what to do
        let mut p = Params::new();
        p.insert("edits".into(), json!([{"op": "remove"}]));
        p.insert("snap".into(), json!(1));
        let err = cook("apply_edits", &p, &[Some(Arc::new(pts.clone())), None]).unwrap_err();
        assert!(err.contains("needs an \"id\""), "{err}");
        p.insert("edits".into(), json!([{"id": 1, "op": "teleport"}]));
        assert!(cook("apply_edits", &p, &[Some(Arc::new(pts)), None]).unwrap_err().contains("unknown op"));
        let plain = Geometry::from_points(vec![[0.0; 3]]);
        p.insert("edits".into(), json!([]));
        assert!(cook("apply_edits", &p, &[Some(Arc::new(plain)), None]).unwrap_err().contains("scatter_stable"));
    }
}
