#![no_std]

extern crate alloc;

mod glyph;
mod input;
mod width;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

pub use input::{encode_focus, encode_key, encode_mouse, encode_paste, Key, MouseButton, MouseEvent, MOD_ALT, MOD_CTRL, MOD_SHIFT};
pub use width::char_width;
pub use glyph::{box_lines, rasterize};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

pub const BOLD: u16 = 1;
pub const DIM: u16 = 2;
pub const ITALIC: u16 = 4;
pub const UNDERLINE: u16 = 8;
pub const BLINK: u16 = 16;
pub const REVERSE: u16 = 32;
pub const INVISIBLE: u16 = 64;
pub const STRIKE: u16 = 128;
pub const WIDE: u16 = 256;
pub const WIDE_SPACER: u16 = 512;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: u16,
}

impl Cell {
    pub const BLANK: Cell = Cell { ch: ' ', fg: Color::Default, bg: Color::Default, flags: 0 };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseMode {
    Off,
    X10,
    Normal,
    Button,
    Any,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseEncoding {
    Default,
    Utf8,
    Sgr,
    Urxvt,
}

#[derive(Clone, Copy, Debug)]
pub struct Modes {
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub autowrap: bool,
    pub origin: bool,
    pub insert: bool,
    pub newline: bool,
    pub cursor_visible: bool,
    pub cursor_blink: bool,
    pub reverse_video: bool,
    pub bracketed_paste: bool,
    pub focus_events: bool,
    pub alt_screen: bool,
    pub mouse: MouseMode,
    pub mouse_encoding: MouseEncoding,
    pub alternate_scroll: bool,
}

impl Modes {
    const fn new() -> Modes {
        Modes {
            app_cursor: false,
            app_keypad: false,
            autowrap: true,
            origin: false,
            insert: false,
            newline: false,
            cursor_visible: true,
            cursor_blink: true,
            reverse_video: false,
            bracketed_paste: false,
            focus_events: false,
            alt_screen: false,
            mouse: MouseMode::Off,
            mouse_encoding: MouseEncoding::Default,
            alternate_scroll: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Ascii,
    Graphics,
    Uk,
}

#[derive(Clone, Copy)]
struct Cursor {
    row: usize,
    col: usize,
    pen: Cell,
    pending_wrap: bool,
    origin: bool,
    charsets: [Charset; 4],
    shift: usize,
}

impl Cursor {
    const fn new() -> Cursor {
        Cursor { row: 0, col: 0, pen: Cell::BLANK, pending_wrap: false, origin: false, charsets: [Charset::Ascii; 4], shift: 0 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    CsiIgnore,
    Osc,
    OscEscape,
    StringIgnore,
    StringEscape,
}

const MAX_PARAMS: usize = 32;

pub struct Term {
    cols: usize,
    rows: usize,
    grid: Vec<Cell>,
    saved_screen: Option<Vec<Cell>>,
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_limit: usize,
    cursor: Cursor,
    saved_main: Cursor,
    saved_alt: Cursor,
    top: usize,
    bottom: usize,
    tabs: Vec<bool>,
    modes: Modes,
    state: State,
    params: [u32; MAX_PARAMS],
    colon: [bool; MAX_PARAMS],
    param_count: usize,
    param_started: bool,
    private: u8,
    intermediate: u8,
    osc: Vec<u8>,
    utf8: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
    last_char: char,
    dirty: Vec<bool>,
    responses: Vec<u8>,
    title: Option<String>,
    bell: bool,
    scrolled: usize,
    cleared_scrollback: bool,
}

impl Term {
    pub fn new(cols: usize, rows: usize, scrollback_limit: usize) -> Term {
        let cols = cols.max(2);
        let rows = rows.max(2);
        Term {
            cols,
            rows,
            grid: alloc::vec![Cell::BLANK; cols * rows],
            saved_screen: None,
            scrollback: VecDeque::new(),
            scrollback_limit,
            cursor: Cursor::new(),
            saved_main: Cursor::new(),
            saved_alt: Cursor::new(),
            top: 0,
            bottom: rows - 1,
            tabs: default_tabs(cols),
            modes: Modes::new(),
            state: State::Ground,
            params: [0; MAX_PARAMS],
            colon: [false; MAX_PARAMS],
            param_count: 0,
            param_started: false,
            private: 0,
            intermediate: 0,
            osc: Vec::new(),
            utf8: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
            last_char: ' ',
            dirty: alloc::vec![true; rows],
            responses: Vec::new(),
            title: None,
            bell: false,
            scrolled: 0,
            cleared_scrollback: false,
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn modes(&self) -> &Modes {
        &self.modes
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor.row, self.cursor.col.min(self.cols - 1))
    }

    pub fn cursor_visible(&self) -> bool {
        self.modes.cursor_visible
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.grid[row * self.cols + col]
    }

    pub fn row(&self, row: usize) -> &[Cell] {
        &self.grid[row * self.cols..(row + 1) * self.cols]
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn scrollback_line(&self, index: usize) -> &[Cell] {
        &self.scrollback[index]
    }

    pub fn is_dirty(&self, row: usize) -> bool {
        self.dirty[row]
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = false);
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    pub fn mark_dirty(&mut self, row: usize) {
        if row < self.rows {
            self.dirty[row] = true;
        }
    }

    pub fn take_scrolled(&mut self) -> usize {
        core::mem::replace(&mut self.scrolled, 0)
    }

    pub fn take_scrollback_cleared(&mut self) -> bool {
        core::mem::replace(&mut self.cleared_scrollback, false)
    }

    pub fn take_responses(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.responses)
    }

    pub fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }

    pub fn take_bell(&mut self) -> bool {
        core::mem::replace(&mut self.bell, false)
    }

    pub fn pen(&self) -> Cell {
        self.cursor.pen
    }

    pub fn set_pen(&mut self, pen: Cell) {
        self.cursor.pen = pen;
    }

    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback_limit = limit;
        while self.scrollback.len() > limit {
            self.scrollback.pop_front();
        }
    }

    pub fn clear_scrollback(&mut self) {
        self.scrollback.clear();
        self.cleared_scrollback = true;
    }

    pub fn reset(&mut self) {
        let limit = self.scrollback_limit;
        let scrollback = core::mem::take(&mut self.scrollback);
        *self = Term::new(self.cols, self.rows, limit);
        self.scrollback = scrollback;
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2);
        let rows = rows.max(2);
        if cols == self.cols && rows == self.rows {
            return;
        }
        let old_cols = self.cols;
        let old_rows = self.rows;
        let mut shift = 0;
        if !self.modes.alt_screen && rows < old_rows {
            let used = (self.cursor.row + 1).max(self.last_used_row() + 1).min(old_rows);
            shift = used.saturating_sub(rows).min(self.cursor.row);
            for r in 0..shift {
                let line = self.grid[r * old_cols..(r + 1) * old_cols].to_vec();
                self.push_scrollback(line);
            }
        }
        let mut pull = 0;
        if !self.modes.alt_screen && rows > old_rows && self.scrollback_limit > 0 {
            pull = (rows - old_rows).min(self.scrollback.len());
        }
        let mut grid = alloc::vec![Cell::BLANK; cols * rows];
        for i in 0..pull {
            let line = self.scrollback.pop_back().unwrap();
            let r = pull - 1 - i;
            let n = cols.min(line.len());
            grid[r * cols..r * cols + n].copy_from_slice(&line[..n]);
        }
        for r in 0..rows - pull {
            let src = r + shift;
            if src >= old_rows {
                break;
            }
            let n = cols.min(old_cols);
            let dst = (r + pull) * cols;
            grid[dst..dst + n].copy_from_slice(&self.grid[src * old_cols..src * old_cols + n]);
        }
        self.grid = grid;
        if let Some(saved) = self.saved_screen.take() {
            let mut resized = alloc::vec![Cell::BLANK; cols * rows];
            for r in 0..rows.min(old_rows) {
                let n = cols.min(old_cols);
                resized[r * cols..r * cols + n].copy_from_slice(&saved[r * old_cols..r * old_cols + n]);
            }
            self.saved_screen = Some(resized);
        }
        self.cols = cols;
        self.rows = rows;
        self.cursor.row = (self.cursor.row + pull).saturating_sub(shift).min(rows - 1);
        self.cursor.col = self.cursor.col.min(cols - 1);
        self.cursor.pending_wrap = false;
        for saved in [&mut self.saved_main, &mut self.saved_alt] {
            saved.row = saved.row.min(rows - 1);
            saved.col = saved.col.min(cols - 1);
        }
        self.top = 0;
        self.bottom = rows - 1;
        self.tabs = default_tabs(cols);
        self.dirty = alloc::vec![true; rows];
        for r in 0..rows {
            self.fix_wide_edge(r);
        }
    }

    fn fix_wide_edge(&mut self, row: usize) {
        let last = row * self.cols + self.cols - 1;
        if self.grid[last].flags & WIDE != 0 {
            self.grid[last] = Cell::BLANK;
        }
        let first = row * self.cols;
        if self.grid[first].flags & WIDE_SPACER != 0 {
            self.grid[first] = Cell::BLANK;
        }
    }

    fn last_used_row(&self) -> usize {
        (0..self.rows).rev().find(|r| self.row(*r).iter().any(|c| *c != Cell::BLANK)).unwrap_or(0)
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        if self.scrollback_limit == 0 {
            return;
        }
        self.scrollback.push_back(line);
        while self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
        }
    }

    fn blank(&self) -> Cell {
        Cell { ch: ' ', fg: Color::Default, bg: self.cursor.pen.bg, flags: 0 }
    }

    fn clear_cells(&mut self, start: usize, end: usize) {
        let blank = self.blank();
        let end = end.min(self.grid.len());
        if start >= end {
            return;
        }
        for cell in &mut self.grid[start..end] {
            *cell = blank;
        }
        for r in start / self.cols..=(end - 1) / self.cols {
            self.dirty[r] = true;
        }
    }

    fn scroll_up_region(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        if n == 0 {
            return;
        }
        if top == 0 && !self.modes.alt_screen && bottom == self.rows - 1 {
            for r in 0..n {
                let line = self.row(r).to_vec();
                self.push_scrollback(line);
            }
            self.scrolled += n;
        }
        let cols = self.cols;
        self.grid.copy_within((top + n) * cols..(bottom + 1) * cols, top * cols);
        self.clear_cells((bottom + 1 - n) * cols, (bottom + 1) * cols);
        for r in top..=bottom {
            self.dirty[r] = true;
        }
    }

    fn scroll_down_region(&mut self, top: usize, bottom: usize, n: usize) {
        let n = n.min(bottom + 1 - top);
        if n == 0 {
            return;
        }
        let cols = self.cols;
        self.grid.copy_within(top * cols..(bottom + 1 - n) * cols, (top + n) * cols);
        self.clear_cells(top * cols, (top + n) * cols);
        for r in top..=bottom {
            self.dirty[r] = true;
        }
    }

    fn line_feed(&mut self) {
        self.cursor.pending_wrap = false;
        if self.cursor.row == self.bottom {
            self.scroll_up_region(self.top, self.bottom, 1);
        } else if self.cursor.row < self.rows - 1 {
            self.set_row(self.cursor.row + 1);
        }
    }

    fn reverse_index(&mut self) {
        self.cursor.pending_wrap = false;
        if self.cursor.row == self.top {
            self.scroll_down_region(self.top, self.bottom, 1);
        } else if self.cursor.row > 0 {
            self.set_row(self.cursor.row - 1);
        }
    }

    fn set_row(&mut self, row: usize) {
        self.dirty[self.cursor.row] = true;
        self.cursor.row = row.min(self.rows - 1);
        self.dirty[self.cursor.row] = true;
    }

    fn move_to(&mut self, row: usize, col: usize) {
        let (min_row, max_row) = if self.cursor.origin { (self.top, self.bottom) } else { (0, self.rows - 1) };
        let row = if self.cursor.origin { row + self.top } else { row };
        self.set_row(row.clamp(min_row, max_row));
        self.cursor.col = col.min(self.cols - 1);
        self.cursor.pending_wrap = false;
    }

    fn translate(&self, ch: char) -> char {
        let set = self.cursor.charsets[self.cursor.shift];
        match set {
            Charset::Ascii => ch,
            Charset::Uk => {
                if ch == '#' {
                    '£'
                } else {
                    ch
                }
            }
            Charset::Graphics => match ch {
                '_' => ' ',
                '`' => '◆',
                'a' => '▒',
                'b' => '␉',
                'c' => '␌',
                'd' => '␍',
                'e' => '␊',
                'f' => '°',
                'g' => '±',
                'h' => '␤',
                'i' => '␋',
                'j' => '┘',
                'k' => '┐',
                'l' => '┌',
                'm' => '└',
                'n' => '┼',
                'o' => '⎺',
                'p' => '⎻',
                'q' => '─',
                'r' => '⎼',
                's' => '⎽',
                't' => '├',
                'u' => '┤',
                'v' => '┴',
                'w' => '┬',
                'x' => '│',
                'y' => '≤',
                'z' => '≥',
                '{' => 'π',
                '|' => '≠',
                '}' => '£',
                '~' => '·',
                other => other,
            },
        }
    }

    fn print(&mut self, ch: char) {
        let ch = self.translate(ch);
        let width = char_width(ch);
        if width == 0 {
            return;
        }
        self.last_char = ch;
        if self.cursor.pending_wrap {
            if self.modes.autowrap {
                self.cursor.col = 0;
                self.line_feed();
            }
            self.cursor.pending_wrap = false;
        }
        if width == 2 && self.cursor.col + 1 >= self.cols {
            if self.modes.autowrap {
                let idx = self.cursor.row * self.cols + self.cursor.col;
                self.grid[idx] = self.blank();
                self.cursor.col = 0;
                self.line_feed();
            } else {
                self.cursor.col = self.cols - 2;
            }
        }
        let row = self.cursor.row;
        let col = self.cursor.col;
        if self.modes.insert {
            let start = row * self.cols + col;
            let end = row * self.cols + self.cols;
            if start + width < end {
                self.grid.copy_within(start..end - width, start + width);
            }
        }
        let base = row * self.cols;
        self.clear_wide_at(row, col);
        if width == 2 {
            self.clear_wide_at(row, col + 1);
        }
        let mut cell = self.cursor.pen;
        cell.ch = ch;
        cell.flags &= !(WIDE | WIDE_SPACER);
        if width == 2 {
            cell.flags |= WIDE;
            self.grid[base + col] = cell;
            let mut spacer = cell;
            spacer.ch = ' ';
            spacer.flags = (spacer.flags & !WIDE) | WIDE_SPACER;
            self.grid[base + col + 1] = spacer;
        } else {
            self.grid[base + col] = cell;
        }
        self.dirty[row] = true;
        if col + width >= self.cols {
            self.cursor.col = self.cols - 1;
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = col + width;
        }
    }

    fn clear_wide_at(&mut self, row: usize, col: usize) {
        if col >= self.cols {
            return;
        }
        let base = row * self.cols;
        let cell = self.grid[base + col];
        if cell.flags & WIDE != 0 && col + 1 < self.cols {
            self.grid[base + col + 1] = Cell { ch: ' ', flags: cell.flags & !(WIDE | WIDE_SPACER), ..cell };
        }
        if cell.flags & WIDE_SPACER != 0 && col > 0 {
            let prev = self.grid[base + col - 1];
            self.grid[base + col - 1] = Cell { ch: ' ', flags: prev.flags & !(WIDE | WIDE_SPACER), ..prev };
        }
    }

    pub fn feed_str(&mut self, text: &str) {
        self.feed(text.as_bytes());
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }

    fn byte(&mut self, b: u8) {
        match self.state {
            State::Osc => {
                match b {
                    0x07 => {
                        self.osc_dispatch();
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::OscEscape,
                    _ => {
                        if self.osc.len() < 4096 {
                            self.osc.push(b);
                        }
                    }
                }
                return;
            }
            State::OscEscape => {
                self.osc_dispatch();
                self.state = State::Ground;
                if b != b'\\' {
                    self.byte(0x1b);
                    self.byte(b);
                }
                return;
            }
            State::StringIgnore => {
                match b {
                    0x07 => self.state = State::Ground,
                    0x1b => self.state = State::StringEscape,
                    _ => {}
                }
                return;
            }
            State::StringEscape => {
                self.state = if b == b'\\' { State::Ground } else { State::StringIgnore };
                return;
            }
            _ => {}
        }
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8[self.utf8_len] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let decoded = core::str::from_utf8(&self.utf8[..self.utf8_len]).ok().and_then(|s| s.chars().next()).unwrap_or('\u{FFFD}');
                    self.utf8_need = 0;
                    self.utf8_len = 0;
                    if self.state == State::Ground {
                        self.print(decoded);
                    }
                }
                return;
            }
            self.utf8_need = 0;
            self.utf8_len = 0;
            if self.state == State::Ground {
                self.print('\u{FFFD}');
            }
        }
        if b < 0x20 || b == 0x7f {
            self.control(b);
            return;
        }
        match self.state {
            State::Ground => {
                if b < 0x80 {
                    self.print(b as char);
                } else {
                    let need = match b {
                        0xC2..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF4 => 4,
                        _ => 0,
                    };
                    if need == 0 {
                        self.print('\u{FFFD}');
                    } else {
                        self.utf8[0] = b;
                        self.utf8_len = 1;
                        self.utf8_need = need;
                    }
                }
            }
            State::Escape => self.escape(b),
            State::EscapeIntermediate => self.escape_intermediate(b),
            State::Csi => self.csi_byte(b),
            State::CsiIgnore => {
                if (0x40..=0x7e).contains(&b) {
                    self.state = State::Ground;
                }
            }
            _ => {}
        }
    }

    fn control(&mut self, b: u8) {
        match b {
            0x1b => {
                self.state = State::Escape;
                self.intermediate = 0;
            }
            0x07 => self.bell = true,
            0x08 => {
                if self.cursor.pending_wrap {
                    self.cursor.pending_wrap = false;
                } else if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
            }
            0x09 => self.tab_forward(1),
            0x0a | 0x0b | 0x0c => {
                self.line_feed();
                if self.modes.newline {
                    self.cursor.col = 0;
                }
            }
            0x0d => {
                self.cursor.col = 0;
                self.cursor.pending_wrap = false;
            }
            0x0e => self.cursor.shift = 1,
            0x0f => self.cursor.shift = 0,
            0x18 | 0x1a => self.state = State::Ground,
            _ => {}
        }
    }

    fn tab_forward(&mut self, n: usize) {
        for _ in 0..n {
            let mut col = self.cursor.col + 1;
            while col < self.cols - 1 && !self.tabs[col] {
                col += 1;
            }
            self.cursor.col = col.min(self.cols - 1);
        }
        self.cursor.pending_wrap = false;
    }

    fn tab_backward(&mut self, n: usize) {
        for _ in 0..n {
            let mut col = self.cursor.col;
            while col > 0 {
                col -= 1;
                if self.tabs[col] {
                    break;
                }
            }
            self.cursor.col = col;
        }
        self.cursor.pending_wrap = false;
    }

    fn escape(&mut self, b: u8) {
        self.state = State::Ground;
        match b {
            b'[' => {
                self.state = State::Csi;
                self.params = [0; MAX_PARAMS];
                self.colon = [false; MAX_PARAMS];
                self.param_count = 0;
                self.param_started = false;
                self.private = 0;
                self.intermediate = 0;
            }
            b']' => {
                self.state = State::Osc;
                self.osc.clear();
            }
            b'P' | b'X' | b'^' | b'_' => self.state = State::StringIgnore,
            0x20..=0x2f => {
                self.intermediate = b;
                self.state = State::EscapeIntermediate;
            }
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => self.line_feed(),
            b'E' => {
                self.cursor.col = 0;
                self.line_feed();
            }
            b'H' => {
                let col = self.cursor.col;
                self.tabs[col] = true;
            }
            b'M' => self.reverse_index(),
            b'c' => {
                self.reset();
                self.mark_all_dirty();
            }
            b'=' => self.modes.app_keypad = true,
            b'>' => self.modes.app_keypad = false,
            b'n' => self.cursor.shift = 2,
            b'o' => self.cursor.shift = 3,
            _ => {}
        }
    }

    fn escape_intermediate(&mut self, b: u8) {
        if (0x20..=0x2f).contains(&b) {
            return;
        }
        self.state = State::Ground;
        let set = match b {
            b'0' => Charset::Graphics,
            b'A' => Charset::Uk,
            _ => Charset::Ascii,
        };
        match self.intermediate {
            b'(' => self.cursor.charsets[0] = set,
            b')' | b'-' => self.cursor.charsets[1] = set,
            b'*' | b'.' => self.cursor.charsets[2] = set,
            b'+' | b'/' => self.cursor.charsets[3] = set,
            b'#' if b == b'8' => {
                let cell = Cell { ch: 'E', ..Cell::BLANK };
                self.grid.iter_mut().for_each(|c| *c = cell);
                self.mark_all_dirty();
            }
            _ => {}
        }
    }

    fn csi_byte(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                if self.param_count == 0 {
                    self.param_count = 1;
                }
                let i = self.param_count - 1;
                self.params[i] = self.params[i].saturating_mul(10).saturating_add((b - b'0') as u32).min(65535);
                self.param_started = true;
            }
            b';' | b':' => {
                if self.param_count == 0 {
                    self.param_count = 1;
                }
                if self.param_count < MAX_PARAMS {
                    self.colon[self.param_count] = b == b':';
                    self.param_count += 1;
                }
                self.param_started = true;
            }
            b'<' | b'=' | b'>' | b'?' => {
                if !self.param_started && self.private == 0 {
                    self.private = b;
                } else {
                    self.state = State::CsiIgnore;
                }
            }
            0x20..=0x2f => self.intermediate = b,
            0x40..=0x7e => {
                self.state = State::Ground;
                self.csi_dispatch(b);
            }
            _ => self.state = State::CsiIgnore,
        }
    }

    fn param(&self, index: usize, default: u32) -> u32 {
        if index < self.param_count && self.params[index] != 0 {
            self.params[index]
        } else {
            default
        }
    }

    fn param_raw(&self, index: usize) -> u32 {
        if index < self.param_count {
            self.params[index]
        } else {
            0
        }
    }

    fn csi_dispatch(&mut self, command: u8) {
        let n = self.param(0, 1) as usize;
        if self.private == b'?' {
            match command {
                b'h' | b'l' => self.private_modes(command == b'h'),
                b'n' if self.param_raw(0) == 6 => {
                    let (row, col) = self.report_position();
                    self.respond(&alloc::format!("\x1b[?{};{}R", row, col));
                }
                _ => {}
            }
            return;
        }
        if self.private == b'>' {
            if command == b'c' {
                self.respond("\x1b[>41;380;0c");
            }
            return;
        }
        if self.private != 0 {
            return;
        }
        if self.intermediate != 0 {
            if self.intermediate == b'!' && command == b'p' {
                self.soft_reset();
            }
            return;
        }
        match command {
            b'@' => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                let n = n.min(self.cols - col);
                let start = row * self.cols + col;
                let end = (row + 1) * self.cols;
                self.clear_wide_at(row, col);
                self.grid.copy_within(start..end - n, start + n);
                self.clear_cells(start, start + n);
                self.fix_wide_edge(row);
                self.cursor.pending_wrap = false;
            }
            b'A' => {
                let min = if self.cursor.row >= self.top { self.top } else { 0 };
                let row = self.cursor.row.saturating_sub(n).max(min);
                self.set_row(row);
                self.cursor.pending_wrap = false;
            }
            b'B' | b'e' => {
                let max = if self.cursor.row <= self.bottom { self.bottom } else { self.rows - 1 };
                let row = (self.cursor.row + n).min(max);
                self.set_row(row);
                self.cursor.pending_wrap = false;
            }
            b'C' | b'a' => {
                self.cursor.col = (self.cursor.col + n).min(self.cols - 1);
                self.cursor.pending_wrap = false;
            }
            b'D' => {
                self.cursor.col = self.cursor.col.saturating_sub(n);
                self.cursor.pending_wrap = false;
            }
            b'E' => {
                let max = if self.cursor.row <= self.bottom { self.bottom } else { self.rows - 1 };
                let row = (self.cursor.row + n).min(max);
                self.set_row(row);
                self.cursor.col = 0;
                self.cursor.pending_wrap = false;
            }
            b'F' => {
                let min = if self.cursor.row >= self.top { self.top } else { 0 };
                let row = self.cursor.row.saturating_sub(n).max(min);
                self.set_row(row);
                self.cursor.col = 0;
                self.cursor.pending_wrap = false;
            }
            b'G' | b'`' => {
                self.cursor.col = (n - 1).min(self.cols - 1);
                self.cursor.pending_wrap = false;
            }
            b'H' | b'f' => {
                let row = self.param(0, 1) as usize - 1;
                let col = self.param(1, 1) as usize - 1;
                self.move_to(row, col);
            }
            b'I' => self.tab_forward(n),
            b'J' => {
                let pos = self.cursor.row * self.cols + self.cursor.col;
                match self.param_raw(0) {
                    0 => {
                        self.clear_wide_at(self.cursor.row, self.cursor.col);
                        self.clear_cells(pos, self.grid.len());
                    }
                    1 => {
                        self.clear_wide_at(self.cursor.row, self.cursor.col);
                        self.clear_cells(0, pos + 1);
                    }
                    2 => self.clear_cells(0, self.grid.len()),
                    3 => self.clear_scrollback(),
                    _ => {}
                }
            }
            b'K' => {
                let row = self.cursor.row;
                let base = row * self.cols;
                let col = self.cursor.col;
                match self.param_raw(0) {
                    0 => {
                        self.clear_wide_at(row, col);
                        self.clear_cells(base + col, base + self.cols);
                    }
                    1 => {
                        self.clear_wide_at(row, col);
                        self.clear_cells(base, base + col + 1);
                    }
                    2 => self.clear_cells(base, base + self.cols),
                    _ => {}
                }
            }
            b'L' => {
                if self.cursor.row >= self.top && self.cursor.row <= self.bottom {
                    self.scroll_down_region(self.cursor.row, self.bottom, n);
                    self.cursor.col = 0;
                    self.cursor.pending_wrap = false;
                }
            }
            b'M' => {
                if self.cursor.row >= self.top && self.cursor.row <= self.bottom {
                    let row = self.cursor.row;
                    let n = n.min(self.bottom + 1 - row);
                    let cols = self.cols;
                    self.grid.copy_within((row + n) * cols..(self.bottom + 1) * cols, row * cols);
                    self.clear_cells((self.bottom + 1 - n) * cols, (self.bottom + 1) * cols);
                    for r in row..=self.bottom {
                        self.dirty[r] = true;
                    }
                    self.cursor.col = 0;
                    self.cursor.pending_wrap = false;
                }
            }
            b'P' => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                let n = n.min(self.cols - col);
                let start = row * self.cols + col;
                let end = (row + 1) * self.cols;
                self.clear_wide_at(row, col);
                self.grid.copy_within(start + n..end, start);
                self.clear_cells(end - n, end);
                self.fix_wide_edge(row);
                self.cursor.pending_wrap = false;
            }
            b'S' => self.scroll_up_region(self.top, self.bottom, n),
            b'T' => {
                if self.param_count <= 1 {
                    self.scroll_down_region(self.top, self.bottom, n);
                }
            }
            b'X' => {
                let row = self.cursor.row;
                let col = self.cursor.col;
                let base = row * self.cols;
                let n = n.min(self.cols - col);
                self.clear_wide_at(row, col);
                self.clear_wide_at(row, col + n - 1);
                self.clear_cells(base + col, base + col + n);
                self.cursor.pending_wrap = false;
            }
            b'Z' => self.tab_backward(n),
            b'b' => {
                let ch = self.last_char;
                for _ in 0..n.min(65535) {
                    self.print(ch);
                }
            }
            b'c' => {
                if self.param_raw(0) == 0 {
                    self.respond("\x1b[?62;22c");
                }
            }
            b'd' => {
                let row = n - 1;
                let col = self.cursor.col;
                self.move_to(row, col);
            }
            b'g' => match self.param_raw(0) {
                0 => {
                    let col = self.cursor.col;
                    self.tabs[col] = false;
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            b'h' | b'l' => {
                let on = command == b'h';
                for i in 0..self.param_count.max(1) {
                    match self.param_raw(i) {
                        4 => self.modes.insert = on,
                        20 => self.modes.newline = on,
                        _ => {}
                    }
                }
            }
            b'm' => self.sgr(),
            b'n' => match self.param_raw(0) {
                5 => self.respond("\x1b[0n"),
                6 => {
                    let (row, col) = self.report_position();
                    self.respond(&alloc::format!("\x1b[{};{}R", row, col));
                }
                _ => {}
            },
            b'r' => {
                let top = self.param(0, 1) as usize - 1;
                let bottom = (self.param(1, self.rows as u32) as usize).min(self.rows) - 1;
                if top < bottom {
                    self.top = top;
                    self.bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            b't' => match self.param_raw(0) {
                18 => {
                    let text = alloc::format!("\x1b[8;{};{}t", self.rows, self.cols);
                    self.respond(&text);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn report_position(&self) -> (usize, usize) {
        let row = if self.cursor.origin { self.cursor.row.saturating_sub(self.top) } else { self.cursor.row };
        (row + 1, self.cursor.col + 1)
    }

    fn respond(&mut self, text: &str) {
        self.responses.extend_from_slice(text.as_bytes());
    }

    fn soft_reset(&mut self) {
        self.modes.insert = false;
        self.modes.origin = false;
        self.modes.autowrap = true;
        self.modes.cursor_visible = true;
        self.modes.app_cursor = false;
        self.modes.app_keypad = false;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.cursor.pen = Cell::BLANK;
        self.cursor.origin = false;
        self.cursor.charsets = [Charset::Ascii; 4];
        self.cursor.shift = 0;
        self.saved_main = Cursor::new();
        self.saved_alt = Cursor::new();
    }

    fn save_cursor(&mut self) {
        if self.modes.alt_screen {
            self.saved_alt = self.cursor;
        } else {
            self.saved_main = self.cursor;
        }
    }

    fn restore_cursor(&mut self) {
        let saved = if self.modes.alt_screen { self.saved_alt } else { self.saved_main };
        self.dirty[self.cursor.row] = true;
        self.cursor = saved;
        self.cursor.row = self.cursor.row.min(self.rows - 1);
        self.cursor.col = self.cursor.col.min(self.cols - 1);
        self.modes.origin = self.cursor.origin;
        self.dirty[self.cursor.row] = true;
    }

    fn enter_alt_screen(&mut self, clear: bool) {
        if self.modes.alt_screen {
            if clear {
                self.clear_cells(0, self.grid.len());
            }
            return;
        }
        let blank = alloc::vec![Cell::BLANK; self.cols * self.rows];
        let main = core::mem::replace(&mut self.grid, blank);
        self.saved_screen = Some(main);
        self.modes.alt_screen = true;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.mark_all_dirty();
    }

    fn leave_alt_screen(&mut self) {
        if !self.modes.alt_screen {
            return;
        }
        if let Some(main) = self.saved_screen.take() {
            self.grid = main;
        }
        self.modes.alt_screen = false;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.mark_all_dirty();
    }

    fn private_modes(&mut self, on: bool) {
        for i in 0..self.param_count.max(1) {
            match self.param_raw(i) {
                1 => self.modes.app_cursor = on,
                5 => {
                    self.modes.reverse_video = on;
                    self.mark_all_dirty();
                }
                6 => {
                    self.modes.origin = on;
                    self.cursor.origin = on;
                    self.move_to(0, 0);
                }
                7 => self.modes.autowrap = on,
                9 => self.modes.mouse = if on { MouseMode::X10 } else { MouseMode::Off },
                12 => self.modes.cursor_blink = on,
                25 => {
                    self.modes.cursor_visible = on;
                    self.dirty[self.cursor.row] = true;
                }
                47 | 1047 => {
                    if on {
                        self.enter_alt_screen(self.param_raw(i) == 1047);
                    } else {
                        if self.param_raw(i) == 1047 && self.modes.alt_screen {
                            self.clear_cells(0, self.grid.len());
                        }
                        self.leave_alt_screen();
                    }
                }
                1048 => {
                    if on {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
                1049 => {
                    if on {
                        self.saved_main = self.cursor;
                        self.enter_alt_screen(true);
                        self.clear_cells(0, self.grid.len());
                    } else {
                        self.leave_alt_screen();
                        let saved = self.saved_main;
                        self.cursor = saved;
                        self.cursor.row = self.cursor.row.min(self.rows - 1);
                        self.cursor.col = self.cursor.col.min(self.cols - 1);
                    }
                }
                1000 => self.modes.mouse = if on { MouseMode::Normal } else { MouseMode::Off },
                1002 => self.modes.mouse = if on { MouseMode::Button } else { MouseMode::Off },
                1003 => self.modes.mouse = if on { MouseMode::Any } else { MouseMode::Off },
                1004 => self.modes.focus_events = on,
                1005 => self.modes.mouse_encoding = if on { MouseEncoding::Utf8 } else { MouseEncoding::Default },
                1006 => self.modes.mouse_encoding = if on { MouseEncoding::Sgr } else { MouseEncoding::Default },
                1007 => self.modes.alternate_scroll = on,
                1015 => self.modes.mouse_encoding = if on { MouseEncoding::Urxvt } else { MouseEncoding::Default },
                2004 => self.modes.bracketed_paste = on,
                _ => {}
            }
        }
    }

    fn sgr(&mut self) {
        let count = self.param_count.max(1);
        let mut i = 0;
        while i < count {
            let p = self.param_raw(i);
            let pen = &mut self.cursor.pen;
            match p {
                0 => {
                    pen.fg = Color::Default;
                    pen.bg = Color::Default;
                    pen.flags = 0;
                }
                1 => pen.flags |= BOLD,
                2 => pen.flags |= DIM,
                3 => pen.flags |= ITALIC,
                4 => {
                    if i + 1 < count && self.colon[i + 1] {
                        if self.params[i + 1] == 0 {
                            pen.flags &= !UNDERLINE;
                        } else {
                            pen.flags |= UNDERLINE;
                        }
                        i += 1;
                    } else {
                        pen.flags |= UNDERLINE;
                    }
                }
                5 | 6 => pen.flags |= BLINK,
                7 => pen.flags |= REVERSE,
                8 => pen.flags |= INVISIBLE,
                9 => pen.flags |= STRIKE,
                21 => pen.flags |= UNDERLINE,
                22 => pen.flags &= !(BOLD | DIM),
                23 => pen.flags &= !ITALIC,
                24 => pen.flags &= !UNDERLINE,
                25 => pen.flags &= !BLINK,
                27 => pen.flags &= !REVERSE,
                28 => pen.flags &= !INVISIBLE,
                29 => pen.flags &= !STRIKE,
                30..=37 => pen.fg = Color::Indexed((p - 30) as u8),
                39 => pen.fg = Color::Default,
                40..=47 => pen.bg = Color::Indexed((p - 40) as u8),
                49 => pen.bg = Color::Default,
                90..=97 => pen.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => pen.bg = Color::Indexed((p - 100 + 8) as u8),
                38 | 48 | 58 => {
                    let (color, used) = self.extended_color(i);
                    if let Some(color) = color {
                        let pen = &mut self.cursor.pen;
                        match p {
                            38 => pen.fg = color,
                            48 => pen.bg = color,
                            _ => {}
                        }
                    }
                    i += used;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn extended_color(&self, i: usize) -> (Option<Color>, usize) {
        let count = self.param_count;
        let colon = i + 1 < count && self.colon[i + 1];
        match self.param_raw(i + 1) {
            5 => (Some(Color::Indexed(self.param_raw(i + 2).min(255) as u8)), 2),
            2 => {
                let mut first = i + 2;
                if colon {
                    let mut end = i + 2;
                    while end < count && self.colon[end] {
                        end += 1;
                    }
                    if end - (i + 2) >= 4 {
                        first = i + 3;
                    }
                    let r = self.param_raw(first).min(255) as u8;
                    let g = self.param_raw(first + 1).min(255) as u8;
                    let b = self.param_raw(first + 2).min(255) as u8;
                    return (Some(Color::Rgb(r, g, b)), end - i - 1);
                }
                let r = self.param_raw(first).min(255) as u8;
                let g = self.param_raw(first + 1).min(255) as u8;
                let b = self.param_raw(first + 2).min(255) as u8;
                (Some(Color::Rgb(r, g, b)), 4)
            }
            _ => (None, if colon { 1 } else { 0 }),
        }
    }

    fn osc_dispatch(&mut self) {
        let text = String::from_utf8_lossy(&self.osc).into_owned();
        self.osc.clear();
        let (code, rest) = match text.split_once(';') {
            Some((c, r)) => (c, r),
            None => (text.as_str(), ""),
        };
        match code {
            "0" | "2" => self.title = Some(String::from(rest)),
            "10" | "11" if rest == "?" => {
                let value = if code == "10" { "rgb:d9d9/dddd/e5e5" } else { "rgb:1313/1515/1919" };
                let reply = alloc::format!("\x1b]{};{}\x1b\\", code, value);
                self.respond(&reply);
            }
            _ => {}
        }
    }
}

fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c > 0 && c % 8 == 0).collect()
}

pub fn palette_rgb(index: u8, base16: &[u32; 16]) -> u32 {
    match index {
        0..=15 => base16[index as usize],
        16..=231 => {
            let n = (index - 16) as u32;
            let level = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
            (level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)
        }
        _ => {
            let v = 8 + (index as u32 - 232) * 10;
            (v << 16) | (v << 8) | v
        }
    }
}
