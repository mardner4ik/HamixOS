use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

const CONFIG_PATH: &str = "/etc/hamix/networks.conf";

#[derive(Clone)]
pub struct Known {
    pub ssid: String,
    pub passphrase: String,
    pub automatic: bool,
    pub last_used: u64,
}

static STORE: Mutex<Vec<Known>> = Mutex::new(Vec::new());
static LOADED: Mutex<bool> = Mutex::new(false);

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

pub fn load() {
    let mut loaded = LOADED.lock();
    if *loaded {
        return;
    }
    *loaded = true;
    let Some(bytes) = crate::fs::VFS.lock().as_mut().and_then(|v| v.read(0, CONFIG_PATH).ok()) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut store = STORE.lock();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let (Some(ssid), Some(passphrase)) = (fields.next(), fields.next()) else {
            continue;
        };
        store.push(Known {
            ssid: unescape(ssid),
            passphrase: unescape(passphrase),
            automatic: fields.next().map(|v| v != "0").unwrap_or(true),
            last_used: fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        });
    }
}

fn persist(store: &[Known]) {
    let mut text = String::new();
    for entry in store {
        text.push_str(&escape(&entry.ssid));
        text.push('\t');
        text.push_str(&escape(&entry.passphrase));
        text.push('\t');
        text.push_str(if entry.automatic { "1" } else { "0" });
        text.push('\t');
        text.push_str(&entry.last_used.to_string());
        text.push('\n');
    }
    let mut guard = crate::fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let _ = vfs.mkdir_all("/etc/hamix", 0);
    if vfs.write(0, CONFIG_PATH, text.as_bytes(), false, 0).is_ok() {
        let _ = vfs.chmod(0, CONFIG_PATH, 0, 0o600);
    }
    drop(guard);
    crate::fs::request_sync();
}

pub fn remember(ssid: &str, passphrase: &str) {
    load();
    let stamp = crate::drivers::rtc::now();
    let mut store = STORE.lock();
    match store.iter_mut().find(|e| e.ssid == ssid) {
        Some(entry) => {
            entry.passphrase = String::from(passphrase);
            entry.automatic = true;
            entry.last_used = stamp;
        }
        None => store.push(Known { ssid: String::from(ssid), passphrase: String::from(passphrase), automatic: true, last_used: stamp }),
    }
    let snapshot: Vec<Known> = store.clone();
    drop(store);
    persist(&snapshot);
}

pub fn forget(ssid: &str) -> bool {
    load();
    let mut store = STORE.lock();
    let before = store.len();
    store.retain(|e| e.ssid != ssid);
    let removed = store.len() != before;
    let snapshot: Vec<Known> = store.clone();
    drop(store);
    if removed {
        persist(&snapshot);
    }
    removed
}

pub fn set_automatic(ssid: &str, automatic: bool) -> bool {
    load();
    let mut store = STORE.lock();
    let Some(entry) = store.iter_mut().find(|e| e.ssid == ssid) else {
        return false;
    };
    entry.automatic = automatic;
    let snapshot: Vec<Known> = store.clone();
    drop(store);
    persist(&snapshot);
    true
}

pub fn list() -> Vec<Known> {
    load();
    STORE.lock().clone()
}

pub fn passphrase_for(ssid: &str) -> Option<String> {
    load();
    STORE.lock().iter().find(|e| e.ssid == ssid && e.automatic).map(|e| e.passphrase.clone())
}

pub fn listing_text() -> String {
    let mut out = String::new();
    for entry in list() {
        out.push_str(&escape(&entry.ssid));
        out.push('\t');
        out.push_str(if entry.automatic { "1" } else { "0" });
        out.push('\t');
        out.push_str(if entry.passphrase.is_empty() { "open" } else { "saved" });
        out.push('\t');
        out.push_str(&entry.last_used.to_string());
        out.push('\n');
    }
    out
}

const MODE_PATH: &str = "/etc/hamix/netmode.conf";

pub fn saved_mode() -> Option<u8> {
    let bytes = crate::fs::VFS.lock().as_mut().and_then(|v| v.read(0, MODE_PATH).ok())?;
    match String::from_utf8_lossy(&bytes).trim() {
        "ethernet" => Some(super::MODE_ETHERNET),
        "wifi" => Some(super::MODE_WIFI),
        "off" => Some(super::MODE_OFF),
        _ => None,
    }
}

pub fn save_mode(mode: u8) {
    let text = alloc::format!("{}\n", super::mode_name(mode));
    let mut guard = crate::fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let _ = vfs.mkdir_all("/etc/hamix", 0);
    let _ = vfs.write(0, MODE_PATH, text.as_bytes(), false, 0);
    drop(guard);
    crate::fs::request_sync();
}
