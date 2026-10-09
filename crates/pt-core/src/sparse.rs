//! Sparse (tiled) scalar field for signed distance data: only tiles near a surface are stored densely.
//!
//! * The index space is global and signed: voxel (i, j, k) sits at `origin + (i, j, k) * voxel`.
//! * Voxels are grouped in 8x8x8 tiles. A tile is either `Dense` (512 values) or `Const` (one value,
//!   used for deep-interior blocks); tiles that are absent hold the `background` value (far outside).
//! * Own design: a plain hash map of tiles, no hierarchical tree (no third-party grid code or format).
//! * Deterministic: all iteration goes through `keys()`, which is sorted, never raw hash order.

use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;

pub const TILE: i32 = 8;
pub const TILE_VOXELS: usize = 512;

type Hasher = BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    Const(f32),
    Dense(Arc<Vec<f32>>),
}

#[derive(Clone, Debug)]
pub struct SparseVolume {
    pub voxel: f32,
    pub origin: [f32; 3],
    /// Value of every voxel in an absent tile (positive = outside for signed distance fields).
    pub background: f32,
    tiles: HashMap<[i32; 3], Tile, Hasher>,
}

#[inline]
fn split(i: i32) -> (i32, usize) {
    (i.div_euclid(TILE), i.rem_euclid(TILE) as usize)
}

#[inline]
fn local(li: usize, lj: usize, lk: usize) -> usize {
    (lk * TILE as usize + lj) * TILE as usize + li
}

impl SparseVolume {
    pub fn new(voxel: f32, origin: [f32; 3], background: f32) -> Self {
        SparseVolume { voxel, origin, background, tiles: HashMap::default() }
    }

    pub fn n_tiles(&self) -> usize {
        self.tiles.len()
    }
    pub fn n_dense(&self) -> usize {
        self.tiles.values().filter(|t| matches!(t, Tile::Dense(_))).count()
    }
    pub fn n_const(&self) -> usize {
        self.tiles.len() - self.n_dense()
    }
    /// Approximate heap use of the tile data in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.tiles.len() * (std::mem::size_of::<[i32; 3]>() + std::mem::size_of::<Tile>() + 16) + self.n_dense() * TILE_VOXELS * 4
    }

    /// Tile keys in a deterministic (lexicographic) order.
    pub fn keys(&self) -> Vec<[i32; 3]> {
        let mut k: Vec<[i32; 3]> = self.tiles.keys().copied().collect();
        k.sort_unstable();
        k
    }
    pub fn dense_keys(&self) -> Vec<[i32; 3]> {
        let mut k: Vec<[i32; 3]> = self.tiles.iter().filter(|(_, t)| matches!(t, Tile::Dense(_))).map(|(k, _)| *k).collect();
        k.sort_unstable();
        k
    }
    pub fn tile(&self, key: [i32; 3]) -> Option<&Tile> {
        self.tiles.get(&key)
    }
    pub fn insert_tile(&mut self, key: [i32; 3], t: Tile) {
        self.tiles.insert(key, t);
    }
    pub fn remove_tile(&mut self, key: [i32; 3]) {
        self.tiles.remove(&key);
    }

    /// Value at integer voxel coordinates.
    #[inline]
    pub fn get(&self, i: i32, j: i32, k: i32) -> f32 {
        let ((ti, li), (tj, lj), (tk, lk)) = (split(i), split(j), split(k));
        match self.tiles.get(&[ti, tj, tk]) {
            None => self.background,
            Some(Tile::Const(v)) => *v,
            Some(Tile::Dense(d)) => d[local(li, lj, lk)],
        }
    }

    /// Mutable access to a tile's 512 values, creating it filled with `fill` (or expanding a const tile).
    pub fn dense_mut(&mut self, key: [i32; 3], fill: f32) -> &mut Vec<f32> {
        let e = self.tiles.entry(key).or_insert_with(|| Tile::Dense(Arc::new(vec![fill; TILE_VOXELS])));
        if let Tile::Const(v) = e {
            *e = Tile::Dense(Arc::new(vec![*v; TILE_VOXELS]));
        }
        match e {
            Tile::Dense(d) => Arc::make_mut(d),
            Tile::Const(_) => unreachable!(),
        }
    }

    pub fn set(&mut self, i: i32, j: i32, k: i32, v: f32) {
        let ((ti, li), (tj, lj), (tk, lk)) = (split(i), split(j), split(k));
        let bg = self.background;
        self.dense_mut([ti, tj, tk], bg)[local(li, lj, lk)] = v;
    }

    pub fn pos(&self, i: i32, j: i32, k: i32) -> [f32; 3] {
        [self.origin[0] + i as f32 * self.voxel, self.origin[1] + j as f32 * self.voxel, self.origin[2] + k as f32 * self.voxel]
    }

    /// Trilinear sample at a world position.
    pub fn sample(&self, p: [f32; 3]) -> f32 {
        let g = [(p[0] - self.origin[0]) / self.voxel, (p[1] - self.origin[1]) / self.voxel, (p[2] - self.origin[2]) / self.voxel];
        let b = [g[0].floor(), g[1].floor(), g[2].floor()];
        let f = [g[0] - b[0], g[1] - b[1], g[2] - b[2]];
        let (i, j, k) = (b[0] as i32, b[1] as i32, b[2] as i32);
        let at = |di: i32, dj: i32, dk: i32| self.get(i + di, j + dj, k + dk);
        let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
        l(
            l(l(at(0, 0, 0), at(1, 0, 0), f[0]), l(at(0, 1, 0), at(1, 1, 0), f[0]), f[1]),
            l(l(at(0, 0, 1), at(1, 0, 1), f[0]), l(at(0, 1, 1), at(1, 1, 1), f[0]), f[1]),
            f[2],
        )
    }

    /// Collapse uniform dense tiles to `Const` and drop const tiles equal to the background.
    pub fn compact(&mut self) {
        let bg = self.background;
        for key in self.keys() {
            let new = match self.tiles.get(&key) {
                Some(Tile::Dense(d)) => {
                    let first = d[0];
                    if d.iter().all(|v| *v == first) { Some(Tile::Const(first)) } else { None }
                }
                _ => None,
            };
            if let Some(t) = new {
                self.tiles.insert(key, t);
            }
            if matches!(self.tiles.get(&key), Some(Tile::Const(v)) if *v == bg) {
                self.tiles.remove(&key);
            }
        }
    }

    /// Voxel index box (inclusive) covered by the stored tiles, or None when empty.
    pub fn index_bounds(&self) -> Option<([i32; 3], [i32; 3])> {
        let keys = self.keys();
        let first = *keys.first()?;
        let (mut lo, mut hi) = (first, first);
        for k in &keys {
            for a in 0..3 {
                lo[a] = lo[a].min(k[a]);
                hi[a] = hi[a].max(k[a]);
            }
        }
        Some(([lo[0] * TILE, lo[1] * TILE, lo[2] * TILE], [hi[0] * TILE + TILE - 1, hi[1] * TILE + TILE - 1, hi[2] * TILE + TILE - 1]))
    }

    /// Order-independent content fingerprint (for tests and cache keys): hashes tiles in key order.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.voxel.to_le_bytes());
        h.update(&self.background.to_le_bytes());
        for k in self.keys() {
            for c in k {
                h.update(&c.to_le_bytes());
            }
            match &self.tiles[&k] {
                Tile::Const(v) => {
                    h.update(&[0]);
                    h.update(&v.to_le_bytes());
                }
                Tile::Dense(d) => {
                    h.update(&[1]);
                    for v in d.iter() {
                        h.update(&v.to_le_bytes());
                    }
                }
            }
        }
        *h.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_across_tiles_and_negative_indices() {
        let mut v = SparseVolume::new(0.5, [0.0; 3], 9.0);
        assert_eq!(v.get(-100, 5, 1000), 9.0);
        v.set(-1, -1, -1, 1.5);
        v.set(7, 8, 9, 2.5);
        v.set(-9, 0, 0, 3.5);
        assert_eq!((v.get(-1, -1, -1), v.get(7, 8, 9), v.get(-9, 0, 0)), (1.5, 2.5, 3.5));
        assert_eq!(v.get(0, 0, 0), 9.0, "untouched voxels in a created tile keep the background fill");
        assert_eq!(v.n_dense(), 3);
        assert_eq!(v.keys(), vec![[-2, 0, 0], [-1, -1, -1], [0, 1, 1]]);
    }

    #[test]
    fn compact_collapses_uniform_tiles_and_drops_background() {
        let mut v = SparseVolume::new(1.0, [0.0; 3], 5.0);
        v.set(0, 0, 0, 5.0); // dense tile that is entirely background
        v.insert_tile([3, 3, 3], Tile::Dense(Arc::new(vec![-2.0; TILE_VOXELS])));
        v.set(20, 0, 0, 1.0);
        v.compact();
        assert_eq!(v.tile([0, 0, 0]), None);
        assert_eq!(v.tile([3, 3, 3]), Some(&Tile::Const(-2.0)));
        assert!(matches!(v.tile([2, 0, 0]), Some(Tile::Dense(_))));
        assert_eq!(v.get(24, 24, 24), -2.0);
    }

    #[test]
    fn sample_is_trilinear_and_fingerprint_ignores_insertion_order() {
        let mut a = SparseVolume::new(1.0, [0.0; 3], 0.0);
        let mut b = SparseVolume::new(1.0, [0.0; 3], 0.0);
        for (i, j, k) in [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)] {
            a.set(i, j, k, (i + 2 * j + 4 * k) as f32);
        }
        for (i, j, k) in [(1, 1, 1), (0, 1, 1), (1, 0, 1), (0, 0, 1), (1, 1, 0), (0, 1, 0), (1, 0, 0), (0, 0, 0)] {
            b.set(i, j, k, (i + 2 * j + 4 * k) as f32);
        }
        assert!((a.sample([0.5, 0.5, 0.5]) - 3.5).abs() < 1e-6);
        assert!((a.sample([0.25, 0.0, 0.0]) - 0.25).abs() < 1e-6);
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
}
