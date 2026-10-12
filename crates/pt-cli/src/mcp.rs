//! MCP server (JSON-RPC 2.0 over stdio, newline-delimited). Local-only by construction:
//! it speaks only on stdin/stdout and never opens a network socket.

use pt_graph::Graph;
use pt_render::{Mode, Opts};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};

pub enum Out {
    Json(Value),
    Image { png: Vec<u8>, meta: Value },
}

pub struct Session {
    pub graph: Graph,
    undo: Vec<String>,
    redo: Vec<String>,
    checkpoints: HashMap<String, String>,
}

fn s<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a.get(k).and_then(Value::as_str).ok_or_else(|| format!("missing string argument '{k}'"))
}

fn tool_defs() -> Value {
    let t = |name: &str, desc: &str, props: Value, req: Value| {
        json!({"name": name, "description": desc,
               "inputSchema": {"type": "object", "properties": props, "required": req}})
    };
    let str_ = || json!({"type": "string"});
    json!([
        t("list_node_types", "List every node type with docs, input count, differentiability and parameters (with defaults). Call first.", json!({}), json!([])),
        t("describe_node", "Full schema of one node type, or of an existing node (with resolved params).", json!({"kind": str_(), "node": str_()}), json!([])),
        t("search_nodes", "Find node types by intent keywords, e.g. 'scatter points on surface'.", json!({"query": str_()}), json!(["query"])),
        t("create_node", "Create a node. Returns its id.", json!({"kind": str_(), "name": str_()}), json!(["kind"])),
        t("set_param", "Set a parameter (JSON number, string or array shaped like the default).", json!({"node": str_(), "name": str_(), "value": {}}), json!(["node", "name", "value"])),
        t("connect", "Wire output of `from` into input slot `input` (default 0) of `to`. Rejects cycles.", json!({"from": str_(), "to": str_(), "input": {"type": "integer"}}), json!(["from", "to"])),
        t("disconnect", "Clear an input slot.", json!({"to": str_(), "input": {"type": "integer"}}), json!(["to"])),
        t("delete_node", "Delete a node and disconnect its outputs.", json!({"node": str_()}), json!(["node"])),
        t("batch", "Apply many edits atomically (rolled back on any error). ops: [{op:'create_node'|'set_param'|'connect'|'disconnect'|'delete_node', ...args}].", json!({"ops": {"type": "array", "items": {"type": "object"}}}), json!(["ops"])),
        t("get_graph", "Return the whole graph as JSON.", json!({}), json!([])),
        t("load_graph", "Replace the graph from a JSON string produced by get_graph.", json!({"graph": str_()}), json!(["graph"])),
        t("checkpoint", "Save the current graph under a name.", json!({"name": str_()}), json!(["name"])),
        t("restore", "Restore a named checkpoint.", json!({"name": str_()}), json!(["name"])),
        t("undo", "Undo the last edit.", json!({}), json!([])),
        t("redo", "Redo an undone edit.", json!({}), json!([])),
        t("cook", "Cook a node; returns geometry stats (counts, bounds, attribute min/max/mean, instances) and cache stats.", json!({"node": str_()}), json!(["node"])),
        t("validate", "Cook a node and check it: NaNs, bad indices, degenerate faces, non-manifold edges, scale sanity, instances. Each issue has a fix hint.", json!({"node": str_()}), json!(["node"])),
        t("explain", "Which node created point/primitive/instance #index of a node's output, and the path to it (needs provenance on).", json!({"node": str_(), "domain": str_(), "index": {"type": "integer"}}), json!(["node", "domain", "index"])),
        t("pick", "What is at pixel (x, y) of a frame from a standing scene camera, and what made it? Casts the camera ray into the cooked world (CPU, no render): returns the prototype and instance number under the pixel, the node and parameters that placed it (provenance), its height above the ground, and what ground-check flags. radius > 0 also counts the classes of object within that many pixels (so 'that patch of moss' names its node, not one stone). frame_size = [w, h] of the picture the pixel refers to (default 1920 x 1080).", json!({"node": str_(), "scene": {"description": "scene object or path of a scene .json with `cameras`"}, "camera": str_(), "x": {"type": "number"}, "y": {"type": "number"}, "radius": {"type": "number"}, "frame_size": {"type": "array", "items": {"type": "integer"}}}), json!(["node", "scene", "camera", "x", "y"])),
        t("render_preview", "Render a PNG preview you can look at. mode: shaded|wire|height|<attribute name>; sheet=true gives 4 views. Large instance sets are subsampled (reported).", json!({"node": str_(), "mode": str_(), "azimuth": {"type": "number"}, "elevation": {"type": "number"}, "width": {"type": "integer"}, "height": {"type": "integer"}, "sheet": {"type": "boolean"}, "path": str_()}), json!(["node"])),
        t("export", "Cook a node and write it to disk. format: obj | gltf | glb | usda (USD with native PointInstancer) | ptgeo. Material names from set_material are kept (glTF materials, USD GeomSubset bindings, OBJ usemtl); instances stay instances.", json!({"node": str_(), "path": str_(), "format": str_()}), json!(["node", "path"])),
        t("run_hcs", "Run an hcs program (Python syntax; see get_guide). Replaces the scene unless replace=false; a failed run changes nothing. Returns outputs' stats, flow diagnostics, spec results.", json!({"code": str_(), "replace": {"type": "boolean"}}), json!(["code"])),
        t("check_hcs", "Statically check an hcs program WITHOUT changing the scene or cooking: syntax, names, parameters and attribute flow, with line numbers and fixes.", json!({"code": str_()}), json!(["code"])),
        t("graph_to_hcs", "Print the current graph as an hcs program (round-trips).", json!({}), json!([])),
        t("check_graph", "Predict every node's attributes (point/primitive/instance) and report errors before cooking.", json!({}), json!([])),
        t("make_asset", "Package the subgraph feeding `output` as a reusable asset. inputs = boundary node ids (become input slots); promote = [{name,node,param}] exposed parameters. Use with node type `hda`.", json!({"name": str_(), "output": str_(), "inputs": {"type": "array", "items": {"type": "string"}}, "promote": {"type": "array", "items": {"type": "object"}}, "doc": str_()}), json!(["name", "output"])),
        t("list_assets", "List assets and their promoted parameters.", json!({}), json!([])),
        t("set_frame", "Set the playback frame ($F / @Frame).", json!({"frame": {"type": "number"}}), json!(["frame"])),
        t("diff", "Structural diff between two states: checkpoint names or \"current\".", json!({"a": str_(), "b": str_()}), json!(["a", "b"])),
        t("merge", "3-way merge into the current scene: base and theirs are checkpoint names (current = ours). Different parameters/nodes merge cleanly; real disagreements are returned as conflicts (ours wins; set force=true to apply anyway).", json!({"base": str_(), "theirs": str_(), "force": {"type": "boolean"}}), json!(["base", "theirs"])),
        t("optimize", "Inverse design: tune parameters to minimize an objective (deterministic CMA-ES with restarts + polish; cooks are cached so evaluations are cheap). params=[{node,param,index?,key?,min,max}] (key addresses object params such as a wrangle's channels). objective={type:'chamfer'|'size'|'silhouette'|'specs'|'image', ...}: chamfer/silhouette take target_node or target_obj (path to an .obj scan); size takes size=[x,y,z]; specs takes specs=[{kind,min,max,name?,node?}]; image takes path (PNG) + azimuth/elevation. The best parameters are applied (undoable).", json!({"node": str_(), "params": {"type": "array", "items": {"type": "object"}}, "objective": {"type": "object"}, "budget": {"type": "integer"}, "restarts": {"type": "integer"}, "seed": {"type": "integer"}}), json!(["node", "params", "objective"])),
        t("vary", "Render N diverse variants (Latin hypercube + maximin) of a node over parameter ranges as one contact sheet, plus each variant's parameter values. The scene is left unchanged; apply a favorite with set_param/batch.", json!({"node": str_(), "params": {"type": "array", "items": {"type": "object"}}, "count": {"type": "integer"}, "seed": {"type": "integer"}, "mode": str_(), "path": str_()}), json!(["node", "params"])),
        t("fit_wrangle", "Differentiable fit: tune a point wrangle's `channels` by gradient descent (forward-mode autodiff, <=8 channels) so its output attribute matches the same attribute on target_node (same point count). The fitted channels are applied.", json!({"node": str_(), "attr": str_(), "component": {"type": "integer"}, "target_node": str_(), "steps": {"type": "integer"}, "lr": {"type": "number"}}), json!(["node", "attr", "target_node"])),
        t("render_final", "Path-traced render (sun + sky + ground, soft shadows, glossy coat) for lookdev and final stills. turntable>1 renders that many azimuths into one sheet. Deterministic: same inputs, same pixels. Quality scales with spp (try 16 for drafts, 64+ for finals).", json!({"node": str_(), "width": {"type": "integer"}, "height": {"type": "integer"}, "spp": {"type": "integer"}, "azimuth": {"type": "number"}, "elevation": {"type": "number"}, "sun_azimuth": {"type": "number"}, "sun_elevation": {"type": "number"}, "sun_intensity": {"type": "number"}, "ground": {"type": "boolean"}, "roughness": {"type": "number"}, "turntable": {"type": "integer"}, "mode": str_(), "path": str_()}), json!(["node"])),
        t("render_compare", "Render 2-4 nodes side by side under ONE shared camera (framed on the first node, or on all nodes with frame=\"union\") so differences in size, proportion and count are visible. Cells go left-to-right, top-to-bottom in the order given; the result lists each node's true size in meters. Use it to compare variants or a result against a target.", json!({"nodes": {"type": "array", "items": {"type": "string"}}, "frame": str_(), "mode": str_(), "azimuth": {"type": "number"}, "elevation": {"type": "number"}, "path": str_()}), json!(["nodes"])),
        t("render_scene", "Photographic render of a node with Blender Cycles (about 10 s on the GPU): real sky and sun, shadows, haze, and your material NAMES (set_material) bound to CC0 PBR sets (names listed in get_guide, section 'Materials, light and photographic renders'; unknown names get a flat colour and are reported). `scene` is the description (object, or a path to a .json): lighting {sky:'nishita'|'hdri'|'flat', sun_elevation, sun_azimuth, fog_density...}, camera {location, look_at, lens} or {eye:{at:[x,z], height, yaw}} or `cameras` (a list, each with a name), render {samples, resolution, exposure}, ground, materials overrides. Empty scene = framed 3/4 view in daylight. Returns the first frame as a picture plus the report: frames, triangles, material_coverage, unresolved and unknown materials, warnings. quick=true renders at quarter samples and half size.", json!({"node": str_(), "scene": {}, "quick": {"type": "boolean"}, "samples": {"type": "integer"}, "lod": {"type": "boolean"}, "path": str_()}), json!(["node"])),
        t("critique", "Image-hygiene check of a node's render (framing, centering, contrast, clipping, detail) with a pass/fail rubric. It does NOT judge whether the model matches the brief (measured: chance-level agreement with spec results, see gym/critique_experiment.json); use spec(...) for that and render_compare to look. With a reference PNG it adds SSIM/PSNR/MSE.", json!({"node": str_(), "reference": str_(), "azimuth": {"type": "number"}, "elevation": {"type": "number"}}), json!(["node"])),
        t("list_recipes","List built-in example recipes (hcs programs with specs).", json!({}), json!([])),
        t("get_recipe", "Return a recipe's hcs source.", json!({"name": str_()}), json!(["name"])),
        t("propose_recipe", "Submit a recipe (hcs source with spec(...) checks). It is validated; if it passes it is stored in recipes/proposed/ for a human to accept.", json!({"name": str_(), "code": str_()}), json!(["name", "code"])),
        t("get_guide", "Return the agent guide: workflow, hcs language, wrangle language, technique notes.", json!({}), json!([])),
        t("set_option", "Session options: provenance (bool), cache_dir (string, enables the persistent cook cache).", json!({"provenance": {"type": "boolean"}, "cache_dir": str_()}), json!([])),
    ])
}

fn apply_edit(g: &mut Graph, op: &str, a: &Value) -> Result<Value, String> {
    match op {
        "create_node" => Ok(json!({"id": g.create(s(a, "kind")?, a.get("name").and_then(Value::as_str))?})),
        "set_param" => {
            g.set_param(s(a, "node")?, s(a, "name")?, a.get("value").cloned().ok_or("missing 'value'")?)?;
            Ok(json!("ok"))
        }
        "connect" => {
            g.connect(s(a, "from")?, s(a, "to")?, a.get("input").and_then(Value::as_u64).unwrap_or(0) as usize)?;
            Ok(json!("ok"))
        }
        "disconnect" => {
            g.disconnect(s(a, "to")?, a.get("input").and_then(Value::as_u64).unwrap_or(0) as usize)?;
            Ok(json!("ok"))
        }
        "delete_node" => g.delete(s(a, "node")?).map(|_| json!("ok")),
        _ => Err(format!("unknown edit op '{op}'")),
    }
}

impl Session {
    pub fn new() -> Self {
        let mut graph = Graph::new();
        graph.provenance = true;
        Session { graph, undo: vec![], redo: vec![], checkpoints: HashMap::new() }
    }

    fn parse_objective(&mut self, o: &Value, default_node: &str) -> Result<pt_opt::Objective, String> {
        use pt_opt::Objective;
        let kind = o.get("type").and_then(Value::as_str).ok_or("objective needs 'type'")?;
        let target_geo = |this: &mut Self| -> Result<std::sync::Arc<pt_core::Geometry>, String> {
            if let Some(n) = o.get("target_node").and_then(Value::as_str) {
                this.graph.cook(n)
            } else if let Some(p) = o.get("target_obj").and_then(Value::as_str) {
                Ok(std::sync::Arc::new(pt_core::io::load_obj(&std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?)?))
            } else {
                Err("objective needs target_node or target_obj".into())
            }
        };
        match kind {
            "chamfer" => Objective::chamfer(&*target_geo(self)?, o.get("samples").and_then(Value::as_u64).unwrap_or(1500) as usize),
            "silhouette" => Objective::silhouette(&*target_geo(self)?, o.get("res").and_then(Value::as_u64).unwrap_or(64).clamp(16, 256) as usize),
            "size" => {
                let v = o.get("size").and_then(Value::as_array).filter(|a| a.len() == 3).ok_or("size objective needs size=[x,y,z]")?;
                Ok(Objective::Size { target: [0, 1, 2].map(|i| v[i].as_f64().unwrap_or(1.0) as f32) })
            }
            "specs" => {
                let list = o.get("specs").and_then(Value::as_array).ok_or("specs objective needs specs=[...]")?;
                let specs = list
                    .iter()
                    .map(|s| pt_hcs::SpecDef {
                        node: s.get("node").and_then(Value::as_str).unwrap_or(default_node).to_string(),
                        kind: s.get("kind").and_then(Value::as_str).unwrap_or("height").to_string(),
                        min: s.get("min").and_then(Value::as_f64),
                        max: s.get("max").and_then(Value::as_f64),
                        name: s.get("name").and_then(Value::as_str).map(String::from),
                        line: 0,
                    })
                    .collect();
                Ok(Objective::Specs(specs))
            }
            "image" => {
                let path = o.get("path").and_then(Value::as_str).ok_or("image objective needs path to a PNG")?;
                let reference = pt_render::decode_png(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)?;
                let opts = Opts {
                    width: reference.w,
                    height: reference.h,
                    azimuth_deg: o.get("azimuth").and_then(Value::as_f64).unwrap_or(35.0) as f32,
                    elevation_deg: o.get("elevation").and_then(Value::as_f64).unwrap_or(28.0) as f32,
                    ..Opts::default()
                };
                Ok(Objective::Image { reference, opts })
            }
            other => Err(format!("unknown objective '{other}' (chamfer | silhouette | size | specs | image)")),
        }
    }

    fn fit_wrangle(&mut self, node: &str, attr: &str, comp: usize, target_node: &str, steps: usize, lr: f32) -> Result<Value, String> {
        let n = self.graph.nodes.get(node).ok_or_else(|| format!("no node '{node}'"))?.clone();
        if n.kind != "wrangle" {
            return Err(format!("'{node}' is a {} node, not a wrangle", n.kind));
        }
        let params = pt_graph::resolved_params(&self.graph, node).ok_or("no node")??;
        if params.get("class").and_then(Value::as_str) != Some("point") {
            return Err("fit_wrangle supports class=point wrangles".into());
        }
        let up = n.inputs.first().cloned().flatten().ok_or("wrangle input 0 is not connected")?;
        let input = self.graph.cook(&up)?;
        let target = self.graph.cook(target_node)?;
        let tcol = target.point.get(attr).ok_or_else(|| format!("target has no point attribute '{attr}'"))?;
        let tvals: Vec<f32> = match tcol {
            pt_core::Column::F32(v) => v.to_vec(),
            pt_core::Column::V3(v) => v.iter().map(|p| p[comp.min(2)]).collect(),
            _ => return Err("target attribute must be float or vec3".into()),
        };
        if tvals.len() != input.n_points() {
            return Err(format!("target has {} points but the wrangle input has {}", tvals.len(), input.n_points()));
        }
        let code = params.get("code").and_then(Value::as_str).unwrap_or("").to_string();
        let env: std::collections::BTreeMap<String, pt_vex::Ty> = input.point.iter().filter_map(|(k, c)| match c {
            pt_core::Column::F32(_) => Some((k.clone(), pt_vex::Ty::Float)),
            pt_core::Column::V3(_) => Some((k.clone(), pt_vex::Ty::Vec3)),
            _ => None,
        }).collect();
        let prog = pt_vex::analyze(&code, &env).map_err(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))?;
        if prog.chans.len() > pt_vex::MAX_GRAD {
            return Err(format!("{} channels exceed the autodiff limit of {}", prog.chans.len(), pt_vex::MAX_GRAD));
        }
        let chans_obj = params.get("channels").and_then(Value::as_object).cloned().unwrap_or_default();
        let init: Vec<f32> = prog.chans.iter().map(|(name, _)| chans_obj.get(name).and_then(Value::as_f64).unwrap_or(0.0) as f32).collect();
        let views: Vec<Option<pt_vex::AttrView>> = prog.attrs.iter().map(|a| match input.point.get(&a.name) {
            Some(pt_core::Column::F32(v)) => Some(pt_vex::AttrView::F(v.as_slice())),
            Some(pt_core::Column::V3(v)) => Some(pt_vex::AttrView::V(v.as_slice())),
            _ => None,
        }).collect();
        let frame = params.get("__frame").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let make = |ch: &[f32]| pt_vex::Inputs { n: input.n_points(), attrs: views.iter().map(|v| v.as_ref().map(|x| match x {
            pt_vex::AttrView::F(s) => pt_vex::AttrView::F(s),
            pt_vex::AttrView::V(s) => pt_vex::AttrView::V(s),
        })).collect(), chans: ch.iter().map(|&c| [c; 3]).collect(), frame };
        let fit = pt_vex::fit::fit_channels(&prog, make, attr, comp, &tvals, &init, None, steps.clamp(1, 5000), lr).ok_or_else(|| format!("the wrangle does not write '{attr}'"))?;
        let mut new_channels = chans_obj.clone();
        for ((name, _), v) in prog.chans.iter().zip(&fit.channels) {
            new_channels.insert(name.clone(), json!(v));
        }
        self.graph.set_param(node, "channels", Value::Object(new_channels.clone()))?;
        Ok(json!({"channels": new_channels, "initial_loss": fit.loss_history.first(), "final_loss": fit.loss_history.last(), "steps": steps}))
    }

    /// A graph for "current" or a named checkpoint.
    fn state_graph(&self, name: &str) -> Result<Graph, String> {
        let mut g = Graph::new();
        let snap = if name == "current" {
            self.graph.snapshot()
        } else {
            self.checkpoints.get(name).cloned().ok_or_else(|| format!("no checkpoint '{name}' (have: {:?})", self.checkpoints.keys().collect::<Vec<_>>()))?
        };
        g.restore(&snap)?;
        Ok(g)
    }

    fn push_undo(&mut self) {
        self.undo.push(self.graph.snapshot());
        self.redo.clear();
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    pub fn call(&mut self, name: &str, a: &Value) -> Result<Out, String> {
        let j = |v: Value| Ok(Out::Json(v));
        match name {
            "list_node_types" => j(json!(pt_graph::all_schemas().iter().map(|s| s.to_json()).collect::<Vec<_>>())),
            "describe_node" => {
                if let Some(kind) = a.get("kind").and_then(Value::as_str) {
                    let sc = pt_graph::schema(kind).ok_or_else(|| format!("unknown node type '{kind}'"))?;
                    j(sc.to_json())
                } else {
                    let id = s(a, "node")?;
                    let n = self.graph.nodes.get(id).ok_or_else(|| format!("no node '{id}'"))?;
                    j(json!({"node": id, "kind": n.kind, "inputs": n.inputs, "params": pt_graph::resolved_params(&self.graph, id).map(|r| match r { Ok(p) => json!(p), Err(e) => json!({"error": e}) }),
                             "schema": pt_graph::schema(&n.kind).map(|s| s.to_json())}))
                }
            }
            "search_nodes" => {
                let q = s(a, "query")?.to_lowercase();
                let words: Vec<&str> = q.split_whitespace().collect();
                let mut scored: Vec<(usize, Value)> = pt_graph::all_schemas()
                    .iter()
                    .map(|sc| {
                        let hay = format!("{} {} {}", sc.kind, sc.doc, sc.params.iter().map(|p| p.name).collect::<Vec<_>>().join(" ")).to_lowercase();
                        (words.iter().filter(|w| hay.contains(*w)).count(), json!({"kind": sc.kind, "doc": sc.doc, "inputs": sc.inputs}))
                    })
                    .filter(|(n, _)| *n > 0)
                    .collect();
                scored.sort_by_key(|x| std::cmp::Reverse(x.0));
                j(json!(scored.into_iter().map(|x| x.1).collect::<Vec<_>>()))
            }
            "create_node" | "set_param" | "connect" | "disconnect" | "delete_node" => {
                self.push_undo();
                match apply_edit(&mut self.graph, name, a) {
                    Ok(v) => j(v),
                    Err(e) => {
                        self.undo.pop();
                        Err(e)
                    }
                }
            }
            "batch" => {
                let ops = a.get("ops").and_then(Value::as_array).ok_or("missing 'ops' array")?;
                let snap = self.graph.snapshot();
                let mut results = Vec::new();
                for (i, op) in ops.iter().enumerate() {
                    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
                    match apply_edit(&mut self.graph, name, op) {
                        Ok(v) => results.push(v),
                        Err(e) => {
                            self.graph.restore(&snap)?;
                            return Err(format!("batch rolled back: op #{i} ({name}) failed: {e}"));
                        }
                    }
                }
                self.undo.push(snap);
                self.redo.clear();
                j(json!({"applied": results.len(), "results": results}))
            }
            "get_graph" => j(self.graph.describe()),
            "load_graph" => {
                self.push_undo();
                let new = Graph::from_json(s(a, "graph")?)?;
                self.graph.nodes = new.nodes;
                j(json!("ok"))
            }
            "checkpoint" => {
                self.checkpoints.insert(s(a, "name")?.to_string(), self.graph.snapshot());
                j(json!({"checkpoints": self.checkpoints.keys().collect::<Vec<_>>()}))
            }
            "restore" => {
                let snap = self.checkpoints.get(s(a, "name")?).cloned().ok_or("no such checkpoint")?;
                self.push_undo();
                self.graph.restore(&snap)?;
                j(json!("ok"))
            }
            "undo" => {
                let prev = self.undo.pop().ok_or("nothing to undo")?;
                self.redo.push(self.graph.snapshot());
                self.graph.restore(&prev)?;
                j(json!("ok"))
            }
            "redo" => {
                let next = self.redo.pop().ok_or("nothing to redo")?;
                self.undo.push(self.graph.snapshot());
                self.graph.restore(&next)?;
                j(json!("ok"))
            }
            "cook" => {
                let t = std::time::Instant::now();
                let e = self.graph.cook_entry(s(a, "node")?)?;
                let mut st = e.geo.stats();
                st["content_hash"] = pt_graph::hex(&e.hash).into();
                st["cook_ms"] = (t.elapsed().as_secs_f64() * 1000.0).into();
                st["cache"] = self.graph.stats.to_json();
                j(st)
            }
            "validate" => {
                let g = self.graph.cook(s(a, "node")?)?;
                let issues = pt_core::validate::validate(&g);
                let errors = issues.iter().filter(|i| i.severity == "error").count();
                let warnings = issues.iter().filter(|i| i.severity == "warning").count();
                j(json!({"ok": errors == 0, "errors": errors, "warnings": warnings, "issues": issues.iter().map(|i| i.to_json()).collect::<Vec<_>>()}))
            }
            "explain" => {
                let idx = a.get("index").and_then(Value::as_u64).ok_or("missing 'index'")? as usize;
                j(self.graph.explain(s(a, "node")?, s(a, "domain")?, idx)?)
            }
            "pick" => {
                // what is at this pixel of a frame, and what made it: a camera ray cast on the cooked world (no render)
                let node = s(a, "node")?;
                let scene = scene_arg(a)?;
                let size = match a.get("frame_size").and_then(Value::as_array) {
                    Some(v) if v.len() == 2 => (v[0].as_u64().unwrap_or(1920) as usize, v[1].as_u64().unwrap_or(1080) as usize),
                    _ => (1920, 1080),
                };
                let num = |k: &str| a.get(k).and_then(Value::as_f64).ok_or_else(|| format!("missing '{k}'"));
                let opts = crate::pick::PickOpts { camera: s(a, "camera")?, x: num("x")? as f32, y: num("y")? as f32, size, radius_px: a.get("radius").and_then(Value::as_f64).unwrap_or(0.0) as f32 };
                if !self.graph.provenance {
                    self.graph.provenance = true;
                }
                j(crate::pick::pick(&mut self.graph, node, &scene, &opts)?)
            }
            "render_preview" => {
                let g = self.graph.cook(s(a, "node")?)?;
                let mode = match a.get("mode").and_then(Value::as_str).unwrap_or("shaded") {
                    "shaded" => Mode::Shaded,
                    "wire" => Mode::Wire,
                    other => Mode::Attr(other.to_string()),
                };
                let mut o = Opts { mode, ..Opts::default() };
                if let Some(v) = a.get("azimuth").and_then(Value::as_f64) { o.azimuth_deg = v as f32; }
                if let Some(v) = a.get("elevation").and_then(Value::as_f64) { o.elevation_deg = v as f32; }
                if let Some(v) = a.get("width").and_then(Value::as_u64) { o.width = (v as usize).clamp(64, 2048); }
                if let Some(v) = a.get("height").and_then(Value::as_u64) { o.height = (v as usize).clamp(64, 2048); }
                let t = std::time::Instant::now();
                let (img, info) = if a.get("sheet").and_then(Value::as_bool).unwrap_or(false) { pt_render::contact_sheet(&g, &o) } else { pt_render::render(&g, &o) };
                let png = pt_render::encode_png(&img)?;
                let mut meta = json!({"width": img.w, "height": img.h, "triangles_drawn": info.triangles_drawn,
                    "instances_total": info.instances_total, "instances_drawn": info.instances_drawn,
                    "render_ms": t.elapsed().as_secs_f64() * 1000.0});
                if let Some(p) = a.get("path").and_then(Value::as_str) {
                    std::fs::write(p, &png).map_err(|e| e.to_string())?;
                    meta["saved"] = p.into();
                }
                Ok(Out::Image { png, meta })
            }
            "render_compare" => {
                let ids: Vec<&str> = a.get("nodes").and_then(Value::as_array).ok_or("missing 'nodes' array")?.iter().filter_map(Value::as_str).collect();
                if !(2..=4).contains(&ids.len()) {
                    return Err("render_compare needs 2 to 4 node ids".into());
                }
                let mut geos = Vec::new();
                for id in &ids {
                    geos.push(self.graph.cook(id)?);
                }
                let boxes: Vec<_> = geos.iter().map(|g| g.bounds()).collect();
                let union = boxes.iter().flatten().fold(None, |acc: Option<([f32; 3], [f32; 3])>, (lo, hi)| match acc {
                    None => Some((*lo, *hi)),
                    Some((l, h)) => Some(([l[0].min(lo[0]), l[1].min(lo[1]), l[2].min(lo[2])], [h[0].max(hi[0]), h[1].max(hi[1]), h[2].max(hi[2])])),
                });
                let fit = if a.get("frame").and_then(Value::as_str) == Some("union") { union } else { boxes[0] };
                let mode = match a.get("mode").and_then(Value::as_str).unwrap_or("shaded") {
                    "shaded" => Mode::Shaded,
                    "wire" => Mode::Wire,
                    other => Mode::Attr(other.to_string()),
                };
                let mut o = Opts { width: 400, height: 300, mode, fit_bounds: fit, ..Opts::default() };
                if let Some(v) = a.get("azimuth").and_then(Value::as_f64) { o.azimuth_deg = v as f32; }
                if let Some(v) = a.get("elevation").and_then(Value::as_f64) { o.elevation_deg = v as f32; }
                let imgs: Vec<_> = geos.iter().map(|g| pt_render::render(g, &o).0).collect();
                let img = pt_render::grid(&imgs, 2, 4);
                let png = pt_render::encode_png(&img)?;
                let sizes: Vec<Value> = ids.iter().zip(&boxes).map(|(id, b)| json!({"node": id, "size_m": b.map(|(lo, hi)| [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])})).collect();
                let mut meta = json!({"layout": "cells left-to-right, top-to-bottom in the order given", "shared_camera_framed_on": if a.get("frame").and_then(Value::as_str) == Some("union") { "union of all nodes" } else { "first node" }, "nodes": sizes});
                if let Some(p) = a.get("path").and_then(Value::as_str) {
                    std::fs::write(p, &png).map_err(|e| e.to_string())?;
                    meta["saved"] = p.into();
                }
                Ok(Out::Image { png, meta })
            }
            "render_scene" => {
                let node = s(a, "node")?;
                let geo = self.graph.cook(node)?;
                let scene = scene_arg(a)?;
                let out = std::path::PathBuf::from(a.get("path").and_then(Value::as_str).map_or_else(|| format!("out/render/mcp_{node}.png"), String::from));
                let opts = crate::blender::Opts { quick: a.get("quick").and_then(Value::as_bool).unwrap_or(false), samples: a.get("samples").and_then(Value::as_u64).map(|n| n as u32), keep: None, lod: a.get("lod").and_then(Value::as_bool).unwrap_or(false), only: None, lookdev: false };
                let report = crate::blender::render_geometry(&geo, &format!("mcp_{node}"), scene, &out, &opts)?;
                let first = report.pointer("/frames/0/file").and_then(Value::as_str).ok_or("Blender reported no frames")?;
                let png = std::fs::read(first).map_err(|e| format!("{first}: {e}"))?;
                Ok(Out::Image { png, meta: report })
            }
            "export" => {
                let geo = self.graph.cook(s(a, "node")?)?;
                let path = s(a, "path")?;
                let fmt = a.get("format").and_then(Value::as_str).unwrap_or(if path.ends_with(".glb") { "glb" } else if path.ends_with(".gltf") { "gltf" } else if path.ends_with(".ptgeo") { "ptgeo" } else if path.ends_with(".usda") || path.ends_with(".usd") { "usda" } else { "obj" });
                let data: Vec<u8> = match fmt {
                    "obj" => pt_core::io::to_obj(&geo).into_bytes(),
                    "gltf" => pt_core::io::to_gltf(&geo).into_bytes(),
                    "glb" => pt_core::gltf::to_glb(&geo),
                    "ptgeo" => pt_core::io::to_bytes(&geo),
                    "usda" => pt_core::io::to_usda(&geo).into_bytes(),
                    f => return Err(format!("unknown format '{f}' (use obj, gltf, glb, usda or ptgeo)")),
                };
                std::fs::write(path, data).map_err(|e| e.to_string())?;
                j(json!({"written": path, "stats": geo.stats()}))
            }
            "run_hcs" => {
                let replace = a.get("replace").and_then(Value::as_bool).unwrap_or(true);
                self.push_undo();
                match pt_hcs::run(s(a, "code")?, &mut self.graph, replace) {
                    Err(ds) => {
                        self.undo.pop();
                        Err(json!({"errors": ds.iter().map(|d| d.to_json()).collect::<Vec<_>>()}).to_string())
                    }
                    Ok(report) => {
                        let flow = self.graph.check();
                        let outputs: Vec<Value> = report
                            .outputs
                            .iter()
                            .map(|o| match self.graph.cook(o) {
                                Ok(g) => {
                                    let st = g.stats();
                                    json!({"node": o, "points": st["points"], "primitives": st["primitives"], "instances": st["instances"], "size": st["size"], "bounds_min": st["bounds_min"], "bounds_max": st["bounds_max"]})
                                }
                                Err(e) => json!({"node": o, "error": e}),
                            })
                            .collect();
                        let specs = pt_hcs::evaluate_specs(&mut self.graph, &report.specs);
                        let all_ok = specs.iter().all(|s| s.ok) && flow.diags.is_empty() && outputs.iter().all(|o| o.get("error").is_none());
                        j(json!({
                            "ok": all_ok,
                            "nodes_created": report.created.len(),
                            "outputs": outputs,
                            "flow_diagnostics": flow.diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
                            "specs": specs.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
                            "prints": report.prints,
                        }))
                    }
                }
            }
            "check_hcs" => match pt_hcs::check(s(a, "code")?, &self.graph) {
                Ok((rep, diags)) => j(json!({"ok": diags.is_empty(), "diagnostics": diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(), "nodes": rep.created.len(), "outputs": rep.outputs, "specs": rep.specs.len()})),
                Err(ds) => j(json!({"ok": false, "errors": ds.iter().map(|d| d.to_json()).collect::<Vec<_>>()})),
            },
            "graph_to_hcs" => j(json!({"code": pt_hcs::to_hcs(&self.graph)})),
            "check_graph" => {
                let r = self.graph.check();
                j(json!({
                    "ok": r.diags.is_empty(),
                    "diagnostics": r.diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
                    "attributes": r.envs.iter().map(|(k, v)| (k.clone(), v.as_ref().map(|e| e.to_json()))).collect::<std::collections::BTreeMap<_, _>>(),
                }))
            }
            "make_asset" => {
                let inputs: Vec<String> = a.get("inputs").and_then(Value::as_array).map(|l| l.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                let promote: Vec<(String, String, String)> = a
                    .get("promote")
                    .and_then(Value::as_array)
                    .map(|l| l.iter().filter_map(|p| Some((p.get("name")?.as_str()?.to_string(), p.get("node")?.as_str()?.to_string(), p.get("param")?.as_str()?.to_string()))).collect())
                    .unwrap_or_default();
                let asset = self.graph.make_asset(s(a, "name")?, a.get("doc").and_then(Value::as_str).unwrap_or(""), s(a, "output")?, &inputs, &promote)?;
                self.push_undo();
                let info = json!({"asset": asset.name, "nodes": asset.nodes.len(), "inputs": asset.num_inputs, "promoted": asset.promoted});
                self.graph.add_asset(asset);
                j(info)
            }
            "list_assets" => j(json!(self.graph.assets.values().map(|a| json!({"name": a.name, "doc": a.doc, "inputs": a.num_inputs, "promoted": a.promoted})).collect::<Vec<_>>())),
            "set_frame" => {
                self.push_undo();
                self.graph.frame = a.get("frame").and_then(Value::as_f64).ok_or("missing number 'frame'")? as f32;
                j(json!({"frame": self.graph.frame}))
            }
            "diff" => {
                let (ga, gb) = (self.state_graph(s(a, "a")?)?, self.state_graph(s(a, "b")?)?);
                let changes = pt_graph::merge::diff(&ga, &gb);
                j(json!({"changes": changes.len(), "list": changes.iter().map(|c| c.to_json()).collect::<Vec<_>>()}))
            }
            "merge" => {
                let (base, theirs) = (self.state_graph(s(a, "base")?)?, self.state_graph(s(a, "theirs")?)?);
                let ours = self.state_graph("current")?;
                let m = pt_graph::merge::merge3(&base, &ours, &theirs);
                let force = a.get("force").and_then(Value::as_bool).unwrap_or(false);
                if !m.conflicts.is_empty() && !force {
                    j(json!({"applied": false, "conflicts": m.conflicts, "hint": "resolve by editing, or pass force=true to keep ours on conflicts"}))
                } else {
                    self.push_undo();
                    self.graph.nodes = m.graph.nodes;
                    self.graph.assets = m.graph.assets;
                    self.graph.frame = m.graph.frame;
                    j(json!({"applied": true, "conflicts": m.conflicts, "nodes": self.graph.nodes.len()}))
                }
            }
            "optimize" => {
                let node = s(a, "node")?.to_string();
                let space = parse_space(a)?;
                let obj = self.parse_objective(a.get("objective").ok_or("missing 'objective'")?, &node)?;
                let opt = pt_opt::OptOptions {
                    budget: a.get("budget").and_then(Value::as_u64).unwrap_or(600).clamp(20, 20000) as usize,
                    seed: a.get("seed").and_then(Value::as_i64).unwrap_or(1),
                    restarts: a.get("restarts").and_then(Value::as_u64).unwrap_or(3).min(20) as usize,
                    sigma0: 0.3,
                };
                self.push_undo();
                match pt_opt::optimize(&mut self.graph, &space, &obj, &node, &opt) {
                    Ok(rep) => j(rep.to_json()),
                    Err(e) => {
                        self.undo.pop();
                        Err(e)
                    }
                }
            }
            "vary" => {
                let node = s(a, "node")?.to_string();
                let space = parse_space(a)?;
                let count = a.get("count").and_then(Value::as_u64).unwrap_or(9).clamp(1, 64) as usize;
                let mode = match a.get("mode").and_then(Value::as_str).unwrap_or("shaded") {
                    "shaded" => Mode::Shaded,
                    "wire" => Mode::Wire,
                    o => Mode::Attr(o.to_string()),
                };
                let v = pt_opt::vary(&mut self.graph, &space, &node, count, a.get("seed").and_then(Value::as_i64).unwrap_or(1), (240, 180), mode, None)?;
                let png = pt_render::encode_png(&v.sheet)?;
                let variants: Vec<Value> = v.values.iter().enumerate().map(|(i, x)| json!({"index": i, "params": space.iter().map(|p| p.label()).zip(x.iter().copied()).collect::<std::collections::BTreeMap<_, _>>()})).collect();
                let mut meta = json!({"variants": variants, "columns": (v.values.len() as f64).sqrt().ceil() as usize, "note": "variants are laid out row-major; the scene was left unchanged"});
                if let Some(p) = a.get("path").and_then(Value::as_str) {
                    std::fs::write(p, &png).map_err(|e| e.to_string())?;
                    meta["saved"] = p.into();
                }
                Ok(Out::Image { png, meta })
            }
            "fit_wrangle" => {
                let node = s(a, "node")?.to_string();
                self.push_undo();
                match self.fit_wrangle(&node, s(a, "attr")?, a.get("component").and_then(Value::as_u64).unwrap_or(1) as usize, s(a, "target_node")?, a.get("steps").and_then(Value::as_u64).unwrap_or(300) as usize, a.get("lr").and_then(Value::as_f64).unwrap_or(0.03) as f32) {
                    Ok(v) => j(v),
                    Err(e) => {
                        self.undo.pop();
                        Err(e)
                    }
                }
            }
            "render_final" => {
                let geo = self.graph.cook(s(a, "node")?)?;
                let mut o = pt_render::pathtrace::PtOpts::default();
                let num = |k: &str| a.get(k).and_then(Value::as_f64);
                if let Some(v) = num("width") { o.width = (v as usize).clamp(32, 2048); }
                if let Some(v) = num("height") { o.height = (v as usize).clamp(32, 2048); }
                if let Some(v) = num("spp") { o.spp = (v as usize).clamp(1, 1024); }
                if let Some(v) = num("azimuth") { o.azimuth_deg = v as f32; }
                if let Some(v) = num("elevation") { o.elevation_deg = v as f32; }
                if let Some(v) = num("sun_intensity") { o.sun_intensity = v as f32; }
                if let Some(v) = num("roughness") { o.roughness = (v as f32).clamp(0.0, 1.0); }
                if let Some(b) = a.get("ground").and_then(Value::as_bool) { o.ground = b; }
                if num("sun_azimuth").is_some() || num("sun_elevation").is_some() {
                    let (az, el) = (num("sun_azimuth").unwrap_or(-35.0) as f32, num("sun_elevation").unwrap_or(45.0) as f32);
                    let (sa, ca) = pt_core::math::sin_cos(pt_core::math::to_radians(az));
                    let (se, ce) = pt_core::math::sin_cos(pt_core::math::to_radians(el));
                    o.sun_dir = [-(ce * sa), se, -(ce * ca)];
                }
                if let Some(m) = a.get("mode").and_then(Value::as_str) {
                    o.mode = match m { "shaded" => Mode::Shaded, other => Mode::Attr(other.to_string()) };
                }
                let t = std::time::Instant::now();
                let turntable = a.get("turntable").and_then(Value::as_u64).unwrap_or(1) as usize;
                let (img, tris, rays) = if turntable > 1 {
                    (pt_render::pathtrace::turntable_sheet(&geo, &o, turntable), 0, 0)
                } else {
                    let (img, st) = pt_render::pathtrace::pathtrace(&geo, &o);
                    (img, st.triangles, st.rays)
                };
                let png = pt_render::encode_png(&img)?;
                let mut meta = json!({"width": img.w, "height": img.h, "spp": o.spp, "triangles": tris, "rays": rays, "render_ms": t.elapsed().as_secs_f64() * 1000.0});
                if let Some(p) = a.get("path").and_then(Value::as_str) {
                    std::fs::write(p, &png).map_err(|e| e.to_string())?;
                    meta["saved"] = p.into();
                }
                Ok(Out::Image { png, meta })
            }
            "critique" => {
                let geo = self.graph.cook(s(a, "node")?)?;
                let mut o = Opts { width: 480, height: 360, ..Opts::default() };
                if let Some(v) = a.get("azimuth").and_then(Value::as_f64) { o.azimuth_deg = v as f32; }
                if let Some(v) = a.get("elevation").and_then(Value::as_f64) { o.elevation_deg = v as f32; }
                let reference = match a.get("reference").and_then(Value::as_str) {
                    Some(rp) => Some(pt_render::decode_png(&std::fs::read(rp).map_err(|e| format!("{rp}: {e}"))?)?),
                    None => None,
                };
                j(crate::critique::critique_json(&geo, &o, reference.as_ref()))
            }
            "list_recipes" => j(json!(RECIPES.iter().map(|(n, src)| json!({"name": n, "summary": src.lines().next().unwrap_or("").trim_start_matches("# ")})).collect::<Vec<_>>())),
            "get_recipe" => {
                let want = s(a, "name")?;
                RECIPES.iter().find(|(n, _)| n.contains(want)).map(|(n, src)| Out::Json(json!({"name": n, "code": src}))).ok_or_else(|| format!("no recipe matching '{want}' (try list_recipes)"))
            }
            "propose_recipe" => {
                let r = crate::recipes::propose(s(a, "name")?, s(a, "code")?)?;
                j(json!({"accepted_for_review": r.failures.is_empty(), "specs": format!("{}/{}", r.specs_ok, r.specs_total), "failures": r.failures, "hash": r.hash}))
            }
            "get_guide" => j(json!({"guide": concat!(include_str!("../../../docs/AGENT_GUIDE.md"), "\n\n", include_str!("../../../docs/WORLD_GUIDE.md"))})),
            "set_option" => {
                if let Some(p) = a.get("provenance").and_then(Value::as_bool) {
                    self.graph.provenance = p;
                }
                if let Some(d) = a.get("cache_dir").and_then(Value::as_str) {
                    self.graph.set_cache_dir(d)?;
                }
                j(json!({"provenance": self.graph.provenance}))
            }
            _ => Err(format!("unknown tool '{name}'")),
        }
    }
}

pub fn handle(session: &mut Session, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned()?; // notifications need no reply
    let method = req["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "proteus", "version": "0.1.0"},
            "instructions": "Procedural 3D node graph (meters, +Y up). Workflow: list_node_types/search_nodes -> batch (create/connect/set) -> validate -> render_preview (look at it!) -> fix -> export. Instances are lazy; use `realize` only when you need real geometry."
        }),
        "tools/list" => json!({"tools": tool_defs()}),
        "tools/call" => {
            let p = &req["params"];
            match session.call(p["name"].as_str().unwrap_or(""), &p["arguments"]) {
                Ok(Out::Json(v)) => json!({"content": [{"type": "text", "text": v.to_string()}]}),
                Ok(Out::Image { png, meta }) => json!({"content": [
                    {"type": "image", "data": pt_core::io::base64_encode(&png), "mimeType": "image/png"},
                    {"type": "text", "text": meta.to_string()}]}),
                Err(e) => json!({"isError": true, "content": [{"type": "text", "text": e}]}),
            }
        }
        "ping" => json!({}),
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

pub fn serve() {
    serve_with_ui(None);
}

/// Run the stdio MCP server; with `ui_addr` also serve the local web UI on the SAME session, so a
/// human watching the page and the agent editing over MCP share one live scene.
pub fn serve_with_ui(ui_addr: Option<String>) {
    let session = std::sync::Arc::new(std::sync::Mutex::new(Session::new()));
    if let Some(addr) = ui_addr {
        match crate::ui::serve_ui(session.clone(), &addr) {
            Ok(_) => eprintln!("proteus UI: http://{addr}/ (shared with the connected agent)"),
            Err(e) => eprintln!("proteus UI disabled: {e}"),
        }
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let line = line.trim_start_matches('\u{feff}');
        let Ok(req) = serde_json::from_str::<Value>(line) else { continue };
        let resp = handle(&mut session.lock().unwrap(), &req);
        if let Some(resp) = resp {
            let _ = writeln!(out, "{resp}");
            let _ = out.flush();
        }
    }
}

/// Standalone UI (no agent attached): blocks until the process is stopped.
pub fn serve_ui_only(addr: &str) -> Result<(), String> {
    let session = std::sync::Arc::new(std::sync::Mutex::new(Session::new()));
    let h = crate::ui::serve_ui(session, addr)?;
    eprintln!("proteus UI: http://{addr}/  (Ctrl-C to stop)");
    h.join().map_err(|_| "UI thread panicked".to_string())
}

/// Built-in example recipes (compiled into the binary so agents can read them anywhere).
const RECIPES: &[(&str, &str)] = &[
    ("01_fence", include_str!("../../../recipes/01_fence.hcs")),
    ("02_spiral_staircase", include_str!("../../../recipes/02_spiral_staircase.hcs")),
    ("03_rock_generator", include_str!("../../../recipes/03_rock_generator.hcs")),
    ("04_tree", include_str!("../../../recipes/04_tree.hcs")),
    ("05_building", include_str!("../../../recipes/05_building.hcs")),
    ("06_pipe_network", include_str!("../../../recipes/06_pipe_network.hcs")),
    ("07_terrain_valley", include_str!("../../../recipes/07_terrain_valley.hcs")),
    ("08_crystal_cluster", include_str!("../../../recipes/08_crystal_cluster.hcs")),
    ("09_colonnade", include_str!("../../../recipes/09_colonnade.hcs")),
    ("10_scifi_panel", include_str!("../../../recipes/10_scifi_panel.hcs")),
    ("11_eroded_mountain", include_str!("../../../recipes/11_eroded_mountain.hcs")),
    ("12_mechanical_bracket", include_str!("../../../recipes/12_mechanical_bracket.hcs")),
    ("13_organic_blob", include_str!("../../../recipes/13_organic_blob.hcs")),
    ("14_particle_fountain", include_str!("../../../recipes/14_particle_fountain.hcs")),
    ("15_flag_in_wind", include_str!("../../../recipes/15_flag_in_wind.hcs")),
    ("16_smoke_plume", include_str!("../../../recipes/16_smoke_plume.hcs")),
    ("17_shattering_wall", include_str!("../../../recipes/17_shattering_wall.hcs")),
    ("18_bolt", include_str!("../../../recipes/18_bolt.hcs")),
    ("19_canyon", include_str!("../../../recipes/19_canyon.hcs")),
    ("20_forest_clearing", include_str!("../../../recipes/20_forest_clearing.hcs")),
    ("21_city_block", include_str!("../../../recipes/21_city_block.hcs")),
    ("22_lathe_vase", include_str!("../../../recipes/22_lathe_vase.hcs")),
    ("23_spring_and_cable", include_str!("../../../recipes/23_spring_and_cable.hcs")),
    ("24_hillside_village", include_str!("../../../recipes/24_hillside_village.hcs")),
    ("25_carved_boulder", include_str!("../../../recipes/25_carved_boulder.hcs")),
    ("26_clearing_planted_by_asking", include_str!("../../../recipes/26_clearing_planted_by_asking.hcs")),
    ("27_woodpile_and_stones", include_str!("../../../recipes/27_woodpile_and_stones.hcs")),
    ("28_stable_orchard", include_str!("../../../recipes/28_stable_orchard.hcs")),
];

/// The `scene` argument of a tool: an object, or the path of a scene `.json` file (none = an empty scene).
fn scene_arg(a: &Value) -> Result<Value, String> {
    match a.get("scene") {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::String(p)) => serde_json::from_str(std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}")),
        Some(v) if v.is_object() => Ok(v.clone()),
        Some(_) => Err("scene must be an object or a path to a .json file".into()),
    }
}

fn parse_space(a: &Value) -> Result<Vec<pt_opt::ParamRef>, String> {
    let list = a.get("params").and_then(Value::as_array).ok_or("missing 'params' array")?;
    list.iter().map(pt_opt::ParamRef::from_json).collect()
}

#[cfg(test)]
mod recipe_list_tests {
    #[test]
    fn compiled_recipe_list_matches_recipes_dir() {
        let on_disk: Vec<String> = crate::recipes::list().iter().map(|p| p.file_stem().unwrap().to_string_lossy().to_string()).collect();
        let compiled: Vec<String> = super::RECIPES.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(on_disk, compiled, "update RECIPES in mcp.rs when adding or accepting a recipe");
    }
}
