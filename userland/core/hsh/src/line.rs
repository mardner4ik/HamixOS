use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::sys::{self, Key};

use crate::shell::write;

pub enum ReadResult {
    Line(String),
    Interrupted,
    Eof,
}

fn visible_len(text: &str) -> usize {
    let mut len = 0;
    let mut escape = false;
    for c in text.chars() {
        if escape {
            if c.is_ascii_alphabetic() {
                escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            escape = true;
            continue;
        }
        len += 1;
    }
    len
}

struct Editor<'a> {
    prompt: &'a str,
    prompt_len: usize,
    buffer: Vec<char>,
    cursor: usize,
    cols: usize,
    drawn_rows: usize,
    cursor_row: usize,
}

impl Editor<'_> {
    fn render(&mut self) {
        let mut out = String::new();
        if self.cursor_row > 0 {
            out.push_str(&format!("\x1b[{}A", self.cursor_row));
        }
        out.push('\r');
        out.push_str(self.prompt);
        let text: String = self.buffer.iter().collect();
        out.push_str(&text);
        let total = self.prompt_len + self.buffer.len();
        if total > 0 && total % self.cols == 0 {
            out.push_str(" \r");
        }
        out.push_str("\x1b[K");
        let end_row = total / self.cols;
        if self.drawn_rows > end_row {
            for _ in end_row..self.drawn_rows {
                out.push_str("\x1b[B\r\x1b[K");
            }
            out.push_str(&format!("\x1b[{}A", self.drawn_rows - end_row));
        }
        let pos = self.prompt_len + self.cursor;
        let (row, col) = (pos / self.cols, pos % self.cols);
        if end_row > row {
            out.push_str(&format!("\x1b[{}A", end_row - row));
        }
        out.push('\r');
        if col > 0 {
            out.push_str(&format!("\x1b[{}C", col));
        }
        self.drawn_rows = end_row.max(row);
        self.cursor_row = row;
        write(1, &out);
    }

    fn finish(&mut self) {
        let total = self.prompt_len + self.buffer.len();
        let end_row = total / self.cols;
        if end_row > self.cursor_row {
            write(1, &format!("\x1b[{}B", end_row - self.cursor_row));
        }
        write(1, "\r\n");
    }

    fn set(&mut self, text: &str) {
        self.buffer = text.chars().collect();
        self.cursor = self.buffer.len();
    }

    fn text(&self) -> String {
        self.buffer.iter().collect()
    }
}

pub fn read_line(prompt: &str, history: &[String], complete: &dyn Fn(&str) -> Vec<String>) -> ReadResult {
    let cols = sys::termsize(1).map(|(c, _)| c as usize).filter(|c| *c >= 20).unwrap_or(80);
    let mut ed = Editor { prompt, prompt_len: visible_len(prompt), buffer: Vec::new(), cursor: 0, cols, drawn_rows: 0, cursor_row: 0 };
    let mut hist_index = history.len();
    let mut draft = String::new();
    let mut last_tab = false;
    write(1, prompt);
    loop {
        let key = sys::read_key();
        let was_tab = last_tab;
        last_tab = false;
        match key {
            Key::Enter => {
                ed.cursor = ed.buffer.len();
                ed.render();
                ed.finish();
                return ReadResult::Line(ed.text());
            }
            Key::Char(3) => {
                ed.cursor = ed.buffer.len();
                ed.render();
                write(1, "^C");
                ed.finish();
                return ReadResult::Interrupted;
            }
            Key::Char(4) => {
                if ed.buffer.is_empty() {
                    write(1, "\r\n");
                    return ReadResult::Eof;
                }
                if ed.cursor < ed.buffer.len() {
                    ed.buffer.remove(ed.cursor);
                    ed.render();
                }
            }
            Key::Backspace => {
                if ed.cursor > 0 {
                    ed.cursor -= 1;
                    ed.buffer.remove(ed.cursor);
                    ed.render();
                }
            }
            Key::Delete => {
                if ed.cursor < ed.buffer.len() {
                    ed.buffer.remove(ed.cursor);
                    ed.render();
                }
            }
            Key::Left | Key::Char(2) => {
                if ed.cursor > 0 {
                    ed.cursor -= 1;
                    ed.render();
                }
            }
            Key::Right | Key::Char(6) => {
                if ed.cursor < ed.buffer.len() {
                    ed.cursor += 1;
                    ed.render();
                }
            }
            Key::Home | Key::Char(1) => {
                ed.cursor = 0;
                ed.render();
            }
            Key::End | Key::Char(5) => {
                ed.cursor = ed.buffer.len();
                ed.render();
            }
            Key::Char(21) => {
                ed.buffer.drain(..ed.cursor);
                ed.cursor = 0;
                ed.render();
            }
            Key::Char(11) => {
                ed.buffer.truncate(ed.cursor);
                ed.render();
            }
            Key::Char(23) => {
                let mut start = ed.cursor;
                while start > 0 && ed.buffer[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && ed.buffer[start - 1] != ' ' {
                    start -= 1;
                }
                ed.buffer.drain(start..ed.cursor);
                ed.cursor = start;
                ed.render();
            }
            Key::Char(12) => {
                write(1, "\x1b[2J\x1b[H");
                ed.drawn_rows = 0;
                ed.cursor_row = 0;
                ed.render();
            }
            Key::Up | Key::Char(16) => {
                if hist_index > 0 {
                    if hist_index == history.len() {
                        draft = ed.text();
                    }
                    hist_index -= 1;
                    ed.set(&history[hist_index]);
                    ed.render();
                }
            }
            Key::Down | Key::Char(14) => {
                if hist_index < history.len() {
                    hist_index += 1;
                    if hist_index == history.len() {
                        let d = draft.clone();
                        ed.set(&d);
                    } else {
                        ed.set(&history[hist_index]);
                    }
                    ed.render();
                }
            }
            Key::Tab => {
                let before: String = ed.buffer[..ed.cursor].iter().collect();
                let candidates = complete(&before);
                let word_start = before.rfind(' ').map(|i| i + 1).unwrap_or(0);
                let word = &before[word_start..];
                if candidates.len() == 1 {
                    let completion = &candidates[0];
                    let suffix: Vec<char> = completion.chars().skip(word.chars().count()).collect();
                    let add_space = !completion.ends_with('/');
                    for (i, c) in suffix.iter().enumerate() {
                        ed.buffer.insert(ed.cursor + i, *c);
                    }
                    ed.cursor += suffix.len();
                    if add_space {
                        ed.buffer.insert(ed.cursor, ' ');
                        ed.cursor += 1;
                    }
                    ed.render();
                } else if candidates.len() > 1 {
                    let mut common: Vec<char> = candidates[0].chars().collect();
                    for c in &candidates[1..] {
                        let chars: Vec<char> = c.chars().collect();
                        let n = common.iter().zip(chars.iter()).take_while(|(a, b)| a == b).count();
                        common.truncate(n);
                    }
                    let typed = word.chars().count();
                    if common.len() > typed {
                        for (i, c) in common[typed..].iter().enumerate() {
                            ed.buffer.insert(ed.cursor + i, *c);
                        }
                        ed.cursor += common.len() - typed;
                        ed.render();
                    } else if was_tab {
                        let saved_cursor = ed.cursor;
                        ed.cursor = ed.buffer.len();
                        ed.render();
                        ed.finish();
                        let width = candidates.iter().map(|c| c.rsplit('/').next().unwrap_or(c).len()).max().unwrap_or(1) + 2;
                        let per_row = (cols / width).max(1);
                        let mut line = String::new();
                        for (i, c) in candidates.iter().take(120).enumerate() {
                            let shown = if c.ends_with('/') { c.trim_end_matches('/').rsplit('/').next().map(|s| format!("{}/", s)).unwrap_or_default() } else { String::from(c.rsplit('/').next().unwrap_or(c)) };
                            line.push_str(&format!("{:<w$}", shown, w = width));
                            if (i + 1) % per_row == 0 {
                                line.push_str("\r\n");
                            }
                        }
                        if !line.ends_with('\n') {
                            line.push_str("\r\n");
                        }
                        write(1, &line);
                        ed.drawn_rows = 0;
                        ed.cursor_row = 0;
                        write(1, prompt);
                        ed.cursor = saved_cursor;
                        ed.render();
                    } else {
                        last_tab = true;
                    }
                }
            }
            Key::Char(c) if c >= 0x20 => {
                let mut bytes = alloc::vec![c];
                if c >= 0xC0 {
                    let extra = if c >= 0xF0 { 3 } else if c >= 0xE0 { 2 } else { 1 };
                    for _ in 0..extra {
                        if let Key::Char(b) = sys::read_key() {
                            bytes.push(b);
                        }
                    }
                }
                let text = String::from_utf8_lossy(&bytes);
                for ch in text.chars() {
                    ed.buffer.insert(ed.cursor, ch);
                    ed.cursor += 1;
                }
                if ed.cursor == ed.buffer.len() && (ed.prompt_len + ed.cursor) % cols != 0 {
                    write(1, &text);
                    let pos = ed.prompt_len + ed.cursor;
                    ed.cursor_row = pos / cols;
                    ed.drawn_rows = ed.drawn_rows.max(ed.cursor_row);
                } else {
                    ed.render();
                }
            }
            _ => {}
        }
    }
}
