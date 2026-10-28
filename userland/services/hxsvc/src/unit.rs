use alloc::string::String;
use alloc::vec::Vec;

use hxstd::{fs, sys};

pub const UNIT_DIR: &str = "/etc/hamix/services";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    No,
    OnFailure,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Boot,
    Manual,
    Vt,
    Demand,
}

#[derive(Clone)]
pub struct Unit {
    pub name: String,
    pub description: String,
    pub exec: Vec<String>,
    pub after: Vec<String>,
    pub restart: Restart,
    pub start: Start,
    pub user: Option<String>,
    pub memory_max_kb: u64,
    pub provides: Vec<String>,
    pub linux_layer: bool,
    pub console: bool,
    pub template: bool,
    pub vt: Vec<String>,
}

impl Unit {
    pub fn command(&self, instance: Option<&str>) -> Vec<String> {
        self.exec.iter().map(|part| match instance {
            Some(i) => part.replace("%i", i),
            None => part.clone(),
        }).collect()
    }
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => {
                if !current.is_empty() {
                    words.push(core::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn parse(name: &str, text: &str) -> Result<Unit, String> {
    let template = name.ends_with('@');
    let mut unit = Unit {
        name: String::from(name),
        description: String::new(),
        exec: Vec::new(),
        after: Vec::new(),
        restart: Restart::No,
        start: Start::Boot,
        user: None,
        memory_max_kb: 0,
        provides: Vec::new(),
        linux_layer: false,
        console: false,
        template,
        vt: Vec::new(),
    };
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(alloc::format!("line {}: expected key = value", number + 1));
        };
        let value = value.trim();
        match key.trim() {
            "description" => unit.description = String::from(value.trim_matches('"')),
            "exec" => unit.exec = split_words(value),
            "after" => unit.after = split_words(value),
            "provides" => unit.provides = split_words(value),
            "user" => unit.user = Some(String::from(value)),
            "memory_max" => unit.memory_max_kb = value.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse::<u64>().unwrap_or(0) * if value.ends_with('M') { 1024 } else { 1 },
            "layer" => unit.linux_layer = value == "linux",
            "console" => unit.console = matches!(value, "yes" | "true" | "1"),
            "vt" => unit.vt = split_words(value),
            "restart" => {
                unit.restart = match value {
                    "no" => Restart::No,
                    "on-failure" => Restart::OnFailure,
                    "always" => Restart::Always,
                    other => return Err(alloc::format!("line {}: unknown restart policy {}", number + 1, other)),
                }
            }
            "start" => {
                unit.start = match value {
                    "boot" => Start::Boot,
                    "manual" => Start::Manual,
                    "vt" => Start::Vt,
                    "demand" => Start::Demand,
                    other => return Err(alloc::format!("line {}: unknown start mode {}", number + 1, other)),
                }
            }
            other => return Err(alloc::format!("line {}: unknown key {}", number + 1, other)),
        }
    }
    if unit.exec.is_empty() {
        return Err(String::from("no exec line"));
    }
    if unit.start == Start::Vt {
        unit.console = true;
    }
    Ok(unit)
}

pub fn load_all(log: &mut dyn FnMut(String)) -> Vec<Unit> {
    let mut units = Vec::new();
    let Some(entries) = sys::read_dir(UNIT_DIR) else {
        return units;
    };
    let mut names: Vec<String> = entries.into_iter().filter(|e| !e.is_dir && e.name.ends_with(".svc")).map(|e| e.name).collect();
    names.sort();
    for file in names {
        let name = file.trim_end_matches(".svc");
        let Some(text) = fs::read_to_string(&alloc::format!("{}/{}", UNIT_DIR, file)) else {
            continue;
        };
        match parse(name, &text) {
            Ok(unit) => units.push(unit),
            Err(e) => log(alloc::format!("{}: {}", file, e)),
        }
    }
    units
}

pub fn boot_order(units: &[Unit]) -> Vec<usize> {
    let mut order = Vec::new();
    let mut state = alloc::vec![0u8; units.len()];
    fn visit(i: usize, units: &[Unit], state: &mut Vec<u8>, order: &mut Vec<usize>) {
        if state[i] != 0 {
            return;
        }
        state[i] = 1;
        for dep in units[i].after.iter() {
            if let Some(j) = units.iter().position(|u| &u.name == dep) {
                visit(j, units, state, order);
            }
        }
        state[i] = 2;
        order.push(i);
    }
    for i in 0..units.len() {
        visit(i, units, &mut state, &mut order);
    }
    order
}
