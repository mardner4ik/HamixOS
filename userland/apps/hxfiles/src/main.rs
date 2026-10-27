#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, env, eprintln, fs, sys, users};
use hxclient::ui::{self, theme, FieldAction, Hits, Menu, MenuBar, MenuEntry, MenuResult, Style, TextField, Ui};
use hxclient::{Event, Window, MOUSE_LEAVE, MOUSE_MOVE, MOUSE_PRESS, MOUSE_WHEEL};
use hxproto::{Request, Text, MESSAGE_MAX, PICK_CANCELLED, PICK_OK};
use vellum::gfx::ellipsize;
use vellum::{Area, Painter};

const INITIAL_W: i32 = 860;
const INITIAL_H: i32 = 560;

fn ww() -> i32 {
    hxclient::window_width()
}

fn hh() -> i32 {
    hxclient::window_height()
}
const SIDEBAR: i32 = 188;
const TOOLBAR: i32 = 52;
const STATUS: i32 = 30;
const PICK_BAR: i32 = 60;
const ROW: i32 = 32;

const CMD_NEW_FOLDER: u32 = 1;
const CMD_NEW_FILE: u32 = 2;
const CMD_OPEN: u32 = 3;
const CMD_RENAME: u32 = 4;
const CMD_DELETE: u32 = 5;
const CMD_PROPERTIES: u32 = 6;
const CMD_QUIT: u32 = 7;
const CMD_PIN: u32 = 8;
const CMD_TERMINAL: u32 = 9;
const CMD_COPY: u32 = 10;
const CMD_CUT: u32 = 11;
const CMD_PASTE: u32 = 12;
const CMD_COPY_PATH: u32 = 13;
const CMD_HIDDEN: u32 = 20;
const CMD_SORT_NAME: u32 = 21;
const CMD_SORT_SIZE: u32 = 22;
const CMD_SORT_KIND: u32 = 23;
const CMD_REFRESH: u32 = 24;
const CMD_BACK: u32 = 30;
const CMD_FORWARD: u32 = 31;
const CMD_UP: u32 = 32;
const CMD_HOME: u32 = 33;
const CMD_PLACE: u32 = 40;
const CMD_ABOUT: u32 = 60;

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Back,
    Forward,
    Up,
    Home,
    Path,
    Place(usize),
    Row(usize),
    Blank,
    Header,
    Context(u32),
    Filter,
    NameField,
    Cancel,
    Accept,
    ConfirmYes,
    ConfirmNo,
}

#[derive(Clone, Copy, PartialEq)]
enum PickMode {
    Open,
    Folder,
    Save,
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Name,
    Size,
    Kind,
}

struct Filter {
    name: String,
    extensions: Vec<String>,
}

impl Filter {
    fn matches(&self, name: &str) -> bool {
        if self.extensions.iter().any(|e| e == "*") {
            return true;
        }
        let lower = name.to_lowercase();
        self.extensions.iter().any(|e| lower.ends_with(&format!(".{}", e)))
    }

    fn label(&self) -> String {
        if self.extensions.iter().any(|e| e == "*") {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.extensions.iter().map(|e| format!("*.{}", e)).collect::<Vec<_>>().join(", "))
        }
    }
}

struct Pick {
    mode: PickMode,
    token: u32,
    title: String,
    filters: Vec<Filter>,
    active: usize,
    name: TextField,
    name_focused: bool,
}

struct Entry {
    name: String,
    is_dir: bool,
    size: u64,
    mode: u32,
    owner: u32,
    kind: char,
}

struct Context {
    x: i32,
    y: i32,
    items: Vec<(u32, &'static str, bool)>,
}

struct App {
    window: Window,
    ui: Ui,
    menu: MenuBar,
    path: String,
    entries: Vec<Entry>,
    selected: Option<usize>,
    scroll: i32,
    history: Vec<String>,
    future: Vec<String>,
    hits: Hits<Hit>,
    hover: Option<Hit>,
    mouse: (i32, i32),
    editing: Option<(TextField, bool)>,
    context: Option<Context>,
    message: String,
    last_click: (u64, usize),
    pick: Option<Pick>,
    clipboard: Option<(String, bool)>,
    show_hidden: bool,
    sort: Sort,
    confirm: Option<usize>,
}

fn places() -> Vec<(String, String, &'static str)> {
    let home = ui::home_dir();
    let mut out = alloc::vec![
        (String::from("Home"), home.clone(), "ui/home"),
        (String::from("Documents"), format!("{}/Documents", home), "ui/folder"),
        (String::from("Videos"), format!("{}/Videos", home), "ui/video"),
        (String::from("Pictures"), String::from("/usr/share/wallpapers"), "ui/image"),
        (String::from("System"), String::from("/"), "ui/disk"),
        (String::from("Temporary"), String::from("/tmp"), "ui/folder"),
    ];
    for line in fs::read_to_string("/proc/mounts").unwrap_or_default().lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 2 {
            out.push((String::from(f[0].trim_start_matches("/dev/")), String::from(f[1]), "ui/disk"));
        }
    }
    out
}

const FIXED_PLACES: usize = 6;

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{}{}", dir, name) } else { format!("{}/{}", dir, name) }
}

fn base_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

fn parse_filters(spec: &str) -> Vec<Filter> {
    let mut filters = Vec::new();
    for part in spec.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, list) = part.split_once('|').unwrap_or((part, "*"));
        let extensions: Vec<String> = list.split(',').map(|e| String::from(e.trim().trim_start_matches("*.").trim_start_matches('.')).to_lowercase()).filter(|e| !e.is_empty()).collect();
        if !extensions.is_empty() {
            filters.push(Filter { name: String::from(name.trim()), extensions });
        }
    }
    if !filters.iter().any(|f| f.extensions.iter().any(|e| e == "*")) {
        filters.push(Filter { name: String::from("All files"), extensions: alloc::vec![String::from("*")] });
    }
    filters
}

fn parse_pick(args: &[String]) -> (Option<Pick>, Option<String>) {
    let value = |key: &str| args.iter().find_map(|a| a.strip_prefix(key).map(String::from));
    let Some(mode) = value("--pick=") else {
        return (None, args.get(1).cloned());
    };
    let mode = match mode.as_str() {
        "folder" => PickMode::Folder,
        "save" => PickMode::Save,
        _ => PickMode::Open,
    };
    let token = value("--token=").and_then(|t| t.parse().ok()).unwrap_or(0);
    let default_title = match mode {
        PickMode::Open => "Open a file",
        PickMode::Folder => "Choose a folder",
        PickMode::Save => "Save as",
    };
    let title = value("--title=").filter(|t| !t.is_empty()).unwrap_or_else(|| String::from(default_title));
    let filters = parse_filters(&value("--filter=").unwrap_or_default());
    let start = value("--start=").filter(|s| !s.is_empty());
    let mut name = TextField::default();
    let mut start_dir = start.clone();
    if mode == PickMode::Save {
        if let Some(s) = &start {
            let is_dir = sys::stat(s).map(|st| st.is_dir()).unwrap_or(false);
            if !is_dir {
                let (dir, file) = match s.rfind('/') {
                    Some(i) => (String::from(&s[..i.max(1)]), String::from(&s[i + 1..])),
                    None => (ui::home_dir(), s.clone()),
                };
                name.set(&file);
                start_dir = Some(dir);
            }
        }
    }
    (Some(Pick { mode, token, title, filters, active: 0, name, name_focused: mode == PickMode::Save }), start_dir)
}

fn remove_tree(path: &str) -> i64 {
    let is_dir = sys::stat(path).map(|s| s.is_dir()).unwrap_or(false);
    if is_dir {
        for entry in sys::read_dir(path).unwrap_or_default() {
            let r = remove_tree(&join(path, &entry.name));
            if r < 0 {
                return r;
            }
        }
    }
    sys::unlink(path)
}

fn copy_tree(from: &str, to: &str) -> Result<(), String> {
    let stat = sys::stat(from).map_err(|_| format!("{} disappeared", base_name(from)))?;
    if stat.is_dir() {
        if to.starts_with(&format!("{}/", from.trim_end_matches('/'))) {
            return Err(String::from("a folder cannot be copied into itself"));
        }
        let r = sys::mkdir(to);
        if r < 0 && sys::stat(to).is_err() {
            return Err(format!("cannot create {}: {}", base_name(to), sys::error_name(r)));
        }
        for entry in sys::read_dir(from).unwrap_or_default() {
            copy_tree(&join(from, &entry.name), &join(to, &entry.name))?;
        }
        return Ok(());
    }
    let data = fs::read(from).ok_or_else(|| format!("cannot read {}", base_name(from)))?;
    if !fs::write(to, &data) {
        return Err(format!("cannot write {}", base_name(to)));
    }
    sys::chmod(to, stat.mode & 0o7777);
    Ok(())
}

fn free_name(dir: &str, name: &str) -> String {
    if sys::stat(&join(dir, name)).is_err() {
        return String::from(name);
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut n = 2;
    loop {
        let candidate = format!("{} ({}){}", stem, n, ext);
        if sys::stat(&join(dir, &candidate)).is_err() {
            return candidate;
        }
        n += 1;
    }
}

impl App {
    fn top(&self) -> i32 {
        if self.pick.is_none() { MenuBar::HEIGHT } else { 0 }
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    fn refresh_menus(&mut self) {
        if self.pick.is_some() {
            return;
        }
        let has = self.selected.is_some();
        let can_paste = self.clipboard.is_some();
        let mut go = alloc::vec![
            MenuEntry::item("Back", "", CMD_BACK).enabled(!self.history.is_empty()),
            MenuEntry::item("Forward", "", CMD_FORWARD).enabled(!self.future.is_empty()),
            MenuEntry::item("Enclosing folder", "Backspace", CMD_UP).enabled(self.path != "/"),
            MenuEntry::separator(),
        ];
        for (i, (label, _, _)) in places().iter().take(FIXED_PLACES).enumerate() {
            go.push(MenuEntry::item(label, "", CMD_PLACE + i as u32));
        }
        let menus = alloc::vec![
            Menu::new(
                "File",
                alloc::vec![
                    MenuEntry::item("New folder", "Ctrl+N", CMD_NEW_FOLDER),
                    MenuEntry::item("New text file", "", CMD_NEW_FILE),
                    MenuEntry::separator(),
                    MenuEntry::item("Open", "Enter", CMD_OPEN).enabled(has),
                    MenuEntry::item("Open a terminal here", "", CMD_TERMINAL),
                    MenuEntry::separator(),
                    MenuEntry::item("Rename…", "F2", CMD_RENAME).enabled(has),
                    MenuEntry::item("Delete", "Del", CMD_DELETE).enabled(has),
                    MenuEntry::item("Properties", "", CMD_PROPERTIES).enabled(has),
                    MenuEntry::separator(),
                    MenuEntry::item("Close", "Ctrl+Q", CMD_QUIT),
                ]
            ),
            Menu::new(
                "Edit",
                alloc::vec![
                    MenuEntry::item("Cut", "Ctrl+X", CMD_CUT).enabled(has),
                    MenuEntry::item("Copy", "Ctrl+C", CMD_COPY).enabled(has),
                    MenuEntry::item("Paste", "Ctrl+V", CMD_PASTE).enabled(can_paste),
                    MenuEntry::separator(),
                    MenuEntry::item("Copy the path", "", CMD_COPY_PATH),
                    MenuEntry::item("Pin as a command", "", CMD_PIN).enabled(has),
                ]
            ),
            Menu::new(
                "View",
                alloc::vec![
                    MenuEntry::item("Show hidden files", "", CMD_HIDDEN).checked(self.show_hidden),
                    MenuEntry::separator(),
                    MenuEntry::item("Sort by name", "", CMD_SORT_NAME).checked(self.sort == Sort::Name),
                    MenuEntry::item("Sort by size", "", CMD_SORT_SIZE).checked(self.sort == Sort::Size),
                    MenuEntry::item("Sort by type", "", CMD_SORT_KIND).checked(self.sort == Sort::Kind),
                    MenuEntry::separator(),
                    MenuEntry::item("Refresh", "Ctrl+R", CMD_REFRESH),
                ]
            ),
            Menu::new("Go", go),
            Menu::new("Help", alloc::vec![MenuEntry::item("About Files", "", CMD_ABOUT)]),
        ];
        self.menu.set_menus(menus);
    }

    fn sort_entries(&mut self) {
        let sort = self.sort;
        self.entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
                Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Sort::Size => b.size.cmp(&a.size).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                Sort::Kind => {
                    let ka = ui::file_kind(&a.name, a.is_dir, a.mode, a.kind);
                    let kb = ui::file_kind(&b.name, b.is_dir, b.mode, b.kind);
                    ka.cmp(kb).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
            })
        });
    }

    fn load(&mut self, path: &str) -> bool {
        let Some(list) = sys::read_dir(path) else {
            self.message = format!("Cannot open {}", path);
            return false;
        };
        let pick = self.pick.as_ref();
        let hidden = self.show_hidden;
        let entries: Vec<Entry> = list
            .into_iter()
            .filter(|e| hidden || !e.name.starts_with('.'))
            .filter(|e| match pick {
                None => true,
                Some(p) if p.mode == PickMode::Folder => e.is_dir,
                Some(p) => e.is_dir || p.filters.get(p.active).map(|f| f.matches(&e.name)).unwrap_or(true),
            })
            .map(|e| Entry { name: e.name, is_dir: e.is_dir, size: e.size, mode: e.mode, owner: e.owner, kind: e.kind })
            .collect();
        self.entries = entries;
        self.sort_entries();
        self.path = String::from(path);
        self.selected = None;
        self.scroll = 0;
        self.context = None;
        self.confirm = None;
        self.message.clear();
        let folder = if path == "/" { "/" } else { base_name(path) };
        let title = match &self.pick {
            Some(p) => format!("{} — {}", p.title, folder),
            None => format!("{} — Files", folder),
        };
        self.window.set_title(&title);
        self.refresh_menus();
        true
    }

    fn reload_keep(&mut self, select: Option<&str>) {
        let path = self.path.clone();
        let scroll = self.scroll;
        let message = core::mem::take(&mut self.message);
        self.load(&path);
        self.scroll = scroll;
        self.message = message;
        if let Some(name) = select {
            self.selected = self.entries.iter().position(|e| e.name == name);
        }
        self.clamp_scroll();
        self.refresh_menus();
    }

    fn navigate(&mut self, path: &str) {
        let old = self.path.clone();
        if self.load(path) {
            self.history.push(old);
            self.future.clear();
            self.refresh_menus();
        }
    }

    fn back(&mut self) {
        if let Some(prev) = self.history.pop() {
            let cur = self.path.clone();
            if self.load(&prev) {
                self.future.push(cur);
            }
        }
        self.refresh_menus();
    }

    fn forward(&mut self) {
        if let Some(next) = self.future.pop() {
            let cur = self.path.clone();
            if self.load(&next) {
                self.history.push(cur);
            }
        }
        self.refresh_menus();
    }

    fn bottom_height(&self) -> i32 {
        if self.pick.is_some() { PICK_BAR } else { STATUS }
    }

    fn list_area(&self) -> Area {
        let top = self.top() + TOOLBAR + 34;
        Area::new(SIDEBAR + 1, top, ww() - SIDEBAR - 1, hh() - top - self.bottom_height())
    }

    fn visible_rows(&self) -> i32 {
        self.list_area().h / ROW
    }

    fn clamp_scroll(&mut self) {
        let max = (self.entries.len() as i32 * ROW - self.list_area().h).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }

    fn open(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let full = join(&self.path, &entry.name);
        if entry.is_dir {
            self.navigate(&full);
            return;
        }
        let name = entry.name.clone();
        if let Some(mode) = self.pick.as_ref().map(|p| p.mode) {
            match mode {
                PickMode::Open => self.finish(PICK_OK, &full),
                PickMode::Save => {
                    if let Some(pick) = &mut self.pick {
                        pick.name.set(&name);
                    }
                    self.accept();
                }
                PickMode::Folder => {}
            }
            return;
        }
        if !ui::open_with_default(&full) {
            self.message = format!("No application can open {}", name);
        }
    }

    fn finish(&mut self, status: u32, path: &str) {
        if let Some(pick) = &self.pick {
            if let Some(server) = hxclient::server() {
                let mut buf = [0u8; MESSAGE_MAX];
                let len = Request::PickerResult { token: pick.token, status, path: Text::new(path) }.encode(&mut buf);
                sys::msg_send(server, &buf[..len]);
            }
        }
        sys::exit(0);
    }

    fn accept(&mut self) {
        let Some(pick) = &self.pick else {
            return;
        };
        match pick.mode {
            PickMode::Open => match self.selected.and_then(|i| self.entries.get(i)) {
                Some(entry) if entry.is_dir => {
                    let full = join(&self.path, &entry.name);
                    self.navigate(&full);
                }
                Some(entry) => {
                    let full = join(&self.path, &entry.name);
                    self.finish(PICK_OK, &full);
                }
                None => self.message = String::from("Select a file first"),
            },
            PickMode::Folder => {
                let chosen = match self.selected.and_then(|i| self.entries.get(i)) {
                    Some(entry) if entry.is_dir => join(&self.path, &entry.name),
                    _ => self.path.clone(),
                };
                self.finish(PICK_OK, &chosen);
            }
            PickMode::Save => {
                let name = String::from(pick.name.text.trim());
                if name.is_empty() {
                    self.message = String::from("Type a file name");
                    return;
                }
                if name.contains('/') {
                    self.message = String::from("A file name cannot contain '/'");
                    return;
                }
                let full = join(&self.path, &name);
                if sys::stat(&full).map(|s| s.is_dir()).unwrap_or(false) {
                    self.navigate(&full);
                    return;
                }
                self.finish(PICK_OK, &full);
            }
        }
    }

    fn up(&mut self) {
        if self.path == "/" {
            return;
        }
        let parent = match self.path.rfind('/') {
            Some(0) | None => String::from("/"),
            Some(i) => String::from(&self.path[..i]),
        };
        self.navigate(&parent);
    }

    fn create(&mut self, folder: bool) {
        let name = free_name(&self.path, if folder { "New folder" } else { "New file.txt" });
        let target = join(&self.path, &name);
        let ok = if folder {
            let r = sys::mkdir(&target);
            if r < 0 {
                self.message = format!("Cannot create a folder here: {}", sys::error_name(r));
            }
            r >= 0
        } else if fs::write(&target, b"") {
            true
        } else {
            self.message = String::from("Cannot create a file here — permission denied?");
            false
        };
        if ok {
            self.reload_keep(Some(&name));
            if self.selected.is_some() {
                self.editing = Some((TextField::new(&name), true));
                self.reveal_selected();
            }
        }
    }

    fn reveal_selected(&mut self) {
        if let Some(s) = self.selected {
            let top = s as i32 * ROW;
            let h = self.list_area().h;
            if top < self.scroll {
                self.scroll = top;
            } else if top + ROW > self.scroll + h {
                self.scroll = top + ROW - h;
            }
        }
    }

    fn delete_now(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let name = entry.name.clone();
        let full = join(&self.path, &name);
        let r = remove_tree(&full);
        if r < 0 {
            self.message = format!("Cannot delete {}: {}", name, sys::error_name(r));
        } else {
            self.message = format!("Deleted {}", name);
        }
        self.reload_keep(None);
    }

    fn paste(&mut self) {
        let Some((source, cut)) = self.clipboard.clone() else {
            return;
        };
        let name = String::from(base_name(&source));
        let parent = match source.rfind('/') {
            Some(0) => String::from("/"),
            Some(i) => String::from(&source[..i]),
            None => String::from("/"),
        };
        if cut && parent == self.path {
            self.clipboard = None;
            self.refresh_menus();
            return;
        }
        let target_name = free_name(&self.path, &name);
        let target = join(&self.path, &target_name);
        let result = if cut {
            let r = sys::rename(&source, &target);
            if r >= 0 {
                Ok(())
            } else {
                copy_tree(&source, &target).and_then(|_| if remove_tree(&source) < 0 { Err(String::from("copied, but the original could not be removed")) } else { Ok(()) })
            }
        } else {
            copy_tree(&source, &target)
        };
        match result {
            Ok(()) => {
                self.message = format!("{} {}", if cut { "Moved" } else { "Copied" }, target_name);
                if cut {
                    self.clipboard = None;
                }
            }
            Err(e) => self.message = format!("Paste failed: {}", e),
        }
        self.reload_keep(Some(&target_name));
        sys::sync();
    }

    fn properties(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let full = join(&self.path, &entry.name);
        let kind = if entry.is_dir { String::from("folder") } else { ui::human_size(entry.size) };
        self.message = format!("{} · {} · owner {} · mode {:o}", full, kind, users::name_of(entry.owner), entry.mode & 0o7777);
    }

    fn command(&mut self, command: u32) -> bool {
        match command {
            CMD_NEW_FOLDER => self.create(true),
            CMD_NEW_FILE => self.create(false),
            CMD_OPEN => {
                if let Some(i) = self.selected {
                    self.open(i);
                }
            }
            CMD_TERMINAL => {
                let path = self.path.clone();
                sys::chdir(&path);
                let spawned = sys::spawn("/usr/bin/hxterm", &[] as &[&str], sys::SPAWN_DETACH);
                if spawned < 0 {
                    self.message = String::from("Cannot start the terminal");
                }
            }
            CMD_RENAME => {
                if let Some(entry) = self.selected_entry() {
                    let name = entry.name.clone();
                    self.editing = Some((TextField::new(&name), true));
                }
            }
            CMD_DELETE => {
                if self.selected.is_some() {
                    self.confirm = self.selected;
                }
            }
            CMD_PROPERTIES => self.properties(),
            CMD_QUIT => return false,
            CMD_PIN => {
                if let Some(i) = self.selected {
                    self.pin(i);
                }
            }
            CMD_COPY | CMD_CUT => {
                if let Some(entry) = self.selected_entry() {
                    let full = join(&self.path, &entry.name);
                    let cut = command == CMD_CUT;
                    self.message = format!("{} {} — choose a folder and paste", if cut { "Cut" } else { "Copied" }, entry.name);
                    self.clipboard = Some((full, cut));
                }
            }
            CMD_PASTE => self.paste(),
            CMD_COPY_PATH => {
                self.message = match self.selected_entry() {
                    Some(e) => join(&self.path, &e.name),
                    None => self.path.clone(),
                };
            }
            CMD_HIDDEN => {
                self.show_hidden = !self.show_hidden;
                self.reload_keep(None);
            }
            CMD_SORT_NAME | CMD_SORT_SIZE | CMD_SORT_KIND => {
                self.sort = match command {
                    CMD_SORT_SIZE => Sort::Size,
                    CMD_SORT_KIND => Sort::Kind,
                    _ => Sort::Name,
                };
                let selected = self.selected_entry().map(|e| e.name.clone());
                self.sort_entries();
                self.selected = selected.and_then(|n| self.entries.iter().position(|e| e.name == n));
            }
            CMD_REFRESH => {
                let selected = self.selected_entry().map(|e| e.name.clone());
                self.reload_keep(selected.as_deref());
            }
            CMD_BACK => self.back(),
            CMD_FORWARD => self.forward(),
            CMD_UP => self.up(),
            CMD_HOME => self.navigate(&ui::home_dir()),
            c if (CMD_PLACE..CMD_PLACE + FIXED_PLACES as u32).contains(&c) => {
                if let Some((_, path, _)) = places().get((c - CMD_PLACE) as usize) {
                    sys::mkdir(path);
                    let path = path.clone();
                    self.navigate(&path);
                }
            }
            CMD_ABOUT => self.message = String::from("Files — browse, copy and organise your documents"),
            _ => {}
        }
        self.refresh_menus();
        true
    }

    fn pin(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let full = join(&self.path, &entry.name);
        let command: String = entry.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        let (program, args): (&str, Vec<String>) = if entry.is_dir { ("/usr/bin/hxfiles", alloc::vec![full.clone()]) } else { ("/usr/bin/hxnotes", alloc::vec![full.clone()]) };
        let name = entry.name.clone();
        if sys::cmd_register(&command, program, &args) >= 0 {
            self.message = format!("Command '{}' now opens {}", command, name);
        } else {
            self.message = String::from("Could not register the command");
        }
    }

    fn commit_edit(&mut self) {
        let Some((field, rename)) = self.editing.take() else {
            return;
        };
        if rename {
            if let Some(i) = self.selected {
                let old = join(&self.path, &self.entries[i].name);
                let new_name = String::from(field.text.trim());
                if new_name.is_empty() || new_name.contains('/') {
                    self.message = String::from("A name cannot be empty or contain '/'");
                    return;
                }
                let new = join(&self.path, &new_name);
                if old == new {
                    return;
                }
                if sys::stat(&new).is_ok() {
                    self.message = format!("{} already exists", new_name);
                    return;
                }
                let r = sys::rename(&old, &new);
                if r < 0 {
                    self.message = format!("Rename failed: {}", sys::error_name(r));
                }
                self.reload_keep(Some(&new_name));
            }
        } else {
            let typed = field.text.trim();
            let target = if typed.is_empty() { String::from("/") } else { String::from(typed) };
            if sys::stat(&target).map(|s| s.is_dir()).unwrap_or(false) {
                self.navigate(&target);
            } else {
                self.message = format!("{} is not a folder", target);
            }
        }
    }

    fn cycle_filter(&mut self) {
        if let Some(pick) = &mut self.pick {
            if pick.filters.len() > 1 {
                pick.active = (pick.active + 1) % pick.filters.len();
                let path = self.path.clone();
                self.load(&path);
            }
        }
    }

    fn open_context(&mut self, x: i32, y: i32, on_entry: Option<usize>) {
        let can_paste = self.clipboard.is_some();
        let items = match on_entry {
            Some(i) => {
                self.selected = Some(i);
                alloc::vec![
                    (CMD_OPEN, "Open", true),
                    (CMD_RENAME, "Rename…", true),
                    (CMD_CUT, "Cut", true),
                    (CMD_COPY, "Copy", true),
                    (CMD_PASTE, "Paste here", can_paste),
                    (CMD_PROPERTIES, "Properties", true),
                    (CMD_PIN, "Pin as a command", true),
                    (CMD_DELETE, "Delete", true),
                ]
            }
            None => {
                self.selected = None;
                alloc::vec![(CMD_NEW_FOLDER, "New folder", true), (CMD_NEW_FILE, "New text file", true), (CMD_PASTE, "Paste", can_paste), (CMD_TERMINAL, "Open a terminal here", true), (CMD_REFRESH, "Refresh", true)]
            }
        };
        self.context = Some(Context { x, y, items });
        self.refresh_menus();
    }

    fn draw_pick_bar(p: &mut Painter, ui: &Ui, hits: &mut Hits<Hit>, hover: Option<Hit>, pick: &mut Pick, message: &str, selected_name: Option<&str>) {
        let bar = Area::new(SIDEBAR + 1, hh() - PICK_BAR, ww() - SIDEBAR - 1, PICK_BAR);
        p.fill(bar, theme::surface());
        p.fill(Area::new(bar.x, bar.y, bar.w, 1), theme::border());
        let accept_label = match pick.mode {
            PickMode::Open => "Open",
            PickMode::Folder => "Select folder",
            PickMode::Save => "Save",
        };
        let accept_w = ui.medium.measure(accept_label) + 40;
        let accept = Area::new(bar.right() - accept_w - 14, bar.y + 13, accept_w, 34);
        let cancel = Area::new(accept.x - 100, bar.y + 13, 92, 34);
        ui::button(p, ui, cancel, "Cancel", Style::Secondary, hover == Some(Hit::Cancel), true);
        let enabled = match pick.mode {
            PickMode::Open => selected_name.is_some(),
            PickMode::Folder => true,
            PickMode::Save => !pick.name.text.trim().is_empty(),
        };
        ui::button(p, ui, accept, accept_label, Style::Primary, hover == Some(Hit::Accept), enabled);
        hits.add(cancel, Hit::Cancel);
        hits.add(accept, Hit::Accept);
        let mut x = bar.x + 14;
        let right_limit = cancel.x - 12;
        if pick.mode == PickMode::Save {
            let field = Area::new(x, bar.y + 13, ((right_limit - x) / 2).max(120), 34);
            ui::text_field(p, ui, field, &mut pick.name, pick.name_focused, "File name");
            hits.add(field, Hit::NameField);
            x = field.right() + 10;
        }
        if pick.mode != PickMode::Folder {
            if let Some(filter) = pick.filters.get(pick.active) {
                let label = filter.label();
                let area = Area::new(x, bar.y + 13, (right_limit - x).max(60), 34);
                let shown = ellipsize(&ui.font, &label, area.w - 44);
                if hover == Some(Hit::Filter) && pick.filters.len() > 1 {
                    p.rounded(area, theme::RADIUS, theme::hover(), 255);
                }
                p.rounded_border(area, theme::RADIUS, theme::border(), 255);
                ui::text_in(p, &ui.font, area.x + 12, area, &shown, theme::text());
                if pick.filters.len() > 1 {
                    ui::symbol(p, ui, "ui/chevron-down", area.right() - 26, area.y + 9, theme::dim());
                }
                hits.add(area, Hit::Filter);
            }
        } else {
            let text = if message.is_empty() { String::from("Pick a folder, or use the one that is open") } else { String::from(message) };
            let area = Area::new(x, bar.y, right_limit - x, PICK_BAR);
            ui::text_in(p, &ui.small, x, area, &text, theme::dim());
        }
    }

    fn draw(&mut self) {
        let list = self.list_area();
        let visible_rows = self.visible_rows();
        let top = self.top();
        let buffer = self.window.buffer();
        let mut p = Painter::new(buffer, ww(), hh());
        let ui = &self.ui;
        let hits = &mut self.hits;
        hits.clear();
        let hover = self.hover;
        p.fill(Area::new(0, 0, ww(), hh()), theme::bg());
        if self.pick.is_none() {
            self.menu.draw(&mut p, ui, ww());
        }

        p.fill(Area::new(0, top, SIDEBAR, hh() - top), theme::sidebar());
        p.fill(Area::new(SIDEBAR, top, 1, hh() - top), theme::border());
        p.text(&ui.small, 18, top + 16, "PLACES", theme::faint());
        let mut y = top + 36;
        for (i, (label, path, icon)) in places().iter().enumerate() {
            if i == FIXED_PLACES {
                p.text(&ui.small, 18, y + 10, "MOUNTED", theme::faint());
                y += 30;
            }
            let row = Area::new(8, y, SIDEBAR - 16, 32);
            let active = *path == self.path;
            ui::list_row(&mut p, row, active, hover == Some(Hit::Place(i)));
            ui::symbol(&mut p, ui, icon, row.x + 10, row.y + 8, if active { theme::on_selection() } else { theme::dim() });
            ui::text_in(&mut p, &ui.font, row.x + 36, row, label, if active { theme::on_selection() } else { theme::text() });
            hits.add(row, Hit::Place(i));
            y += 34;
        }

        let bar = Area::new(SIDEBAR + 1, top, ww() - SIDEBAR - 1, TOOLBAR);
        p.fill(bar, theme::surface());
        p.fill(Area::new(bar.x, top + TOOLBAR - 1, bar.w, 1), theme::border());
        let mut x = bar.x + 10;
        for (hit, icon, enabled) in [
            (Hit::Back, "ui/back", !self.history.is_empty()),
            (Hit::Forward, "ui/forward", !self.future.is_empty()),
            (Hit::Up, "ui/up", self.path != "/"),
            (Hit::Home, "ui/home", true),
        ] {
            let area = Area::new(x, top + 10, 32, 32);
            ui::icon_button(&mut p, ui, area, icon, enabled && hover == Some(hit), false);
            if !enabled {
                p.blend_fill(area, theme::surface(), 150);
            }
            hits.add(area, hit);
            x += 36;
        }
        let path_area = Area::new(x + 6, top + 10, ww() - x - 18, 32);
        match &mut self.editing {
            Some((field, false)) => ui::text_field(&mut p, ui, path_area, field, true, "Type a folder path"),
            _ => {
                p.rounded(path_area, theme::RADIUS, theme::bg(), 255);
                p.rounded_border(path_area, theme::RADIUS, if hover == Some(Hit::Path) { theme::faint() } else { theme::border() }, 255);
                let mut cx = path_area.x + 12;
                let parts: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();
                let ty = path_area.y + (32 - ui.font.height()) / 2;
                cx += p.text(&ui.font, cx, ty, "/", theme::dim());
                for (i, part) in parts.iter().enumerate() {
                    let color = if i + 1 == parts.len() { theme::text() } else { theme::dim() };
                    let shown = ellipsize(&ui.font, part, 160);
                    cx += p.text(&ui.font, cx + 4, ty, &shown, color) + 4;
                    if i + 1 < parts.len() {
                        cx += p.text(&ui.font, cx + 4, ty, "›", theme::faint()) + 4;
                    }
                    if cx > path_area.right() - 20 {
                        break;
                    }
                }
            }
        }
        hits.add(path_area, Hit::Path);

        let header = Area::new(SIDEBAR + 1, top + TOOLBAR, ww() - SIDEBAR - 1, 34);
        p.fill(header, theme::bg());
        p.text(&ui.small, header.x + 52, header.y + 12, "NAME", theme::faint());
        p.text(&ui.small, ww() - 250, header.y + 12, "SIZE", theme::faint());
        p.text(&ui.small, ww() - 160, header.y + 12, "OWNER", theme::faint());
        p.fill(Area::new(header.x + 12, header.bottom() - 1, header.w - 24, 1), theme::border());
        hits.add(header, Hit::Header);

        hits.add(list, Hit::Blank);
        let saved = p.clip;
        p.set_clip(list);
        let first = (self.scroll / ROW) as usize;
        let cut_source = self.clipboard.as_ref().filter(|(_, cut)| *cut).map(|(p, _)| p.clone());
        for (i, entry) in self.entries.iter().enumerate().skip(first).take(visible_rows as usize + 1) {
            let row = Area::new(list.x + 8, list.y + i as i32 * ROW - self.scroll, list.w - 20, ROW - 2);
            let selected = self.selected == Some(i);
            ui::list_row(&mut p, row, selected, hover == Some(Hit::Row(i)));
            let kind = ui::file_kind(&entry.name, entry.is_dir, entry.mode, entry.kind);
            let faded = cut_source.as_deref() == Some(join(&self.path, &entry.name).as_str());
            if let Some(img) = ui.icon(&format!("files/{}", kind)) {
                p.image_scaled(img, Area::new(row.x + 12, row.y + 3, 24, 24), if faded { 110 } else { 255 });
            }
            let name_area = Area::new(row.x + 44, row.y, ww() - 300 - row.x - 44, row.h);
            let name_color = if selected { theme::on_selection() } else if faded { theme::faint() } else { theme::text() };
            match (&mut self.editing, selected) {
                (Some((field, true)), true) => {
                    ui::text_field(&mut p, ui, Area::new(name_area.x - 6, row.y + 1, name_area.w, row.h - 2), field, true, "");
                }
                _ => ui::text_in(&mut p, &ui.font, name_area.x, name_area, &entry.name, name_color),
            }
            let size = if entry.is_dir { String::from("—") } else { ui::human_size(entry.size) };
            ui::text_in(&mut p, &ui.font, ww() - 250, row, &size, theme::dim());
            ui::text_in(&mut p, &ui.font, ww() - 160, row, &users::name_of(entry.owner), theme::dim());
            hits.add(row.intersect(&list), Hit::Row(i));
        }
        if self.entries.is_empty() {
            let empty = match self.pick.as_ref().map(|p| p.mode) {
                Some(PickMode::Folder) => "No folders inside — Select folder picks this one",
                Some(_) => "Nothing here matches the filter",
                None => "This folder is empty",
            };
            ui::centered_text(&mut p, &ui.font, Area::new(list.x, list.y + 40, list.w, 30), empty, theme::faint());
        }
        ui::scrollbar(&mut p, Area::new(ww() - 8, list.y + 4, 4, list.h - 8), self.scroll, self.entries.len() as i32 * ROW, list.h);
        p.clip = saved;

        if let Some(pick) = &mut self.pick {
            let selected_name = self.selected.and_then(|i| self.entries.get(i)).filter(|e| !e.is_dir).map(|e| e.name.as_str());
            App::draw_pick_bar(&mut p, ui, hits, hover, pick, &self.message, selected_name);
            if !self.message.is_empty() && pick.mode != PickMode::Folder {
                let tw = ui.small.measure(&self.message) + 24;
                let toast = Area::new(list.x + (list.w - tw) / 2, list.bottom() - 40, tw, 28);
                p.rounded(toast, 14, 0x000000, 190);
                ui::centered_text(&mut p, &ui.small, toast, &self.message, 0xffffff);
            }
        } else {
            let status = Area::new(SIDEBAR + 1, hh() - STATUS, ww() - SIDEBAR - 1, STATUS);
            p.fill(status, theme::surface());
            p.fill(Area::new(status.x, status.y, status.w, 1), theme::border());
            let dirs = self.entries.iter().filter(|e| e.is_dir).count();
            let text = if !self.message.is_empty() {
                self.message.clone()
            } else if let Some(i) = self.selected {
                let e = &self.entries[i];
                format!("{}  ·  {}", e.name, if e.is_dir { String::from("folder") } else { ui::human_size(e.size) })
            } else {
                format!("{} folders, {} files", dirs, self.entries.len() - dirs)
            };
            ui::text_in(&mut p, &ui.small, status.x + 14, status, &text, theme::dim());
        }

        if let Some(context) = &self.context {
            let rows = context.items.len() as i32;
            let area = Area::new(context.x.min(ww() - 230), context.y.min(hh() - rows * 30 - 16).max(top), 220, rows * 30 + 12);
            p.shadow(area, 10, 12, theme::palette().shadow_alpha, 4);
            p.rounded(area, 10, theme::popup(), 255);
            p.rounded_border(area, 10, theme::border(), 255);
            for (i, (command, label, enabled)) in context.items.iter().enumerate() {
                let row = Area::new(area.x + 6, area.y + 6 + i as i32 * 30, area.w - 12, 30);
                let danger = *command == CMD_DELETE;
                let hot = *enabled && hover == Some(Hit::Context(*command));
                if hot {
                    p.rounded(row, 6, if danger { theme::danger() } else { theme::accent() }, 255);
                }
                let color = if !enabled { theme::faint() } else if hot { theme::accent_text() } else if danger { theme::danger() } else { theme::text() };
                ui::text_in(&mut p, &ui.font, row.x + 12, row, label, color);
                if *enabled {
                    hits.add(row, Hit::Context(*command));
                }
            }
        }

        if let Some(index) = self.confirm {
            if let Some(entry) = self.entries.get(index) {
                p.blend_fill(Area::new(0, 0, ww(), hh()), 0x000000, 90);
                let dialog = Area::new((ww() - 380) / 2, (hh() - 150) / 2, 380, 150);
                p.shadow(dialog, 12, 18, theme::palette().shadow_alpha, 6);
                p.rounded(dialog, 12, theme::popup(), 255);
                p.rounded_border(dialog, 12, theme::border(), 255);
                let title = ellipsize(&ui.medium, &format!("Delete “{}”?", entry.name), dialog.w - 40);
                p.text(&ui.medium, dialog.x + 20, dialog.y + 22, &title, theme::text());
                let detail = if entry.is_dir { "The folder and everything inside it will be removed." } else { "The file will be removed permanently." };
                p.text(&ui.small, dialog.x + 20, dialog.y + 52, detail, theme::dim());
                let yes = Area::new(dialog.right() - 112, dialog.bottom() - 52, 96, 34);
                let no = Area::new(yes.x - 104, yes.y, 96, 34);
                ui::button(&mut p, ui, no, "Cancel", Style::Secondary, hover == Some(Hit::ConfirmNo), true);
                ui::button(&mut p, ui, yes, "Delete", Style::Danger, hover == Some(Hit::ConfirmYes), true);
                hits.add(dialog, Hit::Blank);
                hits.add(no, Hit::ConfirmNo);
                hits.add(yes, Hit::ConfirmYes);
            }
        }
        if self.pick.is_none() {
            self.menu.draw_dropdown(&mut p, ui, ww(), hh());
        }
        self.window.present();
    }
}

fn main() -> i32 {
    let mut ui = Ui::load();
    ui.preload(&["ui/back", "ui/forward", "ui/up", "ui/home", "ui/folder", "ui/image", "ui/disk", "ui/video", "ui/chevron-down"]);
    for kind in ["folder", "file", "text", "image", "video", "exec", "script", "device"] {
        ui.load_icon(&format!("files/{}", kind));
    }
    let (pick, start_arg) = parse_pick(env::args());
    let title = pick.as_ref().map(|p| p.title.clone()).unwrap_or_else(|| String::from("Files"));
    let (w, h) = if pick.is_some() { (780, 520) } else { (INITIAL_W, INITIAL_H) };
    let window = match Window::open(&title, w as u32, h as u32) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hxfiles: {}", e);
            return 1;
        }
    };
    let start = start_arg.unwrap_or_else(ui::home_dir);
    let mut app = App {
        window,
        ui,
        menu: MenuBar::new(Vec::new()),
        path: String::new(),
        entries: Vec::new(),
        selected: None,
        scroll: 0,
        history: Vec::new(),
        future: Vec::new(),
        hits: Hits::new(),
        hover: None,
        mouse: (0, 0),
        editing: None,
        context: None,
        message: String::new(),
        last_click: (0, usize::MAX),
        pick,
        clipboard: None,
        show_hidden: false,
        sort: Sort::Name,
        confirm: None,
    };
    app.window.set_icon("files");
    app.window.set_min_size(560, 360);
    let home = ui::home_dir();
    for folder in ["Documents", "Videos"] {
        let path = format!("{}/{}", home, folder);
        if sys::stat(&path).is_err() {
            sys::mkdir(&path);
        }
    }
    if !app.load(&start) && !app.load(&home) {
        app.load("/");
    }
    app.draw();

    loop {
        let Some(event) = app.window.wait_event(-1) else {
            continue;
        };
        let mut redraw = false;
        match event {
            Event::Close { .. } => {
                if app.pick.is_some() {
                    app.finish(PICK_CANCELLED, "");
                }
                return 0;
            }
            Event::Resize { .. } => {
                app.clamp_scroll();
                redraw = true;
            }
            Event::Theme { .. } => redraw = true,
            Event::Focus { focused: false, .. } => {
                redraw = app.menu.close();
            }
            Event::Mouse { x, y, kind, wheel, .. } => {
                app.mouse = (x, y);
                match kind {
                    MOUSE_MOVE | MOUSE_LEAVE => {
                        if kind == MOUSE_LEAVE {
                            redraw |= app.menu.leave();
                        } else if app.pick.is_none() && app.menu.motion(x, y) {
                            redraw = true;
                        }
                        let over_menu = app.pick.is_none() && app.menu.contains(x, y);
                        let hover = if kind == MOUSE_LEAVE || over_menu { None } else { app.hits.at(x, y) };
                        let hover = match (app.confirm.is_some(), hover) {
                            (true, Some(Hit::ConfirmYes)) | (true, Some(Hit::ConfirmNo)) => hover,
                            (true, _) => None,
                            _ => hover,
                        };
                        if hover != app.hover {
                            app.hover = hover;
                            redraw = true;
                        }
                    }
                    MOUSE_WHEEL => {
                        if !app.menu.is_open() && app.confirm.is_none() {
                            let max = (app.entries.len() as i32 * ROW - app.list_area().h).max(0);
                            app.scroll = (app.scroll + wheel * ROW * 2).clamp(0, max);
                            app.context = None;
                            app.hover = app.hits.at(x, y);
                            redraw = true;
                        }
                    }
                    MOUSE_PRESS => {
                        redraw = true;
                        if app.pick.is_none() {
                            match app.menu.press(x, y) {
                                MenuResult::Command(c) => {
                                    app.context = None;
                                    if !app.command(c) {
                                        return 0;
                                    }
                                    app.draw();
                                    continue;
                                }
                                MenuResult::Consumed => {
                                    app.context = None;
                                    app.draw();
                                    continue;
                                }
                                MenuResult::Ignored => {}
                            }
                        }
                        let hit = app.hits.at(x, y);
                        if app.confirm.is_some() {
                            match hit {
                                Some(Hit::ConfirmYes) => {
                                    if let Some(index) = app.confirm.take() {
                                        app.delete_now(index);
                                    }
                                }
                                Some(Hit::ConfirmNo) => app.confirm = None,
                                _ => {}
                            }
                            app.draw();
                            continue;
                        }
                        if let Some(Hit::Context(command)) = hit {
                            app.context = None;
                            if !app.command(command) {
                                return 0;
                            }
                            app.draw();
                            continue;
                        }
                        app.context = None;
                        if app.editing.is_some() && !matches!(hit, Some(Hit::Path)) {
                            app.commit_edit();
                        }
                        if let Some(pick) = &mut app.pick {
                            pick.name_focused = pick.mode == PickMode::Save && !matches!(hit, Some(Hit::Path));
                        }
                        match hit {
                            Some(Hit::Back) => app.back(),
                            Some(Hit::Forward) => app.forward(),
                            Some(Hit::Up) => app.up(),
                            Some(Hit::Home) => app.navigate(&ui::home_dir()),
                            Some(Hit::Path) => {
                                if app.editing.is_none() {
                                    app.editing = Some((TextField::new(&app.path), false));
                                }
                            }
                            Some(Hit::Place(i)) => {
                                if let Some((_, path, _)) = places().get(i) {
                                    sys::mkdir(path);
                                    let path = path.clone();
                                    app.navigate(&path);
                                }
                            }
                            Some(Hit::Filter) => app.cycle_filter(),
                            Some(Hit::NameField) => {
                                if let Some(pick) = &mut app.pick {
                                    let font = &app.ui.font;
                                    pick.name.click(font, x - (SIDEBAR + 1 + 14 + 10));
                                }
                            }
                            Some(Hit::Cancel) => app.finish(PICK_CANCELLED, ""),
                            Some(Hit::Accept) => app.accept(),
                            Some(Hit::Row(i)) => {
                                if wheel == 2 && app.pick.is_none() {
                                    app.open_context(x, y, Some(i));
                                } else {
                                    let now = sys::uptime_ms();
                                    let double = app.last_click.1 == i && now.saturating_sub(app.last_click.0) < 600;
                                    app.last_click = (now, i);
                                    app.selected = Some(i);
                                    app.message.clear();
                                    if let Some(pick) = &mut app.pick {
                                        if pick.mode == PickMode::Save {
                                            if let Some(entry) = app.entries.get(i).filter(|e| !e.is_dir) {
                                                pick.name.set(&entry.name);
                                            }
                                        }
                                    }
                                    if double {
                                        app.open(i);
                                    }
                                    app.refresh_menus();
                                }
                            }
                            Some(Hit::Blank) => {
                                if wheel == 2 && app.pick.is_none() {
                                    app.open_context(x, y, None);
                                } else {
                                    app.selected = None;
                                    app.refresh_menus();
                                }
                            }
                            _ => {}
                        }
                        app.hover = None;
                    }
                    _ => {}
                }
            }
            Event::Key { code, .. } => {
                redraw = true;
                if app.pick.is_none() {
                    match app.menu.key(code) {
                        MenuResult::Command(c) => {
                            if !app.command(c) {
                                return 0;
                            }
                            app.draw();
                            continue;
                        }
                        MenuResult::Consumed => {
                            app.draw();
                            continue;
                        }
                        MenuResult::Ignored => {}
                    }
                }
                if app.confirm.is_some() {
                    match code {
                        10 => {
                            if let Some(index) = app.confirm.take() {
                                app.delete_now(index);
                            }
                        }
                        27 => app.confirm = None,
                        _ => {}
                    }
                } else if let Some((field, _)) = &mut app.editing {
                    match field.key(code) {
                        FieldAction::Submit => app.commit_edit(),
                        _ if code == 27 => app.editing = None,
                        _ => {}
                    }
                } else if app.pick.as_ref().map(|p| p.name_focused).unwrap_or(false) && !matches!(code, -1 | -2 | 27) {
                    let action = app.pick.as_mut().map(|p| p.name.key(code));
                    if let Some(FieldAction::Submit) = action {
                        app.accept();
                    }
                } else {
                    let shortcut = if app.pick.is_none() {
                        match code {
                            14 => Some(CMD_NEW_FOLDER),
                            3 => Some(CMD_COPY),
                            24 => Some(CMD_CUT),
                            22 => Some(CMD_PASTE),
                            18 => Some(CMD_REFRESH),
                            17 | 23 => Some(CMD_QUIT),
                            -7 => Some(CMD_DELETE),
                            -42 => Some(CMD_RENAME),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(command) = shortcut {
                        if !app.command(command) {
                            return 0;
                        }
                    } else {
                        match code {
                            -1 => app.selected = Some(app.selected.map(|s| s.saturating_sub(1)).unwrap_or(0)),
                            -2 => app.selected = Some(app.selected.map(|s| (s + 1).min(app.entries.len().saturating_sub(1))).unwrap_or(0)),
                            10 => {
                                if let Some(i) = app.selected {
                                    if app.pick.as_ref().map(|p| p.mode == PickMode::Folder).unwrap_or(false) && !app.entries.get(i).map(|e| e.is_dir).unwrap_or(false) {
                                        app.accept();
                                    } else {
                                        app.open(i);
                                    }
                                } else if app.pick.is_some() {
                                    app.accept();
                                }
                            }
                            8 => app.up(),
                            27 => {
                                if app.pick.is_some() && app.context.is_none() {
                                    app.finish(PICK_CANCELLED, "");
                                }
                                app.context = None;
                            }
                            _ => redraw = false,
                        }
                        if app.entries.is_empty() {
                            app.selected = None;
                        }
                        app.reveal_selected();
                        app.refresh_menus();
                    }
                }
            }
            _ => {}
        }
        if redraw {
            app.draw();
        }
    }
}

entry!(main);
