//! Type checking and lowering of the AST to IR. All diagnostics carry a position and a fix.

use crate::parse::{parse, Expr, Pos, Stmt, Target};
use crate::query::{QueryUse, RefAttr, RefEnv, Scope};
use crate::{suggest, AttrSlot, Diag, Program, Ty};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fn_ {
    Sin, Cos, Abs, Sqrt, Floor, Ceil, Fract, Exp, Ln, Sign,
    Min, Max, Pow, Step, Clamp, Mix, Smoothstep, Fit,
    Length, Normalize, Dot, Cross, Distance, Noise, Fbm, Rand,
    Atan2, Atan, Asin, Acos, Mod,
}

/// A question asked of the second input. `usize` is the index into `Program::refs` of the attribute it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Q {
    NearDist,
    NearDistOther,
    NearPos,
    NearCount,
    NearF(usize),
    NearV(usize),
    NearSum(usize),
    SurfDist,
    SurfPos,
    SurfN,
    SurfF(usize),
    SurfV(usize),
    RayDist,
    RayPos,
    RayN,
    RayF(usize),
    RayV(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    PtNum,
    NumPt,
    Frame,
    Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Add, Sub, Mul, Div, Lt, Gt, Le, Ge, Eq, Ne, And, Or,
}

#[derive(Clone, Debug)]
pub enum Ir {
    Const(f32),
    Slot(usize),
    Comp(Box<Ir>, u8),
    Vec3(Box<Ir>, Box<Ir>, Box<Ir>),
    Neg(Box<Ir>),
    Not(Box<Ir>),
    Bin(Bin, Box<Ir>, Box<Ir>),
    Call(Fn_, Vec<Ir>),
    Query(Q, Vec<Ir>),
    Chan(usize),
    ChanV(usize),
    Special(Special),
}

#[derive(Clone, Debug)]
pub enum IStmt {
    Set(usize, Ir),
    SetComp(usize, u8, Ir),
    If(Ir, Vec<IStmt>, Vec<IStmt>),
    For(usize, i32, i32, Vec<IStmt>),
}

const FUNCS: &[&str] = &[
    "sin", "cos", "tan", "abs", "sqrt", "floor", "ceil", "fract", "exp", "ln", "sign", "min", "max", "pow", "step", "clamp", "mix", "lerp",
    "smoothstep", "fit", "length", "normalize", "dot", "cross", "distance", "noise", "fbm", "rand", "vec3", "ch", "chv",
    "atan2", "atan", "asin", "acos", "mod", "radians", "degrees",
];

/// One question a program can ask of its second input. `attr` is Some when the first argument is an attribute name in quotes
/// (read from that scope); `args` are the remaining arguments.
struct QSpec {
    name: &'static str,
    attr: Option<Scope>,
    args: &'static [Ty],
    ret: Ty,
    sig: &'static str,
}

const FL: Ty = Ty::Float;
const V3: Ty = Ty::Vec3;

const QUERIES: &[QSpec] = &[
    QSpec { name: "near_dist", attr: None, args: &[V3], ret: FL, sig: "near_dist(p) -> metres to the closest point of the second input (1e9 if it has none)" },
    QSpec { name: "near_dist_other", attr: None, args: &[V3], ret: FL, sig: "near_dist_other(p) -> like near_dist but skips the point whose index equals @ptnum (use when both inputs are the same geometry)" },
    QSpec { name: "near_pos", attr: None, args: &[V3], ret: V3, sig: "near_pos(p) -> position of the closest point of the second input" },
    QSpec { name: "near_count", attr: None, args: &[V3, FL], ret: FL, sig: "near_count(p, radius) -> how many points of the second input lie within radius metres" },
    QSpec { name: "near_f", attr: Some(Scope::Near), args: &[V3], ret: FL, sig: "near_f(\"attr\", p) -> float attribute of the closest point of the second input" },
    QSpec { name: "near_v", attr: Some(Scope::Near), args: &[V3], ret: V3, sig: "near_v(\"attr\", p) -> vec3 attribute of the closest point of the second input" },
    QSpec { name: "near_sum", attr: Some(Scope::Near), args: &[V3, FL], ret: FL, sig: "near_sum(\"attr\", p, radius) -> sum of a float attribute over the points within radius" },
    QSpec { name: "surf_dist", attr: None, args: &[V3], ret: FL, sig: "surf_dist(p) -> metres to the closest point on the surface of the second input (1e9 if it has none)" },
    QSpec { name: "surf_pos", attr: None, args: &[V3], ret: V3, sig: "surf_pos(p) -> the closest point on the surface of the second input" },
    QSpec { name: "surf_n", attr: None, args: &[V3], ret: V3, sig: "surf_n(p) -> face normal there, turned toward p" },
    QSpec { name: "surf_f", attr: Some(Scope::Surf), args: &[V3], ret: FL, sig: "surf_f(\"attr\", p) -> float point attribute interpolated at the closest surface point" },
    QSpec { name: "surf_v", attr: Some(Scope::Surf), args: &[V3], ret: V3, sig: "surf_v(\"attr\", p) -> vec3 point attribute interpolated at the closest surface point" },
    QSpec { name: "ray_dist", attr: None, args: &[V3, V3], ret: FL, sig: "ray_dist(origin, direction) -> metres along the ray to the first surface of the second input, or -1 for a miss" },
    QSpec { name: "ray_pos", attr: None, args: &[V3, V3], ret: V3, sig: "ray_pos(origin, direction) -> where the ray first hits (zero vector for a miss)" },
    QSpec { name: "ray_n", attr: None, args: &[V3, V3], ret: V3, sig: "ray_n(origin, direction) -> face normal at the hit, facing the origin (zero vector for a miss)" },
    QSpec { name: "ray_f", attr: Some(Scope::Surf), args: &[V3, V3], ret: FL, sig: "ray_f(\"attr\", origin, direction) -> float point attribute interpolated at the hit (0 for a miss)" },
    QSpec { name: "ray_v", attr: Some(Scope::Surf), args: &[V3, V3], ret: V3, sig: "ray_v(\"attr\", origin, direction) -> vec3 point attribute interpolated at the hit (zero vector for a miss)" },
];

fn query_op(name: &str, ai: usize) -> Q {
    match name {
        "near_dist" => Q::NearDist,
        "near_dist_other" => Q::NearDistOther,
        "near_pos" => Q::NearPos,
        "near_count" => Q::NearCount,
        "near_f" => Q::NearF(ai),
        "near_v" => Q::NearV(ai),
        "near_sum" => Q::NearSum(ai),
        "surf_dist" => Q::SurfDist,
        "surf_pos" => Q::SurfPos,
        "surf_n" => Q::SurfN,
        "surf_f" => Q::SurfF(ai),
        "surf_v" => Q::SurfV(ai),
        "ray_dist" => Q::RayDist,
        "ray_pos" => Q::RayPos,
        "ray_n" => Q::RayN,
        "ray_f" => Q::RayF(ai),
        _ => Q::RayV(ai),
    }
}

struct Cx<'a> {
    env: &'a BTreeMap<String, Ty>,
    refenv: Option<&'a RefEnv<'a>>,
    diags: Vec<Diag>,
    scopes: Vec<HashMap<String, (usize, Ty)>>,
    nslots: usize,
    attrs: Vec<AttrSlot>,
    chans: Vec<(String, Ty)>,
    refs: Vec<RefAttr>,
    uses: QueryUse,
}

type L = Option<(Ir, Ty)>;

fn arith(a: Ty, b: Ty) -> Ty {
    if a == Ty::Vec3 || b == Ty::Vec3 {
        Ty::Vec3
    } else {
        Ty::Float
    }
}

impl<'a> Cx<'a> {
    fn err(&mut self, p: Pos, msg: String, fix: impl Into<String>) {
        self.diags.push(Diag::new(p.line, p.col, msg, fix));
    }

    fn new_slot(&mut self) -> usize {
        self.nslots += 1;
        self.nslots - 1
    }

    fn attr_idx(&mut self, name: &str, write_ty: Option<Ty>, p: Pos) -> Option<usize> {
        if let Some(i) = self.attrs.iter().position(|a| a.name == name) {
            if let Some(t) = write_ty {
                if self.attrs[i].ty != t {
                    let have = self.attrs[i].ty;
                    self.err(p, format!("attribute @{name} is {have:?} but you assign {t:?}"), match have {
                        Ty::Vec3 => "wrap the value: vec3(x) or {x, y, z}",
                        Ty::Float => "assign a float, e.g. use a component (.y) or length()",
                    });
                    return None;
                }
            }
            return Some(i);
        }
        let (ty, exists) = match (self.env.get(name), write_ty) {
            (Some(&t), Some(w)) if t != w => {
                let fix = match t {
                    Ty::Vec3 => "wrap the value: vec3(x) or {x, y, z}; or write to a new attribute name",
                    Ty::Float => "assign a float (use a component like .y or length(v)); or write to a new attribute name",
                };
                self.err(p, format!("attribute @{name} is {t:?} on the input but you assign {w:?}"), fix);
                return None;
            }
            (Some(&t), _) => (t, true),
            (None, Some(w)) => (w, false),
            (None, None) => {
                let mut avail: Vec<&str> = self.env.keys().map(String::as_str).collect();
                avail.extend(self.attrs.iter().map(|a| a.name.as_str()));
                let hint = suggest(name, &avail).map(|s| format!("did you mean @{s}? ")).unwrap_or_default();
                self.err(p, format!("attribute @{name} does not exist on the input (available: {avail:?})"), format!("{hint}create it upstream (e.g. normals, scatter, color_by_height) or assign it first"));
                return None;
            }
        };
        let slot = self.new_slot();
        self.attrs.push(AttrSlot { name: name.to_string(), ty, slot, read: false, written: false, exists });
        Some(self.attrs.len() - 1)
    }

    fn find_var(&self, name: &str) -> Option<(usize, Ty)> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn expr(&mut self, e: &Expr) -> L {
        match e {
            Expr::Num(v, _) => Some((Ir::Const(*v), Ty::Float)),
            Expr::Str(_, p) => {
                self.err(*p, "strings are only allowed as the attribute name of ch(\"name\"), chv(\"name\") and the near_/surf_/ray_ lookups".into(), "remove the string");
                None
            }
            Expr::Var(name, p) => match self.find_var(name) {
                Some((slot, ty)) => Some((Ir::Slot(slot), ty)),
                None => {
                    let names: Vec<&str> = self.scopes.iter().flat_map(|s| s.keys().map(String::as_str)).collect();
                    let hint = suggest(name, &names).map(|s| format!("did you mean '{s}'? ")).unwrap_or_default();
                    self.err(*p, format!("unknown variable '{name}'"), format!("{hint}declare it first: float {name} = ...;"));
                    None
                }
            },
            Expr::Attr(name, p) => {
                let special = match name.as_str() {
                    "ptnum" => Some(Special::PtNum),
                    "numpt" => Some(Special::NumPt),
                    "Frame" => Some(Special::Frame),
                    "Time" => Some(Special::Time),
                    _ => None,
                };
                if let Some(s) = special {
                    return Some((Ir::Special(s), Ty::Float));
                }
                let i = self.attr_idx(name, None, *p)?;
                self.attrs[i].read = true;
                Some((Ir::Slot(self.attrs[i].slot), self.attrs[i].ty))
            }
            Expr::Comp(inner, c, p) => {
                let (ir, ty) = self.expr(inner)?;
                if ty != Ty::Vec3 {
                    self.err(*p, "components (.x .y .z) only exist on vec3".into(), "remove the component or use a vec3 value");
                    return None;
                }
                Some((Ir::Comp(Box::new(ir), *c), Ty::Float))
            }
            Expr::Vec(items, p) => {
                let mut v = Vec::new();
                for it in items {
                    let (ir, ty) = self.expr(it)?;
                    if ty != Ty::Float {
                        self.err(*p, "vector literal components must be floats".into(), "use .x/.y/.z to pick a component");
                        return None;
                    }
                    v.push(ir);
                }
                let mut it = v.into_iter();
                Some((Ir::Vec3(Box::new(it.next()?), Box::new(it.next()?), Box::new(it.next()?)), Ty::Vec3))
            }
            Expr::Un(op, inner, p) => {
                let (ir, ty) = self.expr(inner)?;
                match *op {
                    "-" => Some((Ir::Neg(Box::new(ir)), ty)),
                    _ => {
                        if ty != Ty::Float {
                            self.err(*p, "'!' needs a float condition".into(), "compare first, e.g. !(x > 0)");
                            return None;
                        }
                        Some((Ir::Not(Box::new(ir)), Ty::Float))
                    }
                }
            }
            Expr::Bin(op, l, r, p) => {
                let (li, lt) = self.expr(l)?;
                let (ri, rt) = self.expr(r)?;
                let (b, rty) = match *op {
                    "+" => (Bin::Add, arith(lt, rt)),
                    "-" => (Bin::Sub, arith(lt, rt)),
                    "*" => (Bin::Mul, arith(lt, rt)),
                    "/" => (Bin::Div, arith(lt, rt)),
                    other => {
                        if lt != Ty::Float || rt != Ty::Float {
                            self.err(*p, format!("'{other}' compares floats, but an operand is vec3"), "compare components or lengths, e.g. length(v) < 1");
                            return None;
                        }
                        let b = match other {
                            "<" => Bin::Lt, ">" => Bin::Gt, "<=" => Bin::Le, ">=" => Bin::Ge,
                            "==" => Bin::Eq, "!=" => Bin::Ne, "&&" => Bin::And, _ => Bin::Or,
                        };
                        (b, Ty::Float)
                    }
                };
                Some((Ir::Bin(b, Box::new(li), Box::new(ri)), rty))
            }
            Expr::Call(name, args, p) => self.call(name, args, *p),
        }
    }

    /// A question asked of the second input: resolves the attribute name (if any) against what the second input offers.
    fn query_call(&mut self, spec: &QSpec, args: &[Expr], p: Pos) -> L {
        let name = spec.name;
        let Some(renv) = self.refenv else {
            self.err(p, format!("{name}() asks the second input a question, but this node has none"), "connect the geometry you want to ask about to the node's second input");
            return None;
        };
        let want = spec.args.len() + usize::from(spec.attr.is_some());
        if args.len() != want {
            self.err(p, format!("{name}() takes {want} argument(s) but got {}", args.len()), format!("signature: {}", spec.sig));
            return None;
        }
        let mut rest = args;
        let mut ai = 0usize;
        if let Some(scope) = spec.attr {
            let Expr::Str(attr, sp) = &args[0] else {
                self.err(p, format!("the first argument of {name}() is the attribute name in quotes"), format!("signature: {}", spec.sig));
                return None;
            };
            let avail = match scope {
                Scope::Near => renv.near,
                Scope::Surf => renv.surf,
            };
            let Some(&ty) = avail.get(attr) else {
                let names: Vec<&str> = avail.keys().map(String::as_str).collect();
                let hint = suggest(attr, &names).map(|s| format!("did you mean \"{s}\"? ")).unwrap_or_default();
                self.err(*sp, format!("the second input has no attribute \"{attr}\" (available: {names:?})"), format!("{hint}create it upstream of the second input"));
                return None;
            };
            if ty != spec.ret {
                let (have, fix) = match ty {
                    Ty::Float => ("float", "use the _f function (near_f, surf_f, ray_f)"),
                    Ty::Vec3 => ("vec3", "use the _v function (near_v, surf_v, ray_v)"),
                };
                self.err(*sp, format!("attribute \"{attr}\" of the second input is a {have}, but {name}() returns the other type"), fix);
                return None;
            }
            ai = match self.refs.iter().position(|r| r.name == *attr && r.scope == scope) {
                Some(i) => i,
                None => {
                    self.refs.push(RefAttr { name: attr.clone(), ty, scope });
                    self.refs.len() - 1
                }
            };
            rest = &args[1..];
        }
        let mut ir = Vec::new();
        for (e, want) in rest.iter().zip(spec.args) {
            let (i, t) = self.expr(e)?;
            if t != *want {
                self.err(p, format!("wrong argument types for {name}()"), format!("signature: {}", spec.sig));
                return None;
            }
            ir.push(i);
        }
        if name.starts_with("near_") {
            self.uses.near = true;
        } else {
            self.uses.surf = true;
        }
        Some((Ir::Query(query_op(name, ai), ir), spec.ret))
    }

    fn call(&mut self, name: &str, args: &[Expr], p: Pos) -> L {
        if name == "ch" || name == "chv" {
            let [Expr::Str(s, _)] = args else {
                self.err(p, format!("{name}() takes one string literal, e.g. {name}(\"amp\")"), "pass the channel name in quotes");
                return None;
            };
            let ty = if name == "ch" { Ty::Float } else { Ty::Vec3 };
            let idx = match self.chans.iter().position(|(n, _)| n == s) {
                Some(i) => {
                    if self.chans[i].1 != ty {
                        self.err(p, format!("channel \"{s}\" is used as both float and vec3"), "use ch() for floats and chv() for vectors consistently");
                        return None;
                    }
                    i
                }
                None => {
                    self.chans.push((s.clone(), ty));
                    self.chans.len() - 1
                }
            };
            return Some((if ty == Ty::Float { Ir::Chan(idx) } else { Ir::ChanV(idx) }, ty));
        }
        if let Some(spec) = QUERIES.iter().find(|q| q.name == name) {
            return self.query_call(spec, args, p);
        }
        let mut a = Vec::new();
        for e in args {
            a.push(self.expr(e)?);
        }
        let n = a.len();
        let arity = |this: &mut Self, want: usize, sig: &str| -> bool {
            if n != want {
                this.err(p, format!("{name}() takes {want} argument(s) but got {n}"), format!("signature: {sig}"));
                false
            } else {
                true
            }
        };
        let need = |this: &mut Self, ok: bool, sig: &str| -> bool {
            if !ok {
                this.err(p, format!("wrong argument types for {name}()"), format!("signature: {sig}"));
            }
            ok
        };
        let (f, rty): (Fn_, Ty) = match name {
            "sin" | "cos" | "abs" | "sqrt" | "floor" | "ceil" | "fract" | "exp" | "ln" | "sign" | "tan" => {
                if !arity(self, 1, &format!("{name}(x)")) {
                    return None;
                }
                if name == "tan" {
                    let s = Ir::Call(Fn_::Sin, vec![a[0].0.clone()]);
                    let c = Ir::Call(Fn_::Cos, vec![a[0].0.clone()]);
                    return Some((Ir::Bin(Bin::Div, Box::new(s), Box::new(c)), a[0].1));
                }
                let f = match name {
                    "sin" => Fn_::Sin, "cos" => Fn_::Cos, "abs" => Fn_::Abs, "sqrt" => Fn_::Sqrt, "floor" => Fn_::Floor,
                    "ceil" => Fn_::Ceil, "fract" => Fn_::Fract, "exp" => Fn_::Exp, "ln" => Fn_::Ln, _ => Fn_::Sign,
                };
                (f, a[0].1)
            }
            "atan" | "asin" | "acos" => {
                if !arity(self, 1, &format!("{name}(x)")) {
                    return None;
                }
                (match name { "atan" => Fn_::Atan, "asin" => Fn_::Asin, _ => Fn_::Acos }, a[0].1)
            }
            "atan2" | "mod" => {
                if !arity(self, 2, &format!("{name}(a, b)")) {
                    return None;
                }
                (if name == "atan2" { Fn_::Atan2 } else { Fn_::Mod }, arith(a[0].1, a[1].1))
            }
            "radians" | "degrees" => {
                if !arity(self, 1, &format!("{name}(x)")) {
                    return None;
                }
                let k = if name == "radians" { std::f32::consts::PI / 180.0 } else { 180.0 / std::f32::consts::PI };
                return Some((Ir::Bin(Bin::Mul, Box::new(a[0].0.clone()), Box::new(Ir::Const(k))), a[0].1));
            }
            "min" | "max" | "pow" | "step" => {
                if !arity(self, 2, &format!("{name}(a, b)")) {
                    return None;
                }
                let f = match name { "min" => Fn_::Min, "max" => Fn_::Max, "pow" => Fn_::Pow, _ => Fn_::Step };
                (f, arith(a[0].1, a[1].1))
            }
            "clamp" => {
                if !arity(self, 3, "clamp(x, lo, hi)") || !need(self, a[1].1 == Ty::Float || a[1].1 == a[0].1, "clamp(x, lo, hi): lo/hi float or same type as x") {
                    return None;
                }
                (Fn_::Clamp, a[0].1)
            }
            "mix" | "lerp" => {
                if !arity(self, 3, "mix(a, b, t)") || !need(self, a[0].1 == a[1].1 && (a[2].1 == Ty::Float || a[2].1 == a[0].1), "mix(a, b, t): a,b same type, t float") {
                    return None;
                }
                (Fn_::Mix, a[0].1)
            }
            "smoothstep" => {
                if !arity(self, 3, "smoothstep(e0, e1, x)") {
                    return None;
                }
                (Fn_::Smoothstep, a[2].1)
            }
            "fit" => {
                if !arity(self, 5, "fit(x, a, b, c, d)") || !need(self, a.iter().all(|x| x.1 == Ty::Float), "fit(x, a, b, c, d): all floats") {
                    return None;
                }
                (Fn_::Fit, Ty::Float)
            }
            "length" => {
                if !arity(self, 1, "length(v)") || !need(self, a[0].1 == Ty::Vec3, "length(v: vec3) -> float") {
                    return None;
                }
                (Fn_::Length, Ty::Float)
            }
            "normalize" => {
                if !arity(self, 1, "normalize(v)") || !need(self, a[0].1 == Ty::Vec3, "normalize(v: vec3) -> vec3") {
                    return None;
                }
                (Fn_::Normalize, Ty::Vec3)
            }
            "dot" | "distance" => {
                if !arity(self, 2, &format!("{name}(a, b)")) || !need(self, a[0].1 == Ty::Vec3 && a[1].1 == Ty::Vec3, &format!("{name}(a: vec3, b: vec3) -> float")) {
                    return None;
                }
                (if name == "dot" { Fn_::Dot } else { Fn_::Distance }, Ty::Float)
            }
            "cross" => {
                if !arity(self, 2, "cross(a, b)") || !need(self, a[0].1 == Ty::Vec3 && a[1].1 == Ty::Vec3, "cross(a: vec3, b: vec3) -> vec3") {
                    return None;
                }
                (Fn_::Cross, Ty::Vec3)
            }
            "noise" => {
                if !arity(self, 1, "noise(p)") || !need(self, a[0].1 == Ty::Vec3, "noise(p: vec3) -> float in [-1,1]") {
                    return None;
                }
                (Fn_::Noise, Ty::Float)
            }
            "fbm" => {
                if !arity(self, 2, "fbm(p, octaves)") || !need(self, a[0].1 == Ty::Vec3 && a[1].1 == Ty::Float, "fbm(p: vec3, octaves: float) -> float") {
                    return None;
                }
                (Fn_::Fbm, Ty::Float)
            }
            "rand" => {
                if !arity(self, 1, "rand(seed)") || !need(self, a[0].1 == Ty::Float, "rand(seed: float) -> float in [0,1)") {
                    return None;
                }
                (Fn_::Rand, Ty::Float)
            }
            "vec3" => match n {
                1 if a[0].1 == Ty::Float => {
                    let x = a[0].0.clone();
                    return Some((Ir::Vec3(Box::new(x.clone()), Box::new(x.clone()), Box::new(x)), Ty::Vec3));
                }
                3 if a.iter().all(|x| x.1 == Ty::Float) => {
                    let mut it = a.into_iter().map(|x| Box::new(x.0));
                    return Some((Ir::Vec3(it.next()?, it.next()?, it.next()?), Ty::Vec3));
                }
                _ => {
                    self.err(p, "vec3() takes 1 or 3 floats".into(), "vec3(x) or vec3(x, y, z)");
                    return None;
                }
            },
            _ => {
                let mut all: Vec<&str> = FUNCS.to_vec();
                all.extend(QUERIES.iter().map(|q| q.name));
                let hint = suggest(name, &all).map(|s| format!("did you mean {s}()? ")).unwrap_or_default();
                self.err(p, format!("unknown function '{name}'"), format!("{hint}available: {}", all.join(", ")));
                return None;
            }
        };
        Some((Ir::Call(f, a.into_iter().map(|x| x.0).collect()), rty))
    }

    fn stmts(&mut self, list: &[Stmt]) -> Vec<IStmt> {
        let mut out = Vec::new();
        for s in list {
            if let Some(st) = self.stmt(s) {
                out.push(st);
            }
        }
        out
    }

    fn stmt(&mut self, s: &Stmt) -> Option<IStmt> {
        match s {
            Stmt::Decl(ty, name, e, p) => {
                let (ir, ety) = self.expr(e)?;
                if ety != *ty {
                    self.err(*p, format!("'{name}' is declared {ty:?} but the value is {ety:?}"), "change the declared type or the expression");
                    return None;
                }
                if self.scopes.last().is_some_and(|s| s.contains_key(name)) {
                    self.err(*p, format!("'{name}' is already declared in this scope"), "assign with '=' instead of redeclaring");
                    return None;
                }
                let slot = self.new_slot();
                self.scopes.last_mut().unwrap().insert(name.clone(), (slot, *ty));
                Some(IStmt::Set(slot, ir))
            }
            Stmt::Assign(target, comp, op, e, p) => {
                let (rhs, rty) = self.expr(e)?;
                let (slot, tty) = match target {
                    Target::Var(name) => match self.find_var(name) {
                        Some(v) => v,
                        None => {
                            let names: Vec<&str> = self.scopes.iter().flat_map(|s| s.keys().map(String::as_str)).collect();
                            let hint = suggest(name, &names).map(|s| format!("did you mean '{s}'? ")).unwrap_or_default();
                            self.err(*p, format!("unknown variable '{name}'"), format!("{hint}declare it first: float {name} = ...;"));
                            return None;
                        }
                    },
                    Target::Attr(name) => {
                        if matches!(name.as_str(), "ptnum" | "numpt" | "Frame" | "Time") {
                            self.err(*p, format!("@{name} is read-only"), "write to another attribute");
                            return None;
                        }
                        // A whole-attribute '=' may create it; everything else needs it to exist.
                        let hint = if comp.is_none() && *op == "=" { Some(rty) } else { None };
                        let i = if let Some(h) = hint { self.attr_idx(name, Some(h), *p)? } else { self.attr_idx(name, None, *p)? };
                        if hint.is_none() {
                            self.attrs[i].read = true;
                        }
                        self.attrs[i].written = true;
                        (self.attrs[i].slot, self.attrs[i].ty)
                    }
                };
                let cur = |c: Option<u8>| match c {
                    Some(c) => Ir::Comp(Box::new(Ir::Slot(slot)), c),
                    None => Ir::Slot(slot),
                };
                let (target_ty, value) = match comp {
                    Some(c) => {
                        if tty != Ty::Vec3 {
                            self.err(*p, "components (.x .y .z) only exist on vec3".into(), "remove the component");
                            return None;
                        }
                        if rty != Ty::Float {
                            self.err(*p, "a component takes a float value".into(), "assign a float, e.g. length(v) or v.x");
                            return None;
                        }
                        (Ty::Float, Some(*c))
                    }
                    None => (tty, None),
                };
                let ir = match *op {
                    "=" => {
                        if rty != target_ty {
                            self.err(*p, format!("cannot assign {rty:?} to a {target_ty:?}"), if target_ty == Ty::Vec3 { "wrap the float: vec3(x)" } else { "use a component or length()" });
                            return None;
                        }
                        rhs
                    }
                    _ => {
                        let b = match *op { "+=" => Bin::Add, "-=" => Bin::Sub, "*=" => Bin::Mul, _ => Bin::Div };
                        if arith(target_ty, rty) != target_ty {
                            self.err(*p, format!("{rty:?} {op} on a {target_ty:?} changes its type"), "use a float on the right-hand side");
                            return None;
                        }
                        Ir::Bin(b, Box::new(cur(value)), Box::new(rhs))
                    }
                };
                Some(match value {
                    Some(c) => IStmt::SetComp(slot, c, ir),
                    None => IStmt::Set(slot, ir),
                })
            }
            Stmt::If(c, then, els, p) => {
                let (ci, cty) = self.expr(c)?;
                if cty != Ty::Float {
                    self.err(*p, "if-condition must be a float comparison".into(), "e.g. if (@P.y > 1.0)");
                    return None;
                }
                self.scopes.push(HashMap::new());
                let t = self.stmts(then);
                self.scopes.pop();
                self.scopes.push(HashMap::new());
                let e = self.stmts(els);
                self.scopes.pop();
                Some(IStmt::If(ci, t, e))
            }
            Stmt::For(var, lo, hi, body, p) => {
                if hi - lo > 64 || hi < lo {
                    self.err(*p, format!("loop range {lo}..{hi} must satisfy 0 <= size <= 64"), "keep loops small and bounded");
                    return None;
                }
                let slot = self.new_slot();
                self.scopes.push(HashMap::from([(var.clone(), (slot, Ty::Float))]));
                let b = self.stmts(body);
                self.scopes.pop();
                Some(IStmt::For(slot, *lo, *hi, b))
            }
        }
    }
}

pub fn analyze(code: &str, env: &BTreeMap<String, Ty>) -> Result<Program, Vec<Diag>> {
    analyze_ref(code, env, None)
}

/// Like `analyze`, for a node with a second input whose attributes are `refenv`; the near_/surf_/ray_ questions are available.
pub fn analyze_ref(code: &str, env: &BTreeMap<String, Ty>, refenv: Option<&RefEnv>) -> Result<Program, Vec<Diag>> {
    let ast = parse(code).map_err(|d| vec![d])?;
    let mut cx = Cx { env, refenv, diags: vec![], scopes: vec![HashMap::new()], nslots: 0, attrs: vec![], chans: vec![], refs: vec![], uses: QueryUse::default() };
    let stmts = cx.stmts(&ast);
    if !cx.diags.is_empty() {
        return Err(cx.diags);
    }
    Ok(Program { stmts, nslots: cx.nslots, attrs: cx.attrs, chans: cx.chans, refs: cx.refs, uses: cx.uses })
}
