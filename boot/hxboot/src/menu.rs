use crate::bios;
use crate::config::{Config, Kind};
use crate::console;

const TOP: u32 = 4;
const EDIT_MAX: usize = 900;

static mut EDIT: [u8; EDIT_MAX] = [0; EDIT_MAX];

pub struct Choice {
    pub index: usize,
    pub cmdline: Option<&'static str>,
}

fn footer_row(config: &Config) -> u32 {
    TOP + config.count as u32 + 2
}

fn clear_row(row: u32) {
    for c in 0..80 {
        console::put_at(row, c, b' ', 0x07);
    }
}

fn draw(config: &Config, selected: usize, seconds: Option<u32>) {
    console::write_at(2, 2, config.title, 0x0f);
    for i in 0..config.count {
        let color = if i == selected { 0x70 } else { 0x07 };
        let row = TOP + i as u32;
        for c in 2..78 {
            console::put_at(row, c, b' ', color);
        }
        let name = config.entries[i].name;
        console::write_at(row, 4, &name[..name.len().min(72)], color);
    }
    let footer = footer_row(config);
    clear_row(footer);
    clear_row(footer + 1);
    let mut buffer = [0u8; 80];
    let text = match seconds {
        Some(s) => countdown(&mut buffer, s),
        None => "Up/Down choose, Enter boot, e edit the command line",
    };
    console::write_at(footer, 2, text, 0x08);
}

fn countdown(buffer: &mut [u8; 80], seconds: u32) -> &str {
    let prefix = b"Up/Down choose, Enter boot, e edit. Booting in ";
    buffer[..prefix.len()].copy_from_slice(prefix);
    let mut at = prefix.len();
    let mut digits = [0u8; 10];
    let mut n = seconds;
    let mut count = 0;
    loop {
        digits[count] = b'0' + (n % 10) as u8;
        count += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for i in (0..count).rev() {
        buffer[at] = digits[i];
        at += 1;
    }
    buffer[at..at + 2].copy_from_slice(b" s");
    at += 2;
    core::str::from_utf8(&buffer[..at]).unwrap_or("")
}

fn edit(config: &Config, index: usize) -> Option<&'static str> {
    let entry = &config.entries[index];
    let buf = unsafe { &mut *(&raw mut EDIT) };
    let mut len = entry.cmdline.len().min(EDIT_MAX);
    buf[..len].copy_from_slice(&entry.cmdline.as_bytes()[..len]);
    let row = footer_row(config);
    loop {
        clear_row(row);
        clear_row(row + 1);
        clear_row(row + 2);
        console::write_at(row, 2, "Command line (Enter boot, Esc cancel):", 0x0f);
        let visible = if len > 150 { len - 150 } else { 0 };
        let text = core::str::from_utf8(&buf[visible..len]).unwrap_or("");
        let (first, second) = if text.len() > 75 { text.split_at(75) } else { (text, "") };
        console::write_at(row + 1, 2, first, 0x07);
        console::write_at(row + 2, 2, second, 0x07);
        let cursor = (len - visible) as u32;
        console::put_at(row + 1 + cursor / 75, 2 + cursor % 75, b'_', 0x0f);
        let (scan, ascii) = loop {
            if let Some(key) = bios::key_ready() {
                break key;
            }
            bios::wait_ms(20);
        };
        match (scan, ascii) {
            (_, b'\r') => return core::str::from_utf8(&buf[..len]).ok(),
            (_, 0x1b) => return None,
            (_, 0x08) => len = len.saturating_sub(1),
            (_, c @ 0x20..=0x7e) if len < EDIT_MAX => {
                buf[len] = c;
                len += 1;
            }
            _ => {}
        }
    }
}

pub fn run(config: &Config) -> Choice {
    for i in 0..config.count {
        let mut line = [0u8; 80];
        let name = config.entries[i].name.as_bytes();
        let n = name.len().min(74);
        line[0] = b' ';
        line[1] = if i < 9 { b'1' + i as u8 } else { b'+' };
        line[2..6].copy_from_slice(b"    ");
        line[6..6 + n].copy_from_slice(&name[..n]);
        console::serial_line(core::str::from_utf8(&line[..6 + n]).unwrap_or(""));
    }
    let mut selected = config.default;
    if config.timeout == 0 && !bios::shift_held() {
        return Choice { index: selected, cmdline: None };
    }
    console::clear();
    console::banner();
    let mut ticks = config.timeout.max(1) * 10;
    let mut counting = true;
    draw(config, selected, Some(ticks.div_ceil(10)));
    loop {
        if let Some((scan, ascii)) = bios::key_ready() {
            counting = false;
            match (scan, ascii) {
                (0x48, _) => selected = if selected == 0 { config.count - 1 } else { selected - 1 },
                (0x50, _) => selected = (selected + 1) % config.count,
                (0x47, _) => selected = 0,
                (0x4f, _) => selected = config.count - 1,
                (_, b'\r') => return Choice { index: selected, cmdline: None },
                (_, b'e') | (_, b'E') if config.entries[selected].kind == Kind::Kernel => {
                    if let Some(cmdline) = edit(config, selected) {
                        return Choice { index: selected, cmdline: Some(cmdline) };
                    }
                }
                (_, d @ b'1'..=b'9') if ((d - b'1') as usize) < config.count => return Choice { index: (d - b'1') as usize, cmdline: None },
                _ => {}
            }
            draw(config, selected, None);
            continue;
        }
        if counting {
            bios::wait_ms(100);
            ticks -= 1;
            if ticks == 0 {
                return Choice { index: selected, cmdline: None };
            }
            if ticks % 10 == 0 {
                draw(config, selected, Some(ticks / 10));
            }
        } else {
            bios::wait_ms(20);
        }
    }
}
