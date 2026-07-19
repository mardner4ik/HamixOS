use core::fmt;
use spin::Mutex;
use crate::drivers::video::text_mode::{
    TEXT_CONSOLE, attr,
    BLACK, WHITE, LIGHT_GRAY, DARK_GRAY, LIGHT_CYAN,
    LIGHT_RED, LIGHT_GREEN, YELLOW,
};

pub const COLOR_FG: u8      = attr(LIGHT_GRAY, BLACK);
pub const COLOR_PROMPT: u8  = attr(LIGHT_CYAN, BLACK);
pub const COLOR_INPUT: u8   = attr(WHITE, BLACK);
pub const COLOR_ERROR: u8   = attr(LIGHT_RED, BLACK);
pub const COLOR_SUCCESS: u8 = attr(LIGHT_GREEN, BLACK);
pub const COLOR_HEADER: u8  = attr(YELLOW, BLACK);
pub const COLOR_DIM: u8     = attr(DARK_GRAY, BLACK);
pub const COLOR_WARN: u8    = attr(YELLOW, BLACK);

pub struct Console;

impl Console {
    const fn new() -> Self {
        Self
    }

    pub fn clear(&mut self) {
        TEXT_CONSOLE.lock().clear();
    }

    pub fn write_char_colored(&mut self, ch: char, fg: u8) {
        TEXT_CONSOLE.lock().write_char_attr(ch, fg);
    }

    pub fn write_str_colored(&mut self, s: &str, fg: u8) {
        TEXT_CONSOLE.lock().write_str_attr(s, fg);
    }

    #[allow(dead_code)]
    pub fn set_color(&mut self, fg: u8) {
        TEXT_CONSOLE.lock().set_default_attr(fg);
    }

    #[allow(dead_code)]
    pub fn cursor(&self) -> (usize, usize) {
        TEXT_CONSOLE.lock().cursor()
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        TEXT_CONSOLE.lock().write_str(s);
        Ok(())
    }
}

pub static CONSOLE: Mutex<Console> = Mutex::new(Console::new());

pub fn print_colored(s: &str, color: u8) {
    CONSOLE.lock().write_str_colored(s, color);
}

pub fn println_colored(s: &str, color: u8) {
    let mut c = CONSOLE.lock();
    c.write_str_colored(s, color);
    c.write_char_colored('\n', color);
}
