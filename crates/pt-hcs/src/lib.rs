//! hcs: the agent-facing scene language. Python syntax (agents already know it), compiled to
//! the typed node graph: node types are functions, positional arguments are input nodes,
//! keyword arguments are parameters. Static feedback comes from precise diagnostics (with
//! fixes) here and from the attribute-flow checker in `pt-graph`.
//!
//! ```text
//! ground = grid(size=[30, 30], rows=60, cols=60)
//! hills  = noise(ground, amp=3, freq=0.1)
//! seeds  = scatter(hills, count=300)
//! rock   = sphere(radius=0.4)
//! rocks  = copy_to_points(rock, seeds, orient_to_normal=1)
//! out(merge(hills, rocks))
//! ```

mod interp;
mod lex;
mod parse;

use pt_graph::Graph;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Diag {
    pub line: u32,
    pub col: u32,
    pub msg: String,
    pub fix: String,
}

impl Diag {
    pub fn new(line: u32, col: u32, msg: String, fix: impl Into<String>) -> Self {
        Diag { line, col, msg, fix: fix.into() }
    }
    pub fn to_json(&self) -> Value {
        json!({"line": self.line, "col": self.col, "message": self.msg, "fix": self.fix})
    }
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}: {} (fix: {})", self.line, self.col, self.msg, self.fix)
    }
}

#[derive(Clone, Debug)]
pub struct SpecDef {
    pub node: String,
    pub kind: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub name: Option<String>,
    pub line: u32,
}

#[derive(Clone, Debug, Default)]
pub struct RunReport {
    pub outputs: Vec<String>,
    pub created: Vec<String>,
    pub node_lines: BTreeMap<String, u32>,
    pub specs: Vec<SpecDef>,
    pub prints: Vec<String>,
}

impl RunReport {
    pub fn to_json(&self) -> Value {
        json!({"outputs": self.outputs, "nodes_created": self.created.len(), "nodes": self.created,
               "specs": self.specs.len(), "prints": self.prints})
    }
}

/// Parse and run `code`, building nodes into `g`. With `replace`, the graph's nodes are
/// cleared first so a script is a complete, idempotent description of the scene. On error
/// the graph is restored to its previous state.
pub fn run(code: &str, g: &mut Graph, replace: bool) -> Result<RunReport, Vec<Diag>> {
    let ast = parse::parse(code).map_err(|d| vec![d])?;
    let snap = g.snapshot();
    if replace {
        g.nodes.clear();
        g.frame = 1.0;
    }
    let mut it = interp::Interp::new(g);
    let r = it.run(&ast);
    let report = std::mem::take(&mut it.report);
    match r {
        Ok(()) => Ok(report),
        Err(d) => {
            g.restore(&snap).ok();
            Err(vec![d])
        }
    }
}

/// Full static check without touching `g`: build into a scratch copy, then run the
/// attribute-flow checker and map node diagnostics back to source lines.
pub fn check(code: &str, g: &Graph) -> Result<(RunReport, Vec<Diag>), Vec<Diag>> {
    let mut scratch = Graph::new();
    scratch.assets = g.assets.clone();
    scratch.frame = g.frame;
    let report = run(code, &mut scratch, true)?;
    let flow = scratch.check();
    let diags = flow
        .diags
        .iter()
        .map(|d| {
            let base = d.node.split('/').next().unwrap_or(&d.node);
            let line = report.node_lines.get(base).copied().unwrap_or(1);
            Diag::new(line, 1, format!("{} (node '{}')", d.message, d.node), d.fix.clone())
        })
        .collect();
    Ok((report, diags))
}

#[derive(Clone, Debug)]
pub struct SpecResult {
    pub ok: bool,
    pub kind: String,
    pub node: String,
    pub message: String,
    pub measured: Option<f64>,
    pub line: u32,
}

impl SpecResult {
    pub fn to_json(&self) -> Value {
        json!({"ok": self.ok, "spec": self.kind, "node": self.node, "message": self.message, "measured": self.measured, "line": self.line})
    }
}

const SPEC_KINDS: &[&str] = &["height", "width", "depth", "points", "prims", "instances", "watertight", "finite", "grounded", "attribute"];

/// Cook each spec's node and check it. Specs are the agent's definition of "done".
pub fn evaluate_specs(g: &mut Graph, specs: &[SpecDef]) -> Vec<SpecResult> {
    specs
        .iter()
        .map(|s| {
            let mk = |ok: bool, message: String, measured: Option<f64>| SpecResult { ok, kind: s.kind.clone(), node: s.node.clone(), message, measured, line: s.line };
            let geo = match g.cook(&s.node) {
                Ok(geo) => geo,
                Err(e) => return mk(false, format!("could not cook '{}': {e}", s.node), None),
            };
            let range = |v: f64, what: &str| -> SpecResult {
                let lo_ok = s.min.is_none_or(|m| v >= m);
                let hi_ok = s.max.is_none_or(|m| v <= m);
                let want = format!("[{}, {}]", s.min.map_or("-inf".into(), |x| x.to_string()), s.max.map_or("inf".into(), |x| x.to_string()));
                mk(lo_ok && hi_ok, format!("{what} = {v:.4}, required in {want}"), Some(v))
            };
            let size = |axis: usize| geo.bounds().map(|(lo, hi)| (hi[axis] - lo[axis]) as f64).unwrap_or(0.0);
            match s.kind.as_str() {
                "height" => range(size(1), "height (m)"),
                "width" => range(size(0), "width (m)"),
                "depth" => range(size(2), "depth (m)"),
                "points" => range(geo.realize().n_points() as f64, "points"),
                "prims" => range(geo.realize().n_prims() as f64, "primitives"),
                "instances" => range(geo.n_instances() as f64, "instances"),
                "grounded" => {
                    let y = geo.bounds().map(|b| b.0[1] as f64).unwrap_or(f64::NAN);
                    let (lo, hi) = (s.min.unwrap_or(-0.01), s.max.unwrap_or(0.01));
                    mk(y >= lo && y <= hi, format!("lowest point y = {y:.4}, required in [{lo}, {hi}]"), Some(y))
                }
                "finite" => {
                    let bad = pt_core::validate::validate(&geo.realize()).iter().any(|i| i.code == "non_finite_points" || i.code == "non_finite_instance_xf");
                    mk(!bad, if bad { "NaN/inf values found".into() } else { "all values finite".into() }, None)
                }
                "watertight" => {
                    let iss = pt_core::validate::validate(&geo.realize());
                    let open = iss.iter().find(|i| i.code == "boundary_edges").map(|i| !i.message.starts_with("0 ")).unwrap_or(false);
                    let nonman = iss.iter().any(|i| i.code == "non_manifold_edges");
                    mk(!open && !nonman, format!("open boundary: {open}, non-manifold: {nonman}"), None)
                }
                "attribute" => {
                    let name = s.name.clone().unwrap_or_default();
                    let has = geo.point.get(&name).is_some() || geo.prim.get(&name).is_some() || geo.instances.as_ref().is_some_and(|i| i.attrs.get(&name).is_some());
                    mk(has, format!("attribute '{name}' {}", if has { "present" } else { "missing" }), None)
                }
                other => mk(false, format!("unknown spec '{other}' (use one of {SPEC_KINDS:?})"), None),
            }
        })
        .collect()
}

const PY_KEYWORDS: &[&str] = &["frame", "def", "if", "elif", "else", "for", "in", "return", "pass", "and", "or", "not", "True", "False", "None", "range", "out", "spec", "print", "asset"];

fn lit(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Number(n) => format!("{}", n.as_f64().unwrap_or(0.0)),
        Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(a) => format!("[{}]", a.iter().map(lit).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => format!("{{{}}}", o.iter().map(|(k, v)| format!("{}: {}", serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string()), lit(v))).collect::<Vec<_>>().join(", ")),
    }
}

/// Print a graph as an hcs program (assets are not emitted; use the graph JSON for those).
pub fn to_hcs(g: &Graph) -> String {
    // Topological order (inputs first), stable by node id.
    let mut order: Vec<&String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    fn visit<'a>(g: &'a Graph, id: &'a String, seen: &mut std::collections::HashSet<&'a String>, order: &mut Vec<&'a String>) {
        if !seen.insert(id) {
            return;
        }
        for up in g.nodes[id].inputs.iter().flatten() {
            if let Some((k, _)) = g.nodes.get_key_value(up) {
                visit(g, k, seen, order);
            }
        }
        order.push(id);
    }
    for id in g.nodes.keys() {
        visit(g, id, &mut seen, &mut order);
    }
    let mut var: BTreeMap<&String, String> = BTreeMap::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for id in &order {
        let mut v: String = id.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        if v.is_empty() || v.chars().next().is_some_and(|c| c.is_ascii_digit()) || PY_KEYWORDS.contains(&v.as_str()) || pt_graph::schema(&v).is_some() {
            v = format!("n_{v}");
        }
        while !used.insert(v.clone()) {
            v.push('_');
        }
        var.insert(id, v);
    }
    let mut out = String::from("# generated by pt (hcs)\n");
    let mut consumed = std::collections::HashSet::new();
    for id in &order {
        let n = &g.nodes[*id];
        let mut args: Vec<String> = Vec::new();
        let contiguous = n.inputs.iter().position(Option::is_none).is_none_or(|first_gap| n.inputs[first_gap..].iter().all(Option::is_none));
        for (slot, inp) in n.inputs.iter().enumerate() {
            if let Some(up) = inp {
                consumed.insert(up.clone());
                let name = var.get(up).cloned().unwrap_or_else(|| up.clone());
                args.push(if contiguous { name } else { format!("in{slot}={name}") });
            }
        }
        for (k, v) in &n.params {
            args.push(format!("{k}={}", lit(v)));
        }
        if var[id] != **id {
            args.push(format!("id={}", serde_json::to_string(id).unwrap_or_else(|_| "\"\"".to_string())));
        }
        out.push_str(&format!("{} = {}({})\n", var[id], n.kind, args.join(", ")));
    }
    let sinks: Vec<&str> = order.iter().filter(|id| !consumed.contains(**id)).map(|id| var[*id].as_str()).collect();
    if !sinks.is_empty() {
        out.push_str(&format!("out({})\n", sinks.join(", ")));
    }
    if !g.assets.is_empty() {
        out.push_str(&format!("# assets not emitted (kept in graph JSON): {:?}\n", g.assets.keys().collect::<Vec<_>>()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = r#"
ground = grid(size=[30, 30], rows=60, cols=60)
hills  = noise(ground, amp=3, freq=0.1)
seeds  = scatter(hills, count=300, pscale_min=0.5, pscale_max=1.5)
rock   = sphere(radius=0.4)
rocks  = copy_to_points(rock, seeds, orient_to_normal=1, rot_y_random=360)
out(merge(hills, rocks))
"#;

    #[test]
    fn builds_a_scene_and_cooks_it() {
        let mut g = Graph::new();
        let r = run(SCENE, &mut g, true).unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(r.outputs.len(), 1);
        assert!(g.nodes.contains_key("ground") && g.nodes.contains_key("rocks"));
        assert!(g.check().diags.is_empty());
        let geo = g.cook(&r.outputs[0]).unwrap();
        assert_eq!(geo.n_instances(), 300);
        assert!(geo.n_points() > 3000);
    }

    #[test]
    fn functions_loops_and_comprehensions() {
        let code = r#"
def post(h, r=0.05):
    return tube(radius=r, height=h, cols=8, caps=1, center=[0, h / 2, 0])

def fence(n, spacing):
    posts = [transform(post(1.2), t=[i * spacing, 0, 0]) for i in range(n)]
    return merge(posts)

f = fence(4, 0.5)
spec(f, "height", min=1.1, max=1.3)
spec(f, "grounded")
out(f)
"#;
        let mut g = Graph::new();
        let r = run(code, &mut g, true).unwrap_or_else(|e| panic!("{}", e[0]));
        assert_eq!(g.nodes.values().filter(|n| n.kind == "tube").count(), 4);
        let res = evaluate_specs(&mut g, &r.specs);
        assert!(res.iter().all(|s| s.ok), "{:?}", res.iter().map(|s| &s.message).collect::<Vec<_>>());
    }

    #[test]
    fn diagnostics_are_precise() {
        let mut g = Graph::new();
        let e = run("x = nois(grid())", &mut g, true).unwrap_err();
        assert!(e[0].fix.contains("noise") && e[0].line == 1, "{}", e[0]);
        let e = run("g = grid()\nn = noise(g, amplitude=2)", &mut g, true).unwrap_err();
        assert!(e[0].msg.contains("Valid:") && e[0].line == 2);
        let e = run("n = noise(1.5)", &mut g, true).unwrap_err();
        assert!(e[0].msg.contains("expected a node"));
        let e = run("for i in range(3):\nx = 1", &mut g, true).unwrap_err();
        assert!(e[0].msg.contains("indented"));
        let e = run("a = grid(\nb = 2", &mut g, true).unwrap_err();
        assert!(!e.is_empty());
        let e = run("y = undefined_name + 1", &mut g, true).unwrap_err();
        assert!(e[0].msg.contains("unknown name"));
        // Failed runs leave the graph untouched.
        run("keep = grid()", &mut g, true).unwrap();
        assert!(run("a = grid()\nb = bad()", &mut g, true).is_err());
        assert!(g.nodes.contains_key("keep") && !g.nodes.contains_key("a"));
    }

    #[test]
    fn check_maps_flow_errors_to_lines() {
        let code = "g = grid()\nbox1 = box()\nc = copy_to_points(box1, g, orient_to_normal=1)\nout(c)\n";
        let (_, diags) = check(code, &Graph::new()).unwrap();
        assert!(diags.iter().any(|d| d.line == 3 && d.msg.contains("N")), "{:?}", diags.iter().map(|d| d.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn round_trips_through_text() {
        let mut g = Graph::new();
        let r = run(SCENE, &mut g, true).unwrap();
        let h1 = g.cook_entry(&r.outputs[0]).unwrap().hash;
        let text = to_hcs(&g);
        let mut g2 = Graph::new();
        let r2 = run(&text, &mut g2, true).unwrap_or_else(|e| panic!("{}\n{text}", e[0]));
        let h2 = g2.cook_entry(&r2.outputs[0]).unwrap().hash;
        assert_eq!(h1, h2, "printed program must rebuild the identical scene:\n{text}");
    }

    #[test]
    fn assets_from_script() {
        let code = r#"
shaft = tube(radius=0.3, height=3, cols=12)
cap   = tube(radius=0.45, height=0.3, cols=12, center=[0, 1.65, 0])
col   = merge(shaft, cap)
asset("column", output=col, promote={"height": [shaft, "height"], "radius": [shaft, "radius"]})
c1 = hda(asset="column", values={"height": 6.0})
out(c1)
"#;
        let mut g = Graph::new();
        let r = run(code, &mut g, true).unwrap_or_else(|e| panic!("{}", e[0]));
        let geo = g.cook(&r.outputs[0]).unwrap();
        let (lo, hi) = geo.base_bounds().unwrap();
        assert!(hi[1] - lo[1] > 5.9, "promoted height applied: {}", hi[1] - lo[1]);
    }

    #[test]
    fn knobs_and_the_tier_come_from_the_caller_and_default_otherwise() {
        let code = "w = knob(\"width\", 1.0)\nh = lookdev(2.0, 0.5)\nb = box(size=[w, h, 1.0])\nout(b)";
        let size = |g: &mut Graph| {
            let r = run(code, g, true).unwrap_or_else(|e| panic!("{}", e[0]));
            let (lo, hi) = g.cook(&r.outputs[0]).unwrap().base_bounds().unwrap();
            [hi[0] - lo[0], hi[1] - lo[1]]
        };
        let mut g = Graph::new();
        assert_eq!(size(&mut g), [1.0, 2.0], "defaults: no knob given, final tier");
        g.knobs.insert("width".into(), json!(3.5));
        g.lookdev = true;
        assert_eq!(size(&mut g), [3.5, 0.5], "the style's knob and the look-development value");
        // a knob of the wrong kind says what to do
        g.knobs.insert("width".into(), json!("wide"));
        let e = run(code, &mut g, true).unwrap_err();
        assert!(e[0].msg.contains("knob \"width\"") && e[0].fix.contains("same kind"), "{}", e[0]);
    }

    #[test]
    fn edits_of_reads_the_scope_or_gives_an_empty_list() {
        let dir = std::env::temp_dir().join(format!("pt_hcs_edits_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("e.json");
        std::fs::write(&file, r#"{"rocks": [{"id": 5, "op": "remove"}]}"#).unwrap();
        let p = file.to_string_lossy().replace('\\', "/");
        let mut g = Graph::new();
        let code = format!("n = len(edits_of(\"{p}\", \"rocks\"))\nm = len(edits_of(\"{p}\", \"trees\"))\nk = len(edits_of(\"nowhere.json\", \"rocks\"))\nout(box(size=[n + 1, m + 1, k + 1]))");
        let r = run(&code, &mut g, true).unwrap_or_else(|e| panic!("{}", e[0]));
        let (lo, hi) = g.cook(&r.outputs[0]).unwrap().base_bounds().unwrap();
        assert_eq!([hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]], [2.0, 1.0, 1.0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spec_failures_are_reported() {
        let mut g = Graph::new();
        let r = run("b = box(size=[1, 5, 1])\nspec(b, \"height\", min=1, max=2)\nspec(b, \"watertight\")\nspec(b, \"nonsense\")", &mut g, true).unwrap();
        let res = evaluate_specs(&mut g, &r.specs);
        assert!(!res[0].ok && res[0].measured.unwrap() > 4.9);
        assert!(res[1].ok);
        assert!(!res[2].ok && res[2].message.contains("unknown spec"));
    }
}
