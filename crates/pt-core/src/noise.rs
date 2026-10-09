//! Deterministic value noise (integer hash + f32 arithmetic only).

fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6b343)
        ^ (y as u32).wrapping_mul(0xd8163841)
        ^ (z as u32).wrapping_mul(0xcb1ab31f)
        ^ seed.wrapping_mul(0x9E3779B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0
}

/// Smooth value noise in [-1, 1].
pub fn value_noise(p: [f32; 3], seed: u32) -> f32 {
    let f = p.map(f32::floor);
    let i = [f[0] as i32, f[1] as i32, f[2] as i32];
    let t = [p[0] - f[0], p[1] - f[1], p[2] - f[2]];
    let s = t.map(|x| x * x * (3.0 - 2.0 * x));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash3(i[0] + dx, i[1] + dy, i[2] + dz, seed);
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), s[0]);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), s[0]);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), s[0]);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), s[0]);
    lerp(lerp(x00, x10, s[1]), lerp(x01, x11, s[1]), s[2])
}

/// Fractal sum of value noise, normalized to [-1, 1].
pub fn fbm(p: [f32; 3], freq: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut a, mut fr, mut norm) = (0.0f32, 1.0f32, freq, 0.0f32);
    for o in 0..octaves {
        sum += a * value_noise([p[0] * fr, p[1] * fr, p[2] * fr], seed.wrapping_add(o));
        norm += a;
        a *= 0.5;
        fr *= 2.0;
    }
    sum / norm
}
