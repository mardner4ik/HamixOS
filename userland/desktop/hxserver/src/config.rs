use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxclient::ui;

pub struct Config {
    pub wallpaper: Option<String>,
    pub dock: Option<Vec<String>>,
    pub desktop: Vec<(String, i32, i32)>,
    pub desktop_removed: Vec<String>,
    pub desktop_added: Vec<String>,
    pub autohide: bool,
    pub pageflip: bool,
    pub dock_size: i32,
    pub dock_limit: usize,
}

pub const DOCK_SIZE_MIN: i32 = 32;
pub const DOCK_SIZE_MAX: i32 = 80;
pub const DOCK_SIZE_DEFAULT: i32 = 48;
pub const DOCK_LIMIT_MIN: usize = 4;
pub const DOCK_LIMIT_MAX: usize = 30;
pub const DOCK_LIMIT_DEFAULT: usize = 12;

pub fn load() -> Config {
    let entries = ui::read_nook_config();
    let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let list = |value: Option<String>| value.map(|v| v.split(',').filter(|s| !s.is_empty()).map(String::from).collect::<Vec<_>>());
    let mut desktop = Vec::new();
    for item in get("desktop_positions").unwrap_or_default().split(';') {
        let mut parts = item.split(':');
        let (Some(name), Some(pos)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some((x, y)) = pos.split_once(',') else {
            continue;
        };
        if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
            desktop.push((String::from(name), x, y));
        }
    }
    Config {
        wallpaper: get("wallpaper"),
        dock: list(get("dock")),
        desktop,
        desktop_removed: list(get("desktop_removed")).unwrap_or_default(),
        desktop_added: list(get("desktop_added")).unwrap_or_default(),
        autohide: get("dock_autohide").map(|v| v != "no").unwrap_or(true),
        pageflip: get("pageflip").map(|v| v == "yes").unwrap_or(false),
        dock_size: get("dock_size").and_then(|v| v.parse().ok()).unwrap_or(DOCK_SIZE_DEFAULT).clamp(DOCK_SIZE_MIN, DOCK_SIZE_MAX),
        dock_limit: get("dock_limit").and_then(|v| v.parse().ok()).unwrap_or(DOCK_LIMIT_DEFAULT).clamp(DOCK_LIMIT_MIN, DOCK_LIMIT_MAX),
    }
}

pub fn save_dock(names: &[String]) {
    ui::write_nook_config("dock", &names.join(","));
}

pub fn save_desktop(positions: &[(String, i32, i32)], removed: &[String], added: &[String]) {
    let text: Vec<String> = positions.iter().map(|(n, x, y)| format!("{}:{},{}", n, x, y)).collect();
    ui::write_nook_config("desktop_positions", &text.join(";"));
    ui::write_nook_config("desktop_removed", &removed.join(","));
    ui::write_nook_config("desktop_added", &added.join(","));
}
