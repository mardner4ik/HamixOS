#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::{entry, sys};
use hamix_std::sys::Key;

const COLS: usize = 80;
const ROWS: usize = 25;
const CONTENT_ROWS: usize = ROWS - 2;

struct Editor {
    path: String,
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    top: usize,
    modified: bool,
    message: String,
    read_only_view: bool,
}

impl Editor {
    fn new(path: &str, content: &str, read_only_view: bool) -> Self {
        let mut lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        // A trailing '\n' in the file produces one extra empty "line" from
        // split('\n') that isn't really a line the user typed; drop it so
        // saving doesn't grow the file by one blank line every time.
        if lines.len() > 1 && lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        Self {
            path: path.to_string(),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            top: 0,
            modified: false,
            message: if read_only_view {
                "view mode -- Ctrl+Q to close".to_string()
            } else {
                "Ctrl+S save   Ctrl+X save & quit   Ctrl+Q quit".to_string()
            },
            read_only_view,
        }
    }

    fn current_line_len(&self) -> usize {
        self.lines[self.cursor_row].chars().count()
    }

    fn clamp_col(&mut self) {
        let len = self.current_line_len();
        if self.cursor_col > len {
            self.cursor_col = len;
        }
    }

    fn scroll_to_cursor(&mut self) {
        if self.cursor_row < self.top {
            self.top = self.cursor_row;
        } else if self.cursor_row >= self.top + CONTENT_ROWS {
            self.top = self.cursor_row - CONTENT_ROWS + 1;
        }
    }

    fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col();
            self.scroll_to_cursor();
        }
    }

    fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.clamp_col();
            self.scroll_to_cursor();
        }
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_len();
            self.scroll_to_cursor();
        }
    }

    fn move_right(&mut self) {
        if self.cursor_col < self.current_line_len() {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
            self.scroll_to_cursor();
        }
    }

    fn insert_char(&mut self, ch: char) {
        if self.read_only_view {
            return;
        }
        let byte_idx = char_to_byte_idx(&self.lines[self.cursor_row], self.cursor_col);
        self.lines[self.cursor_row].insert(byte_idx, ch);
        self.cursor_col += 1;
        self.modified = true;
    }

    fn insert_newline(&mut self) {
        if self.read_only_view {
            return;
        }
        let byte_idx = char_to_byte_idx(&self.lines[self.cursor_row], self.cursor_col);
        let rest = self.lines[self.cursor_row].split_off(byte_idx);
        self.lines.insert(self.cursor_row + 1, rest);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.modified = true;
        self.scroll_to_cursor();
    }

    fn backspace(&mut self) {
        if self.read_only_view {
            return;
        }
        if self.cursor_col > 0 {
            let byte_idx = char_to_byte_idx(&self.lines[self.cursor_row], self.cursor_col - 1);
            self.lines[self.cursor_row].remove(byte_idx);
            self.cursor_col -= 1;
            self.modified = true;
        } else if self.cursor_row > 0 {
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_len();
            self.lines[self.cursor_row].push_str(&current);
            self.modified = true;
            self.scroll_to_cursor();
        }
    }

    fn delete(&mut self) {
        if self.read_only_view {
            return;
        }
        if self.cursor_col < self.current_line_len() {
            let byte_idx = char_to_byte_idx(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(byte_idx);
            self.modified = true;
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.modified = true;
        }
    }

    fn save(&mut self) {
        if self.read_only_view {
            self.message = "view mode is read-only".to_string();
            return;
        }
        let mut content = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(line);
        }
        sys::truncate(&self.path);
        let fd = sys::open(&self.path);
        if fd < 0 {
            self.message = "save failed: could not open file".to_string();
            return;
        }
        sys::write(fd as u64, content.as_bytes());
        sys::close(fd as u64);
        self.modified = false;
        self.message = "saved".to_string();
    }

    fn render(&self) -> String {
        let mut out = String::new();
        out.push('\x0C');

        for row in 0..CONTENT_ROWS {
            let line_idx = self.top + row;
            if line_idx < self.lines.len() {
                let line = &self.lines[line_idx];
                let display = if line_idx == self.cursor_row {
                    with_cursor_marker(line, self.cursor_col)
                } else {
                    line.clone()
                };
                push_clamped(&mut out, &display, COLS);
            } else {
                out.push('~');
            }
            out.push('\n');
        }

        let modified_marker = if self.modified { "[+]" } else { "" };
        let status = alloc::format!(
            "{} {}  Ln {}, Col {}",
            self.path,
            modified_marker,
            self.cursor_row + 1,
            self.cursor_col + 1
        );
        push_clamped(&mut out, &status, COLS);
        out.push('\n');

        // Last line: no trailing '\n', so we don't scroll the screen we
        // just carefully laid out.
        push_clamped(&mut out, &self.message, COLS);
        out
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(s.len())
}

fn with_cursor_marker(line: &str, col: usize) -> String {
    let byte_idx = char_to_byte_idx(line, col);
    let mut out = String::with_capacity(line.len() + 1);
    out.push_str(&line[..byte_idx]);
    out.push('|');
    out.push_str(&line[byte_idx..]);
    out
}

fn push_clamped(out: &mut String, s: &str, max_cols: usize) {
    for (i, ch) in s.chars().enumerate() {
        if i >= max_cols {
            break;
        }
        out.push(ch);
    }
}

fn read_whole_file(path: &str) -> Option<String> {
    let fd = sys::open(path);
    if fd < 0 {
        return None;
    }
    let size = sys::fstat_size(fd as u64).max(0) as usize;
    let mut buf = alloc::vec![0u8; size];
    let mut total = 0usize;
    while total < size {
        let n = sys::read(fd as u64, &mut buf[total..]);
        if n <= 0 {
            break;
        }
        total += n as usize;
    }
    sys::close(fd as u64);
    buf.truncate(total);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// HamixOS's ELF loader doesn't pass argc/argv to processes at all (see
/// task::elf) -- there's no mechanism for `exec` to hand a command-line
/// argument to what it starts. So `hed` takes its target file (and
/// view/edit mode) from a small handoff file that `hsh`'s `edit`/`help`
/// commands write just before exec'ing hed: line 1 is the path to open,
/// line 2 is either "edit" or "view".
const HANDOFF_PATH: &str = "/run/hed_target";

fn read_handoff() -> Option<(String, bool)> {
    let content = read_whole_file(HANDOFF_PATH)?;
    let mut lines = content.lines();
    let path = lines.next()?.trim().to_string();
    if path.is_empty() {
        return None;
    }
    let read_only_view = lines.next().map(|l| l.trim() == "view").unwrap_or(false);
    Some((path, read_only_view))
}

fn main() -> i32 {
    let (path, read_only_view) = match read_handoff() {
        Some(v) => v,
        None => {
            hamix_std::println!("hed: no file to open -- run this via hsh's `edit <path>` command");
            return 1;
        }
    };
    let content = read_whole_file(&path).unwrap_or_default();
    let mut editor = Editor::new(&path, &content, read_only_view);

    loop {
        let frame = editor.render();
        sys::write(1, frame.as_bytes());

        match sys::read_key() {
            Key::Char(19) => editor.save(),                 // Ctrl+S
            Key::Char(17) => break,                          // Ctrl+Q
            Key::Char(24) => {                                // Ctrl+X
                editor.save();
                break;
            }
            Key::Char(c) if c >= 0x20 && c < 0x7F => editor.insert_char(c as char),
            Key::Enter => editor.insert_newline(),
            Key::Backspace => editor.backspace(),
            Key::Delete => editor.delete(),
            Key::Up => editor.move_up(),
            Key::Down => editor.move_down(),
            Key::Left => editor.move_left(),
            Key::Right => editor.move_right(),
            Key::Home => editor.cursor_col = 0,
            Key::End => editor.cursor_col = editor.current_line_len(),
            _ => {}
        }
    }

    sys::write(1, b"\x0C");
    0
}

entry!(main);
