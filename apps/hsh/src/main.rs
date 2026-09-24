#![no_std]
#![no_main]

extern crate alloc;

mod builtins;
mod lexer;
mod line;
mod parser;
mod shell;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, env, fs, sys};

use shell::{write, Io, Shell};

pub fn run_argv(sh: &mut Shell, argv: &[String], io: Io) -> i32 {
    if argv.is_empty() {
        return 0;
    }
    if builtins::is_builtin(&argv[0]) {
        return builtins::run(sh, argv, io);
    }
    match sh.spawn_external(argv, io, true) {
        Ok(pid) => sh.wait_foreground(pid),
        Err(e) => {
            errln!(io, "{}", e);
            127
        }
    }
}

fn prompt(sh: &Shell) -> String {
    let uid = sys::geteuid();
    let cwd = sys::getcwd();
    let home = sh.home();
    let shown = if home.len() > 1 && (cwd == home || cwd.starts_with(&format!("{}/", home))) { format!("~{}", &cwd[home.len()..]) } else { cwd };
    let user = sh.var("USER");
    let status = if sh.status != 0 { format!("\x1b[91m[{}]\x1b[0m ", sh.status) } else { String::new() };
    if uid == 0 {
        format!("{}\x1b[91mroot@{}\x1b[0m:\x1b[94m{}\x1b[0m# ", status, shell::hostname(), shown)
    } else {
        format!("{}\x1b[92m{}@{}\x1b[0m:\x1b[94m{}\x1b[0m$ ", status, user, shell::hostname(), shown)
    }
}

fn completions(sh: &Shell, before: &str) -> Vec<String> {
    let word_start = before.rfind(|c: char| c == ' ' || c == '|' || c == ';' || c == '&').map(|i| i + 1).unwrap_or(0);
    let word = &before[word_start..];
    let head = before[..word_start].trim_end();
    let command_position = head.is_empty() || head.ends_with('|') || head.ends_with(';') || head.ends_with("&&") || head.ends_with("sudo");
    let mut out: Vec<String> = Vec::new();
    if command_position && !word.contains('/') {
        for name in builtins::names() {
            if name.starts_with(word) {
                out.push(String::from(name));
            }
        }
        for name in sh.functions.keys().chain(sh.aliases.keys()) {
            if name.starts_with(word) {
                out.push(name.clone());
            }
        }
        for dir in sh.var("PATH").split(':') {
            for entry in sys::read_dir(dir).unwrap_or_default() {
                if !entry.is_dir && entry.name.starts_with(word) {
                    out.push(entry.name);
                }
            }
        }
        for command in sys::cmd_list() {
            if command.name.starts_with(word) {
                out.push(command.name);
            }
        }
    } else {
        let expanded = if let Some(rest) = word.strip_prefix('~') { format!("{}{}", sh.home(), rest) } else { String::from(word) };
        let (dir, prefix) = match expanded.rfind('/') {
            Some(i) => (String::from(&expanded[..i + 1]), String::from(&expanded[i + 1..])),
            None => (String::new(), expanded.clone()),
        };
        let shown_dir = match word.rfind('/') {
            Some(i) => String::from(&word[..i + 1]),
            None => String::new(),
        };
        let listing = if dir.is_empty() { String::from(".") } else { dir.clone() };
        for entry in sys::read_dir(&listing).unwrap_or_default() {
            if entry.name.starts_with(&prefix) && (prefix.starts_with('.') || !entry.name.starts_with('.')) {
                let mut candidate = format!("{}{}", shown_dir, entry.name);
                if entry.is_dir {
                    candidate.push('/');
                }
                out.push(candidate);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn history_path(sh: &Shell) -> String {
    format!("{}/.hsh_history", sh.home().trim_end_matches('/'))
}

fn interactive(sh: &mut Shell) -> i32 {
    sh.interactive = true;
    if let Some(text) = fs::read_to_string(&history_path(sh)) {
        sh.history = text.lines().map(String::from).collect();
        let len = sh.history.len();
        if len > 500 {
            sh.history.drain(..len - 500);
        }
    }
    if sh.login {
        if let Some(motd) = fs::read_to_string("/etc/motd") {
            write(1, &motd);
        }
    }
    for rc in [String::from("/etc/hshrc"), format!("{}/.hshrc", sh.home().trim_end_matches('/'))] {
        if sys::stat(&rc).is_ok() {
            sh.run_script(&rc, &[], Io::STD);
        }
    }
    let mut pending = String::new();
    loop {
        if let Some(code) = sh.exit {
            return code;
        }
        let p = if pending.is_empty() { prompt(sh) } else { String::from("\x1b[90m>\x1b[0m ") };
        let history = sh.history.clone();
        let result = {
            let sh_ref: &Shell = sh;
            line::read_line(&p, &history, &|before| completions(sh_ref, before))
        };
        match result {
            line::ReadResult::Eof => {
                if sh.login {
                    write(1, "logout\n");
                }
                return sh.status;
            }
            line::ReadResult::Interrupted => {
                pending.clear();
                sh.status = 130;
                continue;
            }
            line::ReadResult::Line(text) => {
                if !pending.is_empty() {
                    pending.push('\n');
                }
                pending.push_str(&text);
                if parser::needs_more(&pending) {
                    continue;
                }
                let command = core::mem::take(&mut pending);
                if command.trim().is_empty() {
                    continue;
                }
                if sh.history.last() != Some(&command) {
                    sh.history.push(command.clone());
                    if !command.starts_with(' ') && !command.contains('\n') {
                        fs::append(&history_path(sh), format!("{}\n", command).as_bytes());
                    }
                }
                sh.run_source(&command, Io::STD);
                sys::set_foreground(0);
            }
        }
    }
}

fn main() -> i32 {
    let args: Vec<String> = env::args().iter().skip(1).cloned().collect();
    let mut sh = Shell::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "--login" => {
                sh.login = true;
                i += 1;
            }
            "-e" => {
                sh.errexit = true;
                i += 1;
            }
            "-x" => {
                sh.xtrace = true;
                i += 1;
            }
            "-c" => {
                let Some(command) = args.get(i + 1) else {
                    write(2, "hsh: -c needs a command\n");
                    return 2;
                };
                sh.args = alloc::vec![String::from("hsh")];
                sh.args.extend_from_slice(&args[i + 2..]);
                let status = sh.run_source(command, Io::STD);
                return sh.exit.unwrap_or(status);
            }
            "--version" => {
                write(1, "hsh 0.6.1 (HamixOS shell)\n");
                return 0;
            }
            _ => break,
        }
    }
    if i < args.len() {
        let status = sh.run_script(&args[i], &args[i + 1..], Io::STD);
        return sh.exit.unwrap_or(status);
    }
    if !sys::isatty(0) {
        let data = builtins::text::read_all(0);
        let text = String::from_utf8_lossy(&data).into_owned();
        let status = sh.run_source(&text, Io::STD);
        return sh.exit.unwrap_or(status);
    }
    interactive(&mut sh)
}

entry!(main);
