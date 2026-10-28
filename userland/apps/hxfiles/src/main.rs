#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

mod about;
mod props;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxport::Entry;
use hxtoolkit::{button, column, divider, dropdown, icon, icon_button, input, list, row, text, Align, App, Context, Element, Key, KeyPress, Length, MenuItem, Settings, TextSize, Tone};

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
        let lower = name.to_lowercase();
        self.extensions.iter().any(|e| e == "*" || lower.ends_with(&format!(".{}", e)))
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
    title: String,
    filters: Vec<Filter>,
    active: usize,
    name: String,
}

#[derive(Clone, PartialEq)]
enum Editing {
    Rename(String),
    NewFolder,
    NewFile,
}

#[derive(Clone)]
enum Msg {
    Select(usize),
    Activate(usize),
    Context(Option<usize>),
    Place(usize),
    Back,
    Forward,
    Up,

    PathTyped(String),
    PathGo,
    Search(String),
    Open,
    OpenTerminal,
    StartRename,
    StartNew(bool),
    EditName(String),
    CommitName,
    CancelName,
    Delete,
    DeleteConfirmed(bool),
    Cut,
    Copy,
    Paste,
    CopyPath,
    Properties,
    Places,
    Unmount(usize),
    About,
    Shortcuts,
    ToggleHidden,
    ToggleGrid,
    Sort(Sort),
    Refresh,
    Quit,
    PickName(String),
    PickFilter(usize),
    Accept,
    Cancel,
}

struct Files {
    path: String,
    typed: String,
    entries: Vec<Entry>,
    selected: Option<usize>,
    history: Vec<String>,
    future: Vec<String>,
    places: Vec<Place>,
    clipboard: Option<(String, bool)>,
    show_hidden: bool,
    grid: bool,
    sort: Sort,
    search: String,
    editing: Option<Editing>,
    name: String,
    message: (String, Tone),
    pick: Option<Pick>,
    unmounting: Option<(String, String)>,
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{}{}", dir, name) } else { format!("{}/{}", dir, name) }
}

fn base_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("/")
}

fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => String::from(&path[..i]),
    }
}

const VIDEO: [&str; 4] = ["mp4", "m4v", "mov", "3gp"];

fn kind_of(e: &Entry) -> &'static str {
    let lower = e.name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, x)| x).unwrap_or("");
    if e.is_dir {
        "folder"
    } else if matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "bmp") {
        "image"
    } else if VIDEO.contains(&ext) {
        "video"
    } else if ext == "sh" {
        "script"
    } else if e.mode & 0o111 != 0 {
        "exec"
    } else {
        "text"
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "folder" => "Folder",
        "image" => "Picture",
        "video" => "Video",
        "script" => "Script",
        "exec" => "Program",
        _ => "Document",
    }
}

fn parse_filters(spec: &str) -> Vec<Filter> {
    let mut filters = Vec::new();
    for part in spec.split(';').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let (name, items) = part.split_once('|').unwrap_or((part, "*"));
        let extensions: Vec<String> = items.split(',').map(|e| e.trim().trim_start_matches("*.").trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect();
        if !extensions.is_empty() {
            filters.push(Filter { name: String::from(name.trim()), extensions });
        }
    }
    if !filters.iter().any(|f| f.extensions.iter().any(|e| e == "*")) {
        filters.push(Filter { name: String::from("All files"), extensions: alloc::vec![String::from("*")] });
    }
    filters
}

fn copy_tree(from: &str, to: &str) -> Result<(), String> {
    if hxport::is_dir(from) {
        if to.starts_with(&format!("{}/", from.trim_end_matches('/'))) {
            return Err(String::from("a folder cannot be copied into itself"));
        }
        if !hxport::mkdir(to) && !hxport::is_dir(to) {
            return Err(format!("cannot create {}", base_name(to)));
        }
        for entry in hxport::entries(from).unwrap_or_default() {
            copy_tree(&join(from, &entry.name), &join(to, &entry.name))?;
        }
        return Ok(());
    }
    let data = hxport::read(from).ok_or_else(|| format!("cannot read {}", base_name(from)))?;
    if !hxport::write(to, &data) {
        return Err(format!("cannot write {}", base_name(to)));
    }
    if let Some(e) = hxport::entries(&parent(from)).and_then(|l| l.into_iter().find(|e| e.name == base_name(from))) {
        hxport::chmod(to, e.mode & 0o7777);
    }
    Ok(())
}

fn remove_tree(path: &str) -> bool {
    if hxport::is_dir(path) {
        for entry in hxport::entries(path).unwrap_or_default() {
            if !remove_tree(&join(path, &entry.name)) {
                return false;
            }
        }
    }
    hxport::remove(path)
}

fn free_name(dir: &str, name: &str) -> String {
    if !hxport::exists(&join(dir, name)) {
        return String::from(name);
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..).map(|n| format!("{} ({}){}", stem, n, ext)).find(|c| !hxport::exists(&join(dir, c))).unwrap_or_default()
}

#[derive(Clone)]
struct Place {
    name: String,
    path: String,
    glyph: &'static str,
    removable: bool,
}

fn place(name: &str, path: String, glyph: &'static str) -> Place {
    Place { name: String::from(name), path, glyph, removable: false }
}

fn unescape(field: &str) -> String {
    field.replace("\\040", " ")
}

fn places() -> Vec<Place> {
    let home = hxport::home();
    let mut out = alloc::vec![
        place("Home", home.clone(), "ui/home"),
        place("Documents", format!("{}/Documents", home), "ui/folder"),
        place("Pictures", format!("{}/Pictures", home), "ui/image"),
        place("Videos", format!("{}/Videos", home), "ui/video"),
        place("Computer", String::from("/"), "ui/disk"),
    ];
    for line in hxport::read_to_string("/proc/mounts").unwrap_or_default().lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let f: Vec<&str> = if f.len() >= 3 { f } else { line.split(' ').collect() };
        if f.len() < 3 {
            continue;
        }
        let path = unescape(f[1]);
        if !(path.starts_with("/media/") || path.starts_with("/mnt/") || path.starts_with("/run/media/")) {
            continue;
        }
        let kind = f[2];
        let removable = matches!(kind, "iso9660" | "udf" | "vfat" | "exfat" | "ntfs" | "fuseblk");
        let glyph = match kind {
            "iso9660" | "udf" => "ui/optical",
            "vfat" | "exfat" | "ntfs" | "fuseblk" => "ui/usb",
            _ => "ui/disk",
        };
        let label = f.get(6).map(|l| l.trim()).filter(|l| !l.is_empty()).map(String::from).unwrap_or_else(|| String::from(base_name(&path)));
        out.push(Place { name: label, path, glyph, removable });
    }
    out
}

#[cfg(target_os = "hamix")]
fn unmount_media(path: &str) -> bool {
    hxport::spawn("/usr/bin/hxmount", &["unmount", path]).is_ok()
}

#[cfg(not(target_os = "hamix"))]
fn unmount_media(path: &str) -> bool {
    hxport::spawn("udisksctl", &["unmount", "-b", path]).is_ok() || hxport::spawn("umount", &[path]).is_ok()
}

#[cfg(target_os = "hamix")]
fn open_default(path: &str, is_dir: bool) -> bool {
    let lower = path.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, x)| String::from(x)).unwrap_or_default();
    let program = if is_dir {
        "/usr/bin/hxfiles"
    } else if matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        "/usr/bin/hxview"
    } else if VIDEO.contains(&ext.as_str()) {
        "/usr/bin/hxvideo"
    } else if ext == "wav" {
        return hxport::spawn("/usr/bin/hxsound", &["play", path]).is_ok();
    } else if ext == "sh" {
        let quoted = format!("bash '{}'", path.replace('\'', "'\\''"));
        return hxport::spawn("/usr/bin/hxterm", &[&quoted]).is_ok();
    } else if hxport::read(path).map(|b| b.starts_with(b"\x7fELF")).unwrap_or(false) {
        return hxport::spawn(path, &[]).is_ok();
    } else {
        "/usr/bin/hxnotes"
    };
    hxport::spawn(program, &[path]).is_ok()
}

#[cfg(not(target_os = "hamix"))]
fn open_default(path: &str, _is_dir: bool) -> bool {
    hxport::spawn("xdg-open", &[path]).is_ok()
}

#[cfg(target_os = "hamix")]
fn open_terminal(dir: &str) -> bool {
    hxport::chdir(dir) && hxport::spawn("/usr/bin/hxterm", &[]).is_ok()
}

#[cfg(not(target_os = "hamix"))]
fn open_terminal(dir: &str) -> bool {
    hxport::chdir(dir) && hxport::spawn("x-terminal-emulator", &[]).is_ok()
}

impl Files {
    fn new() -> Files {
        let args = hxport::args();
        let value = |key: &str| args.iter().find_map(|a| a.strip_prefix(key).map(String::from));
        let mut start = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned();
        let pick = value("--pick=").map(|mode| {
            let mode = match mode.as_str() {
                "folder" => PickMode::Folder,
                "save" => PickMode::Save,
                _ => PickMode::Open,
            };
            let fallback = match mode {
                PickMode::Open => "Open a file",
                PickMode::Folder => "Choose a folder",
                PickMode::Save => "Save as",
            };
            let mut name = String::new();
            if let Some(s) = value("--start=").filter(|s| !s.is_empty()) {
                if mode == PickMode::Save && !hxport::is_dir(&s) {
                    name = String::from(base_name(&s));
                    start = Some(if s.contains('/') { parent(&s) } else { hxport::home() });
                } else {
                    start = Some(s);
                }
            }
            Pick { mode, title: value("--title=").filter(|t| !t.is_empty()).unwrap_or_else(|| String::from(fallback)), filters: parse_filters(&value("--filter=").unwrap_or_default()), active: 0, name }
        });
        let home = hxport::home();
        for folder in ["Documents", "Pictures", "Videos"] {
            let path = join(&home, folder);
            if !hxport::exists(&path) {
                hxport::mkdir(&path);
            }
        }
        let mut files = Files {
            path: String::new(),
            typed: String::new(),
            entries: Vec::new(),
            selected: None,
            history: Vec::new(),
            future: Vec::new(),
            places: places(),
            clipboard: None,
            show_hidden: false,
            grid: false,
            sort: Sort::Name,
            search: String::new(),
            editing: None,
            name: String::new(),
            message: (String::new(), Tone::Dim),
            pick,
            unmounting: None,
        };
        let start = start.unwrap_or_else(|| home.clone());
        if !files.load(&start) && !files.load(&home) {
            files.load("/");
        }
        files
    }

    fn visible(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        (0..self.entries.len()).filter(|i| needle.is_empty() || self.entries[*i].name.to_lowercase().contains(&needle)).collect()
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    fn sort_entries(&mut self) {
        let sort = self.sort;
        self.entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
                Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Sort::Size => b.size.cmp(&a.size).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                Sort::Kind => kind_of(a).cmp(kind_of(b)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            })
        });
    }

    fn load(&mut self, path: &str) -> bool {
        let Some(list) = hxport::entries(path) else {
            self.message = (format!("Cannot open {}", path), Tone::Danger);
            return false;
        };
        let hidden = self.show_hidden;
        let pick = self.pick.as_ref();
        self.entries = list
            .into_iter()
            .filter(|e| hidden || !e.name.starts_with('.'))
            .filter(|e| match pick {
                None => true,
                Some(p) if p.mode == PickMode::Folder => e.is_dir,
                Some(p) => e.is_dir || p.filters.get(p.active).map(|f| f.matches(&e.name)).unwrap_or(true),
            })
            .collect();
        self.sort_entries();
        self.path = String::from(path);
        self.typed = self.path.clone();
        self.selected = None;
        self.editing = None;
        self.search.clear();
        true
    }

    fn refresh_places(&mut self) {
        let fresh = places();
        if let Some((path, name)) = self.unmounting.take() {
            self.message = if fresh.iter().any(|p| p.path == path) {
                (format!("{} is in use and was not unmounted", name), Tone::Danger)
            } else {
                (format!("{} was unmounted and can be removed safely", name), Tone::Success)
            };
        }
        let lost = self.places.iter().filter(|p| p.removable && !fresh.iter().any(|f| f.path == p.path)).any(|p| self.path == p.path || self.path.starts_with(&format!("{}/", p.path)));
        let changed = fresh.len() != self.places.len() || fresh.iter().zip(self.places.iter()).any(|(a, b)| a.path != b.path || a.name != b.name);
        self.places = fresh;
        if lost {
            let home = hxport::home();
            self.navigate(&home);
        } else if changed && self.path.starts_with("/media") {
            self.reload(None);
        }
    }

    fn reload(&mut self, select: Option<&str>) {
        let path = self.path.clone();
        let message = core::mem::take(&mut self.message);
        self.load(&path);
        self.message = message;
        if let Some(name) = select {
            self.selected = self.entries.iter().position(|e| e.name == name);
        }
    }

    fn navigate(&mut self, path: &str) {
        let old = self.path.clone();
        if path != old && self.load(path) {
            self.history.push(old);
            self.future.clear();
            self.message.0.clear();
        }
    }

    fn open(&mut self, index: usize, cx: &mut Context<Msg>) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let full = join(&self.path, &entry.name);
        if entry.is_dir {
            self.navigate(&full);
            return;
        }
        if let Some(mode) = self.pick.as_ref().map(|p| p.mode) {
            if mode == PickMode::Save {
                if let Some(p) = self.pick.as_mut() {
                    p.name = entry.name.clone();
                }
            }
            self.accept(cx);
            return;
        }
        if !open_default(&full, false) {
            self.message = (format!("No application can open {}", entry.name), Tone::Danger);
        }
    }

    fn accept(&mut self, cx: &mut Context<Msg>) {
        let Some(pick) = &self.pick else {
            return;
        };
        let chosen = match pick.mode {
            PickMode::Open => match self.selected_entry() {
                Some(e) if e.is_dir => {
                    let full = join(&self.path, &e.name);
                    self.navigate(&full);
                    return;
                }
                Some(e) => join(&self.path, &e.name),
                None => {
                    self.message = (String::from("Choose a file first"), Tone::Warning);
                    return;
                }
            },
            PickMode::Folder => match self.selected_entry() {
                Some(e) if e.is_dir => join(&self.path, &e.name),
                _ => self.path.clone(),
            },
            PickMode::Save => {
                let name = String::from(pick.name.trim());
                if name.is_empty() || name.contains('/') {
                    self.message = (String::from("Type a file name without '/'"), Tone::Warning);
                    return;
                }
                let full = join(&self.path, &name);
                if hxport::is_dir(&full) {
                    self.navigate(&full);
                    return;
                }
                full
            }
        };
        hxport::stdout(&format!("{}\n", chosen));
        cx.quit();
    }

    fn start_edit(&mut self, editing: Editing) {
        self.name = match &editing {
            Editing::Rename(n) => n.clone(),
            Editing::NewFolder => free_name(&self.path, "New folder"),
            Editing::NewFile => free_name(&self.path, "New document.txt"),
        };
        self.editing = Some(editing);
    }

    fn commit_name(&mut self) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        let name = String::from(self.name.trim());
        if name.is_empty() || name.contains('/') {
            self.message = (String::from("A name cannot be empty or contain '/'"), Tone::Danger);
            self.editing = Some(editing);
            return;
        }
        let target = join(&self.path, &name);
        if hxport::exists(&target) && editing != Editing::Rename(name.clone()) {
            self.message = (format!("“{}” already exists", name), Tone::Danger);
            self.editing = Some(editing);
            return;
        }
        let ok = match &editing {
            Editing::Rename(old) => old == &name || hxport::rename(&join(&self.path, old), &target),
            Editing::NewFolder => hxport::mkdir(&target),
            Editing::NewFile => hxport::write(&target, b""),
        };
        if ok {
            self.message.0.clear();
            self.reload(Some(&name));
        } else {
            self.message = (format!("Cannot write in {} — permission denied?", self.path), Tone::Danger);
        }
    }

    fn paste(&mut self) {
        let Some((source, cut)) = self.clipboard.clone() else {
            return;
        };
        if cut && parent(&source) == self.path {
            self.clipboard = None;
            return;
        }
        let target_name = free_name(&self.path, base_name(&source));
        let target = join(&self.path, &target_name);
        let result = if cut && hxport::rename(&source, &target) {
            Ok(())
        } else {
            copy_tree(&source, &target).and_then(|_| if cut && !remove_tree(&source) { Err(String::from("copied, but the original could not be removed")) } else { Ok(()) })
        };
        match result {
            Ok(()) => {
                self.message = (format!("{} {}", if cut { "Moved" } else { "Copied" }, target_name), Tone::Success);
                if cut {
                    self.clipboard = None;
                }
            }
            Err(e) => self.message = (format!("Paste failed: {}", e), Tone::Danger),
        }
        self.reload(Some(&target_name));
    }

    fn entry_menu(&self) -> Vec<MenuItem<Msg>> {
        let paste = MenuItem::new("Paste", Msg::Paste);
        alloc::vec![
            MenuItem::new("Open", Msg::Open).shortcut("Enter"),
            MenuItem::separator(),
            MenuItem::new("Cut", Msg::Cut).shortcut("Ctrl+X"),
            MenuItem::new("Copy", Msg::Copy).shortcut("Ctrl+C"),
            if self.clipboard.is_some() { paste } else { paste.disabled() },
            MenuItem::separator(),
            MenuItem::new("Rename…", Msg::StartRename).shortcut("F2"),
            MenuItem::new("Copy path", Msg::CopyPath),
            MenuItem::new("Properties", Msg::Properties).shortcut("Alt+Enter"),
            MenuItem::separator(),
            MenuItem::new("Delete", Msg::Delete).shortcut("Delete"),
        ]
    }

    fn folder_menu(&self) -> Vec<MenuItem<Msg>> {
        let paste = MenuItem::new("Paste", Msg::Paste).shortcut("Ctrl+V");
        alloc::vec![
            MenuItem::new("New folder", Msg::StartNew(true)).shortcut("Ctrl+Shift+N"),
            MenuItem::new("New document", Msg::StartNew(false)),
            MenuItem::separator(),
            if self.clipboard.is_some() { paste } else { paste.disabled() },
            MenuItem::separator(),
            MenuItem::new("Open terminal here", Msg::OpenTerminal),
            MenuItem::new("Show hidden files", Msg::ToggleHidden).checked(self.show_hidden),
            MenuItem::new("Refresh", Msg::Refresh),
            MenuItem::separator(),
            MenuItem::new("Properties", Msg::Properties).shortcut("Alt+Enter"),
        ]
    }

    fn toolbar(&self) -> Element<Msg> {
        let back = button("").icon("ui/back").flat().tooltip("Back").on_press_maybe((!self.history.is_empty()).then_some(Msg::Back));
        let forward = button("").icon("ui/forward").flat().tooltip("Forward").on_press_maybe((!self.future.is_empty()).then_some(Msg::Forward));
        let up = button("").icon("ui/up").flat().tooltip("Enclosing folder").on_press_maybe((self.path != "/").then_some(Msg::Up));
        let view = button("").icon(if self.grid { "ui/menu" } else { "ui/apps" }).flat().tooltip(if self.grid { "List view" } else { "Icon view" }).on_press(Msg::ToggleGrid);
        row![back, forward, up, input(self.typed.clone(), "Location", Msg::PathTyped).on_submit(Msg::PathGo).width(Length::Fill), input(self.search.clone(), "Search", Msg::Search).width(180u32), view]
            .spacing(6)
            .align(Align::Center)
            .padding([8, 10])
            .into()
    }

    fn sidebar(&self) -> Element<Msg> {
        let rows: Vec<Element<Msg>> = self
            .places
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = text(p.name.clone()).width(Length::Fill);
                if p.removable {
                    row![icon(p.glyph).size(18u32, 18u32), name, icon_button("ui/eject", "Unmount").flat().size(TextSize::Small).on_press(Msg::Unmount(i))].spacing(10).align(Align::Center).into()
                } else {
                    row![icon(p.glyph).size(18u32, 18u32), name].spacing(10).align(Align::Center).into()
                }
            })
            .collect();
        let selected = self.places.iter().position(|p| p.path == self.path);
        column![list(rows).selected(selected).on_select(Msg::Place).row_height(34).height(Length::Fill)].padding([8, 6]).sidebar().width(190u32).height(Length::Fill).into()
    }

    fn listing(&self) -> Element<Msg> {
        let visible = self.visible();
        let selected = self.selected.and_then(|s| visible.iter().position(|v| *v == s));
        let rows: Vec<Element<Msg>> = visible
            .iter()
            .map(|i| {
                let e = &self.entries[*i];
                let kind = kind_of(e);
                let cut = self.clipboard.as_ref().map(|(p, c)| *c && *p == join(&self.path, &e.name)).unwrap_or(false);
                let name = text(e.name.clone());
                let name = if cut { name.tone(Tone::Faint) } else { name };
                if self.grid {
                    column![icon(format!("files/{}", kind)).size(48u32, 48u32), name.small().align(Align::Center).width(Length::Fill)].spacing(6).align(Align::Center).width(Length::Fill).into()
                } else {
                    let size = if e.is_dir { hxport::entries(&join(&self.path, &e.name)).map(|l| format!("{} item{}", l.len(), if l.len() == 1 { "" } else { "s" })).unwrap_or_default() } else { hxport::human_size(e.size) };
                    row![icon(format!("files/{}", kind)).size(22u32, 22u32), name.width(Length::Fill), text(size).small().tone(Tone::Dim).width(110u32), text(kind_label(kind)).small().tone(Tone::Dim).width(90u32)].spacing(12).align(Align::Center).into()
                }
            })
            .collect();
        let empty = rows.is_empty();
        let mut l = list(rows).selected(selected).on_select(move |i| Msg::Select(i)).on_activate(Msg::Activate).on_context(Msg::Context);
        l = if self.grid { l.grid(104, 96) } else { l.row_height(36) };
        let mut body = column![].spacing(0).width(Length::Fill).height(Length::Fill);
        if !self.grid {
            body = body.push(
                row![text("Name").small().strong().tone(Tone::Dim).width(Length::Fill), text("Size").small().strong().tone(Tone::Dim).width(110u32), text("Type").small().strong().tone(Tone::Dim).width(90u32)]
                    .spacing(12)
                    .padding([6, 23, 6, 46]),
            );
            body = body.push(divider());
        }
        if empty {
            let note = if self.search.is_empty() { "This folder is empty" } else { "Nothing matches the search" };
            body = body.push(column![icon("ui/folder").size(48u32, 48u32).tone(Tone::Faint), text(note).tone(Tone::Dim)].spacing(10).align(Align::Center).padding([60, 0]).width(Length::Fill));
        }
        body.push(l).into()
    }

    fn edit_bar(&self) -> Option<Element<Msg>> {
        let editing = self.editing.as_ref()?;
        let (label, action) = match editing {
            Editing::Rename(old) => (format!("Rename “{}” to", old), "Rename"),
            Editing::NewFolder => (String::from("New folder name"), "Create"),
            Editing::NewFile => (String::from("New document name"), "Create"),
        };
        Some(
            row![text(label), input(self.name.clone(), "", Msg::EditName).focused().on_submit(Msg::CommitName).width(Length::Fill), button("Cancel").on_press(Msg::CancelName), button(action).primary().on_press(Msg::CommitName)]
                .spacing(10)
                .align(Align::Center)
                .padding([8, 12])
                .into(),
        )
    }

    fn bottom(&self) -> Element<Msg> {
        match &self.pick {
            Some(pick) => {
                let mut r = row![].spacing(10).align(Align::Center).padding([10, 12]);
                if pick.mode == PickMode::Save {
                    r = r.push(text("Name")).push(input(pick.name.clone(), "File name", Msg::PickName).focused().on_submit(Msg::Accept).width(Length::Fill));
                } else {
                    let status = if self.message.0.is_empty() { self.selected_entry().map(|e| e.name.clone()).unwrap_or_default() } else { self.message.0.clone() };
                    r = r.push(text(status).tone(Tone::Dim).width(Length::Fill));
                }
                if pick.mode != PickMode::Folder && pick.filters.len() > 1 {
                    r = r.push(dropdown(pick.filters.iter().map(|f| f.label()), Some(pick.active), Msg::PickFilter).width(200u32));
                }
                let label = match pick.mode {
                    PickMode::Open => "Open",
                    PickMode::Folder => "Choose",
                    PickMode::Save => "Save",
                };
                r.push(button("Cancel").on_press(Msg::Cancel)).push(button(label).primary().on_press(Msg::Accept)).into()
            }
            None => {
                let count = self.visible().len();
                let status = if self.message.0.is_empty() {
                    match self.selected_entry() {
                        Some(e) if e.is_dir => format!("“{}” selected", e.name),
                        Some(e) => format!("“{}” selected ({})", e.name, hxport::human_size(e.size)),
                        None => format!("{} item{}", count, if count == 1 { "" } else { "s" }),
                    }
                } else {
                    self.message.0.clone()
                };
                row![text(status).small().tone(if self.message.0.is_empty() { Tone::Dim } else { self.message.1 })].padding([6, 12]).into()
            }
        }
    }
}

impl App for Files {
    type Message = Msg;

    fn title(&self) -> String {
        let folder = base_name(&self.path);
        match &self.pick {
            Some(p) => format!("{} — {}", p.title, folder),
            None => format!("{} — Files", folder),
        }
    }

    fn view(&self) -> Element<Msg> {
        let mut main = column![self.toolbar(), divider()].spacing(0).width(Length::Fill).height(Length::Fill);
        if let Some(bar) = self.edit_bar() {
            main = main.push(bar).push(divider());
        }
        main = main.push(self.listing()).push(divider()).push(self.bottom());
        row![self.sidebar(), main].spacing(0).align(Align::Stretch).width(Length::Fill).height(Length::Fill).into()
    }

    fn menu(&self) -> Vec<MenuItem<Msg>> {
        if self.pick.is_some() {
            return Vec::new();
        }
        let has = self.selected.is_some();
        let enable = |m: MenuItem<Msg>, on: bool| if on { m } else { m.disabled() };
        let mut go = alloc::vec![
            enable(MenuItem::new("Back", Msg::Back).shortcut("Alt+Left"), !self.history.is_empty()),
            enable(MenuItem::new("Forward", Msg::Forward).shortcut("Alt+Right"), !self.future.is_empty()),
            enable(MenuItem::new("Enclosing folder", Msg::Up).shortcut("Alt+Up"), self.path != "/"),
            MenuItem::separator(),
        ];
        for (i, p) in self.places.iter().enumerate() {
            go.push(MenuItem::new(p.name.clone(), Msg::Place(i)));
        }
        alloc::vec![
            MenuItem::submenu(
                "File",
                alloc::vec![
                    MenuItem::new("New folder", Msg::StartNew(true)).shortcut("Ctrl+Shift+N"),
                    MenuItem::new("New document", Msg::StartNew(false)),
                    MenuItem::separator(),
                    enable(MenuItem::new("Open", Msg::Open), has),
                    MenuItem::new("Open terminal here", Msg::OpenTerminal),
                    MenuItem::separator(),
                    enable(MenuItem::new("Rename…", Msg::StartRename).shortcut("F2"), has),
                    enable(MenuItem::new("Delete", Msg::Delete).shortcut("Delete"), has),
                    MenuItem::new("Properties", Msg::Properties).shortcut("Alt+Enter"),
                    MenuItem::separator(),
                    MenuItem::new("Close", Msg::Quit).shortcut("Ctrl+Q"),
                ]
            ),
            MenuItem::submenu(
                "Edit",
                alloc::vec![
                    enable(MenuItem::new("Cut", Msg::Cut).shortcut("Ctrl+X"), has),
                    enable(MenuItem::new("Copy", Msg::Copy).shortcut("Ctrl+C"), has),
                    enable(MenuItem::new("Paste", Msg::Paste).shortcut("Ctrl+V"), self.clipboard.is_some()),
                    MenuItem::separator(),
                    MenuItem::new("Copy path", Msg::CopyPath),
                ]
            ),
            MenuItem::submenu(
                "View",
                alloc::vec![
                    MenuItem::new("Icons", Msg::ToggleGrid).checked(self.grid),
                    MenuItem::new("Show hidden files", Msg::ToggleHidden).shortcut("Ctrl+H").checked(self.show_hidden),
                    MenuItem::separator(),
                    MenuItem::new("Sort by name", Msg::Sort(Sort::Name)).checked(self.sort == Sort::Name),
                    MenuItem::new("Sort by size", Msg::Sort(Sort::Size)).checked(self.sort == Sort::Size),
                    MenuItem::new("Sort by type", Msg::Sort(Sort::Kind)).checked(self.sort == Sort::Kind),
                    MenuItem::separator(),
                    MenuItem::new("Refresh", Msg::Refresh).shortcut("Ctrl+R"),
                ]
            ),
            MenuItem::submenu("Go", go),
            MenuItem::submenu("Help", alloc::vec![MenuItem::new("Keyboard shortcuts", Msg::Shortcuts), MenuItem::separator(), MenuItem::new("About Files", Msg::About)]),
        ]
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        if self.pick.is_some() { None } else { Some((2000, Msg::Places)) }
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        match (press.key, press.mods.ctrl, press.mods.alt) {
            (Key::Delete, false, false) if self.pick.is_none() => Some(Msg::Delete),
            (Key::F(2), false, false) if self.pick.is_none() => Some(Msg::StartRename),
            (Key::Backspace, false, false) | (Key::Up, false, true) => Some(Msg::Up),
            (Key::Left, false, true) => Some(Msg::Back),
            (Key::Right, false, true) => Some(Msg::Forward),
            (Key::Escape, _, _) if self.editing.is_some() => Some(Msg::CancelName),
            (Key::Escape, _, _) if self.pick.is_some() => Some(Msg::Cancel),
            _ => None,
        }
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Select(i) => {
                self.selected = self.visible().get(i).copied();
                if let (Some(p), Some(e)) = (self.pick.as_mut(), self.selected.and_then(|s| self.entries.get(s))) {
                    if p.mode == PickMode::Save && !e.is_dir {
                        p.name = e.name.clone();
                    }
                }
                self.message.0.clear();
            }
            Msg::Activate(i) => {
                if let Some(index) = self.visible().get(i).copied() {
                    self.selected = Some(index);
                    self.open(index, cx);
                }
            }
            Msg::Context(target) => {
                if self.pick.is_some() {
                    return;
                }
                match target.and_then(|i| self.visible().get(i).copied()) {
                    Some(index) => {
                        self.selected = Some(index);
                        cx.show_menu(self.entry_menu());
                    }
                    None => {
                        self.selected = None;
                        cx.show_menu(self.folder_menu());
                    }
                }
            }
            Msg::Places => self.refresh_places(),
            Msg::Unmount(i) => {
                if let Some(p) = self.places.get(i).cloned() {
                    if self.path == p.path || self.path.starts_with(&format!("{}/", p.path)) {
                        let home = hxport::home();
                        self.navigate(&home);
                    }
                    hxport::chdir(&hxport::home());
                    if unmount_media(&p.path) {
                        self.message = (format!("Unmounting {}…", p.name), Tone::Dim);
                        self.unmounting = Some((p.path.clone(), p.name.clone()));
                        cx.after(800, Msg::Places);
                    } else {
                        self.message = (format!("Could not unmount {}", p.name), Tone::Danger);
                    }
                }
            }
            Msg::Place(i) => {
                if let Some(Place { path, .. }) = self.places.get(i).cloned() {
                    if !hxport::exists(&path) {
                        hxport::mkdir(&path);
                    }
                    self.navigate(&path);
                }
            }
            Msg::Back => {
                if let Some(prev) = self.history.pop() {
                    let cur = self.path.clone();
                    if self.load(&prev) {
                        self.future.push(cur);
                    }
                }
            }
            Msg::Forward => {
                if let Some(next) = self.future.pop() {
                    let cur = self.path.clone();
                    if self.load(&next) {
                        self.history.push(cur);
                    }
                }
            }
            Msg::Up => {
                if self.path != "/" {
                    let up = parent(&self.path);
                    let from = String::from(base_name(&self.path));
                    self.navigate(&up);
                    self.selected = self.entries.iter().position(|e| e.name == from);
                }
            }
            Msg::PathTyped(v) => self.typed = v,
            Msg::PathGo => {
                let target = String::from(self.typed.trim());
                let target = if target.starts_with('~') { target.replacen('~', &hxport::home(), 1) } else { target };
                if hxport::is_dir(&target) {
                    self.navigate(&target);
                } else {
                    self.message = (format!("{} is not a folder", target), Tone::Danger);
                }
            }
            Msg::Search(v) => {
                self.search = v;
                self.selected = None;
            }
            Msg::Open => {
                if let Some(i) = self.selected {
                    self.open(i, cx);
                }
            }
            Msg::OpenTerminal => {
                if !open_terminal(&self.path) {
                    self.message = (String::from("Cannot start the terminal"), Tone::Danger);
                }
            }
            Msg::StartRename => {
                if let Some(name) = self.selected_entry().map(|e| e.name.clone()) {
                    self.start_edit(Editing::Rename(name));
                }
            }
            Msg::StartNew(folder) => self.start_edit(if folder { Editing::NewFolder } else { Editing::NewFile }),
            Msg::EditName(v) => self.name = v,
            Msg::CommitName => self.commit_name(),
            Msg::CancelName => self.editing = None,
            Msg::Delete => {
                if let Some(e) = self.selected_entry() {
                    let what = if e.is_dir { "folder and everything in it" } else { "file" };
                    cx.confirm(format!("Delete “{}”?", e.name), format!("The {} will be deleted permanently.", what), Msg::DeleteConfirmed);
                }
            }
            Msg::DeleteConfirmed(yes) => {
                if let (true, Some(e)) = (yes, self.selected_entry()) {
                    let name = e.name.clone();
                    if remove_tree(&join(&self.path, &name)) {
                        self.message = (format!("Deleted {}", name), Tone::Success);
                    } else {
                        self.message = (format!("Cannot delete {}", name), Tone::Danger);
                    }
                    self.reload(None);
                }
            }
            Msg::Cut | Msg::Copy => {
                if let Some(name) = self.selected_entry().map(|e| e.name.clone()) {
                    let cut = matches!(message, Msg::Cut);
                    self.message = (format!("{} “{}” — open a folder and paste", if cut { "Cut" } else { "Copied" }, name), Tone::Dim);
                    self.clipboard = Some((join(&self.path, &name), cut));
                }
            }
            Msg::Paste => self.paste(),
            Msg::CopyPath => {
                let path = self.selected_entry().map(|e| join(&self.path, &e.name)).unwrap_or_else(|| self.path.clone());
                cx.copy(path.clone());
                self.message = (format!("Copied {}", path), Tone::Dim);
            }
            Msg::Properties => {
                let target = self.selected_entry().map(|e| join(&self.path, &e.name)).unwrap_or_else(|| self.path.clone());
                let arg = format!("--properties={}", target);
                if hxport::spawn(&self_program(), &[&arg]).is_err() {
                    self.message = (String::from("Could not open the properties window"), Tone::Danger);
                }
            }
            Msg::About => {
                if hxport::spawn(&self_program(), &["--about"]).is_err() {
                    self.message = (String::from("Could not open the About window"), Tone::Danger);
                }
            }
            Msg::Shortcuts => cx.message(
                "Keyboard shortcuts",
                "Ctrl+Shift+N  New folder\nF2  Rename\nDelete  Delete\nAlt+Enter  Properties\nCtrl+X / Ctrl+C / Ctrl+V  Cut, copy, paste\nCtrl+H  Show hidden files\nCtrl+R  Refresh\nAlt+Left / Alt+Right  Back, forward\nAlt+Up or Backspace  Enclosing folder\nCtrl+Q  Close",
            ),
            Msg::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.reload(None);
            }
            Msg::ToggleGrid => self.grid = !self.grid,
            Msg::Sort(sort) => {
                self.sort = sort;
                let selected = self.selected_entry().map(|e| e.name.clone());
                self.sort_entries();
                self.selected = selected.and_then(|n| self.entries.iter().position(|e| e.name == n));
            }
            Msg::Refresh => {
                let selected = self.selected_entry().map(|e| e.name.clone());
                self.reload(selected.as_deref());
            }
            Msg::Quit => cx.quit(),
            Msg::PickName(v) => {
                if let Some(p) = self.pick.as_mut() {
                    p.name = v;
                }
            }
            Msg::PickFilter(i) => {
                if let Some(p) = self.pick.as_mut() {
                    p.active = i;
                }
                self.reload(None);
            }
            Msg::Accept => self.accept(cx),
            Msg::Cancel => cx.quit(),
        }
    }
}

#[cfg(target_os = "hamix")]
fn self_program() -> String {
    hxport::current_exe().unwrap_or_else(|| String::from("/usr/bin/hxfiles"))
}

#[cfg(not(target_os = "hamix"))]
fn self_program() -> String {
    hxport::current_exe().unwrap_or_else(|| String::from("hxfiles"))
}

fn run() -> i32 {
    let args = hxport::args();
    if let Some(path) = args.iter().find_map(|a| a.strip_prefix("--properties=")) {
        return hxtoolkit::run(props::Properties::settings(), props::Properties::new(String::from(path)));
    }
    if args.iter().any(|a| a == "--about") {
        return hxtoolkit::run(about::About::settings(), about::About::new());
    }
    let files = Files::new();
    let size = if files.pick.is_some() { (820, 540) } else { (900, 580) };
    hxtoolkit::run(Settings::new("files").icon("files").size(size.0, size.1).min_size(560, 360), files)
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
