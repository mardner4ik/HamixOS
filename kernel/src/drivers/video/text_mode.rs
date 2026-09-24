use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

use crate::arch::io::outb;
use crate::memory::FramebufferInfo;

pub const COLS: usize = 80;
pub const ROWS: usize = 25;
pub const VT_COUNT: usize = 6;
const CELL_COUNT: usize = COLS * ROWS;

pub const BLACK: u8 = 0x0;
pub const RED: u8 = 0x4;
pub const LIGHT_GRAY: u8 = 0x7;
pub const YELLOW: u8 = 0xE;
pub const WHITE: u8 = 0xF;

pub const fn attr(fg: u8, bg: u8) -> u8 {
    ((bg & 0x0F) << 4) | (fg & 0x0F)
}

const ATTR_DEFAULT: u8 = attr(LIGHT_GRAY, BLACK);
const BLANK: u32 = pack(' ', ATTR_DEFAULT);
const CHAR_MASK: u32 = 0x1F_FFFF;

const fn pack(ch: char, attr_byte: u8) -> u32 {
    (ch as u32 & CHAR_MASK) | ((attr_byte as u32) << 21)
}
const VGA_BUFFER: usize = 0xB8000;
const INVALID: u32 = u32::MAX;
const HIGHLIGHT_BIT: u32 = 1 << 29;
const CURSOR_BIT: u32 = 1 << 30;
const MAX_CELL_W: usize = 256;

const VGA_PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA,
    0x555555, 0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

pub static POINTER_CELL: AtomicUsize = AtomicUsize::new(usize::MAX);
pub static SELECTION_START: AtomicUsize = AtomicUsize::new(usize::MAX);
pub static SELECTION_END: AtomicUsize = AtomicUsize::new(usize::MAX);
static GRAPHICS_OWNED: AtomicBool = AtomicBool::new(false);
static FOREGROUND: AtomicUsize = AtomicUsize::new(0);
static CURSOR_CELL: AtomicUsize = AtomicUsize::new(usize::MAX);

struct Shared<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Shared<T> {}

static SCREENS: Shared<[[u32; CELL_COUNT]; VT_COUNT]> = Shared(UnsafeCell::new([[BLANK; CELL_COUNT]; VT_COUNT]));
static RENDERED: Shared<[u32; CELL_COUNT]> = Shared(UnsafeCell::new([INVALID; CELL_COUNT]));
static DISPLAY: Shared<Option<(FramebufferInfo, Geometry)>> = Shared(UnsafeCell::new(None));

fn cells(vt: usize) -> *mut u32 {
    unsafe { (*SCREENS.0.get())[vt.min(VT_COUNT - 1)].as_mut_ptr() }
}

fn rendered() -> *mut u32 {
    unsafe { (*RENDERED.0.get()).as_mut_ptr() }
}

fn display() -> Option<(FramebufferInfo, Geometry)> {
    unsafe { *DISPLAY.0.get() }
}

fn active_display() -> Option<(FramebufferInfo, Geometry)> {
    if GRAPHICS_OWNED.load(Ordering::Relaxed) {
        return None;
    }
    display()
}

#[derive(Clone, Copy)]
struct Geometry {
    origin_x: usize,
    origin_y: usize,
    cell_w: usize,
    cell_h: usize,
    glyph_h: usize,
    glyph_y: usize,
}

fn geometry(fb: &FramebufferInfo) -> Geometry {
    let cell_w = (fb.width as usize / COLS).clamp(1, MAX_CELL_W);
    let cell_h = (fb.height as usize / ROWS).max(1);
    let glyph_h = cell_h.min(cell_w * 2).max(1);
    Geometry {
        origin_x: (fb.width as usize).saturating_sub(cell_w * COLS) / 2,
        origin_y: (fb.height as usize).saturating_sub(cell_h * ROWS) / 2,
        cell_w,
        cell_h,
        glyph_h,
        glyph_y: (cell_h - glyph_h) / 2,
    }
}

pub fn cache_framebuffer() {
    let info = *crate::memory::FRAMEBUFFER.lock();
    unsafe { *DISPLAY.0.get() = info.map(|fb| (fb, geometry(&fb))) };
    invalidate();
}

pub fn set_graphics_owned(owned: bool) {
    GRAPHICS_OWNED.store(owned, Ordering::SeqCst);
    if !owned {
        invalidate();
    }
}

pub fn graphics_owned() -> bool {
    GRAPHICS_OWNED.load(Ordering::Relaxed)
}

pub fn foreground() -> usize {
    FOREGROUND.load(Ordering::Relaxed)
}

fn invalidate() {
    unsafe { core::ptr::write_bytes(rendered() as *mut u8, 0xFF, CELL_COUNT * 4) };
}

fn highlighted(index: usize) -> bool {
    if POINTER_CELL.load(Ordering::Relaxed) == index {
        return true;
    }
    let start = SELECTION_START.load(Ordering::Relaxed);
    if start == usize::MAX {
        return false;
    }
    let end = SELECTION_END.load(Ordering::Relaxed);
    let (lo, hi) = if start <= end { (start, end) } else { (end, start) };
    index >= lo && index <= hi
}

fn cell_key(vt: usize, index: usize) -> u32 {
    let entry = unsafe { *cells(vt).add(index) };
    let mut key = entry;
    if highlighted(index) {
        key |= HIGHLIGHT_BIT;
    }
    if CURSOR_CELL.load(Ordering::Relaxed) == index {
        key |= CURSOR_BIT;
    }
    key
}

pub fn cell_at_pixel(x: i32, y: i32) -> Option<(usize, usize)> {
    let (_, geo) = display()?;
    let col = (x - geo.origin_x as i32).max(0) as usize / geo.cell_w;
    let row = (y - geo.origin_y as i32).max(0) as usize / geo.cell_h;
    Some((col.min(COLS - 1), row.min(ROWS - 1)))
}

pub fn cell_char(index: usize) -> char {
    if index >= CELL_COUNT {
        return ' ';
    }
    let ch = char::from_u32(unsafe { *cells(foreground()).add(index) } & CHAR_MASK).unwrap_or(' ');
    if ch.is_control() { ' ' } else { ch }
}

pub fn render_cell(col: usize, row: usize) {
    if col >= COLS || row >= ROWS {
        return;
    }
    let Some((fb, geo)) = active_display() else {
        return;
    };
    let index = row * COLS + col;
    let key = cell_key(foreground(), index);
    draw_cell(&fb, &geo, index, key);
    unsafe { *rendered().add(index) = key };
}

#[inline]
fn write_row(fb: &FramebufferInfo, x: usize, y: usize, row: &[u32]) {
    if y >= fb.height as usize || x >= fb.width as usize {
        return;
    }
    let count = row.len().min(fb.width as usize - x);
    super::gpu::mark_dirty(x as u32, y as u32, count as u32, 1);
    let base = fb.addr as usize + y * fb.pitch as usize;
    unsafe {
        match fb.bpp {
            32 => core::ptr::copy_nonoverlapping(row.as_ptr(), (base + x * 4) as *mut u32, count),
            24 => {
                let mut ptr = (base + x * 3) as *mut u8;
                for &value in &row[..count] {
                    *ptr = value as u8;
                    *ptr.add(1) = (value >> 8) as u8;
                    *ptr.add(2) = (value >> 16) as u8;
                    ptr = ptr.add(3);
                }
            }
            16 => {
                let mut ptr = (base + x * 2) as *mut u16;
                for &value in &row[..count] {
                    let packed = ((value >> 8) & 0xF800) | ((value >> 5) & 0x07E0) | ((value >> 3) & 0x001F);
                    *ptr = packed as u16;
                    ptr = ptr.add(1);
                }
            }
            _ => {}
        }
    }
}

pub fn ascii_fallback(ch: char) -> u8 {
    if (ch as u32) < 128 {
        return ch as u8;
    }
    match ch {
        '°' | 'º' => b'o',
        '·' | '•' | '∙' | '●' | '○' | '◦' => b'*',
        '…' => b'.',
        '–' | '—' | '−' | '‐' => b'-',
        '‘' | '’' | '′' => b'\'',
        '“' | '”' | '″' | '«' | '»' => b'"',
        '↑' | '▲' | '△' | '▴' | '⇡' => b'^',
        '↓' | '▼' | '▽' | '▾' | '⇣' => b'v',
        '←' | '◀' | '◁' | '◂' | '‹' => b'<',
        '→' | '▶' | '▷' | '▸' | '►' | '›' => b'>',
        '×' => b'x',
        '÷' => b'/',
        '±' => b'+',
        '≤' => b'<',
        '≥' => b'>',
        '≠' => b'#',
        '✓' | '✔' => b'v',
        '✗' | '✘' => b'x',
        '\u{A0}' => b' ',
        _ => {
            let base = match ch {
                'À'..='Å' => 'A',
                'à'..='å' => 'a',
                'È'..='Ë' => 'E',
                'è'..='ë' => 'e',
                'Ì'..='Ï' => 'I',
                'ì'..='ï' => 'i',
                'Ò'..='Ö' | 'Ø' => 'O',
                'ò'..='ö' | 'ø' => 'o',
                'Ù'..='Ü' => 'U',
                'ù'..='ü' => 'u',
                'Ç' => 'C',
                'ç' => 'c',
                'Ñ' => 'N',
                'ñ' => 'n',
                'А' | 'а' => 'a',
                'В' | 'в' => 'B',
                'Е' | 'е' | 'Є' | 'є' | 'Ё' | 'ё' => 'e',
                'К' | 'к' => 'k',
                'М' | 'м' => 'M',
                'Н' | 'н' => 'H',
                'О' | 'о' => 'o',
                'Р' | 'р' => 'p',
                'С' | 'с' => 'c',
                'Т' | 'т' => 'T',
                'Х' | 'х' => 'x',
                'У' | 'у' => 'y',
                'І' | 'і' | 'Ї' | 'ї' => 'i',
                _ => '?',
            };
            base as u8
        }
    }
}

fn draw_cell(fb: &FramebufferInfo, geo: &Geometry, index: usize, key: u32) {
    use crate::drivers::video::font8x8::{GLYPH_H, GLYPH_W};

    let col = index % COLS;
    let row = index / COLS;
    let ch = char::from_u32(key & CHAR_MASK).unwrap_or('?');
    let mut attr_byte = ((key >> 21) & 0xFF) as u8;
    if key & HIGHLIGHT_BIT != 0 {
        attr_byte = attr_byte.rotate_left(4);
    }
    let fg = VGA_PALETTE[(attr_byte & 0x0F) as usize];
    let bg = VGA_PALETTE[(attr_byte >> 4) as usize];

    let cx = geo.origin_x + col * geo.cell_w;
    let cy = geo.origin_y + row * geo.cell_h;
    let width = geo.cell_w;
    let cursor_h = if key & CURSOR_BIT != 0 { (geo.glyph_h / 8).max(2) } else { 0 };

    let mut line = [0u32; MAX_CELL_W];
    if (ch as u32) >= 128 {
        let mut mask = alloc::vec![0u8; width * geo.cell_h];
        let drawn = hxvt::rasterize(ch, width as i32, geo.cell_h as i32, &mut |x, y, w, h, alpha| {
            for yy in y.max(0)..(y + h).min(geo.cell_h as i32) {
                for xx in x.max(0)..(x + w).min(width as i32) {
                    let slot = &mut mask[yy as usize * width + xx as usize];
                    *slot = (*slot).max(alpha);
                }
            }
        });
        if drawn {
            for y in 0..geo.cell_h {
                for x in 0..width {
                    let alpha = mask[y * width + x] as u32;
                    let dither = [0u32, 128, 32, 160, 192, 64, 224, 96][(x % 2) * 4 + (y % 4)];
                    line[x] = if y + cursor_h >= geo.cell_h || alpha > dither { fg } else { bg };
                }
                write_row(fb, cx, cy + y, &line[..width]);
            }
            return;
        }
    }
    let glyph = crate::drivers::video::font8x8::glyph(ascii_fallback(ch));

    let mut column_bit = [0u8; MAX_CELL_W];
    for (x, bit) in column_bit.iter_mut().enumerate().take(width) {
        *bit = (x * GLYPH_W / width) as u8;
    }

    let mut previous: Option<u16> = None;
    for y in 0..geo.cell_h {
        let pattern: u16 = if y + cursor_h >= geo.cell_h {
            0x100
        } else if y >= geo.glyph_y && y < geo.glyph_y + geo.glyph_h {
            glyph[(y - geo.glyph_y) * GLYPH_H / geo.glyph_h] as u16
        } else {
            0
        };
        if previous != Some(pattern) {
            if pattern == 0x100 {
                line[..width].fill(fg);
            } else {
                for x in 0..width {
                    line[x] = if (pattern >> column_bit[x]) & 1 != 0 { fg } else { bg };
                }
            }
            previous = Some(pattern);
        }
        write_row(fb, cx, cy + y, &line[..width]);
    }
}

fn flush_screen(vt: usize) {
    if vt != foreground() {
        return;
    }
    match active_display() {
        Some((fb, geo)) => {
            let shadow = rendered();
            for index in 0..CELL_COUNT {
                let key = cell_key(vt, index);
                unsafe {
                    if *shadow.add(index) != key {
                        draw_cell(&fb, &geo, index, key);
                        *shadow.add(index) = key;
                    }
                }
            }
        }
        None => {
            if display().is_none() && crate::arch::io::PORTS {
                let buf = cells(vt);
                for i in 0..CELL_COUNT {
                    let word = unsafe { *buf.add(i) };
                    let ch = char::from_u32(word & CHAR_MASK).map(ascii_fallback).unwrap_or(b'?');
                    unsafe { *(VGA_BUFFER as *mut u16).add(i) = (((word >> 21) as u16 & 0xFF) << 8) | ch as u16 };
                }
                let pos = CURSOR_CELL.load(Ordering::Relaxed);
                if pos < CELL_COUNT {
                    outb(0x3D4, 0x0F);
                    outb(0x3D5, (pos & 0xFF) as u8);
                    outb(0x3D4, 0x0E);
                    outb(0x3D5, ((pos >> 8) & 0xFF) as u8);
                }
            }
        }
    }
}

pub fn fb_clear_full() {
    if let Some((fb, _)) = display() {
        unsafe { core::ptr::write_bytes(fb.addr as *mut u8, 0, fb.byte_len() as usize) };
        super::gpu::mark_dirty(0, 0, fb.width, fb.height);
    }
    invalidate();
}

pub fn fb_redraw_all() {
    invalidate();
    if let Some(mut console) = TEXT_CONSOLE.try_lock() {
        console.flush_vt(foreground());
    }
}

pub fn set_hw_cursor_visible(visible: bool) {
    if !crate::arch::io::PORTS {
        return;
    }
    outb(0x3D4, 0x0A);
    outb(0x3D5, if visible { 0x00 } else { 0x20 });
}

const ANSI_TO_VGA16: [u8; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];

fn nearest_vga(rgb: u32) -> u8 {
    let (r, g, b) = ((rgb >> 16) as i32 & 0xFF, (rgb >> 8) as i32 & 0xFF, rgb as i32 & 0xFF);
    let mut best = 0u8;
    let mut best_d = i32::MAX;
    for (i, c) in VGA_PALETTE.iter().enumerate() {
        let (cr, cg, cb) = ((c >> 16) as i32 & 0xFF, (c >> 8) as i32 & 0xFF, *c as i32 & 0xFF);
        let d = (r - cr) * (r - cr) * 3 + (g - cg) * (g - cg) * 4 + (b - cb) * (b - cb) * 2;
        if d < best_d {
            best_d = d;
            best = i as u8;
        }
    }
    best
}

fn vga_color(color: hxvt::Color, default: u8) -> u8 {
    match color {
        hxvt::Color::Default => default,
        hxvt::Color::Indexed(n) if n < 16 => ANSI_TO_VGA16[n as usize],
        hxvt::Color::Indexed(n) => {
            let base: [u32; 16] = core::array::from_fn(|i| VGA_PALETTE[ANSI_TO_VGA16[i] as usize]);
            nearest_vga(hxvt::palette_rgb(n, &base))
        }
        hxvt::Color::Rgb(r, g, b) => nearest_vga(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
    }
}

fn cell_word(cell: &hxvt::Cell) -> u32 {
    let mut fg = vga_color(cell.fg, LIGHT_GRAY);
    let mut bg = vga_color(cell.bg, BLACK);
    if cell.flags & hxvt::BOLD != 0 && fg < 8 {
        fg |= 8;
    }
    if cell.flags & hxvt::DIM != 0 && fg >= 8 {
        fg &= 7;
    }
    if cell.flags & hxvt::REVERSE != 0 {
        core::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags & hxvt::INVISIBLE != 0 {
        fg = bg;
    }
    let ch = if cell.flags & hxvt::WIDE_SPACER != 0 { ' ' } else { cell.ch };
    pack(ch, attr(fg, bg))
}

pub struct TextConsole {
    terms: [Option<hxvt::Term>; VT_COUNT],
    input: [alloc::collections::VecDeque<u8>; VT_COUNT],
}

impl TextConsole {
    const fn new() -> Self {
        Self { terms: [const { None }; VT_COUNT], input: [const { alloc::collections::VecDeque::new() }; VT_COUNT] }
    }

    fn term(&mut self, vt: usize) -> &mut hxvt::Term {
        let vt = vt.min(VT_COUNT - 1);
        self.terms[vt].get_or_insert_with(|| hxvt::Term::new(COLS, ROWS, 0))
    }

    pub fn modes(&mut self, vt: usize) -> hxvt::Modes {
        *self.term(vt).modes()
    }

    pub fn take_input(&mut self, vt: usize) -> alloc::vec::Vec<u8> {
        self.input[vt.min(VT_COUNT - 1)].drain(..).collect()
    }

    pub fn push_input(&mut self, vt: usize, bytes: &[u8]) {
        self.input[vt.min(VT_COUNT - 1)].extend(bytes.iter().copied());
    }

    fn sync(&mut self, vt: usize) {
        let vt = vt.min(VT_COUNT - 1);
        let term = self.terms[vt].get_or_insert_with(|| hxvt::Term::new(COLS, ROWS, 0));
        let buf = cells(vt);
        for row in 0..ROWS {
            if !term.is_dirty(row) {
                continue;
            }
            for (col, cell) in term.row(row).iter().enumerate().take(COLS) {
                unsafe { *buf.add(row * COLS + col) = cell_word(cell) };
            }
        }
        term.clear_dirty();
        let responses = term.take_responses();
        if !responses.is_empty() {
            self.input[vt].extend(responses);
            crate::task::notify_input();
        }
    }

    fn flush_vt(&mut self, vt: usize) {
        self.sync(vt);
        if vt == foreground() {
            let term = self.term(vt);
            let cursor = term.cursor();
            let visible = term.cursor_visible();
            CURSOR_CELL.store(if visible { cursor.0 * COLS + cursor.1 } else { usize::MAX }, Ordering::Relaxed);
            flush_screen(vt);
        }
    }

    fn feed_cooked(&mut self, vt: usize, bytes: &[u8]) {
        let term = self.term(vt);
        let mut start = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                term.feed(&bytes[start..i]);
                term.feed(b"\r\n");
                start = i + 1;
            }
        }
        term.feed(&bytes[start..]);
    }

    pub fn write_bytes_to(&mut self, vt: usize, bytes: &[u8]) {
        mirror(vt, bytes, true);
        self.feed_cooked(vt, bytes);
        self.flush_vt(vt);
    }

    pub fn write_raw_to(&mut self, vt: usize, bytes: &[u8]) {
        mirror(vt, bytes, false);
        self.term(vt).feed(bytes);
        self.flush_vt(vt);
    }

    pub fn switch_to(&mut self, vt: usize) {
        if vt >= VT_COUNT {
            return;
        }
        FOREGROUND.store(vt, Ordering::SeqCst);
        invalidate();
        self.flush_vt(vt);
    }
}

pub static TEXT_CONSOLE: Mutex<TextConsole> = Mutex::new(TextConsole::new());

static SERIAL_MIRROR: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(!cfg!(target_arch = "x86_64"));

pub fn set_serial_mirror(enabled: bool) {
    SERIAL_MIRROR.store(enabled, Ordering::Relaxed);
}

pub fn serial_mirror() -> bool {
    SERIAL_MIRROR.load(Ordering::Relaxed)
}

fn mirror(vt: usize, bytes: &[u8], cooked: bool) {
    if vt != 0 || !SERIAL_MIRROR.load(Ordering::Relaxed) {
        return;
    }
    let serial = crate::drivers::serial::SERIAL.lock();
    for &b in bytes {
        if cooked && b == b'\n' {
            serial.write_byte(b'\r');
        }
        serial.write_byte(b);
    }
}

fn draw_centered(buf: *mut u32, row: usize, text: &[u8], line_attr: u8) {
    if row >= ROWS {
        return;
    }
    let len = text.len().min(COLS);
    let start_col = (COLS - len) / 2;
    for (i, &b) in text[..len].iter().enumerate() {
        unsafe { *buf.add(row * COLS + start_col + i) = pack(b as char, line_attr) };
    }
}

pub fn draw_panic_screen(title: &str, reason: &str) {
    let vt = foreground();
    let buf = cells(vt);
    let bg_attr = attr(WHITE, RED);
    let blank: u32 = pack(' ', bg_attr);
    for i in 0..CELL_COUNT {
        unsafe { *buf.add(i) = blank };
    }

    let title_row = ROWS / 2 - 3;
    draw_centered(buf, title_row, title.as_bytes(), attr(YELLOW, RED));
    draw_centered(buf, title_row + 1, b"----------------------------------------", bg_attr);

    let max_width = COLS - 8;
    let mut row = title_row + 3;
    let mut line_buf = [0u8; COLS];
    let mut line_len = 0usize;

    'words: for word in reason.split(' ') {
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
                    break 'words;
                }
            }
            let mut remaining = word_bytes;
            while remaining.len() > max_width {
                draw_centered(buf, row, &remaining[..max_width], bg_attr);
                row += 1;
                if row >= ROWS {
                    break 'words;
                }
                remaining = &remaining[max_width..];
            }
            line_buf[..remaining.len()].copy_from_slice(remaining);
            line_len = remaining.len();
            continue;
        }
        if sep == 1 {
            line_buf[line_len] = b' ';
            line_len += 1;
        }
        line_buf[line_len..line_len + word_bytes.len()].copy_from_slice(word_bytes);
        line_len += word_bytes.len();
    }

    if line_len > 0 && row < ROWS {
        draw_centered(buf, row, &line_buf[..line_len], bg_attr);
    }

    draw_centered(buf, ROWS - 2, b"System halted -- power cycle the machine to restart", attr(LIGHT_GRAY, RED));

    set_hw_cursor_visible(false);
    GRAPHICS_OWNED.store(false, Ordering::SeqCst);
    CURSOR_CELL.store(usize::MAX, Ordering::SeqCst);
    invalidate();
    flush_screen(vt);
}
