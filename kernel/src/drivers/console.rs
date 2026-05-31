use core::fmt;
use spin::Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::video::vesa::FRAMEBUFFER;
use crate::drivers::video::Framebuffer;
use crate::drivers::input::keyboard;

const COLS: usize = 80;
const ROWS: usize = 30;
const FONT_W: usize = 8;
const FONT_H: usize = 16;

const COLOR_BG: u32      = 0x001B2333;
const COLOR_FG: u32      = 0x00C8D3E0;
const COLOR_PROMPT: u32  = 0x0057C7FF;
const COLOR_INPUT: u32   = 0x00FFFFFF;
const COLOR_ERROR: u32   = 0x00FF5555;
const COLOR_SUCCESS: u32 = 0x0050FA7B;
const COLOR_HEADER: u32  = 0x00BD93F9;
const COLOR_DIM: u32     = 0x006272A4;
const COLOR_WARN: u32    = 0x00FFB86C;

static FONT_8X16: &[u8] = include_bytes!("font8x16.bin");

pub struct Console {
    col: usize,
    row: usize,
    color: u32,
    bg: u32,
    screen_w: usize,
    screen_h: usize,
}

impl Console {
    const fn new() -> Self {
        Self {
            col: 0,
            row: 0,
            color: COLOR_FG,
            bg: COLOR_BG,
            screen_w: 0,
            screen_h: 0,
        }
    }

    fn ensure_fb(&mut self) {
        if self.screen_w == 0 {
            let fb = FRAMEBUFFER.lock();
            self.screen_w = fb.width();
            self.screen_h = fb.height();
        }
    }

    pub fn clear(&mut self) {
        self.ensure_fb();
        FRAMEBUFFER.lock().clear(self.bg);
        self.col = 0;
        self.row = 0;
    }

    fn draw_char(&self, ch: char, x: usize, y: usize, fg: u32, bg: u32) {
        let px = x * FONT_W;
        let py = y * FONT_H;
        let code = (ch as usize).min(255);
        let glyph_offset = code * FONT_H;

        let mut fb = FRAMEBUFFER.lock();
        for row in 0..FONT_H {
            let byte = if glyph_offset + row < FONT_8X16.len() {
                FONT_8X16[glyph_offset + row]
            } else {
                0
            };
            for col in 0..FONT_W {
                let bit = (byte >> (7 - col)) & 1;
                let color = if bit != 0 { fg } else { bg };
                fb.put_pixel(px + col, py + row, color);
            }
        }
    }

    fn scroll_one(&self) {
        FRAMEBUFFER.lock().scroll_up(FONT_H, COLOR_BG);
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= ROWS {
            self.scroll_one();
            self.row = ROWS - 1;
        }
    }

    pub fn write_char_colored(&mut self, ch: char, fg: u32) {
        self.ensure_fb();
        match ch {
            '\n' => self.newline(),
            '\r' => { self.col = 0; }
            '\x08' => {
                if self.col > 0 {
                    self.col -= 1;
                    self.draw_char(' ', self.col, self.row, fg, self.bg);
                }
            }
            _ => {
                if self.screen_w == 0 { return; }
                if (ch as u32) < 256 {
                    self.draw_char(ch, self.col, self.row, fg, self.bg);
                } else {
                    self.draw_char('?', self.col, self.row, fg, self.bg);
                }
                self.col += 1;
                if self.col >= COLS {
                    self.newline();
                }
            }
        }
    }

    pub fn write_str_colored(&mut self, s: &str, fg: u32) {
        for ch in s.chars() {
            self.write_char_colored(ch, fg);
        }
    }

    pub fn set_color(&mut self, fg: u32) {
        self.color = fg;
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str_colored(s, self.color);
        Ok(())
    }
}

pub static CONSOLE: Mutex<Console> = Mutex::new(Console::new());

pub fn print_colored(s: &str, color: u32) {
    CONSOLE.lock().write_str_colored(s, color);
}

pub fn println_colored(s: &str, color: u32) {
    let mut c = CONSOLE.lock();
    c.write_str_colored(s, color);
    c.write_char_colored('\n', color);
}

fn read_line_masked(mask: bool) -> String {
    let mut buf = String::new();
    loop {
        let ch = keyboard::read_char_blocking();
        match ch {
            '\n' => {
                CONSOLE.lock().write_char_colored('\n', COLOR_FG);
                break;
            }
            '\x08' => {
                if !buf.is_empty() {
                    buf.pop();
                    CONSOLE.lock().write_char_colored('\x08', COLOR_INPUT);
                }
            }
            _ if ch.is_ascii() && !ch.is_control() => {
                buf.push(ch);
                let display = if mask { '*' } else { ch };
                CONSOLE.lock().write_char_colored(display, COLOR_INPUT);
            }
            _ => {}
        }
    }
    buf
}

fn print_banner() {
    let mut c = CONSOLE.lock();
    c.clear();
    c.write_str_colored("+--------------------------------------------------+\n", COLOR_HEADER);
    c.write_str_colored("|          HamixOS  v0.1.0  (x86_64)              |\n", COLOR_HEADER);
    c.write_str_colored("|     Unix-like kernel -- built in Rust            |\n", COLOR_DIM);
    c.write_str_colored("+--------------------------------------------------+\n", COLOR_HEADER);
    c.write_str_colored("\n", COLOR_FG);
}

fn check_credentials(user: &str, pass: &str) -> bool {
    user == "root" && pass == "hamix"
}

pub fn run_login() -> ! {
    print_banner();

    loop {
        print_colored("login: ", COLOR_PROMPT);
        let username = read_line_masked(false);

        print_colored("password: ", COLOR_PROMPT);
        let password = read_line_masked(true);

        if check_credentials(username.trim(), password.trim()) {
            println_colored("\nLogin successful.", COLOR_SUCCESS);
            run_shell(username.trim());
        } else {
            println_colored("\nLogin incorrect.", COLOR_ERROR);
            print_colored("\n", COLOR_FG);
        }
    }
}

fn run_shell(user: &str) -> ! {
    use alloc::format;

    {
        let mut c = CONSOLE.lock();
        c.write_str_colored("\nWelcome to HamixOS. Type 'help' for available commands.\n\n", COLOR_SUCCESS);
    }

    loop {
        let prompt = if user == "root" {
            format!("{}@hamix:~# ", user)
        } else {
            format!("{}@hamix:~$ ", user)
        };
        print_colored(&prompt, COLOR_PROMPT);

        let line = read_line_masked(false);
        let line_str = line.trim();

        if line_str.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
        let cmd = parts[0];
        let args = parts.get(1).copied().unwrap_or("");

        match cmd {
            "help"     => cmd_help(),
            "clear"    => { CONSOLE.lock().clear(); }
            "echo"     => { println_colored(args, COLOR_FG); }
            "uname"    => cmd_uname(args),
            "whoami"   => println_colored(user, COLOR_FG),
            "meminfo"  => cmd_meminfo(),
            "uptime"   => cmd_uptime(),
            "pwd"      => println_colored("/", COLOR_FG),
            "ls"       => cmd_ls(),
            "hostname" => println_colored("hamix", COLOR_FG),
            "date"     => println_colored("Tue Jan  1 00:00:00 UTC 2000", COLOR_FG),
            "halt" | "poweroff" => cmd_halt(),
            "reboot"   => cmd_reboot(),
            "logout" | "exit" => {
                println_colored("\nLogged out.\n", COLOR_DIM);
                print_banner();
                loop {
                    print_colored("login: ", COLOR_PROMPT);
                    let u = read_line_masked(false);
                    print_colored("password: ", COLOR_PROMPT);
                    let p = read_line_masked(true);
                    if check_credentials(u.trim(), p.trim()) {
                        println_colored("\nLogin successful.", COLOR_SUCCESS);
                        run_shell(u.trim());
                    } else {
                        println_colored("\nLogin incorrect.", COLOR_ERROR);
                        print_colored("\n", COLOR_FG);
                    }
                }
            }
            "version"  => cmd_version(),
            "cpuinfo"  => cmd_cpuinfo(),
            _ => {
                let msg = alloc::format!("{}: command not found\n", cmd);
                print_colored(&msg, COLOR_ERROR);
            }
        }
    }
}

fn cmd_help() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nAvailable commands:\n", COLOR_HEADER);
    let cmds = [
        ("help",       "show this help"),
        ("clear",      "clear the screen"),
        ("echo <msg>", "print a message"),
        ("uname [-a]", "kernel information"),
        ("whoami",     "current user name"),
        ("meminfo",    "memory usage"),
        ("uptime",     "system uptime"),
        ("pwd",        "current directory"),
        ("ls",         "list directory entries"),
        ("hostname",   "system hostname"),
        ("cpuinfo",    "processor information"),
        ("version",    "HamixOS version"),
        ("logout",     "log out current user"),
        ("halt",       "halt the system"),
        ("reboot",     "reboot the system"),
    ];
    for (name, desc) in &cmds {
        let line = alloc::format!("  {:<14}  {}\n", name, desc);
        c.write_str_colored(&line, COLOR_FG);
    }
    c.write_str_colored("\n", COLOR_FG);
}

fn cmd_uname(args: &str) {
    if args.contains('a') {
        println_colored("HamixOS hamix 0.1.0 x86_64 GNU/Rust", COLOR_FG);
    } else {
        println_colored("HamixOS", COLOR_FG);
    }
}

fn cmd_meminfo() {
    use crate::memory::frame::memory_info;
    let (free, total) = memory_info();
    let used = total.saturating_sub(free);
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nMemory:\n", COLOR_HEADER);
    let line = alloc::format!("  Total : {:>8} KB\n", total / 1024);
    c.write_str_colored(&line, COLOR_FG);
    let line = alloc::format!("  Used  : {:>8} KB\n", used / 1024);
    c.write_str_colored(&line, COLOR_WARN);
    let line = alloc::format!("  Free  : {:>8} KB\n\n", free / 1024);
    c.write_str_colored(&line, COLOR_SUCCESS);
}

fn cmd_uptime() {
    let ticks = crate::task::uptime_ticks();
    let secs = ticks / 100;
    let msg = alloc::format!("up {} seconds ({} ticks)\n", secs, ticks);
    print_colored(&msg, COLOR_FG);
}

fn cmd_ls() {
    println_colored("bin  dev  etc  home  proc  root  tmp  usr  var", COLOR_SUCCESS);
}

fn cmd_halt() {
    print_colored("\nSystem halted. Power off your machine.\n", COLOR_WARN);
    use crate::arch::x86_64::disable_interrupts;
    disable_interrupts();
    loop { crate::arch::x86_64::hlt(); }
}

fn cmd_reboot() {
    print_colored("\nRebooting...\n", COLOR_WARN);
    use crate::arch::x86_64::{outb, disable_interrupts};
    disable_interrupts();
    outb(0x64, 0xFE);
    loop { crate::arch::x86_64::hlt(); }
}

fn cmd_version() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("HamixOS v0.1.0\n", COLOR_HEADER);
    c.write_str_colored("Kernel : Rust no_std, x86_64\n", COLOR_FG);
    c.write_str_colored("Boot   : GRUB2 Multiboot2\n", COLOR_FG);
    c.write_str_colored("Target : Pentium G640 / Celeron T3100\n\n", COLOR_FG);
}

fn cmd_cpuinfo() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nProcessor:\n", COLOR_HEADER);
    c.write_str_colored("  Architecture : x86_64\n", COLOR_FG);
    c.write_str_colored("  Mode         : 64-bit Long Mode\n", COLOR_FG);
    c.write_str_colored("  Features     : soft-float, no-SSE, no-MMX\n", COLOR_FG);
    c.write_str_colored("  Compatible   : Pentium G640, Celeron T3100\n\n", COLOR_FG);
}
