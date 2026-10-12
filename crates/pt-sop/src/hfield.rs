//! Heightfield toolkit: nested levels of detail that meet without a crack.
//!
//! A world is seen from one metre and from ten kilometres, so one grid cannot carry it. These nodes build it as nested rings:
//! a coarse outer grid, a finer grid inside a hole in it, and so on down to a hero patch around a camera.
//!
//! * `hf_fit` bends the edge band of an inner heightfield onto the outer grid's surface, so the two meet exactly;
//! * `hf_hole` cuts the cells of a grid that the inner level replaces;
//! * `hf_refine` makes the finer level from the grid itself: smooth (bicubic) heights at a smaller cell, every point attribute
//!   carried across, and an edge that already fits the hole.
//!
//! The seam is exact because both sides are linear along a grid line of the coarser grid: the coarse side by construction, the
//! fine side because its edge points take the coarse grid's bilinear height there.
//!
//! Deterministic: only IEEE arithmetic on indices and heights.

use crate::{ps, sch};
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![
        sch(
            "hf_fit",
            "Seam between two levels of terrain: bend the edge band of the inner heightfield (input 0) onto the surface of the coarser grid around it (input 1, a full grid, before its hole is cut), so the two meshes meet with no crack. Inside the band the inner detail fades in over `blend` metres. Adjusts `elev` too. The inner rectangle's edges should lie on grid lines of the outer grid.",
            2,
            D::None,
            G::Whole,
            vec![ps("blend", json!(60.0), "Width in metres of the band along the inner edge over which the inner heights fade onto the outer surface.")],
        ),
        sch(
            "hf_hole",
            "Cut a rectangle of cells out of a grid heightfield where a finer level replaces it. The rectangle is snapped to grid lines; points no cell uses are dropped. The result is no longer a regular grid.",
            1,
            D::None,
            G::Whole,
            vec![
                ps("size", json!([100.0, 100.0]), "Hole size in metres: [x extent, z extent]."),
                ps("center", json!([0.0, 0.0]), "Hole centre [x, z] in metres."),
            ],
        ),
        sch(
            "hf_refine",
            "A finer grid over a rectangle of a grid heightfield: the hero patch around a camera. Heights are interpolated smoothly (bicubic) and blended onto the coarse surface at the edge, so the patch fits the hole that `hf_hole` cuts with the same size and centre. Every float and vector point attribute is carried across (bilinear), so masks and weights painted on the coarse grid arrive on the patch. Add fine relief after it with a wrangle that fades at the edge (`@edge` is the distance in metres to the patch edge).",
            1,
            D::None,
            G::Whole,
            vec![
                ps("size", json!([100.0, 100.0]), "Patch size in metres: [x extent, z extent] (snapped to the coarse grid lines)."),
                ps("center", json!([0.0, 0.0]), "Patch centre [x, z] in metres."),
                ps("cell", json!(0.5), "Wanted cell size in metres; the coarse cell is split into a whole number of cells close to this."),
                ps("blend", json!(8.0), "Width in metres of the edge band where the smooth heights fade onto the coarse surface."),
            ],
        ),
    ]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    let inp = |i: usize| inputs.get(i).and_then(|g| g.as_deref()).ok_or_else(|| format!("{kind}: input {i} is not connected"));
    match kind {
        "hf_fit" => Some(inp(0).and_then(|a| inp(1).and_then(|b| fit(a, b, p)))),
        "hf_hole" => Some(inp(0).and_then(|g| hole(g, p))),
        "hf_refine" => Some(inp(0).and_then(|g| refine(g, p))),
        _ => None,
    }
}

/// Layout of a regular grid heightfield: `(cols + 1) x (rows + 1)` points, row-major, rows along +Z.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub x0: f32,
    pub z0: f32,
    pub dx: f32,
    pub dz: f32,
}

impl Grid {
    pub fn of(g: &Geometry, who: &str) -> Result<Grid, String> {
        let d = g.detail.get("grid").ok_or_else(|| format!("{who}: input is not a grid heightfield (no grid layout recorded; a ring with a hole is not a grid)"))?;
        let (rows, cols) = (d["rows"].as_u64().unwrap_or(0) as usize, d["cols"].as_u64().unwrap_or(0) as usize);
        let p = g.p();
        if rows < 1 || cols < 1 || p.len() != (rows + 1) * (cols + 1) {
            return Err(format!("{who}: grid layout is inconsistent ({} points for {rows}x{cols})", p.len()));
        }
        let last = p[p.len() - 1];
        let (dx, dz) = ((last[0] - p[0][0]) / cols as f32, (last[2] - p[0][2]) / rows as f32);
        if dx <= 0.0 || dz <= 0.0 {
            return Err(format!("{who}: grid must run toward +X and +Z"));
        }
        Ok(Grid { rows, cols, x0: p[0][0], z0: p[0][2], dx, dz })
    }

    /// Cell index and fraction of a position, clamped to the grid.
    fn locate(&self, x: f32, z: f32) -> (usize, usize, f32, f32) {
        let (gx, gz) = (((x - self.x0) / self.dx).clamp(0.0, self.cols as f32), ((z - self.z0) / self.dz).clamp(0.0, self.rows as f32));
        let (i, j) = ((gx.floor() as usize).min(self.cols - 1), (gz.floor() as usize).min(self.rows - 1));
        (i, j, gx - i as f32, gz - j as f32)
    }

    fn at(&self, i: usize, j: usize) -> usize {
        j * (self.cols + 1) + i
    }

    /// Bilinear value of a per-point scalar at (x, z).
    pub fn bilinear(&self, v: &dyn Fn(usize) -> f32, x: f32, z: f32) -> f32 {
        let (i, j, fx, fz) = self.locate(x, z);
        let top = v(self.at(i, j)) * (1.0 - fx) + v(self.at(i + 1, j)) * fx;
        let bot = v(self.at(i, j + 1)) * (1.0 - fx) + v(self.at(i + 1, j + 1)) * fx;
        top * (1.0 - fz) + bot * fz
    }

    /// Bicubic (Catmull-Rom) value of a per-point scalar at (x, z); the border repeats its last row and column.
    pub fn bicubic(&self, v: &dyn Fn(usize) -> f32, x: f32, z: f32) -> f32 {
        let (i, j, fx, fz) = self.locate(x, z);
        let cr = |a: f32, b: f32, c: f32, d: f32, t: f32| b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)));
        let ci = |k: i64| (i as i64 + k).clamp(0, self.cols as i64) as usize;
        let cj = |k: i64| (j as i64 + k).clamp(0, self.rows as i64) as usize;
        let row = |k: i64| cr(v(self.at(ci(-1), cj(k))), v(self.at(ci(0), cj(k))), v(self.at(ci(1), cj(k))), v(self.at(ci(2), cj(k))), fx);
        cr(row(-1), row(0), row(1), row(2), fz)
    }

    /// Nearest grid line index (0..=n) to a coordinate.
    fn line(&self, v: f32, origin: f32, step: f32, n: usize) -> usize {
        (((v - origin) / step).round().max(0.0) as usize).min(n)
    }

    /// A centred rectangle snapped to grid lines: (i0, i1, j0, j1) with i0 < i1 and j0 < j1.
    fn rect(&self, p: &Params, who: &str) -> Result<(usize, usize, usize, usize), String> {
        let (size, c) = (p.v3("size"), p.v3("center"));
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return Err(format!("{who}: `size` must be positive metres, e.g. [200, 200]"));
        }
        let (i0, i1) = (self.line(c[0] - size[0] / 2.0, self.x0, self.dx, self.cols), self.line(c[0] + size[0] / 2.0, self.x0, self.dx, self.cols));
        let (j0, j1) = (self.line(c[1] - size[1] / 2.0, self.z0, self.dz, self.rows), self.line(c[1] + size[1] / 2.0, self.z0, self.dz, self.rows));
        if i1 <= i0 || j1 <= j0 {
            return Err(format!("{who}: the rectangle (size {size:?} at {c:?}) covers no cell of the grid"));
        }
        Ok((i0, i1, j0, j1))
    }
}

fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fit(inner: &Geometry, outer: &Geometry, p: &Params) -> Res {
    let og = Grid::of(outer, "hf_fit (input 1, the outer grid)")?;
    let (lo, hi) = inner.base_bounds().ok_or("hf_fit: input 0 is empty")?;
    let blend = p.f("blend").max(1e-3);
    let oh = outer.p();
    let height = |k: usize| oh[k][1];
    let mut out = inner.clone();
    let mut delta = vec![0.0f32; inner.n_points()];
    for (q, d) in out.p_mut().iter_mut().zip(delta.iter_mut()) {
        let edge = (q[0] - lo[0]).min(hi[0] - q[0]).min(q[2] - lo[2]).min(hi[2] - q[2]).max(0.0);
        let wt = smooth01(edge / blend);
        if wt < 1.0 {
            let target = og.bilinear(&height, q[0], q[2]);
            *d = (target - q[1]) * (1.0 - wt);
            q[1] += *d;
        }
    }
    if let Some(Column::F32(e)) = out.point.0.get_mut("elev") {
        for (v, d) in Arc::make_mut(e).iter_mut().zip(&delta) {
            *v += d;
        }
    }
    Ok(out)
}

fn hole(g: &Geometry, p: &Params) -> Res {
    let grid = Grid::of(g, "hf_hole")?;
    let (i0, i1, j0, j1) = grid.rect(p, "hf_hole")?;
    let mut prim_keep = vec![true; g.n_prims()];
    if g.n_prims() != grid.rows * grid.cols {
        return Err(format!("hf_hole: expected {} cells, the grid has {} primitives", grid.rows * grid.cols, g.n_prims()));
    }
    for j in j0..j1 {
        for i in i0..i1 {
            prim_keep[j * grid.cols + i] = false;
        }
    }
    let mut point_keep = vec![false; g.n_points()];
    for (r, &keep) in prim_keep.iter().enumerate() {
        if keep {
            for &q in g.prim_points(r) {
                point_keep[q as usize] = true;
            }
        }
    }
    let mut out = g.filter(&point_keep, &prim_keep);
    out.detail.remove("grid");
    out.detail.insert("hole".into(), json!({"x": [grid.x0 + i0 as f32 * grid.dx, grid.x0 + i1 as f32 * grid.dx], "z": [grid.z0 + j0 as f32 * grid.dz, grid.z0 + j1 as f32 * grid.dz]}));
    Ok(out)
}

fn refine(g: &Geometry, p: &Params) -> Res {
    let grid = Grid::of(g, "hf_refine")?;
    let (i0, i1, j0, j1) = grid.rect(p, "hf_refine")?;
    let cell = p.f("cell").max(1e-3);
    let (kx, kz) = (((grid.dx / cell).round() as usize).clamp(1, 64), ((grid.dz / cell).round() as usize).clamp(1, 64));
    let (cols, rows) = ((i1 - i0) * kx, (j1 - j0) * kz);
    if (rows + 1) * (cols + 1) > 16_000_000 {
        return Err(format!("hf_refine: {rows} x {cols} cells is more than 16 million points; use a smaller `size` or a larger `cell`"));
    }
    let blend = p.f("blend").max(1e-3);
    let src = g.p();
    let height = |k: usize| src[k][1];
    // Coordinates come from the coarse grid's own vertices (plus a fraction of a cell), so a patch vertex that shares a place
    // with a coarse vertex has exactly its coordinates.
    let coord = |fine: usize, k: usize, first: usize, last: usize, axis: usize| -> f32 {
        let (c, frac) = (first + fine / k, (fine % k) as f32 / k as f32);
        let at = |ci: usize| if axis == 0 { src[grid.at(ci, 0)][0] } else { src[grid.at(0, ci)][2] };
        let (a, b) = (at(c), at((c + 1).min(last)));
        a + (b - a) * frac
    };
    let (x_lo, x_hi) = (coord(0, kx, i0, grid.cols, 0), coord(cols, kx, i0, grid.cols, 0));
    let (z_lo, z_hi) = (coord(0, kz, j0, grid.rows, 2), coord(rows, kz, j0, grid.rows, 2));
    let (w, d) = (x_hi - x_lo, z_hi - z_lo);
    let wc = cols + 1;
    let n = (rows + 1) * wc;
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut edge: Vec<f32> = Vec::with_capacity(n);
    for j in 0..=rows {
        let z = coord(j, kz, j0, grid.rows, 2);
        for i in 0..=cols {
            let x = coord(i, kx, i0, grid.cols, 0);
            let e = if i == 0 || j == 0 || i == cols || j == rows { 0.0 } else { (x - x_lo).min(x_hi - x).min(z - z_lo).min(z_hi - z).max(0.0) };
            let lin = grid.bilinear(&height, x, z);
            let wt = smooth01(e / blend);
            let y = if wt > 0.0 { lin + (grid.bicubic(&height, x, z) - lin) * wt } else { lin };
            pts.push([x, y, z]);
            edge.push(e);
        }
    }
    let mut polys: Vec<Vec<u32>> = Vec::with_capacity(rows * cols);
    for j in 0..rows {
        for i in 0..cols {
            let a = (j * wc + i) as u32;
            polys.push(vec![a, a + wc as u32, a + wc as u32 + 1, a + 1]);
        }
    }
    let mut out = Geometry::from_polys(pts.clone(), &polys);
    for (name, col) in g.point.iter() {
        if name == "P" {
            continue;
        }
        match col {
            Column::F32(v) => {
                let f = |k: usize| v[k];
                out.point.set(name, Column::from_f32(pts.iter().map(|q| grid.bilinear(&f, q[0], q[2])).collect()));
            }
            Column::V3(v) => {
                let comp = |a: usize| -> Vec<f32> {
                    let f = |k: usize| v[k][a];
                    pts.iter().map(|q| grid.bilinear(&f, q[0], q[2])).collect()
                };
                let (cx, cy, cz) = (comp(0), comp(1), comp(2));
                out.point.set(name, Column::from_v3((0..n).map(|k| [cx[k], cy[k], cz[k]]).collect()));
            }
            _ => {}
        }
    }
    out.point.set("edge", Column::from_f32(edge));
    out.detail = g.detail.clone();
    out.detail.insert("grid".into(), json!({"rows": rows, "cols": cols}));
    out.detail.insert("refined".into(), json!({"x": [x_lo, x_lo + w], "z": [z_lo, z_lo + d], "cell_m": [grid.dx / kx as f32, grid.dz / kz as f32]}));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid over [-size/2, size/2] with height f(x, z) and an attribute `m` = x.
    fn field(n: usize, size: f32, f: impl Fn(f32, f32) -> f32) -> Geometry {
        let mut pts = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                let (x, z) = ((i as f32 / n as f32 - 0.5) * size, (j as f32 / n as f32 - 0.5) * size);
                pts.push([x, f(x, z), z]);
            }
        }
        let w = n as u32 + 1;
        let mut polys = Vec::new();
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let a = j * w + i;
                polys.push(vec![a, a + w, a + w + 1, a + 1]);
            }
        }
        let mut g = Geometry::from_polys(pts.clone(), &polys);
        g.point.set("m", Column::from_f32(pts.iter().map(|q| q[0]).collect()));
        g.detail.insert("grid".into(), json!({"rows": n, "cols": n}));
        g
    }

    fn params(kv: &[(&str, serde_json::Value)]) -> Params {
        let mut p = Params::new();
        for (k, v) in kv {
            p.insert((*k).into(), v.clone());
        }
        p
    }

    #[test]
    fn a_refined_patch_fits_the_hole_and_carries_attributes() {
        let coarse = field(20, 200.0, |x, z| (x * 0.07).sin() * 6.0 + (z * 0.05).cos() * 4.0); // 10 m cells
        let pr = params(&[("size", json!([60.0, 40.0])), ("center", json!([10.0, -10.0])), ("cell", json!(2.0)), ("blend", json!(8.0))]);
        let patch = refine(&coarse, &pr).unwrap();
        let ring = hole(&coarse, &pr).unwrap();
        // 60 x 40 m at 10 m cells = 6 x 4 cells cut; each split in 5
        assert_eq!(ring.n_prims(), 400 - 24);
        assert_eq!(patch.detail["grid"], json!({"rows": 20, "cols": 30}));
        assert!(!ring.detail.contains_key("grid"));
        let grid = Grid::of(&coarse, "t").unwrap();
        let h = |k: usize| coarse.p()[k][1];
        let edge = patch.point.get("edge").unwrap().as_f32().unwrap();
        let (mut on_edge, mut inside_diff) = (0, 0.0f32);
        for (q, &e) in patch.p().iter().zip(edge) {
            let lin = grid.bilinear(&h, q[0], q[2]);
            if e < 1e-4 {
                on_edge += 1;
                assert!((q[1] - lin).abs() < 1e-5, "an edge point lies on the coarse surface: {} vs {lin}", q[1]);
            } else if e > 8.0 {
                inside_diff = inside_diff.max((q[1] - lin).abs());
            }
        }
        assert_eq!(on_edge, 2 * (30 + 20));
        assert!(inside_diff > 0.01, "inside, the bicubic surface differs from the faceted one: {inside_diff}");
        // the attribute m = x is linear, so bilinear carries it exactly
        let m = patch.point.get("m").unwrap().as_f32().unwrap();
        assert!(patch.p().iter().zip(m).all(|(q, &v)| (q[0] - v).abs() < 1e-4));
        // every coarse vertex on the hole's rim is also a patch vertex (no T-junction without a partner)
        let rim: Vec<[f32; 3]> = ring.p().iter().copied().filter(|q| q[0] >= -20.0 - 1e-3 && q[0] <= 40.0 + 1e-3 && q[2] >= -30.0 - 1e-3 && q[2] <= 10.0 + 1e-3).collect();
        assert!(!rim.is_empty());
        for r in rim {
            assert!(patch.p().iter().any(|q| (q[0] - r[0]).abs() < 1e-4 && (q[2] - r[2]).abs() < 1e-4 && (q[1] - r[1]).abs() < 1e-4), "rim vertex {r:?} has no partner");
        }
        // same input, same patch
        assert_eq!(refine(&coarse, &pr).unwrap().content_hash(), patch.content_hash());
        assert!(refine(&coarse, &params(&[("size", json!([0.0, 10.0])), ("center", json!([0.0, 0.0])), ("cell", json!(1.0)), ("blend", json!(1.0))])).is_err());
    }

    #[test]
    fn fitting_puts_the_inner_edge_on_the_outer_surface_and_keeps_the_middle() {
        let outer = field(10, 400.0, |x, _| x * 0.1); // 40 m cells, a ramp
        let mut inner = field(40, 80.0, |x, z| x * 0.1 + 3.0 + (z * 0.3).sin()); // 2 m cells, 3 m higher and wavy
        inner.point.set("elev", Column::from_f32(inner.p().iter().map(|q| q[1] + 1000.0).collect()));
        let fitted = fit(&inner, &outer, &params(&[("blend", json!(10.0))])).unwrap();
        let elev = fitted.point.get("elev").unwrap().as_f32().unwrap();
        for ((q, orig), e) in fitted.p().iter().zip(inner.p()).zip(elev) {
            let edge = (q[0] + 40.0).min(40.0 - q[0]).min(q[2] + 40.0).min(40.0 - q[2]);
            if edge < 1e-4 {
                assert!((q[1] - q[0] * 0.1).abs() < 1e-4, "edge on the ramp: {q:?}");
            }
            if edge >= 10.0 {
                assert_eq!(q[1], orig[1], "the middle is untouched");
            }
            assert!((e - (q[1] + 1000.0)).abs() < 1e-2, "elev follows the height");
        }
        assert!(fit(&inner, &hole(&outer, &params(&[("size", json!([80.0, 80.0])), ("center", json!([0.0, 0.0]))])).unwrap(), &params(&[("blend", json!(10.0))])).unwrap_err().contains("not a grid"));
    }
}
