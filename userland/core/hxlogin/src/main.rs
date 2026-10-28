#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::sys::{self, Key};
use hxstd::{entry, env, fs, print, users};

const LOGIN_CONF: &str = "/etc/hamix/login.conf";

const DEFAULT_SHELL: &str = "/usr/bin/bash";

struct Config {
    shell: String,
    shell_args: Vec<String>,
    desktop: String,
    autostart_desktop: bool,
}

fn read_config() -> Config {
    let mut config = Config { shell: String::from(DEFAULT_SHELL), shell_args: alloc::vec![String::from("-l")], desktop: String::from("/usr/bin/hxserver"), autostart_desktop: false };
    let Some(text) = fs::read_to_string(LOGIN_CONF) else {
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
    if !fs::exists(&config.shell) {
        config.shell = String::from(DEFAULT_SHELL);
        config.shell_args = alloc::vec![String::from("-l")];
    }
    config
}

fn read_line(mask: bool) -> String {
    let mut buf = String::new();
    loop {
        match sys::read_key() {
            Key::Enter => {
                print!("\n");
                return buf;
            }
            Key::Backspace => {
                if buf.pop().is_some() {
                    print!("\x08");
                }
            }
            Key::Char(3) => {
                print!("^C\n");
                return String::new();
            }
            Key::Char(c) if (0x20..0x7F).contains(&c) && buf.len() < 64 => {
                buf.push(c as char);
                if mask {
                    print!("*");
                } else {
                    print!("{}", c as char);
                }
            }
            _ => {}
        }
    }
}

fn hostname() -> String {
    fs::read_to_string("/etc/hostname").map(|h| String::from(h.trim())).filter(|h| !h.is_empty()).unwrap_or_else(|| String::from("hamix"))
}

fn banner(tty: u32) {
    let hostname = hostname();
    let live = fs::read_to_string("/proc/mounts").map(|m| m.contains("live")).unwrap_or(false);
    let mode = if live { "live system, changes stay in RAM" } else { "installed system" };
    print!("\x1b[0m\x0C\x1b[1;36mHamixOS 0.6\x1b[0m  {}  \x1b[90m(tty{}, {})\x1b[0m\n\n", hostname, tty, mode);
}

fn session_env(user: &users::User, shell: &str) -> Vec<String> {
    let mut vars: Vec<String> = alloc::vec![
        String::from("PATH=/usr/bin:/bin:/usr/sbin:/sbin:/var/lib/hamix/commands/bin"),
        format!("HOME={}", user.home),
        format!("USER={}", user.name),
        format!("LOGNAME={}", user.name),
        format!("SHELL={}", shell),
        String::from("TERM=xterm"),
        String::from("LANG=C.UTF-8"),
        format!("XDG_RUNTIME_DIR=/tmp/runtime-{}", user.uid),
    ];
    let mut files = alloc::vec![String::from("/etc/hamix/session.env")];
    if let Some(entries) = sys::read_dir("/etc/hamix/session.d") {
        let mut names: Vec<String> = entries.into_iter().filter(|e| e.name.ends_with(".env")).map(|e| format!("/etc/hamix/session.d/{}", e.name)).collect();
        names.sort();
        files.extend(names);
    }
    for file in files {
        let Some(extra) = fs::read_to_string(&file) else {
            continue;
        };
        for line in extra.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#') && l.contains('=')) {
            let line = line.replace("$UID", &format!("{}", user.uid));
            let (key, value) = line.split_once('=').unwrap_or((&line, ""));
            let value = match vars.iter().find(|v| v.split('=').next() == Some(key)) {
                Some(old) => value.replace(&format!("${}", key), old.split_once('=').map(|(_, v)| v).unwrap_or("")),
                None => String::from(value),
            };
            let entry = format!("{}={}", key, value);
            vars.retain(|v| v.split('=').next() != Some(key));
            vars.push(entry);
        }
    }
    vars
}

fn main() -> i32 {
    let args = env::args();
    let tty: u32 = args.iter().position(|a| a == "--tty").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1);
    let r = sys::session_attach(tty);
    if r < 0 {
        print!("hxlogin: cannot take tty{}: {}\n", tty, sys::error_name(r));
        return 1;
    }
    banner(tty);
    let host = hostname();
    let user = loop {
        print!("\x1b[96m{} login:\x1b[0m ", host);
        let name = String::from(read_line(false).trim());
        if name.is_empty() {
            continue;
        }
        print!("\x1b[96mpassword:\x1b[0m ");
        let password = read_line(true);
        match users::find("/", &name) {
            Some(user) if users::verify_password(&name, password.trim()) => break user,
            _ => {
                print!("\x1b[91mLogin incorrect.\x1b[0m\n\n");
                sys::sleep_ms(1000);
            }
        }
    };
    let config = read_config();
    sys::init_groups(&user.name, user.gid);
    sys::setgid(user.gid);
    if sys::setuid(user.uid) < 0 {
        print!("hxlogin: cannot switch to {}\n", user.name);
        return 1;
    }
    let home = if fs::exists(&user.home) { user.home.clone() } else { String::from("/") };
    sys::chdir(&home);
    let env = session_env(&user, &config.shell);
    if config.autostart_desktop && fs::exists(&config.desktop) {
        let pid = if config.desktop == "/usr/bin/hxserver" && fs::exists(PERM_EXEC) {
            sys::spawn_io_env::<&str, String>(PERM_EXEC, &["org.hamix.Nook", &config.desktop], Some(&env), sys::SPAWN_FOREGROUND, None, None, None)
        } else {
            sys::spawn_io_env::<&str, String>(&config.desktop, &[], Some(&env), sys::SPAWN_FOREGROUND, None, None, None)
        };
        if pid > 0 {
            sys::waitpid(pid, false);
        }
        print!("\x1b[0m\x0C");
    }
    let mut argv = alloc::vec![config.shell.clone()];
    argv.extend(config.shell_args.iter().cloned());
    let r = sys::exec(&config.shell, &argv, &env);
    print!("\x1b[91mlogin: cannot start {}: {}\x1b[0m\n", config.shell, sys::error_name(r));
    sys::sleep_ms(3000);
    1
}

const PERM_EXEC: &str = "/usr/libexec/hamix/hxperm-exec";

entry!(main);
