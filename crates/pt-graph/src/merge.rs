//! Structural diff and 3-way merge of graphs (scene version control for agents).
//! Graphs are data, so merges are meaningful: changes to different parameters of the same
//! node, or to different nodes, combine cleanly; only genuine disagreements conflict.

use crate::{Graph, Node};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    AddNode(String),
    RemoveNode(String),
    Kind { node: String, from: String, to: String },
    Param { node: String, name: String, from: Option<Value>, to: Option<Value> },
    Input { node: String, slot: usize, from: Option<String>, to: Option<String> },
    Asset { name: String, change: &'static str },
    Frame { from: f32, to: f32 },
}

impl Change {
    pub fn to_json(&self) -> Value {
        match self {
            Change::AddNode(n) => json!({"change": "add_node", "node": n}),
            Change::RemoveNode(n) => json!({"change": "remove_node", "node": n}),
            Change::Kind { node, from, to } => json!({"change": "kind", "node": node, "from": from, "to": to}),
            Change::Param { node, name, from, to } => json!({"change": "param", "node": node, "param": name, "from": from, "to": to}),
            Change::Input { node, slot, from, to } => json!({"change": "input", "node": node, "slot": slot, "from": from, "to": to}),
            Change::Asset { name, change } => json!({"change": "asset", "asset": name, "what": change}),
            Change::Frame { from, to } => json!({"change": "frame", "from": from, "to": to}),
        }
    }
}

pub fn diff(a: &Graph, b: &Graph) -> Vec<Change> {
    let mut out = Vec::new();
    let ids: BTreeSet<&String> = a.nodes.keys().chain(b.nodes.keys()).collect();
    for id in ids {
        match (a.nodes.get(id), b.nodes.get(id)) {
            (None, Some(_)) => out.push(Change::AddNode(id.clone())),
            (Some(_), None) => out.push(Change::RemoveNode(id.clone())),
            (Some(x), Some(y)) => {
                if x.kind != y.kind {
                    out.push(Change::Kind { node: id.clone(), from: x.kind.clone(), to: y.kind.clone() });
                }
                let names: BTreeSet<&String> = x.params.keys().chain(y.params.keys()).collect();
                for n in names {
                    let (p, q) = (x.params.get(n), y.params.get(n));
                    if p != q {
                        out.push(Change::Param { node: id.clone(), name: n.clone(), from: p.cloned(), to: q.cloned() });
                    }
                }
                for slot in 0..x.inputs.len().max(y.inputs.len()) {
                    let (p, q) = (x.inputs.get(slot).cloned().flatten(), y.inputs.get(slot).cloned().flatten());
                    if p != q {
                        out.push(Change::Input { node: id.clone(), slot, from: p, to: q });
                    }
                }
            }
            (None, None) => {}
        }
    }
    let names: BTreeSet<&String> = a.assets.keys().chain(b.assets.keys()).collect();
    for n in names {
        match (a.assets.get(n), b.assets.get(n)) {
            (None, Some(_)) => out.push(Change::Asset { name: n.clone(), change: "added" }),
            (Some(_), None) => out.push(Change::Asset { name: n.clone(), change: "removed" }),
            (Some(x), Some(y)) if serde_json::to_value(x).ok() != serde_json::to_value(y).ok() => out.push(Change::Asset { name: n.clone(), change: "modified" }),
            _ => {}
        }
    }
    if a.frame != b.frame {
        out.push(Change::Frame { from: a.frame, to: b.frame });
    }
    out
}

pub struct MergeResult {
    pub graph: Graph,
    pub conflicts: Vec<String>,
}

/// Three-way pick: returns (value, conflicted). On conflict `ours` wins.
fn pick<T: Clone + PartialEq>(base: &T, ours: &T, theirs: &T) -> (T, bool) {
    if ours == theirs || theirs == base {
        (ours.clone(), false)
    } else if ours == base {
        (theirs.clone(), false)
    } else {
        (ours.clone(), true)
    }
}

pub fn merge3(base: &Graph, ours: &Graph, theirs: &Graph) -> MergeResult {
    let mut g = Graph::new();
    g.assets = ours.assets.clone();
    let mut conflicts = Vec::new();
    let ids: BTreeSet<&String> = base.nodes.keys().chain(ours.nodes.keys()).chain(theirs.nodes.keys()).collect();
    for id in ids {
        let (b, o, t) = (base.nodes.get(id), ours.nodes.get(id), theirs.nodes.get(id));
        let eq = |x: &Option<&Node>, y: &Option<&Node>| serde_json::to_value(x).ok() == serde_json::to_value(y).ok();
        let chosen: Option<Node> = if eq(&o, &t) || eq(&t, &b) {
            o.cloned()
        } else if eq(&o, &b) {
            t.cloned()
        } else if let (Some(bn), Some(on), Some(tn)) = (b, o, t) {
            // Both sides edited the node: merge kind, params and input slots separately.
            let mut n = on.clone();
            let (kind, c) = pick(&bn.kind, &on.kind, &tn.kind);
            n.kind = kind;
            if c {
                conflicts.push(format!("node '{id}': kind changed differently ({} vs {})", on.kind, tn.kind));
            }
            let names: BTreeSet<&String> = bn.params.keys().chain(on.params.keys()).chain(tn.params.keys()).collect();
            for name in names {
                let (v, c) = pick(&bn.params.get(name).cloned(), &on.params.get(name).cloned(), &tn.params.get(name).cloned());
                if c {
                    conflicts.push(format!(
                        "node '{id}' parameter '{name}': changed to {} on one side and {} on the other",
                        on.params.get(name).map_or("<default>".into(), |x| x.to_string()),
                        tn.params.get(name).map_or("<default>".into(), |x| x.to_string())
                    ));
                }
                match v {
                    Some(v) => {
                        n.params.insert(name.clone(), v);
                    }
                    None => {
                        n.params.remove(name);
                    }
                }
            }
            for slot in 0..on.inputs.len().max(tn.inputs.len()).max(bn.inputs.len()) {
                let get = |x: &Node| x.inputs.get(slot).cloned().flatten();
                let (v, c) = pick(&get(bn), &get(on), &get(tn));
                if c {
                    conflicts.push(format!("node '{id}' input {slot}: wired to {:?} on one side and {:?} on the other", get(on), get(tn)));
                }
                if slot < n.inputs.len() {
                    n.inputs[slot] = v;
                }
            }
            Some(n)
        } else {
            conflicts.push(format!(
                "node '{id}': {} on one side and {} on the other",
                if o.is_some() { "edited/added" } else { "deleted" },
                if t.is_some() { "edited/added" } else { "deleted" }
            ));
            o.cloned().or_else(|| t.cloned())
        };
        if let Some(n) = chosen {
            g.nodes.insert(id.clone(), n);
        }
    }
    // Assets: same three-way rule by name.
    let names: BTreeSet<&String> = base.assets.keys().chain(ours.assets.keys()).chain(theirs.assets.keys()).collect();
    for name in names {
        let j = |x: Option<&crate::Asset>| serde_json::to_value(x).ok();
        let (b, o, t) = (j(base.assets.get(name)), j(ours.assets.get(name)), j(theirs.assets.get(name)));
        if o != t && t != b && o != b {
            conflicts.push(format!("asset '{name}' changed on both sides"));
        } else if o == b && t != b {
            match theirs.assets.get(name) {
                Some(a) => {
                    g.assets.insert(name.clone(), a.clone());
                }
                None => {
                    g.assets.remove(name);
                }
            }
        }
    }
    let (frame, c) = pick(&base.frame.to_bits(), &ours.frame.to_bits(), &theirs.frame.to_bits());
    g.frame = f32::from_bits(frame);
    if c {
        conflicts.push("frame changed on both sides".into());
    }
    // Drop wires to nodes that no longer exist.
    let existing: BTreeSet<String> = g.nodes.keys().cloned().collect();
    for (id, n) in g.nodes.iter_mut() {
        for (slot, i) in n.inputs.iter_mut().enumerate() {
            if let Some(up) = i {
                if !existing.contains(up) {
                    conflicts.push(format!("node '{id}' input {slot} pointed at deleted node '{up}'; disconnected"));
                    *i = None;
                }
            }
        }
    }
    MergeResult { graph: g, conflicts }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Graph {
        let mut g = Graph::new();
        g.create("grid", Some("g")).unwrap();
        g.create("noise", Some("n")).unwrap();
        g.connect("g", "n", 0).unwrap();
        g
    }

    #[test]
    fn diff_lists_structural_changes() {
        let a = base();
        let mut b = base();
        b.set_param("n", "amp", json!(2.0)).unwrap();
        b.create("box", Some("b")).unwrap();
        let d = diff(&a, &b);
        assert!(d.contains(&Change::AddNode("b".into())));
        assert!(d.iter().any(|c| matches!(c, Change::Param { node, name, .. } if node == "n" && name == "amp")));
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn different_params_and_new_nodes_merge_cleanly() {
        let b = base();
        let mut ours = base();
        ours.set_param("n", "amp", json!(2.0)).unwrap();
        ours.create("box", Some("from_ours")).unwrap();
        let mut theirs = base();
        theirs.set_param("n", "freq", json!(0.5)).unwrap();
        theirs.set_param("g", "rows", json!(50)).unwrap();
        theirs.create("sphere", Some("from_theirs")).unwrap();
        let m = merge3(&b, &ours, &theirs);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        let n = &m.graph.nodes["n"];
        assert_eq!(n.params["amp"], json!(2.0));
        assert_eq!(n.params["freq"], json!(0.5));
        assert_eq!(m.graph.nodes["g"].params["rows"], json!(50));
        assert!(m.graph.nodes.contains_key("from_ours") && m.graph.nodes.contains_key("from_theirs"));
    }

    #[test]
    fn same_param_different_values_conflicts_and_ours_wins() {
        let b = base();
        let mut ours = base();
        ours.set_param("n", "amp", json!(2.0)).unwrap();
        let mut theirs = base();
        theirs.set_param("n", "amp", json!(9.0)).unwrap();
        let m = merge3(&b, &ours, &theirs);
        assert_eq!(m.conflicts.len(), 1);
        assert!(m.conflicts[0].contains("amp"));
        assert_eq!(m.graph.nodes["n"].params["amp"], json!(2.0));
        // Both made the identical change: no conflict.
        theirs.set_param("n", "amp", json!(2.0)).unwrap();
        assert!(merge3(&b, &ours, &theirs).conflicts.is_empty());
    }

    #[test]
    fn delete_vs_edit_conflicts_and_wires_are_repaired() {
        let b = base();
        let mut ours = base();
        ours.delete("g").unwrap();
        let mut theirs = base();
        theirs.set_param("g", "rows", json!(7)).unwrap();
        let m = merge3(&b, &ours, &theirs);
        assert!(m.conflicts.iter().any(|c| c.contains("'g'")));
        // Clean delete on one side: no conflict.
        let m = merge3(&b, &ours, &base());
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        assert!(!m.graph.nodes.contains_key("g"));
        // The other side adds a node wired to the deleted one: the wire is repaired and reported.
        let mut theirs = base();
        theirs.create("noise", Some("extra")).unwrap();
        theirs.connect("g", "extra", 0).unwrap();
        let m = merge3(&b, &ours, &theirs);
        assert!(m.conflicts.iter().any(|c| c.contains("disconnected")), "{:?}", m.conflicts);
        assert!(m.graph.nodes.contains_key("extra") && m.graph.nodes["extra"].inputs[0].is_none());
    }
}
