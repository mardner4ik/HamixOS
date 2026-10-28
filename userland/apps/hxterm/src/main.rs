#![no_std]
#![no_main]

extern crate alloc;

mod glyphs;

use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{env, fs, sys};
use hxtoolkit::{canvas, App, CanvasEvent, Context, Element, KeyPress, MenuItem, MouseButton as UiButton, Pixmap, Settings};
use hxvt::{Cell, Color, Key, MouseButton, MouseEvent, MouseMode, Term};
use vellum::hfont::Font;
use vellum::{Area, Painter};

const INITIAL_COLS: usize = 80;
const INITIAL_ROWS: usize = 25;
const PAD: i32 = 10;
const BAR: i32 = 8;
const SCROLLBACK: usize = 5000;
const SHELL: &str = "/usr/bin/bash";

struct Colors {
    bg: u32,
    fg: u32,
    selection: u32,
    selected_fg: u32,
    palette: [u32; 16],
}

const DARK: Colors = Colors {
    bg: 0x131519,
    fg: 0xd9dde5,
    selection: 0x2f4f86,
    selected_fg: 0xffffff,
    palette: [0x1d2027, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xc8ccd4, 0x5c6370, 0xff7b85, 0xb5e08a, 0xffd68a, 0x82c4ff, 0xdd9cf0, 0x7fd4de, 0xffffff],
};

const LIGHT: Colors = Colors {
    bg: 0xfbfbfc,
    fg: 0x24272e,
    selection: 0xbcd3ff,
    selected_fg: 0x24272e,
    palette: [0x24272e, 0xd1343f, 0x3f8f3e, 0xb07800, 0x2f6fd8, 0xa33bc2, 0x0b8aa3, 0x6b717d, 0x5c6370, 0xe45649, 0x50a14f, 0x986801, 0x4078f2, 0xa626a4, 0x0184bc, 0x2b2f36],
};

#[derive(Clone)]
enum Msg {
    Output,
    Blink,
    Key(KeyPress),
    Canvas(CanvasEvent),
    Copy,
    Paste,
    Pasted(Option<String>),
    SelectAll,
    Clear,
    NewWindow,
}

struct Terminal {
    term: Term,
    mono: Font,
    bold: Option<Font>,
    cell_w: i32,
    cell_h: i32,
    input: u64,
    output: u64,
    child: i64,
    exited: Option<i64>,
    pixmap: Pixmap,
    dark: bool,
    blink: bool,
    view: usize,
    selection: Option<((usize, usize), (usize, usize))>,
    selecting: bool,
    mouse_held: Option<MouseButton>,
    last_mouse_cell: (usize, usize),
    title: String,
    drawn: Vec<Option<(char, u32, u32, u16)>>,
    drawn_key: (i32, i32, usize, usize, usize, usize, bool, u32),
}

fn to_vt(press: &KeyPress) -> Option<(Key, u8)> {
    let key = match press.key {
        hxtoolkit::Key::Char(c) => Key::Char(c),
        hxtoolkit::Key::Enter => Key::Enter,
        hxtoolkit::Key::Escape => Key::Escape,
        hxtoolkit::Key::Backspace => Key::Backspace,
        hxtoolkit::Key::Delete => Key::Delete,
        hxtoolkit::Key::Tab => Key::Tab,
        hxtoolkit::Key::Up => Key::Up,
        hxtoolkit::Key::Down => Key::Down,
        hxtoolkit::Key::Left => Key::Left,
        hxtoolkit::Key::Right => Key::Right,
        hxtoolkit::Key::Home => Key::Home,
        hxtoolkit::Key::End => Key::End,
        hxtoolkit::Key::PageUp => Key::PageUp,
        hxtoolkit::Key::PageDown => Key::PageDown,
        hxtoolkit::Key::Insert => Key::Insert,
        hxtoolkit::Key::F(n) => Key::F(n),
    };
    let mut mods = 0u8;
    if press.mods.shift {
        mods |= hxvt::MOD_SHIFT;
    }
    if press.mods.alt {
        mods |= hxvt::MOD_ALT;
    }
    if press.mods.ctrl {
        mods |= hxvt::MOD_CTRL;
    }
    if let Key::Char(c) = key {
        if !c.is_ascii_alphabetic() || mods & hxvt::MOD_CTRL == 0 {
            mods &= !hxvt::MOD_SHIFT;
        }
    }
    Some((key, mods))
}

impl Terminal {
    fn colors(&self) -> &'static Colors {
        if self.dark { &DARK } else { &LIGHT }
    }

    fn resolve(&self, color: Color, default: u32, bold: bool) -> u32 {
        let palette = &self.colors().palette;
        match color {
            Color::Default => default,
            Color::Indexed(n) if bold && n < 8 => palette[n as usize + 8],
            Color::Indexed(n) => hxvt::palette_rgb(n, palette),
            Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
        }
    }

    fn cell_colors(&self, cell: &Cell, reverse_video: bool) -> (u32, u32) {
        let c = self.colors();
        let (dfg, dbg) = if reverse_video { (c.bg, c.fg) } else { (c.fg, c.bg) };
        let bold = cell.flags & hxvt::BOLD != 0;
        let mut fg = self.resolve(cell.fg, dfg, bold);
        let mut bg = self.resolve(cell.bg, dbg, false);
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

    fn render(&mut self) {
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        if w <= 0 || h <= 0 {
            return;
        }
        let colors = self.colors();
        let rows_total = self.term.rows();
        let cursor = self.term.cursor();
        let lines: Vec<Vec<Cell>> = (0..rows_total).map(|r| self.line(r)).collect();
        let (cw, ch) = (self.cell_w, self.cell_h);
        let show_cursor = self.view == 0 && self.term.cursor_visible() && self.exited.is_none();
        let blink = self.blink || !self.term.modes().cursor_blink;
        let reverse_video = self.term.modes().reverse_video;
        let base_bg = if reverse_video { colors.fg } else { colors.bg };
        let back = self.term.scrollback_len();
        let view = self.view;
        let selection: Vec<Vec<bool>> = (0..rows_total).map(|r| (0..self.term.cols()).map(|c| self.selected(r, c)).collect()).collect();
        let cell_colors: Vec<Vec<(u32, u32)>> = lines.iter().map(|l| l.iter().map(|c| self.cell_colors(c, reverse_video)).collect()).collect();
        let mono = &self.mono;
        let bold_font = self.bold.as_ref();
        let cols = self.term.cols();
        let key = (w, h, rows_total, cols, back, view, reverse_video, base_bg);
        let full = self.drawn_key != key || self.drawn.len() != rows_total * cols;
        if full {
            self.drawn_key = key;
            self.drawn = alloc::vec![None; rows_total * cols];
        }
        let drawn = &mut self.drawn;
        let pixels = self.pixmap.pixels_mut();
        {
            let mut p = Painter::layer(pixels, Area::new(0, 0, w, h));
            if full {
                p.fill(Area::new(0, 0, w, h), base_bg);
            }
            for (row, line) in lines.iter().enumerate() {
                let y = PAD + row as i32 * ch;
                for (col, cell) in line.iter().enumerate() {
                    let slot = row * cols + col;
                    if cell.flags & hxvt::WIDE_SPACER != 0 {
                        if let Some(d) = drawn.get_mut(slot) {
                            *d = Some(('\u{0}', 0, 0, hxvt::WIDE_SPACER));
                        }
                        continue;
                    }
                    let x = PAD + col as i32 * cw;
                    let span = if cell.flags & hxvt::WIDE != 0 { 2 } else { 1 };
                    let (mut fg, mut bg) = cell_colors[row][col];
                    if selection[row][col] {
                        bg = colors.selection;
                        fg = colors.selected_fg;
                    }
                    if show_cursor && blink && (row, col) == cursor {
                        core::mem::swap(&mut fg, &mut bg);
                        if fg == bg {
                            bg = colors.fg;
                            fg = colors.bg;
                        }
                    }
                    let state = Some((cell.ch, fg, bg, cell.flags));
                    if !full && drawn.get(slot).copied().flatten() == state {
                        continue;
                    }
                    if let Some(d) = drawn.get_mut(slot) {
                        *d = state;
                    }
                    if bg != base_bg || !full {
                        p.fill(Area::new(x, y, cw * span, ch), bg);
                    }
                    if cell.ch != ' ' && cell.flags & hxvt::INVISIBLE == 0 && !glyphs::draw(&mut p, x, y, cw * span, ch, cell.ch, fg) {
                        let font = if cell.flags & hxvt::BOLD != 0 { bold_font.unwrap_or(mono) } else { mono };
                        let mut buf = [0u8; 4];
                        let text = cell.ch.encode_utf8(&mut buf);
                        let gx = x + (cw * span - font.advance(cell.ch)).max(0) / 2;
                        font.draw(&mut p, gx, y + 1, text, fg, 255);
                    }
                    if cell.flags & hxvt::UNDERLINE != 0 {
                        p.fill(Area::new(x, y + mono.ascent + 2, cw * span, 1), fg);
                    }
                    if cell.flags & hxvt::STRIKE != 0 {
                        p.fill(Area::new(x, y + ch / 2, cw * span, 1), fg);
                    }
                }
            }
            let total = back + rows_total;
            if back > 0 && full {
                let track_h = rows_total as i32 * ch;
                let thumb_h = (track_h * rows_total as i32 / total as i32).max(24);
                let top = (track_h - thumb_h) * (back - view) as i32 / back.max(1) as i32;
                p.rounded(Area::new(w - BAR + 2, PAD + top, 4, thumb_h), 2, colors.fg, if view > 0 { 140 } else { 60 });
            }
        }
    }

    fn write_input(&mut self, bytes: &[u8]) {
        let mut done = 0usize;
        let mut attempts = 0;
        while done < bytes.len() && attempts < 200 {
            let n = sys::write(self.input, &bytes[done..]);
            if n > 0 {
                done += n as usize;
            } else if n == sys::EAGAIN {
                attempts += 1;
                sys::sleep_ms(5);
            } else {
                break;
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.view = 0;
        self.write_input(bytes);
    }

    fn pump(&mut self) -> bool {
        let mut got = false;
        let mut buf = [0u8; 16384];
        for _ in 0..64 {
            let n = sys::read(self.output, &mut buf);
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
                self.title = if title.is_empty() { String::from("Terminal") } else { title };
            }
            let scrolled = self.term.take_scrolled();
            if self.term.take_scrollback_cleared() {
                self.view = 0;
            } else if self.view > 0 && scrolled > 0 {
                self.view = (self.view + scrolled).min(self.term.scrollback_len());
            }
            if scrolled > 0 {
                self.selection = None;
            }
            self.blink = true;
        }
        got
    }

    fn check_exit(&mut self) -> Option<i64> {
        if self.exited.is_some() || sys::proc_alive(self.child) {
            return None;
        }
        while self.pump() {}
        let status = sys::waitpid(self.child, true);
        sys::close(self.output);
        self.exited = Some(status);
        if status != 0 {
            self.view = 0;
            self.term.feed(alloc::format!("\x1b[0m\x1b[?25h\x1b[?1049l\r\n\x1b[90m[the program exited with status {}]\x1b[0m", status).as_bytes());
        }
        Some(status)
    }

    fn scroll(&mut self, lines: i32) {
        let max = self.term.scrollback_len() as i32;
        self.view = (self.view as i32 + lines).clamp(0, max) as usize;
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.pixmap.resize(width.max(1), height.max(1));
        let cols = ((width as i32 - PAD * 2 - BAR) / self.cell_w).max(10) as usize;
        let rows = ((height as i32 - PAD * 2) / self.cell_h).max(3) as usize;
        if cols != self.term.cols() || rows != self.term.rows() {
            self.term.resize(cols, rows);
            self.view = self.view.min(self.term.scrollback_len());
            self.selection = None;
            sys::set_terminal(self.input, cols as u16, rows as u16);
        }
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

    fn context_menu(&self) -> Vec<MenuItem<Msg>> {
        let mut copy = MenuItem::new("Copy", Msg::Copy).shortcut("Ctrl+Shift+C");
        if self.selection.is_none() {
            copy = copy.disabled();
        }
        alloc::vec![
            copy,
            MenuItem::new("Paste", Msg::Paste).shortcut("Ctrl+Shift+V"),
            MenuItem::new("Select all", Msg::SelectAll).shortcut("Ctrl+Shift+A"),
            MenuItem::separator(),
            MenuItem::new("Clear scrollback", Msg::Clear),
            MenuItem::new("New window", Msg::NewWindow).shortcut("Ctrl+Shift+N"),
        ]
    }

    fn canvas(&mut self, event: CanvasEvent, cx: &mut Context<Msg>) {
        let vt_button = |b: UiButton| match b {
            UiButton::Left => MouseButton::Left,
            UiButton::Right => MouseButton::Right,
            UiButton::Middle => MouseButton::Middle,
        };
        match event {
            CanvasEvent::Resize { width, height } => self.resize(width, height),
            CanvasEvent::Wheel { x, y, delta } => {
                let event = if delta < 0 { MouseEvent::WheelUp } else { MouseEvent::WheelDown };
                if self.report_mouse(event, x, y) {
                    return;
                }
                let modes = *self.term.modes();
                if modes.alt_screen && modes.alternate_scroll {
                    let key = if delta < 0 { Key::Up } else { Key::Down };
                    for _ in 0..3 {
                        let bytes = hxvt::encode_key(key, 0, &modes);
                        self.send(&bytes);
                    }
                    return;
                }
                self.scroll(-delta * 3);
            }
            CanvasEvent::Press { x, y, button } => {
                let b = vt_button(button);
                if self.report_mouse(MouseEvent::Press(b), x, y) {
                    self.mouse_held = Some(b);
                    return;
                }
                match button {
                    UiButton::Left => {
                        let cell = self.cell_at(x, y);
                        self.selection = Some((cell, cell));
                        self.selecting = true;
                    }
                    UiButton::Right => cx.show_menu(self.context_menu()),
                    UiButton::Middle => cx.paste(Msg::Pasted),
                }
            }
            CanvasEvent::Move { x, y } => {
                if self.selecting {
                    let cell = self.cell_at(x, y);
                    if let Some((a, _)) = self.selection {
                        self.selection = Some((a, cell));
                    }
                } else if self.term.modes().mouse != MouseMode::Off {
                    let held = self.mouse_held;
                    self.report_mouse(MouseEvent::Move(held), x, y);
                }
            }
            CanvasEvent::Release { x, y, button } => {
                if self.selecting {
                    self.selecting = false;
                    if let Some((a, b)) = self.selection {
                        if a == b {
                            self.selection = None;
                        }
                    }
                    return;
                }
                if self.report_mouse(MouseEvent::Release(vt_button(button)), x, y) {
                    self.mouse_held = None;
                }
            }
        }
    }

    fn key(&mut self, press: KeyPress, cx: &mut Context<Msg>) {
        if self.exited.is_some() {
            cx.quit();
            return;
        }
        let ctrl_shift = press.mods.ctrl && press.mods.shift;
        match press.key {
            hxtoolkit::Key::Char(c) if ctrl_shift && c.eq_ignore_ascii_case(&'c') => return self.update(Msg::Copy, cx),
            hxtoolkit::Key::Char(c) if ctrl_shift && c.eq_ignore_ascii_case(&'v') => return self.update(Msg::Paste, cx),
            hxtoolkit::Key::Char(c) if ctrl_shift && c.eq_ignore_ascii_case(&'a') => return self.update(Msg::SelectAll, cx),
            hxtoolkit::Key::Char(c) if ctrl_shift && c.eq_ignore_ascii_case(&'n') => return self.update(Msg::NewWindow, cx),
            hxtoolkit::Key::PageUp if press.mods.shift => {
                self.scroll(self.term.rows() as i32 - 2);
                return;
            }
            hxtoolkit::Key::PageDown if press.mods.shift => {
                self.scroll(-(self.term.rows() as i32 - 2));
                return;
            }
            _ => {}
        }
        self.selection = None;
        if let Some((key, mods)) = to_vt(&press) {
            let bytes = hxvt::encode_key(key, mods, self.term.modes());
            self.send(&bytes);
        }
    }
}

impl App for Terminal {
    type Message = Msg;

    fn title(&self) -> String {
        self.title.clone()
    }

    fn view(&self) -> Element<Msg> {
        canvas(self.pixmap.clone()).fill().on_event(|e| Some(Msg::Canvas(e))).into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Output => {
                self.pump();
                if let Some(status) = self.check_exit() {
                    if status == 0 {
                        cx.quit();
                        return;
                    }
                }
            }
            Msg::Blink => {
                self.blink = !self.blink;
                if let Some(0) = self.check_exit() {
                    cx.quit();
                    return;
                }
            }
            Msg::Key(press) => self.key(press, cx),
            Msg::Canvas(event) => self.canvas(event, cx),
            Msg::Copy => {
                if let Some((a, b)) = self.selection {
                    cx.copy(self.text_range(a, b));
                }
            }
            Msg::Paste => cx.paste(Msg::Pasted),
            Msg::Pasted(text) => {
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    let bytes = hxvt::encode_paste(self.term.modes(), &text);
                    self.send(&bytes);
                }
            }
            Msg::SelectAll => {
                let rows = self.term.rows();
                self.selection = Some(((0, 0), (rows - 1, self.term.cols() - 1)));
            }
            Msg::Clear => {
                self.send(b"\x0c");
                self.term.feed(b"\x1b[3J");
                self.view = 0;
            }
            Msg::NewWindow => {
                sys::spawn("/usr/bin/hxterm", &[] as &[&str], sys::SPAWN_DETACH);
            }
        }
        self.render();
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        Some(Msg::Key(*press))
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        Some((530, Msg::Blink))
    }

    fn watch(&self) -> Vec<(i32, Msg)> {
        if self.exited.is_some() { Vec::new() } else { alloc::vec![(self.output as i32, Msg::Output)] }
    }

    fn appearance(&mut self, dark: bool) {
        self.dark = dark;
        self.render();
    }

    fn close_requested(&mut self, _cx: &mut Context<Msg>) -> bool {
        if self.exited.is_none() {
            sys::kill(self.child);
        }
        true
    }
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

fn run() -> i32 {
    let Some(mono) = fs::read("/usr/share/nook/fonts/mono-14.hfnt").and_then(|b| Font::parse(&b)) else {
        hxstd::eprintln!("hxterm: the terminal font is missing");
        return 1;
    };
    let bold = fs::read("/usr/share/nook/fonts/mono-bold-14.hfnt").and_then(|b| Font::parse(&b));
    let cell_w = mono.advance('M').max(6);
    let cell_h = mono.height() + 2;
    let Some((master, slave)) = sys::open_pty() else {
        hxstd::eprintln!("hxterm: cannot create a terminal");
        return 1;
    };
    sys::set_nonblocking(master);
    sys::set_terminal(master, INITIAL_COLS as u16, INITIAL_ROWS as u16);
    let shell = fs::read_to_string("/etc/hamix/login.conf")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == "shell")
        .map(|(_, v)| String::from(v.trim()))
        .filter(|s| fs::exists(s))
        .unwrap_or_else(|| String::from(SHELL));
    let args: Vec<String> = env::args().iter().skip(1).cloned().collect();
    if sys::getcwd() == "/" {
        if let Some(home) = env::var("HOME") {
            sys::chdir(home);
        }
    }
    let (program, argv): (String, Vec<String>) = if args.is_empty() { (shell, Vec::new()) } else { (String::from(SHELL), alloc::vec![String::from("-c"), args.join(" ")]) };
    let child = sys::spawn_io_env(&program, &argv, Some(&child_env()), 0, Some(slave), Some(slave), Some(slave));
    sys::close(slave);
    if child < 0 {
        hxstd::eprintln!("hxterm: cannot start {}: {}", program, sys::error_name(child));
        return 1;
    }
    let width = (INITIAL_COLS as i32 * cell_w + PAD * 2 + BAR) as u32;
    let height = (INITIAL_ROWS as i32 * cell_h + PAD * 2) as u32;
    let terminal = Terminal {
        term: Term::new(INITIAL_COLS, INITIAL_ROWS, SCROLLBACK),
        mono,
        bold,
        cell_w,
        cell_h,
        input: master,
        output: master,
        child,
        exited: None,
        pixmap: Pixmap::new(width, height),
        dark: true,
        blink: true,
        view: 0,
        selection: None,
        selecting: false,
        mouse_held: None,
        last_mouse_cell: (usize::MAX, usize::MAX),
        title: String::from("Terminal"),
        drawn: Vec::new(),
        drawn_key: (0, 0, 0, 0, 0, 0, false, 0),
    };
    let min = ((20 * cell_w + PAD * 2 + BAR) as u32, (5 * cell_h + PAD * 2) as u32);
    hxtoolkit::run(Settings::new("terminal").icon("terminal").size(width, height).min_size(min.0, min.1), terminal)
}

hxstd::entry!(run);
