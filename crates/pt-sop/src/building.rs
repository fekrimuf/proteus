//! A building shell made from a floor plan, inside out: rooms first, then the walls round them, then the openings in the walls.
//!
//! The plan is a rectangle split by full-length partitions into a grid of rooms (`cuts_x`, `cuts_z`). Every wall is built as a
//! thick panel whose faces are cut around its openings (no boolean): the panel is split into columns at every edge of an opening,
//! a room or the gable apex, and each column becomes a few closed pieces (a full height piece, or the pieces below and above a
//! hole) with the jamb, head and sill faces of the hole added. Everything is plain quads with exact numbers, so the cook is
//! deterministic and the same on every platform (`pt_core::math` only).
//!
//! What a later stage reads back (see `docs/WORLD_GUIDE.md`, "A house from a plan"):
//! * the shell carries the primitive attributes `room` (the room whose air the face touches, -1 for any other face), `part`
//!   (a code, see `PART_CODES`) and `opening` (the opening a frame, leaf, pane or reveal belongs to, -1 otherwise), plus the detail
//!   record `building` with the rooms and openings in the node's own frame;
//! * `mode="openings"` gives one 2-point line per opening (from the centre of the opening to one metre along its direction) and
//!   `mode="rooms"` gives the floor rectangle of every room as a polygon, both carrying their numbers as primitive attributes. Being
//!   plain lines and polygons they follow any `transform` of the shell, so the position and direction stay right after the building
//!   is turned and placed.

use crate::{ps, sch};
use pt_core::math;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::{json, Value};

type Res = Result<Geometry, String>;
type V3 = [f32; 3];

const PART_WALL: f32 = 0.0;
const PART_FLOOR: f32 = 1.0;
const PART_ROOF: f32 = 2.0;
const PART_DOOR: f32 = 3.0;
const PART_WINDOW: f32 = 4.0;
const PART_FRAME: f32 = 5.0;
const PART_FOOTING: f32 = 6.0;
const PART_TIMBER: f32 = 7.0;
const PART_CHIMNEY: f32 = 8.0;
const PART_STEP: f32 = 9.0;
/// Entrance steps: no rise above this (m), treads at least this deep (m), and none for a floor lower than the rise.
const MAX_RISE: f32 = 0.2;
const TREAD: f32 = 0.3;

/// The meaning of the `part` attribute, also written into the `building` detail record.
const PART_CODES: [(&str, f32); 10] = [
    ("step", PART_STEP),
    ("wall", PART_WALL),
    ("floor", PART_FLOOR),
    ("roof", PART_ROOF),
    ("door", PART_DOOR),
    ("window", PART_WINDOW),
    ("frame", PART_FRAME),
    ("footing", PART_FOOTING),
    ("timber", PART_TIMBER),
    ("chimney", PART_CHIMNEY),
];

/// Smallest clear door: a person with a little head room.
const MIN_DOOR_W: f32 = 0.8;
const MIN_DOOR_H: f32 = 1.9;
/// Width of the trim round an opening, and how far it stands proud of the wall face.
const TRIM_W: f32 = 0.09;
const TRIM_OUT: f32 = 0.03;
/// An opening stays this far from the end of its room's stretch of wall, and two openings this far apart.
const MARGIN: f32 = 0.12;
const GAP: f32 = 0.3;
/// A room is at least this wide and deep (m).
const MIN_ROOM: f32 = 1.2;
/// Marks a stretch of a wall face that is hidden by another wall (no face is made there).
const HIDDEN: i32 = -2;

const WALL_NAMES: [&str; 4] = ["front (+Z)", "right (+X)", "back (-Z)", "left (-X)"];

pub fn schemas() -> Vec<NodeSchema> {
    vec![sch(
        "building",
        "A building SHELL from a floor plan, built inside out: a rectangular footprint split into rooms, outside walls and partitions with real thickness and real openings (no boolean: the faces are built around the holes), a floor per room, a stone footing, a gable roof (ridge along Z) with thickness and overhang, door frames with a door leaf that swings by `open`, window frames with a pane, corner posts, wall plates and an optional chimney. Plan view: the entrance wall is the FRONT (+Z); rooms are numbered row by row from the front-left (id = row * columns + column; columns count from -X, rows from the front towards the back). Walls are numbered 0 front (+Z), 1 right (+X), 2 back (-Z), 3 left (-X); a position along a wall is a fraction 0..1 measured left to right as seen from OUTSIDE. Output (mode 'shell'): primitive attributes `room` (the room whose air the face touches, -1 otherwise), `part` (0 wall, 1 floor, 2 roof, 3 door, 4 window, 5 frame, 6 footing, 7 timber, 8 chimney, 9 entrance steps) and `opening` (the opening a frame, leaf, pane or reveal belongs to, -1 otherwise), materials hut_stone, hut_wall, hut_wood, hut_roof and hut_door, and the detail record `building` (rooms and openings in the node's own frame). mode 'openings' is one 2-point line per opening (centre, then one metre along the direction a person walks from room_from to room_to; outwards for the entrance and the windows) with primitive attributes opening, kind (0 entrance, 1 inner door, 2 window), wall, room_from, room_to, width, height, sill; mode 'rooms' is the floor rectangle of every room with attributes room, row, col, area, height. Lines and polygons follow any transform of the shell. A plan that cannot be walked (a room with no door to it, a door under 0.8 x 1.9 m, a window across a partition) is refused with the reason.",
        0,
        D::None,
        G::Whole,
        vec![
            ps("width", json!(6.2), "Outside width along X (m)."),
            ps("depth", json!(4.6), "Outside depth along Z (m)."),
            ps("height", json!(2.5), "Wall height from the floor to the eaves (m)."),
            ps("wall", json!(0.3), "Outside wall thickness (m)."),
            ps("partition", json!(0.12), "Partition thickness (m)."),
            ps("floor_y", json!(0.4), "Height of the floor surface above the node's origin (m). The footing runs from here down."),
            ps("floor_thickness", json!(0.1), "Floor slab thickness; the walls stand on the footing this far below the floor surface (m)."),
            ps("footing", json!(1.6), "Height of the stone footing under the walls, from the floor surface down (m); it sticks out 0.2 m round the walls so the house sits on a slope."),
            ps("cuts_x", json!([]), "Partitions across the width, as fractions 0..1 of the inside width from the -X wall, ascending (each makes a wall running along Z). [0.5] gives two rooms side by side."),
            ps("cuts_z", json!([]), "Partitions across the depth, as fractions 0..1 of the inside depth from the front wall, ascending (each makes a wall running along X). [0.6] gives a front room and a back room. At most 3 cuts per axis."),
            ps("door", json!([0.0, 0.5, 1.0, 1.95]), "The entrance: [wall 0-3, position 0..1 along the wall, width, height] (m). At least 0.8 x 1.9."),
            ps("inner_doors", json!([]), "Doors between rooms, a list of [room a, room b, position 0..1, width, height]. The rooms must touch; the position runs along the shared wall from the left (-X) for a wall along X, from the front for a wall along Z."),
            ps("windows", json!([]), "Windows, a list of [wall 0-3, position 0..1 along the wall, width, height, sill height above the floor] (m). Each must lie wholly inside one room's stretch of the wall."),
            ps("pitch", json!(48.0), "Roof pitch (degrees)."),
            ps("overhang", json!(0.7), "Roof overhang past the walls on all four sides (m)."),
            ps("roof_thickness", json!(0.22), "Roof slab thickness, square to the slope (m)."),
            ps("chimney", json!([]), "Optional outside chimney stack on a gable wall: [wall 0 or 2, position 0..1, width, depth it stands out, height above the ridge] (m). Empty = none."),
            ps("open", json!(0.0), "Swing of the entrance door leaf into the room (degrees; 0 = closed)."),
            ps("inner_open", json!(0.0), "Swing of the inner door leaves (degrees; 0 = closed)."),
            ps("glazing", json!(0), "0 = open windows: a frame and nothing in the hole, so light and sky reach the room (the only value shipped; 1, a glass pane, is refused until the render and the engine have a glass material)."),
            ps("mode", json!("shell"), "shell | openings | rooms (see above)."),
        ],
    )]
}

pub fn cook(kind: &str, p: &Params) -> Option<Res> {
    match kind {
        "building" => Some(build(p)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------------------------------------------------------
// Mesh writer
// ---------------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Tag {
    room: i32,
    part: f32,
    open: i32,
    mat: &'static str,
}

#[derive(Default)]
struct Mesh {
    pts: Vec<V3>,
    polys: Vec<Vec<u32>>,
    room: Vec<f32>,
    part: Vec<f32>,
    open: Vec<f32>,
    mats: Vec<&'static str>,
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}

impl Mesh {
    /// One quad, wound so that it faces `out` (the side the air is on). A quad with no area is dropped.
    fn quad(&mut self, q: [V3; 4], out: V3, tag: Tag) {
        let mut n = [0.0f32; 3];
        for i in 0..4 {
            let c = math::cross(q[i], q[(i + 1) % 4]);
            for (s, v) in n.iter_mut().zip(c) {
                *s += v;
            }
        }
        if dot(n, n) < 1e-10 {
            return;
        }
        let base = self.pts.len() as u32;
        self.pts.extend_from_slice(&q);
        let order: [u32; 4] = if dot(n, out) < 0.0 { [3, 2, 1, 0] } else { [0, 1, 2, 3] };
        self.polys.push(order.iter().map(|k| base + k).collect());
        self.room.push(tag.room as f32);
        self.part.push(tag.part);
        self.open.push(tag.open as f32);
        self.mats.push(tag.mat);
    }

    /// A closed box in a wall frame (u along the wall, v across it, y up), optionally turned about a vertical axis at (u, v) by
    /// an angle in radians (positive swings the u axis towards -v). All six faces face away from the box centre.
    fn solid(&mut self, f: &Frame, u: (f32, f32), v: (f32, f32), y: (f32, f32), turn: Option<(f32, f32, f32)>, tag: Tag) {
        let mut c = [[0.0f32; 3]; 8];
        for (i, slot) in c.iter_mut().enumerate() {
            let mut uu = if i & 1 == 0 { u.0 } else { u.1 };
            let mut vv = if i & 2 == 0 { v.0 } else { v.1 };
            let yy = if i & 4 == 0 { y.0 } else { y.1 };
            if let Some((pu, pv, ang)) = turn {
                let (s, co) = math::sin_cos(ang);
                let (a, b) = (uu - pu, vv - pv);
                uu = pu + a * co + b * s;
                vv = pv - a * s + b * co;
            }
            *slot = f.at(uu, vv, yy);
        }
        let mut mid = [0.0f32; 3];
        for q in &c {
            for (m, x) in mid.iter_mut().zip(q) {
                *m += x / 8.0;
            }
        }
        for face in [[0, 2, 6, 4], [1, 3, 7, 5], [0, 1, 5, 4], [2, 3, 7, 6], [0, 1, 3, 2], [4, 5, 7, 6]] {
            let q = [c[face[0]], c[face[1]], c[face[2]], c[face[3]]];
            let mut centre = [0.0f32; 3];
            for pt in &q {
                for (m, x) in centre.iter_mut().zip(pt) {
                    *m += x / 4.0;
                }
            }
            self.quad(q, math::sub(centre, mid), tag);
        }
    }
}

/// A wall's own axes: u along the wall, v across it (v = 0 on the room side), y up.
#[derive(Clone, Copy)]
struct Frame {
    o: V3,
    du: V3,
    dv: V3,
}

impl Frame {
    fn at(&self, u: f32, v: f32, y: f32) -> V3 {
        [self.o[0] + self.du[0] * u + self.dv[0] * v, y, self.o[2] + self.du[2] * u + self.dv[2] * v]
    }
}

const WORLD: Frame = Frame { o: [0.0; 3], du: [1.0, 0.0, 0.0], dv: [0.0, 0.0, 1.0] };

// ---------------------------------------------------------------------------------------------------------------------------
// Wall panels
// ---------------------------------------------------------------------------------------------------------------------------

/// The top edge of a wall: level, the two pitched gable lines (u is along X, apex at u = 0), or sloped across the thickness
/// so that it follows the underside of the roof.
#[derive(Clone, Copy)]
enum Top {
    Flat(f32),
    Gable { apex: f32, tan: f32 },
    Slope { outer: f32, tan: f32, t: f32 },
}

impl Top {
    fn y(&self, u: f32, v: f32) -> f32 {
        match *self {
            Top::Flat(y) => y,
            Top::Gable { apex, tan } => apex - tan * u.abs(),
            Top::Slope { outer, tan, t } => outer + tan * (t - v),
        }
    }
}

#[derive(Clone, Copy)]
struct Hole {
    u0: f32,
    u1: f32,
    y0: f32,
    y1: f32,
    id: i32,
    /// The room on the v = 0 side and on the v = t side (-1 = outside).
    room: i32,
    room_hi: i32,
}

type Spans = Vec<(f32, f32, i32)>;

fn room_at(spans: &Spans, mid: f32) -> i32 {
    spans.iter().find(|s| mid > s.0 && mid < s.1).map_or(HIDDEN, |s| s.2)
}

struct Panel {
    f: Frame,
    u0: f32,
    u1: f32,
    t: f32,
    y_base: f32,
    top: Top,
    /// Which room each stretch of the v = 0 face and of the v = t face looks into (HIDDEN = covered by another wall).
    lo: Spans,
    hi: Spans,
    cap_lo: bool,
    cap_hi: bool,
    holes: Vec<Hole>,
}

impl Panel {
    fn min_top(&self, u0: f32, u1: f32) -> f32 {
        [(u0, 0.0), (u1, 0.0), (u0, self.t), (u1, self.t)].iter().map(|&(u, v)| self.top.y(u, v)).fold(f32::MAX, f32::min)
    }

    fn emit(&self, m: &mut Mesh) {
        let f = &self.f;
        let du = [f.du[0], 0.0, f.du[2]];
        let dv = [f.dv[0], 0.0, f.dv[2]];
        let mut cuts = vec![self.u0, self.u1];
        for h in &self.holes {
            cuts.extend([h.u0, h.u1]);
        }
        for s in self.lo.iter().chain(self.hi.iter()) {
            cuts.extend([s.0, s.1]);
        }
        if matches!(self.top, Top::Gable { .. }) {
            cuts.push(0.0);
        }
        cuts.retain(|c| *c >= self.u0 && *c <= self.u1);
        cuts.sort_by(f32::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        let wall = |room: i32, open: i32| Tag { room, part: PART_WALL, open, mat: "hut_wall" };
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mid = 0.5 * (a + b);
            let hole = self.holes.iter().find(|h| mid > h.u0 && mid < h.u1);
            // (bottom, level top if the piece ends at the sill of a hole, is the bottom the wall's base)
            let mut pieces: Vec<(f32, Option<f32>, bool)> = Vec::new();
            match hole {
                None => pieces.push((self.y_base, None, true)),
                Some(h) => {
                    if h.y0 > self.y_base + 1e-4 {
                        pieces.push((self.y_base, Some(h.y0), true));
                    }
                    if self.top.y(a, 0.0).min(self.top.y(b, 0.0)).min(self.top.y(a, self.t)).min(self.top.y(b, self.t)) > h.y1 + 1e-4 {
                        pieces.push((h.y1, None, false));
                    }
                }
            }
            let (lo_room, hi_room) = (room_at(&self.lo, mid), room_at(&self.hi, mid));
            for (yb, level, is_base) in pieces {
                let ty = |u: f32, v: f32| level.unwrap_or_else(|| self.top.y(u, v));
                let reveal = hole.map_or(-1, |h| h.room);
                let open = hole.map_or(-1, |h| h.id);
                if lo_room != HIDDEN {
                    m.quad([f.at(a, 0.0, yb), f.at(b, 0.0, yb), f.at(b, 0.0, ty(b, 0.0)), f.at(a, 0.0, ty(a, 0.0))], neg(dv), wall(lo_room, -1));
                }
                if hi_room != HIDDEN {
                    m.quad([f.at(a, self.t, yb), f.at(b, self.t, yb), f.at(b, self.t, ty(b, self.t)), f.at(a, self.t, ty(a, self.t))], dv, wall(hi_room, -1));
                }
                let top_room = if level.is_some() { reveal } else { -1 };
                m.quad([f.at(a, 0.0, ty(a, 0.0)), f.at(b, 0.0, ty(b, 0.0)), f.at(b, self.t, ty(b, self.t)), f.at(a, self.t, ty(a, self.t))], [0.0, 1.0, 0.0], wall(top_room, if level.is_some() { open } else { -1 }));
                if !is_base {
                    m.quad([f.at(a, 0.0, yb), f.at(b, 0.0, yb), f.at(b, self.t, yb), f.at(a, self.t, yb)], [0.0, -1.0, 0.0], wall(reveal, open));
                }
                if a == self.u0 && self.cap_lo {
                    m.quad([f.at(a, 0.0, yb), f.at(a, self.t, yb), f.at(a, self.t, ty(a, self.t)), f.at(a, 0.0, ty(a, 0.0))], neg(du), wall(-1, -1));
                }
                if b == self.u1 && self.cap_hi {
                    m.quad([f.at(b, 0.0, yb), f.at(b, self.t, yb), f.at(b, self.t, ty(b, self.t)), f.at(b, 0.0, ty(b, 0.0))], du, wall(-1, -1));
                }
            }
        }
        for h in &self.holes {
            m.quad([f.at(h.u0, 0.0, h.y0), f.at(h.u0, self.t, h.y0), f.at(h.u0, self.t, h.y1), f.at(h.u0, 0.0, h.y1)], du, wall(h.room, h.id));
            m.quad([f.at(h.u1, 0.0, h.y0), f.at(h.u1, self.t, h.y0), f.at(h.u1, self.t, h.y1), f.at(h.u1, 0.0, h.y1)], neg(du), wall(h.room, h.id));
        }
    }

    /// Trim boards round every opening on both faces, then the leaf of a door (a window stays open: see `glazing`).
    fn fit(&self, m: &mut Mesh, h: &Hole, window: bool, swing_deg: f32) {
        let t = self.t;
        for (v, room) in [((-TRIM_OUT, 0.0), h.room), ((t, t + TRIM_OUT), h.room_hi)] {
            let tag = Tag { room, part: PART_FRAME, open: h.id, mat: "hut_wood" };
            let low = if window { h.y0 - TRIM_W } else { h.y0 };
            m.solid(&self.f, (h.u0 - TRIM_W, h.u1 + TRIM_W), v, (h.y1, h.y1 + TRIM_W), None, tag);
            m.solid(&self.f, (h.u0 - TRIM_W, h.u0), v, (low, h.y1), None, tag);
            m.solid(&self.f, (h.u1, h.u1 + TRIM_W), v, (low, h.y1), None, tag);
            if window {
                m.solid(&self.f, (h.u0, h.u1), v, (h.y0 - TRIM_W, h.y0), None, tag);
            }
        }
        let mid = 0.5 * t;
        // A window is an open frame: nothing in the hole, so light and sky reach the room.
        if !window {
            let tag = Tag { room: h.room, part: PART_DOOR, open: h.id, mat: "hut_door" };
            let hinge = h.u0 + 0.01;
            m.solid(&self.f, (hinge, h.u1 - 0.01), (mid - 0.025, mid + 0.025), (h.y0 + 0.005, h.y1 - 0.01), Some((hinge, mid, math::to_radians(swing_deg))), tag);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Entrance,
    Inner,
    Window,
}

struct Rec {
    id: i32,
    kind: Kind,
    wall: i32,
    from: i32,
    to: i32,
    centre: V3,
    dir: V3,
    width: f32,
    height: f32,
    sill: f32,
}

struct RoomRec {
    id: i32,
    row: usize,
    col: usize,
    x: (f32, f32),
    z: (f32, f32),
}

struct Made {
    mesh: Mesh,
    rooms: Vec<RoomRec>,
    recs: Vec<Rec>,
    info: Value,
    floor_y: f32,
    eave_y: f32,
}

fn r4(x: f32) -> f64 {
    ((x as f64) * 1e4).round() / 1e4
}

fn nums(p: &Params, key: &str) -> Result<Vec<f32>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(a)) => a.iter().map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| format!("{key}: expected a list of numbers"))).collect(),
        Some(_) => Err(format!("{key}: expected a list of numbers")),
    }
}

/// A table of numbers: a list of rows of `width` numbers each (a single flat row is accepted too).
fn table(p: &Params, key: &str, width: usize) -> Result<Vec<Vec<f32>>, String> {
    let Some(Value::Array(a)) = p.get(key) else { return Ok(vec![]) };
    if a.is_empty() {
        return Ok(vec![]);
    }
    let rows: Vec<Vec<f32>> = if a.iter().all(Value::is_number) {
        vec![nums(p, key)?]
    } else {
        a.iter()
            .map(|r| match r {
                Value::Array(x) => x.iter().map(|n| n.as_f64().map(|f| f as f32)).collect::<Option<Vec<f32>>>(),
                _ => None,
            }
            .ok_or_else(|| format!("{key}: every entry must be a list of {width} numbers")))
            .collect::<Result<_, _>>()?
    };
    for r in &rows {
        if r.len() != width {
            return Err(format!("{key}: each entry needs {width} numbers, got {}", r.len()));
        }
    }
    Ok(rows)
}

fn index(x: f32, max: usize, what: &str) -> Result<usize, String> {
    let r = x.round();
    if (x - r).abs() > 1e-3 || r < 0.0 || r > max as f32 {
        return Err(format!("{what} must be a whole number from 0 to {max}, got {x}"));
    }
    Ok(r as usize)
}

fn cut_list(p: &Params, key: &str) -> Result<Vec<f32>, String> {
    let c = nums(p, key)?;
    if c.len() > 3 {
        return Err(format!("{key}: at most 3 partitions per axis, got {}", c.len()));
    }
    for (i, &f) in c.iter().enumerate() {
        if !(0.05..=0.95).contains(&f) || (i > 0 && f <= c[i - 1]) {
            return Err(format!("{key}: fractions must rise and lie between 0.05 and 0.95, got {c:?}"));
        }
    }
    Ok(c)
}

fn spans_text(s: &Spans) -> String {
    s.iter().filter(|x| x.2 >= 0).map(|x| format!("room {} from {:.2} to {:.2}", x.2, x.0, x.1)).collect::<Vec<_>>().join(", ")
}

/// One opening as asked for: where along its panel, how big, what kind.
struct Opening {
    uc: f32,
    width: f32,
    y0: f32,
    height: f32,
    kind: Kind,
    wall: i32,
    what: String,
}

/// Check an opening against its panel (inside one room's stretch of wall, clear of corners and partitions, under the roof line)
/// and turn it into the record and the hole.
fn place(panels: &[Panel], fy: f32, id: i32, pi: usize, o: &Opening) -> Result<(Rec, Hole), String> {
    let (what, uc, width, height) = (&o.what, o.uc, o.width, o.height);
    let y1 = o.y0 + height;
    let (u0, u1) = (uc - width * 0.5, uc + width * 0.5);
    let panel = &panels[pi];
    if !(width > 0.1 && height > 0.1) {
        return Err(format!("building: {what} has no size"));
    }
    let Some(span) = panel.lo.iter().find(|s| s.2 >= 0 && u0 >= s.0 + MARGIN && u1 <= s.1 - MARGIN).copied() else {
        return Err(format!("building: {what} (centre {uc:.2}, width {width}) does not fit inside one room: an opening must lie wholly inside one room's stretch of the wall, {MARGIN} m clear of corners and partitions. This wall has: {}", spans_text(&panel.lo)));
    };
    let top = panel.min_top(u0, u1);
    if y1 + TRIM_W + 0.05 > top {
        return Err(format!("building: {what} reaches the roof line (its top {y1:.2} m, the wall is {top:.2} m there): lower it or make it smaller"));
    }
    let room_hi = if o.kind == Kind::Inner { room_at(&panel.hi, uc) } else { -1 };
    let hole = Hole { u0, u1, y0: o.y0, y1, id, room: span.2, room_hi };
    let centre = panel.f.at(uc, 0.5 * panel.t, 0.5 * (o.y0 + y1));
    let dir = [panel.f.dv[0], 0.0, panel.f.dv[2]];
    let (from, to) = if o.kind == Kind::Inner { (span.2, room_hi) } else { (span.2, -1) };
    Ok((Rec { id, kind: o.kind, wall: o.wall, from, to, centre, dir, width, height, sill: o.y0 - fy }, hole))
}

fn make(p: &Params) -> Result<Made, String> {
    let (w, d, h, t, pt) = (p.f("width"), p.f("depth"), p.f("height"), p.f("wall"), p.f("partition"));
    let (fy, fl, foot) = (p.f("floor_y"), p.f("floor_thickness"), p.f("footing"));
    if !(w >= 2.5 && d >= 2.5) {
        return Err(format!("building: width and depth must be at least 2.5 m, got {w} x {d}"));
    }
    if !(h >= 2.3) {
        return Err(format!("building: height must be at least 2.3 m (a door and its trim), got {h}"));
    }
    if !(0.05..=0.6).contains(&t) || !(0.05..=0.4).contains(&pt) {
        return Err("building: wall must be 0.05 to 0.6 m and partition 0.05 to 0.4 m thick".into());
    }
    if !(fl >= 0.02 && foot > fl) {
        return Err("building: floor_thickness must be at least 0.02 and below the footing height".into());
    }
    let (pitch, over, th) = (p.f("pitch"), p.f("overhang"), p.f("roof_thickness"));
    if !(10.0..=70.0).contains(&pitch) || !(0.0..=3.0).contains(&over) || !(0.05..=1.0).contains(&th) {
        return Err("building: pitch must be 10 to 70 degrees, overhang 0 to 3 m, roof_thickness 0.05 to 1 m".into());
    }
    let (sn, cs) = math::sin_cos(math::to_radians(pitch));
    let tn = sn / cs;
    let y_base = fy - fl;
    let y_e = fy + h;
    let rise = w * 0.5 * tn;
    let apex = y_e + rise;

    // ---- the grid of rooms
    let cuts_x = cut_list(p, "cuts_x")?;
    let cuts_z = cut_list(p, "cuts_z")?;
    let (cols, nrows) = (cuts_x.len() + 1, cuts_z.len() + 1);
    let (xin0, xin1, zin0, zin1) = (-w * 0.5 + t, w * 0.5 - t, -d * 0.5 + t, d * 0.5 - t);
    let cx: Vec<f32> = cuts_x.iter().map(|f| xin0 + f * (xin1 - xin0)).collect();
    let cz: Vec<f32> = cuts_z.iter().map(|f| zin1 - f * (zin1 - zin0)).collect();
    let xcols: Vec<(f32, f32)> = (0..cols).map(|c| (if c == 0 { xin0 } else { cx[c - 1] + pt * 0.5 }, if c == cols - 1 { xin1 } else { cx[c] - pt * 0.5 })).collect();
    let zrows: Vec<(f32, f32)> = (0..nrows).map(|r| (if r == nrows - 1 { zin0 } else { cz[r] + pt * 0.5 }, if r == 0 { zin1 } else { cz[r - 1] - pt * 0.5 })).collect();
    for (i, c) in xcols.iter().enumerate() {
        if c.1 - c.0 < MIN_ROOM {
            return Err(format!("building: column {i} of rooms would be {:.2} m wide (at least {MIN_ROOM}): move the cuts_x apart or widen the house", c.1 - c.0));
        }
    }
    for (i, r) in zrows.iter().enumerate() {
        if r.1 - r.0 < MIN_ROOM {
            return Err(format!("building: row {i} of rooms would be {:.2} m deep (at least {MIN_ROOM}): move the cuts_z apart or deepen the house", r.1 - r.0));
        }
    }
    let n_rooms = cols * nrows;
    let room_id = |r: usize, c: usize| (r * cols + c) as i32;
    let rooms: Vec<RoomRec> = (0..n_rooms).map(|i| RoomRec { id: i as i32, row: i / cols, col: i % cols, x: xcols[i % cols], z: zrows[i / cols] }).collect();

    // ---- the wall panels: 0..4 outside walls, then one per x cut, then one per (z cut, column)
    let mut panels: Vec<Panel> = Vec::new();
    let gable = Top::Gable { apex, tan: tn };
    let front_spans: Spans = (0..cols).map(|c| (xcols[c].0, xcols[c].1, room_id(0, c))).collect();
    let back_spans: Spans = (0..cols).map(|c| (-xcols[c].1, -xcols[c].0, room_id(nrows - 1, c))).collect();
    let right_spans: Spans = (0..nrows).map(|r| (-zrows[r].1, -zrows[r].0, room_id(r, cols - 1))).collect();
    let left_spans: Spans = (0..nrows).map(|r| (zrows[r].0, zrows[r].1, room_id(r, 0))).collect();
    let outside = |a: f32, b: f32| -> Spans { vec![(a, b, -1)] };
    let side_u = d * 0.5 - t;
    panels.push(Panel { f: Frame { o: [0.0, 0.0, d * 0.5 - t], du: [1.0, 0.0, 0.0], dv: [0.0, 0.0, 1.0] }, u0: -w * 0.5, u1: w * 0.5, t, y_base, top: gable, lo: front_spans, hi: outside(-w * 0.5, w * 0.5), cap_lo: true, cap_hi: true, holes: vec![] });
    panels.push(Panel { f: Frame { o: [w * 0.5 - t, 0.0, 0.0], du: [0.0, 0.0, -1.0], dv: [1.0, 0.0, 0.0] }, u0: -side_u, u1: side_u, t, y_base, top: Top::Slope { outer: y_e, tan: tn, t }, lo: right_spans, hi: outside(-side_u, side_u), cap_lo: false, cap_hi: false, holes: vec![] });
    panels.push(Panel { f: Frame { o: [0.0, 0.0, -d * 0.5 + t], du: [-1.0, 0.0, 0.0], dv: [0.0, 0.0, -1.0] }, u0: -w * 0.5, u1: w * 0.5, t, y_base, top: gable, lo: back_spans, hi: outside(-w * 0.5, w * 0.5), cap_lo: true, cap_hi: true, holes: vec![] });
    panels.push(Panel { f: Frame { o: [-w * 0.5 + t, 0.0, 0.0], du: [0.0, 0.0, 1.0], dv: [-1.0, 0.0, 0.0] }, u0: -side_u, u1: side_u, t, y_base, top: Top::Slope { outer: y_e, tan: tn, t }, lo: left_spans, hi: outside(-side_u, side_u), cap_lo: false, cap_hi: false, holes: vec![] });
    // A partition running along Z (an x cut) is level and as high as the roof is at the face nearest the ridge; the roof slab
    // is thicker than the little that pokes into it.
    if !cx.is_empty() && pt * tn > th / cs - 0.02 {
        return Err("building: the roof is too thin for the partitions under its slope: raise roof_thickness or lower the pitch".into());
    }
    let x_base = panels.len();
    for (k, &c) in cx.iter().enumerate() {
        let near = (c.abs() - pt * 0.5).max(0.0);
        let lo: Spans = (0..nrows).map(|r| (-zrows[r].1, -zrows[r].0, room_id(r, k))).collect();
        let hi: Spans = (0..nrows).map(|r| (-zrows[r].1, -zrows[r].0, room_id(r, k + 1))).collect();
        panels.push(Panel { f: Frame { o: [c - pt * 0.5, 0.0, 0.0], du: [0.0, 0.0, -1.0], dv: [1.0, 0.0, 0.0] }, u0: -zin1, u1: -zin0, t: pt, y_base, top: Top::Flat(y_e + (w * 0.5 - near) * tn), lo, hi, cap_lo: false, cap_hi: false, holes: vec![] });
    }
    let z_base = panels.len();
    for (k, &c) in cz.iter().enumerate() {
        for col in 0..cols {
            let (a, b) = xcols[col];
            panels.push(Panel { f: Frame { o: [0.0, 0.0, c - pt * 0.5], du: [1.0, 0.0, 0.0], dv: [0.0, 0.0, 1.0] }, u0: a, u1: b, t: pt, y_base, top: gable, lo: vec![(a, b, room_id(k + 1, col))], hi: vec![(a, b, room_id(k, col))], cap_lo: false, cap_hi: false, holes: vec![] });
        }
    }

    // ---- the openings, checked one by one against the room they belong to
    let mut recs: Vec<Rec> = Vec::new();
    let mut specs: Vec<(usize, Hole, Kind)> = Vec::new();
    let add = |panels: &mut Vec<Panel>, recs: &mut Vec<Rec>, specs: &mut Vec<(usize, Hole, Kind)>, pi: usize, o: Opening| -> Result<(), String> {
        let (rec, hole) = place(panels, fy, recs.len() as i32, pi, &o)?;
        specs.push((pi, hole, o.kind));
        panels[pi].holes.push(hole);
        recs.push(rec);
        Ok(())
    };

    if p.i("glazing") != 0 {
        return Err("building: glazing=1 (a glass pane) is not available yet: the render and the engine have no glass material, so a pane would be a black plate that keeps the light out. Windows are open frames (glazing=0)".into());
    }
    let door = table(p, "door", 4)?;
    let Some(dr) = door.first() else {
        return Err("building: no entrance: give `door` as [wall, position, width, height]".into());
    };
    if door.len() > 1 {
        return Err("building: one entrance only; further doors between rooms go in inner_doors".into());
    }
    if dr[2] < MIN_DOOR_W || dr[3] < MIN_DOOR_H {
        return Err(format!("building: the entrance is {} x {} m; a door is at least {MIN_DOOR_W} x {MIN_DOOR_H} m", dr[2], dr[3]));
    }
    let wi = index(dr[0], 3, "door wall")?;
    let len = if wi % 2 == 0 { w } else { d };
    add(&mut panels, &mut recs, &mut specs, wi, Opening { uc: -len * 0.5 + dr[1] * len, width: dr[2], y0: fy, height: dr[3], kind: Kind::Entrance, wall: wi as i32, what: "the entrance door".into() })?;
    let entrance_room = recs[0].from;
    let mut doors_between: Vec<(i32, i32)> = Vec::new();
    for (i, r) in table(p, "inner_doors", 5)?.iter().enumerate() {
        let what = format!("inner door {i}");
        let (a, b) = (index(r[0], n_rooms - 1, &format!("{what}: room a"))?, index(r[1], n_rooms - 1, &format!("{what}: room b"))?);
        let (a, b) = (a.min(b), a.max(b));
        if r[3] < MIN_DOOR_W || r[4] < MIN_DOOR_H {
            return Err(format!("building: {what} is {} x {} m; a door is at least {MIN_DOOR_W} x {MIN_DOOR_H} m", r[3], r[4]));
        }
        let (ra, ca, rb, cb) = (a / cols, a % cols, b / cols, b % cols);
        let (pi, lo_edge, hi_edge) = if ra == rb && cb == ca + 1 {
            (x_base + ca, -zrows[ra].1, -zrows[ra].0)
        } else if ca == cb && rb == ra + 1 {
            (z_base + ra * cols + ca, xcols[ca].0, xcols[ca].1)
        } else {
            return Err(format!("building: {what} joins rooms {a} and {b}, which do not share a wall (rooms are numbered row by row from the front-left, {cols} per row)"));
        };
        add(&mut panels, &mut recs, &mut specs, pi, Opening { uc: lo_edge + r[2] * (hi_edge - lo_edge), width: r[3], y0: fy, height: r[4], kind: Kind::Inner, wall: -1, what: what.clone() })?;
        doors_between.push((a as i32, b as i32));
    }
    for (i, r) in table(p, "windows", 5)?.iter().enumerate() {
        let what = format!("window {i}");
        let wi = index(r[0], 3, &format!("{what}: wall"))?;
        let len = if wi % 2 == 0 { w } else { d };
        if r[4] < 0.2 {
            return Err(format!("building: {what}: the sill must be at least 0.2 m above the floor"));
        }
        add(&mut panels, &mut recs, &mut specs, wi, Opening { uc: -len * 0.5 + r[1] * len, width: r[2], y0: fy + r[4], height: r[3], kind: Kind::Window, wall: wi as i32, what: format!("{what} on the {} wall", WALL_NAMES[wi]) })?;
    }
    for (pi, panel) in panels.iter().enumerate() {
        let mut hs = panel.holes.clone();
        hs.sort_by(|a, b| a.u0.total_cmp(&b.u0));
        for pair in hs.windows(2) {
            if pair[1].u0 - pair[0].u1 < GAP {
                return Err(format!("building: two openings on the same wall (panel {pi}) are closer than {GAP} m: move one of them"));
            }
        }
    }

    // ---- every room must be reachable from the entrance through doors that fit a person
    let mut seen = vec![false; n_rooms];
    let mut stack = vec![entrance_room];
    while let Some(r) = stack.pop() {
        if r < 0 || seen[r as usize] {
            continue;
        }
        seen[r as usize] = true;
        for &(a, b) in &doors_between {
            if a == r {
                stack.push(b);
            }
            if b == r {
                stack.push(a);
            }
        }
    }
    if let Some(lost) = seen.iter().position(|s| !s) {
        return Err(format!("building: room {lost} cannot be reached from the entrance (room {entrance_room}): no chain of inner_doors leads to it. Add an inner door [a, b, position, width, height] that joins it to a room that can be reached"));
    }

    // ---- geometry
    let mut m = Mesh::default();
    let tag = |room: i32, part: f32, mat: &'static str| Tag { room, part, open: -1, mat };
    // stone footing under the whole house
    m.solid(&WORLD, (-w * 0.5 - 0.2, w * 0.5 + 0.2), (-d * 0.5 - 0.2, d * 0.5 + 0.2), (fy - foot, y_base), None, tag(-1, PART_FOOTING, "hut_stone"));
    // floor surface of every room
    for r in &rooms {
        m.quad([[r.x.0, fy, r.z.0], [r.x.1, fy, r.z.0], [r.x.1, fy, r.z.1], [r.x.0, fy, r.z.1]], [0.0, 1.0, 0.0], tag(r.id, PART_FLOOR, "hut_wood"));
    }
    for panel in &panels {
        panel.emit(&mut m);
    }
    for (pi, hole, kind) in &specs {
        let swing = if *kind == Kind::Entrance { p.f("open") } else { p.f("inner_open") };
        panels[*pi].fit(&mut m, hole, *kind == Kind::Window, swing);
    }
    // timber: corner posts and the wall plates the roof rests on
    let timber = tag(-1, PART_TIMBER, "hut_wood");
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            m.solid(&WORLD, (sx * w * 0.5 - 0.12, sx * w * 0.5 + 0.12), (sz * d * 0.5 - 0.12, sz * d * 0.5 + 0.12), (y_base, y_e), None, timber);
        }
        m.solid(&WORLD, (sx * w * 0.5 - 0.13, sx * w * 0.5 + 0.13), (-d * 0.5 - 0.25, d * 0.5 + 0.25), (y_e - 0.16, y_e + 0.06), None, timber);
    }
    // entrance steps: from the ground (y = 0) up to the sill in equal rises of at most MAX_RISE, each tread TREAD deep, a little
    // wider than the door; each block runs down to the foot of the footing so it stands on a slope, and the top tread is the landing
    if fy >= MAX_RISE {
        let n = ((fy / MAX_RISE) - 1e-4).ceil().max(1.0) as usize;
        let rise = fy / n as f32;
        if let Some((pi, hole, _)) = specs.first() {
            let step = tag(-1, PART_STEP, "hut_stone");
            for k in 1..=n {
                let depth = TREAD * (n - k + 1) as f32;
                m.solid(&panels[*pi].f, (hole.u0 - 0.2, hole.u1 + 0.2), (panels[*pi].t, panels[*pi].t + depth), (fy - foot, rise * k as f32), None, step);
            }
        }
    }
    // roof: two slabs meeting at the ridge, their undersides running through the tops of the walls
    let zr = d * 0.5 + over;
    let roof = tag(-1, PART_ROOF, "hut_roof");
    let top_apex = apex + th / cs;
    for sgn in [1.0f32, -1.0] {
        let e = [sgn * (w * 0.5 + over), y_e - over * tn];
        let e2 = [e[0] + th * sgn * sn, e[1] + th * cs];
        let (a, tp) = ([0.0, apex], [0.0, top_apex]);
        let at = |q: [f32; 2], z: f32| [q[0], q[1], z];
        m.quad([at(e, -zr), at(e, zr), at(a, zr), at(a, -zr)], [-sgn * sn, -cs, 0.0], roof);
        m.quad([at(tp, -zr), at(tp, zr), at(e2, zr), at(e2, -zr)], [sgn * sn, cs, 0.0], roof);
        m.quad([at(e, -zr), at(e, zr), at(e2, zr), at(e2, -zr)], [sgn * cs, -sn, 0.0], roof);
        for z in [-zr, zr] {
            m.quad([at(e, z), at(a, z), at(tp, z), at(e2, z)], [0.0, 0.0, z], roof);
        }
    }
    m.solid(&WORLD, (-0.14, 0.14), (-zr - 0.15, zr + 0.15), (top_apex - 0.07, top_apex + 0.17), None, timber);
    // chimney
    let chim = table(p, "chimney", 5)?;
    if let Some(c) = chim.first() {
        let wi = index(c[0], 2, "chimney wall")?;
        if wi == 1 {
            return Err("building: the chimney stands on a gable wall: wall 0 (front) or 2 (back)".into());
        }
        let len = w;
        let uc = -len * 0.5 + c[1] * len;
        let (u0, u1) = (uc - c[2] * 0.5, uc + c[2] * 0.5);
        if u0 < -w * 0.5 + 0.05 || u1 > w * 0.5 - 0.05 || c[2] < 0.3 || c[3] < 0.2 {
            return Err("building: the chimney does not fit on the wall (width at least 0.3, depth at least 0.2, inside the wall's length)".into());
        }
        if panels[wi].holes.iter().any(|h| u1 > h.u0 - 0.2 && u0 < h.u1 + 0.2) {
            return Err("building: the chimney stands in front of a door or window: move it along the wall".into());
        }
        m.solid(&panels[wi].f, (u0, u1), (t - 0.05, t + c[3]), (y_base, top_apex + 0.17 + c[4]), None, tag(-1, PART_CHIMNEY, "hut_stone"));
    }

    // ---- what a later stage reads
    let room_json = |r: &RoomRec| {
        json!({"id": r.id, "row": r.row, "col": r.col, "min": [r4(r.x.0), r4(r.z.0)], "max": [r4(r.x.1), r4(r.z.1)], "floor_y": r4(fy),
               "center": [r4(0.5 * (r.x.0 + r.x.1)), r4(fy), r4(0.5 * (r.z.0 + r.z.1))], "size": [r4(r.x.1 - r.x.0), r4(h), r4(r.z.1 - r.z.0)]})
    };
    let kind_name = |k: Kind| match k {
        Kind::Entrance => "entrance",
        Kind::Inner => "inner_door",
        Kind::Window => "window",
    };
    let open_json = |r: &Rec| {
        json!({"id": r.id, "kind": kind_name(r.kind), "wall": r.wall, "room_from": r.from, "room_to": r.to, "center": [r4(r.centre[0]), r4(r.centre[1]), r4(r.centre[2])],
               "dir": [r4(r.dir[0]), r4(r.dir[1]), r4(r.dir[2])], "width": r4(r.width), "height": r4(r.height), "sill": r4(r.sill)})
    };
    let info = json!({
        "size": [r4(w), r4(h), r4(d)], "wall": r4(t), "partition": r4(pt), "floor_y": r4(fy), "eave_y": r4(y_e), "ridge_y": r4(apex),
        "part_codes": PART_CODES.iter().map(|(n, c)| (n.to_string(), json!(*c as i32))).collect::<serde_json::Map<_, _>>(),
        "rooms": rooms.iter().map(room_json).collect::<Vec<_>>(),
        "openings": recs.iter().map(open_json).collect::<Vec<_>>(),
        "entrance": 0,
        "frame": "the node's own frame; use mode 'openings' and 'rooms' for positions that follow a transform",
    });
    Ok(Made { mesh: m, rooms, recs, info, floor_y: fy, eave_y: y_e })
}

fn build(p: &Params) -> Res {
    let made = make(p)?;
    let mode = p.s("mode");
    let mut g = match mode.as_str() {
        "shell" => shell(&made),
        "openings" => openings(&made),
        "rooms" => rooms(&made),
        o => return Err(format!("building: mode must be shell, openings or rooms, got '{o}'")),
    };
    g.detail.insert("building".into(), made.info);
    Ok(g)
}

fn shell(m: &Made) -> Geometry {
    let mesh = &m.mesh;
    let mut g = Geometry::from_polys(mesh.pts.clone(), &mesh.polys);
    g.prim.set("room", Column::from_f32(mesh.room.clone()));
    g.prim.set("part", Column::from_f32(mesh.part.clone()));
    g.prim.set("opening", Column::from_f32(mesh.open.clone()));
    let mut names: Vec<&str> = mesh.mats.clone();
    names.sort_unstable();
    names.dedup();
    for name in names {
        let sel: Vec<bool> = mesh.mats.iter().map(|x| *x == name).collect();
        g.assign_material(name, &sel);
    }
    g
}

fn openings(m: &Made) -> Geometry {
    let mut pts = Vec::new();
    let mut polys = Vec::new();
    let (mut id, mut kind, mut wall, mut from, mut to, mut width, mut height, mut sill) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for (i, r) in m.recs.iter().enumerate() {
        pts.push(r.centre);
        pts.push([r.centre[0] + r.dir[0], r.centre[1] + r.dir[1], r.centre[2] + r.dir[2]]);
        polys.push(vec![2 * i as u32, 2 * i as u32 + 1]);
        id.push(r.id as f32);
        kind.push(match r.kind {
            Kind::Entrance => 0.0,
            Kind::Inner => 1.0,
            Kind::Window => 2.0,
        });
        wall.push(r.wall as f32);
        from.push(r.from as f32);
        to.push(r.to as f32);
        width.push(r.width);
        height.push(r.height);
        sill.push(r.sill);
    }
    let mut g = Geometry::from_polys(pts, &polys);
    for (k, v) in [("opening", id), ("kind", kind), ("wall", wall), ("room_from", from), ("room_to", to), ("width", width), ("height", height), ("sill", sill)] {
        g.prim.set(k, Column::from_f32(v));
    }
    g
}

fn rooms(m: &Made) -> Geometry {
    let mut pts = Vec::new();
    let mut polys = Vec::new();
    let (mut id, mut row, mut col, mut area, mut height) = (vec![], vec![], vec![], vec![], vec![]);
    for (i, r) in m.rooms.iter().enumerate() {
        let y = m.floor_y;
        let base = 4 * i as u32;
        pts.extend([[r.x.0, y, r.z.0], [r.x.1, y, r.z.0], [r.x.1, y, r.z.1], [r.x.0, y, r.z.1]]);
        // counter-clockwise seen from above: facing up
        polys.push(vec![base, base + 3, base + 2, base + 1]);
        id.push(r.id as f32);
        row.push(r.row as f32);
        col.push(r.col as f32);
        area.push((r.x.1 - r.x.0) * (r.z.1 - r.z.0));
        height.push(m.eave_y - m.floor_y);
    }
    let mut g = Geometry::from_polys(pts, &polys);
    for (k, v) in [("room", id), ("row", row), ("col", col), ("area", area), ("height", height)] {
        g.prim.set(k, Column::from_f32(v));
    }
    g
}
