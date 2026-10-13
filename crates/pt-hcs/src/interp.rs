//! Evaluator: runs an hcs program, building nodes in a Graph as it goes.

use crate::parse::{Arg, Ek, Pos, Stmt, E};
use crate::{Diag, RunReport, SpecDef};
use pt_core::math;
use pt_graph::Graph;
use pt_vex::suggest;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum V {
    Num(f64),
    Str(String),
    Bool(bool),
    None,
    List(Vec<V>),
    Dict(Vec<(String, V)>),
    Node(String),
    Func(Rc<FuncDef>),
}

#[derive(Debug)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<(String, Option<V>)>,
    pub body: Vec<Stmt>,
}

type R<T> = Result<T, Diag>;

enum Flow {
    Normal,
    Return(V),
}

const MAX_STEPS: u64 = 5_000_000;
const MAX_NODES: usize = 200_000;
const BUILTINS: &[&str] = &["frame", "range", "len", "min", "max", "abs", "sum", "round", "int", "float", "sin", "cos", "sqrt", "floor", "ceil", "radians", "degrees", "lerp", "out", "spec", "print", "asset", "str", "pow", "lookdev", "edits_of", "knob"];

pub struct Interp<'g> {
    pub g: &'g mut Graph,
    globals: HashMap<String, V>,
    locals: Vec<HashMap<String, V>>,
    steps: u64,
    depth: usize,
    pub report: RunReport,
    hint: Option<String>,
}

fn d(pos: Pos, msg: impl Into<String>, fix: impl Into<String>) -> Diag {
    Diag::new(pos.line, pos.col, msg.into(), fix)
}

fn type_name(v: &V) -> &'static str {
    match v {
        V::Num(_) => "number",
        V::Str(_) => "string",
        V::Bool(_) => "bool",
        V::None => "None",
        V::List(_) => "list",
        V::Dict(_) => "dict",
        V::Node(_) => "node",
        V::Func(_) => "function",
    }
}

fn truthy(v: &V) -> bool {
    match v {
        V::Num(n) => *n != 0.0,
        V::Bool(b) => *b,
        V::None => false,
        V::Str(s) => !s.is_empty(),
        V::List(l) => !l.is_empty(),
        V::Dict(l) => !l.is_empty(),
        _ => true,
    }
}

/// A JSON value as a language value (lists, dicts, numbers, strings, booleans; null becomes None).
fn from_json(v: &Value) -> V {
    match v {
        Value::Null => V::None,
        Value::Bool(b) => V::Bool(*b),
        Value::Number(n) => V::Num(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => V::Str(s.clone()),
        Value::Array(a) => V::List(a.iter().map(from_json).collect()),
        Value::Object(o) => V::Dict(o.iter().map(|(k, x)| (k.clone(), from_json(x))).collect()),
    }
}

/// The edits file of a world: `{"scope": [ {id, op, ...}, ... ], ...}`. Looked for as given, then under `PT_ROOT`, then in the
/// repository this program was built from. A missing file or scope is an empty list (nothing decided yet).
fn read_edits(path: &str, scope: &str) -> V {
    let candidates = [std::path::PathBuf::from(path), std::env::var("PT_ROOT").map(|r| std::path::Path::new(&r).join(path)).unwrap_or_default(), std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)];
    for c in candidates.iter().filter(|c| c.is_file()) {
        if let Some(v) = std::fs::read_to_string(c).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok()) {
            return v.get(scope).map_or(V::List(vec![]), from_json);
        }
    }
    V::List(vec![])
}

pub fn to_json(v: &V, pos: Pos) -> R<Value> {
    Ok(match v {
        V::Num(n) => json!(n),
        V::Str(s) => json!(s),
        V::Bool(b) => json!(b),
        V::List(l) => Value::Array(l.iter().map(|x| to_json(x, pos)).collect::<R<_>>()?),
        V::Dict(l) => Value::Object(l.iter().map(|(k, x)| Ok((k.clone(), to_json(x, pos)?))).collect::<R<_>>()?),
        other => return Err(d(pos, format!("a {} cannot be used as a parameter value", type_name(other)), "pass nodes as positional inputs, e.g. noise(grid_node, amp=1.0)")),
    })
}

impl<'g> Interp<'g> {
    pub fn new(g: &'g mut Graph) -> Self {
        Interp { g, globals: HashMap::new(), locals: vec![], steps: 0, depth: 0, report: RunReport::default(), hint: None }
    }

    pub fn run(&mut self, prog: &[Stmt]) -> R<()> {
        for s in prog {
            if let Flow::Return(_) = self.stmt(s)? {
                break;
            }
        }
        Ok(())
    }

    fn tick(&mut self, pos: Pos) -> R<()> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err(d(pos, "program exceeded the step limit (5,000,000)", "reduce loop sizes; hcs programs must stay small and bounded"));
        }
        Ok(())
    }

    fn set_var(&mut self, name: &str, v: V) {
        match self.locals.last_mut() {
            Some(l) => l.insert(name.to_string(), v),
            None => self.globals.insert(name.to_string(), v),
        };
    }

    fn get_var(&self, name: &str) -> Option<V> {
        if let Some(l) = self.locals.last() {
            if let Some(v) = l.get(name) {
                return Some(v.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    fn var_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.globals.keys().cloned().collect();
        if let Some(l) = self.locals.last() {
            v.extend(l.keys().cloned());
        }
        v
    }

    fn block(&mut self, b: &[Stmt]) -> R<Flow> {
        for s in b {
            if let Flow::Return(v) = self.stmt(s)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, s: &Stmt) -> R<Flow> {
        match s {
            Stmt::Pass => {}
            Stmt::Expr(e) => {
                self.tick(e.pos)?;
                self.eval(e)?;
            }
            Stmt::Assign(names, e, pos) => {
                self.tick(*pos)?;
                if names.len() == 1 {
                    self.hint = Some(names[0].clone());
                    let v = self.eval(e);
                    self.hint = None;
                    self.set_var(&names[0], v?);
                } else {
                    let v = self.eval(e)?;
                    let items = match v {
                        V::List(l) => l,
                        o => return Err(d(*pos, format!("cannot unpack a {} into {} names", type_name(&o), names.len()), "assign a list or tuple of the same length")),
                    };
                    if items.len() != names.len() {
                        return Err(d(*pos, format!("cannot unpack {} value(s) into {} names", items.len(), names.len()), "make the counts match"));
                    }
                    for (n, v) in names.iter().zip(items) {
                        self.set_var(n, v);
                    }
                }
            }
            Stmt::AugAssign(name, op, e, pos) => {
                self.tick(*pos)?;
                let cur = self.get_var(name).ok_or_else(|| d(*pos, format!("unknown variable '{name}'"), "assign it before using +="))?;
                let rhs = self.eval(e)?;
                let o = &op[..1];
                let v = self.binop(match o { "+" => "+", "-" => "-", "*" => "*", _ => "/" }, cur, rhs, *pos)?;
                self.set_var(name, v);
            }
            Stmt::If(arms, els) => {
                for (c, body) in arms {
                    if truthy(&self.eval(c)?) {
                        return self.block(body);
                    }
                }
                return self.block(els);
            }
            Stmt::For(var, it, body, pos) => {
                let items = match self.eval(it)? {
                    V::List(l) => l,
                    o => return Err(d(*pos, format!("cannot loop over a {}", type_name(&o)), "loop over range(n) or a list")),
                };
                for item in items {
                    self.tick(*pos)?;
                    self.set_var(var, item);
                    if let Flow::Return(v) = self.block(body)? {
                        return Ok(Flow::Return(v));
                    }
                }
            }
            Stmt::Def(name, params, body, pos) => {
                if BUILTINS.contains(&name.as_str()) || pt_graph::schema(name).is_some() {
                    return Err(d(*pos, format!("'{name}' is a built-in name and cannot be redefined"), "pick another function name"));
                }
                let mut ps = Vec::new();
                for (pn, def) in params {
                    ps.push((pn.clone(), match def { Some(e) => Some(self.eval(e)?), None => None }));
                }
                self.globals.insert(name.clone(), V::Func(Rc::new(FuncDef { name: name.clone(), params: ps, body: body.clone() })));
            }
            Stmt::Return(e, pos) => {
                if self.locals.is_empty() {
                    return Err(d(*pos, "'return' outside a function", "remove it or put the code in a def"));
                }
                return Ok(Flow::Return(match e { Some(e) => self.eval(e)?, None => V::None }));
            }
        }
        Ok(Flow::Normal)
    }

    fn num(&self, v: &V, pos: Pos, what: &str) -> R<f64> {
        match v {
            V::Num(n) => Ok(*n),
            V::Bool(b) => Ok(*b as i32 as f64),
            o => Err(d(pos, format!("{what} needs a number but got a {}", type_name(o)), "pass a number")),
        }
    }

    fn binop(&self, op: &str, l: V, r: V, pos: Pos) -> R<V> {
        use V::*;
        Ok(match (op, &l, &r) {
            ("+", Num(a), Num(b)) => Num(a + b),
            ("-", Num(a), Num(b)) => Num(a - b),
            ("*", Num(a), Num(b)) => Num(a * b),
            ("/", Num(a), Num(b)) => {
                if *b == 0.0 {
                    return Err(d(pos, "division by zero", "check the divisor"));
                }
                Num(a / b)
            }
            ("//", Num(a), Num(b)) => {
                if *b == 0.0 {
                    return Err(d(pos, "division by zero", "check the divisor"));
                }
                Num((a / b).floor())
            }
            ("%", Num(a), Num(b)) => {
                if *b == 0.0 {
                    return Err(d(pos, "modulo by zero", "check the divisor"));
                }
                Num(a - (a / b).floor() * b)
            }
            ("**", Num(a), Num(b)) => Num(math::pow64(*a, *b)),
            ("+", Str(a), Str(b)) => Str(format!("{a}{b}")),
            ("+", List(a), List(b)) => List(a.iter().chain(b.iter()).cloned().collect()),
            ("*", List(a), Num(n)) | ("*", Num(n), List(a)) => {
                let mut out = Vec::new();
                for _ in 0..(*n).max(0.0) as usize {
                    out.extend(a.iter().cloned());
                }
                List(out)
            }
            // Elementwise list math makes vector parameters easy: [1,2,3] * 2, a + b.
            (op @ ("+" | "-" | "*" | "/"), List(a), Num(_)) => List(a.iter().map(|x| self.binop(op, x.clone(), r.clone(), pos)).collect::<R<_>>()?),
            (op @ ("+" | "-" | "*" | "/"), Num(_), List(b)) => List(b.iter().map(|x| self.binop(op, l.clone(), x.clone(), pos)).collect::<R<_>>()?),
            (op @ ("-" | "*" | "/"), List(a), List(b)) if a.len() == b.len() => List(a.iter().zip(b).map(|(x, y)| self.binop(op, x.clone(), y.clone(), pos)).collect::<R<_>>()?),
            ("==", a, b) => Bool(equal(a, b)),
            ("!=", a, b) => Bool(!equal(a, b)),
            ("<", Num(a), Num(b)) => Bool(a < b),
            (">", Num(a), Num(b)) => Bool(a > b),
            ("<=", Num(a), Num(b)) => Bool(a <= b),
            (">=", Num(a), Num(b)) => Bool(a >= b),
            _ => return Err(d(pos, format!("cannot apply '{op}' to {} and {}", type_name(&l), type_name(&r)), "check the operand types")),
        })
    }

    pub fn eval(&mut self, e: &E) -> R<V> {
        self.tick(e.pos)?;
        Ok(match &e.k {
            Ek::Num(n, _) => V::Num(*n),
            Ek::Str(s) => V::Str(s.clone()),
            Ek::Bool(b) => V::Bool(*b),
            Ek::None => V::None,
            Ek::Name(n) => match self.get_var(n) {
                Some(v) => v,
                None => match n.as_str() {
                    "pi" => V::Num(std::f64::consts::PI),
                    "tau" => V::Num(std::f64::consts::TAU),
                    _ => {
                        let names = self.var_names();
                        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                        let hint = suggest(n, &refs).map(|s| format!("did you mean '{s}'? ")).unwrap_or_default();
                        return Err(d(e.pos, format!("unknown name '{n}'"), format!("{hint}assign it first, e.g. {n} = ...")));
                    }
                },
            },
            Ek::List(items) | Ek::Tuple(items) => V::List(items.iter().map(|x| self.eval(x)).collect::<R<_>>()?),
            Ek::Dict(items) => {
                let mut out = Vec::new();
                for (k, v) in items {
                    let key = match self.eval(k)? {
                        V::Str(s) => s,
                        o => return Err(d(k.pos, format!("dict keys must be strings, got a {}", type_name(&o)), "use \"name\": value")),
                    };
                    out.push((key, self.eval(v)?));
                }
                V::Dict(out)
            }
            Ek::Neg(x) => {
                let v = self.eval(x)?;
                self.binop("*", v, V::Num(-1.0), e.pos)?
            }
            Ek::Not(x) => V::Bool(!truthy(&self.eval(x)?)),
            Ek::Bin(op, l, r) => {
                let (a, b) = (self.eval(l)?, self.eval(r)?);
                self.binop(op, a, b, e.pos)?
            }
            Ek::And(l, r) => {
                let a = self.eval(l)?;
                if !truthy(&a) { a } else { self.eval(r)? }
            }
            Ek::Or(l, r) => {
                let a = self.eval(l)?;
                if truthy(&a) { a } else { self.eval(r)? }
            }
            Ek::IfExp(c, t, o) => {
                if truthy(&self.eval(c)?) { self.eval(t)? } else { self.eval(o)? }
            }
            Ek::Index(x, i) => {
                let (v, idx) = (self.eval(x)?, self.eval(i)?);
                match (&v, &idx) {
                    (V::List(l), V::Num(n)) => {
                        let k = if *n < 0.0 { l.len() as f64 + n } else { *n };
                        l.get(k as usize).cloned().ok_or_else(|| d(e.pos, format!("index {n} out of range (length {})", l.len()), "use an index within the list"))?
                    }
                    (V::Dict(l), V::Str(k)) => l.iter().find(|(kk, _)| kk == k).map(|x| x.1.clone()).ok_or_else(|| d(e.pos, format!("no key '{k}'"), "check the dict keys"))?,
                    _ => return Err(d(e.pos, format!("cannot index a {} with a {}", type_name(&v), type_name(&idx)), "index lists with numbers and dicts with strings")),
                }
            }
            Ek::ListComp { elem, var, iter, cond } => {
                let items = match self.eval(iter)? {
                    V::List(l) => l,
                    o => return Err(d(iter.pos, format!("cannot loop over a {}", type_name(&o)), "iterate over range(n) or a list")),
                };
                let mut out = Vec::new();
                for item in items {
                    self.tick(e.pos)?;
                    self.set_var(var, item);
                    if let Some(c) = cond {
                        if !truthy(&self.eval(c)?) {
                            continue;
                        }
                    }
                    out.push(self.eval(elem)?);
                }
                V::List(out)
            }
            Ek::Call(callee, args) => {
                let Ek::Name(name) = &callee.k else {
                    return Err(d(e.pos, "only named functions can be called", "call functions by name, e.g. grid(...)"));
                };
                self.call(name, args, e.pos)?
            }
        })
    }

    fn call(&mut self, name: &str, args: &[Arg], pos: Pos) -> R<V> {
        // 1. user functions
        if let Some(V::Func(f)) = self.get_var(name) {
            return self.call_user(&f, args, pos);
        }
        // 2. node constructors
        if let Some(sc) = pt_graph::schema(name) {
            return self.make_node(name, sc.inputs, args, pos);
        }
        // 3. builtins
        if BUILTINS.contains(&name) {
            let mut pos_args = Vec::new();
            let mut kw = Vec::new();
            for a in args {
                let v = self.eval(&a.value)?;
                match &a.name {
                    Some(n) => kw.push((n.clone(), v)),
                    None => pos_args.push(v),
                }
            }
            return self.builtin(name, pos_args, kw, pos);
        }
        let mut cands: Vec<String> = BUILTINS.iter().map(|s| s.to_string()).collect();
        cands.extend(pt_graph::all_schemas().iter().map(|s| s.kind.to_string()));
        cands.extend(self.var_names());
        let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
        let hint = suggest(name, &refs).map(|s| format!("did you mean {s}()? ")).unwrap_or_default();
        Err(d(pos, format!("unknown function '{name}'"), format!("{hint}node types: {}", pt_graph::all_schemas().iter().map(|s| s.kind).collect::<Vec<_>>().join(", "))))
    }

    fn call_user(&mut self, f: &Rc<FuncDef>, args: &[Arg], pos: Pos) -> R<V> {
        if self.depth > 40 {
            return Err(d(pos, "function calls nest too deeply", "avoid deep recursion"));
        }
        let mut frame: HashMap<String, V> = HashMap::new();
        let mut positional = Vec::new();
        for a in args {
            let v = self.eval(&a.value)?;
            match &a.name {
                Some(n) => {
                    if !f.params.iter().any(|p| &p.0 == n) {
                        let names: Vec<&str> = f.params.iter().map(|p| p.0.as_str()).collect();
                        return Err(d(a.value.pos, format!("{}() has no parameter '{n}'", f.name), format!("parameters: {names:?}")));
                    }
                    frame.insert(n.clone(), v);
                }
                None => positional.push(v),
            }
        }
        if positional.len() > f.params.len() {
            return Err(d(pos, format!("{}() takes {} argument(s) but {} were given", f.name, f.params.len(), positional.len()), "remove extra arguments"));
        }
        for (i, v) in positional.into_iter().enumerate() {
            frame.insert(f.params[i].0.clone(), v);
        }
        for (pn, def) in &f.params {
            if !frame.contains_key(pn) {
                match def {
                    Some(v) => {
                        frame.insert(pn.clone(), v.clone());
                    }
                    None => return Err(d(pos, format!("{}() missing argument '{pn}'", f.name), format!("pass {pn}=...")))
                }
            }
        }
        self.locals.push(frame);
        self.depth += 1;
        let r = self.block(&f.body);
        self.depth -= 1;
        self.locals.pop();
        Ok(match r? {
            Flow::Return(v) => v,
            Flow::Normal => V::None,
        })
    }

    fn make_node(&mut self, kind: &str, n_inputs: usize, args: &[Arg], pos: Pos) -> R<V> {
        if self.g.nodes.len() >= MAX_NODES {
            return Err(d(pos, format!("graph exceeds {MAX_NODES} nodes"), "use instances (copy_to_points/array) instead of many nodes"));
        }
        let mut inputs: Vec<(usize, String)> = Vec::new();
        let mut params: Vec<(String, V, Pos)> = Vec::new();
        let mut explicit_id: Option<String> = None;
        let mut next_slot = 0usize;
        let mut add_input = |v: V, slot: Option<usize>, p: Pos, inputs: &mut Vec<(usize, String)>| -> R<()> {
            let nodes: Vec<String> = match v {
                V::Node(n) => vec![n],
                V::List(l) => l.into_iter().map(|x| if let V::Node(n) = x { Ok(n) } else { Err(d(p, "input lists may only contain nodes", "pass nodes")) }).collect::<R<_>>()?,
                o => return Err(d(p, format!("expected a node as input but got a {}", type_name(&o)), "positional arguments are input nodes; parameters must be keyword arguments, e.g. noise(g, amp=2.0)")),
            };
            for n in nodes {
                let s = slot.unwrap_or(next_slot);
                next_slot = s + 1;
                inputs.push((s, n));
            }
            Ok(())
        };
        for a in args {
            let v = self.eval(&a.value)?;
            match a.name.as_deref() {
                None => add_input(v, None, a.value.pos, &mut inputs)?,
                Some("id") => {
                    explicit_id = match v {
                        V::Str(s) => Some(s),
                        _ => return Err(d(a.value.pos, "id must be a string", "id=\"my_node\"")),
                    }
                }
                Some(n) if n.len() == 3 && n.starts_with("in") && n.as_bytes()[2].is_ascii_digit() => {
                    add_input(v, Some((n.as_bytes()[2] - b'0') as usize), a.value.pos, &mut inputs)?
                }
                Some(n) => params.push((n.to_string(), v, a.value.pos)),
            }
        }
        if inputs.len() > n_inputs {
            return Err(d(pos, format!("'{kind}' takes {n_inputs} input node(s) but {} were given", inputs.len()), format!("remove the extra input(s); '{kind}' has {n_inputs} input slot(s)")));
        }
        let want = explicit_id.or_else(|| self.hint.clone()).unwrap_or_else(|| kind.to_string());
        let base: String = want.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
        let id = if !self.g.nodes.contains_key(&base) && !base.is_empty() { base.clone() } else { (1..).map(|i| format!("{base}_{i}")).find(|n| !self.g.nodes.contains_key(n)).unwrap_or_else(|| format!("{base}_new")) };
        self.hint = None;
        self.g.create(kind, Some(&id)).map_err(|m| d(pos, m, "check the node type"))?;
        self.report.node_lines.insert(id.clone(), pos.line);
        self.report.created.push(id.clone());
        for (name, v, p) in params {
            let j = to_json(&v, p)?;
            let alias = name.trim_end_matches('_').to_string(); // allow from_=... for `from`
            self.g.set_param(&id, &alias, j).map_err(|m| {
                let _ = self.g.delete(&id);
                d(p, m, "see describe_node for valid parameters and value shapes")
            })?;
        }
        for (slot, up) in inputs {
            self.g.connect(&up, &id, slot).map_err(|m| d(pos, m, "connect an existing node to a valid input slot"))?;
        }
        Ok(V::Node(id))
    }

    fn builtin(&mut self, name: &str, a: Vec<V>, kw: Vec<(String, V)>, pos: Pos) -> R<V> {
        let kwget = |k: &str| kw.iter().find(|x| x.0 == k).map(|x| x.1.clone());
        let n1 = |this: &Self, what: &str| -> R<f64> {
            if a.len() != 1 {
                return Err(d(pos, format!("{what}() takes 1 argument"), format!("call {what}(x)")));
            }
            this.num(&a[0], pos, what)
        };
        Ok(match name {
            "range" => {
                let nums: Vec<f64> = a.iter().map(|v| self.num(v, pos, "range")).collect::<R<_>>()?;
                let (lo, hi, st) = match nums.as_slice() {
                    [h] => (0.0, *h, 1.0),
                    [l, h] => (*l, *h, 1.0),
                    [l, h, s] => (*l, *h, *s),
                    _ => return Err(d(pos, "range() takes 1-3 arguments", "range(n) or range(a, b[, step])")),
                };
                if st == 0.0 || ((hi - lo) / st).abs() > 1_000_000.0 {
                    return Err(d(pos, "range is empty-step or larger than 1,000,000", "use a smaller range and instances for large counts"));
                }
                let mut out = Vec::new();
                let mut x = lo;
                while (st > 0.0 && x < hi) || (st < 0.0 && x > hi) {
                    out.push(V::Num(x));
                    x += st;
                }
                V::List(out)
            }
            "len" => match a.first() {
                Some(V::List(l)) => V::Num(l.len() as f64),
                Some(V::Str(s)) => V::Num(s.chars().count() as f64),
                Some(V::Dict(l)) => V::Num(l.len() as f64),
                _ => return Err(d(pos, "len() needs a list, string or dict", "len(my_list)")),
            },
            "min" | "max" => {
                let items: Vec<V> = if a.len() == 1 { if let V::List(l) = &a[0] { l.clone() } else { a.clone() } } else { a.clone() };
                let nums: Vec<f64> = items.iter().map(|v| self.num(v, pos, name)).collect::<R<_>>()?;
                if nums.is_empty() {
                    return Err(d(pos, format!("{name}() of an empty sequence"), "pass at least one number"));
                }
                V::Num(nums.into_iter().fold(if name == "min" { f64::INFINITY } else { f64::NEG_INFINITY }, |x, y| if name == "min" { x.min(y) } else { x.max(y) }))
            }
            "sum" => match a.first() {
                Some(V::List(l)) => V::Num(l.iter().map(|v| self.num(v, pos, "sum")).sum::<R<f64>>()?),
                _ => return Err(d(pos, "sum() needs a list of numbers", "sum([1, 2, 3])")),
            },
            "frame" => {
                let v = n1(self, "frame")?;
                if !(0.0..=100000.0).contains(&v) {
                    return Err(d(pos, "frame must be between 0 and 100000", "frame(24)"));
                }
                self.g.frame = v as f32;
                V::None
            }
            "knob" => {
                if a.len() != 2 {
                    return Err(d(pos, "knob() takes 2 arguments", "knob(\"tree_density\", 1.0): the value the active style gives this knob, else the default"));
                }
                let V::Str(name) = &a[0] else { return Err(d(pos, "knob() takes the knob's name as a string first", "knob(\"tree_density\", 1.0)")) };
                match self.g.knobs.get(name) {
                    Some(v) => {
                        let got = from_json(v);
                        let same_kind = matches!((&got, &a[1]), (V::Num(_), V::Num(_)) | (V::Str(_), V::Str(_)) | (V::List(_), V::List(_)) | (V::Bool(_), V::Bool(_)) | (V::Dict(_), V::Dict(_)));
                        if !same_kind {
                            return Err(d(pos, format!("the style gives knob \"{name}\" a {} but the recipe's default is a {}", type_name(&got), type_name(&a[1])), "make the style's value the same kind as the default"));
                        }
                        got
                    }
                    None => a[1].clone(),
                }
            }
            "edits_of" => match (a.first(), a.get(1)) {
                (Some(V::Str(path)), Some(V::Str(scope))) if a.len() == 2 => read_edits(path, scope),
                _ => return Err(d(pos, "edits_of() takes a file path and a scope name", "edits_of(\"edits/world.json\", \"rocks\"): the list of decisions the owner recorded for that class of object")),
            },
            "lookdev" => {
                if a.len() != 2 {
                    return Err(d(pos, "lookdev() takes 2 arguments", "lookdev(full, light): the value for a final render, the cheaper one for look development"));
                }
                if self.g.lookdev { a[1].clone() } else { a[0].clone() }
            }
            "abs" => V::Num(n1(self, "abs")?.abs()),
            "round" => V::Num(n1(self, "round")?.round()),
            "int" => V::Num(n1(self, "int")?.trunc()),
            "float" => V::Num(n1(self, "float")?),
            "floor" => V::Num(n1(self, "floor")?.floor()),
            "ceil" => V::Num(n1(self, "ceil")?.ceil()),
            "sqrt" => V::Num(n1(self, "sqrt")?.sqrt()),
            "sin" => V::Num(math::sin_cos64(n1(self, "sin")?).0),
            "cos" => V::Num(math::sin_cos64(n1(self, "cos")?).1),
            "radians" => V::Num(n1(self, "radians")? * (std::f64::consts::PI / 180.0)),
            "degrees" => V::Num(n1(self, "degrees")? * (180.0 / std::f64::consts::PI)),
            "pow" => {
                if a.len() != 2 {
                    return Err(d(pos, "pow() takes 2 arguments", "pow(base, exponent)"));
                }
                V::Num(math::pow64(self.num(&a[0], pos, "pow")?, self.num(&a[1], pos, "pow")?))
            }
            "lerp" => {
                if a.len() != 3 {
                    return Err(d(pos, "lerp() takes 3 arguments", "lerp(a, b, t)"));
                }
                let (x, y, t) = (self.num(&a[0], pos, "lerp")?, self.num(&a[1], pos, "lerp")?, self.num(&a[2], pos, "lerp")?);
                V::Num(x + (y - x) * t)
            }
            "str" => V::Str(match a.first() {
                Some(V::Num(n)) => format!("{n}"),
                Some(V::Str(s)) => s.clone(),
                Some(V::Bool(b)) => format!("{b}"),
                _ => return Err(d(pos, "str() needs a number, string or bool", "str(3)")),
            }),
            "print" => {
                let text = a.iter().map(|v| match v {
                    V::Num(n) => format!("{n}"),
                    V::Str(s) => s.clone(),
                    V::Node(n) => format!("<node {n}>"),
                    V::Bool(b) => format!("{b}"),
                    other => format!("{other:?}"),
                }).collect::<Vec<_>>().join(" ");
                self.report.prints.push(text);
                V::None
            }
            "out" => {
                if a.is_empty() {
                    return Err(d(pos, "out() needs a node", "out(my_node)"));
                }
                for v in a {
                    match v {
                        V::Node(n) => {
                            if !self.report.outputs.contains(&n) {
                                self.report.outputs.push(n);
                            }
                        }
                        V::List(l) => {
                            for x in l {
                                if let V::Node(n) = x {
                                    if !self.report.outputs.contains(&n) {
                                        self.report.outputs.push(n);
                                    }
                                }
                            }
                        }
                        o => return Err(d(pos, format!("out() needs nodes, got a {}", type_name(&o)), "out(my_node)")),
                    }
                }
                V::None
            }
            "spec" => {
                let (Some(V::Node(node)), Some(V::Str(kind))) = (a.first().cloned(), a.get(1).cloned()) else {
                    return Err(d(pos, "spec() takes (node, kind, ...)", "e.g. spec(tree, \"height\", min=4, max=9)"));
                };
                let numv = |k: &str| match kwget(k) { Some(V::Num(n)) => Some(n), _ => None };
                self.report.specs.push(SpecDef { node, kind, min: numv("min"), max: numv("max"), name: match kwget("name") { Some(V::Str(s)) => Some(s), _ => None }, line: pos.line });
                V::None
            }
            "asset" => {
                let Some(V::Str(aname)) = a.first().cloned() else {
                    return Err(d(pos, "asset() takes (name, output=node, inputs=[...], promote={...})", "e.g. asset(\"column\", output=top, promote={\"height\": [shaft, \"height\"]})"));
                };
                let Some(V::Node(output)) = kwget("output") else {
                    return Err(d(pos, "asset() needs output=<node>", "output=my_final_node"));
                };
                let inputs: Vec<String> = match kwget("inputs") {
                    Some(V::List(l)) => l.into_iter().filter_map(|v| if let V::Node(n) = v { Some(n) } else { None }).collect(),
                    _ => vec![],
                };
                let mut promote = Vec::new();
                if let Some(V::Dict(items)) = kwget("promote") {
                    for (k, v) in items {
                        match v {
                            V::List(l) if l.len() == 2 => match (&l[0], &l[1]) {
                                (V::Node(n), V::Str(p)) => promote.push((k, n.clone(), p.clone())),
                                _ => return Err(d(pos, "promote values must be [node, \"param\"]", "promote={\"height\": [shaft, \"height\"]}")),
                            },
                            _ => return Err(d(pos, "promote values must be [node, \"param\"]", "promote={\"height\": [shaft, \"height\"]}")),
                        }
                    }
                }
                let doc = match kwget("doc") { Some(V::Str(s)) => s, _ => String::new() };
                let asset = self.g.make_asset(&aname, &doc, &output, &inputs, &promote).map_err(|m| d(pos, m, "check the output node, inputs and promoted parameters"))?;
                self.g.add_asset(asset);
                V::None
            }
            other => return Err(d(pos, format!("'{other}' is not a built-in function"), "check the spelling; get_guide lists the built-ins")),
        })
    }
}

fn equal(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Num(x), V::Num(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::None, V::None) => true,
        (V::Node(x), V::Node(y)) => x == y,
        (V::List(x), V::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| equal(p, q)),
        _ => false,
    }
}
