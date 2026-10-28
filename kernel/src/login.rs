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

pub struct LoginConfig {
    pub shell: String,
    pub shell_args: Vec<String>,
    pub desktop: String,
    pub autostart_desktop: bool,
}

pub fn read_config() -> LoginConfig {
    let mut config = LoginConfig {
        shell: String::from("/usr/bin/bash"),
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

fn run(path: &str, args: &[String], uid: u32, cwd: &str) -> Result<i32, &'static str> {
    let vt = crate::task::current_vt();
    let me = crate::task::current_pid();
    let child = crate::task::elf::spawn(path, args, &crate::task::elf::default_env(uid), me, vt, uid, uid, cwd)?;
    crate::tty::vt::set_input_owner(vt, child);
    let code = crate::task::wait_child(child).unwrap_or(-1);
    crate::tty::vt::set_input_owner(vt, 0);
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

fn run_test(script: &str) -> ! {
    let shell = read_config().shell;
    let args = alloc::vec![String::from(script)];
    let code = match run(&shell, &args, 0, "/") {
        Ok(code) => code,
        Err(e) => {
            out(&format!("hxtest: cannot start {}: {}\n", shell, e));
            -1
        }
    };
    fs::sync();
    crate::serial_println!("hxtest: exit {}", code);
    crate::task::sleep_ticks(crate::task::TICK_HZ / 10);
    crate::arch::disable_interrupts();
    crate::arch::platform::poweroff();
    loop {
        crate::arch::hlt();
    }
}

pub fn run_login(vt: usize) -> ! {
    if vt == 0 {
        if let Some(script) = crate::memory::cmdline_value("hxtest") {
            run_test(&script);
        }
    }
    banner(vt);
    out("\x1b[93mThe init process is not running, so there is no login on this console.\x1b[0m\n");
    out("The kernel keeps no user accounts: they live in /etc/passwd and /etc/shadow of the root filesystem.\n");
    out("Press Ctrl+Alt+Del to restart the computer.\n");
    loop {
        if let Key::Ctrl('c') = keyboard::read_key_blocking() {
            out("^C\n");
        }
    }
}
