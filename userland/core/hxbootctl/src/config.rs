use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct Entry {
    pub lines: Vec<String>,
}

pub struct Config {
    pub head: Vec<String>,
    pub entries: Vec<Entry>,
}

fn word(line: &str) -> (&str, &str) {
    let line = line.trim();
    match line.find(char::is_whitespace) {
        Some(i) => (&line[..i], line[i..].trim()),
        None => (line, ""),
    }
}

impl Entry {
    pub fn new(name: &str) -> Entry {
        Entry { lines: alloc::vec![format!("entry {}", name)] }
    }

    pub fn name(&self) -> &str {
        word(&self.lines[0]).1
    }

    pub fn rename(&mut self, name: &str) {
        self.lines[0] = format!("entry {}", name);
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.lines.iter().skip(1).map(|l| word(l)).find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let line = if value.is_empty() { key.to_string() } else { format!("{} {}", key, value) };
        match self.lines.iter().position(|l| word(l).0 == key) {
            Some(i) if i > 0 => self.lines[i] = line,
            _ => self.lines.push(line),
        }
    }

    pub fn modules(&self) -> Vec<(String, bool)> {
        self.lines.iter().map(|l| word(l)).filter(|(k, _)| *k == "module" || *k == "module?").map(|(k, v)| (v.to_string(), k == "module?")).collect()
    }

    pub fn add_module(&mut self, path: &str, name: &str, optional: bool) {
        self.lines.push(format!("{} {} {}", if optional { "module?" } else { "module" }, path, name));
    }

    pub fn kind(&self) -> &'static str {
        if self.value("reboot").is_some() || self.lines.iter().any(|l| l.trim() == "reboot") {
            "reboot"
        } else if self.value("chain").is_some() {
            "chain"
        } else {
            "kernel"
        }
    }

    pub fn describe(&self) -> String {
        match self.kind() {
            "reboot" => String::from("restart the computer"),
            "chain" => format!("chain {}", self.value("chain").unwrap_or("")),
            _ => format!("kernel {}", self.value("kernel").unwrap_or("(missing)")),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut config = Config { head: Vec::new(), entries: Vec::new() };
        for line in text.lines() {
            if word(line).0 == "entry" {
                config.entries.push(Entry { lines: alloc::vec![line.trim().to_string()] });
                continue;
            }
            match config.entries.last_mut() {
                Some(entry) => {
                    if !line.trim().is_empty() {
                        entry.lines.push(line.trim().to_string());
                    }
                }
                None => config.head.push(line.to_string()),
            }
        }
        while config.head.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            config.head.pop();
        }
        config
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        for line in &self.head {
            out.push_str(line);
            out.push('\n');
        }
        for entry in &self.entries {
            out.push('\n');
            for line in &entry.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.head.iter().map(|l| word(l)).find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let line = format!("{} {}", key, value);
        match self.head.iter().position(|l| word(l).0 == key) {
            Some(i) => self.head[i] = line,
            None => self.head.push(line),
        }
    }

    pub fn default_index(&self) -> usize {
        self.get("default").and_then(|v| v.parse().ok()).filter(|&d: &usize| d < self.entries.len()).unwrap_or(0)
    }

    pub fn insert(&mut self, entry: Entry) -> usize {
        let at = match self.entries.last() {
            Some(last) if last.kind() == "reboot" => self.entries.len() - 1,
            _ => self.entries.len(),
        };
        self.entries.insert(at, entry);
        at
    }

    pub fn find(&self, spec: &str) -> Result<usize, String> {
        if let Ok(n) = spec.parse::<usize>() {
            if n >= 1 && n <= self.entries.len() {
                return Ok(n - 1);
            }
            return Err(format!("there is no entry {} (1..{})", n, self.entries.len()));
        }
        let wanted = spec.to_ascii_lowercase();
        let matches: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].name().to_ascii_lowercase().contains(&wanted)).collect();
        match matches.len() {
            1 => Ok(matches[0]),
            0 => Err(format!("no entry is called \"{}\"", spec)),
            _ => Err(format!("\"{}\" matches several entries; use the number", spec)),
        }
    }
}
