use crate::lex::{lex, Tok, Token};
use crate::Diag;

#[derive(Clone, Copy, Debug)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub struct E {
    pub k: Ek,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub name: Option<String>,
    pub value: E,
}

#[derive(Clone, Debug)]
pub enum Ek {
    Num(f64, bool),
    Str(String),
    Bool(bool),
    None,
    Name(String),
    List(Vec<E>),
    Tuple(Vec<E>),
    Dict(Vec<(E, E)>),
    Neg(Box<E>),
    Not(Box<E>),
    Bin(&'static str, Box<E>, Box<E>),
    And(Box<E>, Box<E>),
    Or(Box<E>, Box<E>),
    Call(Box<E>, Vec<Arg>),
    Index(Box<E>, Box<E>),
    IfExp(Box<E>, Box<E>, Box<E>), // (cond, then, else)
    ListComp { elem: Box<E>, var: String, iter: Box<E>, cond: Option<Box<E>> },
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Assign(Vec<String>, E, Pos),
    AugAssign(String, &'static str, E, Pos),
    Expr(E),
    If(Vec<(E, Vec<Stmt>)>, Vec<Stmt>),
    For(String, E, Vec<Stmt>, Pos),
    Def(String, Vec<(String, Option<E>)>, Vec<Stmt>, Pos),
    Return(Option<E>, Pos),
    Pass,
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
        if matches!(p.peek(), Tok::Newline) {
            p.next();
            continue;
        }
        out.push(p.stmt()?);
    }
    Ok(out)
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(v, _) => format!("number {v}"),
        Tok::Str(s) => format!("string \"{s}\""),
        Tok::Ident(s) => format!("'{s}'"),
        Tok::Op(o) => format!("'{o}'"),
        Tok::Newline => "end of line".into(),
        Tok::Indent => "an indent".into(),
        Tok::Dedent => "a dedent".into(),
        Tok::Eof => "end of code".into(),
    }
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
    fn is_op(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == s)
    }
    fn eat_op(&mut self, s: &str) -> bool {
        if self.is_op(s) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect_op(&mut self, s: &str) -> R<()> {
        if self.eat_op(s) {
            Ok(())
        } else {
            self.err(format!("expected '{s}' but found {}", describe(self.peek())), &format!("insert '{s}'"))
        }
    }
    fn is_kw(&self, s: &str) -> bool {
        matches!(self.peek(), Tok::Ident(i) if i == s)
    }
    fn eat_kw(&mut self, s: &str) -> bool {
        if self.is_kw(s) {
            self.next();
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> R<String> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.next();
                Ok(s)
            }
            t => self.err(format!("expected a name but found {}", describe(&t)), "use a letter-based name"),
        }
    }

    fn block(&mut self) -> R<Vec<Stmt>> {
        self.expect_op(":")?;
        if matches!(self.peek(), Tok::Newline) {
            self.next();
            if !matches!(self.peek(), Tok::Indent) {
                return self.err("expected an indented block".into(), "indent the lines under ':' by 4 spaces");
            }
            self.next();
            let mut v = Vec::new();
            while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
                if matches!(self.peek(), Tok::Newline) {
                    self.next();
                    continue;
                }
                v.push(self.stmt()?);
            }
            if matches!(self.peek(), Tok::Dedent) {
                self.next();
            }
            Ok(v)
        } else {
            let s = self.simple()?;
            Ok(vec![s])
        }
    }

    fn stmt(&mut self) -> R<Stmt> {
        let pos = self.pos();
        if self.eat_kw("def") {
            let name = self.ident()?;
            self.expect_op("(")?;
            let mut params = Vec::new();
            while !self.is_op(")") {
                let pn = self.ident()?;
                let def = if self.eat_op("=") { Some(self.expr()?) } else { None };
                params.push((pn, def));
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
            let body = self.block()?;
            return Ok(Stmt::Def(name, params, body, pos));
        }
        if self.eat_kw("if") {
            let mut arms = Vec::new();
            let c = self.expr()?;
            arms.push((c, self.block()?));
            let mut els = Vec::new();
            loop {
                if self.eat_kw("elif") {
                    let c = self.expr()?;
                    arms.push((c, self.block()?));
                } else if self.eat_kw("else") {
                    els = self.block()?;
                    break;
                } else {
                    break;
                }
            }
            return Ok(Stmt::If(arms, els));
        }
        if self.eat_kw("for") {
            let v = self.ident()?;
            if !self.eat_kw("in") {
                return self.err("expected 'in' after the loop variable".into(), "write: for i in range(5):");
            }
            let it = self.expr()?;
            let body = self.block()?;
            return Ok(Stmt::For(v, it, body, pos));
        }
        self.simple()
    }

    fn end_of_line(&mut self) -> R<()> {
        match self.peek() {
            Tok::Newline => {
                self.next();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            t => {
                let d = describe(t);
                self.err(format!("unexpected {d} after the statement"), "put one statement per line")
            }
        }
    }

    fn simple(&mut self) -> R<Stmt> {
        let pos = self.pos();
        if self.eat_kw("pass") {
            self.end_of_line()?;
            return Ok(Stmt::Pass);
        }
        if self.eat_kw("return") {
            let e = if matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Dedent) { None } else { Some(self.expr()?) };
            self.end_of_line()?;
            return Ok(Stmt::Return(e, pos));
        }
        // Assignment: NAME[, NAME...] = expr   |   NAME op= expr
        let save = self.i;
        if let Tok::Ident(first) = self.peek().clone() {
            self.next();
            let mut names = vec![first.clone()];
            while self.is_op(",") {
                self.next();
                match self.peek().clone() {
                    Tok::Ident(n) => {
                        self.next();
                        names.push(n);
                    }
                    _ => break,
                }
            }
            if self.is_op("=") {
                self.next();
                let mut e = self.expr()?;
                if self.is_op(",") {
                    let p = e.pos;
                    let mut items = vec![e];
                    while self.eat_op(",") {
                        items.push(self.expr()?);
                    }
                    e = E { k: Ek::Tuple(items), pos: p };
                }
                self.end_of_line()?;
                return Ok(Stmt::Assign(names, e, pos));
            }
            if names.len() == 1 {
                for op in ["+=", "-=", "*=", "/="] {
                    if self.is_op(op) {
                        self.next();
                        let e = self.expr()?;
                        self.end_of_line()?;
                        return Ok(Stmt::AugAssign(first, op, e, pos));
                    }
                }
            }
            self.i = save;
        }
        let e = self.expr()?;
        self.end_of_line()?;
        Ok(Stmt::Expr(e))
    }

    fn expr(&mut self) -> R<E> {
        let pos = self.pos();
        let e = self.or_expr()?;
        if self.eat_kw("if") {
            let c = self.or_expr()?;
            if !self.eat_kw("else") {
                return self.err("conditional expression needs 'else'".into(), "write: a if cond else b");
            }
            let o = self.expr()?;
            return Ok(E { k: Ek::IfExp(Box::new(c), Box::new(e), Box::new(o)), pos });
        }
        Ok(e)
    }

    fn or_expr(&mut self) -> R<E> {
        let mut l = self.and_expr()?;
        while self.is_kw("or") {
            let pos = self.pos();
            self.next();
            let r = self.and_expr()?;
            l = E { k: Ek::Or(Box::new(l), Box::new(r)), pos };
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> R<E> {
        let mut l = self.not_expr()?;
        while self.is_kw("and") {
            let pos = self.pos();
            self.next();
            let r = self.not_expr()?;
            l = E { k: Ek::And(Box::new(l), Box::new(r)), pos };
        }
        Ok(l)
    }

    fn not_expr(&mut self) -> R<E> {
        let pos = self.pos();
        if self.eat_kw("not") {
            return Ok(E { k: Ek::Not(Box::new(self.not_expr()?)), pos });
        }
        self.cmp()
    }

    fn cmp(&mut self) -> R<E> {
        let mut l = self.arith()?;
        loop {
            let pos = self.pos();
            let op = match self.peek() {
                Tok::Op(o @ ("==" | "!=" | "<" | ">" | "<=" | ">=")) => *o,
                _ => break,
            };
            self.next();
            let r = self.arith()?;
            l = E { k: Ek::Bin(op, Box::new(l), Box::new(r)), pos };
        }
        Ok(l)
    }

    fn arith(&mut self) -> R<E> {
        let mut l = self.term()?;
        loop {
            let pos = self.pos();
            let op = match self.peek() {
                Tok::Op(o @ ("+" | "-")) => *o,
                _ => break,
            };
            self.next();
            let r = self.term()?;
            l = E { k: Ek::Bin(op, Box::new(l), Box::new(r)), pos };
        }
        Ok(l)
    }

    fn term(&mut self) -> R<E> {
        let mut l = self.unary()?;
        loop {
            let pos = self.pos();
            let op = match self.peek() {
                Tok::Op(o @ ("*" | "/" | "//" | "%")) => *o,
                _ => break,
            };
            self.next();
            let r = self.unary()?;
            l = E { k: Ek::Bin(op, Box::new(l), Box::new(r)), pos };
        }
        Ok(l)
    }

    fn unary(&mut self) -> R<E> {
        let pos = self.pos();
        if self.eat_op("-") {
            return Ok(E { k: Ek::Neg(Box::new(self.unary()?)), pos });
        }
        if self.eat_op("+") {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> R<E> {
        let l = self.postfix()?;
        if self.is_op("**") {
            let pos = self.pos();
            self.next();
            let r = self.unary()?;
            return Ok(E { k: Ek::Bin("**", Box::new(l), Box::new(r)), pos });
        }
        Ok(l)
    }

    fn postfix(&mut self) -> R<E> {
        let mut e = self.atom()?;
        loop {
            let pos = self.pos();
            if self.eat_op("(") {
                let mut args = Vec::new();
                while !self.is_op(")") {
                    // keyword argument?
                    if let (Tok::Ident(n), Some(Token { tok: Tok::Op("="), .. })) = (self.peek().clone(), self.t.get(self.i + 1)) {
                        self.next();
                        self.next();
                        args.push(Arg { name: Some(n), value: self.expr()? });
                    } else {
                        args.push(Arg { name: None, value: self.expr()? });
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
                e = E { k: Ek::Call(Box::new(e), args), pos };
            } else if self.eat_op("[") {
                let i = self.expr()?;
                self.expect_op("]")?;
                e = E { k: Ek::Index(Box::new(e), Box::new(i)), pos };
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn atom(&mut self) -> R<E> {
        let pos = self.pos();
        match self.next() {
            Tok::Num(v, i) => Ok(E { k: Ek::Num(v, i), pos }),
            Tok::Str(s) => Ok(E { k: Ek::Str(s), pos }),
            Tok::Ident(n) => Ok(E {
                k: match n.as_str() {
                    "True" => Ek::Bool(true),
                    "False" => Ek::Bool(false),
                    "None" => Ek::None,
                    _ => Ek::Name(n),
                },
                pos,
            }),
            Tok::Op("(") => {
                if self.eat_op(")") {
                    return Ok(E { k: Ek::Tuple(vec![]), pos });
                }
                let first = self.expr()?;
                if self.is_op(",") {
                    let mut items = vec![first];
                    while self.eat_op(",") {
                        if self.is_op(")") {
                            break;
                        }
                        items.push(self.expr()?);
                    }
                    self.expect_op(")")?;
                    return Ok(E { k: Ek::Tuple(items), pos });
                }
                self.expect_op(")")?;
                Ok(first)
            }
            Tok::Op("[") => {
                if self.eat_op("]") {
                    return Ok(E { k: Ek::List(vec![]), pos });
                }
                let first = self.expr()?;
                if self.eat_kw("for") {
                    let var = self.ident()?;
                    if !self.eat_kw("in") {
                        return self.err("expected 'in' in the comprehension".into(), "write: [f(i) for i in range(5)]");
                    }
                    let iter = self.or_expr()?;
                    let cond = if self.eat_kw("if") { Some(Box::new(self.or_expr()?)) } else { None };
                    self.expect_op("]")?;
                    return Ok(E { k: Ek::ListComp { elem: Box::new(first), var, iter: Box::new(iter), cond }, pos });
                }
                let mut items = vec![first];
                while self.eat_op(",") {
                    if self.is_op("]") {
                        break;
                    }
                    items.push(self.expr()?);
                }
                self.expect_op("]")?;
                Ok(E { k: Ek::List(items), pos })
            }
            Tok::Op("{") => {
                let mut items = Vec::new();
                while !self.is_op("}") {
                    let k = self.expr()?;
                    self.expect_op(":")?;
                    let v = self.expr()?;
                    items.push((k, v));
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op("}")?;
                Ok(E { k: Ek::Dict(items), pos })
            }
            t => {
                self.i = self.i.saturating_sub(1);
                self.err(format!("expected a value but found {}", describe(&t)), "write a number, string, list, name or function call")
            }
        }
    }
}
