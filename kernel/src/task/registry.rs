use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::without_interrupts;

pub const REGISTRY_PATH: &str = "/etc/commands";

#[derive(Clone)]
pub struct Command {
    pub name: String,
    pub path: String,
    pub args: Vec<String>,
    pub owner: u32,
}

static REGISTRY: Mutex<BTreeMap<String, Command>> = Mutex::new(BTreeMap::new());

pub const RESERVED: &[&str] = &[
    "help", "clear", "edit", "echo", "uname", "whoami", "id", "meminfo", "uptime", "pwd", "ls", "cd",
    "cat", "mkdir", "touch", "rm", "chmod", "chown", "tree", "fb", "gpuinfo", "drivers", "usb", "mouse",
    "exec", "startx", "diskls", "diskcat", "hostname", "date", "sudo", "passwd", "useradd", "halt",
    "poweroff", "reboot", "version", "cpuinfo", "history", "dmesg", "hexdump", "inport", "outport",
    "regs", "alloctest", "crash", "logout", "exit", "ps", "kill", "cmd", "sync", "hext", "free",
];

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !name.starts_with('.')
}

fn valid_field(value: &str) -> bool {
    !value.contains('\t') && !value.contains('\n') && !value.contains('\0')
}

pub fn load() {
    let text = {
        let mut guard = crate::fs::VFS.lock();
        guard
            .as_mut()
            .and_then(|vfs| vfs.read(vfs.root_id(), REGISTRY_PATH).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    let mut map = BTreeMap::new();
    if let Some(text) = text {
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 3 || !valid_name(fields[0]) {
                continue;
            }
            let owner = fields[1].parse().unwrap_or(0);
            map.insert(
                fields[0].to_string(),
                Command {
                    name: fields[0].to_string(),
                    owner,
                    path: fields[2].to_string(),
                    args: fields[3..].iter().map(|s| s.to_string()).collect(),
                },
            );
        }
    }
    without_interrupts(|| *REGISTRY.lock() = map);
}

fn save() {
    let mut text = String::new();
    for command in list() {
        text.push_str(&format!("{}\t{}\t{}", command.name, command.owner, command.path));
        for arg in &command.args {
            text.push('\t');
            text.push_str(arg);
        }
        text.push('\n');
    }
    if let Some(vfs) = crate::fs::VFS.lock().as_mut() {
        let root = vfs.root_id();
        let _ = vfs.write(root, REGISTRY_PATH, text.as_bytes(), false, 0);
    }
}

pub fn register(name: &str, path: &str, args: Vec<String>, owner: u32) -> Result<(), &'static str> {
    if !valid_name(name) {
        return Err("invalid command name");
    }
    if RESERVED.contains(&name) {
        return Err("name is reserved by a shell builtin");
    }
    if path.is_empty() || !valid_field(path) || !args.iter().all(|a| valid_field(a)) {
        return Err("invalid target");
    }
    let exists = {
        let guard = crate::fs::VFS.lock();
        guard.as_ref().map(|vfs| vfs.exists(vfs.root_id(), path)).unwrap_or(false)
    };
    if !exists {
        return Err("target does not exist");
    }
    without_interrupts(|| {
        let mut registry = REGISTRY.lock();
        if let Some(existing) = registry.get(name) {
            if existing.owner != owner && owner != 0 {
                return Err("command belongs to another user");
            }
        }
        registry.insert(
            name.to_string(),
            Command { name: name.to_string(), path: path.to_string(), args, owner },
        );
        Ok(())
    })?;
    save();
    Ok(())
}

pub fn unregister(name: &str, uid: u32) -> Result<(), &'static str> {
    without_interrupts(|| {
        let mut registry = REGISTRY.lock();
        match registry.get(name) {
            None => Err("no such command"),
            Some(command) if command.owner != uid && uid != 0 => Err("command belongs to another user"),
            Some(_) => {
                registry.remove(name);
                Ok(())
            }
        }
    })?;
    save();
    Ok(())
}

pub fn resolve(name: &str) -> Option<Command> {
    without_interrupts(|| REGISTRY.lock().get(name).cloned())
}

pub fn list() -> Vec<Command> {
    without_interrupts(|| REGISTRY.lock().values().cloned().collect())
}

pub fn render() -> String {
    let mut out = String::new();
    for command in list() {
        out.push_str(&command.name);
        out.push('\t');
        out.push_str(&command.path);
        for arg in &command.args {
            out.push('\t');
            out.push_str(arg);
        }
        out.push('\n');
    }
    out
}
