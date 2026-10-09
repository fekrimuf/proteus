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
