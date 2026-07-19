use spin::Mutex;
use crate::arch::x86_64::outb;

const VGA_BUFFER: usize = 0xB8000;
pub const COLS: usize = 80;
pub const ROWS: usize = 25;

pub const BLACK: u8 = 0x0;
pub const BLUE: u8 = 0x1;
pub const GREEN: u8 = 0x2;
pub const CYAN: u8 = 0x3;
pub const RED: u8 = 0x4;
pub const MAGENTA: u8 = 0x5;
pub const BROWN: u8 = 0x6;
pub const LIGHT_GRAY: u8 = 0x7;
pub const DARK_GRAY: u8 = 0x8;
pub const LIGHT_BLUE: u8 = 0x9;
pub const LIGHT_GREEN: u8 = 0xA;
pub const LIGHT_CYAN: u8 = 0xB;
pub const LIGHT_RED: u8 = 0xC;
pub const LIGHT_MAGENTA: u8 = 0xD;
pub const YELLOW: u8 = 0xE;
pub const WHITE: u8 = 0xF;

pub const fn attr(fg: u8, bg: u8) -> u8 {
    ((bg & 0x0F) << 4) | (fg & 0x0F)
}

const ATTR_DEFAULT: u8 = attr(LIGHT_GRAY, BLACK);

fn buffer() -> *mut u16 {
    VGA_BUFFER as *mut u16
}

fn set_hw_cursor(pos: usize) {
    outb(0x3D4, 0x0F);
    outb(0x3D5, (pos & 0xFF) as u8);
    outb(0x3D4, 0x0E);
    outb(0x3D5, ((pos >> 8) & 0xFF) as u8);
}

pub fn set_hw_cursor_visible(visible: bool) {
    outb(0x3D4, 0x0A);
    outb(0x3D5, if visible { 0x00 } else { 0x20 });
}

pub struct TextConsole {
    col: usize,
    row: usize,
    attr: u8,
}

impl TextConsole {
    const fn new() -> Self {
        Self {
            col: 0,
            row: 0,
            attr: ATTR_DEFAULT,
        }
    }

    pub fn clear(&mut self) {
        let buf = buffer();
        let blank: u16 = ((self.attr as u16) << 8) | (b' ' as u16);
        for i in 0..(COLS * ROWS) {
            unsafe {
                core::ptr::write_volatile(buf.add(i), blank);
            }
        }
        self.col = 0;
        self.row = 0;
        self.sync_hw_cursor();
    }

    fn put_char_at(&self, ch: u8, col: usize, row: usize, attr: u8) {
        if col >= COLS || row >= ROWS {
            return;
        }
        let buf = buffer();
        let idx = row * COLS + col;
        let entry: u16 = ((attr as u16) << 8) | (ch as u16);
        unsafe {
            core::ptr::write_volatile(buf.add(idx), entry);
        }
    }

    fn scroll_one(&mut self) {
        let buf = buffer();
        unsafe {
            core::ptr::copy(buf.add(COLS), buf, COLS * (ROWS - 1));
        }
        for col in 0..COLS {
            self.put_char_at(b' ', col, ROWS - 1, self.attr);
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= ROWS {
            self.scroll_one();
            self.row = ROWS - 1;
        }
    }

    fn sync_hw_cursor(&self) {
        set_hw_cursor(self.row * COLS + self.col);
    }

    pub fn write_char_attr(&mut self, ch: char, attr: u8) {
        match ch {
            '\n' => self.newline(),
            '\r' => {
                self.col = 0;
            }
            '\x08' => {
                if self.col > 0 {
                    self.col -= 1;
                    self.put_char_at(b' ', self.col, self.row, attr);
                }
            }
            _ => {
                let byte = if ch.is_ascii() { ch as u8 } else { b'?' };
                self.put_char_at(byte, self.col, self.row, attr);
                self.col += 1;
                if self.col >= COLS {
                    self.newline();
                }
            }
        }
        self.sync_hw_cursor();
    }

    pub fn write_char(&mut self, ch: char) {
        self.write_char_attr(ch, self.attr);
    }

    pub fn write_str_attr(&mut self, s: &str, attr: u8) {
        for ch in s.chars() {
            self.write_char_attr(ch, attr);
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.write_char(ch);
        }
    }

    pub fn set_default_attr(&mut self, attr: u8) {
        self.attr = attr;
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.col, self.row)
    }
}

pub static TEXT_CONSOLE: Mutex<TextConsole> = Mutex::new(TextConsole::new());

fn draw_centered(buf: *mut u16, row: usize, text: &[u8], line_attr: u8) {
    if row >= ROWS {
        return;
    }
    let len = text.len().min(COLS);
    let start_col = (COLS - len) / 2;
    for (i, &b) in text[..len].iter().enumerate() {
        let idx = row * COLS + start_col + i;
        let entry: u16 = ((line_attr as u16) << 8) | (b as u16);
        unsafe {
            core::ptr::write_volatile(buf.add(idx), entry);
        }
    }
}

pub fn draw_panic_screen(title: &str, reason: &str) {
    let buf = buffer();
    let bg_attr = attr(WHITE, RED);
    let blank: u16 = ((bg_attr as u16) << 8) | (b' ' as u16);
    for i in 0..(COLS * ROWS) {
        unsafe {
            core::ptr::write_volatile(buf.add(i), blank);
        }
    }

    let title_row = ROWS / 2 - 3;
    draw_centered(buf, title_row, title.as_bytes(), attr(YELLOW, RED));
    draw_centered(
        buf,
        title_row + 1,
        b"----------------------------------------",
        bg_attr,
    );

    let max_width = COLS - 8;
    let mut row = title_row + 3;
    let mut line_buf = [0u8; COLS];
    let mut line_len = 0usize;

    for word in reason.split(' ') {
        if word.is_empty() {
            continue;
        }
        let word_bytes = word.as_bytes();
        let sep = if line_len > 0 { 1 } else { 0 };

        if line_len + sep + word_bytes.len() > max_width {
            if line_len > 0 {
                draw_centered(buf, row, &line_buf[..line_len], bg_attr);
                row += 1;
                line_len = 0;
                if row >= ROWS {
                    return;
                }
            }
            let mut remaining = word_bytes;
            while remaining.len() > max_width {
                draw_centered(buf, row, &remaining[..max_width], bg_attr);
                row += 1;
                if row >= ROWS {
                    return;
                }
                remaining = &remaining[max_width..];
            }
            for &b in remaining {
                line_buf[line_len] = b;
                line_len += 1;
            }
            continue;
        }

        if sep == 1 {
            line_buf[line_len] = b' ';
            line_len += 1;
        }
        for &b in word_bytes {
            line_buf[line_len] = b;
            line_len += 1;
        }
    }

    if line_len > 0 && row < ROWS {
        draw_centered(buf, row, &line_buf[..line_len], bg_attr);
    }

    let footer_row = ROWS - 2;
    draw_centered(
        buf,
        footer_row,
        b"System halted -- power cycle the machine to restart",
        attr(LIGHT_GRAY, RED),
    );

    set_hw_cursor_visible(false);
}
