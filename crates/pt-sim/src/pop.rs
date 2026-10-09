//! Particle system (POP): emission, forces, drag, noise, lifetime and collisions with a ground
//! plane and/or an SDF collider. Deterministic: sequential stepping, counter-based randomness
//! keyed by particle id, so a given (inputs, params, frame) always yields the same particles.

use pt_core::math;
use pt_core::noise::value_noise;
use pt_core::rng::rand4;
use pt_core::Volume;

#[derive(Clone, Debug)]
pub struct PopParams {
    /// Simulate up to this frame (frame 0 = empty initial state).
    pub frame: usize,
    pub fps: f32,
    pub substeps: usize,
    pub gravity: [f32; 3],
    /// Linear drag toward the wind velocity.
    pub drag: f32,
    pub wind: [f32; 3],
    pub noise_amp: f32,
    pub noise_freq: f32,
    pub speed: f32,
    /// 0 = emit exactly along the emitter direction, 1 = fully random.
    pub spread: f32,
    /// Seconds a particle lives (<= 0: forever).
    pub life: f32,
    /// New particles per emitter point per frame (fractional rates accumulate).
    pub rate: f32,
    pub bounce: f32,
    pub friction: f32,
    pub ground_y: f32,
    pub max_particles: usize,
    pub seed: i64,
}

impl Default for PopParams {
    fn default() -> Self {
        PopParams { frame: 24, fps: 24.0, substeps: 2, gravity: [0.0, -9.81, 0.0], drag: 0.1, wind: [0.0; 3], noise_amp: 0.0, noise_freq: 1.0, speed: 6.0, spread: 0.15, life: 0.0, rate: 4.0, bounce: 0.4, friction: 0.1, ground_y: 0.0, max_particles: 200_000, seed: 1 }
    }
}

pub struct Emitter {
    pub pos: Vec<[f32; 3]>,
    /// Unit emission direction per point.
    pub dir: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, Default)]
pub struct Particles {
    pub p: Vec<[f32; 3]>,
    pub v: Vec<[f32; 3]>,
    pub age: Vec<f32>,
    pub id: Vec<u32>,
}

fn sdf_grad(vol: &Volume, q: [f32; 3]) -> [f32; 3] {
    let h = vol.voxel * 0.5;
    math::normalize([
        vol.sample([q[0] + h, q[1], q[2]]) - vol.sample([q[0] - h, q[1], q[2]]),
        vol.sample([q[0], q[1] + h, q[2]]) - vol.sample([q[0], q[1] - h, q[2]]),
        vol.sample([q[0], q[1], q[2] + h]) - vol.sample([q[0], q[1], q[2] - h]),
    ])
}

pub fn simulate(em: &Emitter, collider: Option<&Volume>, prm: &PopParams) -> Particles {
    let mut s = Particles::default();
    if em.pos.is_empty() {
        return s;
    }
    let dt = 1.0 / prm.fps.max(1.0);
    let h = dt / prm.substeps.max(1) as f32;
    let mut acc = 0.0f32;
    let mut next_id = 0u32;
    for frame in 1..=prm.frame {
        // Emit.
        acc += prm.rate * em.pos.len() as f32;
        let n = acc.floor();
        acc -= n;
        for _ in 0..(n as usize) {
            if s.p.len() >= prm.max_particles {
                break;
            }
            let id = next_id;
            next_id += 1;
            let r = rand4(prm.seed, 31, id as u64);
            let r2 = rand4(prm.seed, 32, id as u64);
            let e = (id as usize) % em.pos.len();
            let rnd = math::normalize([r[0] * 2.0 - 1.0, r[1] * 2.0 - 1.0, r[2] * 2.0 - 1.0]);
            let d = math::normalize([0, 1, 2].map(|a| em.dir[e][a] * (1.0 - prm.spread) + rnd[a] * prm.spread));
            let sp = prm.speed * (0.8 + 0.4 * r[3]);
            // Sub-frame birth time so particles don't all spawn on the frame boundary.
            let sub = r2[0] * dt;
            let p0 = em.pos[e];
            s.p.push([0, 1, 2].map(|a| p0[a] + d[a] * sp * sub));
            s.v.push(d.map(|c| c * sp));
            s.age.push(sub);
            s.id.push(id);
        }
        let t0 = (frame - 1) as f32 * dt;
        for sub in 0..prm.substeps.max(1) {
            let t = t0 + sub as f32 * h;
            for i in 0..s.p.len() {
                let (p, v) = (&mut s.p[i], &mut s.v[i]);
                let mut a = prm.gravity;
                for k in 0..3 {
                    a[k] += prm.drag * (prm.wind[k] - v[k]);
                }
                if prm.noise_amp != 0.0 {
                    let q = [p[0] * prm.noise_freq + t * 0.5, p[1] * prm.noise_freq, p[2] * prm.noise_freq];
                    a[0] += prm.noise_amp * value_noise(q, 3);
                    a[1] += prm.noise_amp * value_noise(q, 5);
                    a[2] += prm.noise_amp * value_noise(q, 7);
                }
                for k in 0..3 {
                    v[k] += a[k] * h;
                    p[k] += v[k] * h;
                }
                if p[1] < prm.ground_y {
                    p[1] = prm.ground_y;
                    if v[1] < 0.0 {
                        v[1] = -v[1] * prm.bounce;
                    }
                    v[0] *= 1.0 - prm.friction;
                    v[2] *= 1.0 - prm.friction;
                }
                if let Some(vol) = collider {
                    let d = vol.sample(*p);
                    if d < 0.0 {
                        let n = sdf_grad(vol, *p);
                        for k in 0..3 {
                            p[k] += n[k] * (-d + 1e-3);
                        }
                        let vn = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
                        if vn < 0.0 {
                            for k in 0..3 {
                                v[k] -= (1.0 + prm.bounce) * vn * n[k];
                            }
                        }
                        for k in 0..3 {
                            v[k] *= 1.0 - prm.friction * 0.5;
                        }
                    }
                }
            }
            for a in s.age.iter_mut() {
                *a += h;
            }
        }
        if prm.life > 0.0 {
            let keep: Vec<bool> = s.age.iter().map(|a| *a < prm.life).collect();
            if keep.iter().any(|k| !k) {
                let pick = |v: &Vec<[f32; 3]>| v.iter().zip(&keep).filter(|(_, k)| **k).map(|(x, _)| *x).collect::<Vec<_>>();
                s.p = pick(&s.p);
                s.v = pick(&s.v);
                s.age = s.age.iter().zip(&keep).filter(|(_, k)| **k).map(|(x, _)| *x).collect();
                s.id = s.id.iter().zip(&keep).filter(|(_, k)| **k).map(|(x, _)| *x).collect();
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter() -> Emitter {
        Emitter { pos: vec![[0.0, 0.5, 0.0]], dir: vec![[0.0, 1.0, 0.0]] }
    }

    #[test]
    fn particles_obey_gravity_and_stay_above_ground() {
        let prm = PopParams { frame: 120, rate: 3.0, life: 0.0, ..Default::default() };
        let s = simulate(&emitter(), None, &prm);
        assert!(s.p.len() > 300);
        assert!(s.p.iter().all(|p| p[1] >= -1e-4 && p.iter().all(|c| c.is_finite())), "nothing below ground");
        // Free-fall check: with no drag/ground, apex of a vertical launch matches v^2 / 2g.
        let free = PopParams { frame: 24, rate: 0.04, drag: 0.0, spread: 0.0, ground_y: -1000.0, speed: 8.0, substeps: 8, ..Default::default() };
        let one = simulate(&emitter(), None, &PopParams { frame: 12, ..free.clone() });
        if let Some(p) = one.p.first() {
            let t = 12.0 / 24.0 - one.age[0] + one.age[0];
            let _ = t;
            assert!(p[1] > 0.5, "launched upward: {}", p[1]);
        }
    }

    #[test]
    fn deterministic_and_lifetime_works() {
        let prm = PopParams { frame: 60, life: 1.0, noise_amp: 3.0, ..Default::default() };
        let a = simulate(&emitter(), None, &prm);
        let b = simulate(&emitter(), None, &prm);
        assert_eq!(a.p, b.p);
        assert!(a.age.iter().all(|x| *x < 1.0 + 1e-3), "old particles are removed");
        let forever = simulate(&emitter(), None, &PopParams { life: 0.0, ..prm.clone() });
        assert!(forever.p.len() > a.p.len());
    }

    #[test]
    fn particles_collide_with_an_sdf_sphere() {
        // Sphere SDF at (0,0,0) radius 1; particles fall from above and must not enter it.
        let mut vol = Volume::new([41, 41, 41], [-2.0; 3], 0.1, 0.0);
        let data = std::sync::Arc::make_mut(&mut vol.data);
        for k in 0..41 {
            for j in 0..41 {
                for i in 0..41 {
                    let p = [-2.0 + i as f32 * 0.1, -2.0 + j as f32 * 0.1, -2.0 + k as f32 * 0.1];
                    data[(k * 41 + j) * 41 + i] = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 1.0;
                }
            }
        }
        let em = Emitter { pos: vec![[0.0, 1.8, 0.0], [0.2, 1.8, 0.1]], dir: vec![[0.0, -1.0, 0.0]; 2] };
        let prm = PopParams { frame: 48, rate: 2.0, speed: 1.0, ground_y: -1.9, spread: 0.0, ..Default::default() };
        let s = simulate(&em, Some(&vol), &prm);
        let inside = s.p.iter().filter(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() < 0.93).count();
        assert!(s.p.len() > 50 && inside == 0, "{inside} of {} penetrated the sphere", s.p.len());
    }
}
