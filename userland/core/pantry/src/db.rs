use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::{fs, sys};

pub const SYSROOT: &str = "/opt/linux";
pub const DB_PATH: &str = "/var/lib/pantry/installed";
pub const WORLD_PATH: &str = "/etc/pantry/world";
pub const REPOS_PATH: &str = "/etc/pantry/repositories";
pub const KEYS_DIR: &str = "/etc/pantry/keys";
pub const CACHE_DIR: &str = "/var/cache/pantry";
pub const PROVIDED_PATH: &str = "/etc/pantry/provided";
pub const SYSTEM_REPO: &str = "system";
pub const LINK_MAGIC: &[u8] = b"\x7fHXLINK\n";

#[derive(Clone, Default)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub repo: String,
    pub description: String,
    pub depends: Vec<String>,
    pub provides: Vec<String>,
    pub files: Vec<String>,
    pub scripts: Vec<String>,
    pub triggers: Vec<String>,
}

impl Installed {
    pub fn is_system(&self) -> bool {
        self.repo == SYSTEM_REPO
    }
}

pub struct Db {
    pub packages: Vec<Installed>,
    pub world: Vec<String>,
}

pub fn mkdir_p(path: &str) {
    let mut acc = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        acc.push('/');
        acc.push_str(part);
        if sys::stat(&acc).is_err() {
            sys::mkdir(&acc);
        }
    }
}

pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "/",
    }
}

pub fn write_file(path: &str, data: &[u8]) -> bool {
    mkdir_p(parent(path));
    fs::write(path, data)
}

impl Db {
    pub fn load() -> Db {
        let mut packages = Vec::new();
        if let Some(text) = fs::read_to_string(PROVIDED_PATH) {
            for block in text.split("\n\n") {
                let mut pkg = Installed { repo: String::from(SYSTEM_REPO), ..Default::default() };
                for line in block.lines() {
                    match line.split_once(':') {
                        Some(("P", v)) => pkg.name = v.to_string(),
                        Some(("V", v)) => pkg.version = v.to_string(),
                        Some(("T", v)) => pkg.description = v.to_string(),
                        Some(("p", v)) => pkg.provides = v.split_whitespace().map(|s| s.to_string()).collect(),
                        _ => {}
                    }
                }
                if !pkg.name.is_empty() {
                    packages.push(pkg);
                }
            }
        }
        if let Some(text) = fs::read_to_string(DB_PATH) {
            for block in text.split("\n\n") {
                let mut pkg = Installed::default();
                for line in block.lines() {
                    let Some((key, value)) = line.split_once(':') else {
                        continue;
                    };
                    let words = || value.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>();
                    match key {
                        "P" => pkg.name = value.to_string(),
                        "V" => pkg.version = value.to_string(),
                        "R" => pkg.repo = value.to_string(),
                        "T" => pkg.description = value.to_string(),
                        "D" => pkg.depends = words(),
                        "p" => pkg.provides = words(),
                        "F" => pkg.files.push(value.to_string()),
                        "X" => pkg.scripts.push(value.to_string()),
                        "G" => pkg.triggers = words(),
                        _ => {}
                    }
                }
                if !pkg.name.is_empty() && !packages.iter().any(|p: &Installed| p.name == pkg.name) {
                    packages.push(pkg);
                }
            }
        }
        let world = fs::read_to_string(WORLD_PATH).map(|t| t.split_whitespace().map(|s| s.to_string()).collect()).unwrap_or_default();
        Db { packages, world }
    }

    pub fn save(&self) -> bool {
        let mut text = String::new();
        for pkg in self.packages.iter().filter(|p| !p.is_system()) {
            text.push_str(&format!("P:{}\nV:{}\nR:{}\nT:{}\n", pkg.name, pkg.version, pkg.repo, pkg.description));
            if !pkg.depends.is_empty() {
                text.push_str(&format!("D:{}\n", pkg.depends.join(" ")));
            }
            if !pkg.provides.is_empty() {
                text.push_str(&format!("p:{}\n", pkg.provides.join(" ")));
            }
            for script in &pkg.scripts {
                text.push_str(&format!("X:{}\n", script));
            }
            if !pkg.triggers.is_empty() {
                text.push_str(&format!("G:{}\n", pkg.triggers.join(" ")));
            }
            for file in &pkg.files {
                text.push_str(&format!("F:{}\n", file));
            }
            text.push('\n');
        }
        let mut world = self.world.clone();
        world.sort();
        world.dedup();
        let ok = write_file(DB_PATH, text.as_bytes()) && write_file(WORLD_PATH, format!("{}\n", world.join("\n")).as_bytes());
        sys::sync();
        ok
    }

    pub fn find(&self, name: &str) -> Option<&Installed> {
        self.packages.iter().find(|p| p.name == name)
    }

    pub fn owner_of(&self, file: &str) -> Option<&Installed> {
        self.packages.iter().find(|p| p.files.iter().any(|f| f == file))
    }

    pub fn remove(&mut self, name: &str) -> Option<Installed> {
        let index = self.packages.iter().position(|p| p.name == name)?;
        Some(self.packages.remove(index))
    }
}
