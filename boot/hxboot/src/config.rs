pub const MAX_ENTRIES: usize = 20;
pub const MAX_MODULES: usize = 6;

#[derive(Clone, Copy)]
pub struct ModuleSpec {
    pub path: &'static str,
    pub name: &'static str,
    pub optional: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Kernel,
    Chain { drive: u8, signature: u32, partition: u8 },
    Reboot,
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub name: &'static str,
    pub kind: Kind,
    pub kernel: &'static str,
    pub cmdline: &'static str,
    pub modules: [ModuleSpec; MAX_MODULES],
    pub module_count: usize,
    pub text: bool,
}

pub struct Config {
    pub entries: [Entry; MAX_ENTRIES],
    pub count: usize,
    pub timeout: u32,
    pub default: usize,
    pub title: &'static str,
    pub ventoy_param: &'static str,
    pub probe: bool,
}

const EMPTY_MODULE: ModuleSpec = ModuleSpec { path: "", name: "", optional: false };
pub const EMPTY_ENTRY: Entry = Entry { name: "", kind: Kind::Kernel, kernel: "", cmdline: "", modules: [EMPTY_MODULE; MAX_MODULES], module_count: 0, text: false };

fn split_word(text: &'static str) -> (&'static str, &'static str) {
    let text = text.trim();
    match text.find(char::is_whitespace) {
        Some(i) => (&text[..i], text[i..].trim()),
        None => (text, ""),
    }
}

fn number(text: &str) -> Option<u32> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn parse_chain(rest: &str) -> Option<Kind> {
    let (mut drive, mut signature, mut partition) = (0u8, 0u32, 0u8);
    for word in rest.split_whitespace() {
        let (key, value) = word.split_once('=')?;
        match key {
            "drive" => {
                drive = match value.strip_prefix("hd") {
                    Some(n) => 0x80 + n.parse::<u8>().ok()?,
                    None => number(value)? as u8,
                }
            }
            "signature" => signature = u32::from_str_radix(value.trim_start_matches("0x"), 16).ok()?,
            "partition" => partition = value.parse().ok()?,
            _ => return None,
        }
    }
    if (drive < 0x80 && signature == 0) || partition > 4 {
        return None;
    }
    Some(Kind::Chain { drive, signature, partition })
}

fn yes(text: &str) -> bool {
    matches!(text, "yes" | "on" | "true" | "1")
}

impl Config {
    pub fn parse(text: &'static str) -> Config {
        let mut config = Config { entries: [EMPTY_ENTRY; MAX_ENTRIES], count: 0, timeout: 3, default: 0, title: "HamixOS", ventoy_param: "", probe: false };
        let mut broken = [false; MAX_ENTRIES];
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, rest) = split_word(line);
            match key {
                "timeout" => config.timeout = rest.parse().unwrap_or(3),
                "default" => config.default = rest.parse().unwrap_or(0),
                "title" => config.title = rest,
                "ventoy-param" => config.ventoy_param = rest,
                "probe" => config.probe = yes(rest),
                "entry" if config.count < MAX_ENTRIES => {
                    config.entries[config.count] = Entry { name: rest, ..EMPTY_ENTRY };
                    config.count += 1;
                }
                _ if config.count == 0 => {}
                "kernel" => {
                    let entry = &mut config.entries[config.count - 1];
                    let (path, cmdline) = split_word(rest);
                    entry.kernel = path;
                    entry.cmdline = cmdline;
                }
                "text" => config.entries[config.count - 1].text = true,
                "reboot" => config.entries[config.count - 1].kind = Kind::Reboot,
                "chain" => match parse_chain(rest) {
                    Some(kind) => config.entries[config.count - 1].kind = kind,
                    None => broken[config.count - 1] = true,
                },
                "module" | "module?" => {
                    let entry = &mut config.entries[config.count - 1];
                    if entry.module_count < MAX_MODULES {
                        let (path, name) = split_word(rest);
                        entry.modules[entry.module_count] = ModuleSpec { path, name, optional: key == "module?" };
                        entry.module_count += 1;
                    }
                }
                _ => {}
            }
        }
        let mut kept = 0;
        for i in 0..config.count {
            let e = config.entries[i];
            if !broken[i] && (e.kind != Kind::Kernel || !e.kernel.is_empty()) {
                config.entries[kept] = e;
                kept += 1;
            }
        }
        config.count = kept;
        if config.default >= config.count {
            config.default = 0;
        }
        config
    }

    pub fn push(&mut self, entry: Entry) -> bool {
        if self.count >= MAX_ENTRIES {
            return false;
        }
        let reboot_last = self.count > 0 && self.entries[self.count - 1].kind == Kind::Reboot;
        if reboot_last {
            self.entries[self.count] = self.entries[self.count - 1];
            self.entries[self.count - 1] = entry;
        } else {
            self.entries[self.count] = entry;
        }
        self.count += 1;
        true
    }
}
