#![no_std]
#![no_main]

extern crate alloc;

mod glyphs;

use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, env, eprintln, fs, sys};
use hxclient::ui::{self, Ui};
use hxclient::{Event, Window, CURSOR_TEXT, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_RELEASE, MOUSE_WHEEL};
use hxvt::{Cell, Color, Key, MouseButton, MouseEvent, MouseMode, Term};
use vellum::hfont::Font;
use vellum::{Area, Painter};

const INITIAL_COLS: usize = 80;
const INITIAL_ROWS: usize = 25;
const PAD: i32 = 10;
const SCROLLBACK: usize = 5000;
const DARK_BG: u32 = 0x131519;
const DARK_FG: u32 = 0xd9dde5;
const DARK_SELECTION: u32 = 0x2f4f86;
const LIGHT_BG: u32 = 0xfbfbfc;
const LIGHT_FG: u32 = 0x24272e;
const LIGHT_SELECTION: u32 = 0xbcd3ff;

const DARK_PALETTE: [u32; 16] = [
    0x1d2027, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xc8ccd4,
    0x5c6370, 0xff7b85, 0xb5e08a, 0xffd68a, 0x82c4ff, 0xdd9cf0, 0x7fd4de, 0xffffff,
];

const LIGHT_PALETTE: [u32; 16] = [
    0x24272e, 0xd1343f, 0x3f8f3e, 0xb07800, 0x2f6fd8, 0xa33bc2, 0x0b8aa3, 0x6b717d,
    0x5c6370, 0xe45649, 0x50a14f, 0x986801, 0x4078f2, 0xa626a4, 0x0184bc, 0x2b2f36,
];

fn light() -> bool {
    ui::theme::is_light()
}

fn term_bg() -> u32 {
    if light() { LIGHT_BG } else { DARK_BG }
}

fn term_fg() -> u32 {
    if light() { LIGHT_FG } else { DARK_FG }
}

fn selection_bg() -> u32 {
    if light() { LIGHT_SELECTION } else { DARK_SELECTION }
}

fn palette() -> &'static [u32; 16] {
    if light() { &LIGHT_PALETTE } else { &DARK_PALETTE }
}

fn resolve(color: Color, default: u32, bold: bool) -> u32 {
    match color {
        Color::Default => default,
        Color::Indexed(n) if bold && n < 8 => palette()[n as usize + 8],
        Color::Indexed(n) => hxvt::palette_rgb(n, palette()),
        Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
    }
}

fn cell_colors(cell: &Cell, reverse_video: bool) -> (u32, u32) {
    let (dfg, dbg) = if reverse_video { (term_bg(), term_fg()) } else { (term_fg(), term_bg()) };
    let bold = cell.flags & hxvt::BOLD != 0;
    let mut fg = resolve(cell.fg, dfg, bold);
    let mut bg = resolve(cell.bg, dbg, false);
    if cell.flags & hxvt::REVERSE != 0 {
        core::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags & hxvt::DIM != 0 {
        fg = vellum::gfx::mix(fg, bg, 128);
    }
    if cell.flags & hxvt::INVISIBLE != 0 {
        fg = bg;
    }
    (fg, bg)
}

fn to_key(code: i32) -> Option<Key> {
    Some(match code {
        -1 => Key::Up,
        -2 => Key::Down,
        -3 => Key::Left,
        -4 => Key::Right,
        -5 => Key::Home,
        -6 => Key::End,
        -7 => Key::Delete,
        -8 => Key::PageUp,
        -9 => Key::PageDown,
        -53 => Key::Insert,
        -52..=-41 => Key::F((-40 - code) as u8),
        8 => Key::Backspace,
        9 => Key::Tab,
        10 | 13 => Key::Enter,
        27 => Key::Escape,
        127 => Key::Backspace,
        1..=26 => Key::Char((b'a' + code as u8 - 1) as char),
        0 => Key::Char(' '),
        28..=31 => Key::Char((b'\\' + (code as u8 - 28)) as char),
        c if (32..0x11_0000).contains(&c) => Key::Char(char::from_u32(c as u32)?),
        _ => return None,
    })
}

struct App {
    window: Window,
    ui: Ui,
    bold: Option<Font>,
    term: Term,
    cell_w: i32,
    cell_h: i32,
    input: u64,
    output: u64,
    child: i64,
    focused: bool,
    blink: bool,
    view: usize,
    selection: Option<((usize, usize), (usize, usize))>,
    selecting: bool,
    clipboard: String,
    mouse_held: Option<MouseButton>,
    last_mouse_cell: (usize, usize),
    last_cursor: (usize, usize),
}

impl App {
    fn cell_at(&self, x: i32, y: i32) -> (usize, usize) {
        let col = ((x - PAD).max(0) / self.cell_w).clamp(0, self.term.cols() as i32 - 1) as usize;
        let row = ((y - PAD).max(0) / self.cell_h).clamp(0, self.term.rows() as i32 - 1) as usize;
        (row, col)
    }

    fn selected(&self, row: usize, col: usize) -> bool {
        match self.selection {
            Some((a, b)) => {
                let (s, e) = if a <= b { (a, b) } else { (b, a) };
                (row, col) >= s && (row, col) <= e
            }
            None => false,
        }
    }

    fn line(&self, view_row: usize) -> Vec<Cell> {
        let back = self.term.scrollback_len();
        let index = back - self.view + view_row;
        if index < back {
            let mut line = self.term.scrollback_line(index).to_vec();
            line.resize(self.term.cols(), Cell::BLANK);
            line
        } else {
            self.term.row(index - back).to_vec()
        }
    }

    fn text_range(&self, a: (usize, usize), b: (usize, usize)) -> String {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let cols = self.term.cols();
        let mut out = String::new();
        for row in start.0..=end.0 {
            let line = self.line(row);
            let from = if row == start.0 { start.1 } else { 0 };
            let to = if row == end.0 { end.1 + 1 } else { cols };
            let text: String = line[from.min(cols)..to.min(cols)].iter().filter(|c| c.flags & hxvt::WIDE_SPACER == 0).map(|c| c.ch).collect();
            out.push_str(text.trim_end());
            if row != end.0 {
                out.push('\n');
            }
        }
        out
    }

    fn draw(&mut self, full: bool) {
        let rows_total = self.term.rows();
        let cursor = self.term.cursor();
        if cursor != self.last_cursor {
            self.term.mark_dirty(self.last_cursor.0);
            self.term.mark_dirty(cursor.0);
            self.last_cursor = cursor;
        }
        let rows: Vec<usize> = (0..rows_total).filter(|r| full || self.view > 0 || self.term.is_dirty(*r)).collect();
        if rows.is_empty() {
            return;
        }
        let lines: Vec<Vec<Cell>> = rows.iter().map(|r| self.line(*r)).collect();
        let (cw, ch) = (self.cell_w, self.cell_h);
        let show_cursor = self.view == 0 && self.term.cursor_visible();
        let focused = self.focused;
        let blink = self.blink || !self.term.modes().cursor_blink;
        let reverse_video = self.term.modes().reverse_video;
        let selection: Vec<Vec<bool>> = rows.iter().map(|r| (0..self.term.cols()).map(|c| self.selected(*r, c)).collect()).collect();
        let (w, h) = (self.window.width() as i32, self.window.height() as i32);
        let back = self.term.scrollback_len();
        let view = self.view;
        let base_bg = if reverse_video { term_fg() } else { term_bg() };
        let mono = &self.ui.mono;
        let bold_font = self.bold.as_ref();
        {
            let buffer = self.window.buffer();
            let mut p = Painter::new(buffer, w, h);
            if full {
                p.fill(Area::new(0, 0, w, h), base_bg);
            }
            for (i, row) in rows.iter().enumerate() {
                let y = PAD + *row as i32 * ch;
                p.fill(Area::new(0, y, w - 8, ch), base_bg);
                if *row == rows_total - 1 {
                    p.fill(Area::new(0, y + ch, w - 8, h - y - ch), base_bg);
                }
                let line = &lines[i];
                for (col, cell) in line.iter().enumerate() {
                    if cell.flags & hxvt::WIDE_SPACER != 0 {
                        continue;
                    }
                    let x = PAD + col as i32 * cw;
                    let span = if cell.flags & hxvt::WIDE != 0 { 2 } else { 1 };
                    let (mut fg, mut bg) = cell_colors(cell, reverse_video);
                    if selection[i][col] {
                        bg = selection_bg();
                        fg = if light() { term_fg() } else { 0xffffff };
                    }
                    let is_cursor = show_cursor && (*row, col) == cursor;
                    if is_cursor && focused && blink {
                        core::mem::swap(&mut fg, &mut bg);
                        if fg == bg {
                            bg = term_fg();
                            fg = term_bg();
                        }
                    }
                    if bg != base_bg {
                        p.fill(Area::new(x, y, cw * span, ch), bg);
                    }
                    if is_cursor && !focused {
                        p.rounded_border(Area::new(x, y, cw * span, ch), 1, term_fg(), 200);
                    }
                    if cell.ch != ' ' && cell.flags & hxvt::INVISIBLE == 0 {
                        if !glyphs::draw(&mut p, x, y, cw * span, ch, cell.ch, fg) {
                            let font = if cell.flags & hxvt::BOLD != 0 { bold_font.unwrap_or(mono) } else { mono };
                            let mut buf = [0u8; 4];
                            let text = cell.ch.encode_utf8(&mut buf);
                            let gx = x + (cw * span - font.advance(cell.ch)).max(0) / 2;
                            font.draw(&mut p, gx, y + 1, text, fg, 255);
                        }
                    }
                    if cell.flags & hxvt::UNDERLINE != 0 {
                        p.fill(Area::new(x, y + mono.ascent + 2, cw * span, 1), fg);
                    }
                    if cell.flags & hxvt::STRIKE != 0 {
                        p.fill(Area::new(x, y + ch / 2, cw * span, 1), fg);
                    }
                }
            }
            let track = Area::new(w - 7, PAD, 4, rows_total as i32 * ch);
            p.fill(Area::new(w - 8, 0, 8, h), base_bg);
            ui::scrollbar(&mut p, track, (back - view) as i32, (back + rows_total) as i32, rows_total as i32);
        }
        if full || rows.len() == rows_total {
            self.window.present();
        } else {
            let mut start = rows[0];
            let mut prev = rows[0];
            for &row in rows.iter().skip(1).chain(core::iter::once(&usize::MAX)) {
                if row != prev + 1 {
                    let count = prev + 1 - start;
                    self.window.present_rect(0, (PAD + start as i32 * ch) as u32, w as u32, (count as i32 * ch) as u32);
                    start = row;
                }
                prev = row;
            }
            self.window.present_rect((w - 8) as u32, 0, 8, h as u32);
        }
        self.term.clear_dirty();
    }

    fn write_input(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            sys::write(self.input, bytes);
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if self.view != 0 {
            self.view = 0;
            self.term.mark_all_dirty();
        }
        self.write_input(bytes);
    }

    fn key(&mut self, code: i32, mods: u8) {
        let shift = mods & hxvt::MOD_SHIFT != 0;
        let ctrl = mods & hxvt::MOD_CTRL != 0;
        if shift && (code == -8 || code == -9) {
            let page = self.term.rows() as i32 - 2;
            self.scroll(if code == -8 { page } else { -page });
            return;
        }
        if ctrl && shift && (code == 3 || code == b'C' as i32 || code == b'c' as i32) {
            if let Some((a, b)) = self.selection {
                self.clipboard = self.text_range(a, b);
            }
            return;
        }
        if ctrl && shift && (code == 22 || code == b'V' as i32 || code == b'v' as i32) {
            self.paste();
            return;
        }
        let Some(key) = to_key(code) else {
            return;
        };
        let mut mods = mods;
        if let Key::Char(c) = key {
            if code < 32 && c.is_ascii_lowercase() {
                mods |= hxvt::MOD_CTRL;
            }
            if !c.is_ascii_alphabetic() || mods & hxvt::MOD_CTRL == 0 {
                mods &= !hxvt::MOD_SHIFT;
            }
        }
        let bytes = hxvt::encode_key(key, mods, self.term.modes());
        self.send(&bytes);
    }

    fn paste(&mut self) {
        let text = self.clipboard.clone();
        if !text.is_empty() {
            let bytes = hxvt::encode_paste(self.term.modes(), &text);
            self.send(&bytes);
        }
    }

    fn scroll(&mut self, lines: i32) {
        let max = self.term.scrollback_len() as i32;
        let view = (self.view as i32 + lines).clamp(0, max) as usize;
        if view != self.view {
            self.view = view;
            self.selection = None;
            self.term.mark_all_dirty();
        }
    }

    fn resize(&mut self) {
        let (w, h) = (self.window.width() as i32, self.window.height() as i32);
        let cols = ((w - PAD * 2 - 8) / self.cell_w).max(10) as usize;
        let rows = ((h - PAD * 2) / self.cell_h).max(3) as usize;
        if cols != self.term.cols() || rows != self.term.rows() {
            self.term.resize(cols, rows);
            self.view = self.view.min(self.term.scrollback_len());
        }
        self.selection = None;
        sys::set_terminal(self.input, cols as u16, rows as u16);
        sys::set_terminal(self.output, cols as u16, rows as u16);
    }

    fn pump(&mut self) -> bool {
        let mut got = false;
        let mut buf = [0u8; 16384];
        for _ in 0..64 {
            let available = sys::fstat_size(self.output);
            if available <= 0 {
                break;
            }
            let want = (available as usize).min(buf.len());
            let n = sys::read(self.output, &mut buf[..want]);
            if n <= 0 {
                break;
            }
            self.term.feed(&buf[..n as usize]);
            got = true;
        }
        if got {
            let responses = self.term.take_responses();
            self.write_input(&responses);
            if let Some(title) = self.term.take_title() {
                let title = if title.is_empty() { String::from("Terminal") } else { title };
                self.window.set_title(&title);
            }
            let scrolled = self.term.take_scrolled();
            if self.term.take_scrollback_cleared() {
                self.view = 0;
            } else if self.view > 0 && scrolled > 0 {
                self.view = (self.view + scrolled).min(self.term.scrollback_len());
            }
            if scrolled > 0 && self.selection.is_some() {
                self.selection = None;
                self.term.mark_all_dirty();
            }
        }
        got
    }

    fn report_mouse(&mut self, event: MouseEvent, x: i32, y: i32) -> bool {
        let modes = *self.term.modes();
        if modes.mouse == MouseMode::Off {
            return false;
        }
        let (row, col) = self.cell_at(x, y);
        if matches!(event, MouseEvent::Move(_)) && (row, col) == self.last_mouse_cell {
            return true;
        }
        self.last_mouse_cell = (row, col);
        if let Some(bytes) = hxvt::encode_mouse(&modes, event, col, row, 0) {
            self.send(&bytes);
        }
        true
    }

    fn mouse(&mut self, x: i32, y: i32, kind: u32, wheel: i32, buttons: u32) {
        let button = |bit: i32| match bit {
            2 => MouseButton::Right,
            4 => MouseButton::Middle,
            _ => MouseButton::Left,
        };
        match kind {
            MOUSE_WHEEL => {
                let event = if wheel < 0 { MouseEvent::WheelUp } else { MouseEvent::WheelDown };
                if self.report_mouse(event, x, y) {
                    return;
                }
                let modes = *self.term.modes();
                if modes.alt_screen && modes.alternate_scroll {
                    let key = if wheel < 0 { Key::Up } else { Key::Down };
                    for _ in 0..wheel.unsigned_abs().max(1) * 3 {
                        let bytes = hxvt::encode_key(key, 0, &modes);
                        self.send(&bytes);
                    }
                    return;
                }
                self.scroll(-wheel * 3);
            }
            MOUSE_PRESS => {
                let b = button(wheel);
                if self.report_mouse(MouseEvent::Press(b), x, y) {
                    self.mouse_held = Some(b);
                    return;
                }
                if b == MouseButton::Left {
                    let cell = self.cell_at(x, y);
                    self.selection = Some((cell, cell));
                    self.selecting = true;
                    self.term.mark_all_dirty();
                } else {
                    self.paste();
                }
            }
            MOUSE_MOVE => {
                if self.term.modes().mouse != MouseMode::Off && !self.selecting {
                    let held = if buttons & 1 != 0 { Some(MouseButton::Left) } else if buttons & 2 != 0 { Some(MouseButton::Right) } else if buttons & 4 != 0 { Some(MouseButton::Middle) } else { None };
                    self.report_mouse(MouseEvent::Move(held.or(self.mouse_held.filter(|_| buttons != 0))), x, y);
                    return;
                }
                if self.selecting && buttons & 1 != 0 {
                    let cell = self.cell_at(x, y);
                    if let Some((a, old)) = self.selection {
                        if old != cell {
                            self.selection = Some((a, cell));
                            self.term.mark_all_dirty();
                        }
                    }
                }
            }
            MOUSE_RELEASE => {
                if self.selecting {
                    self.selecting = false;
                    if let Some((a, b)) = self.selection {
                        if a == b {
                            self.selection = None;
                        } else {
                            self.clipboard = self.text_range(a, b);
                        }
                        self.term.mark_all_dirty();
                    }
                    return;
                }
                let b = button(wheel);
                if self.report_mouse(MouseEvent::Release(b), x, y) {
                    self.mouse_held = None;
                }
            }
            MOUSE_LEAVE => {}
            _ => {}
        }
    }
}

fn open_terminal() -> Option<(u64, u64, u64, u64)> {
    let master = sys::open("/dev/ptmx");
    if master >= 0 {
        let master = master as u64;
        let mut index = 0u32;
        let r = unsafe { sys::syscall3(16, master, 0x8004_5430, &mut index as *mut u32 as u64) };
        let slave = if r >= 0 { sys::open(&alloc::format!("/dev/pts/{}", index)) } else { -1 };
        if slave >= 0 {
            sys::set_terminal(master, INITIAL_COLS as u16, INITIAL_ROWS as u16);
            return Some((slave as u64, master, master, slave as u64));
        }
        sys::close(master);
    }
    let ((in_r, in_w), (out_r, out_w)) = (sys::pipe().ok()?, sys::pipe().ok()?);
    sys::set_terminal(in_w, INITIAL_COLS as u16, INITIAL_ROWS as u16);
    sys::set_terminal(out_r, INITIAL_COLS as u16, INITIAL_ROWS as u16);
    Some((in_r, in_w, out_r, out_w))
}

fn child_env() -> Vec<String> {
    let mut vars: Vec<String> = env::vars()
        .iter()
        .filter(|(k, _)| k != "TERM" && k != "COLORTERM" && k != "COLUMNS" && k != "LINES")
        .map(|(k, v)| alloc::format!("{}={}", k, v))
        .collect();
    vars.push(String::from("TERM=xterm-256color"));
    vars.push(String::from("COLORTERM=truecolor"));
    vars.push(String::from("TERM_PROGRAM=hxterm"));
    vars
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&[]);
    let bold = fs::read("/usr/share/nook/fonts/mono-bold-14.hfnt").and_then(|b| Font::parse(&b));
    let cell_w = ui.mono.advance('M').max(6);
    let cell_h = ui.mono.height() + 2;
    let width = (INITIAL_COLS as i32 * cell_w + PAD * 2 + 8) as u32;
    let height = (INITIAL_ROWS as i32 * cell_h + PAD * 2) as u32;
    let mut window = match Window::open("Terminal", width, height) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxterm: {}", e);
            return 1;
        }
    };
    window.set_icon("terminal");
    window.set_cursor(CURSOR_TEXT);
    window.set_min_size((20 * cell_w + PAD * 2 + 8) as u32, (5 * cell_h + PAD * 2) as u32);

    let Some((in_r, in_w, out_r, out_w)) = open_terminal() else {
        eprintln!("hxterm: cannot create a terminal");
        return 1;
    };

    let conf = fs::read_to_string("/etc/hamix/login.conf").unwrap_or_default();
    let shell = conf
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "shell")
        .map(|(_, v)| String::from(v.trim()))
        .unwrap_or_else(|| String::from("/usr/bin/hsh"));
    let args: Vec<String> = env::args().iter().skip(1).cloned().collect();
    let cwd = sys::getcwd();
    if cwd == "/" {
        sys::chdir(&ui::home_dir());
    }
    let (program, argv): (String, Vec<String>) = if args.is_empty() { (shell, Vec::new()) } else { (String::from("/usr/bin/hsh"), alloc::vec![String::from("-c"), args.join(" ")]) };
    let vars = child_env();
    let child = sys::spawn_io_env(&program, &argv, Some(&vars), 0, Some(in_r), Some(out_w), Some(out_w));
    sys::close(in_r);
    if out_w != in_r {
        sys::close(out_w);
    }
    if child < 0 {
        eprintln!("hxterm: cannot start {}: {}", program, sys::error_name(child));
        return 1;
    }

    let mut app = App {
        window,
        ui,
        bold,
        term: Term::new(INITIAL_COLS, INITIAL_ROWS, SCROLLBACK),
        cell_w,
        cell_h,
        input: in_w,
        output: out_r,
        child,
        focused: true,
        blink: true,
        view: 0,
        selection: None,
        selecting: false,
        clipboard: String::new(),
        mouse_held: None,
        last_mouse_cell: (usize::MAX, usize::MAX),
        last_cursor: (0, 0),
    };
    app.draw(true);
    let mut last_blink = sys::uptime_ms();
    let mut exited = false;

    loop {
        let ready = sys::wait_event(sys::EVENT_MESSAGE | sys::EVENT_PIPE, 250);
        let mut redraw_full = false;
        if (ready & sys::EVENT_PIPE != 0 || sys::fstat_size(app.output) > 0) && app.pump() {
            app.blink = true;
            last_blink = sys::uptime_ms();
        }
        while let Some(event) = app.window.poll_event() {
            match event {
                Event::Close { .. } => {
                    sys::kill(app.child);
                    return 0;
                }
                Event::Key { code, mods, .. } => {
                    if exited {
                        return 0;
                    }
                    if app.selection.take().is_some() {
                        app.term.mark_all_dirty();
                    }
                    app.key(code, mods as u8);
                }
                Event::Resize { .. } => {
                    app.resize();
                    redraw_full = true;
                }
                Event::Theme { .. } => {
                    app.term.mark_all_dirty();
                    redraw_full = true;
                }
                Event::Focus { focused, .. } => {
                    app.focused = focused;
                    let (row, _) = app.term.cursor();
                    app.term.mark_dirty(row);
                    if let Some(bytes) = hxvt::encode_focus(app.term.modes(), focused) {
                        app.write_input(bytes);
                    }
                }
                Event::Mouse { x, y, kind, wheel, buttons, .. } => app.mouse(x, y, kind, wheel, buttons),
                _ => {}
            }
        }
        if !exited && !sys::proc_alive(app.child) && sys::fstat_size(app.output) <= 0 {
            sys::waitpid(app.child, true);
            sys::close(app.output);
            exited = true;
            app.view = 0;
            app.term.feed(b"\x1b[0m\x1b[?25h\x1b[?1049l\r\n\x1b[90m[process exited -- press any key to close]\x1b[0m");
            redraw_full = true;
        }
        let now = sys::uptime_ms();
        if now.saturating_sub(last_blink) >= 530 {
            last_blink = now;
            app.blink = !app.blink;
            let (row, _) = app.term.cursor();
            app.term.mark_dirty(row);
        }
        app.draw(redraw_full);
    }
}

entry!(main);
