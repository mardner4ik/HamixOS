use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub enum Part {
    Lit(String, bool),
    Var(String, bool),
    Sub(String, bool),
    Tilde(String),
}

#[derive(Clone, Debug)]
pub struct Word {
    pub parts: Vec<Part>,
}

impl Word {
    pub fn plain(&self) -> Option<String> {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Lit(text, false) => out.push_str(text),
                _ => return None,
            }
        }
        Some(out)
    }

    pub fn literal(text: &str) -> Word {
        Word { parts: alloc::vec![Part::Lit(String::from(text), true)] }
    }
}

#[derive(Clone, Debug)]
pub enum Tok {
    Word(Word),
    Op(&'static str),
    Newline,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

const OPERATORS: [&str; 15] = ["2>&1", "&>", "2>>", "2>", ">>", "&&", "||", ";;", ">", "<", "|", "&", ";", "(", ")"];

fn is_var_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let byte_at = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(src.len());

    while i < chars.len() {
        let (pos, c) = chars[i];
        if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
            continue;
        }
        if c == '\\' && chars.get(i + 1).map(|c| c.1) == Some('\n') {
            i += 2;
            continue;
        }
        if c == '\n' {
            tokens.push(Token { tok: Tok::Newline, start: pos, end: pos + 1 });
            i += 1;
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i].1 != '\n' {
                i += 1;
            }
            continue;
        }
        let rest = &src[pos..];
        if let Some(op) = OPERATORS.iter().find(|op| rest.starts_with(**op)) {
            let word_boundary = !(op.starts_with('2')) || tokens.last().map(|t| t.end < pos).unwrap_or(true);
            if word_boundary {
                tokens.push(Token { tok: Tok::Op(op), start: pos, end: pos + op.len() });
                i += op.chars().count();
                continue;
            }
        }

        let start = pos;
        let mut parts: Vec<Part> = Vec::new();
        let mut lit = String::new();
        let mut first = true;
        let flush = |lit: &mut String, parts: &mut Vec<Part>, quoted: bool| {
            if !lit.is_empty() {
                parts.push(Part::Lit(core::mem::take(lit), quoted));
            }
        };
        while i < chars.len() {
            let c = chars[i].1;
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                break;
            }
            let rest = &src[chars[i].0..];
            if OPERATORS.iter().any(|op| !op.starts_with('2') && rest.starts_with(*op)) {
                break;
            }
            match c {
                '~' if first => {
                    let mut name = String::new();
                    i += 1;
                    while i < chars.len() && is_var_char(chars[i].1) {
                        name.push(chars[i].1);
                        i += 1;
                    }
                    parts.push(Part::Tilde(name));
                }
                '\\' => {
                    if let Some(&(_, next)) = chars.get(i + 1) {
                        flush(&mut lit, &mut parts, false);
                        parts.push(Part::Lit(String::from(next), true));
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                '\'' => {
                    flush(&mut lit, &mut parts, false);
                    let mut text = String::new();
                    i += 1;
                    while i < chars.len() && chars[i].1 != '\'' {
                        text.push(chars[i].1);
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(String::from("unterminated single quote"));
                    }
                    i += 1;
                    parts.push(Part::Lit(text, true));
                }
                '"' => {
                    flush(&mut lit, &mut parts, false);
                    i += 1;
                    let mut text = String::new();
                    let mut closed = false;
                    while i < chars.len() {
                        let d = chars[i].1;
                        if d == '"' {
                            closed = true;
                            i += 1;
                            break;
                        }
                        if d == '\\' {
                            if let Some(&(_, next)) = chars.get(i + 1) {
                                if matches!(next, '"' | '\\' | '$' | '`') {
                                    text.push(next);
                                } else if next == 'n' {
                                    text.push('\\');
                                    text.push('n');
                                } else {
                                    text.push('\\');
                                    text.push(next);
                                }
                                i += 2;
                                continue;
                            }
                        }
                        if d == '$' {
                            let (part, consumed) = dollar(&chars, i, src, true)?;
                            if let Some(part) = part {
                                if !text.is_empty() {
                                    parts.push(Part::Lit(core::mem::take(&mut text), true));
                                }
                                parts.push(part);
                                i += consumed;
                                continue;
                            }
                        }
                        text.push(d);
                        i += 1;
                    }
                    if !closed {
                        return Err(String::from("unterminated double quote"));
                    }
                    parts.push(Part::Lit(text, true));
                }
                '$' => {
                    let (part, consumed) = dollar(&chars, i, src, false)?;
                    match part {
                        Some(part) => {
                            flush(&mut lit, &mut parts, false);
                            parts.push(part);
                            i += consumed;
                        }
                        None => {
                            lit.push('$');
                            i += 1;
                        }
                    }
                }
                _ => {
                    lit.push(c);
                    i += 1;
                }
            }
            first = false;
        }
        flush(&mut lit, &mut parts, false);
        tokens.push(Token { tok: Tok::Word(Word { parts }), start, end: byte_at(i) });
    }
    Ok(tokens)
}

fn dollar(chars: &[(usize, char)], i: usize, src: &str, quoted: bool) -> Result<(Option<Part>, usize), String> {
    let next = chars.get(i + 1).map(|c| c.1);
    match next {
        Some('{') => {
            let mut name = String::new();
            let mut j = i + 2;
            while j < chars.len() && chars[j].1 != '}' {
                name.push(chars[j].1);
                j += 1;
            }
            if j >= chars.len() {
                return Err(String::from("unterminated ${"));
            }
            Ok((Some(Part::Var(name, quoted)), j + 1 - i))
        }
        Some('(') => {
            let mut depth = 1;
            let mut j = i + 2;
            while j < chars.len() {
                match chars[j].1 {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if j >= chars.len() {
                return Err(String::from("unterminated $("));
            }
            let inner = &src[chars[i + 2].0..chars[j].0];
            Ok((Some(Part::Sub(String::from(inner), quoted)), j + 1 - i))
        }
        Some(c) if c == '?' || c == '#' || c == '@' || c == '*' || c == '$' || c == '!' || c.is_ascii_digit() => {
            Ok((Some(Part::Var(String::from(c), quoted)), 2))
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            let mut name = String::new();
            let mut j = i + 1;
            while j < chars.len() && is_var_char(chars[j].1) {
                name.push(chars[j].1);
                j += 1;
            }
            Ok((Some(Part::Var(name, quoted)), j - i))
        }
        _ => Ok((None, 1)),
    }
}
