use alloc::string::String;
use alloc::vec::Vec;

use super::posix;
use super::theme::{Theme, RADIUS};
use vellum::{Area, Font, Painter};

const PAD: i32 = 8;
const UNDO_LIMIT: usize = 200;
const MERGE_MS: u64 = 1500;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Other,
}

#[derive(Clone, Default)]
pub struct EditorState {
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub scroll: i32,
    pub scroll_x: i32,
    goal_x: Option<i32>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    last: Option<(EditKind, u64)>,
    known: Option<String>,
}

pub struct Look<'a> {
    pub font: &'a Font,
    pub line_numbers: bool,
    pub highlight_line: bool,
    pub read_only: bool,
}

fn starts(text: &str) -> Vec<usize> {
    let mut v = alloc::vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

fn line_of(starts: &[usize], at: usize) -> usize {
    starts.iter().rposition(|s| *s <= at).unwrap_or(0)
}

fn line_end(text: &str, starts: &[usize], line: usize) -> usize {
    starts.get(line + 1).map(|s| s - 1).unwrap_or(text.len())
}

fn prev(text: &str, at: usize) -> usize {
    text[..at].char_indices().next_back().map(|(i, _)| i).unwrap_or(0)
}

fn next(text: &str, at: usize) -> usize {
    text[at..].chars().next().map(|c| at + c.len_utf8()).unwrap_or(text.len())
}

fn column_at(font: &Font, line: &str, x: i32) -> usize {
    let mut acc = 0;
    for (i, ch) in line.char_indices() {
        let w = font.advance(ch);
        if acc + w / 2 > x {
            return i;
        }
        acc += w;
    }
    line.len()
}

impl EditorState {
    pub fn sync(&mut self, text: &str) {
        if self.known.as_deref() != Some(text) {
            if self.known.is_some() {
                self.undo.clear();
                self.redo.clear();
                self.anchor = None;
            }
            self.known = Some(String::from(text));
        }
        self.cursor = self.cursor.min(text.len());
        while !text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
        if let Some(a) = self.anchor {
            let mut a = a.min(text.len());
            while !text.is_char_boundary(a) {
                a -= 1;
            }
            self.anchor = Some(a);
        }
    }

    fn accept(&mut self, text: &str) {
        self.known = Some(String::from(text));
    }

    pub fn position(&self, text: &str) -> (usize, usize) {
        let s = starts(text);
        let line = line_of(&s, self.cursor);
        (line + 1, text[s[line]..self.cursor].chars().count() + 1)
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cursor).then(|| (a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected<'a>(&self, text: &'a str) -> &'a str {
        self.selection().map(|(a, b)| &text[a..b]).unwrap_or("")
    }

    fn remember(&mut self, text: &str, kind: EditKind) {
        let now = posix::now_ms();
        let merge = kind == EditKind::Typing && matches!(self.last, Some((EditKind::Typing, t)) if now.saturating_sub(t) < MERGE_MS) && !self.undo.is_empty();
        self.last = Some((kind, now));
        if !merge {
            self.undo.push((String::from(text), self.cursor));
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }

    fn delete_selection(&mut self, text: &mut String) -> bool {
        match self.selection() {
            Some((a, b)) => {
                text.replace_range(a..b, "");
                self.cursor = a;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    pub fn insert(&mut self, text: &mut String, piece: &str) {
        self.remember(text, EditKind::Other);
        self.delete_selection(text);
        let clean: String = piece.chars().filter(|c| *c != '\r').map(|c| if c == '\t' { ' ' } else { c }).collect();
        text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        self.accept(text);
    }

    pub fn select_all(&mut self, text: &str) {
        self.anchor = Some(0);
        self.cursor = text.len();
    }

    pub fn cut(&mut self, text: &mut String) -> Option<String> {
        let piece = String::from(self.selected(text));
        if piece.is_empty() {
            return None;
        }
        self.remember(text, EditKind::Other);
        self.delete_selection(text);
        self.accept(text);
        Some(piece)
    }

    pub fn delete(&mut self, text: &mut String) -> bool {
        if self.selection().is_none() {
            return false;
        }
        self.remember(text, EditKind::Other);
        self.delete_selection(text);
        self.accept(text);
        true
    }

    pub fn restore(&mut self, text: &mut String, undo: bool) -> bool {
        let source = if undo { self.undo.pop() } else { self.redo.pop() };
        let Some((old, cursor)) = source else {
            return false;
        };
        let current = (core::mem::replace(text, old), self.cursor);
        if undo {
            self.redo.push(current);
        } else {
            self.undo.push(current);
        }
        self.cursor = cursor.min(text.len());
        self.anchor = None;
        self.last = None;
        self.accept(text);
        true
    }

    pub fn find(&mut self, text: &str, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        let hay = text.to_lowercase();
        let needle = query.to_lowercase();
        if hay.len() != text.len() {
            return false;
        }
        let from = match self.selection() {
            Some((a, _)) if hay[a..].starts_with(&needle) => a + 1,
            _ => self.cursor,
        };
        let from = (from..=hay.len()).find(|i| hay.is_char_boundary(*i)).unwrap_or(hay.len());
        let found = hay[from..].find(&needle).map(|i| i + from).or_else(|| hay.find(&needle));
        match found {
            Some(i) => {
                self.anchor = Some(i);
                self.cursor = i + needle.len();
                true
            }
            None => false,
        }
    }

    fn vertical(&mut self, text: &str, font: &Font, delta: i32) {
        let s = starts(text);
        let line = line_of(&s, self.cursor);
        let goal = self.goal_x.unwrap_or_else(|| font.measure(&text[s[line]..self.cursor]));
        let target = (line as i32 + delta).clamp(0, s.len() as i32 - 1) as usize;
        let content = &text[s[target]..line_end(text, &s, target)];
        self.cursor = s[target] + column_at(font, content, goal);
        self.goal_x = Some(goal);
    }

    pub fn key(&mut self, text: &mut String, look: &Look, code: i32, shift: bool, page: i32) -> bool {
        let moving = matches!(code, -9..=-1 | 1 | 5) && code != -7;
        if moving {
            if shift && self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
            if !shift {
                if let Some((a, b)) = self.selection() {
                    if code == -3 || code == -4 {
                        self.cursor = if code == -3 { a } else { b };
                        self.anchor = None;
                        return false;
                    }
                }
                self.anchor = None;
            }
        }
        if !matches!(code, -1 | -2 | -8 | -9) {
            self.goal_x = None;
        }
        let s = starts(text);
        let line = line_of(&s, self.cursor);
        match code {
            -1 => self.vertical(text, look.font, -1),
            -2 => self.vertical(text, look.font, 1),
            -8 => self.vertical(text, look.font, -page.max(1)),
            -9 => self.vertical(text, look.font, page.max(1)),
            -3 => self.cursor = prev(text, self.cursor),
            -4 => self.cursor = next(text, self.cursor),
            -5 | 1 => self.cursor = s[line],
            -6 | 5 => self.cursor = line_end(text, &s, line),
            27 => self.anchor = None,
            _ if look.read_only => return false,
            10 => {
                self.remember(text, EditKind::Other);
                self.delete_selection(text);
                let here = starts(text);
                let indent: String = text[here[line_of(&here, self.cursor)]..].chars().take_while(|c| *c == ' ').collect();
                let piece = alloc::format!("\n{}", indent);
                text.insert_str(self.cursor, &piece);
                self.cursor += piece.len();
                self.accept(text);
                return true;
            }
            9 => {
                self.remember(text, EditKind::Typing);
                self.delete_selection(text);
                text.insert_str(self.cursor, "    ");
                self.cursor += 4;
                self.accept(text);
                return true;
            }
            8 | -7 => {
                if self.selection().is_some() {
                    self.remember(text, EditKind::Other);
                    self.delete_selection(text);
                } else if code == 8 && self.cursor > 0 {
                    self.remember(text, EditKind::Typing);
                    let start = prev(text, self.cursor);
                    text.replace_range(start..self.cursor, "");
                    self.cursor = start;
                } else if code == -7 && self.cursor < text.len() {
                    self.remember(text, EditKind::Typing);
                    let end = next(text, self.cursor);
                    text.replace_range(self.cursor..end, "");
                } else {
                    return false;
                }
                self.accept(text);
                return true;
            }
            c if (32..127).contains(&c) || c >= 160 => {
                let Some(ch) = char::from_u32(c as u32) else {
                    return false;
                };
                self.remember(text, if ch == ' ' { EditKind::Other } else { EditKind::Typing });
                self.delete_selection(text);
                text.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
                self.accept(text);
                return true;
            }
            _ => {}
        }
        false
    }

    fn gutter(look: &Look, text: &str) -> i32 {
        if look.line_numbers {
            let digits = alloc::format!("{}", starts(text).len()).len().max(3) as i32;
            digits * look.font.advance('0') + 18
        } else {
            0
        }
    }

    pub fn offset_at(&self, text: &str, look: &Look, area: Area, x: i32, y: i32) -> usize {
        let s = starts(text);
        let lh = look.font.height() + 2;
        let line = ((y - area.y - PAD + self.scroll) / lh).clamp(0, s.len() as i32 - 1) as usize;
        let content = &text[s[line]..line_end(text, &s, line)];
        let local = x - area.x - PAD - Self::gutter(look, text) + self.scroll_x;
        s[line] + column_at(look.font, content, local)
    }

    pub fn press(&mut self, text: &str, look: &Look, area: Area, x: i32, y: i32, extend: bool) {
        let at = self.offset_at(text, look, area, x, y);
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = Some(at);
        }
        self.cursor = at;
        self.goal_x = None;
    }

    pub fn drag(&mut self, text: &str, look: &Look, area: Area, x: i32, y: i32) {
        self.cursor = self.offset_at(text, look, area, x, y);
    }

    pub fn wheel(&mut self, text: &str, font: &Font, area: Area, delta: i32) {
        let lh = font.height() + 2;
        let total = starts(text).len() as i32 * lh + PAD * 2;
        self.scroll = (self.scroll + delta * lh * 3).clamp(0, (total - area.h).max(0));
    }

    pub fn page_lines(font: &Font, area: Area) -> i32 {
        (area.h - PAD * 2) / (font.height() + 2)
    }

    pub fn paint(&mut self, p: &mut Painter, th: &Theme, look: &Look, area: Area, text: &str, focused: bool) {
        let font = look.font;
        p.rounded(area, RADIUS, if look.read_only { th.p.surface } else { th.p.bg }, 255);
        p.rounded_border(area, RADIUS, if focused { th.p.accent } else { th.p.border }, 255);
        let lh = font.height() + 2;
        let s = starts(text);
        let line = line_of(&s, self.cursor);
        let gutter = Self::gutter(look, text);
        let visible_h = area.h - PAD * 2;
        let visible_w = area.w - PAD * 2 - gutter - 6;
        let cursor_y = line as i32 * lh;
        let cursor_x = font.measure(&text[s[line]..self.cursor]);
        if focused {
            if cursor_y - self.scroll > visible_h - lh {
                self.scroll = cursor_y - visible_h + lh;
            }
            if cursor_y < self.scroll {
                self.scroll = cursor_y;
            }
            if cursor_x - self.scroll_x > visible_w {
                self.scroll_x = cursor_x - visible_w + 40;
            }
            if cursor_x < self.scroll_x {
                self.scroll_x = (cursor_x - 40).max(0);
            }
        }
        let total = s.len() as i32 * lh;
        self.scroll = self.scroll.clamp(0, (total - visible_h).max(0));
        let saved = p.clip;
        let inner = Area::new(area.x + 2, area.y + 2, area.w - 4, area.h - 4);
        p.set_clip(inner.intersect(&saved));
        if look.line_numbers {
            p.fill(Area::new(inner.x, inner.y, gutter, inner.h), th.p.surface);
            p.fill(Area::new(inner.x + gutter, inner.y, 1, inner.h), th.p.border);
        }
        let selection = self.selection();
        let first = (self.scroll / lh).max(0) as usize;
        let last = ((self.scroll + visible_h) / lh + 1).max(0) as usize;
        let text_x = area.x + PAD + gutter - self.scroll_x;
        let text_clip = Area::new(area.x + PAD + gutter - 2, inner.y, area.w - PAD - gutter, inner.h).intersect(&inner);
        for i in first..s.len().min(last + 1) {
            let y = area.y + PAD + i as i32 * lh - self.scroll;
            let (start, end) = (s[i], line_end(text, &s, i));
            if look.line_numbers {
                let label = alloc::format!("{}", i + 1);
                let w = font.measure(&label);
                p.text(font, inner.x + gutter - 8 - w, y, &label, if i == line { th.p.dim } else { th.p.faint });
            }
            let saved_line = p.clip;
            p.set_clip(text_clip.intersect(&saved_line));
            if look.highlight_line && focused && i == line && selection.is_none() {
                p.fill(Area::new(text_clip.x, y - 1, text_clip.w, lh), th.p.surface);
            }
            if let Some((a, b)) = selection {
                if a <= end && b >= start {
                    let from = a.max(start);
                    let to = b.min(end);
                    let x0 = text_x + font.measure(&text[start..from]);
                    let x1 = text_x + font.measure(&text[start..to]) + if b > end { font.advance(' ') } else { 0 };
                    p.fill(Area::new(x0, y - 1, (x1 - x0).max(1), lh), th.p.selection);
                }
            }
            p.text(font, text_x, y, &text[start..end], th.p.text);
            p.clip = saved_line;
        }
        if focused && !look.read_only {
            p.fill(Area::new(text_x + cursor_x, area.y + PAD + cursor_y - self.scroll - 1, 1, font.height() + 2), th.p.accent_hover);
        }
        p.clip = saved;
        th.scrollbar(p, Area::new(area.right() - 7, area.y + 4, 4, area.h - 8), self.scroll, total, visible_h);
    }
}
