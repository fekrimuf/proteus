//! The procedural node graph: a DAG of SOP nodes with a content-addressed, deterministic
//! cook cache (PLAN Bet 3). Cooks are pure functions keyed by
//! `H(op version, canonical resolved params, input content hashes, backend class)`, so the
//! cache is incremental by construction and has early cutoff: a recomputed node whose output
//! hash is unchanged leaves every downstream key (and cache entry) valid.
//! Chains of pointwise nodes (transform, noise) are fused into one pass over memory.
//!
//! Also here: parameter expressions (`"=2*$F"`, `ch("node/param")`), reusable assets
//! (HDA-style subgraphs with promoted parameters), the `repeat` loop node, and the static
//! attribute-flow `check()` that predicts what every node outputs without cooking.

use pt_core::{canonical, Column, Geometry, NodeSchema, ParamExt, ParamSchema, Params};
use pt_core::{Differentiability as D, Granularity as G};
use pt_sop::flow::GeoEnv;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

pub mod merge;

/// Part of every cache key. Bump when numerics change; GPU/wide-SIMD backends get their own.
pub const BACKEND_CLASS: &str = "ref-cpu/1";
const MAX_ASSET_DEPTH: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub kind: String,
    #[serde(default)]
    pub params: Params,
    pub inputs: Vec<Option<String>>,
}

/// A parameter exposed on an asset's interface.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Promoted {
    pub name: String,
    pub node: String,
    pub param: String,
    pub default: Value,
}

/// A reusable subgraph (HDA-style). `input` nodes inside are bound to the caller's inputs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub doc: String,
    pub nodes: BTreeMap<String, Node>,
    pub output: String,
    pub promoted: Vec<Promoted>,
    pub num_inputs: usize,
}

#[derive(Clone, Debug, Default)]
pub struct CookStats {
    pub mem_hits: u64,
    pub disk_hits: u64,
    pub computed: u64,
    pub fused_passes: u64,
    pub nodes_fused_away: u64,
}

impl CookStats {
    pub fn to_json(&self) -> Value {
        json!({"mem_hits": self.mem_hits, "disk_hits": self.disk_hits, "computed": self.computed,
               "fused_passes": self.fused_passes, "nodes_fused_away": self.nodes_fused_away})
    }
}

#[derive(Clone)]
pub struct Entry {
    pub geo: Arc<Geometry>,
    pub hash: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct CheckDiag {
    pub node: String,
    pub severity: &'static str,
    pub message: String,
    pub fix: String,
}

impl CheckDiag {
    pub fn to_json(&self) -> Value {
        json!({"node": self.node, "severity": self.severity, "message": self.message, "fix": self.fix})
    }
}

pub struct CheckReport {
    pub diags: Vec<CheckDiag>,
    pub envs: BTreeMap<String, Option<GeoEnv>>,
}

fn one() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub nodes: BTreeMap<String, Node>,
    pub assets: BTreeMap<String, Asset>,
    /// Playback frame (`$F` / `@Frame`).
    pub frame: f32,
    /// Stamp every element with the node that created it (enables `explain`).
    #[serde(skip)]
    pub provenance: bool,
    /// Fuse chains of pointwise nodes into one pass (on by default; off only for tests).
    #[serde(skip)]
    pub no_fusion: bool,
    #[serde(skip)]
    cache: HashMap<[u8; 32], Entry>,
    #[serde(skip)]
    cache_dir: Option<PathBuf>,
    #[serde(skip)]
    pub stats: CookStats,
    #[serde(skip)]
    sub_inputs: Vec<Option<Entry>>,
    #[serde(skip)]
    asset_depth: usize,
}

impl Default for Graph {
    fn default() -> Self {
        Graph {
            nodes: BTreeMap::new(),
            assets: BTreeMap::new(),
            frame: one(),
            provenance: false,
            no_fusion: false,
            cache: HashMap::new(),
            cache_dir: None,
            stats: CookStats::default(),
            sub_inputs: vec![],
            asset_depth: 0,
        }
    }
}

pub fn hex(h: &[u8; 32]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn tag_of(name: &str) -> i32 {
    let h = blake3::hash(name.as_bytes());
    i32::from_le_bytes(h.as_bytes()[..4].try_into().unwrap())
}

fn type_matches(default: &Value, v: &Value) -> bool {
    match (default, v) {
        (Value::Number(_), Value::Number(_)) => true,
        (Value::Number(_), Value::String(s)) | (Value::Array(_), Value::String(s)) => s.starts_with('='),
        // A default made of arrays is a variable-length list of vectors (e.g. curve points): any length, each item shaped like the first.
        (Value::Array(a), Value::Array(b)) if a.first().is_some_and(Value::is_array) => b.iter().all(|y| type_matches(&a[0], y)),
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| type_matches(x, y)),
        (Value::String(_), Value::String(_)) => true,
        (Value::Bool(_), Value::Bool(_)) => true,
        (Value::Object(_), Value::Object(_)) => true,
        _ => false,
    }
}

fn graph_schemas() -> Vec<NodeSchema> {
    let ps = |name: &'static str, default: Value, doc: &'static str| ParamSchema { name, default, doc };
    let mk = |kind, doc, inputs, params| NodeSchema { kind, doc, version: 1, inputs, diff: D::Piecewise, granularity: G::Whole, params };
    vec![
        mk("input", "Asset boundary: yields the geometry bound to input slot `slot` (only valid inside assets).", 0, vec![ps("slot", json!(0), "Input slot index.")]),
        mk("hda", "Instance of a saved asset. `values` overrides its promoted parameters.", 4, vec![
            ps("asset", json!(""), "Asset name."),
            ps("values", json!({}), "Promoted parameter overrides, e.g. {\"height\": 3.0}."),
        ]),
        mk("repeat", "Apply an asset N times (output of each pass feeds the next); optional `iter_param` receives the pass index. Use for loops/solvers.", 1, vec![
            ps("asset", json!(""), "Asset name (1 input)."),
            ps("iterations", json!(3), "Number of passes (0-64)."),
            ps("values", json!({}), "Promoted parameter overrides."),
            ps("iter_param", json!(""), "Name of a promoted parameter set to the pass index (0-based)."),
        ]),
    ]
}

/// Every node type an agent can create (SOPs plus graph-level nodes).
pub fn all_schemas() -> Vec<NodeSchema> {
    let mut v = pt_sop::schemas();
    v.extend(graph_schemas());
    v
}

pub fn schema(kind: &str) -> Option<NodeSchema> {
    pt_sop::schema(kind).or_else(|| graph_schemas().into_iter().find(|s| s.kind == kind))
}

fn is_asset_kind(kind: &str) -> bool {
    kind == "hda" || kind == "repeat"
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_cache_dir(&mut self, dir: impl Into<PathBuf>) -> Result<(), String> {
        let d = dir.into();
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        self.cache_dir = Some(d);
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Create a node; returns its id (`name` if given and free, else `<kind><n>`).
    pub fn create(&mut self, kind: &str, name: Option<&str>) -> Result<String, String> {
        let schema = schema(kind).ok_or_else(|| {
            let kinds: Vec<_> = all_schemas().iter().map(|s| s.kind).collect();
            format!("unknown node type '{kind}'. Valid: {kinds:?}")
        })?;
        let id = match name {
            Some(n) if self.nodes.contains_key(n) => return Err(format!("node '{n}' already exists")),
            Some(n) if n.is_empty() || n.contains(char::is_whitespace) || n.contains('/') => return Err("node names must be non-empty, no spaces or '/'".into()),
            Some(n) => n.to_string(),
            None => (1..).map(|i| format!("{kind}{i}")).find(|n| !self.nodes.contains_key(n)).unwrap(),
        };
        self.nodes.insert(id.clone(), Node { kind: kind.into(), params: Params::new(), inputs: vec![None; schema.inputs] });
        Ok(id)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        self.nodes.remove(id).ok_or_else(|| format!("no node '{id}'"))?;
        for n in self.nodes.values_mut() {
            for i in &mut n.inputs {
                if i.as_deref() == Some(id) {
                    *i = None;
                }
            }
        }
        Ok(())
    }

    pub fn set_param(&mut self, id: &str, name: &str, value: Value) -> Result<(), String> {
        let node = self.nodes.get_mut(id).ok_or_else(|| format!("no node '{id}'"))?;
        let schema = schema(&node.kind).unwrap();
        let Some(p) = schema.params.iter().find(|p| p.name == name) else {
            let valid: Vec<_> = schema.params.iter().map(|p| p.name).collect();
            return Err(format!("'{}' has no parameter '{name}'. Valid: {valid:?}", node.kind));
        };
        if !type_matches(&p.default, &value) {
            return Err(format!("parameter '{name}' of '{}' expects a value shaped like {} (or an \"=expression\" string for numbers) but got {value}", node.kind, p.default));
        }
        node.params.insert(name.into(), value);
        Ok(())
    }

    /// Wire `from`'s output into `to`'s input slot `input`. Rejects cycles.
    pub fn connect(&mut self, from: &str, to: &str, input: usize) -> Result<(), String> {
        if !self.nodes.contains_key(from) {
            return Err(format!("no node '{from}'"));
        }
        let slots = self.nodes.get(to).ok_or_else(|| format!("no node '{to}'"))?.inputs.len();
        if input >= slots {
            return Err(format!("'{to}' has {slots} input(s); slot {input} is out of range"));
        }
        if from == to || self.upstream(from).contains(to) {
            return Err(format!("connecting '{from}' -> '{to}' would create a cycle"));
        }
        self.nodes.get_mut(to).unwrap().inputs[input] = Some(from.into());
        Ok(())
    }

    pub fn disconnect(&mut self, to: &str, input: usize) -> Result<(), String> {
        let n = self.nodes.get_mut(to).ok_or_else(|| format!("no node '{to}'"))?;
        *n.inputs.get_mut(input).ok_or_else(|| format!("slot {input} out of range"))? = None;
        Ok(())
    }

    fn upstream(&self, id: &str) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack = vec![id.to_string()];
        while let Some(n) = stack.pop() {
            if let Some(node) = self.nodes.get(&n) {
                for i in node.inputs.iter().flatten() {
                    if seen.insert(i.clone()) {
                        stack.push(i.clone());
                    }
                }
            }
        }
        seen
    }

    // ---- parameter resolution (defaults, expressions, references) ----

    fn resolve_params(&self, id: &str, node: &Node, depth: usize) -> Result<Params, String> {
        let schema = schema(&node.kind).ok_or_else(|| format!("{id}: unknown node type '{}'", node.kind))?;
        let mut params = Params::new();
        for p in schema.params {
            let raw = node.params.get(p.name).cloned().unwrap_or_else(|| p.default.clone());
            let numeric = matches!(p.default, Value::Number(_) | Value::Array(_));
            let v = if numeric { self.eval_value(raw, depth).map_err(|e| format!("{id}.{}: {e}", p.name))? } else { raw };
            params.insert(p.name.into(), v);
        }
        if pt_sop::uses_frame(&node.kind) {
            params.insert("__frame".into(), json!(self.frame));
        }
        Ok(params)
    }

    fn eval_value(&self, v: Value, depth: usize) -> Result<Value, String> {
        match v {
            Value::String(s) if s.starts_with('=') => Ok(json!(self.eval_expr(&s[1..], depth)?)),
            Value::Array(a) => Ok(Value::Array(a.into_iter().map(|x| self.eval_value(x, depth)).collect::<Result<_, _>>()?)),
            other => Ok(other),
        }
    }

    fn eval_expr(&self, src: &str, depth: usize) -> Result<f32, String> {
        if depth > 16 {
            return Err("expression references nest too deeply (cycle?)".into());
        }
        let code = format!("@__r = ({});", src.replace("$F", "@Frame").replace("$T", "@Time"));
        let prog = pt_vex::analyze(&code, &BTreeMap::new()).map_err(|ds| format!("expression '{src}': {}", ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")))?;
        let mut chans = Vec::new();
        for (name, ty) in &prog.chans {
            if *ty != pt_vex::Ty::Float {
                return Err(format!("expression '{src}': only ch() (floats) is allowed in parameter expressions"));
            }
            chans.push([self.param_ref(name, depth + 1)?; 3]);
        }
        let inp = pt_vex::Inputs { n: 1, attrs: prog.attrs.iter().map(|_| None).collect(), chans, frame: self.frame };
        let out = prog.run::<0>(&inp);
        Ok(out.written.first().map(|w| w.1[0][0]).unwrap_or(0.0))
    }

    /// `node/param` or `node/param/index` -> number.
    fn param_ref(&self, path: &str, depth: usize) -> Result<f32, String> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(format!("reference \"{path}\" must look like node/param or node/param/index"));
        }
        let node = self.nodes.get(parts[0]).ok_or_else(|| format!("reference \"{path}\": no node '{}'", parts[0]))?;
        // Resolve only the referenced parameter: resolving the whole node would turn a
        // legitimate reference to a sibling parameter into a false cycle.
        let schema = schema(&node.kind).ok_or_else(|| format!("reference \"{path}\": unknown node type '{}'", node.kind))?;
        let Some(p) = schema.params.iter().find(|p| p.name == parts[1]) else {
            let valid: Vec<_> = schema.params.iter().map(|p| p.name).collect();
            return Err(format!("reference \"{path}\": node '{}' has no parameter '{}'. Valid: {valid:?}", parts[0], parts[1]));
        };
        let raw = node.params.get(parts[1]).cloned().unwrap_or_else(|| p.default.clone());
        let v = if matches!(p.default, Value::Number(_) | Value::Array(_)) { self.eval_value(raw, depth)? } else { raw };
        match (&v, parts.get(2)) {
            (Value::Number(n), None) => Ok(n.as_f64().unwrap_or(0.0) as f32),
            (Value::Array(a), Some(i)) => {
                let i: usize = i.parse().map_err(|_| format!("reference \"{path}\": index must be a number"))?;
                a.get(i).and_then(Value::as_f64).map(|x| x as f32).ok_or_else(|| format!("reference \"{path}\": index {i} out of range"))
            }
            (Value::Array(_), None) => Err(format!("reference \"{path}\" is a vector; add an index, e.g. {path}/0")),
            _ => Err(format!("reference \"{path}\" is not a number")),
        }
    }

    fn consumers(&self) -> HashMap<&str, usize> {
        let mut m: HashMap<&str, usize> = HashMap::new();
        for n in self.nodes.values() {
            for i in n.inputs.iter().flatten() {
                *m.entry(i.as_str()).or_insert(0) += 1;
            }
        }
        m
    }

    // ---- assets ----

    /// Package the subgraph feeding `output` into an asset. Nodes listed in `inputs` become
    /// the asset's input slots; `promote` = (public name, node, param) triples.
    pub fn make_asset(&self, name: &str, doc: &str, output: &str, inputs: &[String], promote: &[(String, String, String)]) -> Result<Asset, String> {
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err("asset names must be non-empty without spaces".into());
        }
        if !self.nodes.contains_key(output) {
            return Err(format!("no node '{output}'"));
        }
        let slot_of: HashMap<&str, usize> = inputs.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
        for n in inputs {
            if !self.nodes.contains_key(n) {
                return Err(format!("input boundary node '{n}' does not exist"));
            }
        }
        let mut nodes = BTreeMap::new();
        let mut stack = vec![output.to_string()];
        let rename = |id: &str| -> String {
            match slot_of.get(id) {
                Some(k) => format!("__input{k}"),
                None => id.to_string(),
            }
        };
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(&k) = slot_of.get(id.as_str()) {
                let mut params = Params::new();
                params.insert("slot".into(), json!(k));
                nodes.insert(format!("__input{k}"), Node { kind: "input".into(), params, inputs: vec![] });
                continue;
            }
            let mut n = self.nodes[&id].clone();
            if is_asset_kind(&n.kind) && n.params.get("asset").and_then(Value::as_str) == Some(name) {
                return Err(format!("asset '{name}' would contain itself (node '{id}')"));
            }
            for i in n.inputs.iter_mut().flatten() {
                stack.push(i.clone());
                *i = rename(i);
            }
            nodes.insert(id, n);
        }
        let mut promoted = Vec::new();
        for (pname, node, param) in promote {
            let n = nodes.get(node).ok_or_else(|| format!("promoted parameter '{pname}': node '{node}' is not inside the asset"))?;
            let sc = schema(&n.kind).unwrap();
            let p = sc.params.iter().find(|p| p.name == param).ok_or_else(|| format!("promoted parameter '{pname}': '{}' has no parameter '{param}'", n.kind))?;
            let default = n.params.get(param).cloned().unwrap_or_else(|| p.default.clone());
            promoted.push(Promoted { name: pname.clone(), node: node.clone(), param: param.clone(), default });
        }
        Ok(Asset { name: name.into(), doc: doc.into(), nodes, output: rename(output), promoted, num_inputs: inputs.len() })
    }

    pub fn add_asset(&mut self, a: Asset) {
        self.assets.insert(a.name.clone(), a);
    }

    fn asset(&self, name: &str) -> Result<&Asset, String> {
        self.assets.get(name).ok_or_else(|| format!("asset '{name}' not found (available: {:?})", self.assets.keys().collect::<Vec<_>>()))
    }

    fn sub_graph(&self, asset: &Asset, values: &serde_json::Map<String, Value>, iter: Option<(&str, usize)>) -> Graph {
        let mut sub = Graph { nodes: asset.nodes.clone(), assets: self.assets.clone(), frame: self.frame, provenance: self.provenance, cache_dir: self.cache_dir.clone(), asset_depth: self.asset_depth + 1, ..Graph::default() };
        for pr in &asset.promoted {
            let mut v = values.get(&pr.name).cloned().unwrap_or_else(|| pr.default.clone());
            if let Some((name, i)) = iter {
                if name == pr.name {
                    v = json!(i);
                }
            }
            if let Some(n) = sub.nodes.get_mut(&pr.node) {
                n.params.insert(pr.param.clone(), v);
            }
        }
        sub
    }

    fn cook_asset(&mut self, kind: &str, p: &Params, ins: &[Option<Entry>]) -> Result<Geometry, String> {
        if self.asset_depth >= MAX_ASSET_DEPTH {
            return Err("assets are nested too deeply".into());
        }
        let asset = self.asset(&p.s("asset"))?.clone();
        let empty = serde_json::Map::new();
        let values = p.get("values").and_then(Value::as_object).unwrap_or(&empty).clone();
        let (iterations, iter_param) = if kind == "repeat" { (p.i("iterations").clamp(0, 64) as usize, p.s("iter_param")) } else { (1, String::new()) };
        let mut cur: Vec<Option<Entry>> = ins.to_vec();
        for it in 0..iterations {
            let mut sub = self.sub_graph(&asset, &values, if iter_param.is_empty() { None } else { Some((iter_param.as_str(), it)) });
            sub.sub_inputs = cur.clone();
            std::mem::swap(&mut sub.cache, &mut self.cache);
            let r = sub.cook_entry(&asset.output);
            std::mem::swap(&mut sub.cache, &mut self.cache);
            self.stats.computed += sub.stats.computed;
            self.stats.mem_hits += sub.stats.mem_hits;
            self.stats.disk_hits += sub.stats.disk_hits;
            self.stats.fused_passes += sub.stats.fused_passes;
            self.stats.nodes_fused_away += sub.stats.nodes_fused_away;
            cur = vec![Some(r.map_err(|e| format!("inside asset '{}': {e}", asset.name))?)];
        }
        match cur.into_iter().next().flatten() {
            Some(e) => Ok((*e.geo).clone()),
            None => Err("asset produced nothing".into()),
        }
    }

    // ---- cooking ----

    /// Cook a node (and everything upstream).
    pub fn cook(&mut self, id: &str) -> Result<Arc<Geometry>, String> {
        Ok(self.cook_entry(id)?.geo)
    }

    pub fn cook_entry(&mut self, id: &str) -> Result<Entry, String> {
        if !self.nodes.contains_key(id) {
            return Err(format!("no node '{id}'"));
        }
        let mut memo = HashMap::new();
        self.cook_node(id, &mut memo)
    }

    fn cook_node(&mut self, id: &str, memo: &mut HashMap<String, Entry>) -> Result<Entry, String> {
        if let Some(e) = memo.get(id) {
            return Ok(e.clone());
        }
        let tail = self.nodes.get(id).ok_or_else(|| format!("no node '{id}'"))?.clone();
        let tail_params = self.resolve_params(id, &tail, 0)?;

        if tail.kind == "input" {
            let slot = tail_params.i("slot").max(0) as usize;
            let e = self.sub_inputs.get(slot).cloned().flatten().ok_or_else(|| format!("{id}: 'input' nodes only work inside assets, and slot {slot} was not provided"))?;
            memo.insert(id.to_string(), e.clone());
            return Ok(e);
        }

        // Collect a fusable chain (tail first), following input 0 through single-consumer
        // pointwise nodes.
        let mut chain: Vec<(String, Node, Params)> = vec![(id.to_string(), tail, tail_params)];
        if pt_sop::point_ops(&chain[0].1.kind, &chain[0].2).is_some() && !self.no_fusion {
            let consumers = self.consumers();
            loop {
                let last = chain.last().unwrap();
                let Some(up) = last.1.inputs.first().cloned().flatten() else { break };
                if consumers.get(up.as_str()).copied().unwrap_or(0) != 1 {
                    break;
                }
                let un = self.nodes[&up].clone();
                let up_params = self.resolve_params(&up, &un, 0)?;
                if pt_sop::point_ops(&un.kind, &up_params).is_none() {
                    break;
                }
                chain.push((up, un, up_params));
            }
        }
        chain.reverse(); // head first
        let fused = pt_sop::point_ops(&chain[0].1.kind, &chain[0].2).is_some();

        // Cook inputs of the chain head (fused chains have exactly one: the base geometry).
        let head = chain[0].1.clone();
        let mut in_entries: Vec<Option<Entry>> = Vec::new();
        for (slot, i) in head.inputs.iter().enumerate() {
            in_entries.push(match i {
                Some(up) => Some(self.cook_node(up, memo).map_err(|e| format!("{e}\n  (while cooking '{}' input {slot})", chain[0].0))?),
                None => None,
            });
        }

        // Content-addressed key.
        let mut h = blake3::Hasher::new();
        h.update(b"ptkey/1");
        h.update(BACKEND_CLASS.as_bytes());
        if self.provenance {
            h.update(id.as_bytes());
        }
        for (_, n, p) in &chain {
            let v = schema(&n.kind).map_or(0, |s| s.version);
            h.update(n.kind.as_bytes());
            h.update(&v.to_le_bytes());
            h.update(canonical(&json!(p)).to_string().as_bytes());
            if is_asset_kind(&n.kind) {
                let a = self.asset(&p.s("asset"))?;
                h.update(serde_json::to_string(a).unwrap().as_bytes());
            }
        }
        for e in &in_entries {
            match e {
                Some(e) => h.update(&e.hash),
                None => h.update(b"-"),
            };
        }
        let key = *h.finalize().as_bytes();

        if let Some(e) = self.cache.get(&key) {
            self.stats.mem_hits += 1;
            memo.insert(id.to_string(), e.clone());
            return Ok(e.clone());
        }
        if let Some(dir) = &self.cache_dir {
            let path = dir.join(format!("{}.ptgeo", hex(&key)));
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(g) = pt_core::io::from_bytes(&bytes) {
                    let e = Entry { hash: g.content_hash(), geo: Arc::new(g) };
                    self.stats.disk_hits += 1;
                    self.cache.insert(key, e.clone());
                    memo.insert(id.to_string(), e.clone());
                    return Ok(e);
                }
            }
        }

        let mut geo = if fused {
            let base = in_entries[0].as_ref().ok_or_else(|| format!("{}: input 0 is not connected", chain[0].0))?;
            let mut ops = Vec::new();
            for (_, n, p) in &chain {
                ops.extend(pt_sop::point_ops(&n.kind, p).unwrap());
            }
            self.stats.fused_passes += 1;
            self.stats.nodes_fused_away += chain.len() as u64 - 1;
            pt_sop::kernel::apply(&base.geo, &ops)
        } else if is_asset_kind(&head.kind) {
            self.cook_asset(&head.kind, &chain[0].2, &in_entries).map_err(|e| format!("{id}: {e}"))?
        } else {
            let inputs: Vec<Option<Arc<Geometry>>> = in_entries.iter().map(|e| e.as_ref().map(|e| e.geo.clone())).collect();
            pt_sop::cook(&head.kind, &chain[0].2, &inputs).map_err(|e| format!("{id}: {e}"))?
        };
        self.stats.computed += 1;
        if self.provenance {
            stamp(&mut geo, tag_of(id));
        }
        let e = Entry { hash: geo.content_hash(), geo: Arc::new(geo) };
        if let Some(dir) = &self.cache_dir {
            let _ = std::fs::write(dir.join(format!("{}.ptgeo", hex(&key))), pt_core::io::to_bytes(&e.geo));
        }
        self.cache.insert(key, e.clone());
        memo.insert(id.to_string(), e.clone());
        Ok(e)
    }

    /// Which node created point/primitive/instance `index` of `node`'s output, and how it got here.
    pub fn explain(&mut self, node: &str, domain: &str, index: usize) -> Result<Value, String> {
        if !self.provenance {
            return Err("provenance is off; enable it with set_option provenance=true and recook".into());
        }
        let g = self.cook(node)?;
        let col = match domain {
            "point" => g.point.get("__src"),
            "primitive" | "prim" => g.prim.get("__src"),
            "instance" => g.instances.as_ref().and_then(|i| i.attrs.get("__src")),
            d => return Err(format!("unknown domain '{d}' (use point | primitive | instance)")),
        };
        let Some(Column::I32(v)) = col else { return Err(format!("no provenance for domain '{domain}'")) };
        let tag = *v.get(index).ok_or_else(|| format!("index {index} out of range (0..{})", v.len()))?;
        self.creator_json(tag, Some(node)).ok_or_else(|| "creator node no longer exists".to_string())
    }

    /// What stamped provenance `tag`: the node (or the node inside an asset), its kind and parameters, and, when `to` is given, the
    /// path of nodes from it to `to`.
    pub fn creator_json(&self, tag: i32, to: Option<&str>) -> Option<Value> {
        if let Some(creator) = self.nodes.keys().find(|n| tag_of(n) == tag).cloned() {
            let cn = &self.nodes[&creator];
            let mut v = json!({"created_by": creator, "kind": cn.kind, "params": cn.params});
            if let Some(to) = to {
                v["path_from_creator"] = json!(self.path(&creator, to));
            }
            return Some(v);
        }
        for a in self.assets.values() {
            if let Some((n, node)) = a.nodes.iter().find(|(n, _)| tag_of(n) == tag) {
                return Some(json!({"created_by": format!("{}/{}", a.name, n), "kind": node.kind, "params": node.params, "inside_asset": a.name}));
            }
        }
        None
    }

    fn path(&self, from: &str, to: &str) -> Vec<String> {
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut queue = std::collections::VecDeque::from([from.to_string()]);
        let mut seen = HashSet::from([from.to_string()]);
        while let Some(n) = queue.pop_front() {
            if n == to {
                break;
            }
            for (id, node) in &self.nodes {
                if node.inputs.iter().flatten().any(|i| *i == n) && seen.insert(id.clone()) {
                    prev.insert(id.clone(), n.clone());
                    queue.push_back(id.clone());
                }
            }
        }
        let mut p = vec![to.to_string()];
        while let Some(q) = prev.get(p.last().unwrap()) {
            p.push(q.clone());
        }
        p.reverse();
        p
    }

    // ---- static attribute-flow check ----

    /// Predict every node's attribute environment without cooking and report precise errors.
    pub fn check(&self) -> CheckReport {
        let mut diags = Vec::new();
        let envs = self.flow(&[], 0, "", &mut diags);
        CheckReport { diags, envs }
    }

    fn flow(&self, input_envs: &[Option<GeoEnv>], depth: usize, prefix: &str, diags: &mut Vec<CheckDiag>) -> BTreeMap<String, Option<GeoEnv>> {
        let mut memo: BTreeMap<String, Option<GeoEnv>> = BTreeMap::new();
        for id in self.nodes.keys() {
            self.flow_node(id, input_envs, depth, prefix, diags, &mut memo);
        }
        memo
    }

    fn flow_node(&self, id: &str, input_envs: &[Option<GeoEnv>], depth: usize, prefix: &str, diags: &mut Vec<CheckDiag>, memo: &mut BTreeMap<String, Option<GeoEnv>>) -> Option<GeoEnv> {
        if let Some(e) = memo.get(id) {
            return e.clone();
        }
        let node = &self.nodes[id];
        let name = format!("{prefix}{id}");
        macro_rules! err {
            ($sev:expr, $msg:expr, $fix:expr) => {
                diags.push(CheckDiag { node: name.clone(), severity: $sev, message: $msg, fix: $fix })
            };
        }
        let mut result = None;
        'done: {
            let params = match self.resolve_params(id, node, 0) {
                Ok(p) => p,
                Err(e) => {
                    err!("error", e, "fix the parameter value or expression".into());
                    break 'done;
                }
            };
            if node.kind == "input" {
                result = input_envs.get(params.i("slot").max(0) as usize).cloned().flatten();
                break 'done;
            }
            let mut ins: Vec<Option<GeoEnv>> = Vec::new();
            let mut connected = 0;
            let mut upstream_failed = false;
            for slot in &node.inputs {
                match slot {
                    Some(up) => {
                        connected += 1;
                        let e = self.flow_node(up, input_envs, depth, prefix, diags, memo);
                        upstream_failed |= e.is_none();
                        ins.push(e);
                    }
                    None => ins.push(None),
                }
            }
            let need = if is_asset_kind(&node.kind) { 0 } else { pt_sop::min_inputs(&node.kind) };
            if connected < need {
                let missing = node.inputs.iter().position(Option::is_none).unwrap_or(0);
                err!("error", format!("'{}' needs {need} connected input(s) but {connected} are connected (input {missing} is empty)", node.kind), format!("connect a node to input {missing} of '{id}'"));
                break 'done;
            }
            if upstream_failed {
                break 'done; // upstream error already reported; avoid cascades
            }
            if is_asset_kind(&node.kind) {
                let asset = match self.asset(&params.s("asset")) {
                    Ok(a) => a,
                    Err(e) => {
                        err!("error", e, "create the asset first (make_asset) or fix the name".into());
                        break 'done;
                    }
                };
                if depth >= MAX_ASSET_DEPTH {
                    err!("error", "assets nested too deeply".into(), "flatten the asset nesting".into());
                    break 'done;
                }
                let empty = serde_json::Map::new();
                let values = params.get("values").and_then(Value::as_object).unwrap_or(&empty);
                let sub = self.sub_graph(asset, values, None);
                let passes = if node.kind == "repeat" { params.i("iterations").clamp(1, 2) } else { 1 };
                let mut cur = ins.clone();
                let mut before = diags.len();
                let mut last = None;
                for _ in 0..passes {
                    let envs = sub.flow(&cur, depth + 1, &format!("{name}/"), diags);
                    last = envs.get(&asset.output).cloned().flatten();
                    cur = vec![last.clone()];
                    if diags.len() > before {
                        break;
                    }
                    before = diags.len();
                }
                result = last;
                break 'done;
            }
            match pt_sop::flow::effect(&node.kind, &params, &ins) {
                Ok(e) => result = Some(e),
                Err(ds) => {
                    for d in ds {
                        err!("error", d.message, d.fix);
                    }
                }
            }
        }
        memo.insert(id.to_string(), result.clone());
        result
    }

    pub fn describe(&self) -> Value {
        json!({"nodes": self.nodes, "assets": self.assets.keys().collect::<Vec<_>>(), "frame": self.frame})
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    /// Cheap snapshot of the graph definition (for undo / checkpoints).
    pub fn snapshot(&self) -> String {
        serde_json::to_string(&(&self.nodes, &self.assets, self.frame)).unwrap()
    }

    pub fn restore(&mut self, snap: &str) -> Result<(), String> {
        let (nodes, assets, frame): (BTreeMap<String, Node>, BTreeMap<String, Asset>, f32) = serde_json::from_str(snap).map_err(|e| e.to_string())?;
        self.nodes = nodes;
        self.assets = assets;
        self.frame = frame;
        Ok(())
    }
}

/// Fill `__src` where missing (or wrongly sized) with this node's tag; preserves creators
/// of elements that passed through unchanged.
fn stamp(g: &mut Geometry, tag: i32) {
    fn fix(set: &mut pt_core::AttrSet, n: usize, tag: i32) {
        let ok = matches!(set.get("__src"), Some(Column::I32(v)) if v.len() == n);
        if !ok {
            set.set("__src", Column::from_i32(vec![tag; n]));
        }
    }
    let (np, nr) = (g.n_points(), g.n_prims());
    fix(&mut g.point, np, tag);
    fix(&mut g.prim, nr, tag);
    if let Some(i) = &mut g.instances {
        let n = i.len();
        fix(&mut i.attrs, n, tag);
    }
}

/// Resolved parameters of a node (defaults + overrides + evaluated expressions), for agents.
pub fn resolved_params(g: &Graph, id: &str) -> Option<Result<Params, String>> {
    g.nodes.get(id).map(|n| g.resolve_params(id, n, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Graph {
        let mut g = Graph::new();
        g.create("grid", Some("g")).unwrap();
        g.set_param("g", "rows", json!(40)).unwrap();
        g.set_param("g", "cols", json!(40)).unwrap();
        g.create("transform", Some("t")).unwrap();
        g.set_param("t", "s", json!([2.0, 1.0, 2.0])).unwrap();
        g.create("noise", Some("n")).unwrap();
        g.connect("g", "t", 0).unwrap();
        g.connect("t", "n", 0).unwrap();
        g.create("scatter", Some("s")).unwrap();
        g.set_param("s", "count", json!(500)).unwrap();
        g.connect("n", "s", 0).unwrap();
        g.create("box", Some("b")).unwrap();
        g.create("copy_to_points", Some("c")).unwrap();
        g.connect("b", "c", 0).unwrap();
        g.connect("s", "c", 1).unwrap();
        g
    }

    #[test]
    fn fused_equals_unfused_bitwise() {
        let mut a = scene();
        let mut b = scene();
        b.no_fusion = true;
        let ha = a.cook_entry("c").unwrap().hash;
        let hb = b.cook_entry("c").unwrap().hash;
        assert_eq!(ha, hb);
        assert!(a.stats.nodes_fused_away > 0 && b.stats.nodes_fused_away == 0);
    }

    #[test]
    fn deterministic_across_thread_counts() {
        let run = |threads: usize| {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
            pool.install(|| {
                let mut g = scene();
                g.cook_entry("c").unwrap().hash
            })
        };
        assert_eq!(run(1), run(7));
        assert_eq!(run(1), run(3));
    }

    #[test]
    fn incremental_recook_and_early_cutoff() {
        let mut g = scene();
        g.cook("c").unwrap();
        let first = g.stats.computed;
        g.cook("c").unwrap();
        assert_eq!(g.stats.computed, first, "second cook must be all cache hits");
        g.set_param("b", "size", json!([2.0, 2.0, 2.0])).unwrap();
        g.cook("c").unwrap();
        assert_eq!(g.stats.computed, first + 2, "only box and copy_to_points recompute");
        g.set_param("b", "size", json!([2.0, 2.0, 2.0])).unwrap();
        g.cook("c").unwrap();
        assert_eq!(g.stats.computed, first + 2);
    }

    #[test]
    fn instances_are_lazy_and_realize_matches() {
        let mut g = scene();
        let c = g.cook("c").unwrap();
        assert_eq!(c.n_points(), 0);
        assert_eq!(c.n_instances(), 500);
        let r = c.realize();
        assert_eq!(r.n_points(), 500 * 8);
        assert_eq!(r.n_prims(), 500 * 6);
    }

    #[test]
    fn disk_cache_roundtrip() {
        let dir = std::env::temp_dir().join("pt_cache_test2");
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = scene();
        a.set_cache_dir(&dir).unwrap();
        let ha = a.cook_entry("c").unwrap().hash;
        let mut b = scene();
        b.set_cache_dir(&dir).unwrap();
        let hb = b.cook_entry("c").unwrap().hash;
        assert_eq!(ha, hb);
        assert!(b.stats.disk_hits > 0 && b.stats.computed == 0);
    }

    #[test]
    fn provenance_explains_creator() {
        let mut g = scene();
        g.provenance = true;
        assert_eq!(g.explain("c", "instance", 3).unwrap()["created_by"], "c");
        assert_eq!(g.explain("n", "point", 5).unwrap()["created_by"], "g");
    }

    #[test]
    fn errors_are_actionable() {
        let mut g = scene();
        assert!(g.set_param("b", "sise", json!([1, 1, 1])).unwrap_err().contains("Valid:"));
        assert!(g.set_param("b", "size", json!(3)).is_err());
        assert!(g.connect("c", "b", 0).is_err());
        assert!(g.create("nope", None).unwrap_err().contains("Valid:"));
    }

    #[test]
    fn expressions_frames_and_references() {
        let mut g = Graph::new();
        g.create("box", Some("b")).unwrap();
        g.set_param("b", "size", json!(["=1+$F", 1.0, 1.0])).unwrap();
        g.create("tube", Some("t")).unwrap();
        g.set_param("t", "height", json!("=ch(\"b/size/0\") * 2")).unwrap();
        g.frame = 3.0;
        let tube = g.cook("t").unwrap();
        let (lo, hi) = tube.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 8.0).abs() < 1e-4, "height should be (1+3)*2 = 8, got {}", hi[1] - lo[1]);
        g.frame = 1.0;
        let tube = g.cook("t").unwrap();
        let (lo, hi) = tube.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 4.0).abs() < 1e-4);
        // Reference cycles are rejected, not infinite loops.
        g.set_param("b", "size", json!(["=ch(\"t/height\")", 1.0, 1.0])).unwrap();
        assert!(g.cook("t").unwrap_err().contains("deeply"));
        assert!(g.set_param("b", "size", json!(["=1+", 1.0, 1.0])).is_ok());
        assert!(g.cook("b").is_err());
    }

    #[test]
    fn frame_changes_only_recook_dependent_nodes() {
        let mut g = scene();
        g.create("wrangle", Some("w")).unwrap();
        g.set_param("w", "code", json!("@P.y += @Frame * 0.1;")).unwrap();
        g.connect("n", "w", 0).unwrap();
        g.cook("w").unwrap();
        g.cook("c").unwrap();
        let before = g.stats.computed;
        g.frame = 2.0;
        g.cook("w").unwrap();
        g.cook("c").unwrap();
        assert_eq!(g.stats.computed, before + 1, "only the frame-dependent wrangle recooks");
    }

    fn pillar_asset(g: &mut Graph) -> Asset {
        g.create("input", Some("in")).unwrap();
        g.create("tube", Some("pillar")).unwrap();
        g.create("transform", Some("lift")).unwrap();
        g.connect("pillar", "lift", 0).unwrap();
        g.make_asset("column", "a column", "lift", &[], &[("height".into(), "pillar".into(), "height".into()), ("radius".into(), "pillar".into(), "radius".into())]).unwrap()
    }

    #[test]
    fn assets_promote_parameters_and_cache() {
        let mut g = Graph::new();
        let a = pillar_asset(&mut g);
        g.nodes.clear();
        g.add_asset(a);
        g.create("hda", Some("c1")).unwrap();
        g.set_param("c1", "asset", json!("column")).unwrap();
        g.set_param("c1", "values", json!({"height": 5.0})).unwrap();
        let geo = g.cook("c1").unwrap();
        let (lo, hi) = geo.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 5.0).abs() < 1e-4);
        let before = g.stats.computed;
        g.cook("c1").unwrap();
        assert_eq!(g.stats.computed, before, "asset result is cached");
        g.set_param("c1", "values", json!({"height": 7.0})).unwrap();
        let geo = g.cook("c1").unwrap();
        let (lo, hi) = geo.base_bounds().unwrap();
        assert!((hi[1] - lo[1] - 7.0).abs() < 1e-4);
        // Assets survive JSON round trips.
        let g2 = Graph::from_json(&g.to_json()).unwrap();
        assert!(g2.assets.contains_key("column"));
    }

    #[test]
    fn asset_inputs_and_repeat() {
        let mut g = Graph::new();
        g.create("grid", Some("src")).unwrap();
        g.create("peak", Some("up")).unwrap();
        g.connect("src", "up", 0).unwrap();
        g.set_param("up", "amount", json!(1.0)).unwrap();
        let a = g.make_asset("raise", "raise by amount", "up", &["src".to_string()], &[("amount".into(), "up".into(), "amount".into())]).unwrap();
        assert_eq!(a.num_inputs, 1);
        g.add_asset(a);
        g.nodes.clear();
        g.create("grid", Some("g")).unwrap();
        g.create("repeat", Some("loop")).unwrap();
        g.set_param("loop", "asset", json!("raise")).unwrap();
        g.set_param("loop", "iterations", json!(3)).unwrap();
        g.connect("g", "loop", 0).unwrap();
        let out = g.cook("loop").unwrap();
        let (lo, hi) = out.base_bounds().unwrap();
        assert!((lo[1] - 3.0).abs() < 1e-4 && (hi[1] - 3.0).abs() < 1e-4, "grid raised 3x1 m: {lo:?} {hi:?}");
    }

    #[test]
    fn static_check_catches_errors_before_cooking() {
        let mut g = Graph::new();
        g.create("grid", Some("g")).unwrap();
        g.create("copy_to_points", Some("c")).unwrap();
        g.set_param("c", "orient_to_normal", json!(1)).unwrap();
        g.create("box", Some("b")).unwrap();
        g.connect("b", "c", 0).unwrap();
        g.connect("g", "c", 1).unwrap();
        let r = g.check();
        assert!(r.diags.iter().any(|d| d.node == "c" && d.message.contains("no vec3 point attribute N")), "{:?}", r.diags.iter().map(|d| &d.message).collect::<Vec<_>>());
        // Fix it and the check is clean.
        g.create("normals", Some("nn")).unwrap();
        g.connect("g", "nn", 0).unwrap();
        g.connect("nn", "c", 1).unwrap();
        let r = g.check();
        assert!(r.diags.is_empty(), "{:?}", r.diags.iter().map(|d| &d.message).collect::<Vec<_>>());
        assert!(r.envs["c"].as_ref().unwrap().inst.is_some());
        // Unconnected required input.
        g.create("noise", Some("lonely")).unwrap();
        assert!(g.check().diags.iter().any(|d| d.node == "lonely" && d.message.contains("needs 1 connected")));
        // Wrangle reading a missing attribute.
        g.create("wrangle", Some("w")).unwrap();
        g.set_param("w", "code", json!("@P.y = @pscalee;")).unwrap();
        g.connect("g", "w", 0).unwrap();
        let r = g.check();
        assert!(r.diags.iter().any(|d| d.node == "w" && d.message.contains("@pscalee")));
    }
}
