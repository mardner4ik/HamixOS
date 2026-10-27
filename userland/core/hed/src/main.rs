#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::console;
use hamix_std::sys::{self, Key};
use hamix_std::{entry, env, fs};

const GUTTER: usize = 5;
const MIN_COLS: usize = 20;
const MIN_ROWS: usize = 4;

struct Editor {
    path: String,
    lines: Vec<String>,
    row: usize,
    col: usize,
    top: usize,
    left: usize,
    modified: bool,
    message: String,
    view_only: bool,
    quit_armed: bool,
    clipboard: Option<String>,
    cols: usize,
    rows: usize,
}

fn byte_index(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

impl Editor {
    fn new(path: &str, content: &str, view_only: bool) -> Self {
        let mut lines: Vec<String> = content.split('\n').map(|s| s.replace('\r', "")).collect();
        if lines.len() > 1 && lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            path: path.to_string(),
            lines,
            row: 0,
            col: 0,
            top: 0,
            left: 0,
            modified: false,
            message: if view_only {
                "view mode  |  arrows/PgUp/PgDn scroll  |  q or Ctrl+Q closes".to_string()
            } else {
                "Ctrl+S save  Ctrl+X save+quit  Ctrl+Q quit  Ctrl+K cut line  Ctrl+U paste".to_string()
            },
            view_only,
            quit_armed: false,
            clipboard: None,
            cols: 80,
            rows: 25,
        }
    }

    fn text_rows(&self) -> usize {
        self.rows.saturating_sub(2).max(1)
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(MIN_COLS);
        self.rows = rows.max(MIN_ROWS);
    }

    fn line_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    fn clamp(&mut self) {
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.col.min(self.line_len());
        if self.row < self.top {
            self.top = self.row;
        }
        let text_rows = self.text_rows();
        if self.row >= self.top + text_rows {
            self.top = self.row + 1 - text_rows;
        }
        let width = self.cols.saturating_sub(GUTTER).max(1);
        if self.col < self.left {
            self.left = self.col;
        }
        if self.col >= self.left + width {
            self.left = self.col + 1 - width;
        }
    }

    fn insert(&mut self, ch: char) {
        if self.view_only {
            return;
        }
        let at = byte_index(&self.lines[self.row], self.col);
        self.lines[self.row].insert(at, ch);
        self.col += 1;
        self.modified = true;
    }

    fn newline(&mut self) {
        if self.view_only {
            self.row += 1;
            return;
        }
        let at = byte_index(&self.lines[self.row], self.col);
        let rest = self.lines[self.row].split_off(at);
        let indent: String = self.lines[self.row].chars().take_while(|c| *c == ' ').collect();
        self.col = indent.len();
        self.lines.insert(self.row + 1, indent + &rest);
        self.row += 1;
        self.modified = true;
    }

    fn backspace(&mut self) {
        if self.view_only {
            return;
        }
        if self.col > 0 {
            let at = byte_index(&self.lines[self.row], self.col - 1);
            self.lines[self.row].remove(at);
            self.col -= 1;
            self.modified = true;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len();
            self.lines[self.row].push_str(&current);
            self.modified = true;
        }
    }

    fn delete(&mut self) {
        if self.view_only {
            return;
        }
        if self.col < self.line_len() {
            let at = byte_index(&self.lines[self.row], self.col);
            self.lines[self.row].remove(at);
            self.modified = true;
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
            self.modified = true;
        }
    }

    fn cut_line(&mut self) {
        if self.view_only {
            return;
        }
        let line = if self.lines.len() == 1 {
            core::mem::take(&mut self.lines[0])
        } else {
            self.lines.remove(self.row)
        };
        self.clipboard = Some(line);
        self.modified = true;
        self.message = "line cut -- Ctrl+U pastes it".to_string();
    }

    fn paste_line(&mut self) {
        if self.view_only {
            return;
        }
        if let Some(line) = self.clipboard.clone() {
            self.lines.insert(self.row, line);
            self.modified = true;
        }
    }

    fn save(&mut self) -> bool {
        if self.view_only {
            self.message = "view mode is read-only".to_string();
            return false;
        }
        let mut content = self.lines.join("\n");
        content.push('\n');
        if fs::write(&self.path, content.as_bytes()) {
            sys::sync();
            self.modified = false;
            self.message = format!("saved {} lines", self.lines.len());
            true
        } else {
            self.message = "save failed: permission denied?".to_string();
            false
        }
    }

    fn render(&self) -> String {
        let (cols, rows) = (self.cols, self.rows);
        let text_rows = self.text_rows();
        let mut out = String::with_capacity(cols * rows * 2);
        out.push_str("\x1b[H\x1b[0m\x1b[44m\x1b[97m");
        let title = format!(
            " hed  {}{}{}",
            self.path,
            if self.modified { "  [modified]" } else { "" },
            if self.view_only { "  [view]" } else { "" }
        );
        let position = format!("Ln {}/{}, Col {} ", self.row + 1, self.lines.len(), self.col + 1);
        let mut bar: String = title.chars().take(cols).collect();
        let used = bar.chars().count();
        let room = cols.saturating_sub(used);
        if position.len() < room {
            for _ in 0..room - position.len() {
                bar.push(' ');
            }
            bar.push_str(&position);
        } else {
            for _ in 0..room {
                bar.push(' ');
            }
        }
        out.push_str(&bar);
        out.push_str("\x1b[0m");

        let width = cols.saturating_sub(GUTTER).max(1);
        for screen_row in 0..text_rows {
            let index = self.top + screen_row;
            out.push_str(&format!("\x1b[{};1H", screen_row + 2));
            if index < self.lines.len() {
                out.push_str(&format!("\x1b[90m{:>4} \x1b[0m", index + 1));
                let visible: String = self.lines[index].chars().skip(self.left).take(width).collect();
                out.push_str(&visible);
            } else {
                out.push_str("\x1b[34m   ~\x1b[0m");
            }
            out.push_str("\x1b[K");
        }

        out.push_str(&format!("\x1b[{};1H\x1b[0m\x1b[7m", rows));
        let mut status: String = self.message.chars().take(cols - 1).collect();
        while status.chars().count() < cols - 1 {
            status.push(' ');
        }
        out.push_str(&status);
        out.push_str("\x1b[0m");
        out.push_str(&format!(
            "\x1b[{};{}H",
            self.row - self.top + 2,
            (self.col - self.left + GUTTER + 1).min(cols)
        ));
        out
    }
}

fn main() -> i32 {
    let args = env::args();
    let mut path = None;
    let mut view_only = false;
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--view" | "-v" => view_only = true,
            other => path = Some(other.to_string()),
        }
    }
    let Some(path) = path else {
        hamix_std::println!("usage: hed [--view] <file>");
        return 1;
    };

    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut editor = Editor::new(&path, &content, view_only);
    let mut window = console::Size::new();
    editor.resize(window.cols(), window.rows());
    let mut cleared = false;
    if !view_only && !path.starts_with("/run/") {
        sys::cmd_register("hedlast", "/usr/bin/hed", &[path.as_str()]);
    }

    loop {
        if window.changed() {
            editor.resize(window.cols(), window.rows());
            cleared = false;
        }
        if !cleared {
            cleared = true;
            sys::write(1, b"\x1b[2J");
        }
        editor.clamp();
        sys::write(1, editor.render().as_bytes());

        let key = sys::read_key();
        if key == Key::Resize {
            continue;
        }
        if key != Key::Char(17) {
            editor.quit_armed = false;
        }
        match key {
            Key::Char(19) => {
                editor.save();
            }
            Key::Char(17) => {
                if editor.modified && !editor.quit_armed {
                    editor.quit_armed = true;
                    editor.message = "unsaved changes -- press Ctrl+Q again to discard them".to_string();
                } else {
                    break;
                }
            }
            Key::Char(24) => {
                if editor.view_only || editor.save() {
                    break;
                }
            }
            Key::Char(11) => editor.cut_line(),
            Key::Char(21) => editor.paste_line(),
            Key::Char(b'q') if editor.view_only => break,
            Key::Escape if editor.view_only => break,
            Key::Char(c) if (0x20..0x7F).contains(&c) => editor.insert(c as char),
            Key::Tab => {
                for _ in 0..4 {
                    editor.insert(' ');
                }
            }
            Key::Enter => editor.newline(),
            Key::Backspace => editor.backspace(),
            Key::Delete => editor.delete(),
            Key::Up => editor.row = editor.row.saturating_sub(1),
            Key::Down => editor.row += 1,
            Key::Left => {
                if editor.col > 0 {
                    editor.col -= 1;
                } else if editor.row > 0 {
                    editor.row -= 1;
                    editor.col = usize::MAX;
                }
            }
            Key::Right => {
                if editor.col < editor.line_len() {
                    editor.col += 1;
                } else if editor.row + 1 < editor.lines.len() {
                    editor.row += 1;
                    editor.col = 0;
                }
            }
            Key::Home => editor.col = 0,
            Key::End => editor.col = usize::MAX,
            Key::PageUp => {
                let step = editor.text_rows();
                editor.row = editor.row.saturating_sub(step);
                editor.top = editor.top.saturating_sub(step);
            }
            Key::PageDown => {
                let step = editor.text_rows();
                editor.row += step;
                editor.top += step;
                editor.top = editor.top.min(editor.lines.len().saturating_sub(1));
            }
            _ => {}
        }
        if editor.view_only && editor.row > editor.lines.len() - 1 {
            editor.row = editor.lines.len() - 1;
        }
    }

    sys::write(1, b"\x1b[0m\x0C");
    0
}

entry!(main);
