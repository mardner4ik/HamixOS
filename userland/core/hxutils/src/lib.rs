#![no_std]

extern crate alloc;

pub mod disk;
pub mod files;
pub mod media;
pub mod net;
pub mod perm;
pub mod system;
pub mod users;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{env, fs, sys};

#[derive(Clone, Copy)]
pub struct Io {
    pub input: u64,
    pub out: u64,
    pub err: u64,
}

impl Io {
    pub const STD: Io = Io { input: 0, out: 1, err: 2 };
}

pub fn write(fd: u64, text: &str) {
    let bytes = text.as_bytes();
    let mut done = 0;
    while done < bytes.len() {
        let n = sys::write(fd, &bytes[done..]);
        if n <= 0 {
            break;
        }
        done += n as usize;
    }
}

#[macro_export]
macro_rules! out {
    ($io:expr, $($arg:tt)*) => { $crate::write($io.out, &alloc::format!($($arg)*)) };
}

#[macro_export]
macro_rules! outln {
    ($io:expr) => { $crate::write($io.out, "\n") };
    ($io:expr, $($arg:tt)*) => {{ let mut s = alloc::format!($($arg)*); s.push('\n'); $crate::write($io.out, &s) }};
}

#[macro_export]
macro_rules! errln {
    ($io:expr, $($arg:tt)*) => {{ let mut s = alloc::format!($($arg)*); s.push('\n'); $crate::write($io.err, &s) }};
}

#[macro_export]
macro_rules! program {
    ($run:path) => {
        #[unsafe(no_mangle)]
        pub extern "Rust" fn __hamix_main() -> i32 {
            $crate::run($run)
        }
    };
}

pub type Command = fn(&[String], Io) -> i32;

pub fn run(command: Command) -> i32 {
    let mut args: Vec<String> = env::args().to_vec();
    if args.is_empty() {
        args.push(String::from(env::program()));
    }
    if let Some(first) = args.first_mut() {
        if let Some(base) = first.rsplit('/').next() {
            *first = String::from(base);
        }
    }
    command(&args, Io::STD)
}

pub struct Opts {
    pub short: Vec<char>,
    pub long: Vec<String>,
    pub values: Vec<(String, String)>,
    pub rest: Vec<String>,
}

impl Opts {
    pub fn has(&self, c: char) -> bool {
        self.short.contains(&c)
    }

    pub fn value(&self, name: &str) -> Option<String> {
        self.values.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
}

pub fn parse_opts(args: &[String], with_value: &[&str]) -> Opts {
    let mut opts = Opts { short: Vec::new(), long: Vec::new(), values: Vec::new(), rest: Vec::new() };
    let mut i = 0;
    let mut literal = false;
    while i < args.len() {
        let arg = &args[i];
        if literal || arg == "-" || !arg.starts_with('-') || arg.len() == 1 {
            opts.rest.push(arg.clone());
            i += 1;
            continue;
        }
        if arg == "--" {
            literal = true;
            i += 1;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            if let Some((k, v)) = long.split_once('=') {
                opts.values.push((String::from(k), String::from(v)));
            } else if with_value.contains(&long) && i + 1 < args.len() {
                opts.values.push((String::from(long), args[i + 1].clone()));
                i += 1;
            } else {
                opts.long.push(String::from(long));
            }
            i += 1;
            continue;
        }
        let body: Vec<char> = arg[1..].chars().collect();
        if arg[1..].chars().all(|c| c.is_ascii_digit()) {
            opts.rest.push(arg.clone());
            i += 1;
            continue;
        }
        let mut j = 0;
        while j < body.len() {
            let key = String::from(body[j]);
            if with_value.contains(&key.as_str()) {
                let remainder: String = body[j + 1..].iter().collect();
                if !remainder.is_empty() {
                    opts.values.push((key, remainder));
                } else if i + 1 < args.len() {
                    opts.values.push((key, args[i + 1].clone()));
                    i += 1;
                }
                break;
            }
            opts.short.push(body[j]);
            j += 1;
        }
        i += 1;
    }
    opts
}

pub fn absolute(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let cwd = sys::getcwd();
    if !path.starts_with('/') {
        parts.extend(cwd.split('/').filter(|p| !p.is_empty()));
    }
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{}{}", dir, name) } else { format!("{}/{}", dir, name) }
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 4 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}", bytes)
    } else if value >= 10.0 {
        format!("{}{}", (value + 0.5) as u64, UNITS[unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        format!("{}.{}{}", tenths / 10, tenths % 10, UNITS[unit])
    }
}

pub fn remove_tree(path: &str) -> i64 {
    if let Ok(stat) = sys::stat(path) {
        if stat.is_dir() {
            for entry in sys::read_dir(path).unwrap_or_default() {
                let child = join(path, &entry.name);
                if entry.kind == 'd' {
                    remove_tree(&child);
                }
            }
        }
    }
    sys::unlink(path)
}

pub fn read_all(fd: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = sys::read(fd, &mut buf);
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    out
}

pub fn read_line_fd(fd: u64, secret: bool) -> Option<String> {
    let terminal = sys::isatty(fd);
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = sys::read(fd, &mut byte);
        if n <= 0 {
            if bytes.is_empty() {
                return None;
            }
            break;
        }
        match byte[0] {
            b'\n' | b'\r' => break,
            0x7f | 0x08 if terminal => {
                if bytes.pop().is_some() && !secret {
                    write(1, "\x08 \x08");
                }
            }
            0x03 if terminal => {
                write(1, "^C\n");
                return None;
            }
            0x04 if terminal && bytes.is_empty() => {
                write(1, "\n");
                return None;
            }
            b => bytes.push(b),
        }
    }
    if terminal && secret {
        write(1, "\n");
    }
    Some(String::from(String::from_utf8_lossy(&bytes).trim_end_matches('\r')))
}

pub fn parse_conf(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            let v = v.trim();
            let v = if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) { &v[1..v.len() - 1] } else { v };
            Some((String::from(k.trim()), String::from(v)))
        })
        .collect()
}

pub fn set_conf_value(path: &str, key: &str, value: &str) -> bool {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    let mut done = false;
    for line in text.lines() {
        if line.trim_start().split('=').next().map(|k| k.trim()) == Some(key) && !line.trim_start().starts_with('#') {
            out.push_str(&format!("{}={}\n", key, value));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !done {
        out.push_str(&format!("{}={}\n", key, value));
    }
    fs::write(path, out.as_bytes())
}

pub fn print_kv(io: Io, key: &str, value: &str) {
    out!(io, "  \x1b[90m{:<14}\x1b[0m {}\n", key, value);
}

pub fn hostname() -> String {
    fs::read_to_string("/etc/hostname").map(|s| String::from(s.trim())).filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("hamix"))
}

pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c)) {
        return String::from(arg);
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

pub const DEFAULT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin:/var/lib/hamix/commands/bin";
pub const SHELL: &str = "/usr/bin/bash";

pub fn resolve_program(name: &str) -> Option<String> {
    if name.contains('/') {
        let path = absolute(name);
        return sys::stat(&path).ok().filter(|s| !s.is_dir()).map(|_| path);
    }
    let path = env::var("PATH").unwrap_or(DEFAULT_PATH);
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate = format!("{}/{}", dir.trim_end_matches('/'), name);
        if let Ok(stat) = sys::stat(&candidate) {
            if !stat.is_dir() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn command_line(argv: &[String]) -> Option<(String, Vec<String>)> {
    let name = argv.first()?;
    let Some(path) = resolve_program(name) else {
        let command = sys::cmd_list().into_iter().find(|c| &c.name == name)?;
        let mut args = command.args.clone();
        args.extend_from_slice(&argv[1..]);
        return Some((command.path, args));
    };
    let head = fs::read_prefix(&path, 4).unwrap_or_default();
    if head.starts_with(b"\x7fELF") || head.starts_with(b"#!") {
        return Some((path, argv[1..].to_vec()));
    }
    let mut args = alloc::vec![path];
    args.extend_from_slice(&argv[1..]);
    Some((String::from(SHELL), args))
}

pub fn environment() -> Vec<String> {
    env::vars().iter().map(|(k, v)| format!("{}={}", k, v)).collect()
}

pub fn spawn_env(program: &str, args: &[String], env: &[String], flags: u64, io: Io) -> i64 {
    sys::spawn_io_env(program, args, Some(env), flags, Some(io.input), Some(io.out), Some(io.err))
}

pub fn wait(pid: i64) -> i32 {
    let code = sys::waitpid(pid, false);
    sys::set_foreground(0);
    if code < 0 { 1 } else { code as i32 }
}

pub fn run_foreground(program: &str, args: &[&str], io: Io) -> i32 {
    let args: Vec<String> = args.iter().map(|a| String::from(*a)).collect();
    let pid = spawn_env(program, &args, &environment(), sys::SPAWN_FOREGROUND, io);
    if pid < 0 {
        errln!(io, "{}: {}", program, sys::error_name(pid));
        return 127;
    }
    wait(pid)
}

pub fn civil(epoch: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (epoch / 86400) as i64;
    let secs = (epoch % 86400) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
}

pub fn format_duration(ms: u64) -> String {
    let secs = ms / 1000;
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{}d {}h {}m", d, h, m)
    } else if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m {}s", m, s)
    }
}

pub fn exec_shell() -> i32 {
    let mut args: Vec<String> = env::args().to_vec();
    if args.is_empty() {
        args.push(String::from("sh"));
    }
    let r = sys::exec(SHELL, &args, &environment());
    let io = Io::STD;
    errln!(io, "{}: cannot start {}: {}", args[0], SHELL, sys::error_name(r));
    127
}
