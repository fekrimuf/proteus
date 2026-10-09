//! Reference interpreter. One implementation serves both plain evaluation (G = 0) and
//! forward-mode autodiff (G > 0 gradient channels): every value carries G tangents with
//! respect to the program's `ch()` parameters. Scalars are stored splatted across 3 lanes,
//! so float and vec3 share all arithmetic.

use crate::check::{Bin, Fn_, IStmt, Ir, Special};
use crate::Program;
use pt_core::math;
use pt_core::noise;
use pt_core::rng::rand4;
use rayon::prelude::*;

#[derive(Clone, Copy)]
pub struct Val<const G: usize> {
    pub v: [f32; 3],
    pub d: [[f32; 3]; G],
}

impl<const G: usize> Val<G> {
    fn zero() -> Self {
        Val { v: [0.0; 3], d: [[0.0; 3]; G] }
    }
    fn splat(s: f32) -> Self {
        Val { v: [s; 3], d: [[0.0; 3]; G] }
    }
    fn un(a: Self, f: impl Fn(f32) -> f32, df: impl Fn(f32) -> f32) -> Self {
        let mut r = Self::zero();
        for c in 0..3 {
            r.v[c] = f(a.v[c]);
            let k = df(a.v[c]);
            for g in 0..G {
                r.d[g][c] = a.d[g][c] * k;
            }
        }
        r
    }
    fn bin(a: Self, b: Self, f: impl Fn(f32, f32) -> f32, da: impl Fn(f32, f32) -> f32, db: impl Fn(f32, f32) -> f32) -> Self {
        let mut r = Self::zero();
        for c in 0..3 {
            let (x, y) = (a.v[c], b.v[c]);
            r.v[c] = f(x, y);
            let (ka, kb) = (da(x, y), db(x, y));
            for g in 0..G {
                r.d[g][c] = a.d[g][c] * ka + b.d[g][c] * kb;
            }
        }
        r
    }
    fn cmp(a: Self, b: Self, f: impl Fn(f32, f32) -> bool) -> Self {
        Self::splat(if f(a.v[0], b.v[0]) { 1.0 } else { 0.0 })
    }
    fn add(a: Self, b: Self) -> Self {
        Self::bin(a, b, |x, y| x + y, |_, _| 1.0, |_, _| 1.0)
    }
    fn sub(a: Self, b: Self) -> Self {
        Self::bin(a, b, |x, y| x - y, |_, _| 1.0, |_, _| -1.0)
    }
    fn mul(a: Self, b: Self) -> Self {
        Self::bin(a, b, |x, y| x * y, |_, y| y, |x, _| x)
    }
    fn div(a: Self, b: Self) -> Self {
        Self::bin(a, b, |x, y| x / y, |_, y| 1.0 / y, |x, y| -x / (y * y))
    }
    fn min(a: Self, b: Self) -> Self {
        Self::bin(a, b, f32::min, |x, y| if x <= y { 1.0 } else { 0.0 }, |x, y| if x <= y { 0.0 } else { 1.0 })
    }
    fn max(a: Self, b: Self) -> Self {
        Self::bin(a, b, f32::max, |x, y| if x >= y { 1.0 } else { 0.0 }, |x, y| if x >= y { 0.0 } else { 1.0 })
    }
    fn pow(a: Self, b: Self) -> Self {
        Self::bin(
            a,
            b,
            math::pow,
            |x, y| if x > 0.0 { y * math::pow(x, y - 1.0) } else { 0.0 },
            |x, y| if x > 0.0 { math::pow(x, y) * math::ln(x) } else { 0.0 },
        )
    }
    fn c(x: f32) -> Self {
        Self::splat(x)
    }
    fn sum3(a: Self) -> Self {
        let mut r = Self::zero();
        let s = a.v[0] + a.v[1] + a.v[2];
        r.v = [s; 3];
        for g in 0..G {
            let t = a.d[g][0] + a.d[g][1] + a.d[g][2];
            r.d[g] = [t; 3];
        }
        r
    }
    fn dot(a: Self, b: Self) -> Self {
        Self::sum3(Self::mul(a, b))
    }
    fn sqrt(a: Self) -> Self {
        Self::un(a, f32::sqrt, |x| if x > 0.0 { 0.5 / x.sqrt() } else { 0.0 })
    }
    fn length(a: Self) -> Self {
        Self::sqrt(Self::dot(a, a))
    }
    fn clamp(x: Self, lo: Self, hi: Self) -> Self {
        Self::max(Self::min(x, hi), lo)
    }
    fn comp(a: Self, i: usize) -> Self {
        let mut r = Self::zero();
        r.v = [a.v[i]; 3];
        for g in 0..G {
            r.d[g] = [a.d[g][i]; 3];
        }
        r
    }
    fn vec3(x: Self, y: Self, z: Self) -> Self {
        let mut r = Self::zero();
        r.v = [x.v[0], y.v[0], z.v[0]];
        for g in 0..G {
            r.d[g] = [x.d[g][0], y.d[g][0], z.d[g][0]];
        }
        r
    }
}

pub enum AttrView<'a> {
    F(&'a [f32]),
    V(&'a [[f32; 3]]),
}

pub struct Inputs<'a> {
    pub n: usize,
    /// One entry per `Program::attrs`; Some for attributes that exist on the input.
    pub attrs: Vec<Option<AttrView<'a>>>,
    /// One entry per `Program::chans` (floats splatted).
    pub chans: Vec<[f32; 3]>,
    pub frame: f32,
}

pub struct Outputs<const G: usize> {
    /// For each written attribute: (index into Program::attrs, values, tangents per point).
    pub written: Vec<(usize, Vec<[f32; 3]>, Vec<[[f32; 3]; G]>)>,
}

struct St<'a, const G: usize> {
    slots: Vec<Val<G>>,
    chans: &'a [[f32; 3]],
    ptnum: f32,
    numpt: f32,
    frame: f32,
}

fn eval<const G: usize>(ir: &Ir, st: &St<G>) -> Val<G> {
    match ir {
        Ir::Const(x) => Val::splat(*x),
        Ir::Slot(i) => st.slots[*i],
        Ir::Comp(a, c) => Val::comp(eval(a, st), *c as usize),
        Ir::Vec3(x, y, z) => Val::vec3(eval(x, st), eval(y, st), eval(z, st)),
        Ir::Neg(a) => Val::un(eval(a, st), |x| -x, |_| -1.0),
        Ir::Not(a) => Val::splat(if eval(a, st).v[0] == 0.0 { 1.0 } else { 0.0 }),
        Ir::Chan(i) => {
            let mut v = Val::splat(st.chans[*i][0]);
            if *i < G {
                v.d[*i] = [1.0; 3];
            }
            v
        }
        Ir::ChanV(i) => Val { v: st.chans[*i], d: [[0.0; 3]; G] },
        Ir::Special(s) => Val::splat(match s {
            Special::PtNum => st.ptnum,
            Special::NumPt => st.numpt,
            Special::Frame => st.frame,
            Special::Time => st.frame / 24.0,
        }),
        Ir::Bin(op, a, b) => {
            let (x, y) = (eval(a, st), eval(b, st));
            match op {
                Bin::Add => Val::add(x, y),
                Bin::Sub => Val::sub(x, y),
                Bin::Mul => Val::mul(x, y),
                Bin::Div => Val::div(x, y),
                Bin::Lt => Val::cmp(x, y, |a, b| a < b),
                Bin::Gt => Val::cmp(x, y, |a, b| a > b),
                Bin::Le => Val::cmp(x, y, |a, b| a <= b),
                Bin::Ge => Val::cmp(x, y, |a, b| a >= b),
                Bin::Eq => Val::cmp(x, y, |a, b| a == b),
                Bin::Ne => Val::cmp(x, y, |a, b| a != b),
                Bin::And => Val::cmp(x, y, |a, b| a != 0.0 && b != 0.0),
                Bin::Or => Val::cmp(x, y, |a, b| a != 0.0 || b != 0.0),
            }
        }
        Ir::Call(f, args) => {
            let a: Vec<Val<G>> = args.iter().map(|e| eval(e, st)).collect();
            call(*f, &a)
        }
    }
}

fn call<const G: usize>(f: Fn_, a: &[Val<G>]) -> Val<G> {
    match f {
        Fn_::Sin => Val::un(a[0], |x| math::sin_cos(x).0, |x| math::sin_cos(x).1),
        Fn_::Cos => Val::un(a[0], |x| math::sin_cos(x).1, |x| -math::sin_cos(x).0),
        Fn_::Abs => Val::un(a[0], f32::abs, |x| if x >= 0.0 { 1.0 } else { -1.0 }),
        Fn_::Sqrt => Val::sqrt(a[0]),
        Fn_::Floor => Val::un(a[0], f32::floor, |_| 0.0),
        Fn_::Ceil => Val::un(a[0], f32::ceil, |_| 0.0),
        Fn_::Fract => Val::un(a[0], |x| x - x.floor(), |_| 1.0),
        Fn_::Exp => Val::un(a[0], math::exp, math::exp),
        Fn_::Ln => Val::un(a[0], math::ln, |x| 1.0 / x),
        Fn_::Sign => Val::un(a[0], |x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }, |_| 0.0),
        Fn_::Min => Val::min(a[0], a[1]),
        Fn_::Max => Val::max(a[0], a[1]),
        Fn_::Pow => Val::pow(a[0], a[1]),
        Fn_::Step => Val::bin(a[0], a[1], |e, x| if x >= e { 1.0 } else { 0.0 }, |_, _| 0.0, |_, _| 0.0),
        Fn_::Clamp => Val::clamp(a[0], a[1], a[2]),
        Fn_::Mix => Val::add(a[0], Val::mul(Val::sub(a[1], a[0]), a[2])),
        Fn_::Smoothstep => {
            let t = Val::clamp(Val::div(Val::sub(a[2], a[0]), Val::sub(a[1], a[0])), Val::c(0.0), Val::c(1.0));
            Val::mul(Val::mul(t, t), Val::sub(Val::c(3.0), Val::mul(Val::c(2.0), t)))
        }
        Fn_::Fit => {
            // fit(x, a, b, c, d): remap [a,b] -> [c,d], clamped.
            let (x, lo, hi, c, d) = (a[0], a[1], a[2], a[3], a[4]);
            if hi.v[0] == lo.v[0] {
                return c;
            }
            let t = Val::clamp(Val::div(Val::sub(x, lo), Val::sub(hi, lo)), Val::c(0.0), Val::c(1.0));
            Val::add(c, Val::mul(t, Val::sub(d, c)))
        }
        Fn_::Length => Val::length(a[0]),
        Fn_::Distance => Val::length(Val::sub(a[0], a[1])),
        Fn_::Dot => Val::dot(a[0], a[1]),
        Fn_::Normalize => {
            let l = Val::length(a[0]);
            if l.v[0] > 0.0 { Val::div(a[0], l) } else { a[0] }
        }
        Fn_::Cross => {
            let (p, q) = (a[0], a[1]);
            let (px, py, pz) = (Val::comp(p, 0), Val::comp(p, 1), Val::comp(p, 2));
            let (qx, qy, qz) = (Val::comp(q, 0), Val::comp(q, 1), Val::comp(q, 2));
            Val::vec3(
                Val::sub(Val::mul(py, qz), Val::mul(pz, qy)),
                Val::sub(Val::mul(pz, qx), Val::mul(px, qz)),
                Val::sub(Val::mul(px, qy), Val::mul(py, qx)),
            )
        }
        Fn_::Noise => num_grad(a[0], |p| noise::value_noise(p, 0)),
        Fn_::Fbm => {
            let oct = a[1].v[0].clamp(1.0, 8.0) as u32;
            num_grad(a[0], move |p| noise::fbm(p, 1.0, oct, 0))
        }
        Fn_::Rand => Val::splat(rand4(a[0].v[0].to_bits() as i64, 0, 0)[0]),
    }
}

/// Scalar field of a vec3 with tangents from central differences (noise is only piecewise
/// smooth, so this is the declared `Piecewise` differentiability).
fn num_grad<const G: usize>(p: Val<G>, f: impl Fn([f32; 3]) -> f32) -> Val<G> {
    let mut r = Val::splat(f(p.v));
    if G > 0 {
        let h = 1.0e-3f32;
        let mut gr = [0.0f32; 3];
        for (c, g) in gr.iter_mut().enumerate() {
            let (mut hi, mut lo) = (p.v, p.v);
            hi[c] += h;
            lo[c] -= h;
            *g = (f(hi) - f(lo)) / (2.0 * h);
        }
        for k in 0..G {
            let t = gr[0] * p.d[k][0] + gr[1] * p.d[k][1] + gr[2] * p.d[k][2];
            r.d[k] = [t; 3];
        }
    }
    r
}

fn exec<const G: usize>(stmts: &[IStmt], st: &mut St<G>) {
    for s in stmts {
        match s {
            IStmt::Set(slot, ir) => st.slots[*slot] = eval(ir, st),
            IStmt::SetComp(slot, c, ir) => {
                let v = eval(ir, st);
                let c = *c as usize;
                st.slots[*slot].v[c] = v.v[0];
                for g in 0..G {
                    st.slots[*slot].d[g][c] = v.d[g][0];
                }
            }
            IStmt::If(c, t, e) => {
                if eval(c, st).v[0] != 0.0 {
                    exec(t, st);
                } else {
                    exec(e, st);
                }
            }
            IStmt::For(slot, lo, hi, body) => {
                for i in *lo..*hi {
                    st.slots[*slot] = Val::splat(i as f32);
                    exec(body, st);
                }
            }
        }
    }
}

const CHUNK: usize = 4096;

impl Program {
    pub fn run<const G: usize>(&self, inp: &Inputs) -> Outputs<G> {
        let written: Vec<usize> = (0..self.attrs.len()).filter(|&i| self.attrs[i].written).collect();
        let nchunks = inp.n.div_ceil(CHUNK);
        let parts: Vec<Vec<(Vec<[f32; 3]>, Vec<[[f32; 3]; G]>)>> = (0..nchunks)
            .into_par_iter()
            .map(|ci| {
                let (lo, hi) = (ci * CHUNK, ((ci + 1) * CHUNK).min(inp.n));
                let mut st = St::<G> { slots: vec![Val::zero(); self.nslots], chans: &inp.chans, ptnum: 0.0, numpt: inp.n as f32, frame: inp.frame };
                let mut outs: Vec<(Vec<[f32; 3]>, Vec<[[f32; 3]; G]>)> = written.iter().map(|_| (Vec::with_capacity(hi - lo), Vec::with_capacity(hi - lo))).collect();
                for i in lo..hi {
                    st.ptnum = i as f32;
                    for s in st.slots.iter_mut() {
                        *s = Val::zero();
                    }
                    for (ai, a) in self.attrs.iter().enumerate() {
                        if a.exists {
                            match &inp.attrs[ai] {
                                Some(AttrView::F(v)) => st.slots[a.slot] = Val::splat(v[i]),
                                Some(AttrView::V(v)) => st.slots[a.slot].v = v[i],
                                None => {}
                            }
                        }
                    }
                    exec(&self.stmts, &mut st);
                    for (k, &ai) in written.iter().enumerate() {
                        let v = st.slots[self.attrs[ai].slot];
                        outs[k].0.push(v.v);
                        outs[k].1.push(v.d);
                    }
                }
                outs
            })
            .collect();
        let mut result: Vec<(usize, Vec<[f32; 3]>, Vec<[[f32; 3]; G]>)> = written.iter().map(|&ai| (ai, Vec::with_capacity(inp.n), Vec::with_capacity(inp.n))).collect();
        for part in parts {
            for (k, (v, d)) in part.into_iter().enumerate() {
                result[k].1.extend(v);
                result[k].2.extend(d);
            }
        }
        Outputs { written: result }
    }
}
