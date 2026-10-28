use alloc::string::String;
use vellum::gfx::ellipsize;
use vellum::{Area, Font, Painter};

use super::theme::Theme;

#[derive(Clone, Default)]
pub struct Field {
    pub text: String,
    pub cursor: usize,
    pub scroll: i32,
    sent: String,
}

pub enum FieldAction {
    None,
    Changed,
    Submit,
    Next,
}

impl Field {
    pub fn new(text: &str) -> Field {
        Field { text: String::from(text), cursor: text.chars().count(), scroll: 0, sent: String::from(text) }
    }

    pub fn sync(&mut self, value: &str) {
        if self.text != value && self.sent != value {
            self.text = String::from(value);
            self.sent = String::from(value);
            self.cursor = self.len();
        }
    }

    pub fn sent(&mut self) {
        self.sent = self.text.clone();
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    fn shown(&self, secret: bool) -> String {
        if secret { self.text.chars().map(|_| '•').collect() } else { self.text.clone() }
    }

    pub fn insert(&mut self, piece: &str) {
        let clean: String = piece.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    pub fn key(&mut self, code: i32) -> FieldAction {
        let len = self.len();
        match code {
            10 => FieldAction::Submit,
            9 => FieldAction::Next,
            8 if self.cursor > 0 => {
                let (a, b) = (self.byte(self.cursor - 1), self.byte(self.cursor));
                self.text.replace_range(a..b, "");
                self.cursor -= 1;
                FieldAction::Changed
            }
            -7 if self.cursor < len => {
                let (a, b) = (self.byte(self.cursor), self.byte(self.cursor + 1));
                self.text.replace_range(a..b, "");
                FieldAction::Changed
            }
            -3 => {
                self.cursor = self.cursor.saturating_sub(1);
                FieldAction::None
            }
            -4 => {
                self.cursor = (self.cursor + 1).min(len);
                FieldAction::None
            }
            -5 | 1 => {
                self.cursor = 0;
                FieldAction::None
            }
            -6 | 5 => {
                self.cursor = len;
                FieldAction::None
            }
            21 => {
                self.text.clear();
                self.cursor = 0;
                FieldAction::Changed
            }
            c if (32..127).contains(&c) || c >= 160 => match char::from_u32(c as u32) {
                Some(ch) => {
                    let at = self.byte(self.cursor);
                    self.text.insert(at, ch);
                    self.cursor += 1;
                    FieldAction::Changed
                }
                None => FieldAction::None,
            },
            _ => FieldAction::None,
        }
    }

    pub fn click(&mut self, font: &Font, secret: bool, local_x: i32) {
        let mut x = -self.scroll;
        let mut index = 0;
        for ch in self.shown(secret).chars() {
            let w = font.advance(ch);
            if x + w / 2 > local_x {
                break;
            }
            x += w;
            index += 1;
        }
        self.cursor = index;
    }

    pub fn paint(&mut self, p: &mut Painter, th: &Theme, area: Area, focused: bool, secret: bool, placeholder: &str) {
        th.field_frame(p, area, focused);
        let inner = area.inset(10);
        let font = &th.fonts.body;
        let shown = self.shown(secret);
        let prefix: String = shown.chars().take(self.cursor).collect();
        let cursor_x = font.measure(&prefix);
        if cursor_x - self.scroll > inner.w - 2 {
            self.scroll = cursor_x - inner.w + 2;
        }
        if cursor_x - self.scroll < 0 {
            self.scroll = cursor_x;
        }
        let saved = p.clip;
        p.set_clip(Area::new(area.x + 8, area.y, area.w - 16, area.h).intersect(&saved));
        let ty = area.y + (area.h - font.height()) / 2;
        if shown.is_empty() {
            p.text(font, inner.x, ty, &ellipsize(font, placeholder, inner.w), th.p.faint);
        } else {
            p.text(font, inner.x - self.scroll, ty, &shown, th.p.text);
        }
        if focused {
            p.fill(Area::new(inner.x - self.scroll + cursor_x, ty - 1, 1, font.height() + 2), th.p.accent_hover);
        }
        p.clip = saved;
    }
}
