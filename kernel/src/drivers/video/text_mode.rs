use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;
use crate::arch::x86_64::outb;

pub static POINTER_CELL: AtomicUsize = AtomicUsize::new(usize::MAX);
pub static SELECTION_START: AtomicUsize = AtomicUsize::new(usize::MAX);
pub static SELECTION_END: AtomicUsize = AtomicUsize::new(usize::MAX);
static GRAPHICS_OWNED: AtomicBool = AtomicBool::new(false);

static mut FB_CACHE: Option<crate::memory::FramebufferInfo> = None;

pub fn cache_framebuffer() {
    let info = *crate::memory::FRAMEBUFFER.lock();
    unsafe { FB_CACHE = info };
}

fn framebuffer() -> Option<crate::memory::FramebufferInfo> {
    if GRAPHICS_OWNED.load(Ordering::Relaxed) {
        return None;
    }
    unsafe { *(&raw const FB_CACHE) }
}

pub fn set_graphics_owned(owned: bool) {
    GRAPHICS_OWNED.store(owned, Ordering::SeqCst);
}

pub fn graphics_owned() -> bool {
    GRAPHICS_OWNED.load(Ordering::Relaxed)
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

pub fn cell_at_pixel(x: i32, y: i32) -> Option<(usize, usize)> {
    let fb = unsafe { *(&raw const FB_CACHE) }?;
    let geo = geometry(&fb);
    let col = (x - geo.origin_x as i32).max(0) as usize / geo.cell_w;
    let row = (y - geo.origin_y as i32).max(0) as usize / geo.cell_h;
    Some((col.min(COLS - 1), row.min(ROWS - 1)))
}

pub fn cell_char(index: usize) -> char {
    if index >= COLS * ROWS {
        return ' ';
    }
    let entry = unsafe { core::ptr::read_volatile(buffer().add(index)) };
    let byte = (entry & 0xFF) as u8;
    if byte.is_ascii_graphic() || byte == b' ' { byte as char } else { ' ' }
}

pub fn render_cell(col: usize, row: usize) {
    if col >= COLS || row >= ROWS {
        return;
    }
    let entry = unsafe { core::ptr::read_volatile(buffer().add(row * COLS + col)) };
    fb_render_entry(col, row, entry);
}

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

const VGA_PALETTE: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0x00, 0x00, 0xAA),
    (0x00, 0xAA, 0x00),
    (0x00, 0xAA, 0xAA),
    (0xAA, 0x00, 0x00),
    (0xAA, 0x00, 0xAA),
    (0xAA, 0x55, 0x00),
    (0xAA, 0xAA, 0xAA),
    (0x55, 0x55, 0x55),
    (0x55, 0x55, 0xFF),
    (0x55, 0xFF, 0x55),
    (0x55, 0xFF, 0xFF),
    (0xFF, 0x55, 0x55),
    (0xFF, 0x55, 0xFF),
    (0xFF, 0xFF, 0x55),
    (0xFF, 0xFF, 0xFF),
];

unsafe fn fill_hline(fb: &crate::memory::FramebufferInfo, x: usize, y: usize, w: usize, rgb: (u8, u8, u8)) {
    if y >= fb.height as usize || w == 0 || x >= fb.width as usize {
        return;
    }
    let count = w.min(fb.width as usize - x);
    let (r, g, b) = rgb;
    let bytes = (fb.bpp as usize / 8).max(1);
    let base = fb.addr as usize + y * fb.pitch as usize + x * bytes;
    unsafe {
        match fb.bpp {
            32 => {
                let value = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
                let mut ptr = base as *mut u32;
                for _ in 0..count {
                    core::ptr::write_volatile(ptr, value);
                    ptr = ptr.add(1);
                }
            }
            24 => {
                let mut ptr = base as *mut u8;
                for _ in 0..count {
                    core::ptr::write_volatile(ptr, b);
                    core::ptr::write_volatile(ptr.add(1), g);
                    core::ptr::write_volatile(ptr.add(2), r);
                    ptr = ptr.add(3);
                }
            }
            16 => {
                let value = (((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3);
                let mut ptr = base as *mut u16;
                for _ in 0..count {
                    core::ptr::write_volatile(ptr, value);
                    ptr = ptr.add(1);
                }
            }
            _ => {}
        }
    }
}

unsafe fn fill_rect_fb(
    fb: &crate::memory::FramebufferInfo,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgb: (u8, u8, u8),
) {
    for dy in 0..h {
        unsafe { fill_hline(fb, x, y + dy, w, rgb) };
    }
}

#[derive(Clone, Copy)]
struct Geometry {
    origin_x: usize,
    origin_y: usize,
    cell_w: usize,
    cell_h: usize,
    glyph_w: usize,
    glyph_h: usize,
    glyph_x: usize,
    glyph_y: usize,
}

fn geometry(fb: &crate::memory::FramebufferInfo) -> Geometry {
    let cell_w = (fb.width as usize / COLS).max(1);
    let cell_h = (fb.height as usize / ROWS).max(1);
    let glyph_w = cell_w;
    let glyph_h = cell_h.min(glyph_w * 2).max(1);

    Geometry {
        origin_x: (fb.width as usize).saturating_sub(cell_w * COLS) / 2,
        origin_y: (fb.height as usize).saturating_sub(cell_h * ROWS) / 2,
        cell_w,
        cell_h,
        glyph_w,
        glyph_h,
        glyph_x: (cell_w - glyph_w) / 2,
        glyph_y: (cell_h - glyph_h) / 2,
    }
}

fn fb_render_entry(col: usize, row: usize, entry: u16) {
    let Some(fb) = framebuffer() else {
        return;
    };
    render_entry_on(&fb, &geometry(&fb), col, row, entry);
}

fn render_entry_on(
    fb: &crate::memory::FramebufferInfo,
    geo: &Geometry,
    col: usize,
    row: usize,
    entry: u16,
) {
    use crate::drivers::video::font8x8::{GLYPH_H, GLYPH_W};

    let ch = (entry & 0xFF) as u8;
    let mut attr_byte = (entry >> 8) as u8;
    if highlighted(row * COLS + col) {
        attr_byte = ((attr_byte & 0x0F) << 4) | ((attr_byte >> 4) & 0x0F);
    }
    let fg = VGA_PALETTE[(attr_byte & 0x0F) as usize];
    let bg = VGA_PALETTE[((attr_byte >> 4) & 0x0F) as usize];
    let glyph = crate::drivers::video::font8x8::glyph(ch);

    let cx = geo.origin_x + col * geo.cell_w;
    let cy = geo.origin_y + row * geo.cell_h;
    unsafe { fill_rect_fb(fb, cx, cy, geo.cell_w, geo.cell_h, bg) };

    if fg == bg {
        return;
    }

    let gx0 = cx + geo.glyph_x;
    let gy0 = cy + geo.glyph_y;
    for gy in 0..GLYPH_H {
        let bits = glyph[gy];
        if bits == 0 {
            continue;
        }
        let y0 = gy * geo.glyph_h / GLYPH_H;
        let y1 = (gy + 1) * geo.glyph_h / GLYPH_H;
        let mut gx = 0;
        while gx < GLYPH_W {
            if (bits >> gx) & 1 == 0 {
                gx += 1;
                continue;
            }
            let start = gx;
            while gx < GLYPH_W && (bits >> gx) & 1 != 0 {
                gx += 1;
            }
            let x0 = start * geo.glyph_w / GLYPH_W;
            let x1 = gx * geo.glyph_w / GLYPH_W;
            unsafe { fill_rect_fb(fb, gx0 + x0, gy0 + y0, x1 - x0, y1 - y0, fg) };
        }
    }
}

pub fn fb_clear_full() {
    if let Some(fb) = unsafe { *(&raw const FB_CACHE) } {
        let total = fb.pitch as usize * fb.height as usize;
        unsafe {
            core::ptr::write_bytes(fb.addr as *mut u8, 0, total);
        }
    }
}

static LAST_FB_CURSOR: Mutex<Option<(usize, usize)>> = Mutex::new(None);

fn fb_draw_cursor(col: usize, row: usize, attr: u8) {
    let mut last = LAST_FB_CURSOR.lock();
    if let Some((lc, lr)) = *last {
        if (lc, lr) != (col, row) {
            let buf = buffer();
            let entry = unsafe { core::ptr::read_volatile(buf.add(lr * COLS + lc)) };
            fb_render_entry(lc, lr, entry);
        }
    }
    *last = Some((col, row));
    let Some(fb) = framebuffer() else {
        return;
    };
    let geo = geometry(&fb);
    let fg = VGA_PALETTE[(attr & 0x0F) as usize];
    let thickness = (geo.glyph_h / 4).max(1).min(geo.cell_h);
    let x = geo.origin_x + col * geo.cell_w + geo.glyph_x;
    let y = geo.origin_y + (row + 1) * geo.cell_h - geo.glyph_y - thickness;
    unsafe { fill_rect_fb(&fb, x, y, geo.glyph_w, thickness, fg) };
}

pub fn fb_redraw_all() {
    let Some(fb) = framebuffer() else {
        vga_sync_all();
        return;
    };
    let geo = geometry(&fb);
    let buf = buffer();
    for row in 0..ROWS {
        for col in 0..COLS {
            let entry = unsafe { core::ptr::read_volatile(buf.add(row * COLS + col)) };
            render_entry_on(&fb, &geo, col, row, entry);
        }
    }
    *LAST_FB_CURSOR.lock() = None;
}

struct CellBuffer(core::cell::UnsafeCell<[u16; COLS * ROWS]>);
unsafe impl Sync for CellBuffer {}

static CELLS: CellBuffer = CellBuffer(core::cell::UnsafeCell::new([0; COLS * ROWS]));

fn buffer() -> *mut u16 {
    CELLS.0.get() as *mut u16
}

fn vga_mirroring() -> bool {
    unsafe { (&raw const FB_CACHE).read().is_none() }
}

fn vga_write_cell(idx: usize, entry: u16) {
    if !vga_mirroring() {
        return;
    }
    unsafe {
        core::ptr::write_volatile((VGA_BUFFER as *mut u16).add(idx), entry);
    }
}

fn vga_sync_all() {
    if !vga_mirroring() {
        return;
    }
    let buf = buffer();
    unsafe {
        for i in 0..(COLS * ROWS) {
            let entry = core::ptr::read_volatile(buf.add(i));
            core::ptr::write_volatile((VGA_BUFFER as *mut u16).add(i), entry);
        }
    }
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
        fb_redraw_all();
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
        vga_write_cell(idx, entry);
        fb_render_entry(col, row, entry);
    }

    fn scroll_one(&mut self) {
        let buf = buffer();
        unsafe {
            core::ptr::copy(buf.add(COLS), buf, COLS * (ROWS - 1));
        }
        for col in 0..COLS {
            self.put_char_at(b' ', col, ROWS - 1, self.attr);
        }
        fb_redraw_all();
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
        fb_draw_cursor(self.col, self.row, self.attr);
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
            // Form feed: full-screen apps (currently just `hed`) use this
            // as a "clear and home the cursor" signal since userspace has
            // no ioctl/ANSI escape support to address the screen directly.
            '\x0C' => self.clear(),
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

    /// Copies the live VGA text buffer plus cursor position/attribute out
    /// into plain RAM, so a virtual terminal switch (see vt.rs) can stash
    /// exactly what's on screen right now and put it back byte-for-byte
    /// later. `cells` is in the same packed (attr<<8 | char) format the
    /// hardware buffer itself uses, so this is a straight memcpy each way.
    pub fn snapshot(&self, cells: &mut [u16; COLS * ROWS]) -> (usize, usize, u8) {
        let buf = buffer();
        unsafe {
            for i in 0..(COLS * ROWS) {
                cells[i] = core::ptr::read_volatile(buf.add(i));
            }
        }
        (self.col, self.row, self.attr)
    }

    pub fn restore(&mut self, cells: &[u16; COLS * ROWS], col: usize, row: usize, attr: u8) {
        let buf = buffer();
        unsafe {
            for i in 0..(COLS * ROWS) {
                core::ptr::write_volatile(buf.add(i), cells[i]);
            }
        }
        self.col = col;
        self.row = row;
        self.attr = attr;
        fb_redraw_all();
        self.sync_hw_cursor();
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
    fb_redraw_all();
}
