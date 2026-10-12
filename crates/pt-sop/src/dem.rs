//! `dem`: real elevation as a heightfield. Reads Terrarium PNG tiles (height in metres = R*256 + G + B/256 - 32768; the US
//! tiles are USGS 3DEP, public domain, fetched through the licence ledger into `<corpus>/dem/terrarium/<zoom>/<x>/<y>.png`)
//! and samples them on a regular grid in a local east/south frame around a latitude and longitude.
//!
//! Frame: +X east, +Y up, +Z south (so the grid faces +Y like `grid`). Heights are `elevation - datum`; the absolute elevation
//! stays on every point as `elev`. A near window can blend into a coarser far zoom at its edge (`far_zoom`, `blend`), and a
//! far ring can leave out a rectangle (`hole`) where the near window sits: if the hole's edges lie on far grid lines the two
//! meshes meet exactly, because both are linear along a grid line.
//!
//! Deterministic: only IEEE arithmetic, `pt_core::math::sin_cos64`, and a series for the logarithm (no platform libm).

use crate::{ps, sch};
use pt_core::math::sin_cos64;
use pt_core::{Column, Differentiability as D, Geometry, Granularity as G, NodeSchema, ParamExt, Params};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type Res = Result<Geometry, String>;

const EARTH_R: f64 = 6_378_137.0;
const PI64: f64 = std::f64::consts::PI;

pub fn schemas() -> Vec<NodeSchema> {
    vec![sch(
        "dem",
        "Real elevation as a grid heightfield, from Terrarium PNG tiles in the corpus (US 3DEP, public domain; `pt corpus fetch corpus/dem_manifest.json`). X is east, Z is south, heights are elevation minus `datum`; the absolute elevation is the point attribute `elev`. `far_zoom` blends the window edge into a coarser zoom so it meets a far ring; `hole` leaves a rectangle out of a far ring where the near window sits.",
        0,
        D::None,
        G::Whole,
        vec![
            ps("tiles", json!("dem/terrarium"), "Tile folder relative to the corpus root: <tiles>/<zoom>/<x>/<y>.png."),
            ps("zoom", json!(14), "Tile zoom of the data to sample (14 is about 7 m per pixel at 44 degrees north, 11 about 55 m)."),
            ps("lat", json!(43.7767), "Latitude of the window centre, degrees north."),
            ps("lon", json!(-110.9028), "Longitude of the window centre, degrees east (negative is west)."),
            ps("size", json!([1200.0, 1200.0]), "Window size in metres: [east-west, north-south]."),
            ps("rows", json!(480), "Grid rows (north-south cells)."),
            ps("cols", json!(480), "Grid columns (east-west cells)."),
            ps("datum", json!(0.0), "Elevation in metres that becomes y = 0 (use the valley floor so recipes can speak in heights above it)."),
            ps("far_zoom", json!(0), "If > 0, blend the window's edge into this coarser zoom over `blend` metres so it meets a far ring without a step."),
            ps("blend", json!(100.0), "Width in metres of that edge blend."),
            ps("hole", json!([0.0, 0.0]), "For a far ring: leave out cells inside this centred rectangle [east-west, north-south] metres. Its edges should lie on grid lines."),
        ],
    )]
}

pub fn cook(kind: &str, p: &Params) -> Option<Res> {
    match kind {
        "dem" => Some(dem(p)),
        _ => None,
    }
}

/// Natural logarithm for x > 0 from the atanh series, using only basic arithmetic so every platform agrees to the last bit.
fn ln64(x: f64) -> f64 {
    // x = m * 2^k with m in [0.75, 1.5): ln x = k ln 2 + 2 atanh((m-1)/(m+1))
    const LN2: f64 = std::f64::consts::LN_2;
    let (mut m, mut k) = (x, 0i32);
    while m >= 1.5 {
        m *= 0.5;
        k += 1;
    }
    while m < 0.75 {
        m *= 2.0;
        k -= 1;
    }
    let y = (m - 1.0) / (m + 1.0);
    let y2 = y * y;
    let (mut term, mut sum) = (y, 0.0);
    for n in 0..30 {
        sum += term / (2 * n + 1) as f64;
        term *= y2;
    }
    k as f64 * LN2 + 2.0 * sum
}

/// Web Mercator y (radians) of a latitude: ln tan(pi/4 + phi/2).
fn merc_y(phi: f64) -> f64 {
    let (s, c) = sin_cos64(PI64 / 4.0 + phi / 2.0);
    ln64(s / c)
}

fn tile_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<f32>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<f32>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One 256 x 256 Terrarium tile as metres, decoded once per process.
fn load_tile(path: &Path) -> Result<Arc<Vec<f32>>, String> {
    if let Some(t) = tile_cache().lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        return Ok(t.clone());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("dem: {}: {e} (fetch the tiles with `pt corpus fetch corpus/dem_manifest.json`, root {})", path.display(), pt_core::gltf::corpus_root().display()))?;
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().map_err(|e| format!("dem: {}: {e}", path.display()))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("dem: {}: {e}", path.display()))?;
    let ch = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => return Err(format!("dem: {}: expected an RGB Terrarium tile, got {other:?}", path.display())),
    };
    if info.width != 256 || info.height != 256 {
        return Err(format!("dem: {}: expected 256 x 256 pixels, got {} x {}", path.display(), info.width, info.height));
    }
    let h: Vec<f32> = buf[..info.buffer_size()].chunks(ch).map(|p| (p[0] as f32) * 256.0 + p[1] as f32 + (p[2] as f32) / 256.0 - 32768.0).collect();
    let t = Arc::new(h);
    tile_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), t.clone());
    Ok(t)
}

/// Bilinear elevation sampler over the tiles of one zoom level.
struct Sampler {
    dir: PathBuf,
    zoom: u32,
    local: HashMap<(i64, i64), Arc<Vec<f32>>>,
}

impl Sampler {
    fn new(dir: PathBuf, zoom: u32) -> Sampler {
        Sampler { dir, zoom, local: HashMap::new() }
    }

    fn pixel(&mut self, gx: i64, gy: i64) -> Result<f64, String> {
        let (tx, ty) = (gx.div_euclid(256), gy.div_euclid(256));
        if !self.local.contains_key(&(tx, ty)) {
            let t = load_tile(&self.dir.join(self.zoom.to_string()).join(tx.to_string()).join(format!("{ty}.png")))?;
            self.local.insert((tx, ty), t);
        }
        Ok(self.local[&(tx, ty)][(gy.rem_euclid(256) * 256 + gx.rem_euclid(256)) as usize] as f64)
    }

    /// Elevation in metres at a latitude and longitude (radians).
    fn height(&mut self, phi: f64, lam: f64) -> Result<f64, String> {
        let n = 256.0 * (1u64 << self.zoom) as f64;
        let px = (lam / PI64 + 1.0) / 2.0 * n - 0.5; // pixel centres sit at +0.5
        let py = (1.0 - merc_y(phi) / PI64) / 2.0 * n - 0.5;
        let (x0, y0) = (px.floor(), py.floor());
        let (fx, fy) = (px - x0, py - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = self.pixel(x0, y0)? * (1.0 - fx) + self.pixel(x0 + 1, y0)? * fx;
        let bot = self.pixel(x0, y0 + 1)? * (1.0 - fx) + self.pixel(x0 + 1, y0 + 1)? * fx;
        Ok(top * (1.0 - fy) + bot * fy)
    }
}

fn smooth01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The tiles a window needs at a zoom level: inclusive ranges (x0, x1, y0, y1) for a rectangle of `w` by `d` metres centred on
/// a latitude and longitude (degrees), with one pixel of margin for the bilinear lookup. The same projection as the sampler, so a
/// manifest made from this list is exactly what `dem` will read.
pub fn tile_range(lat: f64, lon: f64, zoom: u32, w: f64, d: f64) -> (i64, i64, i64, i64) {
    let n = 256.0 * (1u64 << zoom) as f64;
    let (phi0, lam0) = (lat * (PI64 / 180.0), lon * (PI64 / 180.0));
    let px = |lam: f64| (lam / PI64 + 1.0) / 2.0 * n - 0.5;
    let py = |phi: f64| (1.0 - merc_y(phi) / PI64) / 2.0 * n - 0.5;
    let (north, south) = (phi0 + d / 2.0 / EARTH_R, phi0 - d / 2.0 / EARTH_R);
    // meridians converge toward the pole: the widest longitude span is on the edge nearer the pole
    let cos_min = sin_cos64(north).1.min(sin_cos64(south).1);
    let dlam = w / 2.0 / (EARTH_R * cos_min);
    let tile = |v: f64| (v.floor() as i64).div_euclid(256);
    (tile(px(lam0 - dlam) - 1.0), tile(px(lam0 + dlam) + 2.0), tile(py(north) - 1.0), tile(py(south) + 2.0))
}

fn dem(p: &Params) -> Res {
    let (rows, cols) = (p.i("rows").max(1) as usize, p.i("cols").max(1) as usize);
    let size = p.v3("size");
    let (w, d) = (size[0] as f64, size[1] as f64);
    if w <= 0.0 || d <= 0.0 {
        return Err("dem: `size` must be positive metres, e.g. [1200, 1200]".into());
    }
    let f64p = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let (lat, lon) = (f64p("lat"), f64p("lon"));
    if !(-85.0..=85.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(format!("dem: lat {lat} / lon {lon} out of range (degrees north, degrees east)"));
    }
    let tiles = if p.s("tiles").is_empty() { "dem/terrarium".to_string() } else { p.s("tiles") };
    if tiles.contains("..") {
        return Err("dem: `tiles` may not contain '..'".into());
    }
    let dir = pt_core::gltf::corpus_root().join(&tiles);
    let zoom = p.i("zoom").clamp(0, 18) as u32;
    let far_zoom = p.i("far_zoom").clamp(0, 18) as u32;
    let blend = f64p("blend").max(0.0);
    let datum = f64p("datum");
    let hole = p.v3("hole");
    let (hw, hd) = (hole[0] as f64 / 2.0, hole[1] as f64 / 2.0);
    let (phi0, lam0) = (lat.to_radians_det(), lon.to_radians_det());
    let mut near = Sampler::new(dir.clone(), zoom);
    let mut far = Sampler::new(dir, far_zoom);

    let wcols = cols + 1;
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity((rows + 1) * wcols);
    let mut elev: Vec<f32> = Vec::with_capacity((rows + 1) * wcols);
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for j in 0..=rows {
        let s = (j as f64 / rows as f64 - 0.5) * d; // metres south of the centre
        let phi = phi0 - s / EARTH_R;
        let cosphi = sin_cos64(phi).1;
        for i in 0..=cols {
            let e = (i as f64 / cols as f64 - 0.5) * w; // metres east of the centre
            let lam = lam0 + e / (EARTH_R * cosphi);
            let mut h = near.height(phi, lam)?;
            if far_zoom > 0 && blend > 0.0 {
                let edge = (w / 2.0 - e.abs()).min(d / 2.0 - s.abs()).max(0.0);
                let wt = smooth01(edge / blend);
                if wt < 1.0 {
                    let hf = far.height(phi, lam)?;
                    h = hf + (h - hf) * wt;
                }
            }
            lo = lo.min(h);
            hi = hi.max(h);
            pts.push([e as f32, (h - datum) as f32, s as f32]);
            elev.push(h as f32);
        }
    }

    let inside = |idx: usize| -> bool {
        let (i, j) = (idx % wcols, idx / wcols);
        let (e, s) = ((i as f64 / cols as f64 - 0.5) * w, (j as f64 / rows as f64 - 0.5) * d);
        hw > 0.0 && hd > 0.0 && e.abs() <= hw + 1e-3 && s.abs() <= hd + 1e-3
    };
    let mut polys: Vec<Vec<u32>> = Vec::with_capacity(rows * cols);
    for j in 0..rows {
        for i in 0..cols {
            let a = j * wcols + i;
            let quad = [a, a + wcols, a + wcols + 1, a + 1];
            if quad.iter().all(|&q| inside(q)) {
                continue;
            }
            polys.push(quad.iter().map(|&q| q as u32).collect());
        }
    }
    let ring = hw > 0.0 && hd > 0.0;
    let (pts, elev) = if ring {
        // a ring leaves vertices inside the hole that no cell uses; drop them (they would export with zero normals)
        let mut remap = vec![u32::MAX; pts.len()];
        let (mut kept, mut kept_elev) = (Vec::new(), Vec::new());
        for poly in polys.iter_mut() {
            for q in poly.iter_mut() {
                let old = *q as usize;
                if remap[old] == u32::MAX {
                    remap[old] = kept.len() as u32;
                    kept.push(pts[old]);
                    kept_elev.push(elev[old]);
                }
                *q = remap[old];
            }
        }
        (kept, kept_elev)
    } else {
        (pts, elev)
    };
    let mut g = Geometry::from_polys(pts, &polys);
    g.point.set("elev", Column::from_f32(elev));
    if !ring {
        g.detail.insert("grid".into(), json!({"rows": rows, "cols": cols}));
    }
    g.detail.insert(
        "dem".into(),
        json!({"lat": lat, "lon": lon, "zoom": zoom, "far_zoom": far_zoom, "datum_m": datum, "size_m": [w, d], "min_elev_m": lo, "max_elev_m": hi,
               "source": "USGS 3DEP via Terrain Tiles on AWS (public domain; 3DEP data courtesy of the U.S. Geological Survey)"}),
    );
    Ok(g)
}

trait ToRadiansDet {
    fn to_radians_det(self) -> f64;
}

impl ToRadiansDet for f64 {
    fn to_radians_det(self) -> f64 {
        self * (PI64 / 180.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_png(f: impl Fn(usize, usize) -> f32) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(256 * 256 * 3);
        for y in 0..256 {
            for x in 0..256 {
                let v = f(x, y) + 32768.0;
                let (r, g, b) = ((v / 256.0).floor(), (v.floor() % 256.0), ((v - v.floor()) * 256.0).floor());
                rgb.extend_from_slice(&[r as u8, g as u8, b as u8]);
            }
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 256, 256);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgb).unwrap();
        }
        out
    }

    #[test]
    fn the_logarithm_series_matches_known_values() {
        for (x, want) in [(1.0, 0.0), (2.0, std::f64::consts::LN_2), (2.47, 0.904_218_5), (0.5, -std::f64::consts::LN_2), (10.0, std::f64::consts::LN_10)] {
            assert!((ln64(x) - want).abs() < 1e-6, "ln {x}: {} vs {want}", ln64(x));
        }
        // Web Mercator y at 45 degrees north is 0.8814 radians (asinh(1))
        assert!((merc_y(45.0f64.to_radians_det()) - 0.881_373_587).abs() < 1e-8);
    }

    #[test]
    fn the_tile_list_of_a_window_matches_the_tiles_the_teton_valley_was_fetched_with() {
        // 8 km around 43.765 N, 110.8406 W at zoom 14 is the 5 x 5 block the first manifest listed (checked against the files on disk)
        assert_eq!(tile_range(43.765, -110.8406, 14, 8000.0, 8000.0), (3145, 3149, 5970, 5974));
        // one tile at zoom 0 holds the world; a small window sits in one tile
        assert_eq!(tile_range(10.0, 20.0, 0, 1000.0, 1000.0), (0, 0, 0, 0));
        let (x0, x1, y0, y1) = tile_range(43.765, -110.8406, 14, 100.0, 100.0);
        assert!(x1 - x0 <= 1 && y1 - y0 <= 1);
        // every sample the node takes for that window falls inside the listed tiles
        let (x0, x1, y0, y1) = tile_range(43.7722, -110.8232, 12, 24000.0, 24000.0);
        let n = 256.0 * 4096.0;
        for (de, ds) in [(-12000.0, -12000.0), (12000.0, 12000.0), (-12000.0, 12000.0), (12000.0, -12000.0)] {
            let phi = 43.7722f64.to_radians_det() - ds / EARTH_R;
            let lam = (-110.8232f64).to_radians_det() + de / (EARTH_R * sin_cos64(phi).1);
            let (px, py) = ((lam / PI64 + 1.0) / 2.0 * n - 0.5, (1.0 - merc_y(phi) / PI64) / 2.0 * n - 0.5);
            for (gx, gy) in [(px.floor() as i64, py.floor() as i64), (px.floor() as i64 + 1, py.floor() as i64 + 1)] {
                let (tx, ty) = (gx.div_euclid(256), gy.div_euclid(256));
                assert!((x0..=x1).contains(&tx) && (y0..=y1).contains(&ty), "tile {tx},{ty} outside {x0}..{x1}, {y0}..{y1}");
            }
        }
    }

    #[test]
    fn terrarium_heights_decode_and_the_grid_faces_up() {
        let _g = crate::CORPUS_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("pt_dem_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // zoom 1 world: four tiles; put a tilted plane (height rises 1 m per pixel eastward) on tile (0,0) and (1,0)
        for (tx, ty) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let path = dir.join("dem/terrarium/1").join(tx.to_string());
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join(format!("{ty}.png")), tile_png(|x, _| 1000.0 + x as f32 + 256.0 * tx as f32)).unwrap();
        }
        std::env::set_var("PT_CORPUS", &dir);
        let mut p = Params::new();
        for (k, v) in [("zoom", json!(1)), ("lat", json!(10.0)), ("lon", json!(0.0)), ("size", json!([4000.0, 4000.0])), ("rows", json!(4)), ("cols", json!(4))] {
            p.insert(k.into(), v);
        }
        let g = dem(&p).unwrap();
        assert_eq!((g.n_points(), g.n_prims()), (25, 16));
        let elev = g.point.get("elev").unwrap().as_f32().unwrap();
        // at lon 0 the sample sits on the boundary between tiles: x pixel 255.5 of tile 0 and 0 of tile 1 are 1 m apart after the offset
        // of 256 per tile, so elevation is about 1000 + 255.5 + 0.5
        assert!((elev[12] - 1256.0).abs() < 0.7, "{}", elev[12]);
        // east of the centre is higher (+X is east, and the plane rises eastward)
        assert!(elev[14] > elev[10]);
        // the grid faces +Y like `grid`
        assert!(g.triangles().iter().take(3).all(|t| {
            let (a, b, c) = (g.p()[t[0] as usize], g.p()[t[1] as usize], g.p()[t[2] as usize]);
            pt_core::math::cross(pt_core::math::sub(b, a), pt_core::math::sub(c, a))[1] > 0.0
        }));
        // datum shifts y only; a hole removes the cells inside it and drops the grid flag
        p.insert("datum".into(), json!(1000.0));
        p.insert("hole".into(), json!([2000.0, 2000.0]));
        let ring = dem(&p).unwrap();
        assert_eq!(ring.n_prims(), 16 - 4, "the four central cells are left out");
        assert!(!ring.detail.contains_key("grid") && g.detail.contains_key("grid"));
        assert_eq!(ring.n_points(), 24, "the centre vertex belongs only to removed cells and is dropped, so nothing exports with a zero normal");
        assert!((ring.p()[0][1] - (elev[0] - 1000.0)).abs() < 1e-3, "heights are elevation minus the datum");
        // a missing tile names the fix
        p.insert("zoom".into(), json!(2));
        assert!(dem(&p).unwrap_err().contains("pt corpus fetch"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
