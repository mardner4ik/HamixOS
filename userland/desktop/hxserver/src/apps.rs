use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, UnsafeCell};
use hxstd::{fs, sys};
use hxclient::ui::Ui;
use vellum::Image;

pub const NATIVE_DIRS: [&str; 2] = ["/usr/share/applications", "/usr/local/share/applications"];
pub const LINUX_ROOT: &str = "/linux";
const PROGRAM_DIRS: [&str; 7] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin", "/usr/local/bin", "/usr/games", "/usr/local/games"];
const ICON_BASE: &str = "/usr/share/icons";
pub const GENERIC_LINUX_ICON: &str = "linux-app";
const ICON_SIZES: [&str; 12] = ["48x48", "64x64", "96x96", "128x128", "256x256", "512x512", "32x32", "72x72", "24x24", "22x22", "16x16", "scalable"];
const PREFERRED_THEMES: [&str; 5] = ["hicolor", "Adwaita", "breeze", "Papirus", "HighContrast"];
const ADHOC_PREFIX: &str = "window-";
pub const ICON_THEME: &str = "/usr/share/icons/Nook Icons";
const ICON_THEME_SIZES: [&str; 3] = ["128x128", "48x48", "24x24"];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Category {
    Accessories,
    Development,
    Education,
    Games,
    Graphics,
    Internet,
    Multimedia,
    Office,
    System,
    Settings,
    Other,
}

pub const CATEGORIES: [Category; 11] = [
    Category::Accessories,
    Category::Development,
    Category::Education,
    Category::Games,
    Category::Graphics,
    Category::Internet,
    Category::Multimedia,
    Category::Office,
    Category::System,
    Category::Settings,
    Category::Other,
];

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::Accessories => "Accessories",
            Category::Development => "Development",
            Category::Education => "Education",
            Category::Games => "Games",
            Category::Graphics => "Graphics",
            Category::Internet => "Internet",
            Category::Multimedia => "Sound & Video",
            Category::Office => "Office",
            Category::System => "System",
            Category::Settings => "Settings",
            Category::Other => "Other",
        }
    }

    fn from_name(name: &str) -> Option<Category> {
        Some(match name.to_ascii_lowercase().as_str() {
            "utility" | "accessories" => Category::Accessories,
            "development" => Category::Development,
            "education" | "science" => Category::Education,
            "game" | "games" => Category::Games,
            "graphics" => Category::Graphics,
            "network" | "internet" => Category::Internet,
            "audiovideo" | "audio" | "video" | "multimedia" => Category::Multimedia,
            "office" => Category::Office,
            "system" => Category::System,
            "settings" => Category::Settings,
            _ => return None,
        })
    }

    fn classify(categories: &str) -> Category {
        let names: Vec<&str> = categories.split(';').map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
        let priority = ["Settings", "Game", "Development", "Office", "Graphics", "AudioVideo", "Audio", "Video", "Network", "Education", "Science", "System", "Utility"];
        for wanted in priority {
            if names.iter().any(|n| n.eq_ignore_ascii_case(wanted)) {
                return Category::from_name(wanted).unwrap_or(Category::Other);
            }
        }
        Category::Other
    }
}

pub struct App {
    pub key: String,
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub description: String,
    pub keywords: String,
    pub terminal: bool,
    pub linux: bool,
    pub live_only: bool,
    pub order: i32,
    pub signature: String,
    pub removed: Cell<bool>,
    pub category: Category,
    pub roles: Vec<String>,
    pub dock: bool,
    pub desktop: bool,
    pub stem: String,
    pub id: String,
    pub wm_class: String,
    pub exec_name: String,
    pub icon_name: String,
    icon_value: String,
}

struct Registry(UnsafeCell<Vec<&'static App>>);

unsafe impl Sync for Registry {}

static REGISTRY: Registry = Registry(UnsafeCell::new(Vec::new()));

fn registry() -> &'static mut Vec<&'static App> {
    unsafe { &mut *REGISTRY.0.get() }
}

pub fn count() -> usize {
    registry().len()
}

pub fn get(index: usize) -> &'static App {
    registry()[index]
}

pub fn by_key(key: &str) -> Option<usize> {
    let list = registry();
    list.iter().position(|a| a.key == key && !a.removed.get()).or_else(|| list.iter().position(|a| a.key == key))
}

pub fn alive(index: usize) -> bool {
    index < count() && !get(index).removed.get()
}

pub fn by_role(role: &str) -> Option<usize> {
    let list = registry();
    let candidates = || (0..list.len()).filter(|i| !list[*i].removed.get());
    candidates()
        .filter(|i| list[*i].roles.iter().any(|r| r == role))
        .min_by_key(|i| (list[*i].linux, list[*i].order))
        .or_else(|| candidates().find(|i| list[*i].key == role))
}

pub fn default_dock() -> Vec<usize> {
    let mut list: Vec<usize> = (0..count()).filter(|i| alive(*i) && get(*i).dock).collect();
    list.sort_by_key(|i| (get(*i).order, get(*i).name.to_lowercase()));
    list
}

pub fn default_desktop() -> Vec<usize> {
    let mut list: Vec<usize> = (0..count()).filter(|i| alive(*i) && get(*i).desktop).collect();
    list.sort_by_key(|i| (get(*i).order, get(*i).name.to_lowercase()));
    list
}

pub fn safe_icon_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 96 && !name.contains("..") && !name.starts_with('/') && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+/".contains(c))
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn parse_entry(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.entry(k.trim().to_string()).or_insert_with(|| unescape(v.trim()));
        }
    }
    map
}

fn split_exec(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    current.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if any || !current.is_empty() {
                    out.push(core::mem::take(&mut current));
                    any = false;
                }
            }
            '%' => match chars.next() {
                Some('%') => current.push('%'),
                Some(_) | None => {}
            },
            other => current.push(other),
        }
    }
    if any || !current.is_empty() {
        out.push(current);
    }
    out.retain(|a| !a.is_empty());
    out
}

fn is_file(path: &str) -> bool {
    matches!(sys::stat(path), Ok(s) if !s.is_dir())
}

fn find_program(name: &str, root: &str) -> Option<String> {
    if name.starts_with('/') {
        let path = format!("{}{}", root, name);
        return if is_file(&path) { Some(path) } else { None };
    }
    PROGRAM_DIRS.iter().map(|d| format!("{}{}/{}", root, d, name)).find(|p| is_file(p))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn icon_themes(root: &str) -> Vec<String> {
    let mut themes: Vec<String> = PREFERRED_THEMES.iter().map(|t| t.to_string()).collect();
    for entry in sys::read_dir(&format!("{}{}", root, ICON_BASE)).unwrap_or_default() {
        if entry.is_dir && !themes.contains(&entry.name) {
            themes.push(entry.name);
        }
    }
    themes
}

fn find_icon(icon: &str, themes: &[String], root: &str) -> Option<String> {
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with('/') {
        return Some(format!("{}{}", root, icon)).filter(|p| imago::extension_format(p).is_some() && is_file(p));
    }
    let name = [".png", ".svg", ".svgz", ".xpm"].iter().find_map(|ext| icon.strip_suffix(ext)).unwrap_or(icon);
    if let Some(own) = ICON_THEME_SIZES.iter().map(|size| format!("{}/{}/apps/{}.png", ICON_THEME, size, name)).find(|p| is_file(p)) {
        return Some(own);
    }
    for theme in themes {
        let theme_dir = format!("{}{}/{}", root, ICON_BASE, theme);
        if sys::stat(&theme_dir).is_err() {
            continue;
        }
        for size in ICON_SIZES {
            for group in ["apps", "applications"] {
                for ext in ["png", "svg"] {
                    let path = format!("{}/{}/{}/{}.{}", theme_dir, size, group, name, ext);
                    if is_file(&path) {
                        return Some(path);
                    }
                }
            }
        }
    }
    ["png", "svg", "jpg", "ico"]
        .iter()
        .flat_map(|ext| [format!("{}/usr/share/pixmaps/{}.{}", root, name, ext), format!("{}{}/{}.{}", root, ICON_BASE, name, ext)])
        .find(|p| is_file(p))
}

fn load_png(path: &str) -> Option<Image> {
    let bytes = fs::read_following(path)?;
    if path.ends_with(".svg") {
        return Image::decode_sized(&bytes, 128, 128);
    }
    Image::decode(&bytes)
}

pub fn fit(image: &Image, size: i32) -> Image {
    if image.w == size && image.h == size {
        return Image { w: image.w, h: image.h, px: image.px.clone() };
    }
    let scale_w = size;
    let scale_h = (image.h as i64 * size as i64 / image.w.max(1) as i64) as i32;
    let (w, h) = if scale_h <= size { (scale_w, scale_h.max(1)) } else { ((image.w as i64 * size as i64 / image.h.max(1) as i64) as i32, size) };
    let scaled = image.scaled(w, h);
    let mut px = alloc::vec![0u32; (size * size) as usize];
    let ox = (size - w) / 2;
    let oy = (size - h) / 2;
    for y in 0..h {
        for x in 0..w {
            px[((y + oy) * size + x + ox) as usize] = scaled.px[(y * w + x) as usize];
        }
    }
    Image { w: size, h: size, px }
}

fn install_generic(ui: &mut Ui, key: &str) {
    ui.load_icon(&format!("apps/{}", GENERIC_LINUX_ICON));
    ui.load_icon(&format!("apps/{}-24", GENERIC_LINUX_ICON));
    let big = ui.icon(&format!("apps/{}", GENERIC_LINUX_ICON)).map(|i| fit(i, 48));
    let small = ui.icon(&format!("apps/{}-24", GENERIC_LINUX_ICON)).map(|i| fit(i, 24));
    if let (Some(big), Some(small)) = (big, small) {
        ui.insert_icon(&format!("apps/{}", key), big);
        ui.insert_icon(&format!("apps/{}-24", key), small);
    }
}

fn install_icons(ui: &mut Ui, key: &str, source: Option<&str>) -> bool {
    if let Some(path) = source.filter(|p| p.ends_with(".svg")) {
        if let Some(bytes) = fs::read_following(path) {
            if let (Some(big), Some(small)) = (Image::decode_sized(&bytes, 48, 48), Image::decode_sized(&bytes, 24, 24)) {
                ui.insert_icon(&format!("apps/{}", key), big);
                ui.insert_icon(&format!("apps/{}-24", key), small);
                return true;
            }
        }
    }
    match source.and_then(load_png) {
        Some(img) => {
            ui.insert_icon(&format!("apps/{}", key), fit(&img, 48));
            ui.insert_icon(&format!("apps/{}-24", key), fit(&img, 24));
            true
        }
        None => {
            install_generic(ui, key);
            false
        }
    }
}

fn read_dir_entries(dir: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in sys::read_dir(dir).unwrap_or_default() {
        if entry.is_dir || !entry.name.ends_with(".desktop") {
            continue;
        }
        let path = format!("{}/{}", dir, entry.name);
        if let Some(bytes) = fs::read_following(&path) {
            out.push((entry.name.trim_end_matches(".desktop").to_string(), String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    out
}

fn user_dir() -> Option<String> {
    hxstd::env::var("HOME").map(|home| format!("{}/.local/share/applications", home))
}

fn roles_of(e: &BTreeMap<String, String>) -> Vec<String> {
    let mut roles: Vec<String> = e.get("X-Nook-Role").map(|r| r.split(';').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let categories = e.get("Categories").cloned().unwrap_or_default();
    for (category, role) in [("FileManager", "files"), ("TerminalEmulator", "terminal"), ("TextEditor", "editor"), ("WebBrowser", "browser")] {
        if categories.split(';').any(|c| c == category) && !roles.iter().any(|r| r == role) {
            roles.push(String::from(role));
        }
    }
    roles
}

fn entry_to_app(stem: &str, text: &str, root: &str) -> Option<App> {
    let e = parse_entry(text);
    if e.get("Type").map(|t| t != "Application").unwrap_or(true) {
        return None;
    }
    let truthy = |k: &str| e.get(k).map(|v| v == "true").unwrap_or(false);
    if truthy("NoDisplay") || truthy("Hidden") {
        return None;
    }
    if let Some(only) = e.get("OnlyShowIn") {
        if !only.split(';').any(|d| d == "Nook" || d == "HamixOS") {
            return None;
        }
    }
    if e.get("NotShowIn").map(|n| n.split(';').any(|d| d == "Nook" || d == "HamixOS")).unwrap_or(false) {
        return None;
    }
    if let Some(try_exec) = e.get("TryExec") {
        find_program(try_exec, root)?;
    }
    let exec = split_exec(e.get("Exec")?);
    let (first, rest) = exec.split_first()?;
    let program = find_program(first, root)?;
    let linux = fs::personality(&program) == fs::Personality::Linux;
    let name = e.get("Name")?.clone();
    let description = e.get("Comment").or_else(|| e.get("GenericName")).cloned().unwrap_or_default();
    let categories = e.get("Categories").cloned().unwrap_or_default();
    let keywords = format!("{} {} {}", e.get("Keywords").map(|s| s.as_str()).unwrap_or(""), e.get("GenericName").map(|s| s.as_str()).unwrap_or(""), categories).replace(';', " ");
    let icon_value = e.get("Icon").cloned().unwrap_or_default();
    let key = match (linux, e.get("X-Nook-Key")) {
        (false, Some(k)) => k.clone(),
        (false, None) => format!("native-{}", stem),
        (true, _) => format!("linux-{}", stem),
    };
    let category = e.get("X-Nook-Category").and_then(|c| Category::from_name(c)).unwrap_or_else(|| Category::classify(&categories));
    let order = e.get("X-Nook-Order").and_then(|o| o.parse().ok()).unwrap_or(if linux { 10_000 } else { 5_000 });
    let signature = format!("{}|{}|{}|{}|{}|{}|{}|{}", name, program, rest.join(" "), description, icon_value, truthy("Terminal"), categories, e.get("X-Nook-Role").map(|s| s.as_str()).unwrap_or(""));
    let icon_name = basename(&icon_value).trim_end_matches(".png").trim_end_matches(".svg").to_lowercase();
    Some(App {
        key,
        name,
        exec_name: basename(&program).to_lowercase(),
        program,
        args: rest.to_vec(),
        description,
        keywords: keywords.to_lowercase(),
        terminal: truthy("Terminal"),
        linux,
        live_only: truthy("X-Nook-LiveOnly"),
        order,
        signature,
        removed: Cell::new(false),
        category,
        roles: roles_of(&e),
        dock: truthy("X-Nook-Dock"),
        desktop: truthy("X-Nook-Desktop"),
        stem: stem.to_lowercase(),
        id: String::from(stem),
        wm_class: e.get("StartupWMClass").map(|c| c.to_lowercase()).unwrap_or_default(),
        icon_name,
        icon_value,
    })
}

fn app_dirs() -> Vec<(String, &'static str)> {
    let mut dirs: Vec<(String, &'static str)> = NATIVE_DIRS.iter().map(|d| (d.to_string(), "")).collect();
    dirs.extend(user_dir().map(|d| (d, "")));
    dirs.extend(NATIVE_DIRS.iter().map(|d| (format!("{}{}", LINUX_ROOT, d), LINUX_ROOT)));
    dirs
}

fn collect() -> Vec<App> {
    let mut found: Vec<App> = Vec::new();
    for (dir, root) in app_dirs() {
        for (stem, text) in read_dir_entries(&dir) {
            if let Some(app) = entry_to_app(&stem, &text, root) {
                found.push(app);
            }
        }
    }
    found
}

pub fn fingerprint() -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            hash = (hash ^ *b as u64).wrapping_mul(0x100_0000_01b3);
        }
    };
    for (dir, _) in app_dirs() {
        mix(dir.as_bytes());
        for entry in sys::read_dir(&dir).unwrap_or_default() {
            mix(entry.name.as_bytes());
            mix(&entry.size.to_le_bytes());
        }
    }
    hash
}

pub fn scan(ui: &mut Ui, sized_cache: &mut BTreeMap<(String, i32), Image>) -> bool {
    let found = collect();
    let mut themes: Option<Vec<String>> = None;
    let list = registry();
    let mut changed = false;
    let mut seen: Vec<bool> = alloc::vec![false; list.len()];
    for app in found {
        let index = list.iter().position(|a| a.key == app.key);
        if let Some(index) = index {
            if seen.get(index).copied().unwrap_or(false) {
                continue;
            }
        }
        match index {
            Some(index) => {
                seen[index] = true;
                let current = list[index];
                if current.signature != app.signature {
                    install_app_icon(ui, &app, &mut themes);
                    sized_cache.retain(|(name, _), _| !name.starts_with(&format!("apps/{}-", app.key)) && *name != format!("apps/{}", app.key));
                    list[index] = Box::leak(Box::new(app));
                    changed = true;
                } else if current.removed.get() {
                    current.removed.set(false);
                    changed = true;
                }
            }
            None => {
                install_app_icon(ui, &app, &mut themes);
                list.push(Box::leak(Box::new(app)));
                seen.push(true);
                changed = true;
            }
        }
    }
    for (index, app) in list.iter().enumerate() {
        if !seen.get(index).copied().unwrap_or(false) && !app.removed.get() {
            app.removed.set(true);
            changed = true;
        }
    }
    changed
}

fn install_app_icon(ui: &mut Ui, app: &App, themes: &mut Option<Vec<String>>) {
    let icon = app.icon_value.as_str();
    if app.linux {
        let linux_themes = icon_themes(LINUX_ROOT);
        let themes = &linux_themes;
        let source = find_icon(icon, themes, LINUX_ROOT).or_else(|| find_icon(&app.stem, themes, LINUX_ROOT)).or_else(|| find_icon(&app.exec_name, themes, LINUX_ROOT));
        install_icons(ui, &app.key, source.as_deref());
        return;
    }
    if icon.is_empty() || icon == app.key {
        if !ui.load_icon(&format!("apps/{}", app.key)) {
            install_generic(ui, &app.key);
        }
        return;
    }
    if icon.starts_with('/') {
        install_icons(ui, &app.key, Some(icon));
    } else if safe_icon_name(icon) && ui.load_icon(&format!("apps/{}", icon)) {
        let big = ui.icon(&format!("apps/{}", icon)).map(|i| fit(i, 48));
        let small = ui.load_icon(&format!("apps/{}-24", icon)).then(|| ui.icon(&format!("apps/{}-24", icon)).map(|i| fit(i, 24))).flatten();
        if let Some(big) = big {
            let small = small.unwrap_or_else(|| fit(&big, 24));
            ui.insert_icon(&format!("apps/{}", app.key), big);
            ui.insert_icon(&format!("apps/{}-24", app.key), small);
        }
    } else {
        let themes = themes.get_or_insert_with(|| icon_themes(""));
        let source = find_icon(icon, themes, "");
        install_icons(ui, &app.key, source.as_deref());
    }
}

fn normalize(id: &str) -> String {
    id.trim().trim_end_matches(".desktop").to_lowercase()
}

fn last_segment(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

pub fn match_identity(candidates: &[String]) -> Option<usize> {
    let list = registry();
    let ids: Vec<String> = candidates.iter().map(|c| normalize(c)).filter(|c| !c.is_empty()).collect();
    if ids.is_empty() {
        return None;
    }
    let live = |i: &usize| !list[*i].removed.get();
    let checks: [&dyn Fn(&App, &str) -> bool; 6] = [
        &|a, id| a.stem == id,
        &|a, id| !a.wm_class.is_empty() && a.wm_class == id,
        &|a, id| last_segment(&a.stem) == id || a.stem == last_segment(id),
        &|a, id| a.exec_name == id || a.exec_name == last_segment(id),
        &|a, id| !a.icon_name.is_empty() && (a.icon_name == id || a.icon_name == last_segment(id)),
        &|a, id| a.name.to_lowercase() == id,
    ];
    for check in checks {
        for id in &ids {
            let mut hits: Vec<usize> = (0..list.len()).filter(live).filter(|i| check(list[*i], id)).collect();
            hits.sort_by_key(|i| (!list[*i].linux, list[*i].order));
            if let Some(first) = hits.first() {
                return Some(*first);
            }
        }
    }
    None
}

pub fn adhoc_key(ui: &mut Ui, candidates: &[String]) -> String {
    let ids: Vec<String> = candidates.iter().map(|c| normalize(c)).filter(|c| !c.is_empty()).collect();
    let tag: String = ids.first().map(|s| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).take(48).collect()).unwrap_or_else(|| String::from("app"));
    let key = format!("{}{}", ADHOC_PREFIX, tag);
    if ui.icon(&format!("apps/{}", key)).is_some() {
        return key;
    }
    let mut source = None;
    for root in ["", LINUX_ROOT] {
        let themes = icon_themes(root);
        source = ids.iter().find_map(|id| find_icon(id, &themes, root).or_else(|| find_icon(last_segment(id), &themes, root)).or_else(|| find_icon(&id.replace(' ', "-"), &themes, root)));
        if source.is_some() {
            break;
        }
    }
    install_icons(ui, &key, source.as_deref());
    key
}

pub fn launcher_order() -> Vec<usize> {
    let list = registry();
    let mut indices: Vec<usize> = (0..list.len()).filter(|i| !list[*i].removed.get()).collect();
    indices.sort_by(|a, b| {
        let (x, y) = (list[*a], list[*b]);
        (x.linux, x.order, x.name.to_lowercase()).cmp(&(y.linux, y.order, y.name.to_lowercase()))
    });
    indices
}

pub fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,+".contains(c)) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}
