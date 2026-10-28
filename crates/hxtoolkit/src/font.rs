extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub use vellum::hfont::Glyph;
pub use vellum::Font;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontFamily {
    Sans,
    Serif,
    Monospace,
    Custom(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontWeight {
    Light,
    Regular,
    Medium,
    Bold,
}

pub fn load_font_bytes(bytes: &[u8]) -> Option<Font> {
    Font::parse(bytes)
}

pub fn load_font_file(path: &str) -> Option<Font> {
    #[cfg(feature = "std")]
    {
        if let Ok(bytes) = std::fs::read(path) {
            return Font::parse(&bytes);
        }
    }
    #[cfg(feature = "nook")]
    {
        if let Some(bytes) = hxproto::client::posix::read_file(path) {
            return Font::parse(&bytes);
        }
    }
    let _ = path;
    None
}

pub fn load_system_font(name: &str) -> Option<Font> {
    let clean = name.trim_end_matches(".hfnt");
    let candidates = [
        format!("/usr/share/nook/fonts/{}.hfnt", clean),
        format!("/usr/share/fonts/{}.hfnt", clean),
        format!("/usr/share/nook/fonts/{}", clean),
        format!("/usr/share/fonts/{}", clean),
    ];
    for path in &candidates {
        if let Some(f) = load_font_file(path) {
            return Some(f);
        }
    }
    None
}

pub fn measure_text(font: &Font, text: &str) -> i32 {
    font.measure(text)
}

pub fn line_height(font: &Font) -> i32 {
    font.height() + 2
}

pub fn measure_text_bounds(font: &Font, text: &str) -> (i32, i32) {
    let lh = line_height(font);
    let mut max_w = 0;
    let mut lines = 0;
    for line in text.lines() {
        lines += 1;
        let w = font.measure(line);
        if w > max_w {
            max_w = w;
        }
    }
    if lines == 0 {
        lines = 1;
    }
    (max_w, lines * lh)
}

pub fn wrap_text(text: &str, font: &Font, max_width: i32) -> Vec<String> {
    if max_width <= 0 {
        return text.lines().map(ToString::to_string).collect();
    }
    let mut result = Vec::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_w = 0;
        for word in raw_line.split_whitespace() {
            let space_w = if current.is_empty() { 0 } else { font.measure(" ") };
            let word_w = font.measure(word);
            if current.is_empty() {
                current.push_str(word);
                current_w = word_w;
            } else if current_w + space_w + word_w <= max_width {
                current.push(' ');
                current.push_str(word);
                current_w += space_w + word_w;
            } else {
                result.push(current);
                current = String::from(word);
                current_w = word_w;
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    result
}

pub fn ellipsize_text(text: &str, font: &Font, max_width: i32) -> String {
    if max_width <= 0 || font.measure(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ell_w = font.measure(ellipsis);
    if ell_w >= max_width {
        return ellipsis.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let mut test = out.clone();
        test.push(ch);
        if font.measure(&test) + ell_w > max_width {
            break;
        }
        out.push(ch);
    }
    out.push_str(ellipsis);
    out
}

#[derive(Default)]
pub struct FontStore {
    fonts: BTreeMap<String, Font>,
}

impl FontStore {
    pub fn new() -> Self {
        Self {
            fonts: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, name: impl Into<String>, font: Font) {
        self.fonts.insert(name.into(), font);
    }

    pub fn load_file(&mut self, name: impl Into<String>, path: &str) -> bool {
        if let Some(font) = load_font_file(path) {
            self.fonts.insert(name.into(), font);
            true
        } else {
            false
        }
    }

    pub fn load_system(&mut self, name: &str) -> bool {
        if let Some(font) = load_system_font(name) {
            self.fonts.insert(String::from(name), font);
            true
        } else {
            false
        }
    }

    pub fn get(&self, name: &str) -> Option<&Font> {
        self.fonts.get(name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.fonts.contains_key(name)
    }
}
