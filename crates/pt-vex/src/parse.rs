use crate::lex::{lex, Tok, Token};
use crate::{Diag, Ty};

#[derive(Clone, Copy, Debug)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f32, Pos),
    Str(String, Pos),
    Attr(String, Pos),
    Var(String, Pos),
    Call(String, Vec<Expr>, Pos),
    Un(&'static str, Box<Expr>, Pos),
    Bin(&'static str, Box<Expr>, Box<Expr>, Pos),
    Vec(Vec<Expr>, Pos),
    Comp(Box<Expr>, u8, Pos),
}

impl Expr {
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Num(_, p) | Expr::Str(_, p) | Expr::Attr(_, p) | Expr::Var(_, p) | Expr::Call(_, _, p) | Expr::Un(_, _, p) | Expr::Bin(_, _, _, p) | Expr::Vec(_, p) | Expr::Comp(_, _, p) => *p,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Target {
    Attr(String),
    Var(String),
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Decl(Ty, String, Expr, Pos),
    Assign(Target, Option<u8>, &'static str, Expr, Pos),
    If(Expr, Vec<Stmt>, Vec<Stmt>, Pos),
    For(String, i32, i32, Vec<Stmt>, Pos),
}

struct P {
    t: Vec<Token>,
    i: usize,
}

type R<T> = Result<T, Diag>;

pub fn parse(src: &str) -> R<Vec<Stmt>> {
    let mut p = P { t: lex(src)?, i: 0 };
    let mut out = Vec::new();
    while !matches!(p.peek(), Tok::Eof) {
        out.push(p.stmt()?);
    }
    Ok(out)
}

impl P {
    fn peek(&self) -> &Tok {
        &self.t[self.i].tok
    }
    fn pos(&self) -> Pos {
        Pos { line: self.t[self.i].line, col: self.t[self.i].col }
    }
    fn next(&mut self) -> Tok {
        let t = self.t[self.i].tok.clone();
        if self.i + 1 < self.t.len() {
            self.i += 1;
        }
        t
    }
    fn err<T>(&self, msg: String, fix: &str) -> R<T> {
        let p = self.pos();
        Err(Diag::new(p.line, p.col, msg, fix))
    }
    fn is(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::P(p) if *p == s)
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.is(s) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> R<()> {
        if self.eat(s) {
            Ok(())
        } else {
            self.err(format!("expected '{s}' but found {}", describe(self.peek())), &format!("insert '{s}'"))
        }
    }
    fn ident(&mut self) -> R<String> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            t => {
                self.i = self.i.saturating_sub(1);
                self.err(format!("expected a name but found {}", describe(&t)), "use a letter-based name")
            }
        }
    }

    fn block(&mut self) -> R<Vec<Stmt>> {
        self.expect("{")?;
        let mut v = Vec::new();
        while !self.is("}") {
            if matches!(self.peek(), Tok::Eof) {
                return self.err("missing '}' before end of code".into(), "close the block with '}'");
            }
            v.push(self.stmt()?);
        }
        self.expect("}")?;
        Ok(v)
    }

    fn stmt(&mut self) -> R<Stmt> {
        let pos = self.pos();
        if let Tok::Ident(name) = self.peek().clone() {
            match name.as_str() {
                "float" | "int" | "vec3" | "vector" | "vec" => {
                    self.next();
                    let ty = if matches!(name.as_str(), "float" | "int") { Ty::Float } else { Ty::Vec3 };
                    let var = self.ident()?;
                    self.expect("=")?;
                    let e = self.expr()?;
                    self.expect(";")?;
                    return Ok(Stmt::Decl(ty, var, e, pos));
                }
                "if" => {
                    self.next();
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let then = self.block()?;
                    let els = if matches!(self.peek(), Tok::Ident(s) if s == "else") {
                        self.next();
                        if matches!(self.peek(), Tok::Ident(s) if s == "if") { vec![self.stmt()?] } else { self.block()? }
                    } else {
                        vec![]
                    };
                    return Ok(Stmt::If(c, then, els, pos));
                }
                "for" => {
                    self.next();
                    let var = self.ident()?;
                    if !matches!(self.next(), Tok::Ident(s) if s == "in") {
                        return self.err("expected 'in' after the loop variable".into(), "write: for i in 0..8 { ... }");
                    }
                    let lo = self.int_lit()?;
                    self.expect("..")?;
                    let hi = self.int_lit()?;
                    let body = self.block()?;
                    return Ok(Stmt::For(var, lo, hi, body, pos));
                }
                _ => {}
            }
        }
        let target = match self.next() {
            Tok::At(a) => Target::Attr(a),
            Tok::Ident(v) => Target::Var(v),
            t => {
                self.i = self.i.saturating_sub(1);
                return self.err(format!("a statement cannot start with {}", describe(&t)), "statements look like: @P.y += 1; or float x = 2;");
            }
        };
        let comp = if self.eat(".") { Some(self.comp_name()?) } else { None };
        let op = match self.next() {
            Tok::P(p @ ("=" | "+=" | "-=" | "*=" | "/=")) => p,
            t => {
                self.i = self.i.saturating_sub(1);
                return self.err(format!("expected an assignment operator but found {}", describe(&t)), "use =, +=, -=, *= or /=");
            }
        };
        let e = self.expr()?;
        self.expect(";")?;
        Ok(Stmt::Assign(target, comp, op, e, pos))
    }

    fn int_lit(&mut self) -> R<i32> {
        let neg = self.eat("-");
        match self.next() {
            Tok::Num(v) if v.fract() == 0.0 => Ok(if neg { -(v as i32) } else { v as i32 }),
            _ => {
                self.i = self.i.saturating_sub(1);
                self.err("loop bounds must be integer literals".into(), "bounded loops keep programs differentiable: use for i in 0..8")
            }
        }
    }

    fn comp_name(&mut self) -> R<u8> {
        match self.next() {
            Tok::Ident(s) => match s.as_str() {
                "x" | "r" => Ok(0),
                "y" | "g" => Ok(1),
                "z" | "b" => Ok(2),
                _ => {
                    self.i = self.i.saturating_sub(1);
                    self.err(format!("unknown component '.{s}'"), "use .x .y .z (or .r .g .b)")
                }
            },
            _ => {
                self.i = self.i.saturating_sub(1);
                self.err("expected a component after '.'".into(), "use .x .y .z")
            }
        }
    }

    fn expr(&mut self) -> R<Expr> {
        self.bin(0)
    }

    fn bin(&mut self, level: usize) -> R<Expr> {
        const LEVELS: &[&[&str]] = &[&["||"], &["&&"], &["==", "!="], &["<", ">", "<=", ">="], &["+", "-"], &["*", "/"]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.bin(level + 1)?;
        loop {
            let pos = self.pos();
            let Tok::P(p) = self.peek().clone() else { break };
            if !LEVELS[level].contains(&p) {
                break;
            }
            self.next();
            let r = self.bin(level + 1)?;
            l = Expr::Bin(p, Box::new(l), Box::new(r), pos);
        }
        Ok(l)
    }

    fn unary(&mut self) -> R<Expr> {
        let pos = self.pos();
        if self.eat("-") {
            return Ok(Expr::Un("-", Box::new(self.unary()?), pos));
        }
        if self.eat("!") {
            return Ok(Expr::Un("!", Box::new(self.unary()?), pos));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        while self.is(".") {
            let pos = self.pos();
            self.next();
            let c = self.comp_name()?;
            e = Expr::Comp(Box::new(e), c, pos);
        }
        Ok(e)
    }

    fn primary(&mut self) -> R<Expr> {
        let pos = self.pos();
        match self.next() {
            Tok::Num(v) => Ok(Expr::Num(v, pos)),
            Tok::Str(s) => Ok(Expr::Str(s, pos)),
            Tok::At(a) => Ok(Expr::Attr(a, pos)),
            Tok::Ident(name) => {
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.is(")") {
                        loop {
                            args.push(self.expr()?);
                            if !self.eat(",") {
                                break;
                            }
                        }
                    }
                    self.expect(")")?;
                    Ok(Expr::Call(name, args, pos))
                } else {
                    Ok(Expr::Var(name, pos))
                }
            }
            Tok::P("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::P("{") => {
                let mut v = vec![self.expr()?];
                while self.eat(",") {
                    v.push(self.expr()?);
                }
                self.expect("}")?;
                if v.len() != 3 {
                    return Err(Diag::new(pos.line, pos.col, format!("vector literal needs 3 components, got {}", v.len()), "write {x, y, z} or vec3(x, y, z)"));
                }
                Ok(Expr::Vec(v, pos))
            }
            t => {
                self.i = self.i.saturating_sub(1);
                self.err(format!("expected a value but found {}", describe(&t)), "write a number, attribute (@P), variable or function call")
            }
        }
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(v) => format!("number {v}"),
        Tok::Str(s) => format!("string \"{s}\""),
        Tok::Ident(s) => format!("'{s}'"),
        Tok::At(s) => format!("'@{s}'"),
        Tok::P(p) => format!("'{p}'"),
        Tok::Eof => "end of code".into(),
    }
}
