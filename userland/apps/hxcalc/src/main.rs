#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{button, column, row, text, App, Align, Context, Element, Key, KeyPress, Length, Settings, TextSize, Tone};

const KEYS: [&str; 20] = ["C", "(", ")", "÷", "7", "8", "9", "×", "4", "5", "6", "−", "1", "2", "3", "+", "±", "0", ".", "="];

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    _src: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn expr(&mut self) -> Result<f64, &'static str> {
        let mut value = self.term()?;
        while let Some(c) = self.peek() {
            match c {
                '+' => {
                    self.pos += 1;
                    value += self.term()?;
                }
                '-' | '−' => {
                    self.pos += 1;
                    value -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<f64, &'static str> {
        let mut value = self.factor()?;
        while let Some(c) = self.peek() {
            match c {
                '*' | '×' => {
                    self.pos += 1;
                    value *= self.factor()?;
                }
                '/' | '÷' => {
                    self.pos += 1;
                    let d = self.factor()?;
                    if d == 0.0 {
                        return Err("Cannot divide by zero");
                    }
                    value /= d;
                }
                '%' => {
                    self.pos += 1;
                    value /= 100.0;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    fn factor(&mut self) -> Result<f64, &'static str> {
        match self.peek() {
            Some('-') | Some('−') => {
                self.pos += 1;
                Ok(-self.factor()?)
            }
            Some('(') => {
                self.pos += 1;
                let v = self.expr()?;
                if self.peek() == Some(')') {
                    self.pos += 1;
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() || c == '.' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                parse_number(&text).ok_or("Invalid number")
            }
            _ => Err("Incomplete expression"),
        }
    }
}

fn parse_number(text: &str) -> Option<f64> {
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in whole.chars() {
        value = value * 10.0 + c.to_digit(10)? as f64;
    }
    let mut scale = 0.1;
    for c in frac.chars() {
        value += c.to_digit(10)? as f64 * scale;
        scale /= 10.0;
    }
    Some(value)
}

fn evaluate(expr: &str) -> Result<f64, &'static str> {
    let mut parser = Parser { chars: expr.chars().filter(|c| !c.is_whitespace()).collect(), pos: 0, _src: expr };
    let value = parser.expr()?;
    if parser.pos != parser.chars.len() {
        return Err("Unexpected symbol");
    }
    if value.is_nan() || value.is_infinite() {
        return Err("Result is too large");
    }
    Ok(value)
}

fn format_number(value: f64) -> String {
    if value == (value as i64) as f64 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let mut text = format!("{:.10}", value);
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[derive(Default)]
struct Calculator {
    expr: String,
    result: String,
    error: bool,
    history: Vec<String>,
}

#[derive(Clone)]
enum Msg {
    Press(&'static str),
    Back,
}

impl Calculator {
    fn press(&mut self, key: &str) {
        if self.error {
            self.expr.clear();
            self.error = false;
        }
        match key {
            "C" => {
                self.expr.clear();
                self.result.clear();
            }
            "=" => match evaluate(&self.expr) {
                Ok(v) => {
                    let text = format_number(v);
                    self.history.push(format!("{} = {}", self.expr, text));
                    if self.history.len() > 4 {
                        self.history.remove(0);
                    }
                    self.result = String::new();
                    self.expr = text;
                }
                Err(e) => {
                    self.result = String::from(e);
                    self.error = true;
                }
            },
            "±" => {
                if self.expr.starts_with('−') {
                    self.expr.remove(0);
                } else {
                    self.expr.insert(0, '−');
                }
            }
            "back" => {
                self.expr.pop();
            }
            other => self.expr.push_str(other),
        }
        if !self.error && key != "=" {
            self.result = match evaluate(&self.expr) {
                Ok(v) if !self.expr.is_empty() && self.expr.chars().any(|c| "+−×÷()%".contains(c)) => format!("= {}", format_number(v)),
                _ => String::new(),
            };
        }
    }
}

impl App for Calculator {
    type Message = Msg;

    fn title(&self) -> String {
        String::from("Calculator")
    }

    fn view(&self) -> Element<Msg> {
        let mut display = column![].spacing(4).align(Align::End).card();
        for line in self.history.iter().rev().take(2).collect::<Vec<_>>().into_iter().rev() {
            display = display.push(text(line.clone()).small().tone(Tone::Faint));
        }
        let shown = if self.expr.is_empty() { String::from("0") } else { self.expr.clone() };
        display = display.push(text(shown).size(TextSize::Display)).push(text(self.result.clone()).tone(if self.error { Tone::Danger } else { Tone::Dim }));
        let mut keypad = column![].spacing(10).height(Length::Fill);
        for chunk in KEYS.chunks(4) {
            let mut line = row![].spacing(10).height(Length::Fill).align(Align::Stretch);
            for key in chunk.iter() {
                let b = button(*key).on_press(Msg::Press(key)).width(Length::Fill).height(Length::Fill).size(TextSize::Title);
                line = line.push(if *key == "=" { b.primary() } else { b });
            }
            keypad = keypad.push(line);
        }
        column![display, keypad].padding(16).spacing(16).fill().into()
    }

    fn update(&mut self, message: Msg, _cx: &mut Context<Msg>) {
        match message {
            Msg::Press(key) => self.press(key),
            Msg::Back => self.press("back"),
        }
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        if press.mods.ctrl || press.mods.alt {
            return None;
        }
        Some(match press.key {
            Key::Char(c) if c.is_ascii_digit() => Msg::Press(KEYS.iter().find(|k| k.starts_with(c)).copied()?),
            Key::Char('+') => Msg::Press("+"),
            Key::Char('-') => Msg::Press("−"),
            Key::Char('*') | Key::Char('x') => Msg::Press("×"),
            Key::Char('/') => Msg::Press("÷"),
            Key::Char('(') => Msg::Press("("),
            Key::Char(')') => Msg::Press(")"),
            Key::Char('.') | Key::Char(',') => Msg::Press("."),
            Key::Char('%') => Msg::Press("%"),
            Key::Enter | Key::Char('=') => Msg::Press("="),
            Key::Backspace => Msg::Back,
            Key::Escape | Key::Char('c') => Msg::Press("C"),
            _ => return None,
        })
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("calculator").size(340, 500).min_size(300, 440), Calculator::default())
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
