use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::input::keyboard::{self, Key};
use crate::drivers::video::text_mode::TEXT_CONSOLE;
use crate::fs;

pub const CONFIG_PATH: &str = "/etc/hamix/login.conf";

fn out(text: &str) {
    let vt = crate::task::current_vt();
    TEXT_CONSOLE.lock().write_bytes_to(vt, text.as_bytes());
}

fn read_line(mask: bool) -> String {
    let mut buf = String::new();
    loop {
        match keyboard::read_key_blocking() {
            Key::Enter => {
                out("\n");
                return buf;
            }
            Key::Backspace => {
                if buf.pop().is_some() {
                    out("\x08");
                }
            }
            Key::Ctrl('c') => {
                out("^C\n");
                return String::new();
            }
            Key::Char(ch) if ch.is_ascii() && !ch.is_control() && buf.len() < 64 => {
                buf.push(ch);
                if mask {
                    out("*");
                } else {
                    let mut tmp = [0u8; 4];
                    out(ch.encode_utf8(&mut tmp));
                }
            }
            _ => {}
        }
    }
}

pub struct LoginConfig {
    pub shell: String,
    pub shell_args: Vec<String>,
    pub desktop: String,
    pub autostart_desktop: bool,
}

pub fn read_config() -> LoginConfig {
    let mut config = LoginConfig {
        shell: String::from("/usr/bin/hsh"),
        shell_args: alloc::vec![String::from("-l")],
        desktop: String::from("/usr/bin/hxserver"),
        autostart_desktop: false,
    };
    let text = {
        let mut guard = fs::VFS.lock();
        guard.as_mut().and_then(|v| v.read(0, CONFIG_PATH).ok()).map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let Some(text) = text else {
        return config;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "shell" if !value.is_empty() => config.shell = String::from(value),
            "shell_args" => config.shell_args = value.split_whitespace().map(String::from).collect(),
            "desktop" if !value.is_empty() => config.desktop = String::from(value),
            "autostart_desktop" => config.autostart_desktop = matches!(value, "yes" | "true" | "1" | "on"),
            _ => {}
        }
    }
    config
}

fn executable(path: &str) -> bool {
    let guard = fs::VFS.lock();
    guard.as_ref().and_then(|v| v.resolve(0, path).map(|id| !v.is_dir(id))).unwrap_or(false)
}

fn run(path: &str, args: &[String], uid: u32, cwd: &str) -> Result<i32, &'static str> {
    let vt = crate::task::current_vt();
    let me = crate::task::current_pid();
    let child = crate::task::elf::spawn(path, args, &crate::task::elf::default_env(uid), me, vt, uid, uid, cwd)?;
    crate::vt::set_input_owner(vt, child);
    let code = crate::task::wait_child(child).unwrap_or(-1);
    crate::vt::set_input_owner(vt, 0);
    Ok(code)
}

fn banner(vt: usize) {
    let hostname = {
        let mut guard = fs::VFS.lock();
        guard
            .as_mut()
            .and_then(|v| v.read(0, "/etc/hostname").ok())
            .map(|b| String::from(String::from_utf8_lossy(&b).trim()))
            .unwrap_or_else(|| String::from("hamix"))
    };
    let mode = if fs::hextfs::root_is_live() { "live system, changes stay in RAM" } else { "installed system" };
    static FIRST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
    if !(vt == 0 && FIRST.swap(false, core::sync::atomic::Ordering::Relaxed)) {
        out("\x1b[0m\x0C");
    }
    out(&format!("\x1b[1;36mHamixOS 0.6\x1b[0m  {}  \x1b[90m(tty{}, {})\x1b[0m\n\n", hostname, vt + 1, mode));
}

pub fn run_login(vt: usize) -> ! {
    loop {
        banner(vt);
        let (user, record) = loop {
            out("\x1b[96mhamix login:\x1b[0m ");
            let name = read_line(false);
            let name = String::from(name.trim());
            if name.is_empty() {
                continue;
            }
            out("\x1b[96mpassword:\x1b[0m ");
            let password = read_line(true);
            crate::users::reload();
            match crate::users::find_by_name(&name) {
                Some(record) if crate::users::verify_password(&name, password.trim()) => break (name, record),
                _ => {
                    out("\x1b[91mLogin incorrect.\x1b[0m\n\n");
                    crate::task::sleep_ticks(crate::task::TICK_HZ);
                }
            }
        };

        let config = read_config();
        let home = if executable(&record.home) || fs::VFS.lock().as_ref().map(|v| v.exists(0, &record.home)).unwrap_or(false) {
            record.home.clone()
        } else {
            String::from("/")
        };

        let _ = user;
        if config.autostart_desktop && crate::task::display::owner().is_none() && executable(&config.desktop) {
            if let Err(e) = run(&config.desktop, &[], record.uid, &home) {
                out(&format!("\x1b[91mdesktop {}: {}\x1b[0m\n", config.desktop, e));
            }
            out("\x1b[0m\x0C");
        }

        if !executable(&config.shell) {
            out(&format!(
                "\x1b[91mlogin: the shell {} from {} does not exist\x1b[0m\n",
                config.shell, CONFIG_PATH
            ));
            crate::task::sleep_ticks(crate::task::TICK_HZ * 3);
            continue;
        }
        match run(&config.shell, &config.shell_args, record.uid, &home) {
            Ok(_) => {}
            Err(e) => {
                out(&format!("\x1b[91mlogin: cannot start {}: {}\x1b[0m\n", config.shell, e));
                crate::task::sleep_ticks(crate::task::TICK_HZ * 3);
            }
        }
        fs::sync();
    }
}
