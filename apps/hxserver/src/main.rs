#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::sys::{self, Key, MouseState, MOUSE_LEFT, MOUSE_RIGHT};
use hamix_std::{entry, println};
use vellum::font::{draw_text, text_width, GLYPH_H, GLYPH_W};
use vellum::{Canvas, Color, Rect};

const WALLPAPER: &str = "/usr/share/wallpapers/nook_default.png";
const PANEL_HEIGHT: u32 = 26;
const TITLE_HEIGHT: u32 = 22;
const MENU_WIDTH: u32 = 190;
const MENU_ITEM_HEIGHT: u32 = 24;

const PANEL_BG: Color = Color::rgb(0x1b, 0x20, 0x2b);
const PANEL_FG: Color = Color::rgb(0xd8, 0xde, 0xe9);
const ACCENT: Color = Color::rgb(0x5e, 0x94, 0xd6);
const WINDOW_BG: Color = Color::rgb(0x24, 0x29, 0x33);
const WINDOW_FG: Color = Color::rgb(0xc6, 0xcd, 0xda);
const TITLE_ACTIVE: Color = Color::rgb(0x33, 0x5a, 0x8c);
const TITLE_IDLE: Color = Color::rgb(0x2c, 0x32, 0x3e);
const SHADOW: Color = Color::rgb(0x10, 0x12, 0x18);
const CLOSE_HOT: Color = Color::rgb(0xc4, 0x5b, 0x5b);

struct Surface {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

impl Surface {
    fn new(width: u32, height: u32) -> Self {
        Self { pixels: alloc::vec![0u32; (width * height) as usize], width, height }
    }

    fn copy_from(&mut self, other: &Surface) {
        self.pixels.copy_from_slice(&other.pixels);
    }

    fn blit_to_framebuffer(&self, addr: u64, pitch: u32, bytes_per_pixel: u32) {
        for y in 0..self.height {
            let mut target = (addr as usize + (y * pitch) as usize) as *mut u8;
            let row = &self.pixels[(y * self.width) as usize..((y + 1) * self.width) as usize];
            for value in row {
                unsafe {
                    match bytes_per_pixel {
                        4 => core::ptr::write_volatile(target as *mut u32, *value),
                        3 => {
                            core::ptr::write_volatile(target, *value as u8);
                            core::ptr::write_volatile(target.add(1), (*value >> 8) as u8);
                            core::ptr::write_volatile(target.add(2), (*value >> 16) as u8);
                        }
                        2 => {
                            let packed = (((*value >> 19) & 0x1F) << 11)
                                | (((*value >> 10) & 0x3F) << 5)
                                | ((*value >> 3) & 0x1F);
                            core::ptr::write_volatile(target as *mut u16, packed as u16);
                        }
                        _ => {}
                    }
                    target = target.add(bytes_per_pixel as usize);
                }
            }
        }
    }
}

impl Canvas for Surface {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = color.as_u32();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowKind {
    About,
    System,
    Palette,
}

struct Window {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    title: String,
    kind: WindowKind,
}

impl Window {
    fn title_bar(&self) -> Rect {
        Rect::new(self.x.max(0) as u32, self.y.max(0) as u32, self.width, TITLE_HEIGHT)
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x + self.width as i32
            && y < self.y + self.height as i32
    }

    fn on_title_bar(&self, x: i32, y: i32) -> bool {
        self.contains(x, y) && y < self.y + TITLE_HEIGHT as i32
    }

    fn close_button(&self) -> (i32, i32, u32) {
        (self.x + self.width as i32 - 18, self.y + 5, 13)
    }

    fn on_close_button(&self, x: i32, y: i32) -> bool {
        let (bx, by, size) = self.close_button();
        x >= bx && y >= by && x < bx + size as i32 && y < by + size as i32
    }
}

struct Desktop {
    windows: Vec<Window>,
    menu_open: bool,
    dragging: Option<(usize, i32, i32)>,
    next_offset: i32,
    running: bool,
    status: String,
}

const MENU_ITEMS: [(&str, WindowKind); 3] = [
    ("About Nook", WindowKind::About),
    ("System info", WindowKind::System),
    ("Colour palette", WindowKind::Palette),
];

impl Desktop {
    fn new() -> Self {
        Self {
            windows: Vec::new(),
            menu_open: false,
            dragging: None,
            next_offset: 0,
            running: true,
            status: "Esc quits  --  click Nook for the menu".to_string(),
        }
    }

    fn open(&mut self, kind: WindowKind, screen_width: u32, screen_height: u32) {
        let (width, height) = match kind {
            WindowKind::About => (360u32, 170u32),
            WindowKind::System => (420, 200),
            WindowKind::Palette => (330, 190),
        };
        let title = match kind {
            WindowKind::About => "About Nook",
            WindowKind::System => "System info",
            WindowKind::Palette => "Colour palette",
        };
        let x = 70 + self.next_offset;
        let y = PANEL_HEIGHT as i32 + 40 + self.next_offset;
        self.next_offset = (self.next_offset + 28) % 140;
        self.windows.push(Window {
            x: x.min(screen_width as i32 - width as i32 - 10).max(0),
            y: y.min(screen_height as i32 - height as i32 - 10).max(PANEL_HEIGHT as i32),
            width,
            height,
            title: title.to_string(),
            kind,
        });
    }

    fn press(&mut self, x: i32, y: i32, screen_width: u32, screen_height: u32) {
        if self.menu_open {
            let height = MENU_ITEMS.len() as u32 * MENU_ITEM_HEIGHT + 8;
            if x >= 0
                && x < MENU_WIDTH as i32
                && y >= PANEL_HEIGHT as i32
                && y < PANEL_HEIGHT as i32 + height as i32
            {
                let index = ((y - PANEL_HEIGHT as i32 - 4) / MENU_ITEM_HEIGHT as i32).max(0) as usize;
                if index < MENU_ITEMS.len() {
                    let kind = MENU_ITEMS[index].1;
                    self.open(kind, screen_width, screen_height);
                    self.status = alloc::format!("opened {}", MENU_ITEMS[index].0);
                }
            }
            self.menu_open = false;
            return;
        }

        if y < PANEL_HEIGHT as i32 {
            if x < 90 {
                self.menu_open = true;
            }
            return;
        }

        for index in (0..self.windows.len()).rev() {
            if self.windows[index].on_close_button(x, y) {
                self.windows.remove(index);
                self.status = "window closed".to_string();
                return;
            }
            if self.windows[index].contains(x, y) {
                let window = self.windows.remove(index);
                let offset = (x - window.x, y - window.y);
                let on_title = y < window.y + TITLE_HEIGHT as i32;
                self.windows.push(window);
                if on_title {
                    self.dragging = Some((self.windows.len() - 1, offset.0, offset.1));
                }
                return;
            }
        }
    }

    fn release(&mut self) {
        self.dragging = None;
    }

    fn motion(&mut self, x: i32, y: i32, screen_width: u32, screen_height: u32) {
        if let Some((index, dx, dy)) = self.dragging {
            if let Some(window) = self.windows.get_mut(index) {
                window.x = (x - dx).clamp(
                    -(window.width as i32) + 40,
                    screen_width as i32 - 40,
                );
                window.y = (y - dy).clamp(PANEL_HEIGHT as i32, screen_height as i32 - 30);
            }
        }
    }
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let fd = sys::open(path);
    if fd < 0 {
        return None;
    }
    let size = sys::fstat_size(fd as u64).max(0) as usize;
    let mut buffer = alloc::vec![0u8; size];
    let mut total = 0usize;
    while total < size {
        let n = sys::read(fd as u64, &mut buffer[total..]);
        if n <= 0 {
            break;
        }
        total += n as usize;
    }
    sys::close(fd as u64);
    buffer.truncate(total);
    if buffer.is_empty() { None } else { Some(buffer) }
}

fn paint_wallpaper(surface: &mut Surface) {
    let decoded = read_file(WALLPAPER).and_then(|bytes| mini_png::decode(&bytes).ok());

    match decoded {
        Some(image) if image.width > 0 && image.height > 0 => {
            for y in 0..surface.height {
                let source_y = y * image.height / surface.height;
                for x in 0..surface.width {
                    let source_x = x * image.width / surface.width;
                    let (r, g, b, _) = image.pixel(source_x, source_y);
                    surface.set_pixel(x, y, Color::rgb(r, g, b));
                }
            }
        }
        _ => {
            let top = Color::rgb(0x1d, 0x2a, 0x3f);
            let bottom = Color::rgb(0x0d, 0x11, 0x18);
            for y in 0..surface.height {
                let t = (y * 255 / surface.height.max(1)) as u8;
                let color = top.lerp(bottom, t);
                for x in 0..surface.width {
                    surface.set_pixel(x, y, color);
                }
            }
        }
    }
}

fn shaded_rect(surface: &mut Surface, x: i32, y: i32, width: u32, height: u32, color: Color) {
    if x + (width as i32) < 0 || y + (height as i32) < 0 {
        return;
    }
    let left = x.max(0) as u32;
    let top = y.max(0) as u32;
    let right = (x + width as i32).max(0) as u32;
    let bottom = (y + height as i32).max(0) as u32;
    surface.fill_rect(Rect::new(left, top, right.saturating_sub(left), bottom.saturating_sub(top)), color);
}

fn draw_panel(surface: &mut Surface, desktop: &Desktop, seconds: i64) {
    let width = surface.width;
    shaded_rect(surface, 0, 0, width, PANEL_HEIGHT, PANEL_BG);
    shaded_rect(surface, 0, PANEL_HEIGHT as i32 - 1, width, 1, ACCENT);

    shaded_rect(surface, 0, 0, 90, PANEL_HEIGHT, if desktop.menu_open { ACCENT } else { PANEL_BG });
    draw_text(surface, 16, 9, "Nook", 1, PANEL_FG);

    draw_text(surface, 110, 9, &desktop.status, 1, Color::rgb(0x8b, 0x95, 0xa6));

    let clock = alloc::format!("up {:02}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60);
    let x = width as i32 - text_width(&clock, 1) as i32 - 14;
    draw_text(surface, x, 9, &clock, 1, PANEL_FG);
}

fn draw_menu(surface: &mut Surface) {
    let height = MENU_ITEMS.len() as u32 * MENU_ITEM_HEIGHT + 8;
    shaded_rect(surface, 3, PANEL_HEIGHT as i32 + 3, MENU_WIDTH, height, SHADOW);
    shaded_rect(surface, 0, PANEL_HEIGHT as i32, MENU_WIDTH, height, WINDOW_BG);
    shaded_rect(surface, 0, PANEL_HEIGHT as i32, 3, height, ACCENT);

    for (index, (label, _)) in MENU_ITEMS.iter().enumerate() {
        let y = PANEL_HEIGHT as i32 + 4 + index as i32 * MENU_ITEM_HEIGHT as i32;
        draw_text(surface, 16, y + 8, label, 1, WINDOW_FG);
    }
}

fn draw_window_body(surface: &mut Surface, window: &Window) {
    let inner_x = window.x + 12;
    let mut line_y = window.y + TITLE_HEIGHT as i32 + 12;
    let step = (GLYPH_H + 6) as i32;

    let lines: Vec<String> = match window.kind {
        WindowKind::About => alloc::vec![
            "Nook -- the HamixOS desktop".to_string(),
            "".to_string(),
            "Drag windows by the title bar,".to_string(),
            "close them with the box on the right.".to_string(),
            "Esc returns to the hsh shell.".to_string(),
        ],
        WindowKind::System => {
            let mut state = MouseState::default();
            sys::mouse(&mut state);
            alloc::vec![
                alloc::format!("resolution   {} x {}", surface.width, surface.height),
                alloc::format!("pointer      {}, {}", state.x, state.y),
                alloc::format!("buttons      {:03b}", state.buttons & 7),
                alloc::format!("wheel        {}", state.wheel),
                alloc::format!("uid          {}", sys::getuid()),
                alloc::format!("pid          {}", sys::getpid()),
            ]
        }
        WindowKind::Palette => Vec::new(),
    };

    for line in &lines {
        draw_text(surface, inner_x, line_y, line, 1, WINDOW_FG);
        line_y += step;
    }

    if window.kind == WindowKind::Palette {
        let swatches = [
            Color::rgb(0xbf, 0x61, 0x6a),
            Color::rgb(0xd0, 0x87, 0x70),
            Color::rgb(0xeb, 0xcb, 0x8b),
            Color::rgb(0xa3, 0xbe, 0x8c),
            Color::rgb(0x88, 0xc0, 0xd0),
            Color::rgb(0xb4, 0x8e, 0xad),
        ];
        for (index, color) in swatches.iter().enumerate() {
            let column = index as i32 % 3;
            let row = index as i32 / 3;
            shaded_rect(
                surface,
                window.x + 20 + column * 96,
                window.y + TITLE_HEIGHT as i32 + 20 + row * 62,
                84,
                50,
                *color,
            );
        }
    }
}

fn draw_window(surface: &mut Surface, window: &Window, focused: bool, pointer: (i32, i32)) {
    shaded_rect(surface, window.x + 5, window.y + 5, window.width, window.height, SHADOW);
    shaded_rect(surface, window.x, window.y, window.width, window.height, WINDOW_BG);

    let title_color = if focused { TITLE_ACTIVE } else { TITLE_IDLE };
    shaded_rect(surface, window.x, window.y, window.width, TITLE_HEIGHT, title_color);
    draw_text(surface, window.x + 10, window.y + 7, &window.title, 1, PANEL_FG);

    let (bx, by, size) = window.close_button();
    let hot = window.on_close_button(pointer.0, pointer.1);
    shaded_rect(surface, bx, by, size, size, if hot { CLOSE_HOT } else { Color::rgb(0x4a, 0x51, 0x5e) });
    draw_text(surface, bx + 3, by + 3, "x", 1, PANEL_FG);

    shaded_rect(surface, window.x, window.y + TITLE_HEIGHT as i32, window.width, 1, ACCENT);

    draw_window_body(surface, window);
    let _ = GLYPH_W;
}

const CURSOR: [&[u8]; 16] = [
    b"X...............",
    b"XX..............",
    b"X.X.............",
    b"X..X............",
    b"X...X...........",
    b"X....X..........",
    b"X.....X.........",
    b"X......X........",
    b"X.......X.......",
    b"X........X......",
    b"X.....XXXXX.....",
    b"X...X..X........",
    b"X..X...X........",
    b"X.X.....X.......",
    b"XX......X.......",
    b"X........X......",
];

fn draw_cursor(surface: &mut Surface, x: i32, y: i32) {
    for (row, line) in CURSOR.iter().enumerate() {
        for (column, cell) in line.iter().enumerate() {
            if *cell != b'X' {
                continue;
            }
            let px = x + column as i32;
            let py = y + row as i32;
            if px < 1 || py < 1 {
                continue;
            }
            surface.set_pixel(px as u32, py as u32, Color::WHITE);
            surface.set_pixel(px as u32 - 1, py as u32, Color::BLACK);
            surface.set_pixel(px as u32, py as u32 - 1, Color::BLACK);
        }
    }
}

fn main() -> i32 {
    let mut info = sys::HamixFbInfo::default();
    if sys::fbmap(&mut info) != 0 {
        println!("hxserver: the kernel reported no linear framebuffer");
        return 1;
    }

    let width = info.width;
    let height = info.height;
    let bytes_per_pixel = (info.bpp / 8).max(1);

    let mut background = Surface::new(width, height);
    paint_wallpaper(&mut background);
    let mut frame = Surface::new(width, height);

    let mut desktop = Desktop::new();
    desktop.open(WindowKind::About, width, height);

    let mut previous_buttons = 0u32;
    let start = sys::clock_gettime().sec;

    while desktop.running {
        let mut state = MouseState::default();
        sys::mouse(&mut state);

        let pressed = state.buttons & !previous_buttons;
        let released = previous_buttons & !state.buttons;
        previous_buttons = state.buttons;

        if pressed & MOUSE_LEFT != 0 {
            desktop.press(state.x, state.y, width, height);
        }
        if released & MOUSE_LEFT != 0 {
            desktop.release();
        }
        if pressed & MOUSE_RIGHT != 0 {
            desktop.menu_open = false;
        }
        desktop.motion(state.x, state.y, width, height);

        while let Some(key) = sys::poll_key() {
            match key {
                Key::Char(27) => desktop.running = false,
                Key::Char(b'q') | Key::Char(b'Q') => desktop.running = false,
                Key::Char(b'a') => desktop.open(WindowKind::About, width, height),
                Key::Char(b's') => desktop.open(WindowKind::System, width, height),
                Key::Char(b'p') => desktop.open(WindowKind::Palette, width, height),
                _ => {}
            }
        }

        frame.copy_from(&background);

        let focused = desktop.windows.len().saturating_sub(1);
        for (index, window) in desktop.windows.iter().enumerate() {
            draw_window(&mut frame, window, index == focused, (state.x, state.y));
        }

        draw_panel(&mut frame, &desktop, sys::clock_gettime().sec - start);
        if desktop.menu_open {
            draw_menu(&mut frame);
        }
        draw_cursor(&mut frame, state.x, state.y);

        frame.blit_to_framebuffer(info.addr, info.pitch, bytes_per_pixel);
    }

    sys::release_framebuffer();
    0
}

entry!(main);
