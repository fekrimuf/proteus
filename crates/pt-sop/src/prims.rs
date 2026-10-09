//! Primitive generators and instance-style transform ops.

use crate::{ps, sch};
use pt_core::math::{self, Mat34};
use pt_core::{Differentiability as D, Geometry, Granularity as G, Instances, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch("tube", "Cylinder/cone/frustum along Y. radius_top < 0 means same as radius; radius_top = 0 makes a cone.", 0, D::Smooth, G::Whole, vec![
            ps("radius", json!(0.5), "Bottom radius (m)."),
            ps("radius_top", json!(-1.0), "Top radius (<0 = same as bottom)."),
            ps("height", json!(1.0), "Height (m)."),
            ps("rows", json!(1), "Height divisions."),
            ps("cols", json!(16), "Radial divisions (>=3)."),
            ps("caps", json!(1), "1 = close the ends."),
            ps("center", json!([0.0, 0.0, 0.0]), "Center."),
        ]),
        sch("torus", "Torus around Y.", 0, D::Smooth, G::Whole, vec![
            ps("radius", json!(1.0), "Major radius (m)."),
            ps("minor_radius", json!(0.25), "Tube radius (m)."),
            ps("rows", json!(12), "Divisions around the tube."),
            ps("cols", json!(24), "Divisions around the ring."),
            ps("center", json!([0.0, 0.0, 0.0]), "Center."),
        ]),
        sch("circle", "Planar n-gon in the XZ plane (normal +Y).", 0, D::Smooth, G::Whole, vec![
            ps("radius", json!(1.0), "Radius (m)."),
            ps("divisions", json!(16), "Points (>=3)."),
            ps("center", json!([0.0, 0.0, 0.0]), "Center."),
        ]),
        sch("line", "Polyline made of 2-point segment primitives.", 0, D::Smooth, G::Whole, vec![
            ps("from", json!([0.0, 0.0, 0.0]), "Start."),
            ps("to", json!([0.0, 1.0, 0.0]), "End."),
            ps("segments", json!(1), "Number of segments."),
        ]),
        sch("mirror", "Mirror across an axis-aligned plane (flips winding), optionally keeping the original.", 1, D::Smooth, G::Whole, vec![
            ps("axis", json!("x"), "x | y | z."),
            ps("offset", json!(0.0), "Plane position along the axis."),
            ps("keep_original", json!(1), "1 = output original + mirror."),
        ]),
        sch("array", "Linear array: N lazy instances with accumulated offset/rotation/scale.", 1, D::Smooth, G::Whole, vec![
            ps("count", json!(5), "Number of copies."),
            ps("offset", json!([1.0, 0.0, 0.0]), "Translation per copy (m)."),
            ps("rotate", json!([0.0, 0.0, 0.0]), "Rotation per copy (degrees)."),
            ps("scale", json!([1.0, 1.0, 1.0]), "Scale factor per copy."),
        ]),
        sch("radial_array", "Place N lazy instances around the Y axis on a circle; +X of each copy faces outward.", 1, D::Smooth, G::Whole, vec![
            ps("count", json!(8), "Number of copies."),
            ps("radius", json!(2.0), "Circle radius (m)."),
            ps("start_angle", json!(0.0), "Start angle (degrees)."),
            ps("sweep", json!(360.0), "Total sweep (degrees); 360 = closed ring."),
        ]),
        sch("center", "Recenter geometry including instances: mode 'center' (all axes) or 'ground' (center XZ, sit on y=0).", 1, D::Smooth, G::PerPoint, vec![
            ps("mode", json!("ground"), "center | ground."),
        ]),
        sch("bound", "Box around the input bounds (including instances).", 1, D::Smooth, G::Whole, vec![
            ps("padding", json!(0.0), "Extra margin on all sides (m)."),
        ]),
    ]
}

fn axis_index(s: &str) -> Result<usize, String> {
    match s {
        "x" => Ok(0),
        "y" => Ok(1),
        "z" => Ok(2),
        o => Err(format!("axis must be x, y or z, got '{o}'")),
    }
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| -> Result<&Geometry, String> {
        inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"))
    };
    Some(match kind {
        "tube" => Ok(tube(p)),
        "torus" => Ok(torus(p)),
        "circle" => Ok(circle(p)),
        "line" => Ok(line(p)),
        "mirror" => inp(0).and_then(|g| mirror(g, p)),
        "array" => inp(0).map(|g| array(g, p)),
        "radial_array" => inp(0).map(|g| radial_array(g, p)),
        "center" => inp(0).map(|g| center(g, p)),
        "bound" => inp(0).map(|g| bound(g, p)),
        _ => return None,
    })
}

fn tube(p: &Params) -> Geometry {
    let r0 = p.f("radius");
    let r1 = if p.f("radius_top") < 0.0 { r0 } else { p.f("radius_top") };
    let h = p.f("height");
    let rows = p.i("rows").max(1) as u32;
    let cols = p.i("cols").max(3) as u32;
    let c = p.v3("center");
    let mut pts = Vec::new();
    for j in 0..=rows {
        let t = j as f32 / rows as f32;
        let r = r0 + (r1 - r0) * t;
        let y = c[1] + (t - 0.5) * h;
        for i in 0..cols {
            let (s, co) = math::sin_cos(2.0 * math::PI * i as f32 / cols as f32);
            pts.push([c[0] + r * co, y, c[2] + r * s]);
        }
    }
    let at = |j: u32, i: u32| j * cols + i % cols;
    let mut polys: Vec<Vec<u32>> = Vec::new();
    for j in 0..rows {
        for i in 0..cols {
            polys.push(vec![at(j, i), at(j + 1, i), at(j + 1, i + 1), at(j, i + 1)]);
        }
    }
    if p.i("caps") != 0 {
        polys.push((0..cols).map(|i| at(0, i)).collect());
        polys.push((0..cols).rev().map(|i| at(rows, i)).collect());
    }
    Geometry::from_polys(pts, &polys)
}

fn torus(p: &Params) -> Geometry {
    let (big, small) = (p.f("radius"), p.f("minor_radius"));
    let rows = p.i("rows").max(3) as u32;
    let cols = p.i("cols").max(3) as u32;
    let c = p.v3("center");
    let mut pts = Vec::new();
    for i in 0..cols {
        let (su, cu) = math::sin_cos(2.0 * math::PI * i as f32 / cols as f32);
        for j in 0..rows {
            let (sv, cv) = math::sin_cos(2.0 * math::PI * j as f32 / rows as f32);
            let r = big + small * cv;
            pts.push([c[0] + r * cu, c[1] + small * sv, c[2] + r * su]);
        }
    }
    let at = |i: u32, j: u32| (i % cols) * rows + j % rows;
    let mut polys = Vec::new();
    for i in 0..cols {
        for j in 0..rows {
            polys.push(vec![at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j)]);
        }
    }
    Geometry::from_polys(pts, &polys)
}

fn circle(p: &Params) -> Geometry {
    let n = p.i("divisions").max(3) as u32;
    let (r, c) = (p.f("radius"), p.v3("center"));
    let pts: Vec<[f32; 3]> = (0..n)
        .map(|i| {
            let (s, co) = math::sin_cos(2.0 * math::PI * i as f32 / n as f32);
            [c[0] + r * co, c[1], c[2] + r * s]
        })
        .collect();
    Geometry::from_polys(pts, &[(0..n).rev().collect()])
}

fn line(p: &Params) -> Geometry {
    let (a, b) = (p.v3("from"), p.v3("to"));
    let n = p.i("segments").max(1) as u32;
    let pts: Vec<[f32; 3]> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
        })
        .collect();
    let polys: Vec<Vec<u32>> = (0..n).map(|i| vec![i, i + 1]).collect();
    Geometry::from_polys(pts, &polys)
}

fn mirror(src: &Geometry, p: &Params) -> Res {
    let ax = axis_index(&p.s("axis"))?;
    let off = p.f("offset");
    let mut m = src.clone();
    for q in m.p_mut() {
        q[ax] = 2.0 * off - q[ax];
    }
    let polys: Vec<Vec<u32>> = (0..src.n_prims()).map(|i| src.prim_points(i).iter().rev().copied().collect()).collect();
    let mut flipped = m.with_polys(&polys);
    flipped.prim = src.prim.clone();
    if p.i("keep_original") != 0 {
        let mut out = src.clone();
        out.append(&flipped)?;
        Ok(out)
    } else {
        Ok(flipped)
    }
}

fn instances_of(src: &Geometry, xf: Vec<Mat34>) -> Geometry {
    let mut g = Geometry::new();
    g.instances = Some(Instances { protos: vec![Arc::new(src.clone())], proto_id: Arc::new(vec![0; xf.len()]), xf: Arc::new(xf), attrs: Default::default() });
    g
}

fn array(src: &Geometry, p: &Params) -> Geometry {
    let n = p.i("count").max(0) as usize;
    let (off, rot, sc) = (p.v3("offset"), p.v3("rotate"), p.v3("scale"));
    let mut xf = Vec::with_capacity(n);
    let mut s = [1.0f32; 3];
    for i in 0..n {
        let f = i as f32;
        xf.push(math::trs([off[0] * f, off[1] * f, off[2] * f], [rot[0] * f, rot[1] * f, rot[2] * f], s));
        s = [s[0] * sc[0], s[1] * sc[1], s[2] * sc[2]];
    }
    instances_of(src, xf)
}

fn radial_array(src: &Geometry, p: &Params) -> Geometry {
    let n = p.i("count").max(0) as usize;
    let (r, start, sweep) = (p.f("radius"), p.f("start_angle"), p.f("sweep"));
    // A full ring divides by n (no duplicate at 360); a partial arc includes both ends.
    let div = if sweep.abs() >= 359.999 || n < 2 { n.max(1) as f32 } else { (n - 1) as f32 };
    let xf: Vec<Mat34> = (0..n)
        .map(|i| {
            let a = start + sweep * i as f32 / div;
            let (s, c) = math::sin_cos(math::to_radians(a));
            math::trs([r * c, 0.0, r * s], [0.0, -a, 0.0], [1.0, 1.0, 1.0])
        })
        .collect();
    instances_of(src, xf)
}

fn center(src: &Geometry, p: &Params) -> Geometry {
    let Some((lo, hi)) = src.bounds() else { return src.clone() };
    let ground = p.s("mode") != "center";
    let t = [-(lo[0] + hi[0]) * 0.5, if ground { -lo[1] } else { -(lo[1] + hi[1]) * 0.5 }, -(lo[2] + hi[2]) * 0.5];
    crate::kernel::apply(src, &[crate::kernel::PointOp::Affine(math::trs(t, [0.0; 3], [1.0; 3]))])
}

fn bound(src: &Geometry, p: &Params) -> Geometry {
    let Some((lo, hi)) = src.bounds() else { return Geometry::new() };
    let pad = p.f("padding");
    let mut bp = Params::new();
    bp.insert("size".into(), json!([hi[0] - lo[0] + 2.0 * pad, hi[1] - lo[1] + 2.0 * pad, hi[2] - lo[2] + 2.0 * pad]));
    bp.insert("center".into(), json!([(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5]));
    crate::make_box(&bp)
}
