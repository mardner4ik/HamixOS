use alloc::vec::Vec;

use crate::element::{Element, ImageSource, Stack, Text};
use crate::style::{Align, Axis, Length, TextSize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, ..*self }
    }

    pub fn inset(&self, left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { x: self.x + left, y: self.y + top, w: (self.w - left - right).max(0), h: (self.h - top - bottom).max(0) }
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Rect { x, y, w: (r - x).max(0), h: (b - y).max(0) }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

pub trait Metrics {
    fn text_width(&self, text: &str, size: TextSize) -> i32;
    fn line_height(&self, size: TextSize) -> i32;
    fn image_size(&self, source: &ImageSource) -> (i32, i32);
    fn sizes(&self) -> &Sizes;
}

#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub control: i32,
    pub button_padding: i32,
    pub button_min: i32,
    pub icon_button: i32,
    pub input_width: i32,
    pub check_box: i32,
    pub check_gap: i32,
    pub switch: (i32, i32),
    pub slider_height: i32,
    pub progress_height: i32,
    pub dropdown_extra: i32,
    pub row_height: i32,
    pub row_padding: i32,
    pub list_default: (i32, i32),
    pub editor_default: (i32, i32),
    pub scrollbar: i32,
}

impl Default for Sizes {
    fn default() -> Sizes {
        Sizes {
            control: 34,
            button_padding: 16,
            button_min: 72,
            icon_button: 34,
            input_width: 220,
            check_box: 18,
            check_gap: 10,
            switch: (44, 24),
            slider_height: 24,
            progress_height: 8,
            dropdown_extra: 44,
            row_height: 34,
            row_padding: 10,
            list_default: (240, 200),
            editor_default: (420, 260),
            scrollbar: 8,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub rect: Rect,
    pub content: (i32, i32),
    pub children: Vec<Node>,
}

pub fn wrap_lines<'a>(text: &'a str, max: i32, width: impl Fn(&str) -> i32) -> Vec<&'a str> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if max <= 0 || width(paragraph) <= max {
            out.push(paragraph);
            continue;
        }
        let mut start = 0usize;
        let mut last_break = None;
        let mut idx = 0usize;
        let bytes = paragraph.as_bytes();
        while idx < bytes.len() {
            if bytes[idx] == b' ' {
                last_break = Some(idx);
            }
            let ch_len = paragraph[idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            let candidate = &paragraph[start..idx + ch_len];
            if width(candidate) > max && idx > start {
                let cut = match last_break {
                    Some(b) if b > start => b,
                    _ => idx,
                };
                out.push(&paragraph[start..cut]);
                start = if cut < bytes.len() && bytes[cut] == b' ' { cut + 1 } else { cut };
                last_break = None;
                idx = start;
                continue;
            }
            idx += ch_len;
        }
        out.push(&paragraph[start..]);
    }
    out
}

fn text_size<T: Metrics>(t: &Text, max_w: i32, m: &T) -> (i32, i32) {
    let lh = m.line_height(t.size);
    if t.wrap {
        let lines = wrap_lines(&t.content, max_w, |s| m.text_width(s, t.size));
        let w = lines.iter().map(|l| m.text_width(l, t.size)).max().unwrap_or(0);
        (w.min(max_w.max(0)), lh * lines.len().max(1) as i32)
    } else {
        let lines: Vec<&str> = t.content.split('\n').collect();
        let w = lines.iter().map(|l| m.text_width(l, t.size)).max().unwrap_or(0);
        (w, lh * lines.len().max(1) as i32)
    }
}

fn fixed_or(len: Length, natural: i32) -> i32 {
    match len {
        Length::Fixed(px) => px as i32,
        _ => natural,
    }
}

pub fn intrinsic<M, T: Metrics>(e: &Element<M>, max_w: i32, m: &T) -> (i32, i32) {
    let s = m.sizes();
    let (w, h) = match e {
        Element::Text(t) => text_size(t, max_w, m),
        Element::Button(b) => {
            if b.label.is_empty() {
                (s.icon_button, s.icon_button)
            } else {
                let icon = if b.icon.is_some() { 24 } else { 0 };
                ((m.text_width(&b.label, b.size) + s.button_padding * 2 + icon).max(s.button_min), s.control.max(m.line_height(b.size) + 12))
            }
        }
        Element::Input(_) => (s.input_width, s.control),
        Element::Editor(_) => s.editor_default,
        Element::Checkbox(c) => {
            let label = if c.label.is_empty() { 0 } else { s.check_gap + m.text_width(&c.label, TextSize::Body) };
            (s.check_box + label, s.check_box.max(m.line_height(TextSize::Body)) + 4)
        }
        Element::Switch(_) => s.switch,
        Element::Slider(_) => (180, s.slider_height),
        Element::Progress(_) => (160, s.progress_height),
        Element::Dropdown(d) => {
            let widest = d.options.iter().map(|o| m.text_width(o, TextSize::Body)).max().unwrap_or(60);
            (widest + s.dropdown_extra, s.control)
        }
        Element::List(l) if l.grid.is_some() => {
            let (cw, ch) = l.grid.map(|(w, h)| (w as i32, h as i32)).unwrap_or((96, 96));
            let inner_w = (max_w - s.scrollbar - 8).max(cw);
            let columns = grid_columns(inner_w, cw);
            let gap = if columns > 1 { ((inner_w - columns * cw) / (columns - 1)).clamp(4, 24) } else { 0 };
            let rows = (l.rows.len() as i32 + columns - 1) / columns;
            (max_w, rows.max(1) * (ch + gap) + 8)
        }
        Element::List(_) => s.list_default,
        Element::Stack(stack) => stack_intrinsic(stack, max_w, m),
        Element::Scroll(sc) => {
            let (cw, ch) = intrinsic(&sc.child, max_w, m);
            (cw + s.scrollbar, ch.min(240))
        }
        Element::Space(_) => (0, 0),
        Element::Divider => (1, 1),
        Element::Image(i) => m.image_size(&i.source),
        Element::Canvas(c) => (c.pixmap.width() as i32, c.pixmap.height() as i32),
    };
    (fixed_or(e.width(), w), fixed_or(e.height(), h))
}

fn stack_intrinsic<M, T: Metrics>(stack: &Stack<M>, max_w: i32, m: &T) -> (i32, i32) {
    let pad_w = stack.padding.horizontal();
    let pad_h = stack.padding.vertical();
    let inner_max = (match stack.width {
        Length::Fixed(px) => px as i32,
        _ => max_w,
    } - pad_w)
        .max(0);
    let gaps = stack.spacing as i32 * (stack.children.len().saturating_sub(1)) as i32;
    match stack.axis {
        Axis::Horizontal => {
            let sizes: Vec<(i32, i32)> = stack.children.iter().map(|c| intrinsic(c, inner_max, m)).collect();
            let fixed: i32 = stack.children.iter().zip(&sizes).filter(|(c, _)| !c.width().fills()).map(|(_, s)| s.0).sum();
            let portions: u32 = stack.children.iter().filter(|c| c.width().fills()).map(|c| c.width().portion()).sum();
            let free = (inner_max - fixed - gaps).max(0);
            let mut w = 0;
            let mut h = 0;
            for (child, (cw, ch)) in stack.children.iter().zip(sizes) {
                w += cw;
                let ch = if child.width().fills() && portions > 0 { intrinsic(child, (free as i64 * child.width().portion() as i64 / portions as i64) as i32, m).1 } else { ch };
                h = h.max(ch);
            }
            (w + gaps + pad_w, h + pad_h)
        }
        Axis::Vertical => {
            let mut w = 0;
            let mut h = 0;
            for child in &stack.children {
                let (cw, ch) = intrinsic(child, inner_max, m);
                w = w.max(cw);
                h += ch;
            }
            (w + pad_w, h + gaps + pad_h)
        }
    }
}

pub fn layout<M, T: Metrics>(e: &Element<M>, rect: Rect, m: &T) -> Node {
    match e {
        Element::Stack(stack) => layout_stack(stack, rect, m),
        Element::Scroll(sc) => {
            let viewport_w = (rect.w - m.sizes().scrollbar).max(0);
            let (_, natural_h) = intrinsic(&sc.child, viewport_w, m);
            let child_h = if sc.child.height().fills() { natural_h.max(rect.h) } else { natural_h };
            let child = layout(&sc.child, Rect::new(rect.x, rect.y, viewport_w, child_h), m);
            Node { rect, content: (viewport_w, child_h), children: alloc::vec![child] }
        }
        Element::List(list) if list.grid.is_some() => {
            let s = m.sizes();
            let (cw, ch) = list.grid.map(|(w, h)| (w as i32, h as i32)).unwrap_or((96, 96));
            let inner_w = (rect.w - s.scrollbar - 8).max(cw);
            let columns = grid_columns(inner_w, cw);
            let gap = if columns > 1 { ((inner_w - columns * cw) / (columns - 1)).clamp(4, 24) } else { 0 };
            let mut children = Vec::with_capacity(list.rows.len());
            for (i, row) in list.rows.iter().enumerate() {
                let (cx, cy) = ((i as i32) % columns, (i as i32) / columns);
                let cell = Rect::new(rect.x + 4 + cx * (cw + gap), rect.y + 4 + cy * (ch + gap), cw, ch);
                let inner = Rect::new(cell.x + 6, cell.y + 6, cw - 12, ch - 12);
                children.push(Node { rect: cell, content: (inner.w, inner.h), children: alloc::vec![layout(row, inner, m)] });
            }
            let rows = (list.rows.len() as i32 + columns - 1) / columns;
            Node { rect, content: (inner_w, rows * (ch + gap) + 8), children }
        }
        Element::List(list) => {
            let s = m.sizes();
            let inner_w = (rect.w - s.scrollbar - s.row_padding).max(0);
            let mut y = rect.y + 4;
            let mut children = Vec::with_capacity(list.rows.len());
            for row in &list.rows {
                let (_, natural) = intrinsic(row, inner_w - s.row_padding * 2, m);
                let rh = list.row_height.map(|h| h as i32).unwrap_or(natural + 12).max(s.row_height);
                let row_rect = Rect::new(rect.x + 4, y, inner_w, rh);
                let content_rect = Rect::new(row_rect.x + s.row_padding, row_rect.y, (row_rect.w - s.row_padding * 2).max(0), rh);
                let content = layout(row, content_rect, m);
                children.push(Node { rect: row_rect, content: (content_rect.w, rh), children: alloc::vec![content] });
                y += rh + 2;
            }
            Node { rect, content: (inner_w, y - rect.y + 2), children }
        }
        Element::Text(t) if t.wrap => {
            let (w, h) = text_size(t, rect.w, m);
            Node { rect, content: (w, h), children: Vec::new() }
        }
        _ => Node { rect, content: (rect.w, rect.h), children: Vec::new() },
    }
}

fn aligned(align: Align, start: i32, room: i32, size: i32) -> i32 {
    match align {
        Align::Start | Align::Stretch => start,
        Align::Center => start + (room - size) / 2,
        Align::End => start + room - size,
    }
}

fn layout_stack<M, T: Metrics>(stack: &Stack<M>, rect: Rect, m: &T) -> Node {
    let inner = rect.inset(stack.padding.left as i32, stack.padding.top as i32, stack.padding.right as i32, stack.padding.bottom as i32);
    let n = stack.children.len();
    if n == 0 {
        return Node { rect, content: (0, 0), children: Vec::new() };
    }
    let horizontal = stack.axis == Axis::Horizontal;
    let main_total = if horizontal { inner.w } else { inner.h };
    let cross_total = if horizontal { inner.h } else { inner.w };
    let gaps = stack.spacing as i32 * (n as i32 - 1);
    let mut main_sizes: Vec<i32> = Vec::with_capacity(n);
    let mut cross_sizes: Vec<i32> = Vec::with_capacity(n);
    let mut portions = 0u32;
    let mut used = 0;
    for child in &stack.children {
        let main_len = if horizontal { child.width() } else { child.height() };
        let cross_len = if horizontal { child.height() } else { child.width() };
        let cross_fill = cross_len.fills() || matches!(child, Element::Divider) || (stack.align == Align::Stretch && !matches!(cross_len, Length::Fixed(_)));
        let (nw, nh) = intrinsic(child, if horizontal { main_total } else { cross_total }, m);
        let (natural_main, natural_cross) = if horizontal { (nw, nh) } else { (nh, nw) };
        let cross = if cross_fill { cross_total } else { natural_cross.min(cross_total) };
        cross_sizes.push(cross);
        if main_len.fills() {
            portions += main_len.portion();
            main_sizes.push(0);
        } else {
            let size = if !horizontal && cross_fill && matches!(child, Element::Text(t) if t.wrap) { intrinsic(child, cross, m).1 } else { natural_main };
            used += size;
            main_sizes.push(size);
        }
    }
    let free = (main_total - used - gaps).max(0);
    if portions > 0 {
        let mut handed = 0;
        let fill_count = stack.children.iter().filter(|c| (if horizontal { c.width() } else { c.height() }).fills()).count();
        let mut seen = 0;
        for (i, child) in stack.children.iter().enumerate() {
            let main_len = if horizontal { child.width() } else { child.height() };
            if main_len.fills() {
                seen += 1;
                let share = if seen == fill_count { free - handed } else { (free as i64 * main_len.portion() as i64 / portions as i64) as i32 };
                main_sizes[i] = share;
                handed += share;
            }
        }
    }
    if horizontal {
        for (i, child) in stack.children.iter().enumerate() {
            let cross_fill = child.height().fills() || matches!(child, Element::Divider) || (stack.align == Align::Stretch && !matches!(child.height(), Length::Fixed(_)));
            if child.width().fills() && !cross_fill {
                cross_sizes[i] = intrinsic(child, main_sizes[i], m).1.min(cross_total);
            }
        }
    }
    let content_main: i32 = main_sizes.iter().sum::<i32>() + gaps;
    let mut cursor = 0;
    let mut children = Vec::with_capacity(n);
    for (i, child) in stack.children.iter().enumerate() {
        let main = main_sizes[i];
        let cross = cross_sizes[i].min(cross_total).max(0);
        let cross_pos = aligned(stack.align, 0, cross_total, cross);
        let child_rect = if horizontal { Rect::new(inner.x + cursor, inner.y + cross_pos, main, cross) } else { Rect::new(inner.x + cross_pos, inner.y + cursor, cross, main) };
        children.push(layout(child, child_rect, m));
        cursor += main + stack.spacing as i32;
    }
    let content = if horizontal { (content_main + stack.padding.horizontal(), rect.h) } else { (rect.w, content_main + stack.padding.vertical()) };
    Node { rect, content, children }
}

pub fn grid_columns(width: i32, cell: i32) -> i32 {
    ((width + 4) / (cell + 4)).max(1)
}
