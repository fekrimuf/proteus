//! Counter-based RNG (Philox4x32-10). Each random value is a pure function of
//! (seed, stream, element index), so parallel evaluation order never changes results.

pub fn philox4x32(counter: [u32; 4], key: [u32; 2]) -> [u32; 4] {
    let (mut c, mut k) = (counter, key);
    for _ in 0..10 {
        let p0 = 0xD251_1F53u64 * c[0] as u64;
        let p1 = 0xCD9E_8D57u64 * c[2] as u64;
        c = [
            (p1 >> 32) as u32 ^ c[1] ^ k[0],
            p1 as u32,
            (p0 >> 32) as u32 ^ c[3] ^ k[1],
            p0 as u32,
        ];
        k = [k[0].wrapping_add(0x9E37_79B9), k[1].wrapping_add(0xBB67_AE85)];
    }
    c
}

/// Four uniform floats in [0, 1) for element `index` of `stream` under `seed`.
pub fn rand4(seed: i64, stream: u32, index: u64) -> [f32; 4] {
    let s = seed as u64;
    let r = philox4x32([index as u32, (index >> 32) as u32, stream, 0], [s as u32, (s >> 32) as u32]);
    r.map(|x| (x >> 8) as f32 / 16_777_216.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_answer_zero() {
        // Random123 known-answer test: philox4x32-10, counter 0, key 0.
        assert_eq!(philox4x32([0; 4], [0; 2]), [0x6627e8d5, 0xe169c58d, 0xbc57ac4c, 0x9b00dbd8]);
    }
    #[test]
    fn range() {
        for i in 0..1000 {
            for v in rand4(7, 1, i) {
                assert!((0.0..1.0).contains(&v));
            }
        }
    }
}
