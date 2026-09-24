pub mod basic;
pub mod disk;
pub mod files;
pub mod net;
pub mod system;
pub mod text;
pub mod users;

use alloc::string::String;
use alloc::vec::Vec;

use crate::shell::{Io, Shell};

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

pub struct Builtin {
    pub name: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
    pub group: &'static str,
    pub run: fn(&mut Shell, &[String], Io) -> i32,
}

pub fn table() -> Vec<Builtin> {
    let mut all = Vec::new();
    all.extend(basic::commands());
    all.extend(files::commands());
    all.extend(text::commands());
    all.extend(system::commands());
    all.extend(users::commands());
    all.extend(disk::commands());
    all.extend(net::commands());
    all
}

pub fn is_builtin(name: &str) -> bool {
    table().iter().any(|b| b.name == name)
}

pub fn run(shell: &mut Shell, argv: &[String], io: Io) -> i32 {
    let name = argv[0].as_str();
    match table().into_iter().find(|b| b.name == name) {
        Some(builtin) => (builtin.run)(shell, argv, io),
        None => 127,
    }
}

pub fn names() -> Vec<&'static str> {
    table().iter().map(|b| b.name).collect()
}
