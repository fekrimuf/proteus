//! Level of detail. Scanned assets are authored for offline rendering (a tree can be 3.9 million triangles), so an engine
//! needs lower levels it can switch to with distance. Two techniques, chosen per material:
//!
//! * solid surfaces (trunks, rocks, ground): attribute-aware edge-collapse simplification (meshoptimizer, the library
//!   Godot and many engines use), with UVs and normals in the error metric so textures and shading survive;
//! * foliage cards (alpha-cut leaves and fronds): a deterministic share of the cards is kept and the survivors are enlarged
//!   about their centres so the canopy keeps its coverage; edge collapse would destroy a card.
//!
//! `lod_chain` turns one prototype into levels with switch-in distances (scaled by the prototype's size) and a draw
//! distance beyond which it is not drawn. The exporter writes one node per instance and level; the engine bridge turns
//! the ranges into visibility ranges.

use crate::column::Column;
use crate::geo::{gather_groups, Geometry, MATERIAL_PREFIX};
use crate::math::sq;
use meshopt::{simplify_with_attributes_and_locks, SimplifyOptions, VertexDataAdapter};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct LodPolicy {
    /// Share of triangles kept per level; the first is 1.0 (the original).
    pub ratios: Vec<f32>,
    /// Switch-in distance of each level as a multiple of the object's diagonal (level 0 always starts at 0).
    pub switch_factors: Vec<f32>,
    /// Draw distance = diagonal x `cull_factor`, clamped to [`min_cull`, `max_cull`] metres.
    pub cull_factor: f32,
    pub min_cull: f32,
    pub max_cull: f32,
    /// Meshes with fewer triangles than this get one level only.
    pub min_triangles: usize,
    /// Engine profile: level 0 of a heavier mesh is itself reduced to about this many triangles (0 = keep the original as level 0).
    pub max_level0_triangles: usize,
    /// Content-addressed disk cache for decimated levels (None = no cache).
    pub cache_dir: Option<PathBuf>,
    /// The last level of a plant (a prototype that is mostly foliage) is a SHELL: about a hundred triangles with the plant's
    /// outline, see [`shell`]. A forest a kilometre deep is then drawn with a hundred triangles per distant tree.
    pub shell: bool,
    /// Linear colour of a shell (it carries no texture).
    pub shell_color: [f32; 3],
    /// GROUND COVER (grass, ferns, pebbles: a prototype no taller than this many metres; 0 = no such class) is what a world has
    /// most of and what a viewer looks at least: it gets its own, much lower level-0 cap and a short draw distance, and an
    /// engine should not let it cast shadows.
    pub cover_height: f32,
    pub cover_level0_triangles: usize,
    pub cover_cull: f32,
    /// Ground cover gets at most this many levels (0 = as many as everything else): full, reduced, and a last one.
    pub cover_levels: usize,
    /// A prototype this tall (metres) or taller gets the full level-0 cap; a shorter one a share in proportion to its height,
    /// down to a tenth (a bush is not worth a tree's triangles). 0 = every prototype gets the full cap.
    pub full_detail_height: f32,
}

impl LodPolicy {
    /// Is this prototype ground cover under this policy?
    pub fn is_cover(&self, g: &Geometry) -> bool {
        self.cover_height > 0.0 && hinted_cull(g).is_none() && g.bounds().is_some_and(|(lo, hi)| hi[1] - lo[1] <= self.cover_height)
    }
}

impl Default for LodPolicy {
    fn default() -> Self {
        LodPolicy { ratios: vec![1.0, 0.25, 0.07, 0.02], switch_factors: vec![0.0, 3.0, 8.0, 20.0], cull_factor: 120.0, min_cull: 60.0, max_cull: 1500.0, min_triangles: 2000, max_level0_triangles: 0, cache_dir: None, shell: false, shell_color: [0.035, 0.06, 0.03], cover_height: 0.0, cover_level0_triangles: 0, cover_cull: 0.0, cover_levels: 0, full_detail_height: 0.0 }
    }
}

impl LodPolicy {
    /// What a game engine can ship. Sized for a forest, not a courtyard: a square kilometre holds tens of thousands of trees and
    /// thousands of patches of grass, so no level is above about 24 000 triangles, the steps are steep, the last level of a plant
    /// is a shell of a hundred triangles, a prototype shorter than 12 m gets a share of the cap in proportion to its height, and
    /// ground cover (under 2 m) is capped at 6 000 triangles and drawn to 90 m. (The first engine
    /// profile capped level 0 at 200 000 and ended at 4 percent; it measured well on a sparse valley and came to 240 million
    /// triangles in one frame once every instance was really drawn.)
    pub fn engine() -> LodPolicy {
        LodPolicy {
            ratios: vec![1.0, 0.2, 0.04, 0.01],
            switch_factors: vec![0.0, 1.3, 3.5, 8.0],
            max_level0_triangles: 24_000,
            cull_factor: 60.0,
            shell: true,
            cover_height: 2.0,
            cover_level0_triangles: 6_000,
            cover_cull: 90.0,
            cover_levels: 3,
            full_detail_height: 12.0,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct LodLevel {
    pub geo: Arc<Geometry>,
    pub triangles: usize,
    /// Metres (before instance scaling) where this level starts and stops being drawn.
    pub begin: f32,
    pub end: f32,
}

/// Detail key written by the `lod_hint` node: `{"cull_m": metres (0 = by size), "merge": bool}`.
pub const LOD_HINT: &str = "lod_hint";

/// Detail key: how much the surviving foliage cards of this geometry have been enlarged so far, in all passes together
/// (1 = they are the scan's own size). Every decimation pass reads it and writes it back, so a cap on card growth is a cap on the
/// whole chain: the import cap and the engine's level 0 used to enlarge the same cards one after the other (2.3 x 4 = 9 times).
pub const CARD_GROWTH: &str = "card_growth";

/// The enlargement already applied to the cards of `g` (1 when none).
pub fn card_growth(g: &Geometry) -> f32 {
    g.detail.get(CARD_GROWTH).and_then(serde_json::Value::as_f64).map_or(1.0, |v| (v as f32).max(1.0))
}

/// The draw distance a prototype asks for (`lod_hint`), if any.
pub fn hinted_cull(g: &Geometry) -> Option<f32> {
    g.detail.get(LOD_HINT).and_then(|h| h.get("cull_m")).and_then(serde_json::Value::as_f64).map(|c| c as f32).filter(|c| *c > 0.0)
}

/// Does the prototype ask for one merged mesh in offline render exports (`lod_hint`)?
pub fn hinted_merge(g: &Geometry) -> bool {
    g.detail.get(LOD_HINT).and_then(|h| h.get("merge")).and_then(serde_json::Value::as_bool).unwrap_or(false)
}

fn diagonal(g: &Geometry) -> f32 {
    g.base_bounds().map_or(1.0, |(lo, hi)| { let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]; (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3) })
}

/// Names of the materials that are alpha-cut (leaves, fronds, grass cards), recorded by `import_asset`.
fn cutout_materials(g: &Geometry) -> BTreeSet<String> {
    g.detail
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(crate::gltf::ASSET_MATERIAL_PREFIX)?, v)))
        .filter(|(_, v)| v.get("cutout").and_then(serde_json::Value::as_bool).unwrap_or(false))
        .map(|(k, _)| k.to_string())
        .collect()
}

struct Dsu(Vec<u32>);
impl Dsu {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            self.0[x as usize] = self.0[self.0[x as usize] as usize];
            x = self.0[x as usize];
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb) as usize] = ra.min(rb);
        }
    }
}

/// Deterministic value in [0, 1) from an integer (so the same cards survive on every machine and in every level).
fn hash01(x: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

/// A copy of `g` with about `ratio` of its triangles (triangles only; vertex attributes and material names are kept).
pub fn decimate(g: &Geometry, ratio: f32) -> Geometry {
    decimate_scaled(g, ratio, 4.0)
}

/// [`decimate`] with a limit on how much a surviving foliage card may grow, counted from the scan's own size across every pass
/// (`CARD_GROWTH`). A canopy keeps its coverage when its remaining cards quadruple; a patch of grass thinned to a seventh must
/// not become grass three times as tall; a geometry already enlarged by an earlier pass may grow only up to the same total.
pub fn decimate_scaled(g: &Geometry, ratio: f32, max_scale: f32) -> Geometry {
    let g = g.realize();
    let ratio = ratio.clamp(0.001, 1.0);
    let mats = g.prim_materials();
    let cutout = cutout_materials(&g);
    let (mut tris, mut tri_mat): (Vec<[u32; 3]>, Vec<Option<String>>) = (vec![], vec![]);
    for i in 0..g.n_prims() {
        let pr = g.prim_points(i);
        for k in 1..pr.len().saturating_sub(1) {
            tris.push([pr[0], pr[k], pr[k + 1]]);
            tri_mat.push(mats[i].map(String::from));
        }
    }
    let is_cutout = |t: usize| tri_mat[t].as_deref().is_some_and(|m| cutout.contains(m));
    let np = g.n_points();
    let pos: Vec<[f32; 3]> = g.p().to_vec();

    // ---- solid surfaces: attribute-aware simplification ----
    let solid: Vec<usize> = (0..tris.len()).filter(|&t| !is_cutout(t)).collect();
    let mut rewired: Vec<([u32; 3], Option<String>)> = Vec::new();
    if !solid.is_empty() {
        let solid_idx: Vec<u32> = solid.iter().flat_map(|&t| tris[t]).collect();
        let target = (((solid.len() as f32 * ratio).round() as usize).max(4)) * 3;
        let out: Vec<u32> = if target >= solid_idx.len() {
            solid_idx.clone()
        } else if let Ok(vertices) = VertexDataAdapter::new(bytemuck::cast_slice(&pos), 12, 0) {
            let n = g.point.get("N").and_then(Column::as_v3).filter(|c| c.len() == np);
            let uv = match g.point.get("uv") {
                Some(Column::V2(c)) if c.len() == np => Some(c.as_slice()),
                _ => None,
            };
            let stride = n.map_or(0, |_| 3) + uv.map_or(0, |_| 2);
            let mut attrs: Vec<f32> = Vec::with_capacity(np * stride);
            for i in 0..np {
                if let Some(n) = n {
                    attrs.extend_from_slice(&n[i]);
                }
                if let Some(uv) = uv {
                    attrs.extend_from_slice(&uv[i]);
                }
            }
            let mut weights: Vec<f32> = Vec::new();
            if n.is_some() {
                weights.extend([0.5, 0.5, 0.5]);
            }
            if uv.is_some() {
                weights.extend([1.0, 1.0]);
            }
            let locks = vec![false; np];
            simplify_with_attributes_and_locks(&solid_idx, &vertices, &attrs, &weights, stride * 4, &locks, target, 0.2, SimplifyOptions::Prune | SimplifyOptions::Sparse | SimplifyOptions::Permissive, None)
        } else {
            // the positions cannot be presented to the simplifier (never the case for 12-byte points): keep the surface as it is
            solid_idx.clone()
        };
        for t in out.chunks_exact(3) {
            // the simplifier keeps vertex ids but not triangle ids; the material of a triangle follows its first vertex's owner
            rewired.push(([t[0], t[1], t[2]], None));
        }
        // material per output triangle: look it up from the original solid triangles by vertex ownership
        let mut owner: Vec<Option<String>> = vec![None; np];
        for &t in &solid {
            for &v in &tris[t] {
                if owner[v as usize].is_none() {
                    owner[v as usize] = tri_mat[t].clone();
                }
            }
        }
        for r in rewired.iter_mut() {
            r.1 = owner[r.0[0] as usize].clone();
        }
    }

    // ---- foliage cards: keep a share of the cards (by triangle budget), enlarge the survivors ----
    // A "unit" is a whole card (a connected piece of at most CHUNK triangles) or a CHUNK-triangle run of a bigger connected
    // piece (scanned canopies weld many leaves together). Units are ranked by a hash of their first triangle, so the kept
    // set is nested from level to level and identical on every machine; each kept unit gets its own vertices and is scaled
    // about its centre so the canopy keeps its coverage.
    const CHUNK: usize = 48;
    let cards: Vec<usize> = (0..tris.len()).filter(|&t| is_cutout(t)).collect();
    let mut card_units: Vec<Vec<usize>> = Vec::new();
    let had = card_growth(&g);
    let mut scale = 1.0f32;
    if !cards.is_empty() {
        let mut dsu = Dsu((0..np as u32).collect());
        for &t in &cards {
            dsu.union(tris[t][0], tris[t][1]);
            dsu.union(tris[t][1], tris[t][2]);
        }
        let mut by_root: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for &t in &cards {
            let r = dsu.find(tris[t][0]);
            by_root.entry(r).or_default().push(t);
        }
        let mut units: Vec<Vec<usize>> = Vec::new();
        for comp in by_root.into_values() {
            if comp.len() <= CHUNK {
                units.push(comp);
            } else {
                units.extend(comp.chunks(CHUNK).map(<[usize]>::to_vec));
            }
        }
        units.sort_by(|a, b| hash01(a[0] as u32).total_cmp(&hash01(b[0] as u32)).then(a[0].cmp(&b[0])));
        let total = cards.len();
        let target = ((total as f32 * ratio).ceil() as usize).max(1);
        let mut kept = 0usize;
        for u in units {
            if kept >= target && card_units.len() >= 3 {
                break;
            }
            kept += u.len();
            card_units.push(u);
        }
        if ratio < 0.999 {
            scale = (1.0 / (kept as f32 / total as f32).sqrt()).min((max_scale / had).max(1.0));
        }
    }

    // ---- assemble: solids share vertices, every kept card unit gets its own ----
    let mut out_pos: Vec<[f32; 3]> = Vec::new();
    let mut src: Vec<u32> = Vec::new();
    let mut polys: Vec<Vec<u32>> = Vec::new();
    let mut tri_mats: Vec<Option<String>> = Vec::new();
    let mut remap = vec![u32::MAX; np];
    for (t, m) in &rewired {
        let mut p = Vec::with_capacity(3);
        for &v in t {
            if remap[v as usize] == u32::MAX {
                remap[v as usize] = out_pos.len() as u32;
                out_pos.push(pos[v as usize]);
                src.push(v);
            }
            p.push(remap[v as usize]);
        }
        polys.push(p);
        tri_mats.push(m.clone());
    }
    for unit in &card_units {
        let verts: BTreeSet<u32> = unit.iter().flat_map(|&t| tris[t]).collect();
        let mut c = [0.0f32; 3];
        for &v in &verts {
            for a in 0..3 {
                c[a] += pos[v as usize][a] / verts.len() as f32;
            }
        }
        let mut local: BTreeMap<u32, u32> = BTreeMap::new();
        for &t in unit {
            let mut p = Vec::with_capacity(3);
            for &v in &tris[t] {
                let id = *local.entry(v).or_insert_with(|| {
                    let q = pos[v as usize];
                    out_pos.push([c[0] + (q[0] - c[0]) * scale, c[1] + (q[1] - c[1]) * scale, c[2] + (q[2] - c[2]) * scale]);
                    src.push(v);
                    out_pos.len() as u32 - 1
                });
                p.push(id);
            }
            polys.push(p);
            tri_mats.push(tri_mat[t].clone());
        }
    }
    let old_ids = src;
    let mut out = Geometry::from_polys(out_pos, &polys);
    let mut set = g.point.gather(&old_ids);
    set.remove("P");
    for (k, c) in set.0 {
        out.point.set(&k, c);
    }
    out.point_groups = gather_groups(&g.point_groups, &old_ids);
    out.detail = g.detail.clone();
    if had * scale > 1.0001 {
        out.detail.insert(CARD_GROWTH.into(), serde_json::json!(had * scale));
    }
    let names: BTreeSet<&str> = tri_mats.iter().filter_map(|m| m.as_deref()).collect();
    for n in names {
        out.prim_groups.insert(format!("{MATERIAL_PREFIX}{n}"), Arc::new(tri_mats.iter().map(|m| m.as_deref() == Some(n)).collect()));
    }
    out
}

/// Words in a material name that mark foliage when the material is not alpha-cut (some scans model every needle).
const FOLIAGE_WORDS: [&str; 9] = ["twig", "leaf", "leaves", "needle", "foliage", "frond", "grass", "fern", "petal"];

/// A far-distance stand-in for a plant: a closed six-sided shell that follows the outline of its foliage (the radius that
/// holds 80 percent of the foliage at each of six heights), about a hundred triangles, one flat colour carried as the point
/// colour `Cd` under the material `lod_shell`. `None` if less than 40 percent of the triangles are foliage (a rock, a log, a
/// building: those are simplified, not replaced). Deterministic.
pub fn shell(g: &Geometry, color: [f32; 3]) -> Option<Geometry> {
    const RINGS: usize = 6;
    const SIDES: usize = 6;
    let g = g.realize();
    let mats = g.prim_materials();
    let cutout = cutout_materials(&g);
    let leafy = |m: Option<&str>| {
        m.is_some_and(|m| {
            let l = m.to_ascii_lowercase();
            cutout.contains(m) || FOLIAGE_WORDS.iter().any(|w| l.contains(w))
        })
    };
    let src = g.p();
    let (mut pts, mut leafy_tris, mut all_tris): (Vec<[f32; 3]>, usize, usize) = (vec![], 0, 0);
    for (i, m) in mats.iter().enumerate() {
        let pr = g.prim_points(i);
        let t = pr.len().saturating_sub(2);
        all_tris += t;
        if leafy(*m) {
            leafy_tris += t;
            pts.extend(pr.iter().map(|&q| src[q as usize]));
        }
    }
    if all_tris == 0 || (leafy_tris as f32) < 0.4 * all_tris as f32 || pts.len() < 12 {
        return None;
    }
    let (ylo, yhi) = pts.iter().fold((f32::MAX, f32::MIN), |(l, h), q| (l.min(q[1]), h.max(q[1])));
    if yhi - ylo < 1e-4 {
        return None;
    }
    let n = pts.len() as f64;
    let (cx, cz) = ((pts.iter().map(|q| q[0] as f64).sum::<f64>() / n) as f32, (pts.iter().map(|q| q[2] as f64).sum::<f64>() / n) as f32);
    let step = (yhi - ylo) / RINGS as f32;
    let mut radius = [0.0f32; RINGS + 1];
    for (k, r) in radius.iter_mut().enumerate() {
        let y = ylo + step * k as f32;
        let mut d: Vec<f32> = pts.iter().filter(|q| (q[1] - y).abs() <= step * 0.75).map(|q| (sq(q[0] - cx) + sq(q[2] - cz)).sqrt()).collect();
        if !d.is_empty() {
            d.sort_by(f32::total_cmp);
            *r = d[((d.len() - 1) as f32 * 0.8) as usize];
        }
    }
    let widest = radius.iter().fold(0.0f32, |a, &b| a.max(b));
    if widest < 1e-4 {
        return None;
    }
    radius[RINGS] = radius[RINGS].min(0.15 * widest);
    for r in radius.iter_mut() {
        *r = r.max(0.04 * widest); // no ring collapses to a line
    }
    let (mut out_pts, mut normals) = (Vec::with_capacity((RINGS + 1) * SIDES), Vec::with_capacity((RINGS + 1) * SIDES));
    for k in 0..=RINGS {
        let (below, above) = (radius[k.saturating_sub(1)], radius[(k + 1).min(RINGS)]);
        let lean = (below - above) / (2.0 * step); // the shell narrows upward: its normal tips up
        for s in 0..SIDES {
            let (sn, cs) = crate::math::sin_cos(2.0 * std::f32::consts::PI * s as f32 / SIDES as f32);
            out_pts.push([cx + radius[k] * cs, ylo + step * k as f32, cz + radius[k] * sn]);
            normals.push(crate::math::normalize([cs, lean, sn]));
        }
    }
    let at = |k: usize, s: usize| (k * SIDES + s % SIDES) as u32;
    let mut polys: Vec<Vec<u32>> = Vec::with_capacity(RINGS * SIDES + 2);
    for k in 0..RINGS {
        for s in 0..SIDES {
            polys.push(vec![at(k, s), at(k + 1, s), at(k + 1, s + 1), at(k, s + 1)]);
        }
    }
    polys.push((0..SIDES).map(|s| at(0, s)).collect()); // faces down
    polys.push((0..SIDES).rev().map(|s| at(RINGS, s)).collect()); // faces up
    let count = out_pts.len();
    let mut out = Geometry::from_polys(out_pts, &polys);
    out.point.set("N", Column::from_v3(normals));
    out.point.set("Cd", Column::from_v3(vec![color; count]));
    out.assign_material("lod_shell", &vec![true; polys.len()]);
    Some(out)
}

/// Bump when `decimate` changes, so cached levels from an older algorithm are never reused.
const LOD_ALGORITHM: &str = "a4";

fn cache_path(policy: &LodPolicy, g: &Geometry, ratio: f32, tag: &str) -> Option<PathBuf> {
    let dir = policy.cache_dir.as_ref()?;
    let h = g.content_hash();
    let hex: String = h.iter().take(12).map(|b| format!("{b:02x}")).collect();
    Some(dir.join(format!("{hex}_{ratio:.4}_{LOD_ALGORITHM}{tag}.ptgeo")))
}

/// The levels of one prototype: the original, then decimated copies, each with its distance range.
pub fn lod_chain(g: &Geometry, policy: &LodPolicy) -> Vec<LodLevel> {
    let real = g.realize();
    let tris = real.triangles().len();
    let diag = diagonal(&real);
    let cover = policy.is_cover(&real);
    let cull = hinted_cull(g).unwrap_or_else(|| if cover { policy.cover_cull.max(1.0) } else { (diag * policy.cull_factor).clamp(policy.min_cull, policy.max_cull) });
    let n_levels = if tris >= policy.min_triangles { policy.ratios.len().min(policy.switch_factors.len()) } else { 1 };
    let n_levels = if cover && policy.cover_levels > 0 { n_levels.min(policy.cover_levels) } else { n_levels };
    let stand_in = if policy.shell && n_levels >= 2 { shell(&real, policy.shell_color) } else { None };
    // ground cover that is thinned must not grow tall to make up for it (see `decimate_scaled`)
    let (grow, tag) = if cover { (1.25f32, "c") } else { (4.0f32, "") };
    let height = real.base_bounds().map_or(0.0, |(lo, hi)| hi[1] - lo[1]);
    let cap = if cover && policy.cover_level0_triangles > 0 {
        policy.cover_level0_triangles
    } else if policy.full_detail_height > 0.0 && policy.max_level0_triangles > 0 {
        (policy.max_level0_triangles as f32 * (height / policy.full_detail_height).clamp(0.1, 1.0)) as usize
    } else {
        policy.max_level0_triangles
    };
    let base = if cap > 0 && tris > cap { cap as f32 / tris as f32 } else { 1.0 };
    let mut geos: Vec<(Arc<Geometry>, usize)> = Vec::new();
    for l in 0..n_levels {
        if let (true, Some(s)) = (l + 1 == n_levels, &stand_in) {
            let t = s.triangles().len();
            geos.push((Arc::new(s.clone()), t));
            continue;
        }
        let ratio = base * policy.ratios[l];
        if l == 0 && base >= 1.0 {
            geos.push((Arc::new(real.clone()), tris));
            continue;
        }
        let cached = cache_path(policy, &real, ratio, tag).and_then(|p| std::fs::read(p).ok()).and_then(|b| crate::io::from_bytes(&b).ok());
        let level = cached.unwrap_or_else(|| {
            let d = decimate_scaled(&real, ratio, grow);
            if let Some(p) = cache_path(policy, &real, ratio, tag) {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(p, crate::io::to_bytes(&d));
            }
            d
        });
        let t = level.triangles().len();
        geos.push((Arc::new(level), t));
    }
    let n = geos.len();
    geos.into_iter()
        .enumerate()
        .map(|(i, (geo, triangles))| {
            let begin = if i == 0 { 0.0 } else { (diag * policy.switch_factors[i]).min(cull * 0.9) };
            let end = if i + 1 < n { (diag * policy.switch_factors[i + 1]).min(cull * 0.95) } else { cull };
            LodLevel { geo, triangles, begin, end }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A UV sphere (smooth, closed) with normals and UVs.
    fn sphere(rows: usize, cols: usize) -> Geometry {
        let mut pts = Vec::new();
        let mut uv = Vec::new();
        for j in 0..=rows {
            let t = std::f32::consts::PI * j as f32 / rows as f32;
            for i in 0..=cols {
                let p = 2.0 * std::f32::consts::PI * i as f32 / cols as f32;
                let ((st, ct), (sp, cp)) = (crate::math::sin_cos(t), crate::math::sin_cos(p));
                pts.push([st * cp, ct, st * sp]);
                uv.push([i as f32 / cols as f32, j as f32 / rows as f32]);
            }
        }
        let w = cols + 1;
        let mut polys = Vec::new();
        for j in 0..rows {
            for i in 0..cols {
                let a = (j * w + i) as u32;
                polys.push(vec![a, a + w as u32, a + 1]);
                polys.push(vec![a + 1, a + w as u32, a + w as u32 + 1]);
            }
        }
        let mut g = Geometry::from_polys(pts.clone(), &polys);
        g.point.set("N", Column::from_v3(pts));
        g.point.set("uv", Column::V2(Arc::new(uv)));
        g.assign_material("stone", &vec![true; g.n_prims()]);
        g
    }

    /// `n` separate leaf cards (two triangles each) scattered in a unit cube, in an alpha-cut material.
    fn canopy(n: usize) -> Geometry {
        let mut pts = Vec::new();
        let mut polys = Vec::new();
        for k in 0..n {
            let c = [hash01(k as u32 * 3), hash01(k as u32 * 3 + 1), hash01(k as u32 * 3 + 2)];
            let b = pts.len() as u32;
            pts.extend([[c[0], c[1], c[2]], [c[0] + 0.05, c[1], c[2]], [c[0] + 0.05, c[1] + 0.05, c[2]], [c[0], c[1] + 0.05, c[2]]]);
            polys.push(vec![b, b + 1, b + 2]);
            polys.push(vec![b, b + 2, b + 3]);
        }
        let mut g = Geometry::from_polys(pts, &polys);
        g.assign_material("tree__leaves", &vec![true; g.n_prims()]);
        g.detail.insert("material:tree__leaves".into(), json!({"source": "x.gltf", "index": 0, "cutout": true}));
        g
    }

    #[test]
    fn solid_surfaces_lose_triangles_but_keep_shape_materials_and_attributes() {
        let g = sphere(64, 96);
        let full = g.triangles().len();
        let d = decimate(&g, 0.1);
        let got = d.triangles().len();
        assert!(got < full / 5 && got > full / 40, "{got} of {full} triangles");
        assert_eq!(d.material_names(), ["stone"]);
        assert!(d.point.get("uv").is_some() && d.point.get("N").is_some(), "attributes survive");
        // the shape stays a unit sphere: every kept vertex is on (or very near) the surface
        assert!(d.p().iter().all(|p| ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0).abs() < 0.05));
        assert!(pt_validate_finite(&d));
    }

    fn pt_validate_finite(g: &Geometry) -> bool {
        g.p().iter().flatten().all(|v| v.is_finite()) && g.topo.indices.iter().all(|&i| (i as usize) < g.n_points())
    }

    #[test]
    fn foliage_cards_are_culled_whole_and_the_survivors_grow() {
        let g = canopy(400);
        let d = decimate(&g, 0.25);
        let cards = d.triangles().len() / 2;
        assert!((60..=140).contains(&cards), "about a quarter of 400 cards survive, got {cards}");
        // survivors are enlarged by 1/sqrt(0.25) = 2: a 0.05 card is now 0.1 wide
        let (lo, hi) = d.base_bounds().unwrap();
        assert!(hi[0] - lo[0] > 0.9, "the canopy keeps its extent");
        let w = d.p().chunks(4).next().map(|c| c[1][0] - c[0][0]).unwrap();
        assert!((w - 0.1).abs() < 1e-4, "card width {w}");
        // deterministic and nested: the cards kept at a lower share are a subset of those kept at a higher one
        assert_eq!(decimate(&g, 0.25).content_hash(), d.content_hash());
        assert!(decimate(&g, 0.1).triangles().len() < d.triangles().len());
        assert_eq!(d.material_names(), ["tree__leaves"]);
    }


    /// Width of the first surviving card of a canopy level (each card keeps its own four points).
    fn card_width(g: &Geometry) -> f32 {
        g.p().chunks(4).next().map(|c| c[1][0] - c[0][0]).unwrap()
    }

    #[test]
    fn needle_cards_are_never_enlarged_past_the_cap_however_many_passes_cut_them() {
        // The creek's fir sapling was cut twice: by the import cap (about 2.3 x) and again by the engine's level 0 (4 x), so its
        // needles came out 9 times their real length in the survey's village frame. The growth is now counted over all passes.
        let scan = canopy(4000); // cards 0.05 wide
        let imported = decimate(&scan, 0.25); // the import cap's pass
        assert!((card_growth(&imported) - 2.0).abs() < 0.01, "the pass records its growth: {}", card_growth(&imported));
        let policy = LodPolicy { max_level0_triangles: 500, min_triangles: 0, cover_height: 0.0, full_detail_height: 0.0, shell: false, ..LodPolicy::engine() };
        let chain = lod_chain(&imported, &policy);
        assert!(chain[0].triangles < imported.triangles().len() / 2, "level 0 is cut again: {}", chain[0].triangles);
        for (l, level) in chain.iter().enumerate() {
            let cap = 4.0f32;
            let width = card_width(&level.geo);
            assert!(width <= 0.05 * cap * 1.001, "level {l}: a card is {:.2} x its scan size, the cap is {cap}", width / 0.05);
            assert!(card_growth(&level.geo) <= cap * 1.001, "level {l} records {}", card_growth(&level.geo));
        }
        // a geometry no pass has touched still grows to the cap and no further
        let alone = lod_chain(&scan, &policy);
        let w0 = card_width(&alone[0].geo);
        assert!((w0 - 0.05 * 4.0).abs() < 0.002, "level 0 of a fresh scan grows to its cap: {}", w0 / 0.05);
    }
    #[test]
    fn welded_canopies_are_cut_into_chunks_and_small_plants_never_vanish() {
        // one connected canopy: a 100 x 100 grid of triangles (20 000) in a cutout material, ONE component
        let n = 100u32;
        let pts: Vec<[f32; 3]> = (0..=n).flat_map(|j| (0..=n).map(move |i| [i as f32 * 0.1, 0.0, j as f32 * 0.1])).collect();
        let polys: Vec<Vec<u32>> = (0..n).flat_map(|j| (0..n).flat_map(move |i| { let a = j * (n + 1) + i; [vec![a, a + n + 1, a + 1], vec![a + 1, a + n + 1, a + n + 2]] })).collect();
        let mut big = Geometry::from_polys(pts, &polys);
        big.assign_material("leaves", &vec![true; big.n_prims()]);
        big.detail.insert("material:leaves".into(), json!({"source": "x.gltf", "index": 0, "cutout": true}));
        let d = decimate(&big, 0.1);
        let kept = d.triangles().len();
        assert!((1800..=2400).contains(&kept), "about 10% of 20 000 triangles, got {kept}");
        // a plant of 5 cards (10 triangles) at 2% still keeps at least 3 cards
        let plant = canopy(5);
        let p = decimate(&plant, 0.02);
        assert!(p.triangles().len() >= 6, "never vanishes: {} triangles", p.triangles().len());
    }

    #[test]
    fn the_engine_profile_caps_level_zero() {
        let g = sphere(64, 96); // 12 288 triangles
        let policy = LodPolicy { max_level0_triangles: 3000, full_detail_height: 0.0, cover_height: 0.0, ..LodPolicy::engine() };
        let chain = lod_chain(&g, &policy);
        let t: Vec<usize> = chain.iter().map(|l| l.triangles).collect();
        assert!(t[0] <= 3600 && t[0] >= 2400, "level 0 is reduced to about the cap, got {t:?}");
        assert!(t.windows(2).all(|w| w[1] < w[0]), "{t:?}");
        // below the cap nothing changes: level 0 is the original
        let small = lod_chain(&sphere(32, 48), &LodPolicy { max_level0_triangles: 250_000, ..LodPolicy::engine() });
        assert_eq!(small[0].triangles, 32 * 48 * 2);
        // the full engine profile: the sphere is 2 m tall, so it is ground cover: capped at 6 000 and drawn to 90 m
        let cover = lod_chain(&g, &LodPolicy::engine());
        assert!(cover[0].triangles <= 7200 && cover[0].triangles >= 4800, "{}", cover[0].triangles);
        assert_eq!(cover.last().unwrap().end, 90.0);
        // taller than cover but shorter than 12 m: a share of the cap in proportion to its height (6 m of 12 m: half of 40 000 > 12 288)
        let mut tall = g.clone();
        for q in tall.p_mut() {
            *q = [q[0], q[1] * 3.0, q[2]];
        }
        let chain = lod_chain(&tall, &LodPolicy { max_level0_triangles: 12_000, ..LodPolicy::engine() });
        assert!(chain[0].triangles <= 7200 && chain[0].triangles >= 4800, "half the cap for a 6 m object: {}", chain[0].triangles);
        // a hinted far stand-in keeps its own draw distance
        let mut hinted = g.clone();
        hinted.detail.insert(LOD_HINT.into(), json!({"cull_m": 4200.0, "merge": true}));
        assert_eq!(lod_chain(&hinted, &LodPolicy::engine()).last().unwrap().end, 4200.0);
        assert!(hinted_merge(&hinted) && !hinted_merge(&g));
    }

    #[test]
    fn a_chain_has_levels_ranges_that_tile_and_a_draw_distance() {
        let g = sphere(64, 96);
        let chain = lod_chain(&g, &LodPolicy::default());
        assert_eq!(chain.len(), 4);
        assert!(chain.windows(2).all(|w| w[1].triangles < w[0].triangles), "{:?}", chain.iter().map(|l| l.triangles).collect::<Vec<_>>());
        assert_eq!(chain[0].begin, 0.0);
        assert!(chain.windows(2).all(|w| (w[0].end - w[1].begin).abs() < 1e-3 || w[1].begin >= w[0].end * 0.9), "ranges follow each other");
        assert!(chain.last().unwrap().end >= 60.0, "draw distance has a floor");
        // a small mesh is one level only
        assert_eq!(lod_chain(&sphere(8, 8), &LodPolicy::default()).len(), 1);
    }

    /// A conifer: a thin trunk (not foliage) and needle cards whose spread narrows from 3 m at the foot to nothing at 12 m.
    fn conifer(cards: usize) -> Geometry {
        let mut pts = vec![[-0.1, 0.0, 0.0], [0.1, 0.0, 0.0], [0.0, 12.0, 0.0]];
        let mut polys = vec![vec![0u32, 1, 2]];
        for k in 0..cards {
            let (h, a, d) = (hash01(k as u32 * 3), hash01(k as u32 * 3 + 1) * 6.283, hash01(k as u32 * 3 + 2));
            let y = 1.0 + 11.0 * h;
            let r = 3.0 * (1.0 - h) * d.sqrt();
            let (s, c) = crate::math::sin_cos(a);
            let b = pts.len() as u32;
            pts.extend([[r * c, y, r * s], [r * c + 0.1, y, r * s], [r * c, y + 0.1, r * s]]);
            polys.push(vec![b, b + 1, b + 2]);
        }
        let mut g = Geometry::from_polys(pts, &polys);
        let twig: Vec<bool> = (0..g.n_prims()).map(|i| i > 0).collect();
        g.assign_material("fir__twig", &twig);
        g.assign_material("fir__bark", &twig.iter().map(|t| !t).collect::<Vec<_>>());
        g
    }

    #[test]
    fn a_plant_gets_a_shell_that_follows_its_outline_and_a_rock_does_not() {
        let tree = conifer(4000);
        let s = shell(&tree, [0.03, 0.06, 0.03]).expect("mostly foliage: gets a shell");
        assert_eq!(s.triangles().len(), 6 * 6 * 2 + 4 + 4, "six rings of six quads and two caps");
        assert_eq!(s.material_names(), ["lod_shell"]);
        assert_eq!(s.point.get("Cd").and_then(Column::as_v3).unwrap()[0], [0.03, 0.06, 0.03]);
        assert!(pt_validate_finite(&s));
        // the outline: as tall as the foliage, wide at the foot, narrow at the tip, and inside the foliage's own spread
        let (lo, hi) = s.base_bounds().unwrap();
        assert!((lo[1] - 1.0).abs() < 0.2 && (hi[1] - 12.0).abs() < 0.2, "{lo:?} {hi:?}");
        let width = |y0: f32, y1: f32| s.p().iter().filter(|q| q[1] >= y0 && q[1] <= y1).map(|q| (q[0] * q[0] + q[2] * q[2]).sqrt()).fold(0.0f32, f32::max);
        assert!(width(0.9, 1.2) > 1.8 && width(0.9, 1.2) < 3.1, "foot radius {}", width(0.9, 1.2));
        assert!(width(11.5, 12.5) < 0.6, "tip radius {}", width(11.5, 12.5));
        // it faces outward: every side quad's normal points away from the axis
        for i in 0..36 {
            let pr = s.prim_points(i);
            let (a, b, c) = (s.p()[pr[0] as usize], s.p()[pr[1] as usize], s.p()[pr[2] as usize]);
            let n = crate::math::cross(crate::math::sub(b, a), crate::math::sub(c, a));
            assert!(n[0] * a[0] + n[2] * a[2] > 0.0, "quad {i} faces inward");
        }
        assert_eq!(shell(&tree, [0.03, 0.06, 0.03]).unwrap().content_hash(), s.content_hash(), "deterministic");
        assert!(shell(&sphere(16, 24), [0.0; 3]).is_none(), "a rock is simplified, not replaced");
        // in the engine profile the shell is the plant's last level; without the flag it is an ordinary reduced mesh
        let chain = lod_chain(&tree, &LodPolicy::engine());
        assert_eq!(chain.last().unwrap().triangles, 80);
        assert_eq!(chain.last().unwrap().geo.material_names(), ["lod_shell"]);
        assert!(chain[..chain.len() - 1].iter().all(|l| l.geo.material_names().contains(&"fir__twig".to_string())));
        let plain = lod_chain(&tree, &LodPolicy { shell: false, ..LodPolicy::engine() });
        assert!(plain.last().unwrap().geo.material_names().contains(&"fir__twig".to_string()));
        assert!(lod_chain(&sphere(64, 96), &LodPolicy::engine()).iter().all(|l| l.geo.material_names() == ["stone"]));
    }
}

#[cfg(test)]
mod scan_probe {
    use super::*;

    /// Not a gate: prints where the triangles go when a real scan is decimated (run with --ignored --nocapture).
    #[test]
    #[ignore]
    fn jacaranda_breakdown() {
        let path = crate::gltf::corpus_root().join("polyhaven/models/jacaranda_tree/jacaranda_tree_1k.gltf");
        let scene = crate::gltf::load_file(&path).unwrap();
        let g = scene.merged();
        println!("materials: {:?}", g.material_names());
        let mats = g.prim_materials();
        let mut per: BTreeMap<String, usize> = BTreeMap::new();
        for (i, m) in mats.iter().enumerate() {
            *per.entry(m.unwrap_or("-").to_string()).or_default() += g.prim_points(i).len().saturating_sub(2);
        }
        println!("triangles per material: {per:?}");
        println!("cutout: {:?}", cutout_materials(&g));
        for ratio in [0.25f32, 0.07, 0.02] {
            let t = std::time::Instant::now();
            let d = decimate(&g, ratio);
            let dm = d.prim_materials();
            let mut per: BTreeMap<String, usize> = BTreeMap::new();
            for (i, m) in dm.iter().enumerate() {
                *per.entry(m.unwrap_or("-").to_string()).or_default() += d.prim_points(i).len().saturating_sub(2);
            }
            println!("ratio {ratio}: {} tris in {:.1}s {per:?}", d.triangles().len(), t.elapsed().as_secs_f64());
        }
    }
}
