use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs;

pub struct Permission {
    pub group: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub superpermission: bool,
}

pub const PERMISSIONS: &[Permission] = &[
    Permission { group: "inet", label: "Internet", description: "Connect to the network and the internet", superpermission: false },
    Permission { group: "audio", label: "Sound", description: "Play and record sound through the sound card", superpermission: false },
    Permission { group: "video", label: "Direct graphics access", description: "Use the GPU and the screen device directly, as games and video players with 3D do. Windows still show without it", superpermission: false },
    Permission { group: "input", label: "Direct input access", description: "Read every keyboard and mouse event, even for other windows. Typing into the app's own windows works without it", superpermission: false },
    Permission { group: "power", label: "Power off and restart", description: "Shut down or restart the computer without a password", superpermission: true },
    Permission { group: "sysconf", label: "Change system settings", description: "Change settings for the whole computer without a password", superpermission: true },
];

pub const FOREIGN_DEFAULT: &[&str] = &["inet", "audio", "video"];

pub const GRANTS_DIR: &str = "/etc/hamix/permissions";

pub const APPLICATION_DIRS: &[&str] = &["/usr/share/applications", "/usr/local/share/applications", "/linux/usr/share/applications", "/linux/usr/local/share/applications"];

pub fn find(group: &str) -> Option<&'static Permission> {
    PERMISSIONS.iter().find(|p| p.group == group)
}

pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.starts_with('.') && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn list(value: &str) -> Vec<String> {
    value.split(';').map(|s| s.trim()).filter(|s| find(s).is_some()).map(String::from).collect()
}

pub struct Declaration {
    pub path: String,
    pub name: String,
    pub icon: String,
    pub requested: Option<Vec<String>>,
    pub required: Vec<String>,
    pub hidden: bool,
}

pub fn desktop_file(app: &str) -> Option<String> {
    if !valid_app_id(app) {
        return None;
    }
    APPLICATION_DIRS.iter().map(|dir| format!("{}/{}.desktop", dir, app)).find(|p| fs::exists(p))
}

pub fn declaration(app: &str) -> Option<Declaration> {
    let path = desktop_file(app)?;
    let text = fs::read_to_string(&path)?;
    let mut in_entry = false;
    let mut name = String::new();
    let mut icon = String::new();
    let mut requested = None;
    let mut required = Vec::new();
    let mut hidden = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "Name" => name = String::from(value.trim()),
            "Icon" => icon = String::from(value.trim()),
            "X-Hamix-Permissions" => requested = Some(list(value)),
            "X-Hamix-Permissions-Required" => required = list(value),
            "NoDisplay" | "Hidden" => hidden |= value.trim() == "true",
            _ => {}
        }
    }
    Some(Declaration { path, name, icon, requested, required, hidden })
}

pub fn requested(app: &str) -> Vec<String> {
    match declaration(app).and_then(|d| d.requested) {
        Some(list) => list,
        None => FOREIGN_DEFAULT.iter().map(|s| String::from(*s)).collect(),
    }
}

pub fn grants_file(root: &str, app: &str) -> String {
    format!("{}{}/{}", root.trim_end_matches('/'), GRANTS_DIR, app)
}

pub fn stored(root: &str, app: &str) -> Option<Vec<String>> {
    if !valid_app_id(app) {
        return None;
    }
    let text = fs::read_to_string(&grants_file(root, app))?;
    Some(text.lines().map(|l| l.trim()).filter(|l| find(l).is_some()).map(String::from).collect())
}

pub fn granted(app: &str) -> Vec<String> {
    let requested = requested(app);
    let required = declaration(app).map(|d| d.required).unwrap_or_default();
    let mut list = match stored("", app) {
        Some(stored) => stored.into_iter().filter(|g| requested.contains(g)).collect(),
        None => requested,
    };
    for group in required {
        if !list.contains(&group) {
            list.push(group);
        }
    }
    list
}

pub fn store(root: &str, app: &str, groups: &[String]) -> Result<(), &'static str> {
    if !valid_app_id(app) {
        return Err("invalid application id");
    }
    let dir = format!("{}{}", root.trim_end_matches('/'), GRANTS_DIR);
    let _ = crate::sys::mkdir(&dir);
    let mut out = String::new();
    for group in groups.iter().filter(|g| find(g).is_some()) {
        out.push_str(group);
        out.push('\n');
    }
    let path = grants_file(root, app);
    if !fs::write(&path, out.as_bytes()) {
        return Err("cannot write the permission file");
    }
    crate::sys::chmod(&path, 0o644);
    Ok(())
}

pub fn reset(root: &str, app: &str) -> bool {
    valid_app_id(app) && crate::sys::unlink(&grants_file(root, app)) >= 0
}

pub fn applications() -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for dir in APPLICATION_DIRS {
        for entry in crate::sys::read_dir(dir).unwrap_or_default() {
            if let Some(id) = entry.name.strip_suffix(".desktop") {
                if valid_app_id(id) && !ids.iter().any(|i| i == id) {
                    ids.push(String::from(id));
                }
            }
        }
    }
    ids.sort();
    ids
}
