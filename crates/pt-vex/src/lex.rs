use crate::Diag;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(f32),
    Str(String),
    Ident(String),
    At(String),
    P(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

const PUNCT: &[&str] = &[
    "+=", "-=", "*=", "/=", "==", "!=", "<=", ">=", "&&", "||", "..", "(", ")", "{", "}", "[", "]", ",", ";", ".", "+", "-", "*", "/",
    "=", "<", ">", "!",
];

pub fn lex(src: &str) -> Result<Vec<Token>, Diag> {
    let b: Vec<char> = src.chars().collect();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    let mut out = Vec::new();
    macro_rules! adv {
        () => {{
            if b[i] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            adv!();
            continue;
        }
        if c == '#' || (c == '/' && b.get(i + 1) == Some(&'/')) {
            while i < b.len() && b[i] != '\n' {
                adv!();
            }
            continue;
        }
        let (l0, c0) = (line, col);
        let push = |out: &mut Vec<Token>, tok| out.push(Token { tok, line: l0, col: c0 });
        if c.is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                adv!();
            }
            if i + 1 < b.len() && b[i] == '.' && b[i + 1].is_ascii_digit() {
                adv!();
                while i < b.len() && b[i].is_ascii_digit() {
                    adv!();
                }
            }
            if i < b.len() && (b[i] == 'e' || b[i] == 'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == '+' || b[j] == '-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    while i < j {
                        adv!();
                    }
                    while i < b.len() && b[i].is_ascii_digit() {
                        adv!();
                    }
                }
            }
            let text: String = b[s..i].iter().collect();
            let v: f32 = text.parse().map_err(|_| Diag::new(l0, c0, format!("bad number '{text}'"), "write numbers like 1.5 or 2e-3"))?;
            push(&mut out, Tok::Num(v));
        } else if c.is_alphabetic() || c == '_' {
            let s = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                adv!();
            }
            push(&mut out, Tok::Ident(b[s..i].iter().collect()));
        } else if c == '@' {
            adv!();
            let s = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                adv!();
            }
            if s == i {
                return Err(Diag::new(l0, c0, "'@' must be followed by an attribute name".into(), "e.g. @P, @Cd, @pscale"));
            }
            push(&mut out, Tok::At(b[s..i].iter().collect()));
        } else if c == '"' {
            adv!();
            let s = i;
            while i < b.len() && b[i] != '"' && b[i] != '\n' {
                adv!();
            }
            if i >= b.len() || b[i] != '"' {
                return Err(Diag::new(l0, c0, "unterminated string".into(), "close the string with a matching \""));
            }
            let text: String = b[s..i].iter().collect();
            adv!();
            push(&mut out, Tok::Str(text));
        } else {
            let rest: String = b[i..(i + 2).min(b.len())].iter().collect();
            let Some(p) = PUNCT.iter().find(|p| rest.starts_with(**p)) else {
                return Err(Diag::new(l0, c0, format!("unexpected character '{c}'"), "remove it or check for a typo"));
            };
            for _ in 0..p.len() {
                adv!();
            }
            push(&mut out, Tok::P(p));
        }
    }
    out.push(Token { tok: Tok::Eof, line, col });
    Ok(out)
}
