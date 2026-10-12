//! Rigid bodies via rapier3d (single-threaded, `enhanced-determinism`): each piece becomes a
//! dynamic body with a convex-hull collider; a ground plane and an optional radial impact
//! impulse drive the motion. Returns one affine transform per piece.

use pt_core::math::{self, Mat34};
use pt_core::rng::rand4;
use rapier3d::prelude::*;

#[derive(Clone, Debug)]
pub struct RbdParams {
    pub frame: usize,
    pub fps: f32,
    pub substeps: usize,
    pub gravity: [f32; 3],
    pub ground_y: f32,
    pub friction: f32,
    pub restitution: f32,
    pub density: f32,
    pub impact_center: [f32; 3],
    pub impact_radius: f32,
    /// Peak launch speed (m/s) at the impact center, falling off linearly to the radius.
    pub impact_strength: f32,
    pub spin: f32,
    pub seed: i64,
}

impl Default for RbdParams {
    fn default() -> Self {
        RbdParams { frame: 48, fps: 24.0, substeps: 4, gravity: [0.0, -9.81, 0.0], ground_y: 0.0, friction: 0.7, restitution: 0.15, density: 1000.0, impact_center: [0.0; 3], impact_radius: 0.0, impact_strength: 0.0, spin: 2.0, seed: 1 }
    }
}

fn quat_to_rot(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// Simulate `pieces` (world-space rest points each) and return, per piece, the transform that
/// maps its rest points to their position at `prm.frame`.
pub fn simulate(pieces: &[Vec<[f32; 3]>], prm: &RbdParams) -> Vec<Mat34> {
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut impulse_joints = ImpulseJointSet::new();
    let mut multibody_joints = MultibodyJointSet::new();
    let mut soft_bodies = SoftBodySet::new();
    let mut pipeline = PhysicsPipeline::new();
    let mut bf = BroadPhaseBvh::new();
    let mut nf = NarrowPhase::new();
    let mut islands = IslandManager::new();
    let mut ccd = CCDSolver::new();
    let substeps = prm.substeps.max(1);
    let mut params = IntegrationParameters::default();
    params.dt = 1.0 / (prm.fps.max(1.0) * substeps as f32);

    let ground = bodies.insert(RigidBodyBuilder::fixed().translation(Vector::new(0.0, prm.ground_y - 0.5, 0.0)));
    colliders.insert_with_parent(ColliderBuilder::cuboid(500.0, 0.5, 500.0).friction(prm.friction).restitution(prm.restitution), ground, &mut bodies);

    let mut handles = Vec::with_capacity(pieces.len());
    let mut centers = Vec::with_capacity(pieces.len());
    for (i, pts) in pieces.iter().enumerate() {
        let n = pts.len().max(1) as f32;
        let c = [0, 1, 2].map(|a| pts.iter().map(|p| p[a]).sum::<f32>() / n);
        let local: Vec<Vector> = pts.iter().map(|p| Vector::new(p[0] - c[0], p[1] - c[1], p[2] - c[2])).collect();
        let d = math::sub(c, prm.impact_center);
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let mut vel = [0.0f32; 3];
        if prm.impact_radius > 0.0 && dist < prm.impact_radius {
            let fall = 1.0 - dist / prm.impact_radius;
            let dir = math::normalize([d[0], d[1] + 0.3 * prm.impact_radius.min(1.0), d[2]]);
            vel = dir.map(|x| x * prm.impact_strength * fall);
        }
        let r = rand4(prm.seed, 61, i as u64);
        let ang = Vector::new((r[0] - 0.5) * prm.spin, (r[1] - 0.5) * prm.spin, (r[2] - 0.5) * prm.spin);
        let body = bodies.insert(RigidBodyBuilder::dynamic().translation(Vector::new(c[0], c[1], c[2])).linvel(Vector::new(vel[0], vel[1], vel[2])).angvel(ang).ccd_enabled(true));
        let builder = ColliderBuilder::convex_hull(&local).unwrap_or_else(|| {
            let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for p in &local {
                lo = [lo[0].min(p.x), lo[1].min(p.y), lo[2].min(p.z)];
                hi = [hi[0].max(p.x), hi[1].max(p.y), hi[2].max(p.z)];
            }
            ColliderBuilder::cuboid(((hi[0] - lo[0]) * 0.5).max(0.01), ((hi[1] - lo[1]) * 0.5).max(0.01), ((hi[2] - lo[2]) * 0.5).max(0.01))
        });
        colliders.insert_with_parent(builder.density(prm.density).friction(prm.friction).restitution(prm.restitution), body, &mut bodies);
        handles.push(body);
        centers.push(c);
    }

    let gravity = Vector::new(prm.gravity[0], prm.gravity[1], prm.gravity[2]);
    for _ in 0..prm.frame * substeps {
        pipeline.step(gravity, &params, &mut islands, &mut bf, &mut nf, &mut bodies, &mut colliders, &mut impulse_joints, &mut multibody_joints, &mut soft_bodies, &mut ccd, &(), &());
    }

    handles
        .iter()
        .zip(&centers)
        .map(|(h, c)| {
            let b = &bodies[*h];
            let t = b.translation();
            let q = b.rotation();
            let r = quat_to_rot([q.x, q.y, q.z, q.w]);
            // world = R * (p - c) + t = R p + (t - R c)
            let rc = [0, 1, 2].map(|i| r[i][0] * c[0] + r[i][1] * c[1] + r[i][2] * c[2]);
            [r[0][0], r[0][1], r[0][2], t.x - rc[0], r[1][0], r[1][1], r[1][2], t.y - rc[1], r[2][0], r[2][1], r[2][2], t.z - rc[2]]
        })
        .collect()
}

/// A body to settle: the convex shape of one object (points relative to its origin, already scaled and turned the way the object
/// stands) and where its origin starts.
pub struct Body {
    pub hull: Vec<[f32; 3]>,
    pub origin: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct SettleParams {
    pub frames: usize,
    pub fps: f32,
    pub substeps: usize,
    pub gravity: [f32; 3],
    pub friction: f32,
    pub restitution: f32,
    pub density: f32,
    /// Angular damping: resistance to rolling. 0 lets a log roll down any slope; dressing wants them to stay.
    pub rolling: f32,
}

impl Default for SettleParams {
    fn default() -> Self {
        SettleParams { frames: 300, fps: 60.0, substeps: 2, gravity: [0.0, -9.81, 0.0], friction: 0.8, restitution: 0.05, density: 1500.0, rolling: 6.0 }
    }
}

/// Where a settled body ended: the turn and the position of its origin, and whether it came to rest.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub rot: [[f32; 3]; 3],
    pub pos: [f32; 3],
    pub rest: bool,
}

/// Lets `bodies` fall onto a fixed triangle mesh (`ground_pts`, `ground_tris`) and returns where each came to rest. Runs until every
/// body sleeps or `frames` are used up. Deterministic (single-threaded, enhanced determinism).
pub fn settle(bodies: &[Body], ground_pts: &[[f32; 3]], ground_tris: &[[u32; 3]], prm: &SettleParams) -> Result<Vec<Pose>, String> {
    let mut set = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut impulse_joints = ImpulseJointSet::new();
    let mut multibody_joints = MultibodyJointSet::new();
    let mut soft_bodies = SoftBodySet::new();
    let mut pipeline = PhysicsPipeline::new();
    let mut bf = BroadPhaseBvh::new();
    let mut nf = NarrowPhase::new();
    let mut islands = IslandManager::new();
    let mut ccd = CCDSolver::new();
    let substeps = prm.substeps.max(1);
    let mut params = IntegrationParameters::default();
    params.dt = 1.0 / (prm.fps.max(1.0) * substeps as f32);

    let ground = set.insert(RigidBodyBuilder::fixed());
    let verts: Vec<Vector> = ground_pts.iter().map(|p| Vector::new(p[0], p[1], p[2])).collect();
    let tri = ColliderBuilder::trimesh(verts, ground_tris.to_vec()).map_err(|e| format!("the ground mesh cannot be a collider: {e:?}"))?;
    colliders.insert_with_parent(tri.friction(prm.friction).restitution(prm.restitution), ground, &mut set);

    let mut handles = Vec::with_capacity(bodies.len());
    for b in bodies {
        let local: Vec<Vector> = b.hull.iter().map(|p| Vector::new(p[0], p[1], p[2])).collect();
        let body = set.insert(
            RigidBodyBuilder::dynamic().translation(Vector::new(b.origin[0], b.origin[1], b.origin[2])).linear_damping(0.5).angular_damping(prm.rolling.max(0.0)).ccd_enabled(true),
        );
        let shape = ColliderBuilder::convex_hull(&local).unwrap_or_else(|| {
            let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for p in &local {
                lo = [lo[0].min(p.x), lo[1].min(p.y), lo[2].min(p.z)];
                hi = [hi[0].max(p.x), hi[1].max(p.y), hi[2].max(p.z)];
            }
            ColliderBuilder::cuboid(((hi[0] - lo[0]) * 0.5).max(0.01), ((hi[1] - lo[1]) * 0.5).max(0.01), ((hi[2] - lo[2]) * 0.5).max(0.01))
        });
        colliders.insert_with_parent(shape.density(prm.density).friction(prm.friction).restitution(prm.restitution), body, &mut set);
        handles.push(body);
    }

    let gravity = Vector::new(prm.gravity[0], prm.gravity[1], prm.gravity[2]);
    let check_every = 10 * substeps;
    for step in 0..prm.frames * substeps {
        pipeline.step(gravity, &params, &mut islands, &mut bf, &mut nf, &mut set, &mut colliders, &mut impulse_joints, &mut multibody_joints, &mut soft_bodies, &mut ccd, &(), &());
        if step % check_every == check_every - 1 && handles.iter().all(|h| set[*h].is_sleeping()) {
            break;
        }
    }
    Ok(handles
        .iter()
        .map(|h| {
            let b = &set[*h];
            let (t, q) = (b.translation(), b.rotation());
            let (v, w) = (b.linvel(), b.angvel());
            let moving = v.length() > 0.05 || w.length() > 0.2;
            Pose { rot: quat_to_rot([q.x, q.y, q.z, q.w]), pos: [t.x, t.y, t.z], rest: b.is_sleeping() || !moving }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(c: [f32; 3], s: f32) -> Vec<[f32; 3]> {
        (0..8).map(|i| [c[0] + if i & 1 == 1 { s } else { -s }, c[1] + if i & 2 == 2 { s } else { -s }, c[2] + if i & 4 == 4 { s } else { -s }]).collect()
    }

    fn apply(m: &Mat34, pts: &[[f32; 3]]) -> Vec<[f32; 3]> {
        pts.iter().map(|p| math::xform_point(m, *p)).collect()
    }

    #[test]
    fn dropped_pieces_land_on_the_ground_and_do_not_tunnel() {
        let pieces = vec![cube([0.0, 3.0, 0.0], 0.25), cube([0.1, 4.0, 0.0], 0.25), cube([3.0, 2.0, 1.0], 0.2)];
        let t = simulate(&pieces, &RbdParams { frame: 120, ..Default::default() });
        for (m, p) in t.iter().zip(&pieces) {
            let w = apply(m, p);
            let min_y = w.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min);
            assert!(min_y > -0.03, "piece sank into the ground: {min_y}");
            assert!(min_y < 0.6, "piece is resting on the ground or stacked on another piece: {min_y}");
            assert!(w.iter().all(|q| q.iter().all(|c| c.is_finite())));
            // Rigid: the pairwise distances between corners are preserved.
            let d0 = |a: [f32; 3], b: [f32; 3]| {
                let d = math::sub(a, b);
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            };
            assert!((d0(w[0], w[7]) - d0(p[0], p[7])).abs() < 1e-3, "piece deformed");
        }
    }

    #[test]
    fn bodies_settle_on_a_tilted_mesh_and_do_not_sink_through_it() {
        // a ground tilted 20 degrees about z (rises with x), 40 m square
        let (c, s) = (20.0f32.to_radians().cos(), 20.0f32.to_radians().sin());
        let g = |x: f32, z: f32| [x * c, x * s, z];
        let pts = vec![g(-20.0, -20.0), g(20.0, -20.0), g(20.0, 20.0), g(-20.0, 20.0)];
        let tris = vec![[0, 1, 2], [0, 2, 3]];
        let height_at = |x: f32| x * s / c; // ground height under a point at world x
        let hull = |h: f32| cube([0.0, 0.0, 0.0], h);
        let bodies: Vec<Body> = [(2.0f32, 0.3f32), (5.0, 0.25), (-4.0, 0.4), (8.0, 0.2)].iter().enumerate().map(|(i, (x, h))| Body { hull: hull(*h), origin: [*x, height_at(*x) + 0.8, i as f32 - 1.0] }).collect();
        let prm = SettleParams::default();
        let a = settle(&bodies, &pts, &tris, &prm).unwrap();
        for (b, p) in bodies.iter().zip(&a) {
            assert!(p.rest, "a body must come to rest");
            let world: Vec<[f32; 3]> = b.hull.iter().map(|q| [0, 1, 2].map(|r| p.rot[r][0] * q[0] + p.rot[r][1] * q[1] + p.rot[r][2] * q[2] + p.pos[r])).collect();
            // every corner is on or above the ground plane (distance along its normal), and the lowest one touches it
            let n = [-s, c, 0.0f32];
            let d: Vec<f32> = world.iter().map(|q| (q[0] - pts[0][0]) * n[0] + (q[1] - pts[0][1]) * n[1] + (q[2] - pts[0][2]) * n[2]).collect();
            let lowest = d.iter().cloned().fold(f32::INFINITY, f32::min);
            assert!(lowest > -0.03, "a corner is below the ground by {}", -lowest);
            assert!(lowest < 0.03, "the body hovers {lowest} above the ground");
        }
        let b = settle(&bodies, &pts, &tris, &prm).unwrap();
        assert!(a.iter().zip(&b).all(|(x, y)| x.pos == y.pos && x.rot == y.rot), "settling is deterministic");
    }

    #[test]
    fn impact_pushes_pieces_away_and_runs_are_deterministic() {
        let pieces = vec![cube([1.0, 0.25, 0.0], 0.25), cube([-1.0, 0.25, 0.0], 0.25), cube([8.0, 0.25, 0.0], 0.25)];
        let prm = RbdParams { frame: 36, impact_center: [0.0, 0.25, 0.0], impact_radius: 3.0, impact_strength: 8.0, ..Default::default() };
        let a = simulate(&pieces, &prm);
        let b = simulate(&pieces, &prm);
        assert_eq!(a, b);
        let cx = |m: &Mat34, p: &Vec<[f32; 3]>| apply(m, p).iter().map(|q| q[0]).sum::<f32>() / 8.0;
        assert!(cx(&a[0], &pieces[0]) > 1.5, "right piece pushed right: {}", cx(&a[0], &pieces[0]));
        assert!(cx(&a[1], &pieces[1]) < -1.5, "left piece pushed left");
        assert!((cx(&a[2], &pieces[2]) - 8.0).abs() < 0.2, "piece outside the radius stays put");
    }
}
