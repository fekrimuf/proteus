//! Deterministic math (PLAN D2). Uses only +, -, *, /, round and sqrt, which IEEE-754 specifies
//! bit-exactly, so results are identical on every platform. Do not call std transcendentals.

pub const PI: f32 = std::f32::consts::PI;
const TAU64: f64 = std::f64::consts::TAU;

/// Returns (sin x, cos x). Range-reduces then evaluates a Taylor series in f64.
pub fn sin_cos(x: f32) -> (f32, f32) {
    let x = x as f64;
    let r = x - (x / TAU64).round() * TAU64;
    let r2 = r * r;
    let (mut ts, mut s) = (r, r);
    let (mut tc, mut c) = (1.0f64, 1.0f64);
    for i in 1..14 {
        let (a, b) = ((2 * i) as f64, (2 * i + 1) as f64);
        ts *= -r2 / (a * b);
        s += ts;
        tc *= -r2 / ((a - 1.0) * a);
        c += tc;
    }
    (s as f32, c as f32)
}

pub fn to_radians(deg: f32) -> f32 {
    deg * (PI / 180.0)
}

pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        v
    }
}

pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// 3x4 row-major affine transform: rows are [r00 r01 r02 tx, r10 r11 r12 ty, r20 r21 r22 tz].
pub type Mat34 = [f32; 12];

pub const IDENTITY: Mat34 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

pub fn xform_point(m: &Mat34, p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3],
        m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7],
        m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11],
    ]
}

/// `a` applied after `b` (a * b).
pub fn mul34(a: &Mat34, b: &Mat34) -> Mat34 {
    let mut o = [0.0; 12];
    for r in 0..3 {
        for c in 0..4 {
            let mut v = a[r * 4] * b[c] + a[r * 4 + 1] * b[4 + c] + a[r * 4 + 2] * b[8 + c];
            if c == 3 {
                v += a[r * 4 + 3];
            }
            o[r * 4 + c] = v;
        }
    }
    o
}

/// M = T * Rz * Ry * Rx * S, rotations in degrees.
pub fn trs(t: [f32; 3], r_deg: [f32; 3], s: [f32; 3]) -> Mat34 {
    let (sx, cx) = sin_cos(to_radians(r_deg[0]));
    let (sy, cy) = sin_cos(to_radians(r_deg[1]));
    let (sz, cz) = sin_cos(to_radians(r_deg[2]));
    let rx: Mat34 = [1.0, 0.0, 0.0, 0.0, 0.0, cx, -sx, 0.0, 0.0, sx, cx, 0.0];
    let ry: Mat34 = [cy, 0.0, sy, 0.0, 0.0, 1.0, 0.0, 0.0, -sy, 0.0, cy, 0.0];
    let rz: Mat34 = [cz, -sz, 0.0, 0.0, sz, cz, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let mut m = mul34(&rz, &mul34(&ry, &rx));
    for row in 0..3 {
        for col in 0..3 {
            m[row * 4 + col] *= s[col];
        }
    }
    m[3] = t[0];
    m[7] = t[1];
    m[11] = t[2];
    m
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // std is the oracle here
    use super::*;
    #[test]
    fn sin_cos_accuracy() {
        for k in -2000..2000 {
            let x = k as f32 * 0.01;
            let (s, c) = sin_cos(x);
            assert!((s as f64 - (x as f64).sin()).abs() < 2e-6, "sin {x}");
            assert!((c as f64 - (x as f64).cos()).abs() < 2e-6, "cos {x}");
        }
    }
}

const LN2: f64 = std::f64::consts::LN_2;

/// Deterministic natural log for x > 0 (returns NaN otherwise).
pub fn ln(x: f32) -> f32 {
    if x.is_nan() || x <= 0.0 {
        return f32::NAN;
    }
    let bits = (x as f64).to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let (mut term, mut sum) = (s, s);
    for k in 1..14 {
        term *= s2;
        sum += term / (2 * k + 1) as f64;
    }
    (2.0 * sum + e as f64 * LN2) as f32
}

/// Deterministic exp; saturates outside the f32 range.
pub fn exp(x: f32) -> f32 {
    let x = (x as f64).clamp(-87.0, 88.0);
    let k = (x / LN2).round();
    let r = x - k * LN2;
    let (mut term, mut sum) = (1.0f64, 1.0f64);
    for i in 1..16 {
        term *= r / i as f64;
        sum += term;
    }
    (sum * f64::from_bits(((k as i64 + 1023) as u64) << 52)) as f32
}

/// x^y for x >= 0 (0^y = 0 for y > 0, 1 for y = 0).
pub fn pow(x: f32, y: f32) -> f32 {
    if y == 0.0 {
        1.0
    } else if x == 0.0 {
        if y > 0.0 { 0.0 } else { f32::INFINITY }
    } else if x < 0.0 {
        f32::NAN
    } else {
        exp(y * ln(x))
    }
}

#[cfg(test)]
mod tests_exp {
    #![allow(clippy::disallowed_methods)]
    use super::*;
    #[test]
    fn exp_ln_pow_accuracy() {
        for k in 1..400 {
            let x = k as f32 * 0.05;
            assert!((ln(x) as f64 - (x as f64).ln()).abs() < 5e-6, "ln {x}");
        }
        for k in -300..300 {
            let x = k as f32 * 0.05;
            let rel = ((exp(x) as f64) / (x as f64).exp() - 1.0).abs();
            assert!(rel < 5e-6, "exp {x} rel {rel}");
        }
        assert!((pow(2.0, 10.0) - 1024.0).abs() < 0.01);
        assert!((pow(9.0, 0.5) - 3.0).abs() < 1e-4);
    }
}

/// f64 versions (deterministic, basic ops only) used by the build-time `hcs` language.
pub fn sin_cos64(x: f64) -> (f64, f64) {
    let r = x - (x / TAU64).round() * TAU64;
    let r2 = r * r;
    let (mut ts, mut s) = (r, r);
    let (mut tc, mut c) = (1.0f64, 1.0f64);
    for i in 1..14 {
        let (a, b) = ((2 * i) as f64, (2 * i + 1) as f64);
        ts *= -r2 / (a * b);
        s += ts;
        tc *= -r2 / ((a - 1.0) * a);
        c += tc;
    }
    (s, c)
}

/// x^y for x >= 0 in f64: exact repeated multiplication for small integer y, else exp(y ln x).
pub fn pow64(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    if y.fract() == 0.0 && y.abs() <= 64.0 {
        let mut r = 1.0f64;
        for _ in 0..(y.abs() as u32) {
            r *= x;
        }
        return if y < 0.0 { 1.0 / r } else { r };
    }
    if x <= 0.0 {
        return if x == 0.0 && y > 0.0 { 0.0 } else { f64::NAN };
    }
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let (mut term, mut sum) = (s, s);
    for k in 1..20 {
        term *= s2;
        sum += term / (2 * k + 1) as f64;
    }
    let lnx = 2.0 * sum + e as f64 * LN2;
    let t = (y * lnx).clamp(-700.0, 700.0);
    let k = (t / LN2).round();
    let r = t - k * LN2;
    let (mut term, mut sum) = (1.0f64, 1.0f64);
    for i in 1..20 {
        term *= r / i as f64;
        sum += term;
    }
    sum * f64::from_bits(((k as i64 + 1023) as u64) << 52)
}

/// Deterministic arctangent (radians), basic ops + sqrt only.
pub fn atan64(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let (neg, mut x) = (x < 0.0, x.abs());
    let mut base = 0.0;
    let mut mul = 1.0;
    if x > 1.0 {
        x = 1.0 / x;
        base = std::f64::consts::FRAC_PI_2;
        mul = -1.0;
    }
    // Halve the angle twice: atan(x) = 2 atan(x / (1 + sqrt(1 + x^2))).
    for _ in 0..3 {
        x /= 1.0 + (1.0 + x * x).sqrt();
    }
    let x2 = x * x;
    let (mut term, mut sum) = (x, x);
    for k in 1..12 {
        term *= -x2;
        sum += term / (2 * k + 1) as f64;
    }
    let a = 8.0 * sum;
    let r = base + mul * a;
    if neg { -r } else { r }
}

/// Deterministic atan2 in f32 (computed in f64 from basic ops, so identical on every platform).
pub fn atan2(y: f32, x: f32) -> f32 {
    let (y, x) = (y as f64, x as f64);
    let pi = std::f64::consts::PI;
    let r = if x > 0.0 {
        atan64(y / x)
    } else if x < 0.0 {
        if y >= 0.0 { atan64(y / x) + pi } else { atan64(y / x) - pi }
    } else if y > 0.0 {
        pi / 2.0
    } else if y < 0.0 {
        -pi / 2.0
    } else {
        0.0
    };
    r as f32
}

/// Deterministic asin / acos / atan for f32 (inputs to asin/acos are clamped to [-1, 1]).
pub fn asin(x: f32) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    atan2(x, (1.0 - x * x).max(0.0).sqrt())
}
pub fn acos(x: f32) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    atan2((1.0 - x * x).max(0.0).sqrt(), x)
}
pub fn atan(x: f32) -> f32 {
    atan64(x as f64) as f32
}

#[cfg(test)]
mod tests_atan {
    #![allow(clippy::disallowed_methods)]
    use super::*;
    #[test]
    fn atan_accuracy() {
        for k in -400..400 {
            let x = k as f64 * 0.05;
            assert!((atan64(x) - x.atan()).abs() < 1e-9, "atan {x}");
        }
    }
}

#[cfg(test)]
mod tests_atan2 {
    #![allow(clippy::disallowed_methods)]
    use super::*;
    #[test]
    fn atan2_asin_acos_match_std() {
        for yi in -20..=20 {
            for xi in -20..=20 {
                let (y, x) = (yi as f32 * 0.37, xi as f32 * 0.29);
                assert!((atan2(y, x) - y.atan2(x)).abs() < 2e-6, "atan2({y},{x})");
            }
        }
        for k in -100..=100 {
            let x = k as f32 * 0.01;
            assert!((asin(x) - x.asin()).abs() < 3e-6, "asin {x}");
            assert!((acos(x) - x.acos()).abs() < 3e-6, "acos {x}");
        }
    }
}
