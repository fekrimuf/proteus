//! The questions a program can ask of a second geometry (the "ref"): what is the nearest point, how many points are within a
//! radius, what is the closest surface, what does a ray hit. The language side lives here; the geometry side implements `RefGeo`.

use crate::Ty;
use pt_core::spatial::SurfHit;
use std::collections::BTreeMap;

/// What a program may ask. Implemented over a real geometry by the wrangle node; deterministic (equal distances go to the lower index).
pub trait RefGeo: Sync {
    /// Index and distance of the closest point, ignoring point `skip`.
    fn nearest_point(&self, q: [f32; 3], skip: Option<u32>) -> Option<(u32, f32)>;
    fn point_pos(&self, i: u32) -> [f32; 3];
    /// Calls `f` with every point within `r` of `q`, in a fixed order.
    fn points_within(&self, q: [f32; 3], r: f32, f: &mut dyn FnMut(u32));
    fn closest_surface(&self, q: [f32; 3]) -> Option<SurfHit>;
    /// First surface met by the ray from `o` along `d`.
    fn ray(&self, o: [f32; 3], d: [f32; 3]) -> Option<SurfHit>;
}

/// Where an attribute lookup reads from: the points (or instances) of the ref, or interpolated over its surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Near,
    Surf,
}

/// One attribute of the ref that the program reads (an entry of `Program::refs`).
#[derive(Clone, Debug)]
pub struct RefAttr {
    pub name: String,
    pub ty: Ty,
    pub scope: Scope,
}

/// The attributes the ref offers, by scope, so the checker can resolve names and types before anything runs.
pub struct RefEnv<'a> {
    pub near: &'a BTreeMap<String, Ty>,
    pub surf: &'a BTreeMap<String, Ty>,
}

/// Which index a program needs, so the host builds only that one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryUse {
    pub near: bool,
    pub surf: bool,
}

/// The ref at run time: the geometry and one view per `Program::refs` entry.
pub struct RefQuery<'a> {
    pub geo: &'a dyn RefGeo,
    pub attrs: Vec<Option<crate::AttrView<'a>>>,
}

/// Distance reported when the ref has nothing to measure to (large, so `smoothstep` and `min` behave).
pub const FAR: f32 = 1.0e9;
