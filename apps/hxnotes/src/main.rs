#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, env, eprintln, fs, sys};
use hxclient::ui::{self, theme, FieldAction, Menu, MenuBar, MenuEntry, MenuResult, TextField, Ui};
use hxclient::{Event, Window, CURSOR_ARROW, CURSOR_TEXT, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_RELEASE, MOUSE_WHEEL};
use vellum::{Area, Painter};

const INITIAL_W: i32 = 780;
const INITIAL_H: i32 = 560;
const STATUS: i32 = 28;
const FIND_BAR: i32 = 44;
const UNDO_LIMIT: usize = 200;

const CMD_NEW: u32 = 1;
const CMD_OPEN: u32 = 2;
const CMD_SAVE: u32 = 3;
const CMD_SAVE_AS: u32 = 4;
const CMD_QUIT: u32 = 5;
const CMD_UNDO: u32 = 10;
const CMD_REDO: u32 = 11;
const CMD_CUT: u32 = 12;
const CMD_COPY: u32 = 13;
const CMD_PASTE: u32 = 14;
const CMD_DELETE: u32 = 15;
const CMD_SELECT_ALL: u32 = 16;
const CMD_FIND: u32 = 17;
const CMD_FIND_NEXT: u32 = 18;
const CMD_GO_TOP: u32 = 19;
const CMD_GO_BOTTOM: u32 = 20;
const CMD_LINE_NUMBERS: u32 = 30;
const CMD_STATUS_BAR: u32 = 31;
const CMD_HIGHLIGHT_LINE: u32 = 32;
const CMD_SHORTCUTS: u32 = 40;
const CMD_ABOUT: u32 = 41;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}

type Pos = (usize, usize);

#[derive(Clone, Copy, PartialEq)]
enum EditKind {
    Typing,
    Other,
}

struct Snapshot {
    lines: Vec<Vec<char>>,
    cursor: Pos,
}

struct App {
    window: Window,
    ui: Ui,
    menu: MenuBar,
    lines: Vec<Vec<char>>,
    cursor: Pos,
    anchor: Option<Pos>,
    top: usize,
    left: usize,
    path: Option<String>,
    modified: bool,
    clipboard: String,
    status: (String, u32),
    selecting: bool,
    char_w: i32,
    line_h: i32,
    focused: bool,
    blink: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: (EditKind, u64),
    find: Option<TextField>,
    line_numbers: bool,
    status_bar: bool,
    highlight_line: bool,
    hover_text: bool,
    shown_title: String,
}

fn menus(app_state: (bool, bool, bool, bool, bool, bool)) -> Vec<Menu> {
    let (can_undo, can_redo, has_selection, line_numbers, status_bar, highlight) = app_state;
    alloc::vec![
        Menu::new(
            "File",
            alloc::vec![
                MenuEntry::item("New", "Ctrl+N", CMD_NEW),
                MenuEntry::item("Open…", "Ctrl+O", CMD_OPEN),
                MenuEntry::separator(),
                MenuEntry::item("Save", "Ctrl+S", CMD_SAVE),
                MenuEntry::item("Save as…", "", CMD_SAVE_AS),
                MenuEntry::separator(),
                MenuEntry::item("Quit", "Ctrl+Q", CMD_QUIT),
            ]
        ),
        Menu::new(
            "Edit",
            alloc::vec![
                MenuEntry::item("Undo", "Ctrl+Z", CMD_UNDO).enabled(can_undo),
                MenuEntry::item("Redo", "Ctrl+Y", CMD_REDO).enabled(can_redo),
                MenuEntry::separator(),
                MenuEntry::item("Cut", "Ctrl+X", CMD_CUT).enabled(has_selection),
                MenuEntry::item("Copy", "Ctrl+C", CMD_COPY).enabled(has_selection),
                MenuEntry::item("Paste", "Ctrl+V", CMD_PASTE),
                MenuEntry::item("Delete", "Del", CMD_DELETE).enabled(has_selection),
                MenuEntry::separator(),
                MenuEntry::item("Select all", "Ctrl+A", CMD_SELECT_ALL),
            ]
        ),
        Menu::new(
            "Search",
            alloc::vec![
                MenuEntry::item("Find…", "Ctrl+F", CMD_FIND),
                MenuEntry::item("Find next", "Ctrl+G", CMD_FIND_NEXT),
                MenuEntry::separator(),
                MenuEntry::item("Go to the start", "Ctrl+Home", CMD_GO_TOP),
                MenuEntry::item("Go to the end", "Ctrl+End", CMD_GO_BOTTOM),
            ]
        ),
        Menu::new(
            "View",
            alloc::vec![
                MenuEntry::item("Line numbers", "", CMD_LINE_NUMBERS).checked(line_numbers),
                MenuEntry::item("Highlight the current line", "", CMD_HIGHLIGHT_LINE).checked(highlight),
                MenuEntry::item("Status bar", "", CMD_STATUS_BAR).checked(status_bar),
            ]
        ),
        Menu::new("Help", alloc::vec![MenuEntry::item("Keyboard shortcuts", "", CMD_SHORTCUTS), MenuEntry::item("About Notes", "", CMD_ABOUT)]),
    ]
}

impl App {
    fn refresh_menus(&mut self) {
        let state = (!self.undo.is_empty(), !self.redo.is_empty(), self.selection().is_some(), self.line_numbers, self.status_bar, self.highlight_line);
        self.menu.set_menus(menus(state));
    }

    fn bottom(&self) -> i32 {
        (if self.status_bar { STATUS } else { 0 }) + if self.find.is_some() { FIND_BAR } else { 0 }
    }

    fn text_area(&self) -> Area {
        Area::new(0, MenuBar::HEIGHT, ww(), hh() - MenuBar::HEIGHT - self.bottom())
    }

    fn gutter(&self) -> i32 {
        if !self.line_numbers {
            return 4;
        }
        let digits = format!("{}", self.lines.len()).len().max(3) as i32;
        digits * self.char_w + 24
    }

    fn visible(&self) -> (usize, usize) {
        let area = self.text_area();
        (((area.h - 12) / self.line_h).max(1) as usize, ((area.w - self.gutter() - 20) / self.char_w).max(1) as usize)
    }

    fn text(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            out.extend(line.iter());
            if i + 1 < self.lines.len() {
                out.push('\n');
            }
        }
        out
    }

    fn set_text(&mut self, text: &str) {
        self.lines = text.split('\n').map(|l| l.trim_end_matches('\r').chars().collect()).collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.cursor = (0, 0);
        self.anchor = None;
        self.top = 0;
        self.left = 0;
        self.modified = false;
        self.undo.clear();
        self.redo.clear();
    }

    fn file_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.rsplit('/').next()).filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| String::from("Untitled"))
    }

    fn update_title(&mut self) {
        let name = self.file_name();
        let title = format!("{}{} — Notes", if self.modified { "• " } else { "" }, name);
        if title != self.shown_title {
            self.window.set_title(&title);
            self.shown_title = title;
        }
    }

    fn open_path(&mut self, path: &str) {
        match fs::read(path) {
            Some(bytes) => {
                if bytes.iter().take(4096).any(|b| *b == 0) {
                    self.status = (format!("{} looks like a binary file", path), theme::danger());
                    return;
                }
                let text = String::from_utf8_lossy(&bytes).into_owned();
                self.set_text(&text);
                self.path = Some(String::from(path));
                self.status = (format!("Opened {}", path), theme::dim());
            }
            None => {
                if sys::stat(path).is_err() {
                    self.set_text("");
                    self.path = Some(String::from(path));
                    self.status = (String::from("New file — it will be created when you save"), theme::dim());
                } else {
                    self.status = (format!("Cannot open {}", path), theme::danger());
                }
            }
        }
        self.update_title();
    }

    fn dialog_start(&self) -> String {
        match self.path.as_deref().and_then(|p| p.rfind('/').map(|i| (p, i))) {
            Some((p, i)) if i > 0 => String::from(&p[..i]),
            _ => format!("{}/Documents", ui::home_dir()),
        }
    }

    fn open_dialog(&mut self) {
        let start = self.dialog_start();
        if let Some(path) = self.window.open_file_dialog("Open a text file", "Text files|txt,md,conf,log,rs,sh,json,csv,ini,toml,c,h,py;All files|*", &start) {
            self.open_path(&path);
        }
    }

    fn save_dialog(&mut self) {
        let suggested = self.path.clone().unwrap_or_else(|| format!("{}/Documents/note.txt", ui::home_dir()));
        if let Some(path) = self.window.save_file_dialog("Save the note as", "Text files|txt,md;All files|*", &suggested) {
            self.path = Some(path);
            self.save();
        }
    }

    fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            self.save_dialog();
            return;
        };
        if let Some(i) = path.rfind('/') {
            if i > 0 {
                let dir = &path[..i];
                if sys::stat(dir).is_err() {
                    sys::mkdir(dir);
                }
            }
        }
        let mut text = self.text();
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        if fs::write(&path, text.as_bytes()) {
            self.modified = false;
            self.status = (format!("Saved {} ({})", path, ui::human_size(text.len() as u64)), theme::success());
            sys::sync();
        } else {
            self.status = (format!("Cannot save {} — permission denied?", path), theme::danger());
        }
        self.update_title();
    }

    fn new_document(&mut self) {
        self.set_text("");
        self.path = None;
        self.status = (String::from("New note"), theme::dim());
        self.update_title();
    }

    fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some(if anchor < self.cursor { (anchor, self.cursor) } else { (self.cursor, anchor) })
    }

    fn selected_text(&self) -> String {
        let Some((s, e)) = self.selection() else {
            return String::new();
        };
        let mut out = String::new();
        for row in s.0..=e.0 {
            let from = if row == s.0 { s.1 } else { 0 };
            let to = if row == e.0 { e.1 } else { self.lines[row].len() };
            out.extend(self.lines[row][from.min(self.lines[row].len())..to.min(self.lines[row].len())].iter());
            if row != e.0 {
                out.push('\n');
            }
        }
        out
    }

    fn remember(&mut self, kind: EditKind) {
        let now = sys::uptime_ms();
        let merge = kind == EditKind::Typing && self.last_edit.0 == EditKind::Typing && now.saturating_sub(self.last_edit.1) < 1500 && !self.undo.is_empty();
        self.last_edit = (kind, now);
        if merge {
            return;
        }
        self.undo.push(Snapshot { lines: self.lines.clone(), cursor: self.cursor });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, from_undo: bool) {
        let source = if from_undo { self.undo.pop() } else { self.redo.pop() };
        let Some(snapshot) = source else {
            self.status = (String::from(if from_undo { "Nothing to undo" } else { "Nothing to redo" }), theme::faint());
            return;
        };
        let current = Snapshot { lines: core::mem::replace(&mut self.lines, snapshot.lines), cursor: self.cursor };
        if from_undo {
            self.redo.push(current);
        } else {
            self.undo.push(current);
        }
        self.cursor = snapshot.cursor;
        self.cursor.0 = self.cursor.0.min(self.lines.len() - 1);
        self.cursor.1 = self.cursor.1.min(self.lines[self.cursor.0].len());
        self.anchor = None;
        self.modified = true;
        self.last_edit = (EditKind::Other, 0);
    }

    fn delete_selection(&mut self) -> bool {
        let Some((s, e)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let tail: Vec<char> = self.lines[e.0][e.1.min(self.lines[e.0].len())..].to_vec();
        self.lines[s.0].truncate(s.1);
        self.lines[s.0].extend(tail);
        self.lines.drain(s.0 + 1..=e.0);
        self.cursor = s;
        self.anchor = None;
        self.modified = true;
        true
    }

    fn insert(&mut self, text: &str) {
        self.delete_selection();
        for ch in text.chars() {
            if ch == '\n' {
                let rest = self.lines[self.cursor.0].split_off(self.cursor.1);
                self.lines.insert(self.cursor.0 + 1, rest);
                self.cursor = (self.cursor.0 + 1, 0);
            } else if ch == '\t' {
                for _ in 0..4 {
                    self.lines[self.cursor.0].insert(self.cursor.1, ' ');
                    self.cursor.1 += 1;
                }
            } else if ch != '\r' {
                self.lines[self.cursor.0].insert(self.cursor.1, ch);
                self.cursor.1 += 1;
            }
        }
        self.modified = true;
    }

    fn copy(&mut self, cut: bool) {
        let text = self.selected_text();
        if text.is_empty() {
            return;
        }
        self.clipboard = text;
        if cut {
            self.remember(EditKind::Other);
            self.delete_selection();
        }
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.remember(EditKind::Other);
        let clip = self.clipboard.clone();
        self.insert(&clip);
    }

    fn find_next(&mut self) {
        let Some(query) = self.find.as_ref().map(|f| f.text.clone()).filter(|q| !q.is_empty()) else {
            self.open_find();
            return;
        };
        let needle: Vec<char> = query.to_lowercase().chars().collect();
        let rows = self.lines.len();
        let start = self.cursor;
        for step in 0..=rows {
            let row = (start.0 + step) % rows;
            let line: Vec<char> = self.lines[row].iter().flat_map(|c| c.to_lowercase()).collect();
            let from = if step == 0 { start.1 } else { 0 };
            if line.len() < needle.len() {
                continue;
            }
            let found = (from..=line.len() - needle.len()).find(|&i| line[i..i + needle.len()] == needle[..]);
            if let Some(col) = found {
                if step == 0 && self.selection().map(|(s, _)| s == (row, col)).unwrap_or(false) {
                    let next = (col + 1..=line.len() - needle.len()).find(|&i| line[i..i + needle.len()] == needle[..]);
                    if let Some(col) = next {
                        self.anchor = Some((row, col));
                        self.cursor = (row, col + needle.len());
                        self.status = (format!("Found “{}” on line {}", query, row + 1), theme::dim());
                        self.ensure_visible();
                        return;
                    }
                    continue;
                }
                self.anchor = Some((row, col));
                self.cursor = (row, col + needle.len());
                self.status = (format!("Found “{}” on line {}", query, row + 1), theme::dim());
                self.ensure_visible();
                return;
            }
        }
        self.status = (format!("“{}” was not found", query), theme::warn());
    }

    fn open_find(&mut self) {
        let seed = self.selected_text();
        let mut field = self.find.take().unwrap_or_default();
        if !seed.is_empty() && !seed.contains('\n') {
            field.set(&seed);
        }
        self.find = Some(field);
    }

    fn command(&mut self, command: u32) -> bool {
        match command {
            CMD_NEW => self.new_document(),
            CMD_OPEN => self.open_dialog(),
            CMD_SAVE => self.save(),
            CMD_SAVE_AS => self.save_dialog(),
            CMD_QUIT => return false,
            CMD_UNDO => self.restore(true),
            CMD_REDO => self.restore(false),
            CMD_CUT => self.copy(true),
            CMD_COPY => self.copy(false),
            CMD_PASTE => self.paste(),
            CMD_DELETE => {
                if self.selection().is_some() {
                    self.remember(EditKind::Other);
                    self.delete_selection();
                }
            }
            CMD_SELECT_ALL => {
                self.anchor = Some((0, 0));
                let last = self.lines.len() - 1;
                self.cursor = (last, self.lines[last].len());
            }
            CMD_FIND => self.open_find(),
            CMD_FIND_NEXT => self.find_next(),
            CMD_GO_TOP => {
                self.anchor = None;
                self.cursor = (0, 0);
            }
            CMD_GO_BOTTOM => {
                self.anchor = None;
                let last = self.lines.len() - 1;
                self.cursor = (last, self.lines[last].len());
            }
            CMD_LINE_NUMBERS => self.line_numbers = !self.line_numbers,
            CMD_STATUS_BAR => self.status_bar = !self.status_bar,
            CMD_HIGHLIGHT_LINE => self.highlight_line = !self.highlight_line,
            CMD_SHORTCUTS => self.status = (String::from("Ctrl+N new · Ctrl+O open · Ctrl+S save · Ctrl+Z/Y undo/redo · Ctrl+F find · Ctrl+G next"), theme::dim()),
            CMD_ABOUT => self.status = (String::from("Notes — a plain text editor for HamixOS"), theme::dim()),
            _ => {}
        }
        self.ensure_visible();
        self.update_title();
        self.refresh_menus();
        true
    }

    fn key(&mut self, code: i32) -> bool {
        let row_len = |app: &App, r: usize| app.lines[r].len();
        match code {
            19 => return self.command(CMD_SAVE),
            15 => return self.command(CMD_OPEN),
            14 => return self.command(CMD_NEW),
            17 => return self.command(CMD_QUIT),
            26 => return self.command(CMD_UNDO),
            25 => return self.command(CMD_REDO),
            6 => return self.command(CMD_FIND),
            7 => return self.command(CMD_FIND_NEXT),
            1 => return self.command(CMD_SELECT_ALL),
            3 => return self.command(CMD_COPY),
            24 => return self.command(CMD_CUT),
            22 => return self.command(CMD_PASTE),
            27 => {
                self.anchor = None;
            }
            10 => {
                self.remember(EditKind::Other);
                let indent: String = self.lines[self.cursor.0].iter().take_while(|c| **c == ' ').collect();
                self.insert("\n");
                self.insert(&indent);
            }
            9 => {
                self.remember(EditKind::Typing);
                self.insert("\t");
            }
            8 => {
                if self.selection().is_some() {
                    self.remember(EditKind::Other);
                    self.delete_selection();
                } else if self.cursor.1 > 0 {
                    self.remember(EditKind::Typing);
                    self.anchor = None;
                    self.cursor.1 -= 1;
                    self.lines[self.cursor.0].remove(self.cursor.1);
                    self.modified = true;
                } else if self.cursor.0 > 0 {
                    self.remember(EditKind::Other);
                    self.anchor = None;
                    let line = self.lines.remove(self.cursor.0);
                    self.cursor.0 -= 1;
                    self.cursor.1 = self.lines[self.cursor.0].len();
                    self.lines[self.cursor.0].extend(line);
                    self.modified = true;
                }
            }
            -7 => {
                if self.selection().is_some() {
                    self.remember(EditKind::Other);
                    self.delete_selection();
                } else if self.cursor.1 < row_len(self, self.cursor.0) {
                    self.remember(EditKind::Typing);
                    self.anchor = None;
                    self.lines[self.cursor.0].remove(self.cursor.1);
                    self.modified = true;
                } else if self.cursor.0 + 1 < self.lines.len() {
                    self.remember(EditKind::Other);
                    self.anchor = None;
                    let next = self.lines.remove(self.cursor.0 + 1);
                    self.lines[self.cursor.0].extend(next);
                    self.modified = true;
                }
            }
            -9..=-1 => {
                self.anchor = None;
                let (rows, _) = self.visible();
                match code {
                    -1 => self.cursor.0 = self.cursor.0.saturating_sub(1),
                    -2 => self.cursor.0 = (self.cursor.0 + 1).min(self.lines.len() - 1),
                    -3 => {
                        if self.cursor.1 > 0 {
                            self.cursor.1 -= 1;
                        } else if self.cursor.0 > 0 {
                            self.cursor.0 -= 1;
                            self.cursor.1 = row_len(self, self.cursor.0);
                        }
                    }
                    -4 => {
                        if self.cursor.1 < row_len(self, self.cursor.0) {
                            self.cursor.1 += 1;
                        } else if self.cursor.0 + 1 < self.lines.len() {
                            self.cursor = (self.cursor.0 + 1, 0);
                        }
                    }
                    -5 => self.cursor.1 = 0,
                    -6 => self.cursor.1 = row_len(self, self.cursor.0),
                    -8 => self.cursor.0 = self.cursor.0.saturating_sub(rows),
                    _ => self.cursor.0 = (self.cursor.0 + rows).min(self.lines.len() - 1),
                }
                self.cursor.1 = self.cursor.1.min(row_len(self, self.cursor.0));
                self.last_edit = (EditKind::Other, 0);
            }
            c if c >= 32 && c != 127 => {
                if let Some(ch) = char::from_u32(c as u32) {
                    self.remember(if ch == ' ' { EditKind::Other } else { EditKind::Typing });
                    let mut buf = [0u8; 4];
                    self.insert(ch.encode_utf8(&mut buf));
                }
            }
            _ => {}
        }
        self.ensure_visible();
        self.update_title();
        self.refresh_menus();
        true
    }

    fn ensure_visible(&mut self) {
        let (rows, cols) = self.visible();
        if self.cursor.0 < self.top {
            self.top = self.cursor.0;
        }
        if self.cursor.0 >= self.top + rows {
            self.top = self.cursor.0 + 1 - rows;
        }
        if self.cursor.1 < self.left {
            self.left = self.cursor.1;
        }
        if self.cursor.1 >= self.left + cols {
            self.left = self.cursor.1 + 1 - cols;
        }
    }

    fn position_at(&self, x: i32, y: i32) -> Pos {
        let area = self.text_area();
        let row = (self.top as i32 + (y - area.y - 6).max(0) / self.line_h).clamp(0, self.lines.len() as i32 - 1) as usize;
        let col = (self.left as i32 + (x - area.x - self.gutter() - 8 + self.char_w / 2).max(0) / self.char_w) as usize;
        (row, col.min(self.lines[row].len()))
    }

    fn draw(&mut self) {
        let area = self.text_area();
        let gutter = self.gutter();
        let (rows, cols) = self.visible();
        let selection = self.selection();
        let (cw, lh) = (self.char_w, self.line_h);
        let buffer = self.window.buffer();
        let mut p = Painter::new(buffer, ww(), hh());
        let ui = &self.ui;
        p.fill(Area::new(0, 0, ww(), hh()), theme::bg());
        self.menu.draw(&mut p, ui, ww());

        if self.line_numbers {
            p.fill(Area::new(0, area.y, gutter, area.h), theme::gutter());
            p.fill(Area::new(gutter, area.y, 1, area.h), theme::border());
        }
        let saved = p.clip;
        p.set_clip(area);
        for i in 0..=rows {
            let row = self.top + i;
            if row >= self.lines.len() {
                break;
            }
            let y = area.y + 6 + i as i32 * lh;
            let current = row == self.cursor.0;
            if current && selection.is_none() && self.highlight_line {
                p.fill(Area::new(gutter + 1, y, area.w - gutter, lh), theme::current_line());
            }
            if self.line_numbers {
                let number = format!("{}", row + 1);
                let nw = ui.mono.measure(&number);
                p.text(&ui.mono, gutter - 12 - nw, y + 1, &number, if current { theme::dim() } else { theme::faint() });
            }
            let line = &self.lines[row];
            let text_x = gutter + 8;
            if let Some((s, e)) = selection {
                if row >= s.0 && row <= e.0 {
                    let from = if row == s.0 { s.1 } else { 0 };
                    let to = if row == e.0 { e.1 } else { line.len() + 1 };
                    let from = from.max(self.left);
                    if to > from {
                        p.fill(Area::new(text_x + (from - self.left) as i32 * cw, y, (to - from).min(cols + 1) as i32 * cw, lh), theme::selection());
                    }
                }
            }
            let mut x = text_x;
            for ch in line.iter().skip(self.left).take(cols + 1) {
                if *ch != ' ' {
                    let mut buf = [0u8; 4];
                    p.text(&ui.mono, x, y + 1, ch.encode_utf8(&mut buf), theme::text());
                }
                x += cw;
            }
        }
        if self.focused && self.find.is_none() && self.blink && !self.menu.is_open() {
            let (r, c) = self.cursor;
            if r >= self.top && r < self.top + rows + 1 && c >= self.left {
                let x = gutter + 8 + (c - self.left) as i32 * cw;
                let y = area.y + 6 + (r - self.top) as i32 * lh;
                p.fill(Area::new(x, y, 2, lh), theme::accent_hover());
            }
        }
        ui::scrollbar(&mut p, Area::new(ww() - 8, area.y + 4, 4, area.h - 8), self.top as i32 * lh, (self.lines.len() + rows) as i32 * lh, rows as i32 * lh);
        p.clip = saved;

        if let Some(field) = self.find.as_mut() {
            let bar = Area::new(0, hh() - (if self.status_bar { STATUS } else { 0 }) - FIND_BAR, ww(), FIND_BAR);
            p.fill(bar, theme::surface());
            p.fill(Area::new(0, bar.y, ww(), 1), theme::border());
            let label = "Find";
            let lw = p.text(&ui.medium, 14, bar.y + (FIND_BAR - ui.medium.height()) / 2, label, theme::dim());
            let input = Area::new(14 + lw + 12, bar.y + 7, (ww() - lw - 60).min(420), FIND_BAR - 14);
            ui::text_field(&mut p, ui, input, field, true, "Text to look for — Enter finds the next match, Esc closes");
        }

        if self.status_bar {
            let status = Area::new(0, hh() - STATUS, ww(), STATUS);
            p.fill(status, theme::surface());
            p.fill(Area::new(0, status.y, ww(), 1), theme::border());
            let where_text = self.path.clone().unwrap_or_else(|| String::from("not saved yet"));
            let info = format!("Ln {}, Col {}   ·   {} lines   ·   UTF-8", self.cursor.0 + 1, self.cursor.1 + 1, self.lines.len());
            let iw = ui.small.measure(&info);
            p.text(&ui.small, ww() - iw - 14, status.y + (STATUS - ui.small.height()) / 2, &info, theme::dim());
            let message = if self.status.0.is_empty() { (where_text, theme::faint()) } else { self.status.clone() };
            ui::text_in(&mut p, &ui.small, 14, Area::new(0, status.y, ww() - iw - 40, STATUS), &message.0, message.1);
        }
        self.menu.draw_dropdown(&mut p, ui, ww(), hh());
        self.window.present();
    }
}

fn main() -> i32 {
    let ui = Ui::load();
    let char_w = ui.mono.advance('M');
    let line_h = ui.mono.height() + 3;
    let window = match Window::open("Notes", INITIAL_W as u32, INITIAL_H as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxnotes: {}", e);
            return 1;
        }
    };
    let mut app = App {
        window,
        ui,
        menu: MenuBar::new(Vec::new()),
        lines: alloc::vec![Vec::new()],
        cursor: (0, 0),
        anchor: None,
        top: 0,
        left: 0,
        path: None,
        modified: false,
        clipboard: String::new(),
        status: (String::from("Ctrl+S save · Ctrl+O open · Ctrl+F find · Ctrl+Z undo"), theme::faint()),
        selecting: false,
        char_w,
        line_h,
        focused: true,
        blink: true,
        undo: Vec::new(),
        redo: Vec::new(),
        last_edit: (EditKind::Other, 0),
        find: None,
        line_numbers: true,
        status_bar: true,
        highlight_line: true,
        hover_text: false,
        shown_title: String::new(),
    };
    app.refresh_menus();
    app.window.set_icon("notes");
    match env::args().get(1) {
        Some(path) => {
            let absolute = if path.starts_with('/') { path.clone() } else { format!("{}/{}", sys::getcwd().trim_end_matches('/'), path) };
            app.open_path(&absolute);
        }
        None => app.update_title(),
    }
    app.window.set_min_size(420, 260);
    app.draw();
    let mut last_blink = sys::uptime_ms();
    loop {
        let event = app.window.wait_event(500);
        let mut redraw = false;
        match event {
            Some(Event::Close { .. }) => return 0,
            Some(Event::Resize { .. }) | Some(Event::Theme { .. }) => redraw = true,
            Some(Event::Focus { focused, .. }) => {
                app.focused = focused;
                if !focused {
                    app.menu.close();
                }
                redraw = true;
            }
            Some(Event::Key { code, .. }) => {
                redraw = true;
                app.blink = true;
                last_blink = sys::uptime_ms();
                match app.menu.key(code) {
                    MenuResult::Command(c) => {
                        if !app.command(c) {
                            return 0;
                        }
                    }
                    MenuResult::Consumed => {}
                    MenuResult::Ignored => {
                        if let Some(field) = app.find.as_mut() {
                            match code {
                                27 => app.find = None,
                                7 => app.find_next(),
                                _ => {
                                    if let FieldAction::Submit = field.key(code) {
                                        app.find_next();
                                    }
                                }
                            }
                        } else if !app.key(code) {
                            return 0;
                        }
                    }
                }
            }
            Some(Event::Mouse { x, y, kind, wheel, buttons, .. }) => match kind {
                MOUSE_MOVE | MOUSE_LEAVE => {
                    if kind == MOUSE_LEAVE {
                        redraw |= app.menu.leave();
                    } else if app.selecting && buttons & 1 != 0 {
                        app.cursor = app.position_at(x, y);
                        app.ensure_visible();
                        redraw = true;
                    } else {
                        redraw |= app.menu.motion(x, y);
                        let over_text = kind == MOUSE_MOVE && !app.menu.contains(x, y) && app.text_area().contains(x, y);
                        if over_text != app.hover_text {
                            app.hover_text = over_text;
                        }
                        app.window.set_cursor(if over_text { CURSOR_TEXT } else { CURSOR_ARROW });
                    }
                }
                MOUSE_WHEEL => {
                    if !app.menu.is_open() {
                        let max = app.lines.len().saturating_sub(1);
                        app.top = ((app.top as i32 + wheel * 3).max(0) as usize).min(max);
                        redraw = true;
                    }
                }
                MOUSE_PRESS => {
                    redraw = true;
                    match app.menu.press(x, y) {
                        MenuResult::Command(c) => {
                            if !app.command(c) {
                                return 0;
                            }
                        }
                        MenuResult::Consumed => {}
                        MenuResult::Ignored => {
                            if app.text_area().contains(x, y) {
                                if wheel == 1 {
                                    let pos = app.position_at(x, y);
                                    app.cursor = pos;
                                    app.anchor = Some(pos);
                                    app.selecting = true;
                                    app.last_edit = (EditKind::Other, 0);
                                } else if wheel == 4 {
                                    app.paste();
                                    app.update_title();
                                }
                                app.refresh_menus();
                            }
                        }
                    }
                }
                MOUSE_RELEASE => {
                    if app.selecting {
                        app.selecting = false;
                        app.refresh_menus();
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if sys::uptime_ms().saturating_sub(last_blink) >= 530 {
            last_blink = sys::uptime_ms();
            app.blink = !app.blink;
            redraw = true;
        }
        if redraw {
            app.draw();
        }
    }
}

entry!(main);
