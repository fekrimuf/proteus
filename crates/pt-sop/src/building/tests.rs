//! Checks on the class of defect, run over several plans (a cottage, a row of three rooms with a chimney, a four-room grid, one room),
//! not over one house.

use super::*;

fn cook_with(set: &[(&str, Value)]) -> Res {
    let mut p = Params::new();
    for s in schemas().into_iter().flat_map(|s| s.params) {
        p.insert(s.name.into(), s.default);
    }
    for (k, v) in set {
        p.insert((*k).into(), v.clone());
    }
    build(&p)
}

type Plan = Vec<(&'static str, Value)>;

/// A two-room cottage the size of the creek hut: a front room with the door and a back room, a door between them.
fn cottage(extra: &[(&'static str, Value)]) -> Plan {
    let mut v: Plan = vec![
        ("cuts_z", json!([0.55])),
        ("door", json!([0, 0.32, 1.0, 1.95])),
        ("inner_doors", json!([[0, 1, 0.5, 0.9, 2.0]])),
        ("windows", json!([[0, 0.74, 0.8, 0.7, 1.0], [1, 0.22, 0.8, 0.8, 1.0], [3, 0.22, 0.8, 0.8, 1.0], [3, 0.82, 0.8, 0.8, 1.0], [2, 0.5, 0.8, 0.8, 1.0]])),
    ];
    for (k, x) in extra {
        v.retain(|(n, _)| n != k);
        v.push((k, x.clone()));
    }
    v
}

fn shell_of(set: &[(&str, Value)]) -> Geometry {
    cook_with(set).unwrap()
}

fn mode_of(set: &[(&str, Value)], mode: &'static str) -> Geometry {
    let mut s = set.to_vec();
    s.push(("mode", json!(mode)));
    cook_with(&s).unwrap()
}

fn attr(g: &Geometry, k: &str) -> Vec<f32> {
    g.prim.get(k).and_then(Column::as_f32).unwrap().to_vec()
}

/// Every hit of a ray with the triangles of the primitives `keep` accepts, nearest first: (distance, primitive).
fn ray(g: &Geometry, o: V3, d: V3, keep: &dyn Fn(usize) -> bool) -> Vec<(f32, usize)> {
    let mut hits = Vec::new();
    for i in (0..g.n_prims()).filter(|&i| keep(i)) {
        let pr = g.prim_points(i);
        for k in 1..pr.len() - 1 {
            let (a, b, c) = (g.p()[pr[0] as usize], g.p()[pr[k] as usize], g.p()[pr[k + 1] as usize]);
            let (e1, e2) = (math::sub(b, a), math::sub(c, a));
            let pv = math::cross(d, e2);
            let det = dot(e1, pv);
            if det.abs() < 1e-9 {
                continue;
            }
            let tv = math::sub(o, a);
            let u = dot(tv, pv) / det;
            let qv = math::cross(tv, e1);
            let v = dot(d, qv) / det;
            let t = dot(e2, qv) / det;
            if u >= -1e-6 && v >= -1e-6 && u + v <= 1.0 + 1e-6 && t > 0.0 {
                hits.push((t, i));
            }
        }
    }
    // a ray through the diagonal of a quad meets both its triangles: count the primitive once
    hits.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.total_cmp(&b.0)));
    hits.dedup_by_key(|h| h.1);
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits
}

fn is_part(g: &Geometry, part: f32) -> impl Fn(usize) -> bool {
    let parts = attr(g, "part");
    move |i| parts[i] == part
}

fn centroid(g: &Geometry, i: usize) -> V3 {
    let pr = g.prim_points(i);
    let mut c = [0.0f32; 3];
    for &k in pr {
        for (a, x) in c.iter_mut().zip(g.p()[k as usize]) {
            *a += x / pr.len() as f32;
        }
    }
    c
}

fn face_normal(g: &Geometry, i: usize) -> V3 {
    let pr = g.prim_points(i);
    let p = g.p();
    math::normalize(math::cross(math::sub(p[pr[1] as usize], p[pr[0] as usize]), math::sub(p[pr[2] as usize], p[pr[0] as usize])))
}

/// (centre, direction, width, kind, from, to) of every opening, read from the `openings` output.
fn read_openings(set: &[(&str, Value)]) -> Vec<(V3, V3, f32, f32, i32, i32)> {
    let g = mode_of(set, "openings");
    let (w, k, f, t) = (attr(&g, "width"), attr(&g, "kind"), attr(&g, "room_from"), attr(&g, "room_to"));
    (0..g.n_prims())
        .map(|i| {
            let (a, b) = (g.p()[2 * i], g.p()[2 * i + 1]);
            (a, math::sub(b, a), w[i], k[i], f[i] as i32, t[i] as i32)
        })
        .collect()
}

type Rect = ([f32; 2], [f32; 2]);

fn room_rects(set: &[(&str, Value)]) -> Vec<Rect> {
    let g = mode_of(set, "rooms");
    (0..g.n_prims())
        .map(|i| {
            let pr = g.prim_points(i);
            let xs: Vec<f32> = pr.iter().map(|&k| g.p()[k as usize][0]).collect();
            let zs: Vec<f32> = pr.iter().map(|&k| g.p()[k as usize][2]).collect();
            let (lo, hi) = (|v: &[f32]| v.iter().cloned().fold(f32::MAX, f32::min), |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max));
            ([lo(&xs), hi(&xs)], [lo(&zs), hi(&zs)])
        })
        .collect()
}

fn inside(r: &Rect, p: V3) -> bool {
    p[0] > r.0[0] && p[0] < r.0[1] && p[2] > r.1[0] && p[2] < r.1[1]
}

fn plans() -> Vec<Plan> {
    vec![
        cottage(&[]),
        vec![
            ("depth", json!(5.6)),
            ("cuts_z", json!([0.4, 0.7])),
            ("door", json!([0, 0.3, 1.0, 1.95])),
            ("inner_doors", json!([[0, 1, 0.3, 0.9, 2.0], [1, 2, 0.7, 0.9, 2.0]])),
            ("windows", json!([[0, 0.75, 0.8, 0.7, 1.0], [1, 0.2, 0.8, 0.8, 1.0], [3, 0.85, 0.8, 0.8, 1.0], [2, 0.7, 0.8, 0.8, 1.0]])),
            ("chimney", json!([2, 0.25, 0.9, 0.5, 0.8])),
        ],
        vec![
            ("width", json!(8.0)),
            ("depth", json!(6.0)),
            ("cuts_x", json!([0.5])),
            ("cuts_z", json!([0.5])),
            ("door", json!([0, 0.2, 1.0, 2.0])),
            ("inner_doors", json!([[0, 1, 0.5, 0.9, 2.0], [0, 2, 0.5, 0.9, 2.0], [2, 3, 0.5, 0.9, 2.0]])),
            ("windows", json!([[0, 0.7, 0.9, 0.9, 1.0], [1, 0.3, 0.8, 0.9, 1.0], [1, 0.8, 0.8, 0.9, 1.0], [2, 0.3, 0.8, 0.9, 1.0], [2, 0.8, 0.8, 0.9, 1.0], [3, 0.8, 0.8, 0.9, 1.0]])),
        ],
        vec![("door", json!([0, 0.32, 1.0, 1.95])), ("windows", json!([[0, 0.74, 0.8, 0.7, 1.1]]))],
    ]
}

#[test]
fn every_door_and_window_is_a_real_hole_and_the_wall_stands_beside_it() {
    for plan in plans() {
        let g = shell_of(&plan);
        let walls = is_part(&g, PART_WALL);
        let rects = room_rects(&plan);
        let opens = read_openings(&plan);
        assert!(opens.len() >= 2);
        for (n, (c, dir, width, kind, from, to)) in opens.iter().enumerate() {
            let thick = if *kind == 1.0 { 0.12 } else { 0.3 };
            // through the centre: from the side the air is on to the room behind, no wall on the way
            let (start, step, room) = if *kind == 1.0 { ([c[0] - dir[0] * 0.8, c[1], c[2] - dir[2] * 0.8], *dir, *to) } else { ([c[0] + dir[0] * 0.8, c[1], c[2] + dir[2] * 0.8], neg(*dir), *from) };
            let len = 0.8 + thick * 0.5 + 0.4;
            let hits: Vec<_> = ray(&g, start, step, &walls).into_iter().filter(|h| h.0 < len).collect();
            assert!(hits.is_empty(), "opening {n} (kind {kind}) is closed: the centre ray hits a wall at {hits:?}");
            let end = [start[0] + step[0] * len, 0.0, start[2] + step[2] * len];
            assert!(inside(&rects[room as usize], end), "opening {n}: the ray ends at {end:?}, outside room {room}");
            // one wall-thickness beside the opening there is wall
            let lat = [-dir[2], 0.0, dir[0]];
            for s in [-1.0f32, 1.0] {
                let off = s * (width * 0.5 + thick);
                let o = [start[0] + lat[0] * off, start[1], start[2] + lat[2] * off];
                let hit = ray(&g, o, step, &walls);
                assert!(hit.first().is_some_and(|h| h.0 < len), "opening {n}: no wall one thickness beside it (side {s}): {hit:?}");
            }
        }
    }
}

#[test]
fn walls_have_the_asked_thickness_on_every_face() {
    for plan in plans() {
        let g = shell_of(&plan);
        let (room, part, open) = (attr(&g, "room"), attr(&g, "part"), attr(&g, "opening"));
        let walls = is_part(&g, PART_WALL);
        let mut checked = 0;
        for i in 0..g.n_prims() {
            let n = face_normal(&g, i);
            // the faces of the walls that look into a room, not the reveals inside an opening
            if part[i] != PART_WALL || room[i] < 0.0 || open[i] >= 0.0 || n[1].abs() > 0.01 {
                continue;
            }
            let c = centroid(&g, i);
            let o = [c[0] + n[0] * 0.02, c[1], c[2] + n[2] * 0.02];
            let hits = ray(&g, o, neg(n), &walls);
            let through = hits.get(1).map_or(-1.0, |h| h.0 - 0.02);
            assert!(hits.first().is_some_and(|h| (h.0 - 0.02).abs() < 1e-3), "face {i} is not met first: {hits:?}");
            assert!((through - 0.3).abs() < 2e-3 || (through - 0.12).abs() < 2e-3, "face {i} (room {}, centre {c:?}, normal {n:?}) has a wall {through} m thick: {hits:?}", room[i]);
            checked += 1;
        }
        assert!(checked >= 8, "only {checked} faces were measured");
    }
}

#[test]
fn every_room_is_reachable_through_doors_a_person_fits() {
    for plan in plans() {
        let opens = read_openings(&plan);
        let n_rooms = room_rects(&plan).len();
        let mut seen = vec![false; n_rooms];
        let mut stack = vec![opens[0].4];
        while let Some(r) = stack.pop() {
            if std::mem::replace(&mut seen[r as usize], true) {
                continue;
            }
            stack.extend(opens.iter().filter(|o| o.3 == 1.0 && o.4 == r).map(|o| o.5));
            stack.extend(opens.iter().filter(|o| o.3 == 1.0 && o.5 == r).map(|o| o.4));
        }
        assert!(seen.iter().all(|s| *s), "some room cannot be reached: {seen:?}");
        let g = mode_of(&plan, "rooms");
        for o in g.detail["building"]["openings"].as_array().unwrap().iter().filter(|o| o["kind"] != "window") {
            assert!(o["width"].as_f64().unwrap() >= 0.8 && o["height"].as_f64().unwrap() >= 1.9, "a door too small to walk through: {o}");
        }
    }
}

#[test]
fn a_plan_nobody_can_walk_is_refused_with_the_reason() {
    let e = cook_with(&cottage(&[("inner_doors", json!([]))])).unwrap_err();
    assert!(e.contains("room 1 cannot be reached") && e.contains("inner door"), "{e}");
    assert!(cook_with(&cottage(&[("inner_doors", json!([[0, 1, 0.5, 0.7, 2.0]]))])).unwrap_err().contains("at least 0.8 x 1.9"));
    assert!(cook_with(&cottage(&[("door", json!([0, 0.32, 1.0, 1.7]))])).unwrap_err().contains("at least 0.8 x 1.9"));
    let far = vec![("door", json!([0, 0.2, 1.0, 1.95])), ("cuts_x", json!([0.5])), ("cuts_z", json!([0.5])), ("inner_doors", json!([[0, 3, 0.5, 0.9, 2.0]]))];
    let e = cook_with(&far).unwrap_err();
    assert!(e.contains("do not share a wall"), "{e}");
    assert!(cook_with(&[("door", json!([]))]).unwrap_err().contains("no entrance"));
}

#[test]
fn a_window_belongs_to_exactly_one_room() {
    for plan in plans() {
        let rects = room_rects(&plan);
        for (n, (c, dir, width, kind, from, to)) in read_openings(&plan).iter().enumerate().filter(|(_, o)| o.3 == 2.0) {
            assert_eq!((*to, *kind), (-1, 2.0));
            let lat = [-dir[2], 0.0, dir[0]];
            // both edges of the window, pushed a little into the house, are inside the room it names and in no other
            for s in [-1.0f32, 1.0] {
                let q = [c[0] + lat[0] * s * width * 0.5 - dir[0] * 0.4, 0.0, c[2] + lat[2] * s * width * 0.5 - dir[2] * 0.4];
                let owners: Vec<usize> = (0..rects.len()).filter(|&r| inside(&rects[r], q)).collect();
                assert_eq!(owners, vec![*from as usize], "window {n} edge {q:?} is in rooms {owners:?}, expected only {from}");
            }
        }
    }
    // across a partition, and in a corner: refused
    let e = cook_with(&cottage(&[("windows", json!([[1, 0.45, 1.2, 0.8, 1.0]]))])).unwrap_err();
    assert!(e.contains("does not fit inside one room"), "{e}");
    assert!(cook_with(&cottage(&[("windows", json!([[0, 0.02, 0.4, 0.8, 1.0]]))])).is_err());
}

/// Wall-top points that no roof slab covers: the gap the roof must not leave.
fn roof_gaps(g: &Geometry) -> usize {
    let (part, open) = (attr(g, "part"), attr(g, "opening"));
    let roof = is_part(g, PART_ROOF);
    let mut gaps = 0;
    for i in 0..g.n_prims() {
        if part[i] != PART_WALL || open[i] >= 0.0 || face_normal(g, i)[1] < 0.5 {
            continue;
        }
        let c = centroid(g, i);
        for &k in g.prim_points(i) {
            let q = g.p()[k as usize];
            // a point a little in from the corner, a little above the wall top; inside a slab a ray along Z crosses its end once
            let o = [c[0] + (q[0] - c[0]) * 0.9, c[1] + (q[1] - c[1]) * 0.9 + 1e-3, c[2] + (q[2] - c[2]) * 0.9];
            if ray(g, o, [0.0, 0.0, 1.0], &roof).len() % 2 == 0 {
                gaps += 1;
            }
        }
    }
    gaps
}

#[test]
fn the_roof_closes_the_building() {
    for plan in plans() {
        assert_eq!(roof_gaps(&shell_of(&plan)), 0);
    }
    // the check sees a gap: lift the roof 0.1 m
    let mut g = shell_of(&cottage(&[]));
    let part = attr(&g, "part");
    let mut lifted = vec![false; g.n_points()];
    for i in (0..g.n_prims()).filter(|&i| part[i] == PART_ROOF) {
        for &k in g.prim_points(i) {
            lifted[k as usize] = true;
        }
    }
    for (q, up) in g.p_mut().iter_mut().zip(lifted) {
        if up {
            q[1] += 0.1;
        }
    }
    assert!(roof_gaps(&g) > 10, "a lifted roof must be reported");
}

#[test]
fn rooms_and_openings_are_carried_on_the_geometry() {
    let g = shell_of(&cottage(&[]));
    let (room, part) = (attr(&g, "room"), attr(&g, "part"));
    for code in [PART_WALL, PART_FLOOR, PART_ROOF, PART_DOOR, PART_WINDOW, PART_FRAME, PART_FOOTING, PART_TIMBER] {
        assert!(part.contains(&code), "no primitive with part {code}");
    }
    let floors: Vec<f32> = (0..g.n_prims()).filter(|&i| part[i] == PART_FLOOR).map(|i| room[i]).collect();
    assert_eq!(floors, vec![0.0, 1.0]);
    assert_eq!(g.material_names(), vec!["hut_dark", "hut_door", "hut_roof", "hut_stone", "hut_wall", "hut_wood"]);
    let info = &g.detail["building"];
    assert_eq!(info["rooms"].as_array().unwrap().len(), 2);
    assert_eq!(info["openings"].as_array().unwrap().len(), 7);
    let open = attr(&g, "opening");
    for id in 0..7 {
        assert!(open.contains(&(id as f32)), "nothing belongs to opening {id}");
    }
}

#[test]
fn the_door_swings_into_the_room_by_the_angle_asked() {
    let leaf_min_z = |deg: f32| {
        let g = shell_of(&cottage(&[("open", json!(deg))]));
        let (part, open) = (attr(&g, "part"), attr(&g, "opening"));
        let mut lo = f32::MAX;
        for i in (0..g.n_prims()).filter(|&i| part[i] == PART_DOOR && open[i] == 0.0) {
            for &k in g.prim_points(i) {
                lo = lo.min(g.p()[k as usize][2]);
            }
        }
        lo
    };
    let wall_inner = 2.3 - 0.3;
    assert!(leaf_min_z(0.0) > wall_inner - 0.1, "a closed door sits in the wall");
    assert!(leaf_min_z(90.0) < wall_inner - 0.8, "an open door stands out into the room");
}

#[test]
fn the_same_house_comes_out_the_same_every_time() {
    for plan in plans() {
        assert_eq!(shell_of(&plan).content_hash(), shell_of(&plan).content_hash());
    }
    assert_ne!(shell_of(&cottage(&[])).content_hash(), shell_of(&cottage(&[("open", json!(30.0))])).content_hash());
}

#[test]
fn the_openings_and_rooms_outputs_say_where_things_are() {
    let o = mode_of(&cottage(&[]), "openings");
    let (c, tip) = (o.p()[0], o.p()[1]);
    assert!((c[2] - (2.3 - 0.15)).abs() < 1e-4 && (tip[2] - c[2] - 1.0).abs() < 1e-4, "the entrance looks out of the front: {c:?} {tip:?}");
    let r = mode_of(&cottage(&[]), "rooms");
    assert_eq!(r.n_prims(), 2);
    assert!(attr(&r, "area").iter().all(|a| *a > 4.0));
}
