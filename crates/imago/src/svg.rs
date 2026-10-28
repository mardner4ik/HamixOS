use alloc::string::String;
use alloc::vec::Vec;

use crate::{check_size, Bitmap, Error, Result};

const SUBSAMPLES: usize = 5;
const DEFAULT_SIZE: f32 = 64.0;

pub fn looks_like(data: &[u8]) -> bool {
    let head = &data[..data.len().min(4096)];
    let text = core::str::from_utf8(head).unwrap_or("");
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    (trimmed.starts_with("<?xml") || trimmed.starts_with("<svg") || trimmed.starts_with("<!--") || trimmed.starts_with("<!DOCTYPE svg")) && text.contains("<svg")
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix([f32; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn mul(&self, o: &Matrix) -> Matrix {
        let (a, b) = (&self.0, &o.0);
        Matrix([
            a[0] * b[0] + a[2] * b[1],
            a[1] * b[0] + a[3] * b[1],
            a[0] * b[2] + a[2] * b[3],
            a[1] * b[2] + a[3] * b[3],
            a[0] * b[4] + a[2] * b[5] + a[4],
            a[1] * b[4] + a[3] * b[5] + a[5],
        ])
    }

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.0;
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    fn translate(x: f32, y: f32) -> Matrix {
        Matrix([1.0, 0.0, 0.0, 1.0, x, y])
    }

    fn scale(x: f32, y: f32) -> Matrix {
        Matrix([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    fn inverse(&self) -> Option<Matrix> {
        let m = &self.0;
        let det = m[0] * m[3] - m[1] * m[2];
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        Some(Matrix([m[3] * inv, -m[1] * inv, -m[2] * inv, m[0] * inv, (m[2] * m[5] - m[3] * m[4]) * inv, (m[1] * m[4] - m[0] * m[5]) * inv]))
    }

    fn expansion(&self) -> f32 {
        let m = &self.0;
        sqrt((m[0] * m[3] - m[1] * m[2]).abs()).max(1e-3)
    }
}

fn sqrt(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut x = if v > 1.0 { v } else { 1.0 };
    for _ in 0..20 {
        x = 0.5 * (x + v / x);
    }
    x
}

fn floor(v: f32) -> f32 {
    let t = v as i64 as f32;
    if t > v { t - 1.0 } else { t }
}

fn ceil(v: f32) -> f32 {
    let t = v as i64 as f32;
    if t < v { t + 1.0 } else { t }
}

const PI: f32 = core::f32::consts::PI;

fn sin(x: f32) -> f32 {
    let mut x = x % (2.0 * PI);
    if x > PI {
        x -= 2.0 * PI;
    } else if x < -PI {
        x += 2.0 * PI;
    }
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
}

fn cos(x: f32) -> f32 {
    sin(x + PI / 2.0)
}

fn atan2(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let ax = x.abs();
    let ay = y.abs();
    let (a, swap) = if ax >= ay { (ay / ax, false) } else { (ax / ay, true) };
    let s = a * a;
    let mut r = ((-0.046_496_474 * s + 0.159_314_22) * s - 0.327_622_77) * s * a + a;
    if swap {
        r = PI / 2.0 - r;
    }
    if x < 0.0 {
        r = PI - r;
    }
    if y < 0.0 { -r } else { r }
}

struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<usize>,
    text: String,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().rev().find(|(k, _)| k == key || k.rsplit(':').next() == Some(key) && key == "href").map(|(_, v)| v.as_str())
    }
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return String::from(s);
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').unwrap_or(0);
        let name = &tail[1..end.max(1)];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
            _ if name.starts_with('#') => name[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) if end > 0 => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_xml(text: &str) -> Result<Vec<Node>> {
    let mut nodes: Vec<Node> = alloc::vec![Node { name: String::from("#root"), attrs: Vec::new(), children: Vec::new(), text: String::new() }];
    let mut stack: Vec<usize> = alloc::vec![0];
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let end = text[i..].find('<').map(|e| i + e).unwrap_or(b.len());
            let parent = *stack.last().unwrap();
            nodes[parent].text.push_str(&decode_entities(&text[i..end]));
            i = end;
            continue;
        }
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").unwrap_or(rest.len());
            let parent = *stack.last().unwrap();
            nodes[parent].text.push_str(&rest[9..end.max(9)]);
            i += (end + 3).min(rest.len());
            continue;
        }
        if rest.starts_with("<!") {
            let mut depth = 0i32;
            let mut j = 0;
            for (k, c) in rest.bytes().enumerate() {
                match c {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => {
                        j = k + 1;
                        break;
                    }
                    _ => {}
                }
            }
            i += if j == 0 { rest.len() } else { j };
            continue;
        }
        if rest.starts_with("<?") {
            i += rest.find("?>").map(|e| e + 2).unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("</") {
            let end = rest.find('>').ok_or(Error::Truncated)?;
            if stack.len() > 1 {
                stack.pop();
            }
            i += end + 1;
            continue;
        }
        let mut j = 1;
        while j < rest.len() && !matches!(rest.as_bytes()[j], b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>') {
            j += 1;
        }
        let name = String::from(rest[1..j].rsplit(':').next().unwrap_or(""));
        let mut attrs = Vec::new();
        let rb = rest.as_bytes();
        let mut closed = false;
        loop {
            while j < rb.len() && rb[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= rb.len() {
                return Err(Error::Truncated);
            }
            if rb[j] == b'>' {
                j += 1;
                break;
            }
            if rb[j] == b'/' {
                closed = true;
                j += 1;
                continue;
            }
            let k0 = j;
            while j < rb.len() && !matches!(rb[j], b'=' | b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') {
                j += 1;
            }
            let key = &rest[k0..j];
            while j < rb.len() && rb[j].is_ascii_whitespace() {
                j += 1;
            }
            let mut value = String::new();
            if j < rb.len() && rb[j] == b'=' {
                j += 1;
                while j < rb.len() && rb[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < rb.len() && (rb[j] == b'"' || rb[j] == b'\'') {
                    let q = rb[j];
                    let v0 = j + 1;
                    j = v0;
                    while j < rb.len() && rb[j] != q {
                        j += 1;
                    }
                    value = decode_entities(&rest[v0..j.min(rest.len())]);
                    j += 1;
                } else {
                    let v0 = j;
                    while j < rb.len() && !rb[j].is_ascii_whitespace() && rb[j] != b'>' {
                        j += 1;
                    }
                    value = String::from(&rest[v0..j]);
                }
            }
            if !key.is_empty() {
                attrs.push((String::from(key), value));
            }
        }
        let index = nodes.len();
        nodes.push(Node { name, attrs, children: Vec::new(), text: String::new() });
        let parent = *stack.last().unwrap();
        nodes[parent].children.push(index);
        if !closed {
            stack.push(index);
        }
        i += j;
    }
    Ok(nodes)
}

fn number_prefix(s: &str) -> Option<(f32, &str)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
        i += 1;
    }
    let start = i;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = false;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits = true;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits = true;
        }
    }
    if !digits {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') && i + 1 < b.len() && (b[i + 1].is_ascii_digit() || ((b[i + 1] == b'-' || b[i + 1] == b'+') && i + 2 < b.len() && b[i + 2].is_ascii_digit())) {
        i += 2;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    let v = parse_float(&s[start..i])?;
    Some((v, &s[i..]))
}

fn parse_float(s: &str) -> Option<f32> {
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], s[i + 1..].parse::<i32>().ok()?),
        None => (s, 0),
    };
    let (neg, m) = match mantissa.as_bytes().first() {
        Some(b'-') => (true, &mantissa[1..]),
        Some(b'+') => (false, &mantissa[1..]),
        _ => (false, mantissa),
    };
    let mut v = 0f64;
    let mut scale = 0i32;
    let mut frac = false;
    for c in m.bytes() {
        match c {
            b'0'..=b'9' => {
                v = v * 10.0 + (c - b'0') as f64;
                if frac {
                    scale -= 1;
                }
            }
            b'.' => frac = true,
            _ => return None,
        }
    }
    let mut e = scale + exponent;
    while e > 0 {
        v *= 10.0;
        e -= 1;
    }
    while e < 0 {
        v /= 10.0;
        e += 1;
    }
    Some(if neg { -v as f32 } else { v as f32 })
}

fn numbers(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((v, r)) = number_prefix(rest) {
        out.push(v);
        rest = r;
    }
    out
}

fn length(s: &str, reference: f32) -> Option<f32> {
    let s = s.trim();
    let (v, unit) = number_prefix(s)?;
    let unit = unit.trim();
    Some(match unit {
        "" | "px" => v,
        "pt" => v * 4.0 / 3.0,
        "pc" => v * 16.0,
        "mm" => v * 3.779_528,
        "cm" => v * 37.795_28,
        "in" => v * 96.0,
        "em" => v * 16.0,
        "ex" => v * 8.0,
        "%" => v * reference / 100.0,
        _ => v,
    })
}

fn parse_transform(s: &str) -> Matrix {
    let mut m = Matrix::IDENTITY;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let close = rest[open..].find(')').map(|c| open + c).unwrap_or(rest.len());
        let args = numbers(&rest[open + 1..close]);
        let a = |i: usize, d: f32| args.get(i).copied().unwrap_or(d);
        let t = match name {
            "matrix" if args.len() >= 6 => Matrix([args[0], args[1], args[2], args[3], args[4], args[5]]),
            "translate" => Matrix::translate(a(0, 0.0), a(1, 0.0)),
            "scale" => Matrix::scale(a(0, 1.0), a(1, a(0, 1.0))),
            "rotate" => {
                let r = a(0, 0.0) * PI / 180.0;
                let (s, c) = (sin(r), cos(r));
                let rot = Matrix([c, s, -s, c, 0.0, 0.0]);
                if args.len() >= 3 {
                    Matrix::translate(args[1], args[2]).mul(&rot).mul(&Matrix::translate(-args[1], -args[2]))
                } else {
                    rot
                }
            }
            "skewX" => {
                let t = a(0, 0.0) * PI / 180.0;
                Matrix([1.0, 0.0, sin(t) / cos(t), 1.0, 0.0, 0.0])
            }
            "skewY" => {
                let t = a(0, 0.0) * PI / 180.0;
                Matrix([1.0, sin(t) / cos(t), 0.0, 1.0, 0.0, 0.0])
            }
            _ => Matrix::IDENTITY,
        };
        m = m.mul(&t);
        rest = &rest[(close + 1).min(rest.len())..];
    }
    m
}

#[derive(Clone, Copy, PartialEq)]
enum Paint {
    None,
    Color(u32),
    Gradient(usize, u32),
    Current,
}

fn named_color(name: &str) -> Option<u32> {
    Some(match name {
        "black" => 0x000000,
        "white" => 0xFFFFFF,
        "red" => 0xFF0000,
        "green" => 0x008000,
        "lime" => 0x00FF00,
        "blue" => 0x0000FF,
        "yellow" => 0xFFFF00,
        "cyan" | "aqua" => 0x00FFFF,
        "magenta" | "fuchsia" => 0xFF00FF,
        "gray" | "grey" => 0x808080,
        "silver" => 0xC0C0C0,
        "maroon" => 0x800000,
        "olive" => 0x808000,
        "purple" => 0x800080,
        "teal" => 0x008080,
        "navy" => 0x000080,
        "orange" => 0xFFA500,
        "pink" => 0xFFC0CB,
        "brown" => 0xA52A2A,
        "gold" => 0xFFD700,
        "darkgray" | "darkgrey" => 0xA9A9A9,
        "lightgray" | "lightgrey" => 0xD3D3D3,
        "dimgray" | "dimgrey" => 0x696969,
        "darkblue" => 0x00008B,
        "darkred" => 0x8B0000,
        "darkgreen" => 0x006400,
        "skyblue" => 0x87CEEB,
        "steelblue" => 0x4682B4,
        "tomato" => 0xFF6347,
        "crimson" => 0xDC143C,
        "indigo" => 0x4B0082,
        "violet" => 0xEE82EE,
        "beige" => 0xF5F5DC,
        "whitesmoke" => 0xF5F5F5,
        "gainsboro" => 0xDCDCDC,
        "slategray" | "slategrey" => 0x708090,
        "coral" => 0xFF7F50,
        "salmon" => 0xFA8072,
        "khaki" => 0xF0E68C,
        "orchid" => 0xDA70D6,
        "turquoise" => 0x40E0D0,
        "chocolate" => 0xD2691E,
        "tan" => 0xD2B48C,
        "transparent" => return None,
        _ => return None,
    })
}

fn parse_color(s: &str) -> Option<(u32, f32)> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            3 => Some((((v >> 8) & 15) * 0x110000 | ((v >> 4) & 15) * 0x1100 | (v & 15) * 0x11, 1.0)),
            4 => Some((((v >> 12) & 15) * 0x110000 | ((v >> 8) & 15) * 0x1100 | ((v >> 4) & 15) * 0x11, (v & 15) as f32 / 15.0)),
            6 => Some((v, 1.0)),
            8 => Some((v >> 8, (v & 255) as f32 / 255.0)),
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("rgb") {
        let inner = &s[s.find('(')? + 1..s.rfind(')')?];
        let parts: Vec<&str> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
        let channel = |p: &str| -> u32 {
            let v = if p.ends_with('%') { length(p, 255.0).unwrap_or(0.0) } else { parse_float(p).unwrap_or(0.0) };
            (v.clamp(0.0, 255.0) + 0.5) as u32
        };
        if parts.len() < 3 {
            return None;
        }
        let alpha = parts.get(3).map(|a| if a.ends_with('%') { length(a, 1.0).unwrap_or(1.0) } else { parse_float(a).unwrap_or(1.0) }).unwrap_or(1.0);
        return Some((channel(parts[0]) << 16 | channel(parts[1]) << 8 | channel(parts[2]), alpha.clamp(0.0, 1.0)));
    }
    named_color(&lower).map(|c| (c, 1.0))
}

#[derive(Clone)]
struct Style {
    fill: Paint,
    fill_opacity: f32,
    fill_evenodd: bool,
    stroke: Paint,
    stroke_opacity: f32,
    stroke_width: f32,
    cap: u8,
    join: u8,
    miter: f32,
    opacity: f32,
    color: u32,
    visible: bool,
}

impl Style {
    fn initial() -> Style {
        Style { fill: Paint::Color(0), fill_opacity: 1.0, fill_evenodd: false, stroke: Paint::None, stroke_opacity: 1.0, stroke_width: 1.0, cap: 0, join: 0, miter: 4.0, opacity: 1.0, color: 0, visible: true }
    }
}

struct Css {
    rules: Vec<(String, Vec<(String, String)>)>,
}

fn declarations(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let v = v.trim().trim_end_matches("!important").trim();
            Some((String::from(k.trim()), String::from(v)))
        })
        .collect()
}

fn parse_css(text: &str) -> Css {
    let mut rules = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let selectors = &rest[..open];
        let close = rest[open..].find('}').map(|c| open + c).unwrap_or(rest.len());
        let body = declarations(&rest[open + 1..close]);
        for sel in selectors.split(',') {
            let sel = sel.trim();
            if !sel.is_empty() && !sel.starts_with('@') {
                rules.push((String::from(sel), body.clone()));
            }
        }
        rest = &rest[(close + 1).min(rest.len())..];
    }
    Css { rules }
}

fn selector_matches(sel: &str, node: &Node) -> bool {
    let last = sel.rsplit([' ', '>']).next().unwrap_or(sel).trim();
    if last == "*" {
        return true;
    }
    let mut tag = last;
    let mut ok = true;
    if let Some(i) = last.find(['.', '#']) {
        tag = &last[..i];
        let mut rest = &last[i..];
        while !rest.is_empty() {
            let kind = rest.as_bytes()[0];
            let end = rest[1..].find(['.', '#']).map(|e| e + 1).unwrap_or(rest.len());
            let name = &rest[1..end];
            ok &= if kind == b'.' { node.attr("class").map(|c| c.split_whitespace().any(|x| x == name)).unwrap_or(false) } else { node.attr("id") == Some(name) };
            rest = &rest[end..];
        }
    }
    ok && (tag.is_empty() || tag == node.name)
}

struct Gradient {
    linear: bool,
    coords: [Option<f32>; 5],
    percent: [bool; 5],
    units_user: Option<bool>,
    transform: Option<Matrix>,
    stops: Vec<(f32, u32, f32)>,
    href: Option<String>,
    spread: u8,
}

struct Shape {
    paths: Vec<Vec<(f32, f32)>>,
    style: Style,
    bbox: [f32; 4],
    user: Matrix,
}

enum Item {
    Shape(Shape),
    Group(Group),
}

struct Group {
    items: Vec<Item>,
    opacity: f32,
    clip: Option<(Vec<Vec<(f32, f32)>>, bool)>,
    mask: Option<Vec<Item>>,
    blur: f32,
}

const ISOLATION_LIMIT: usize = 4 << 20;

struct Doc<'a> {
    nodes: &'a [Node],
    css: Css,
    gradients: Vec<(String, Gradient)>,
    stack: Vec<Vec<Item>>,
    viewport: (f32, f32),
    tolerance: f32,
    size: (usize, usize),
}

fn find_by_id(nodes: &[Node], id: &str) -> Option<usize> {
    (0..nodes.len()).find(|&i| nodes[i].attr("id") == Some(id))
}

fn url_ref(v: &str) -> Option<&str> {
    let v = v.trim();
    let inner = v.strip_prefix("url(")?.trim_end().strip_suffix(')')?;
    Some(inner.trim().trim_matches(|c| c == '\'' || c == '"').trim_start_matches('#'))
}

impl<'a> Doc<'a> {
    fn properties(&self, index: usize) -> Vec<(String, String)> {
        let node = &self.nodes[index];
        let mut props: Vec<(String, String)> = node.attrs.clone();
        for (sel, body) in self.css.rules.iter() {
            if selector_matches(sel, node) {
                props.extend(body.iter().cloned());
            }
        }
        if let Some(style) = node.attr("style") {
            props.extend(declarations(style));
        }
        props
    }

    fn paint(&mut self, v: &str, style: &Style) -> (Paint, f32) {
        let v = v.trim();
        if v == "none" {
            return (Paint::None, 1.0);
        }
        if v == "currentColor" {
            return (Paint::Current, 1.0);
        }
        if let Some(id) = url_ref(v) {
            if let Some(g) = self.gradient(id) {
                return (Paint::Gradient(g, 0), 1.0);
            }
            let fallback = v[v.find(')').unwrap_or(v.len() - 1) + 1..].trim();
            if !fallback.is_empty() {
                return self.paint(fallback, style);
            }
            return (Paint::None, 1.0);
        }
        match parse_color(v) {
            Some((c, a)) => (Paint::Color(c), a),
            None => (Paint::None, 1.0),
        }
    }

    fn gradient(&mut self, id: &str) -> Option<usize> {
        if let Some(i) = self.gradients.iter().position(|(gid, _)| gid == id) {
            return Some(i);
        }
        let index = find_by_id(self.nodes, id)?;
        let node = &self.nodes[index];
        let linear = match node.name.as_str() {
            "linearGradient" => true,
            "radialGradient" => false,
            _ => return None,
        };
        let keys = if linear { ["x1", "y1", "x2", "y2", ""] } else { ["cx", "cy", "r", "fx", "fy"] };
        let mut coords = [None; 5];
        let mut percent = [false; 5];
        for (i, k) in keys.iter().enumerate() {
            if let Some(v) = node.attr(k) {
                let v = v.trim();
                percent[i] = v.ends_with('%');
                coords[i] = if let Some(p) = v.strip_suffix('%') { parse_float(p).map(|f| f / 100.0) } else { length(v, 1.0) };
            }
        }
        let mut stops = Vec::new();
        for &c in node.children.iter() {
            if self.nodes[c].name != "stop" {
                continue;
            }
            let props = self.properties(c);
            let get = |k: &str| props.iter().rev().find(|(p, _)| p == k).map(|(_, v)| v.as_str());
            let offset = get("offset").map(|o| if let Some(p) = o.trim().strip_suffix('%') { parse_float(p).unwrap_or(0.0) / 100.0 } else { parse_float(o.trim()).unwrap_or(0.0) }).unwrap_or(0.0);
            let (color, alpha) = get("stop-color").and_then(parse_color).unwrap_or((0, 1.0));
            let opacity = get("stop-opacity").and_then(|o| parse_float(o.trim())).unwrap_or(1.0);
            let offset = offset.clamp(0.0, 1.0).max(stops.last().map(|s: &(f32, u32, f32)| s.0).unwrap_or(0.0));
            stops.push((offset, color, (alpha * opacity).clamp(0.0, 1.0)));
        }
        let gradient = Gradient {
            linear,
            coords,
            percent,
            units_user: node.attr("gradientUnits").map(|u| u == "userSpaceOnUse"),
            transform: node.attr("gradientTransform").map(parse_transform),
            stops,
            href: node.attr("href").map(|h| String::from(h.trim_start_matches('#'))),
            spread: match node.attr("spreadMethod") {
                Some("reflect") => 1,
                Some("repeat") => 2,
                _ => 0,
            },
        };
        self.gradients.push((String::from(id), gradient));
        let me = self.gradients.len() - 1;
        if let Some(parent) = self.gradients[me].1.href.clone() {
            if let Some(p) = self.gradient(&parent) {
                let (stops, coords, percent, units, transform, spread) = {
                    let pg = &self.gradients[p].1;
                    (pg.stops.clone(), pg.coords, pg.percent, pg.units_user, pg.transform, pg.spread)
                };
                let g = &mut self.gradients[me].1;
                if g.stops.is_empty() {
                    g.stops = stops;
                }
                for i in 0..5 {
                    if g.coords[i].is_none() {
                        g.coords[i] = coords[i];
                        g.percent[i] = percent[i];
                    }
                }
                if g.units_user.is_none() {
                    g.units_user = units;
                }
                if g.transform.is_none() {
                    g.transform = transform;
                }
                if g.spread == 0 {
                    g.spread = spread;
                }
            }
        }
        Some(me)
    }

    fn style(&mut self, index: usize, parent: &Style) -> Style {
        let mut s = parent.clone();
        s.opacity = 1.0;
        let props = self.properties(index);
        let mut fill_alpha = 1.0;
        let mut stroke_alpha = 1.0;
        for (k, v) in props.iter() {
            let v = v.trim();
            if v == "inherit" {
                continue;
            }
            match k.as_str() {
                "color" => {
                    if let Some((c, _)) = parse_color(v) {
                        s.color = c;
                    }
                }
                "fill" => {
                    let (p, a) = self.paint(v, &s);
                    s.fill = p;
                    fill_alpha = a;
                }
                "stroke" => {
                    let (p, a) = self.paint(v, &s);
                    s.stroke = p;
                    stroke_alpha = a;
                }
                "fill-opacity" => s.fill_opacity = opacity_value(v),
                "stroke-opacity" => s.stroke_opacity = opacity_value(v),
                "opacity" => s.opacity = opacity_value(v),
                "fill-rule" => s.fill_evenodd = v == "evenodd",
                "stroke-width" => s.stroke_width = length(v, self.viewport.0.max(self.viewport.1)).unwrap_or(1.0).max(0.0),
                "stroke-linecap" => s.cap = match v {
                    "round" => 1,
                    "square" => 2,
                    _ => 0,
                },
                "stroke-linejoin" => s.join = match v {
                    "round" => 1,
                    "bevel" => 2,
                    _ => 0,
                },
                "stroke-miterlimit" => s.miter = parse_float(v).unwrap_or(4.0).max(1.0),
                "display" => {
                    if v == "none" {
                        s.visible = false;
                    }
                }
                "visibility" => s.visible = v != "hidden" && v != "collapse",
                _ => {}
            }
        }
        s.fill_opacity *= fill_alpha;
        s.stroke_opacity *= stroke_alpha;
        s
    }

    fn walk(&mut self, index: usize, matrix: Matrix, parent: &Style, depth: usize) {
        if depth > 64 {
            return;
        }
        let name = self.nodes[index].name.clone();
        if matches!(name.as_str(), "defs" | "clipPath" | "mask" | "symbol" | "linearGradient" | "radialGradient" | "pattern" | "marker" | "style" | "title" | "desc" | "metadata" | "filter" | "text" | "script") {
            return;
        }
        let mut style = self.style(index, parent);
        if !style.visible && name != "g" {
            return;
        }
        if self.properties(index).iter().any(|(k, v)| k == "display" && v.trim() == "none") {
            return;
        }
        let local = self.nodes[index].attr("transform").map(parse_transform).unwrap_or(Matrix::IDENTITY);
        let matrix = matrix.mul(&local);
        let props = self.properties(index);
        let prop = |k: &str| props.iter().rev().find(|(p, _)| p == k).map(|(_, v)| String::from(v.trim()));
        let clip = prop("clip-path").and_then(|v| url_ref(&v).map(String::from)).and_then(|id| self.clip_polygons(&id, matrix, depth));
        let mask = prop("mask").and_then(|v| url_ref(&v).map(String::from)).and_then(|id| self.mask_items(&id, matrix, &style, depth));
        let blur = prop("filter").and_then(|v| url_ref(&v).map(String::from)).map(|id| self.blur_radius(&id) * matrix.expansion()).unwrap_or(0.0);
        let leaf = !matches!(name.as_str(), "svg" | "g" | "a" | "switch" | "use");
        let both = style.fill != Paint::None && style.stroke != Paint::None;
        let isolate = clip.is_some() || mask.is_some() || blur > 0.3 || (style.opacity < 0.999 && (!leaf || both));
        let opacity = style.opacity;
        if isolate {
            self.stack.push(Vec::new());
        } else {
            style.fill_opacity *= style.opacity;
            style.stroke_opacity *= style.opacity;
        }
        style.opacity = 1.0;
        self.walk_inner(index, &name, matrix, style, depth);
        if isolate {
            let items = self.stack.pop().unwrap_or_default();
            let group = Item::Group(Group { items, opacity, clip, mask, blur });
            if let Some(top) = self.stack.last_mut() {
                top.push(group);
            }
        }
    }

    fn clip_polygons(&mut self, id: &str, matrix: Matrix, depth: usize) -> Option<(Vec<Vec<(f32, f32)>>, bool)> {
        let index = find_by_id(self.nodes, id)?;
        if self.nodes[index].name != "clipPath" || depth > 32 {
            return None;
        }
        let own = self.nodes[index].attr("transform").map(parse_transform).unwrap_or(Matrix::IDENTITY);
        let base = matrix.mul(&own);
        let mut polys = Vec::new();
        let mut evenodd = false;
        let kids = self.nodes[index].children.clone();
        for child in kids {
            let (mut node, mut m) = (child, base.mul(&self.nodes[child].attr("transform").map(parse_transform).unwrap_or(Matrix::IDENTITY)));
            if self.nodes[node].name == "use" {
                let Some(target) = self.nodes[node].attr("href").and_then(|h| find_by_id(self.nodes, h.trim_start_matches('#'))) else {
                    continue;
                };
                let x = self.nodes[node].attr("x").and_then(|v| length(v, 0.0)).unwrap_or(0.0);
                let y = self.nodes[node].attr("y").and_then(|v| length(v, 0.0)).unwrap_or(0.0);
                m = m.mul(&Matrix::translate(x, y)).mul(&self.nodes[target].attr("transform").map(parse_transform).unwrap_or(Matrix::IDENTITY));
                node = target;
            }
            let props = self.properties(node);
            if props.iter().any(|(k, v)| (k == "clip-rule" || k == "fill-rule") && v.trim() == "evenodd") {
                evenodd = true;
            }
            let name = self.nodes[node].name.clone();
            for p in self.geometry(node, &name) {
                polys.push(p.iter().map(|&(x, y)| m.apply(x, y)).collect());
            }
        }
        Some((polys, evenodd))
    }

    fn mask_items(&mut self, id: &str, matrix: Matrix, style: &Style, depth: usize) -> Option<Vec<Item>> {
        let index = find_by_id(self.nodes, id)?;
        if self.nodes[index].name != "mask" || depth > 32 {
            return None;
        }
        let user_units = self.nodes[index].attr("maskUnits") == Some("userSpaceOnUse");
        let region = if user_units {
            let (vw, vh) = self.viewport;
            let get = |k: &str, r: f32, d: f32| self.nodes[index].attr(k).and_then(|v| length(v, r)).unwrap_or(d);
            let (x, y, w, h) = (get("x", vw, -0.1 * vw), get("y", vh, -0.1 * vh), get("width", vw, 1.2 * vw), get("height", vh, 1.2 * vh));
            Some([matrix.apply(x, y), matrix.apply(x + w, y), matrix.apply(x + w, y + h), matrix.apply(x, y + h)])
        } else {
            None
        };
        self.stack.push(Vec::new());
        let kids = self.nodes[index].children.clone();
        let mut inner = style.clone();
        inner.fill = Paint::Color(0);
        inner.stroke = Paint::None;
        inner.fill_opacity = 1.0;
        inner.stroke_opacity = 1.0;
        for c in kids {
            self.walk(c, matrix, &inner, depth + 1);
        }
        let items = self.stack.pop()?;
        match region {
            Some(r) => Some(alloc::vec![Item::Group(Group { items, opacity: 1.0, clip: Some((alloc::vec![r.to_vec()], false)), mask: None, blur: 0.0 })]),
            None => Some(items),
        }
    }

    fn blur_radius(&self, id: &str) -> f32 {
        let Some(index) = find_by_id(self.nodes, id) else {
            return 0.0;
        };
        let kids = &self.nodes[index].children;
        let pure = kids.iter().all(|&c| {
            let n = &self.nodes[c];
            n.name == "feGaussianBlur" && n.attr("in").map(|v| v == "SourceGraphic").unwrap_or(true)
        });
        if !pure {
            return 0.0;
        }
        self.nodes[index]
            .children
            .iter()
            .filter(|&&c| self.nodes[c].name == "feGaussianBlur")
            .filter_map(|&c| self.nodes[c].attr("stdDeviation").map(numbers))
            .map(|v| v.iter().copied().fold(0.0, f32::max))
            .fold(0.0, f32::max)
    }

    fn walk_inner(&mut self, index: usize, name: &str, matrix: Matrix, style: Style, depth: usize) {
        let mut matrix = matrix;
        match name {
            "svg" if depth > 0 => {
                let x = self.nodes[index].attr("x").and_then(|v| length(v, self.viewport.0)).unwrap_or(0.0);
                let y = self.nodes[index].attr("y").and_then(|v| length(v, self.viewport.1)).unwrap_or(0.0);
                matrix = matrix.mul(&Matrix::translate(x, y));
                if let Some(vb) = self.nodes[index].attr("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0) {
                    let w = self.nodes[index].attr("width").and_then(|v| length(v, self.viewport.0)).unwrap_or(vb[2]);
                    let h = self.nodes[index].attr("height").and_then(|v| length(v, self.viewport.1)).unwrap_or(vb[3]);
                    matrix = matrix.mul(&fit(vb[0], vb[1], vb[2], vb[3], w, h, self.nodes[index].attr("preserveAspectRatio")));
                }
                self.children(index, matrix, &style, depth);
            }
            "svg" | "g" | "a" | "switch" => self.children(index, matrix, &style, depth),
            "use" => {
                let Some(target) = self.nodes[index].attr("href").map(|h| String::from(h.trim_start_matches('#'))) else {
                    return;
                };
                let Some(t) = find_by_id(self.nodes, &target) else {
                    return;
                };
                let x = self.nodes[index].attr("x").and_then(|v| length(v, self.viewport.0)).unwrap_or(0.0);
                let y = self.nodes[index].attr("y").and_then(|v| length(v, self.viewport.1)).unwrap_or(0.0);
                let m = matrix.mul(&Matrix::translate(x, y));
                if self.nodes[t].name == "symbol" {
                    let mut m = m;
                    if let Some(vb) = self.nodes[t].attr("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0) {
                        let w = self.nodes[index].attr("width").and_then(|v| length(v, self.viewport.0)).unwrap_or(vb[2]);
                        let h = self.nodes[index].attr("height").and_then(|v| length(v, self.viewport.1)).unwrap_or(vb[3]);
                        m = m.mul(&fit(vb[0], vb[1], vb[2], vb[3], w, h, self.nodes[t].attr("preserveAspectRatio")));
                    }
                    let symbol_style = self.style(t, &style);
                    self.children(t, m, &symbol_style, depth + 1);
                } else {
                    self.walk(t, m, &style, depth + 1);
                }
            }
            _ => {
                let paths = self.geometry(index, name);
                if !paths.is_empty() && style.visible {
                    self.add_shape(paths, style, matrix);
                }
            }
        }
    }

    fn children(&mut self, index: usize, matrix: Matrix, style: &Style, depth: usize) {
        let kids = self.nodes[index].children.clone();
        for c in kids {
            self.walk(c, matrix, style, depth + 1);
        }
    }

    fn geometry(&self, index: usize, name: &str) -> Vec<Vec<(f32, f32)>> {
        let n = &self.nodes[index];
        let (vw, vh) = self.viewport;
        let diag = sqrt(vw * vw + vh * vh) / core::f32::consts::SQRT_2;
        let get = |k: &str, r: f32| n.attr(k).and_then(|v| length(v, r)).unwrap_or(0.0);
        let mut b = PathBuilder::new(self.tolerance);
        match name {
            "path" => {
                if let Some(d) = n.attr("d") {
                    b.svg_path(d);
                }
            }
            "rect" => {
                let (x, y, w, h) = (get("x", vw), get("y", vh), get("width", vw), get("height", vh));
                if w <= 0.0 || h <= 0.0 {
                    return Vec::new();
                }
                let rx_attr = n.attr("rx").and_then(|v| length(v, vw));
                let ry_attr = n.attr("ry").and_then(|v| length(v, vh));
                let rx = rx_attr.or(ry_attr).unwrap_or(0.0).min(w / 2.0);
                let ry = ry_attr.or(rx_attr).unwrap_or(0.0).min(h / 2.0);
                if rx > 0.0 && ry > 0.0 {
                    b.move_to(x + rx, y);
                    b.line_to(x + w - rx, y);
                    b.arc(rx, ry, 0.0, false, true, x + w, y + ry);
                    b.line_to(x + w, y + h - ry);
                    b.arc(rx, ry, 0.0, false, true, x + w - rx, y + h);
                    b.line_to(x + rx, y + h);
                    b.arc(rx, ry, 0.0, false, true, x, y + h - ry);
                    b.line_to(x, y + ry);
                    b.arc(rx, ry, 0.0, false, true, x + rx, y);
                } else {
                    b.move_to(x, y);
                    b.line_to(x + w, y);
                    b.line_to(x + w, y + h);
                    b.line_to(x, y + h);
                }
                b.close();
            }
            "circle" | "ellipse" => {
                let (cx, cy) = (get("cx", vw), get("cy", vh));
                let (rx, ry) = if name == "circle" {
                    let r = get("r", diag);
                    (r, r)
                } else {
                    (get("rx", vw), get("ry", vh))
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return Vec::new();
                }
                b.move_to(cx + rx, cy);
                b.arc(rx, ry, 0.0, false, true, cx - rx, cy);
                b.arc(rx, ry, 0.0, false, true, cx + rx, cy);
                b.close();
            }
            "line" => {
                b.move_to(get("x1", vw), get("y1", vh));
                b.line_to(get("x2", vw), get("y2", vh));
            }
            "polyline" | "polygon" => {
                let pts = n.attr("points").map(numbers).unwrap_or_default();
                for (i, p) in pts.chunks_exact(2).enumerate() {
                    if i == 0 {
                        b.move_to(p[0], p[1]);
                    } else {
                        b.line_to(p[0], p[1]);
                    }
                }
                if name == "polygon" {
                    b.close();
                }
            }
            _ => {}
        }
        b.finish()
    }

    fn add_shape(&mut self, paths: Vec<Vec<(f32, f32)>>, style: Style, matrix: Matrix) {
        let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in paths.iter() {
            for &(x, y) in p.iter() {
                bbox[0] = bbox[0].min(x);
                bbox[1] = bbox[1].min(y);
                bbox[2] = bbox[2].max(x);
                bbox[3] = bbox[3].max(y);
            }
        }
        if let Some(top) = self.stack.last_mut() {
            top.push(Item::Shape(Shape { paths, style, bbox, user: matrix }));
        }
    }
}

fn opacity_value(v: &str) -> f32 {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        return (parse_float(p).unwrap_or(100.0) / 100.0).clamp(0.0, 1.0);
    }
    parse_float(v).unwrap_or(1.0).clamp(0.0, 1.0)
}

fn fit(vx: f32, vy: f32, vw: f32, vh: f32, w: f32, h: f32, aspect: Option<&str>) -> Matrix {
    let aspect = aspect.unwrap_or("xMidYMid meet").trim();
    let (sx, sy) = (w / vw, h / vh);
    if aspect.starts_with("none") {
        return Matrix::scale(sx, sy).mul(&Matrix::translate(-vx, -vy));
    }
    let slice = aspect.contains("slice");
    let s = if slice { sx.max(sy) } else { sx.min(sy) };
    let align = aspect.split_whitespace().next().unwrap_or("xMidYMid");
    let ax = if align.contains("xMin") { 0.0 } else if align.contains("xMax") { 1.0 } else { 0.5 };
    let ay = if align.contains("YMin") { 0.0 } else if align.contains("YMax") { 1.0 } else { 0.5 };
    let tx = (w - vw * s) * ax;
    let ty = (h - vh * s) * ay;
    Matrix::translate(tx, ty).mul(&Matrix::scale(s, s)).mul(&Matrix::translate(-vx, -vy))
}

struct PathBuilder {
    paths: Vec<Vec<(f32, f32)>>,
    current: Vec<(f32, f32)>,
    start: (f32, f32),
    pos: (f32, f32),
    tolerance: f32,
}

impl PathBuilder {
    fn new(tolerance: f32) -> PathBuilder {
        PathBuilder { paths: Vec::new(), current: Vec::new(), start: (0.0, 0.0), pos: (0.0, 0.0), tolerance }
    }

    fn flush(&mut self) {
        if self.current.len() > 1 {
            let path = core::mem::take(&mut self.current);
            self.paths.push(path);
        } else {
            self.current.clear();
        }
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.current.push((x, y));
        self.start = (x, y);
        self.pos = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if self.current.is_empty() {
            self.current.push(self.pos);
        }
        self.current.push((x, y));
        self.pos = (x, y);
    }

    fn close(&mut self) {
        if !self.current.is_empty() {
            let first = self.current[0];
            if *self.current.last().unwrap() != first {
                self.current.push(first);
            }
        }
        self.flush();
        self.pos = self.start;
    }

    fn segments(&self, length: f32) -> usize {
        ((sqrt(length / self.tolerance.max(0.01)) * 1.5) as usize).clamp(2, 256)
    }

    fn cubic(&mut self, c1: (f32, f32), c2: (f32, f32), p: (f32, f32)) {
        let p0 = self.pos;
        let d = |a: (f32, f32), b: (f32, f32)| sqrt((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1));
        let n = self.segments(d(p0, c1) + d(c1, c2) + d(c2, p));
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let x = u * u * u * p0.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * p.0;
            let y = u * u * u * p0.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * p.1;
            self.line_to(x, y);
        }
    }

    fn quad(&mut self, c: (f32, f32), p: (f32, f32)) {
        let p0 = self.pos;
        let d = |a: (f32, f32), b: (f32, f32)| sqrt((a.0 - b.0) * (a.0 - b.0) + (a.1 - b.1) * (a.1 - b.1));
        let n = self.segments(d(p0, c) + d(c, p));
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.line_to(u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p.0, u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p.1);
        }
    }

    fn arc(&mut self, rx: f32, ry: f32, angle: f32, large: bool, sweep: bool, x: f32, y: f32) {
        let (x1, y1) = self.pos;
        let (mut rx, mut ry) = (rx.abs(), ry.abs());
        if rx == 0.0 || ry == 0.0 || (x1 == x && y1 == y) {
            self.line_to(x, y);
            return;
        }
        let phi = angle * PI / 180.0;
        let (sp, cp) = (sin(phi), cos(phi));
        let dx = (x1 - x) / 2.0;
        let dy = (y1 - y) / 2.0;
        let x1p = cp * dx + sp * dy;
        let y1p = -sp * dx + cp * dy;
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = sqrt(lambda);
            rx *= s;
            ry *= s;
        }
        let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
        let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
        let mut coef = if den == 0.0 { 0.0 } else { sqrt((num / den).max(0.0)) };
        if large == sweep {
            coef = -coef;
        }
        let cxp = coef * rx * y1p / ry;
        let cyp = -coef * ry * x1p / rx;
        let cx = cp * cxp - sp * cyp + (x1 + x) / 2.0;
        let cy = sp * cxp + cp * cyp + (y1 + y) / 2.0;
        let a1 = atan2((y1p - cyp) / ry, (x1p - cxp) / rx);
        let a2 = atan2((-y1p - cyp) / ry, (-x1p - cxp) / rx);
        let mut delta = a2 - a1;
        if sweep && delta < 0.0 {
            delta += 2.0 * PI;
        } else if !sweep && delta > 0.0 {
            delta -= 2.0 * PI;
        }
        let n = self.segments(delta.abs() * rx.max(ry)).max(4);
        for i in 1..=n {
            let t = a1 + delta * i as f32 / n as f32;
            let (st, ct) = (sin(t), cos(t));
            let px = cx + rx * ct * cp - ry * st * sp;
            let py = cy + rx * ct * sp + ry * st * cp;
            if i == n {
                self.line_to(x, y);
            } else {
                self.line_to(px, py);
            }
        }
    }

    fn svg_path(&mut self, d: &str) {
        let mut rest = d;
        let mut cmd = b'M';
        let mut last_ctrl: Option<(f32, f32)> = None;
        let mut last_cmd = b'M';
        loop {
            let trimmed = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
            if trimmed.is_empty() {
                break;
            }
            let c = trimmed.as_bytes()[0];
            if c.is_ascii_alphabetic() {
                cmd = c;
                rest = &trimmed[1..];
                if cmd == b'Z' || cmd == b'z' {
                    self.close();
                    last_ctrl = None;
                    last_cmd = cmd;
                    continue;
                }
            } else {
                rest = trimmed;
            }
            let rel = cmd.is_ascii_lowercase();
            let (ox, oy) = if rel { self.pos } else { (0.0, 0.0) };
            let count = match cmd.to_ascii_uppercase() {
                b'M' | b'L' | b'T' => 2,
                b'H' | b'V' => 1,
                b'C' => 6,
                b'S' | b'Q' => 4,
                b'A' => 7,
                _ => return,
            };
            let mut args = [0f32; 7];
            let mut ok = true;
            let arc = cmd.to_ascii_uppercase() == b'A';
            for (i, slot) in args.iter_mut().enumerate().take(count) {
                let t = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
                if arc && (i == 3 || i == 4) {
                    match t.as_bytes().first() {
                        Some(b'0') | Some(b'1') => {
                            *slot = (t.as_bytes()[0] - b'0') as f32;
                            rest = &t[1..];
                            continue;
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                match number_prefix(t) {
                    Some((v, r)) => {
                        *slot = v;
                        rest = r;
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            let upper = cmd.to_ascii_uppercase();
            match upper {
                b'M' => {
                    self.move_to(ox + args[0], oy + args[1]);
                    cmd = if rel { b'l' } else { b'L' };
                }
                b'L' => self.line_to(ox + args[0], oy + args[1]),
                b'H' => self.line_to(ox + args[0], self.pos.1),
                b'V' => self.line_to(self.pos.0, oy + args[0]),
                b'C' => {
                    let c2 = (ox + args[2], oy + args[3]);
                    self.cubic((ox + args[0], oy + args[1]), c2, (ox + args[4], oy + args[5]));
                    last_ctrl = Some(c2);
                }
                b'S' => {
                    let c1 = match (last_ctrl, last_cmd.to_ascii_uppercase()) {
                        (Some(c), b'C' | b'S') => (2.0 * self.pos.0 - c.0, 2.0 * self.pos.1 - c.1),
                        _ => self.pos,
                    };
                    let c2 = (ox + args[0], oy + args[1]);
                    self.cubic(c1, c2, (ox + args[2], oy + args[3]));
                    last_ctrl = Some(c2);
                }
                b'Q' => {
                    let c = (ox + args[0], oy + args[1]);
                    self.quad(c, (ox + args[2], oy + args[3]));
                    last_ctrl = Some(c);
                }
                b'T' => {
                    let c = match (last_ctrl, last_cmd.to_ascii_uppercase()) {
                        (Some(c), b'Q' | b'T') => (2.0 * self.pos.0 - c.0, 2.0 * self.pos.1 - c.1),
                        _ => self.pos,
                    };
                    self.quad(c, (ox + args[0], oy + args[1]));
                    last_ctrl = Some(c);
                }
                b'A' => self.arc(args[0], args[1], args[2], args[3] != 0.0, args[4] != 0.0, ox + args[5], oy + args[6]),
                _ => return,
            }
            if !matches!(upper, b'C' | b'S' | b'Q' | b'T') {
                last_ctrl = None;
            }
            last_cmd = upper;
        }
    }

    fn finish(mut self) -> Vec<Vec<(f32, f32)>> {
        self.flush();
        self.paths
    }
}

struct Edge {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    dir: i32,
}

struct Raster {
    width: usize,
    height: usize,
    coverage: Vec<f32>,
    edges: Vec<Edge>,
}

impl Raster {
    fn new(width: usize, height: usize) -> Raster {
        Raster { width, height, coverage: alloc::vec![0.0; width * height], edges: Vec::new() }
    }

    fn add_polygon(&mut self, points: &[(f32, f32)]) {
        if points.len() < 2 {
            return;
        }
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            if a.1 == b.1 {
                continue;
            }
            if a.1 < b.1 {
                self.edges.push(Edge { x0: a.0, y0: a.1, x1: b.0, y1: b.1, dir: 1 });
            } else {
                self.edges.push(Edge { x0: b.0, y0: b.1, x1: a.0, y1: a.1, dir: -1 });
            }
        }
    }

    fn fill(&mut self, evenodd: bool) -> (usize, usize) {
        self.coverage.fill(0.0);
        if self.edges.is_empty() {
            return (0, 0);
        }
        self.edges.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(core::cmp::Ordering::Equal));
        let ymin = floor(self.edges[0].y0).max(0.0) as usize;
        let ymax = (ceil(self.edges.iter().map(|e| e.y1).fold(f32::MIN, f32::max)) as isize).clamp(0, self.height as isize) as usize;
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        let mut active: Vec<usize> = Vec::new();
        let mut next = 0usize;
        let weight = 1.0 / SUBSAMPLES as f32;
        let w = self.width as f32;
        for y in ymin..ymax {
            let row = &mut self.coverage[y * self.width..(y + 1) * self.width];
            for s in 0..SUBSAMPLES {
                let sy = y as f32 + (s as f32 + 0.5) / SUBSAMPLES as f32;
                while next < self.edges.len() && self.edges[next].y0 <= sy {
                    active.push(next);
                    next += 1;
                }
                active.retain(|&i| self.edges[i].y1 > sy);
                crossings.clear();
                for &i in active.iter() {
                    let e = &self.edges[i];
                    if e.y0 > sy {
                        continue;
                    }
                    let t = (sy - e.y0) / (e.y1 - e.y0);
                    crossings.push((e.x0 + (e.x1 - e.x0) * t, e.dir));
                }
                if crossings.len() < 2 {
                    continue;
                }
                crossings.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
                let mut wind = 0i32;
                for i in 0..crossings.len() - 1 {
                    wind += crossings[i].1;
                    let inside = if evenodd { wind & 1 != 0 } else { wind != 0 };
                    if !inside {
                        continue;
                    }
                    let (xa, xb) = (crossings[i].0.clamp(0.0, w), crossings[i + 1].0.clamp(0.0, w));
                    if xb <= xa {
                        continue;
                    }
                    let (ia, ib) = (floor(xa) as usize, floor(xb) as usize);
                    if ia == ib {
                        if ia < self.width {
                            row[ia] += (xb - xa) * weight;
                        }
                        continue;
                    }
                    row[ia] += ((ia + 1) as f32 - xa) * weight;
                    for c in row.iter_mut().take(ib.min(self.width)).skip(ia + 1) {
                        *c += weight;
                    }
                    if ib < self.width {
                        row[ib] += (xb - ib as f32) * weight;
                    }
                }
            }
        }
        self.edges.clear();
        (ymin, ymax)
    }
}

fn stroke_polygons(path: &[(f32, f32)], width: f32, cap: u8, join: u8, miter: f32, tolerance: f32) -> Vec<Vec<(f32, f32)>> {
    let mut out = Vec::new();
    let half = width / 2.0;
    if half <= 0.0 || path.len() < 2 {
        return out;
    }
    let closed = path.len() > 2 && path[0] == path[path.len() - 1];
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(path.len());
    for &p in path {
        if pts.last().map(|q: &(f32, f32)| (q.0 - p.0).abs() > 1e-4 || (q.1 - p.1).abs() > 1e-4).unwrap_or(true) {
            pts.push(p);
        }
    }
    if pts.len() < 2 {
        if cap == 1 {
            out.push(circle(pts[0], half, tolerance));
        } else if cap == 2 {
            let p = pts[0];
            out.push(alloc::vec![(p.0 - half, p.1 - half), (p.0 + half, p.1 - half), (p.0 + half, p.1 + half), (p.0 - half, p.1 + half)]);
        }
        return out;
    }
    let normal = |a: (f32, f32), b: (f32, f32)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = sqrt(dx * dx + dy * dy).max(1e-6);
        (-dy / len * half, dx / len * half, dx / len, dy / len)
    };
    let n = pts.len();
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        let (nx, ny, tx, ty) = normal(a, b);
        let (mut a2, mut b2) = (a, b);
        if !closed && cap == 2 {
            if i == 0 {
                a2 = (a.0 - tx * half, a.1 - ty * half);
            }
            if i == n - 2 {
                b2 = (b.0 + tx * half, b.1 + ty * half);
            }
        }
        out.push(alloc::vec![(a2.0 + nx, a2.1 + ny), (b2.0 + nx, b2.1 + ny), (b2.0 - nx, b2.1 - ny), (a2.0 - nx, a2.1 - ny)]);
    }
    let joints: Vec<usize> = if closed { (0..n - 1).collect() } else { (1..n - 1).collect() };
    for &j in joints.iter() {
        let prev = if j == 0 { pts[n - 2] } else { pts[j - 1] };
        let (p, next) = (pts[j], pts[j + 1]);
        let (n1x, n1y, t1x, t1y) = normal(prev, p);
        let (n2x, n2y, t2x, t2y) = normal(p, next);
        let cross = t1x * t2y - t1y * t2x;
        if join == 1 {
            out.push(circle(p, half, tolerance));
            continue;
        }
        let side = if cross > 0.0 { -1.0 } else { 1.0 };
        let o1 = (p.0 + n1x * side, p.1 + n1y * side);
        let o2 = (p.0 + n2x * side, p.1 + n2y * side);
        if join == 0 {
            let cos_theta = (t1x * t2x + t1y * t2y).clamp(-1.0, 1.0);
            let ratio = 1.0 / sqrt(((1.0 + cos_theta) / 2.0).max(1e-6));
            if ratio <= miter {
                let (bx, by) = (n1x + n2x, n1y + n2y);
                let blen = sqrt(bx * bx + by * by).max(1e-6);
                let m = (p.0 + bx / blen * half * ratio * side, p.1 + by / blen * half * ratio * side);
                out.push(alloc::vec![p, o1, m, o2]);
                continue;
            }
        }
        out.push(alloc::vec![p, o1, o2]);
    }
    if !closed && cap == 1 {
        out.push(circle(pts[0], half, tolerance));
        out.push(circle(pts[n - 1], half, tolerance));
    }
    for poly in out.iter_mut() {
        let mut area = 0.0;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            area += a.0 * b.1 - b.0 * a.1;
        }
        if area < 0.0 {
            poly.reverse();
        }
    }
    out
}

fn circle(c: (f32, f32), r: f32, tolerance: f32) -> Vec<(f32, f32)> {
    let n = ((sqrt(r / tolerance.max(0.01)) * 4.0) as usize).clamp(8, 64);
    (0..n).map(|i| {
        let t = i as f32 / n as f32 * 2.0 * PI;
        (c.0 + r * cos(t), c.1 + r * sin(t))
    }).collect()
}

fn gradient_color(g: &Gradient, t: f32) -> (u32, f32) {
    if g.stops.is_empty() {
        return (0, 0.0);
    }
    let t = match g.spread {
        1 => {
            let m = t.abs() % 2.0;
            if m > 1.0 { 2.0 - m } else { m }
        }
        2 => t - floor(t),
        _ => t.clamp(0.0, 1.0),
    };
    let first = g.stops[0];
    if t <= first.0 {
        return (first.1, first.2);
    }
    for w in g.stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let span = (b.0 - a.0).max(1e-6);
            let f = ((t - a.0) / span).clamp(0.0, 1.0);
            let ch = |s: u32| {
                let (x, y) = (((a.1 >> s) & 255) as f32, ((b.1 >> s) & 255) as f32);
                ((x + (y - x) * f + 0.5) as u32) << s
            };
            return (ch(16) | ch(8) | ch(0), a.2 + (b.2 - a.2) * f);
        }
    }
    let last = g.stops[g.stops.len() - 1];
    (last.1, last.2)
}

fn composite(out: &mut [f32], width: usize, raster: &Raster, rows: (usize, usize), paint: &dyn Fn(f32, f32) -> (u32, f32), alpha: f32) {
    for y in rows.0..rows.1 {
        for x in 0..width {
            let c = raster.coverage[y * width + x];
            if c <= 0.001 {
                continue;
            }
            let (color, a) = paint(x as f32 + 0.5, y as f32 + 0.5);
            let a = (c.min(1.0) * a * alpha).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let i = (y * width + x) * 4;
            let inv = 1.0 - a;
            out[i] = a + out[i] * inv;
            out[i + 1] = ((color >> 16) & 255) as f32 * a + out[i + 1] * inv;
            out[i + 2] = ((color >> 8) & 255) as f32 * a + out[i + 2] * inv;
            out[i + 3] = (color & 255) as f32 * a + out[i + 3] * inv;
        }
    }
}

impl Doc<'_> {
    fn draw_items(&self, items: &[Item], acc: &mut [f32], raster: &mut Raster) {
        for item in items {
            match item {
                Item::Shape(shape) => self.draw_shape(shape, acc, raster, None),
                Item::Group(group) => self.draw_group(group, acc, raster),
            }
        }
    }

    fn draw_group(&self, group: &Group, acc: &mut [f32], raster: &mut Raster) {
        let (w, h) = self.size;
        if w * h > ISOLATION_LIMIT {
            self.draw_items(&group.items, acc, raster);
            return;
        }
        let mut layer = alloc::vec![0f32; w * h * 4];
        self.draw_items(&group.items, &mut layer, raster);
        if group.blur > 0.3 {
            blur(&mut layer, w, h, group.blur);
        }
        if let Some((polys, evenodd)) = group.clip.as_ref() {
            for p in polys {
                raster.add_polygon(p);
            }
            raster.fill(*evenodd);
            for i in 0..w * h {
                let c = raster.coverage[i].min(1.0);
                for k in 0..4 {
                    layer[i * 4 + k] *= c;
                }
            }
        }
        if let Some(mask) = group.mask.as_ref() {
            let mut m = alloc::vec![0f32; w * h * 4];
            self.draw_items(mask, &mut m, raster);
            for i in 0..w * h {
                let lum = (0.2125 * m[i * 4 + 1] + 0.7154 * m[i * 4 + 2] + 0.0721 * m[i * 4 + 3]) / 255.0;
                for k in 0..4 {
                    layer[i * 4 + k] *= lum.clamp(0.0, 1.0);
                }
            }
        }
        let o = group.opacity;
        for i in 0..w * h {
            let a = layer[i * 4] * o;
            if a <= 0.0 {
                continue;
            }
            let inv = 1.0 - a;
            acc[i * 4] = a + acc[i * 4] * inv;
            for k in 1..4 {
                acc[i * 4 + k] = layer[i * 4 + k] * o + acc[i * 4 + k] * inv;
            }
        }
    }

    fn draw_shape(&self, shape: &Shape, acc: &mut [f32], raster: &mut Raster, _clip: Option<&[f32]>) {
        let w = self.size.0;
        let device_tolerance = 0.25;
        for pass in 0..2 {
            let (paint, opacity) = if pass == 0 { (shape.style.fill, shape.style.fill_opacity) } else { (shape.style.stroke, shape.style.stroke_opacity) };
            if paint == Paint::None || opacity <= 0.0 {
                continue;
            }
            if pass == 0 {
                for p in shape.paths.iter() {
                    let pts: Vec<(f32, f32)> = p.iter().map(|&(x, y)| shape.user.apply(x, y)).collect();
                    raster.add_polygon(&pts);
                }
            } else {
                let width = shape.style.stroke_width * shape.user.expansion();
                if width <= 0.0 {
                    continue;
                }
                for p in shape.paths.iter() {
                    let pts: Vec<(f32, f32)> = p.iter().map(|&(x, y)| shape.user.apply(x, y)).collect();
                    for poly in stroke_polygons(&pts, width.max(0.5), shape.style.cap, shape.style.join, shape.style.miter, device_tolerance) {
                        raster.add_polygon(&poly);
                    }
                }
            }
            let rows = raster.fill(pass == 0 && shape.style.fill_evenodd);
            match paint {
                Paint::Color(c) => composite(acc, w, raster, rows, &|_, _| (c, 1.0), opacity),
                Paint::Current => {
                    let c = shape.style.color;
                    composite(acc, w, raster, rows, &|_, _| (c, 1.0), opacity)
                }
                Paint::Gradient(gi, _) => {
                    let g = &self.gradients[gi].1;
                    let bb = shape.bbox;
                    let user = !g.units_user.unwrap_or(false);
                    let bbox_m = if user { Matrix([(bb[2] - bb[0]).max(1e-6), 0.0, 0.0, (bb[3] - bb[1]).max(1e-6), bb[0], bb[1]]) } else { Matrix::IDENTITY };
                    let full = shape.user.mul(&bbox_m).mul(&g.transform.unwrap_or(Matrix::IDENTITY));
                    let Some(inv) = full.inverse() else {
                        continue;
                    };
                    let (vw, vh) = self.viewport;
                    let diag = sqrt(vw * vw + vh * vh) / core::f32::consts::SQRT_2;
                    let refs = [vw, vh, if g.linear { vw } else { diag }, if g.linear { vh } else { vw }, vh];
                    let coord = |i: usize, d: f32| match g.coords[i] {
                        Some(v) if !user && g.percent[i] => v * refs[i],
                        Some(v) => v,
                        None => d,
                    };
                    if g.linear {
                        let (x1, y1, x2, y2) = (coord(0, 0.0), coord(1, 0.0), coord(2, if user { 1.0 } else { vw }), coord(3, 0.0));
                        let (dx, dy) = (x2 - x1, y2 - y1);
                        let len2 = (dx * dx + dy * dy).max(1e-9);
                        composite(acc, w, raster, rows, &|px, py| {
                            let (u, v) = inv.apply(px, py);
                            gradient_color(g, ((u - x1) * dx + (v - y1) * dy) / len2)
                        }, opacity);
                    } else {
                        let (cx, cy, r) = (coord(0, if user { 0.5 } else { vw / 2.0 }), coord(1, if user { 0.5 } else { vh / 2.0 }), coord(2, if user { 0.5 } else { diag / 2.0 }));
                        let r = r.max(1e-6);
                        composite(acc, w, raster, rows, &|px, py| {
                            let (u, v) = inv.apply(px, py);
                            let (dx, dy) = (u - cx, v - cy);
                            gradient_color(g, sqrt(dx * dx + dy * dy) / r)
                        }, opacity);
                    }
                }
                Paint::None => {}
            }
        }
    }
}

fn box_pass(data: &mut [f32], w: usize, h: usize, radius: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let index = |o: usize, i: usize| if horizontal { (o * w + i) * 4 } else { (i * w + o) * 4 };
    let mut line = alloc::vec![0f32; inner * 4];
    let norm = 1.0 / (2 * radius + 1) as f32;
    for o in 0..outer {
        for i in 0..inner {
            let at = index(o, i);
            line[i * 4..i * 4 + 4].copy_from_slice(&data[at..at + 4]);
        }
        let mut sum = [0f32; 4];
        for i in 0..=radius.min(inner - 1) {
            for k in 0..4 {
                sum[k] += line[i * 4 + k];
            }
        }
        for i in 0..inner {
            let at = index(o, i);
            for k in 0..4 {
                data[at + k] = sum[k] * norm;
            }
            let add = i + radius + 1;
            if add < inner {
                for k in 0..4 {
                    sum[k] += line[add * 4 + k];
                }
            }
            if i >= radius {
                let sub = i - radius;
                for k in 0..4 {
                    sum[k] -= line[sub * 4 + k];
                }
            }
        }
    }
}

fn blur(data: &mut [f32], w: usize, h: usize, sigma: f32) {
    let d = (sigma * 3.0 * sqrt(2.0 * PI) / 4.0 + 0.5) as usize;
    let radius = (d / 2).max(1);
    for _ in 0..3 {
        box_pass(data, w, h, radius, true);
        box_pass(data, w, h, radius, false);
    }
}

fn intrinsic(nodes: &[Node]) -> Option<(usize, f32, f32, Option<[f32; 4]>)> {
    let root = nodes.iter().position(|n| n.name == "svg")?;
    let n = &nodes[root];
    let vb = n.attr("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).map(|v| [v[0], v[1], v[2], v[3]]);
    let w = n.attr("width").filter(|v| !v.trim().ends_with('%')).and_then(|v| length(v, 0.0));
    let h = n.attr("height").filter(|v| !v.trim().ends_with('%')).and_then(|v| length(v, 0.0));
    let (w, h) = match (w, h, vb) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(v)) => (w, w * v[3] / v[2]),
        (None, Some(h), Some(v)) => (h * v[2] / v[3], h),
        (None, None, Some(v)) => (v[2], v[3]),
        _ => (DEFAULT_SIZE, DEFAULT_SIZE),
    };
    Some((root, w.max(1.0), h.max(1.0), vb))
}

pub fn dimensions(data: &[u8]) -> Result<(u32, u32)> {
    let text = core::str::from_utf8(data).map_err(|_| Error::Corrupt("svg: not UTF-8"))?;
    let nodes = parse_xml(text)?;
    let (_, w, h, _) = intrinsic(&nodes).ok_or(Error::Corrupt("svg: no <svg> element"))?;
    Ok(((w + 0.5) as u32, (h + 0.5) as u32))
}

pub fn render(data: &[u8], width: u32, height: u32) -> Result<Bitmap> {
    let text = core::str::from_utf8(data).map_err(|_| Error::Corrupt("svg: not UTF-8"))?;
    let nodes = parse_xml(text)?;
    let (root, iw, ih, vb) = intrinsic(&nodes).ok_or(Error::Corrupt("svg: no <svg> element"))?;
    let (ow, oh) = match (width, height) {
        (0, 0) => ((iw + 0.5) as u32, (ih + 0.5) as u32),
        (w, 0) => (w, ((w as f32 * ih / iw) + 0.5) as u32),
        (0, h) => (((h as f32 * iw / ih) + 0.5) as u32, h),
        (w, h) => (w, h),
    };
    let (ow, oh) = (ow.max(1), oh.max(1));
    check_size(ow, oh)?;
    let mut base = Matrix::scale(ow as f32 / iw, oh as f32 / ih);
    if let Some(v) = vb {
        base = base.mul(&fit(v[0], v[1], v[2], v[3], iw, ih, nodes[root].attr("preserveAspectRatio")));
    }
    let mut css_text = String::new();
    for n in nodes.iter().filter(|n| n.name == "style") {
        css_text.push_str(&n.text);
        css_text.push('\n');
    }
    let viewport = vb.map(|v| (v[2], v[3])).unwrap_or((iw, ih));
    let (w, h) = (ow as usize, oh as usize);
    let mut doc = Doc { nodes: &nodes, css: parse_css(&css_text), gradients: Vec::new(), stack: alloc::vec![Vec::new()], viewport, tolerance: 0.2 / base.expansion(), size: (w, h) };
    doc.walk(root, base, &Style::initial(), 0);
    let items = doc.stack.pop().unwrap_or_default();
    let mut acc = alloc::vec![0f32; w * h * 4];
    let mut raster = Raster::new(w, h);
    doc.draw_items(&items, &mut acc, &mut raster);
    let mut out = Bitmap::new(ow, oh)?;
    for (i, px) in out.pixels.iter_mut().enumerate() {
        let a = acc[i * 4];
        if a <= 0.002 {
            continue;
        }
        let un = |v: f32| ((v / a).clamp(0.0, 255.0) + 0.5) as u32;
        *px = (((a * 255.0 + 0.5) as u32).min(255)) << 24 | un(acc[i * 4 + 1]) << 16 | un(acc[i * 4 + 2]) << 8 | un(acc[i * 4 + 3]);
    }
    Ok(out)
}
