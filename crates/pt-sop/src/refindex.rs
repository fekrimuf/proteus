//! The second input of a wrangle, indexed so a program can ask it questions (`near_dist`, `surf_dist`, `ray_dist`, ...).

use pt_core::spatial::{PointTree, SurfHit, SurfaceTree};
use pt_core::Geometry;
use pt_vex::{QueryUse, RefGeo};

pub(crate) struct RefIndex {
    near: Option<PointTree>,
    surf: Option<SurfaceTree>,
}

impl RefIndex {
    /// Builds only the index the program uses: a point tree over `near_pts`, a triangle tree over the base surface of `geo`.
    pub fn build(uses: QueryUse, near_pts: &[[f32; 3]], geo: &Geometry) -> RefIndex {
        RefIndex { near: uses.near.then(|| PointTree::new(near_pts)), surf: if uses.surf { SurfaceTree::new(geo) } else { None } }
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
