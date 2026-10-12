//! The second input of a wrangle, indexed so a program can ask it questions (`near_dist`, `surf_dist`, `ray_dist`, ...), and the
//! shared cache of surface indices (a world cooks many nodes against the same terrain, and a tree over a million triangles takes
//! seconds to build).

use pt_core::spatial::{PointTree, SurfHit, SurfaceTree};
use pt_core::Geometry;
use pt_vex::{QueryUse, RefGeo};
use std::sync::{Arc, Mutex, OnceLock};

type Cache = Mutex<Vec<([u8; 32], Arc<SurfaceTree>)>>;
static TREES: OnceLock<Cache> = OnceLock::new();

/// The triangle tree of `g`'s base surface, built once per distinct geometry (by content) and kept for the next few nodes.
pub(crate) fn surface_of(g: &Geometry) -> Option<Arc<SurfaceTree>> {
    if g.n_prims() == 0 {
        return None;
    }
    let key = g.content_hash();
    let cache = TREES.get_or_init(|| Mutex::new(Vec::new()));
    if let Some((_, t)) = cache.lock().ok()?.iter().find(|(k, _)| *k == key) {
        return Some(t.clone());
    }
    let tree = Arc::new(SurfaceTree::new(g)?);
    let mut c = cache.lock().ok()?;
    c.push((key, tree.clone()));
    if c.len() > 3 {
        c.remove(0);
    }
    Some(tree)
}

pub(crate) struct RefIndex {
    near: Option<PointTree>,
    surf: Option<Arc<SurfaceTree>>,
}

impl RefIndex {
    /// Builds only the index the program uses: a point tree over `near_pts`, a triangle tree over the base surface of `geo`.
    pub fn build(uses: QueryUse, near_pts: &[[f32; 3]], geo: &Geometry) -> RefIndex {
        RefIndex { near: uses.near.then(|| PointTree::new(near_pts)), surf: if uses.surf { surface_of(geo) } else { None } }
    }
}

impl RefGeo for RefIndex {
    fn nearest_point(&self, q: [f32; 3], skip: Option<u32>) -> Option<(u32, f32)> {
        self.near.as_ref().and_then(|t| t.nearest(q, skip))
    }
    fn point_pos(&self, i: u32) -> [f32; 3] {
        self.near.as_ref().map_or([0.0; 3], |t| t.pos(i))
    }
    fn points_within(&self, q: [f32; 3], r: f32, f: &mut dyn FnMut(u32)) {
        if let Some(t) = &self.near {
            t.within(q, r, f);
        }
    }
    fn closest_surface(&self, q: [f32; 3]) -> Option<SurfHit> {
        self.surf.as_ref().map(|t| t.closest(q))
    }
    fn ray(&self, o: [f32; 3], d: [f32; 3]) -> Option<SurfHit> {
        self.surf.as_ref().and_then(|t| t.ray(o, d))
    }
}
