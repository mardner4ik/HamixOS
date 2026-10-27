#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, eprintln};
use hxclient::ui::{self, theme, Hits, Ui};
use hxclient::{Event, Window, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

const INITIAL_W: i32 = 340;
const INITIAL_H: i32 = 500;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}

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

struct App {
    window: Window,
    ui: Ui,
    expr: String,
    result: String,
    error: bool,
    history: Vec<String>,
    hits: Hits<usize>,
    hover: Option<usize>,
    pressed: Option<usize>,
}

impl App {
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

    fn draw(&mut self) {
        let ui = &self.ui;
        let buffer = self.window.buffer();
        let mut p = Painter::new(buffer, ww(), hh());
        let hits = &mut self.hits;
        hits.clear();
        p.fill(Area::new(0, 0, ww(), hh()), theme::bg());
        let display = Area::new(16, 16, ww() - 32, 138);
        ui::card(&mut p, display);
        let mut y = display.y + 10;
        for line in self.history.iter().rev().take(2).collect::<Vec<_>>().iter().rev() {
            let text = ellipsize(&ui.small, line, display.w - 28);
            let tw = ui.small.measure(&text);
            p.text(&ui.small, display.right() - 14 - tw, y, &text, theme::faint());
            y += 16;
        }
        let shown = if self.expr.is_empty() { String::from("0") } else { self.expr.clone() };
        let font = if ui.big.measure(&shown) < display.w - 28 { &ui.big } else { &ui.title };
        let text = ellipsize(font, &shown, display.w - 28);
        let tw = font.measure(&text);
        p.text(font, display.right() - 14 - tw, display.y + 58, &text, theme::text());
        let rw = ui.font.measure(&self.result);
        p.text(&ui.font, display.right() - 14 - rw, display.bottom() - 26, &self.result, if self.error { theme::danger() } else { theme::dim() });

        let grid_top = display.bottom() + 16;
        let (bw, bh, gap) = ((ww() - 32 - 3 * 10) / 4, (hh() - grid_top - 16 - 4 * 10) / 5, 10);
        for (i, key) in KEYS.iter().enumerate() {
            let area = Area::new(16 + (i as i32 % 4) * (bw + gap), grid_top + (i as i32 / 4) * (bh + gap), bw, bh);
            let operator = i % 4 == 3 || i < 3;
            let equals = *key == "=";
            let hot = self.hover == Some(i);
            let down = self.pressed == Some(i);
            let fill = if equals {
                if hot { theme::accent_hover() } else { theme::accent() }
            } else if operator {
                if hot { theme::pressed() } else { theme::surface_2() }
            } else if hot {
                theme::hover()
            } else {
                theme::surface()
            };
            p.rounded(if down { area.inset(1) } else { area }, 12, fill, 255);
            let font = &ui.title;
            ui::centered_text(&mut p, font, area, key, if equals { 0xffffff } else if operator { theme::accent_hover() } else { theme::text() });
            hits.add(area, i);
        }
        self.window.present();
    }
}

fn main() -> i32 {
    let ui = Ui::load();
    let window = match Window::open("Calculator", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxcalc: {}", e);
            return 1;
        }
    };
    let mut app = App { window, ui, expr: String::new(), result: String::new(), error: false, history: Vec::new(), hits: Hits::new(), hover: None, pressed: None };
    app.window.set_icon("calculator");
    app.window.set_size_hints(300, 440, 640, 900);
    app.draw();
    loop {
        match app.window.wait_event(-1) {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Resize { .. }) | Some(Event::Theme { .. }) => app.draw(),
            Some(Event::Mouse { x, y, kind, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    let hover = if kind == MOUSE_LEAVE { None } else { app.hits.at(x, y) };
                    if hover != app.hover {
                        app.hover = hover;
                        app.draw();
                    }
                }
                MOUSE_PRESS => {
                    if let Some(i) = app.hits.at(x, y) {
                        app.press(KEYS[i]);
                        app.pressed = Some(i);
                        app.draw();
                        app.pressed = None;
                        app.draw();
                    }
                }
                _ => {}
            },
            Some(Event::Key { code, .. }) => {
                let key = match code {
                    c if (b'0' as i32..=b'9' as i32).contains(&c) => Some(String::from(c as u8 as char)),
                    43 => Some(String::from("+")),
                    45 => Some(String::from("−")),
                    42 | 120 => Some(String::from("×")),
                    47 => Some(String::from("÷")),
                    40 => Some(String::from("(")),
                    41 => Some(String::from(")")),
                    46 | 44 => Some(String::from(".")),
                    37 => Some(String::from("%")),
                    10 | 61 => Some(String::from("=")),
                    8 => Some(String::from("back")),
                    27 | 99 => Some(String::from("C")),
                    _ => None,
                };
                if let Some(k) = key {
                    app.press(&k);
                    app.draw();
                }
            }
            _ => {}
        }
    }
}

entry!(main);
