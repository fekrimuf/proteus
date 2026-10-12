//! `pt terrain <recipe> [--out DIR]`: statistics and a top-down map of a terrain.
//!
//! The statistics are the ones the plan names for terrain (B12.3): relief, the slope distribution, and the hypsometric integral,
//! computed on the cooked mesh with every triangle weighted by its footprint, so a steep wall counts for the ground it covers on the
//! map. They are what a synthetic terrain will later be fitted to, and what the scorecard reports for the world it measures.

use pt_core::Geometry;
use pt_render::Image;
use serde_json::{json, Value};
use std::path::Path;

/// Footprint-weighted statistics of the base mesh. `elev` (absolute metres per point) is used for the elevation figures when given.
pub fn terrain_stats(geo: &Geometry, elev: Option<&[f32]>) -> Value {
    let p = geo.p();
    let tris = geo.triangles();
    if tris.is_empty() {
        return json!({"error": "no triangles"});
    }
    let mut slopes: Vec<(f32, f64)> = Vec::with_capacity(tris.len()); // (degrees, footprint m2)
    let mut footprint = 0.0f64;
    for t in &tris {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        let (e1, e2) = (pt_core::math::sub(b, a), pt_core::math::sub(c, a));
        let n = pt_core::math::cross(e1, e2);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < 1e-12 {
            continue;
        }
        let area_xz = (n[1].abs() as f64) * 0.5; // projected area
        let cos = (n[1].abs() / len).clamp(0.0, 1.0);
        slopes.push((cos.acos().to_degrees(), area_xz));
        footprint += area_xz;
    }
    slopes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let pct = |q: f64| -> f64 {
        let target = footprint * q;
        let mut acc = 0.0;
        for (deg, w) in &slopes {
            acc += w;
            if acc >= target {
                return *deg as f64;
            }
        }
        slopes.last().map_or(0.0, |s| s.0 as f64)
    };
    let mean_slope = slopes.iter().map(|(d, w)| *d as f64 * w).sum::<f64>() / footprint.max(1e-9);
    let steeper = |deg: f32| slopes.iter().filter(|(d, _)| *d > deg).map(|(_, w)| w).sum::<f64>() / footprint.max(1e-9);
    let heights: Vec<f32> = match elev {
        Some(e) if e.len() == p.len() => e.to_vec(),
        _ => p.iter().map(|q| q[1]).collect(),
    };
    let (lo, hi) = heights.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let mean_h = heights.iter().map(|&v| v as f64).sum::<f64>() / heights.len().max(1) as f64;
    json!({
        "area_km2": footprint / 1.0e6,
        "min_elev_m": lo, "max_elev_m": hi, "relief_m": hi - lo,
        "hypsometric_integral": if hi > lo { (mean_h - lo as f64) / (hi - lo) as f64 } else { 0.0 },
        "mean_slope_deg": mean_slope,
        "slope_deg": {"p10": pct(0.10), "p50": pct(0.50), "p90": pct(0.90), "p99": pct(0.99)},
        "steeper_than_30_deg": steeper(30.0), "steeper_than_45_deg": steeper(45.0),
    })
}

/// A height-tinted, hill-shaded top-down map (north up, east right; the mesh frame is +X east, +Z south).
pub fn terrain_map(geo: &Geometry, px: usize) -> Image {
    let p = geo.p();
    let (lo, hi) = geo.base_bounds().unwrap_or(([-1.0; 3], [1.0; 3]));
    let (w, d) = ((hi[0] - lo[0]).max(1e-3), (hi[2] - lo[2]).max(1e-3));
    let scale = px as f32 / w.max(d);
    let (iw, ih) = (((w * scale) as usize).max(1), ((d * scale) as usize).max(1));
    let mut height = vec![f32::NAN; iw * ih];
    let mut shade = vec![0.0f32; iw * ih];
    let sun = pt_core::math::normalize([-0.5, 0.75, -0.45]); // from the north-west, 45 degrees up
    for t in geo.triangles() {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        let n = pt_core::math::normalize(pt_core::math::cross(pt_core::math::sub(b, a), pt_core::math::sub(c, a)));
        let lambert = (n[0] * sun[0] + n[1].abs() * sun[1] + n[2] * sun[2]).clamp(0.0, 1.0);
        let q = |v: [f32; 3]| ((v[0] - lo[0]) * scale, (v[2] - lo[2]) * scale);
        let (qa, qb, qc) = (q(a), q(b), q(c));
        let (x0, x1) = (qa.0.min(qb.0).min(qc.0).floor().max(0.0) as usize, (qa.0.max(qb.0).max(qc.0).ceil() as usize).min(iw - 1));
        let (y0, y1) = (qa.1.min(qb.1).min(qc.1).floor().max(0.0) as usize, (qa.1.max(qb.1).max(qc.1).ceil() as usize).min(ih - 1));
        let den = (qb.1 - qc.1) * (qa.0 - qc.0) + (qc.0 - qb.0) * (qa.1 - qc.1);
        if den.abs() < 1e-9 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let l1 = ((qb.1 - qc.1) * (fx - qc.0) + (qc.0 - qb.0) * (fy - qc.1)) / den;
                let l2 = ((qc.1 - qa.1) * (fx - qc.0) + (qa.0 - qc.0) * (fy - qc.1)) / den;
                let l3 = 1.0 - l1 - l2;
                if l1 >= -1e-4 && l2 >= -1e-4 && l3 >= -1e-4 {
                    height[y * iw + x] = l1 * a[1] + l2 * b[1] + l3 * c[1];
                    shade[y * iw + x] = lambert;
                }
            }
        }
    }
    let ramp = |t: f32| -> [f32; 3] {
        // green valley floor, olive and brown slopes, grey rock, white summits
        let stops: [(f32, [f32; 3]); 5] = [(0.0, [0.25, 0.45, 0.2]), (0.3, [0.45, 0.5, 0.25]), (0.55, [0.5, 0.42, 0.3]), (0.8, [0.6, 0.6, 0.6]), (1.0, [0.97, 0.97, 0.97])];
        for k in 1..stops.len() {
            if t <= stops[k].0 {
                let u = (t - stops[k - 1].0) / (stops[k].0 - stops[k - 1].0);
                return [0, 1, 2].map(|c| stops[k - 1].1[c] + (stops[k].1[c] - stops[k - 1].1[c]) * u);
            }
        }
        stops[4].1
    };
    let mut rgb = Vec::with_capacity(iw * ih * 3);
    for i in 0..iw * ih {
        let h = height[i];
        if h.is_nan() {
            rgb.extend_from_slice(&[255, 255, 255]);
            continue;
        }
        let tint = ramp(((h - lo[1]) / (hi[1] - lo[1]).max(1e-3)).clamp(0.0, 1.0));
        let l = 0.35 + 0.75 * shade[i];
        rgb.extend(tint.iter().map(|c| ((c * l).clamp(0.0, 1.0) * 255.0) as u8));
    }
    Image { w: iw, h: ih, rgb }
}

/// Valley-floor candidates on a grid heightfield (`rows` x `cols`, row-major, +X east, +Z south): flat cells (slope under `flat_deg`) that have
/// steep ground (over `steep_deg`) on BOTH sides within `radius` metres along some direction, scored by the share of steep ground around
/// them and the extent of the flat floor. Returns (x, z, elevation, score) for the best cells at least `spacing` metres apart.
pub fn find_valleys(geo: &Geometry, rows: usize, cols: usize, size: [f32; 2], radius: f32, count: usize, spacing: f32) -> Vec<[f32; 4]> {
    let p = geo.p();
    let w = cols + 1;
    if p.len() != (rows + 1) * w {
        return vec![];
    }
    let (dx, dz) = (size[0] / cols as f32, size[1] / rows as f32);
    let h = |i: usize, j: usize| p[j * w + i][1];
    let mut slope = vec![0.0f32; p.len()];
    for j in 1..rows {
        for i in 1..cols {
            let gx = (h(i + 1, j) - h(i - 1, j)) / (2.0 * dx);
            let gz = (h(i, j + 1) - h(i, j - 1)) / (2.0 * dz);
            slope[j * w + i] = (gx * gx + gz * gz).sqrt().atan().to_degrees();
        }
    }
    let r = (radius / dx).round().max(2.0) as usize;
    let step = (r / 8).max(1);
    let mut cands: Vec<[f32; 4]> = Vec::new();
    for j in (r..rows.saturating_sub(r)).step_by(step) {
        for i in (r..cols.saturating_sub(r)).step_by(step) {
            if slope[j * w + i] > 8.0 {
                continue;
            }
            // flat floor: share of flat cells in a small box; walls: steep cells along the four axis directions
            let (mut flat, mut total) = (0usize, 0usize);
            for dj in (0..=r / 2).step_by(step) {
                for di in (0..=r / 2).step_by(step) {
                    for (si, sj) in [(1i64, 1i64), (-1, 1), (1, -1), (-1, -1)] {
                        let (ii, jj) = ((i as i64 + si * di as i64) as usize, (j as i64 + sj * dj as i64) as usize);
                        total += 1;
                        flat += (slope[jj * w + ii] < 10.0) as usize;
                    }
                }
            }
            let floor = flat as f32 / total.max(1) as f32;
            let steep_dir = |si: i64, sj: i64| -> bool {
                let mut steep = 0usize;
                let mut n = 0usize;
                for k in (r / 4..=r).step_by(step) {
                    let (ii, jj) = ((i as i64 + si * k as i64) as usize, (j as i64 + sj * k as i64) as usize);
                    n += 1;
                    steep += (slope[jj * w + ii] > 32.0) as usize;
                }
                steep * 3 >= n
            };
            let walls = (steep_dir(1, 0) && steep_dir(-1, 0)) || (steep_dir(0, 1) && steep_dir(0, -1));
            if !walls || floor < 0.45 {
                continue;
            }
            let area_steep = {
                let (mut s, mut n) = (0usize, 0usize);
                for dj in (0..=r).step_by(step) {
                    for di in (0..=r).step_by(step) {
                        for (si, sj) in [(1i64, 1i64), (-1, 1), (1, -1), (-1, -1)] {
                            let (ii, jj) = ((i as i64 + si * di as i64) as usize, (j as i64 + sj * dj as i64) as usize);
                            n += 1;
                            s += (slope[jj * w + ii] > 30.0) as usize;
                        }
                    }
                }
                s as f32 / n.max(1) as f32
            };
            let score = floor * (0.4 + area_steep.min(0.5));
            cands.push([p[j * w + i][0], p[j * w + i][2], h(i, j), score]);
        }
    }
    cands.sort_by(|a, b| b[3].partial_cmp(&a[3]).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<[f32; 4]> = Vec::new();
    for c in cands {
        if out.iter().all(|o| ((o[0] - c[0]).powi(2) + (o[1] - c[1]).powi(2)).sqrt() >= spacing) {
            out.push(c);
            if out.len() >= count {
                break;
            }
        }
    }
    out
}

/// `pt dem-manifest <manifest.json> --id ID --lat DEG --lon DEG --zoom Z --size METRES`: add (or replace) the entry `ID` in an
/// elevation manifest with the tiles a square window needs, computed from the latitude and longitude by the same projection the
/// `dem` node samples with. `pt corpus fetch <manifest.json>` then downloads them through the licence ledger. US windows only:
/// the entry is written with the USGS public-domain licence, which holds where the source of the tiles is 3DEP.
pub fn dem_manifest_cmd(args: &[String]) -> Result<(), String> {
    let usage = "usage: pt dem-manifest <manifest.json> --id ID --lat DEG --lon DEG --zoom Z --size METRES";
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1));
    let path = args.first().filter(|a| !a.starts_with("--")).ok_or(usage)?;
    let num = |f: &str| flag(f).and_then(|s| s.parse::<f64>().ok()).ok_or_else(|| format!("{f} is missing or not a number\n{usage}"));
    let (id, lat, lon, zoom, size) = (flag("--id").ok_or(usage)?, num("--lat")?, num("--lon")?, num("--zoom")? as u32, num("--size")?);
    if !(24.0..=50.0).contains(&lat) || !(-125.0..=-66.0).contains(&lon) {
        return Err(format!("lat {lat} / lon {lon} is outside the contiguous United States: the public-domain licence written here only holds for USGS 3DEP tiles"));
    }
    let (x0, x1, y0, y1) = pt_sop::dem::tile_range(lat, lon, zoom, size, size);
    let mut files = Vec::new();
    for x in x0..=x1 {
        for y in y0..=y1 {
            files.push(json!({"path": format!("dem/terrarium/{zoom}/{x}/{y}.png"), "url": format!("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{zoom}/{x}/{y}.png")}));
        }
    }
    let metres_per_px = 156_543.033_928 * lat.to_radians().cos() / (1u64 << zoom) as f64;
    let entry = json!({
        "id": id, "source": "usgs-3dep-terrarium", "set": "dem", "category": "terrain", "difficulty": "terrain",
        "license": "PD-USGS", "license_url": "https://www.usgs.gov/information-policies-and-instructions/copyrights-and-credits",
        "attribution": format!("3DEP data courtesy of the U.S. Geological Survey. USGS: \"USGS-authored or produced data and information are considered to be in the U.S. Public Domain\" (credit requested). Tiles from Terrain Tiles on AWS (Mapzen), https://registry.opendata.aws/terrain-tiles/; the tile area lies inside the United States, where the source is 3DEP. Zoom {zoom}: about {metres_per_px:.1} m per pixel at this latitude; {} x {} tiles for a window of {size:.0} m around {lat:.4} N, {lon:.4} E (tile numbers computed by `pt dem-manifest`).", x1 - x0 + 1, y1 - y0 + 1),
        "main": format!("dem/terrarium/{zoom}/{}/{}.png", (x0 + x1) / 2, (y0 + y1) / 2),
        "files": files,
    });
    let mut doc: Value = match std::fs::read_to_string(path.as_str()) {
        Ok(t) => serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| format!("{path}: {e}"))?,
        Err(_) => json!({"version": 1, "note": "US elevation tiles (Terrarium PNG: height in metres = R*256 + G + B/256 - 32768).", "assets": []}),
    };
    let assets = doc.get_mut("assets").and_then(Value::as_array_mut).ok_or_else(|| format!("{path}: no 'assets' array"))?;
    assets.retain(|a| a.get("id").and_then(Value::as_str) != Some(id.as_str()));
    assets.push(entry);
    std::fs::write(path.as_str(), serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?).map_err(|e| format!("{path}: {e}"))?;
    println!("{id}: zoom {zoom}, tiles x {x0}..{x1}, y {y0}..{y1} ({} files, {metres_per_px:.1} m per pixel) -> {path}", (x1 - x0 + 1) * (y1 - y0 + 1));
    Ok(())
}

/// Hill-shaded top-down map with the water on it (north up): channel and lake cells in blue, banks in sand. Needs the `river`
/// mask of `hf_flow` (with `river_width` set); `None` if the geometry is not such a grid.
pub fn water_map(geo: &Geometry, px: usize) -> Option<Image> {
    let river = geo.point.get("river")?.as_f32()?;
    let mut img = terrain_map(geo, px);
    let p = geo.p();
    let (lo, hi) = geo.base_bounds()?;
    let scale = px as f32 / (hi[0] - lo[0]).max(hi[2] - lo[2]).max(1e-3);
    let lake = geo.point.get("lake").and_then(|c| c.as_f32());
    for (k, q) in p.iter().enumerate() {
        let wet = river[k].max(lake.map_or(0.0, |l| l[k]));
        if wet <= 0.02 {
            continue;
        }
        let (x, y) = ((((q[0] - lo[0]) * scale) as usize).min(img.w - 1), (((q[2] - lo[2]) * scale) as usize).min(img.h - 1));
        let c = if wet > 0.9 { [40u8, 110, 220] } else { [200u8, 185, 140] };
        img.rgb[(y * img.w + x) * 3..(y * img.w + x) * 3 + 3].copy_from_slice(&c);
    }
    Some(img)
}

/// The widest reaches of the channels: (x, z, ground height, channel width, catchment in km2), one per `spacing` metres.
pub fn main_reaches(geo: &Geometry, count: usize, spacing: f32) -> Vec<[f32; 5]> {
    let (Some(sd), Some(wd), Some(acc)) = (geo.point.get("river_sd").and_then(|c| c.as_f32()), geo.point.get("river_w").and_then(|c| c.as_f32()), geo.point.get("flow_acc").and_then(|c| c.as_f32())) else { return vec![] };
    let p = geo.p();
    // in-channel cells, widest channel first; among equals the centre line (largest catchment) first
    let mut cells: Vec<usize> = (0..p.len()).filter(|&k| sd[k] < 0.0 && wd[k] > 0.0).collect();
    let ord = |x: f32, y: f32| y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal);
    cells.sort_by(|&a, &b| ord(wd[a], wd[b]).then(ord(acc[a], acc[b])).then(a.cmp(&b)));
    let mut out: Vec<[f32; 5]> = Vec::new();
    for k in cells {
        let q = p[k];
        if out.iter().all(|o| ((o[0] - q[0]).powi(2) + (o[1] - q[2]).powi(2)).sqrt() >= spacing) {
            out.push([q[0], q[2], q[1], wd[k], acc[k] / 1.0e6]);
            if out.len() >= count {
                break;
            }
        }
    }
    out
}

/// `pt terrain <recipe.hcs> [--out DIR] [--px N] [--find-valley RADIUS_M]`
pub fn terrain_cmd(args: &[String]) -> Result<(), String> {
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1));
    let recipe = pos.first().ok_or("usage: pt terrain <recipe.hcs> [--out DIR] [--px N]")?;
    let code = std::fs::read_to_string(recipe.as_str()).map_err(|e| format!("{recipe}: {e}"))?;
    let mut g = pt_graph::Graph::new();
    let rep = pt_hcs::run(&code, &mut g, true).map_err(|d| d[0].to_string())?;
    let geo = g.cook(rep.outputs.first().ok_or("recipe has no out()")?)?;
    let elev = geo.point.get("elev").and_then(|c| c.as_f32().map(|s| s.to_vec()));
    let stats = terrain_stats(&geo, elev.as_deref());
    println!("{}", serde_json::to_string_pretty(&stats).unwrap_or_default());
    if let Some(radius) = flag("--find-valley").and_then(|s| s.parse::<f32>().ok()) {
        let (grid, dem) = (geo.detail.get("grid"), geo.detail.get("dem"));
        let dims = grid.and_then(|g| Some((g.get("rows")?.as_u64()? as usize, g.get("cols")?.as_u64()? as usize)));
        let size = dem.and_then(|d| Some([d.pointer("/size_m/0")?.as_f64()? as f32, d.pointer("/size_m/1")?.as_f64()? as f32]));
        let (Some((rows, cols)), Some(size)) = (dims, size) else { return Err("--find-valley needs a `dem` grid (no hole) as the recipe's output".into()) };
        let (lat0, lon0) = (dem.and_then(|d| d["lat"].as_f64()).unwrap_or(0.0), dem.and_then(|d| d["lon"].as_f64()).unwrap_or(0.0));
        let datum = dem.and_then(|d| d["datum_m"].as_f64()).unwrap_or(0.0);
        println!("valley-floor candidates (x east, z south of the centre, metres; elevation; score):");
        for c in find_valleys(&geo, rows, cols, size, radius, 8, radius * 1.5) {
            let (dlat, dlon) = (-(c[1] as f64) / 111_320.0, c[0] as f64 / (111_320.0 * (lat0.to_radians()).cos()));
            println!("  x {:>7.0} z {:>7.0}  elevation {:>6.0} m  score {:.2}  -> lat {:.4} lon {:.4}", c[0], c[1], c[2] as f64 + datum, c[3], lat0 + dlat, lon0 + dlon);
        }
    }
    let reaches = main_reaches(&geo, flag("--reaches").and_then(|s| s.parse().ok()).unwrap_or(14), flag("--reach-spacing").and_then(|s| s.parse().ok()).unwrap_or(120.0));
    if !reaches.is_empty() {
        println!("widest reaches of the channels (x east, z south, ground height above the datum, channel width, catchment):");
        for r in &reaches {
            println!("  x {:>7.0} z {:>7.0}  y {:>6.1} m  width {:>4.1} m  catchment {:>5.2} km2", r[0], r[1], r[2], r[3], r[4]);
        }
    }
    if let Some(dir) = flag("--out") {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let px = flag("--px").and_then(|s| s.parse().ok()).unwrap_or(1024);
        if let Some(img) = water_map(&geo, px) {
            std::fs::write(Path::new(dir).join("water_map.png"), pt_render::encode_png(&img)?).map_err(|e| e.to_string())?;
            println!("wrote {dir}/water_map.png");
        }
        std::fs::write(Path::new(dir).join("terrain_map.png"), pt_render::encode_png(&terrain_map(&geo, px))?).map_err(|e| e.to_string())?;
        std::fs::write(Path::new(dir).join("terrain_stats.json"), serde_json::to_string_pretty(&stats).unwrap_or_default()).map_err(|e| e.to_string())?;
        println!("wrote {dir}/terrain_map.png and terrain_stats.json");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid ramp rising `rise` metres per metre eastward over a 100 x 100 m square.
    fn ramp(rise: f32) -> Geometry {
        let (n, size) = (10usize, 100.0f32);
        let mut pts = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                let x = i as f32 / n as f32 * size;
                pts.push([x, x * rise, j as f32 / n as f32 * size]);
            }
        }
        let mut polys = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                polys.push(vec![a, a + n as u32 + 1, a + n as u32 + 2, a + 1]);
            }
        }
        Geometry::from_polys(pts, &polys)
    }

    #[test]
    fn a_ramp_has_one_slope_and_the_right_area_and_relief() {
        let s = terrain_stats(&ramp(1.0), None); // 45 degrees
        assert!((s["mean_slope_deg"].as_f64().unwrap() - 45.0).abs() < 0.1, "{s}");
        assert!((s["slope_deg"]["p50"].as_f64().unwrap() - 45.0).abs() < 0.1);
        assert!((s["area_km2"].as_f64().unwrap() - 0.01).abs() < 1e-6, "100 m x 100 m is 0.01 km2");
        assert!((s["relief_m"].as_f64().unwrap() - 100.0).abs() < 1e-3);
        assert!((s["hypsometric_integral"].as_f64().unwrap() - 0.5).abs() < 0.02, "a plane has an integral of one half");
        let flat = terrain_stats(&ramp(0.0), None);
        assert!(flat["steeper_than_30_deg"].as_f64().unwrap() < 1e-9 && flat["relief_m"].as_f64().unwrap() < 1e-6);
    }

    #[test]
    fn the_map_is_north_up_and_tinted_by_height() {
        let img = terrain_map(&ramp(1.0), 64);
        assert_eq!((img.w, img.h), (64, 64));
        // west is low (green tint, darker), east is high (near white): compare the left and right edge brightness
        let lum = |x: usize| img.rgb[(32 * img.w + x) * 3..(32 * img.w + x) * 3 + 3].iter().map(|&c| c as u32).sum::<u32>();
        assert!(lum(60) > lum(3), "{} vs {}", lum(60), lum(3));
    }
}
