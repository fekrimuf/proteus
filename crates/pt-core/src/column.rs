//! Typed attribute columns. Each column is an `Arc`'d vector, so cloning a geometry or an
//! attribute set shares memory; mutation goes through `Arc::make_mut` (copy-on-write).

use rayon::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Column {
    F32(Arc<Vec<f32>>),
    I32(Arc<Vec<i32>>),
    V2(Arc<Vec<[f32; 2]>>),
    V3(Arc<Vec<[f32; 3]>>),
    V4(Arc<Vec<[f32; 4]>>),
}

/// Decode little-endian bytes into a vector of plain-old-data elements.
pub fn collect<T: bytemuck::Pod>(b: &[u8]) -> Result<Vec<T>, String> {
    if !b.len().is_multiple_of(std::mem::size_of::<T>()) {
        return Err("corrupt column bytes".into());
    }
    Ok(bytemuck::pod_collect_to_vec::<u8, T>(b))
}

macro_rules! each {
    ($s:expr, $v:ident => $e:expr) => {
        match $s {
            Column::F32($v) => $e,
            Column::I32($v) => $e,
            Column::V2($v) => $e,
            Column::V3($v) => $e,
            Column::V4($v) => $e,
        }
    };
}

impl Column {
    pub fn len(&self) -> usize {
        each!(self, v => v.len())
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn tag(&self) -> &'static str {
        match self {
            Column::F32(_) => "f32",
            Column::I32(_) => "i32",
            Column::V2(_) => "v2",
            Column::V3(_) => "v3",
            Column::V4(_) => "v4",
        }
    }
    pub fn from_f32(v: Vec<f32>) -> Self {
        Column::F32(Arc::new(v))
    }
    pub fn from_v3(v: Vec<[f32; 3]>) -> Self {
        Column::V3(Arc::new(v))
    }
    pub fn from_i32(v: Vec<i32>) -> Self {
        Column::I32(Arc::new(v))
    }
    pub fn zeros_like(&self, n: usize) -> Column {
        match self {
            Column::F32(_) => Column::F32(Arc::new(vec![0.0; n])),
            Column::I32(_) => Column::I32(Arc::new(vec![0; n])),
            Column::V2(_) => Column::V2(Arc::new(vec![[0.0; 2]; n])),
            Column::V3(_) => Column::V3(Arc::new(vec![[0.0; 3]; n])),
            Column::V4(_) => Column::V4(Arc::new(vec![[0.0; 4]; n])),
        }
    }
    pub fn extend_zeros(&mut self, n: usize) {
        let m = self.len() + n;
        match self {
            Column::F32(v) => Arc::make_mut(v).resize(m, 0.0),
            Column::I32(v) => Arc::make_mut(v).resize(m, 0),
            Column::V2(v) => Arc::make_mut(v).resize(m, [0.0; 2]),
            Column::V3(v) => Arc::make_mut(v).resize(m, [0.0; 3]),
            Column::V4(v) => Arc::make_mut(v).resize(m, [0.0; 4]),
        }
    }
    pub fn extend(&mut self, o: &Column) -> Result<(), String> {
        match (self, o) {
            (Column::F32(a), Column::F32(b)) => Arc::make_mut(a).extend_from_slice(b),
            (Column::I32(a), Column::I32(b)) => Arc::make_mut(a).extend_from_slice(b),
            (Column::V2(a), Column::V2(b)) => Arc::make_mut(a).extend_from_slice(b),
            (Column::V3(a), Column::V3(b)) => Arc::make_mut(a).extend_from_slice(b),
            (Column::V4(a), Column::V4(b)) => Arc::make_mut(a).extend_from_slice(b),
            (a, b) => return Err(format!("attribute type mismatch: {} vs {}", a.tag(), b.tag())),
        }
        Ok(())
    }
    pub fn as_bytes(&self) -> &[u8] {
        each!(self, v => bytemuck::cast_slice(v.as_slice()))
    }
    pub fn from_bytes(tag: &str, b: &[u8]) -> Result<Column, String> {
        Ok(match tag {
            "f32" => Column::F32(Arc::new(collect(b)?)),
            "i32" => Column::I32(Arc::new(collect(b)?)),
            "v2" => Column::V2(Arc::new(collect(b)?)),
            "v3" => Column::V3(Arc::new(collect(b)?)),
            "v4" => Column::V4(Arc::new(collect(b)?)),
            t => return Err(format!("unknown column type '{t}'")),
        })
    }
    pub fn as_f32(&self) -> Option<&[f32]> {
        if let Column::F32(v) = self { Some(v) } else { None }
    }
    pub fn as_v3(&self) -> Option<&[[f32; 3]]> {
        if let Column::V3(v) = self { Some(v) } else { None }
    }
    /// Merkle-style hash: fixed-size chunks hashed in parallel, then combined in order.
    pub fn hash_into(&self, h: &mut blake3::Hasher) {
        const CHUNK: usize = 1 << 16;
        let chunks: Vec<[u8; 32]> = self.as_bytes().par_chunks(CHUNK).map(|c| *blake3::hash(c).as_bytes()).collect();
        h.update(self.tag().as_bytes());
        h.update(&(self.len() as u64).to_le_bytes());
        for c in chunks {
            h.update(&c);
        }
    }
    /// (min, max, mean) per component, for agent-facing summaries.
    pub fn summary(&self) -> serde_json::Value {
        fn agg(comps: usize, n: usize, get: &dyn Fn(usize, usize) -> f64) -> serde_json::Value {
            let mut mn = vec![f64::INFINITY; comps];
            let mut mx = vec![f64::NEG_INFINITY; comps];
            let mut sum = vec![0.0f64; comps];
            for i in 0..n {
                for c in 0..comps {
                    let x = get(i, c);
                    mn[c] = mn[c].min(x);
                    mx[c] = mx[c].max(x);
                    sum[c] += x;
                }
            }
            if n == 0 {
                return serde_json::json!({"min": null, "max": null, "mean": null});
            }
            let mean: Vec<f64> = sum.iter().map(|s| s / n as f64).collect();
            serde_json::json!({"min": mn, "max": mx, "mean": mean})
        }
        let n = self.len();
        let mut j = match self {
            Column::F32(v) => agg(1, n, &|i, _| v[i] as f64),
            Column::I32(v) => agg(1, n, &|i, _| v[i] as f64),
            Column::V2(v) => agg(2, n, &|i, c| v[i][c] as f64),
            Column::V3(v) => agg(3, n, &|i, c| v[i][c] as f64),
            Column::V4(v) => agg(4, n, &|i, c| v[i][c] as f64),
        };
        j["type"] = self.tag().into();
        j
    }
}

/// Named columns that all share one element count (a domain: point, vertex, primitive...).
#[derive(Clone, Debug, Default)]
pub struct AttrSet(pub BTreeMap<String, Column>);

impl AttrSet {
    pub fn get(&self, k: &str) -> Option<&Column> {
        self.0.get(k)
    }
    pub fn set(&mut self, k: &str, c: Column) {
        self.0.insert(k.to_string(), c);
    }
    pub fn remove(&mut self, k: &str) -> Option<Column> {
        self.0.remove(k)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Column)> {
        self.0.iter()
    }
    pub fn names(&self) -> Vec<&String> {
        self.0.keys().collect()
    }
    /// Append `other`'s rows; columns missing on either side are zero-filled.
    pub fn append(&mut self, n_self: usize, other: &AttrSet, n_other: usize) -> Result<(), String> {
        for (name, col) in self.0.iter_mut() {
            match other.0.get(name) {
                Some(o) => col.extend(o).map_err(|e| format!("attribute '{name}': {e}"))?,
                None => col.extend_zeros(n_other),
            }
        }
        for (name, o) in &other.0 {
            if !self.0.contains_key(name) {
                let mut c = o.zeros_like(n_self);
                c.extend(o)?;
                self.0.insert(name.clone(), c);
            }
        }
        Ok(())
    }
}

impl Column {
    /// New column with `idx.len()` rows taken from this one.
    pub fn gather(&self, idx: &[u32]) -> Column {
        match self {
            Column::F32(v) => Column::F32(Arc::new(idx.iter().map(|&i| v[i as usize]).collect())),
            Column::I32(v) => Column::I32(Arc::new(idx.iter().map(|&i| v[i as usize]).collect())),
            Column::V2(v) => Column::V2(Arc::new(idx.iter().map(|&i| v[i as usize]).collect())),
            Column::V3(v) => Column::V3(Arc::new(idx.iter().map(|&i| v[i as usize]).collect())),
            Column::V4(v) => Column::V4(Arc::new(idx.iter().map(|&i| v[i as usize]).collect())),
        }
    }
}

impl AttrSet {
    pub fn gather(&self, idx: &[u32]) -> AttrSet {
        AttrSet(self.0.iter().map(|(k, c)| (k.clone(), c.gather(idx))).collect())
    }
}
