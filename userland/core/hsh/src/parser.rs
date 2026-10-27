use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::lexer::{Tok, Token, Word};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirKind {
    In,
    Out,
    Append,
    ErrOut,
    ErrAppend,
    ErrToOut,
    AllOut,
}

#[derive(Clone, Debug)]
pub struct Redir {
    pub kind: RedirKind,
    pub target: Option<Word>,
}

#[derive(Clone, Debug)]
pub enum Node {
    Simple { assigns: Vec<(String, Word)>, words: Vec<Word>, redirs: Vec<Redir>, src: String },
    Pipeline(Vec<Node>, bool),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    List(Vec<(Node, bool)>),
    If { branches: Vec<(Node, Node)>, otherwise: Option<Box<Node>>, src: String },
    Loop { cond: Box<Node>, body: Box<Node>, until: bool, src: String },
    For { var: String, items: Vec<Word>, body: Box<Node>, src: String },
    Group(Box<Node>, Vec<Redir>, String),
    Function(String, Box<Node>),
}

impl Node {
    pub fn source(&self) -> String {
        match self {
            Node::Simple { src, .. } | Node::If { src, .. } | Node::Loop { src, .. } | Node::For { src, .. } | Node::Group(_, _, src) => src.clone(),
            _ => String::new(),
        }
    }
}

pub struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    src: &'a str,
}

const KEYWORDS: [&str; 12] = ["if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "in", "}"];

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<Token>, src: &'a str) -> Self {
        Self { tokens, pos: 0, src }
    }

    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    fn peek_word(&self) -> Option<String> {
        match self.peek() {
            Some(Tok::Word(w)) => w.plain(),
            _ => None,
        }
    }

    fn is_keyword(&self, kw: &str) -> bool {
        self.peek_word().as_deref() == Some(kw)
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) || self.is_op(";") {
            self.pos += 1;
        }
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), String> {
        self.skip_newlines();
        if self.is_keyword(kw) {
            self.pos += 1;
            Ok(())
        } else {
            Err(alloc::format!("syntax error: expected '{}'", kw))
        }
    }

    fn start_offset(&self) -> usize {
        self.tokens.get(self.pos).map(|t| t.start).unwrap_or(self.src.len())
    }

    fn prev_end(&self) -> usize {
        if self.pos == 0 { 0 } else { self.tokens[self.pos - 1].end }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub fn parse_program(&mut self) -> Result<Node, String> {
        let list = self.parse_list(&[])?;
        self.skip_newlines();
        if !self.at_end() {
            return Err(String::from("syntax error near unexpected token"));
        }
        Ok(list)
    }

    fn parse_list(&mut self, terminators: &[&str]) -> Result<Node, String> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_end() {
                break;
            }
            if let Some(word) = self.peek_word() {
                if terminators.contains(&word.as_str()) {
                    break;
                }
            }
            if self.is_op(")") && terminators.contains(&")") {
                break;
            }
            let node = self.parse_and_or()?;
            let mut background = false;
            if self.is_op("&") {
                background = true;
                self.pos += 1;
            } else if self.is_op(";") || matches!(self.peek(), Some(Tok::Newline)) {
                self.pos += 1;
            } else if !self.at_end() {
                let word = self.peek_word();
                let terminated = word.map(|w| terminators.contains(&w.as_str())).unwrap_or(false) || (self.is_op(")") && terminators.contains(&")"));
                if !terminated {
                    return Err(String::from("syntax error near unexpected token"));
                }
            }
            items.push((node, background));
        }
        Ok(Node::List(items))
    }

    fn parse_and_or(&mut self) -> Result<Node, String> {
        let mut left = self.parse_pipeline()?;
        loop {
            if self.is_op("&&") {
                self.pos += 1;
                self.skip_only_newlines();
                let right = self.parse_pipeline()?;
                left = Node::And(Box::new(left), Box::new(right));
            } else if self.is_op("||") {
                self.pos += 1;
                self.skip_only_newlines();
                let right = self.parse_pipeline()?;
                left = Node::Or(Box::new(left), Box::new(right));
            } else {
                return Ok(left);
            }
        }
    }

    fn skip_only_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.pos += 1;
        }
    }

    fn parse_pipeline(&mut self) -> Result<Node, String> {
        let mut negate = false;
        if self.is_keyword("!") {
            negate = true;
            self.pos += 1;
        }
        let mut stages = alloc::vec![self.parse_command()?];
        while self.is_op("|") {
            self.pos += 1;
            self.skip_only_newlines();
            stages.push(self.parse_command()?);
        }
        if stages.len() == 1 && !negate {
            return Ok(stages.pop().unwrap());
        }
        Ok(Node::Pipeline(stages, negate))
    }

    fn parse_redirs(&mut self, redirs: &mut Vec<Redir>) -> Result<bool, String> {
        let kind = match self.peek() {
            Some(Tok::Op("<")) => RedirKind::In,
            Some(Tok::Op(">")) => RedirKind::Out,
            Some(Tok::Op(">>")) => RedirKind::Append,
            Some(Tok::Op("2>")) => RedirKind::ErrOut,
            Some(Tok::Op("2>>")) => RedirKind::ErrAppend,
            Some(Tok::Op("&>")) => RedirKind::AllOut,
            Some(Tok::Op("2>&1")) => {
                self.pos += 1;
                redirs.push(Redir { kind: RedirKind::ErrToOut, target: None });
                return Ok(true);
            }
            _ => return Ok(false),
        };
        self.pos += 1;
        match self.tokens.get(self.pos).map(|t| t.tok.clone()) {
            Some(Tok::Word(w)) => {
                self.pos += 1;
                redirs.push(Redir { kind, target: Some(w) });
                Ok(true)
            }
            _ => Err(String::from("syntax error: redirection without a target")),
        }
    }

    fn parse_command(&mut self) -> Result<Node, String> {
        self.skip_only_newlines();
        let start = self.start_offset();
        match self.peek_word().as_deref() {
            Some("if") => {
                self.pos += 1;
                let mut branches = Vec::new();
                let cond = self.parse_list(&["then"])?;
                self.expect_keyword("then")?;
                let body = self.parse_list(&["elif", "else", "fi"])?;
                branches.push((cond, body));
                let mut otherwise = None;
                loop {
                    self.skip_newlines();
                    match self.peek_word().as_deref() {
                        Some("elif") => {
                            self.pos += 1;
                            let cond = self.parse_list(&["then"])?;
                            self.expect_keyword("then")?;
                            let body = self.parse_list(&["elif", "else", "fi"])?;
                            branches.push((cond, body));
                        }
                        Some("else") => {
                            self.pos += 1;
                            otherwise = Some(Box::new(self.parse_list(&["fi"])?));
                        }
                        Some("fi") => {
                            self.pos += 1;
                            break;
                        }
                        _ => return Err(String::from("syntax error: expected 'fi'")),
                    }
                }
                let src = String::from(&self.src[start..self.prev_end()]);
                return Ok(Node::If { branches, otherwise, src });
            }
            Some(kw @ ("while" | "until")) => {
                let until = kw == "until";
                self.pos += 1;
                let cond = self.parse_list(&["do"])?;
                self.expect_keyword("do")?;
                let body = self.parse_list(&["done"])?;
                self.expect_keyword("done")?;
                let src = String::from(&self.src[start..self.prev_end()]);
                return Ok(Node::Loop { cond: Box::new(cond), body: Box::new(body), until, src });
            }
            Some("for") => {
                self.pos += 1;
                let var = self.peek_word().ok_or_else(|| String::from("syntax error: for needs a variable name"))?;
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_only_newlines();
                if self.is_keyword("in") {
                    self.pos += 1;
                    while let Some(Tok::Word(w)) = self.peek().cloned() {
                        if w.plain().as_deref() == Some("do") {
                            break;
                        }
                        items.push(w);
                        self.pos += 1;
                    }
                } else {
                    items.push(Word { parts: alloc::vec![crate::lexer::Part::Var(String::from("@"), true)] });
                }
                self.expect_keyword("do")?;
                let body = self.parse_list(&["done"])?;
                self.expect_keyword("done")?;
                let src = String::from(&self.src[start..self.prev_end()]);
                return Ok(Node::For { var, items, body: Box::new(body), src });
            }
            Some("{") => {
                self.pos += 1;
                let body = self.parse_list(&["}"])?;
                self.expect_keyword("}")?;
                let mut redirs = Vec::new();
                while self.parse_redirs(&mut redirs)? {}
                let src = String::from(&self.src[start..self.prev_end()]);
                return Ok(Node::Group(Box::new(body), redirs, src));
            }
            _ => {}
        }
        if self.is_op("(") {
            self.pos += 1;
            let body = self.parse_list(&[")"])?;
            if !self.is_op(")") {
                return Err(String::from("syntax error: expected ')'"));
            }
            self.pos += 1;
            let mut redirs = Vec::new();
            while self.parse_redirs(&mut redirs)? {}
            let src = String::from(&self.src[start..self.prev_end()]);
            return Ok(Node::Group(Box::new(body), redirs, src));
        }

        let mut assigns = Vec::new();
        let mut words = Vec::new();
        let mut redirs = Vec::new();
        loop {
            if self.parse_redirs(&mut redirs)? {
                continue;
            }
            let Some(Tok::Word(word)) = self.peek().cloned() else {
                break;
            };
            if words.is_empty() {
                if let Some(crate::lexer::Part::Lit(text, false)) = word.parts.first() {
                    if let Some(eq) = text.find('=') {
                        let name = &text[..eq];
                        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit()) {
                            let mut value = word.clone();
                            if let Some(crate::lexer::Part::Lit(first, _)) = value.parts.first_mut() {
                                *first = String::from(&text[eq + 1..]);
                            }
                            assigns.push((String::from(name), value));
                            self.pos += 1;
                            continue;
                        }
                    }
                }
                if let Some(plain) = word.plain() {
                    if KEYWORDS.contains(&plain.as_str()) && plain != "in" {
                        break;
                    }
                }
            }
            words.push(word);
            self.pos += 1;
        }
        if words.len() == 1 && self.is_op("(") && matches!(self.tokens.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Op(")"))) {
            let name = words[0].plain().ok_or_else(|| String::from("syntax error: bad function name"))?;
            self.pos += 2;
            self.skip_only_newlines();
            let body = self.parse_command()?;
            return Ok(Node::Function(name, Box::new(body)));
        }
        if words.is_empty() && assigns.is_empty() && redirs.is_empty() {
            return Err(String::from("syntax error near unexpected token"));
        }
        let src = String::from(&self.src[start..self.prev_end()]);
        Ok(Node::Simple { assigns, words, redirs, src })
    }
}

pub fn parse(src: &str) -> Result<Node, String> {
    let tokens = crate::lexer::tokenize(src)?;
    Parser::new(tokens, src).parse_program()
}

pub fn needs_more(src: &str) -> bool {
    match parse(src) {
        Ok(_) => false,
        Err(e) => e.starts_with("unterminated") || e.contains("expected 'fi'") || e.contains("expected 'done'") || e.contains("expected '}'") || e.contains("expected 'then'") || e.contains("expected 'do'"),
    }
}
