//! Indentation-aware lexer for the Python-syntax `hcs` language.

use crate::Diag;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(f64, bool),
    Str(String),
    Ident(String),
    Op(&'static str),
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

const OPS: &[&str] = &[
    "**", "//", "==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "(", ")", "[", "]", "{", "}", ",", ":", ".", "+", "-", "*", "/", "%", "=", "<", ">",
];

pub fn lex(src: &str) -> Result<Vec<Token>, Diag> {
    let flat = flatten_triple_quotes(src);
    let src = flat.as_str();
    let mut out: Vec<Token> = Vec::new();
    let mut indents = vec![0usize];
    let mut depth = 0i32; // bracket nesting: no Newline/Indent inside
    let lines: Vec<&str> = src.lines().collect();
    for (ln, raw) in lines.iter().enumerate() {
        let line = ln as u32 + 1;
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0usize;
        if depth == 0 {
            let mut col = 0usize;
            while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                col += if chars[i] == '\t' { 4 } else { 1 };
                i += 1;
            }
            if i >= chars.len() || chars[i] == '#' {
                continue; // blank or comment-only line
            }
            if col > *indents.last().unwrap() {
                indents.push(col);
                out.push(Token { tok: Tok::Indent, line, col: 1 });
            } else {
                while col < *indents.last().unwrap() {
                    indents.pop();
                    out.push(Token { tok: Tok::Dedent, line, col: 1 });
                }
                if col != *indents.last().unwrap() {
                    return Err(Diag::new(line, 1, "inconsistent indentation".into(), "indent blocks with the same number of spaces"));
                }
            }
        }
        while i < chars.len() {
            let c = chars[i];
            let col = i as u32 + 1;
            if c == ' ' || c == '\t' {
                i += 1;
            } else if c == '#' {
                break;
            } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
                let s = i;
                let mut is_int = true;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '.' && chars.get(i + 1).is_none_or(|d| *d != '.') {
                    is_int = false;
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        is_int = false;
                        i = j;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let text: String = chars[s..i].iter().collect();
                let v: f64 = text.parse().map_err(|_| Diag::new(line, col, format!("bad number '{text}'"), "write numbers like 1.5 or 2e-3"))?;
                out.push(Token { tok: Tok::Num(v, is_int), line, col });
            } else if c.is_alphabetic() || c == '_' {
                let s = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Token { tok: Tok::Ident(chars[s..i].iter().collect()), line, col });
            } else if c == '"' || c == '\'' {
                let q = c;
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != q {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                        s.push(match chars[i] { 'n' => '\n', 't' => '\t', o => o });
                    } else {
                        s.push(chars[i]);
                    }
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(Diag::new(line, col, "unterminated string".into(), "close the string with a matching quote"));
                }
                i += 1;
                out.push(Token { tok: Tok::Str(s), line, col });
            } else {
                let rest: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let Some(op) = OPS.iter().find(|o| rest.starts_with(**o)) else {
                    return Err(Diag::new(line, col, format!("unexpected character '{c}'"), "remove it or check for a typo"));
                };
                match *op {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth -= 1,
                    _ => {}
                }
                i += op.len();
                out.push(Token { tok: Tok::Op(op), line, col });
            }
        }
        if depth == 0 {
            out.push(Token { tok: Tok::Newline, line, col: chars.len() as u32 + 1 });
        }
    }
    let end = lines.len() as u32 + 1;
    if depth != 0 {
        return Err(Diag::new(end, 1, "unclosed bracket".into(), "add the missing ) ] or }"));
    }
    while indents.len() > 1 {
        indents.pop();
        out.push(Token { tok: Tok::Dedent, line: end, col: 1 });
    }
    out.push(Token { tok: Tok::Eof, line: end, col: 1 });
    Ok(out)
}

/// Rewrite `"""..."""` / `'''...'''` literals into single-line strings (newlines become \n),
/// re-emitting the removed newlines right after so line numbers stay correct.
fn flatten_triple_quotes(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        let q = chars[i];
        if (q == '"' || q == '\'') && chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q) {
            let mut j = i + 3;
            let mut body = String::new();
            let mut newlines = 0;
            while j + 2 < chars.len() + 0 && !(chars[j] == q && chars.get(j + 1) == Some(&q) && chars.get(j + 2) == Some(&q)) {
                match chars[j] {
                    '\n' => {
                        body.push_str("\\n");
                        newlines += 1;
                    }
                    '\\' => body.push_str("\\\\"),
                    '"' => body.push_str("\\\""),
                    c => body.push(c),
                }
                j += 1;
            }
            if j + 2 >= chars.len() + 0 && !(chars.get(j) == Some(&q) && chars.get(j + 1) == Some(&q) && chars.get(j + 2) == Some(&q)) {
                // Unterminated: leave as-is so the normal lexer reports it.
                out.push(chars[i]);
                i += 1;
                continue;
            }
            out.push('"');
            out.push_str(&body);
            out.push('"');
            for _ in 0..newlines {
                out.push('\n');
            }
            i = j + 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}
