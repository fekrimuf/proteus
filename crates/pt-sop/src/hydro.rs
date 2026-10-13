//! `hf_flow`: hydrology on a grid heightfield. Depressions are filled with the priority-flood algorithm (Barnes et al. 2014: always
//! expand the lowest unvisited cell, so every basin is flooded up to its lowest rim), and each cell drains to the neighbour that first
//! reached it, which gives a valid drainage tree even across flat lakes and flats. Accumulating that tree downstream gives the catchment
//! area of every cell. From these the node writes the masks a placement or material rule needs: streams, wetness, lakes.
//!
//! Deterministic: ties in the queue break by cell index, accumulation runs in flood order, and nothing depends on thread count.

use crate::{ps, sch};
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::json;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;

type Res = Result<Geometry, String>;
type Inputs<'a> = &'a [Option<Arc<Geometry>>];

pub fn schemas() -> Vec<NodeSchema> {
    vec![sch(
        "hf_flow",
        "Hydrology on a grid heightfield: fill depressions (priority-flood), drain every cell downhill, accumulate the catchment. Writes point attrs `filled` (height with basins filled), `fill_depth` (metres below the filled level: ponds and lakes), `flow_acc` (upstream catchment area in m2), `twi` (topographic wetness index, ln(area / tan slope): high in hollows and along streams), `stream` (0..1 soft mask where the catchment passes `stream_area`) and `lake` (1 where fill_depth exceeds `lake_depth`). Run it on a window larger than the area you dress, so streams that cross the edge have their upstream area, then copy the masks across with attribute_transfer.",
        1,
        D::None,
        G::Whole,
        vec![
            ps("stream_area", json!(30000.0), "Catchment area in m2 where a stream starts (30000 = 0.03 km2)."),
            ps("lake_depth", json!(0.5), "Fill depth in metres above which a cell is lake."),
            ps("level_lakes", json!(0), "1 = raise lake cells to their filled height, so the surface of a lake is flat."),
            ps("river_width", json!(0.0), "If > 0: give the streams a channel. Width in metres where a stream starts; it grows with the square root of the catchment up to `river_width_max`. Adds point attrs `river_sd` (metres to the channel edge, negative inside the channel), `river_w` (channel width there), `water_y` (height of the water surface) and `river` (1 in the channel, falling to 0 across the bank). Carve the bed and build the water surface from these in a wrangle."),
            ps("river_width_max", json!(12.0), "Largest channel width in metres."),
            ps("river_bank", json!(3.0), "Metres over which the bank rises from the channel edge to untouched ground (width of the `river` mask's falloff)."),
            ps("river_freeboard", json!(0.3), "Metres the water surface lies below the ground at the channel's centre line."),
        ],
    )]
}

pub fn cook(kind: &str, p: &Params, inputs: Inputs) -> Option<Res> {
    match kind {
        "hf_flow" => Some(inputs.first().and_then(|g| g.as_deref()).ok_or_else(|| "hf_flow: input 0 is not connected".to_string()).and_then(|g| flow(g, p))),
        _ => None,
    }
}

/// Monotone integer key of a finite f32, so the heap orders by height and then by index.
fn key(h: f32) -> u32 {
    let b = h.to_bits();
    if b >> 31 == 1 { !b } else { b | 0x8000_0000 }
}

pub struct Hydro {
    pub filled: Vec<f32>,
    /// Catchment area in m2 including the cell itself.
    pub acc: Vec<f32>,
    /// Index of the cell this one drains into, or `u32::MAX` for an outlet on the border.
    pub parent: Vec<u32>,
}

/// Priority-flood fill, drainage tree and accumulation on a `(rows+1) x (cols+1)` grid of heights with cell size `dx` x `dz`.
pub fn hydrology(h: &[f32], rows: usize, cols: usize, dx: f32, dz: f32) -> Hydro {
    let (w, n) = (cols + 1, h.len());
    let mut filled = h.to_vec();
    let mut parent = vec![u32::MAX; n];
    let mut seen = vec![false; n];
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for j in 0..=rows {
        for i in 0..=cols {
            if i == 0 || j == 0 || i == cols || j == rows {
                let c = j * w + i;
                seen[c] = true;
                heap.push(Reverse((key(h[c]), c as u32)));
            }
        }
    }
    let mut order: Vec<u32> = Vec::with_capacity(n);
    while let Some(Reverse((_, c))) = heap.pop() {
        let c = c as usize;
        order.push(c as u32);
        let (i, j) = ((c % w) as i64, (c / w) as i64);
        for dj in -1i64..=1 {
            for di in -1i64..=1 {
                if di == 0 && dj == 0 {
                    continue;
                }
                let (ni, nj) = (i + di, j + dj);
                if ni < 0 || nj < 0 || ni > cols as i64 || nj > rows as i64 {
                    continue;
                }
                let m = nj as usize * w + ni as usize;
                if seen[m] {
                    continue;
                }
                seen[m] = true;
                filled[m] = filled[m].max(filled[c]);
                parent[m] = c as u32;
                heap.push(Reverse((key(filled[m]), m as u32)));
            }
        }
    }
    let mut acc = vec![dx * dz; n];
    for &c in order.iter().rev() {
        let p = parent[c as usize];
        if p != u32::MAX {
            let a = acc[c as usize];
            acc[p as usize] += a;
        }
    }
    Hydro { filled, acc, parent }
}

fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub struct RiverOpts {
    pub stream_area: f32,
    pub width: f32,
    pub width_max: f32,
    pub bank: f32,
    pub freeboard: f32,
}

pub struct Rivers {
    /// Metres to the edge of the nearest channel or lake (negative inside it); capped at the search reach far from water.
    pub sd: Vec<f32>,
    /// Width of that channel in metres (0 far from water).
    pub width: Vec<f32>,
    /// Height of the water surface near water; two metres under the ground elsewhere, so a surface built from it stays hidden.
    pub water_y: Vec<f32>,
}

/// Channels along the drainage tree. A stream cell (catchment over `stream_area`) is a point of a centre line; its channel is as
/// wide as `width * sqrt(catchment / stream_area)` (hydraulic geometry: width grows with the square root of the discharge), and
/// its water lies `freeboard` under the filled ground there, which never rises downstream. Every cell within reach belongs to
/// the channel whose EDGE is nearest, so a creek that joins a river does not pinch it. Lakes count as water with their filled level.
pub fn rivers(h: &[f32], hy: &Hydro, lake: &[f32], rows: usize, cols: usize, dx: f32, dz: f32, o: &RiverOpts) -> Rivers {
    let (w, n) = (cols + 1, h.len());
    let reach = o.bank * 3.0 + 6.0;
    let mut sd = vec![f32::MAX; n];
    let mut owner = vec![u32::MAX; n];
    let mut own_width = vec![0.0f32; n];
    let mut level = vec![0.0f32; n];
    let mut stamp = |c: usize, half: f32, lvl: f32, sd: &mut Vec<f32>, owner: &mut Vec<u32>| {
        let (ci, cj) = ((c % w) as i64, (c / w) as i64);
        let r = half + reach;
        let (ri, rj) = ((r / dx).ceil() as i64, (r / dz).ceil() as i64);
        for dj in -rj..=rj {
            for di in -ri..=ri {
                let (ni, nj) = (ci + di, cj + dj);
                if ni < 0 || nj < 0 || ni > cols as i64 || nj > rows as i64 {
                    continue;
                }
                let (ex, ez) = (di as f32 * dx, dj as f32 * dz);
                let s = (ex * ex + ez * ez).sqrt() - half;
                let m = nj as usize * w + ni as usize;
                if s <= reach && s < sd[m] {
                    sd[m] = s;
                    owner[m] = c as u32;
                    own_width[m] = half * 2.0;
                    level[m] = lvl;
                }
            }
        }
    };
    for c in 0..n {
        if hy.acc[c] >= o.stream_area {
            let width = (o.width * (hy.acc[c] / o.stream_area).sqrt()).min(o.width_max);
            stamp(c, width * 0.5, hy.filled[c] - o.freeboard, &mut sd, &mut owner);
        } else if lake[c] > 0.5 {
            stamp(c, dx.max(dz) * 0.75, hy.filled[c], &mut sd, &mut owner);
        }
    }
    // Round the corners of the centre line's staircase (cells drain to one of eight neighbours) and let the water level run
    // smoothly down the channel: a few binomial passes over the cells within reach only.
    let blur = |v: &mut Vec<f32>, passes: usize| {
        for _ in 0..passes {
            let src = v.clone();
            for j in 0..=rows {
                for i in 0..=cols {
                    let c = j * w + i;
                    if owner[c] == u32::MAX {
                        continue;
                    }
                    let (mut sum, mut wt) = (0.0f32, 0.0f32);
                    for dj in -1i64..=1 {
                        for di in -1i64..=1 {
                            let (ni, nj) = (i as i64 + di, j as i64 + dj);
                            if ni < 0 || nj < 0 || ni > cols as i64 || nj > rows as i64 {
                                continue;
                            }
                            let m = nj as usize * w + ni as usize;
                            if owner[m] == u32::MAX {
                                continue;
                            }
                            let k = if di == 0 && dj == 0 { 4.0 } else if di == 0 || dj == 0 { 2.0 } else { 1.0 };
                            sum += src[m] * k;
                            wt += k;
                        }
                    }
                    v[c] = sum / wt;
                }
            }
        }
    };
    blur(&mut sd, 2);
    blur(&mut level, 6);
    let mut water_y = Vec::with_capacity(n);
    for c in 0..n {
        if owner[c] == u32::MAX {
            sd[c] = reach;
            water_y.push(h[c] - 2.0);
        } else {
            water_y.push(level[c]);
        }
    }
    Rivers { sd, width: own_width, water_y }
}

fn flow(g: &Geometry, p: &Params) -> Res {
    let d = g.detail.get("grid").ok_or("hf_flow: input is not a grid heightfield (no grid layout recorded)")?;
    let (rows, cols) = (d["rows"].as_u64().unwrap_or(0) as usize, d["cols"].as_u64().unwrap_or(0) as usize);
    if rows < 2 || cols < 2 || g.n_points() != (rows + 1) * (cols + 1) {
        return Err(format!("hf_flow: grid layout is inconsistent ({} points for {rows}x{cols})", g.n_points()));
    }
    let pts = g.p();
    let w = cols + 1;
    let (dx, dz) = ((pts[1][0] - pts[0][0]).abs().max(1e-6), (pts[w][2] - pts[0][2]).abs().max(1e-6));
    let h: Vec<f32> = pts.iter().map(|q| q[1]).collect();
    let hy = hydrology(&h, rows, cols, dx, dz);
    let (stream_area, lake_depth) = (p.f("stream_area").max(1.0), p.f("lake_depth").max(0.0));
    let n = h.len();
    let (mut fill_depth, mut twi, mut stream, mut lake) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for j in 0..=rows {
        for i in 0..=cols {
            let c = j * w + i;
            let (i0, i1, j0, j1) = (i.saturating_sub(1), (i + 1).min(cols), j.saturating_sub(1), (j + 1).min(rows));
            let gx = (h[j * w + i1] - h[j * w + i0]) / ((i1 - i0) as f32 * dx);
            let gz = (h[j1 * w + i] - h[j0 * w + i]) / ((j1 - j0) as f32 * dz);
            let tan = (gx * gx + gz * gz).sqrt();
            let depth = (hy.filled[c] - h[c]).max(0.0);
            fill_depth.push(depth);
            // wetness index: area per unit contour width over the slope (a small floor keeps flats finite)
            twi.push(pt_core::math::ln((hy.acc[c] / dx.min(dz)).max(1.0)) - pt_core::math::ln(tan + 0.02));
            stream.push(smooth01((hy.acc[c] / stream_area - 0.5) / 1.0));
            lake.push((depth > lake_depth) as u8 as f32);
        }
    }
    let mut out = g.clone();
    if p.i("level_lakes") != 0 {
        for (q, (&hf, &l)) in out.p_mut().iter_mut().zip(hy.filled.iter().zip(&lake)) {
            if l > 0.5 {
                q[1] = hf;
            }
        }
    }
    if p.f("river_width") > 0.0 {
        let r = rivers(&h, &hy, &lake, rows, cols, dx, dz, &RiverOpts { stream_area, width: p.f("river_width"), width_max: p.f("river_width_max").max(p.f("river_width")), bank: p.f("river_bank").max(0.1), freeboard: p.f("river_freeboard").max(0.0) });
        let bank = p.f("river_bank").max(0.1);
        out.point.set("river", Column::from_f32(r.sd.iter().map(|&s| 1.0 - smooth01(s / bank)).collect()));
        out.point.set("river_sd", Column::from_f32(r.sd));
        out.point.set("river_w", Column::from_f32(r.width));
        out.point.set("water_y", Column::from_f32(r.water_y));
    }
    out.point.set("filled", Column::from_f32(hy.filled));
    out.point.set("fill_depth", Column::from_f32(fill_depth));
    out.point.set("flow_acc", Column::from_f32(hy.acc));
    out.point.set("twi", Column::from_f32(twi));
    out.point.set("stream", Column::from_f32(stream));
    out.point.set("lake", Column::from_f32(lake));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A V-shaped valley: height = |x - mid| * 0.3 + (rows - j) * 0.1, so water runs to the centre line and then south... to row 0 is high.
    fn valley(rows: usize, cols: usize) -> Vec<f32> {
        let mid = cols as f32 / 2.0;
        (0..=rows).flat_map(|j| (0..=cols).map(move |i| (i as f32 - mid).abs() * 0.3 + (rows - j) as f32 * 0.1)).collect()
    }

    #[test]
    fn catchment_grows_down_the_valley_axis_and_every_cell_reaches_the_edge() {
        let (rows, cols) = (40usize, 40usize);
        let h = valley(rows, cols);
        let hy = hydrology(&h, rows, cols, 1.0, 1.0);
        let w = cols + 1;
        let axis = |j: usize| hy.acc[j * w + cols / 2];
        assert!(axis(rows - 5) > axis(10) * 2.0, "area accumulates downstream: {} vs {}", axis(rows - 5), axis(10));
        assert!(hy.acc[w + 2] < 20.0, "a ridge cell has a small catchment");
        // total area leaving through the border cells equals the whole grid
        let outlets: f32 = (0..h.len()).filter(|&c| hy.parent[c] == u32::MAX).map(|c| hy.acc[c]).sum();
        assert!((outlets - h.len() as f32).abs() < 1.0, "{outlets} vs {}", h.len());
        // no cell is its own ancestor: following parents always ends at an outlet
        for start in [w * 7 + 9, w * 30 + 3, w * 20 + 20] {
            let (mut c, mut steps) = (start as u32, 0);
            while hy.parent[c as usize] != u32::MAX && steps < 10_000 {
                c = hy.parent[c as usize];
                steps += 1;
            }
            assert!(steps < 10_000);
        }
    }

    #[test]
    fn a_channel_follows_the_valley_axis_widens_downstream_and_its_water_never_runs_uphill() {
        let (rows, cols) = (80usize, 80usize);
        let h = valley(rows, cols);
        let hy = hydrology(&h, rows, cols, 2.0, 2.0);
        let lake = vec![0.0f32; h.len()];
        let o = RiverOpts { stream_area: 400.0, width: 2.0, width_max: 9.0, bank: 2.0, freeboard: 0.3 };
        let r = rivers(&h, &hy, &lake, rows, cols, 2.0, 2.0, &o);
        let w = cols + 1;
        let axis = |j: usize| j * w + cols / 2;
        // on the axis, downstream (large j): inside the channel; a ridge cell far to the side: capped at the reach, water hidden
        assert!(r.sd[axis(70)] < 0.0, "{}", r.sd[axis(70)]);
        let side = 70 * w + 2;
        assert_eq!(r.sd[side], o.bank * 3.0 + 6.0);
        assert_eq!(r.water_y[side], h[side] - 2.0);
        assert!(r.width[axis(70)] > r.width[axis(30)], "wider downstream: {} vs {}", r.width[axis(70)], r.width[axis(30)]);
        assert!(r.width.iter().all(|&x| x <= 9.0 + 1e-4));
        // the water lies under the ground on the centre line and falls (or stays level) going downstream
        let mut prev = f32::MAX;
        for j in 30..75 {
            let c = axis(j);
            assert!(r.water_y[c] < h[c], "water under the bank at row {j}: {} vs {}", r.water_y[c], h[c]);
            assert!(r.water_y[c] <= prev + 1e-4, "water runs uphill at row {j}");
            prev = r.water_y[c];
        }
        // through the node: the mask is 1 in the channel and 0 away from it
        let g = {
            let pts: Vec<[f32; 3]> = (0..=rows).flat_map(|j| (0..=cols).map(move |i| (i, j))).map(|(i, j)| [i as f32 * 2.0, h[j * w + i], j as f32 * 2.0]).collect();
            let mut polys = Vec::new();
            for j in 0..rows {
                for i in 0..cols {
                    let a = (j * w + i) as u32;
                    polys.push(vec![a, a + w as u32, a + w as u32 + 1, a + 1]);
                }
            }
            let mut g = Geometry::from_polys(pts, &polys);
            g.detail.insert("grid".into(), json!({"rows": rows, "cols": cols}));
            g
        };
        let mut p = Params::new();
        for (k, v) in [("stream_area", json!(400.0)), ("lake_depth", json!(0.5)), ("river_width", json!(2.0)), ("river_width_max", json!(9.0)), ("river_bank", json!(2.0)), ("river_freeboard", json!(0.3))] {
            p.insert(k.into(), v);
        }
        let out = flow(&g, &p).unwrap();
        let river = out.point.get("river").unwrap().as_f32().unwrap();
        assert_eq!(river[axis(70)], 1.0);
        assert_eq!(river[side], 0.0);
        for attr in ["river_sd", "river_w", "water_y"] {
            assert_eq!(out.point.get(attr).map(|c| c.len()), Some(g.n_points()), "{attr}");
        }
        assert_eq!(flow(&g, &p).unwrap().content_hash(), out.content_hash());
    }

    #[test]
    fn a_pit_is_filled_to_its_rim_and_reported_as_a_lake() {
        let (rows, cols) = (20usize, 20usize);
        let mut h: Vec<f32> = (0..=rows).flat_map(|j| (0..=cols).map(move |_| 10.0 + j as f32 * 0.5)).collect();
        let w = cols + 1;
        for (i, j) in [(9, 9), (10, 9), (9, 10), (10, 10)] {
            h[j * w + i] -= 3.0; // a 3 m deep hole in a tilted plane
        }
        let hy = hydrology(&h, rows, cols, 2.0, 2.0);
        for (i, j) in [(9, 9), (10, 10)] {
            assert!(hy.filled[j * w + i] - h[j * w + i] > 1.9, "the pit fills to its lowest rim (14 m): {} -> {}", h[j * w + i], hy.filled[j * w + i]);
        }
        assert!((hy.filled[2 * w + 2] - h[2 * w + 2]).abs() < 1e-6, "ground that drains is untouched");
        let g = {
            let mut pts = Vec::new();
            for j in 0..=rows {
                for i in 0..=cols {
                    pts.push([i as f32 * 2.0, h[j * w + i], j as f32 * 2.0]);
                }
            }
            let mut polys = Vec::new();
            for j in 0..rows {
                for i in 0..cols {
                    let a = (j * w + i) as u32;
                    polys.push(vec![a, a + w as u32, a + w as u32 + 1, a + 1]);
                }
            }
            let mut g = Geometry::from_polys(pts, &polys);
            g.detail.insert("grid".into(), json!({"rows": rows, "cols": cols}));
            g
        };
        let mut p = Params::new();
        p.insert("lake_depth".into(), json!(0.5));
        p.insert("stream_area".into(), json!(40.0));
        p.insert("level_lakes".into(), json!(1));
        let out = flow(&g, &p).unwrap();
        let lake = out.point.get("lake").unwrap().as_f32().unwrap();
        assert_eq!(lake.iter().filter(|&&l| l > 0.5).count() >= 4, true, "the pit is a lake");
        assert_eq!(lake[2 * w + 2], 0.0);
        // levelled: the four pit cells now share one height
        let ys: Vec<f32> = [(9, 9), (10, 9), (9, 10), (10, 10)].iter().map(|&(i, j)| out.p()[j * w + i][1]).collect();
        assert!(ys.iter().all(|&y| (y - ys[0]).abs() < 1e-5), "{ys:?}");
        for attr in ["filled", "fill_depth", "flow_acc", "twi", "stream"] {
            assert_eq!(out.point.get(attr).map(|c| c.len()), Some(g.n_points()), "{attr}");
        }
        // same input, same result
        assert_eq!(flow(&g, &p).unwrap().point.get("flow_acc").unwrap().as_f32().unwrap(), out.point.get("flow_acc").unwrap().as_f32().unwrap());
    }
}
