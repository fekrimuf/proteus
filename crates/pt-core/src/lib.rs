//! Core data model: geometry v2, deterministic math/RNG, parameters and node schemas.

pub mod column;
pub mod geo;
pub mod gltf;
pub mod io;
pub mod lod;
pub mod math;
pub mod noise;
pub mod rng;
pub mod seat;
pub mod sparse;
pub mod spatial;
pub mod validate;

pub use column::{AttrSet, Column};
pub use geo::{Geometry, Instances, Topology, Volume};
pub use sparse::{SparseVolume, Tile};

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Node parameters are plain JSON values so agents can read and write them directly.
pub type Params = BTreeMap<String, Value>;

pub trait ParamExt {
    fn f(&self, k: &str) -> f32;
    fn i(&self, k: &str) -> i64;
    fn v3(&self, k: &str) -> [f32; 3];
    fn s(&self, k: &str) -> String;
}

impl ParamExt for Params {
    fn f(&self, k: &str) -> f32 {
        self.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32
    }
    fn i(&self, k: &str) -> i64 {
        self.get(k)
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
            .unwrap_or(0)
    }
    fn v3(&self, k: &str) -> [f32; 3] {
        let mut out = [0.0; 3];
        if let Some(Value::Array(a)) = self.get(k) {
            for (o, v) in out.iter_mut().zip(a) {
                *o = v.as_f64().unwrap_or(0.0) as f32;
            }
        } else if let Some(x) = self.get(k).and_then(Value::as_f64) {
            out = [x as f32; 3];
        }
        out
    }
    fn s(&self, k: &str) -> String {
        self.get(k).and_then(Value::as_str).unwrap_or("").to_string()
    }
}

/// Can gradients flow through this node (PLAN Bet 1/5)?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Differentiability {
    Smooth,
    Piecewise,
    None,
}

/// At which granularity does this op depend on its inputs (incremental cooking)?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    Whole,
    PerPoint,
    PerPrim,
}

#[derive(Clone, Debug)]
pub struct ParamSchema {
    pub name: &'static str,
    pub default: Value,
    pub doc: &'static str,
}

#[derive(Clone, Debug)]
pub struct NodeSchema {
    pub kind: &'static str,
    pub doc: &'static str,
    pub version: u32,
    pub inputs: usize,
    pub diff: Differentiability,
    pub granularity: Granularity,
    pub params: Vec<ParamSchema>,
}

impl NodeSchema {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.kind,
            "doc": self.doc,
            "version": self.version,
            "inputs": self.inputs,
            "differentiability": format!("{:?}", self.diff),
            "granularity": format!("{:?}", self.granularity),
            "params": self.params.iter().map(|p| json!({
                "name": p.name, "default": p.default, "doc": p.doc
            })).collect::<Vec<_>>(),
        })
    }
}

/// Canonical JSON for hashing: all numbers as f64, object keys sorted (BTreeMap order).
pub fn canonical(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap_or(0.0)),
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), canonical(v))).collect()),
        other => other.clone(),
    }
}
