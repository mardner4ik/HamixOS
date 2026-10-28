use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxport::{FileKind, Metadata};
use hxtoolkit::{button, checkbox, column, divider, icon, progress, row, scroll, text, title, Align, App, Context, DisplayServer, Element, Key, KeyPress, Length, Settings, Tone};

use crate::{base_name, join, open_default, parent, VIDEO};

const SCAN_STEP: usize = 400;
const SCAN_LIMIT: u64 = 200_000;

#[derive(Clone)]
pub enum Msg {
    Scan,
    Toggle(u32, bool),
    Open,
    OpenLocation,
    CopyPath,
    Close,
}

struct Scan {
    pending: Vec<String>,
    files: u64,
    folders: u64,
    bytes: u64,
    disk: u64,
    unreadable: u64,
    done: bool,
}

pub struct Properties {
    path: String,
    link: Option<Metadata>,
    meta: Option<Metadata>,
    target: Option<String>,
    details: Vec<(String, String)>,
    scan: Option<Scan>,
    users: Vec<(u32, String)>,
    groups: Vec<(u32, String)>,
    status: (String, Tone),
}

fn kind_name(path: &str, meta: &Metadata) -> (&'static str, &'static str) {
    let lower = path.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, x)| x).unwrap_or("");
    match meta.kind {
        FileKind::Dir => ("folder", "Folder"),
        FileKind::File if matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "bmp") => ("image", "Picture"),
        FileKind::File if VIDEO.contains(&ext) => ("video", "Video"),
        FileKind::File if ext == "sh" => ("script", "Shell script"),
        FileKind::File if meta.mode & 0o111 != 0 => ("exec", "Program"),
        FileKind::File => ("text", "Document"),
        FileKind::Symlink => ("text", "Link"),
        FileKind::CharDevice | FileKind::BlockDevice => ("exec", "Device"),
        FileKind::Fifo | FileKind::Socket => ("text", "Special file"),
        FileKind::Unknown => ("text", "Unknown"),
    }
}

fn mime_type(path: &str, head: &[u8], meta: &Metadata) -> &'static str {
    if meta.kind == FileKind::Dir {
        return "inode/directory";
    }
    if meta.kind != FileKind::File {
        return "inode/special";
    }
    if head.starts_with(b"\x89PNG") {
        return "image/png";
    }
    if head.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if head.starts_with(b"GIF8") {
        return "image/gif";
    }
    if head.starts_with(b"BM") {
        return "image/bmp";
    }
    if head.starts_with(b"\x7fELF") {
        return match head.get(16) {
            Some(3) => "application/x-sharedlib",
            Some(1) => "application/x-object",
            _ => "application/x-executable",
        };
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
        return "audio/x-wav";
    }
    if head.get(4..8) == Some(b"ftyp") {
        return "video/mp4";
    }
    if head.starts_with(b"%PDF") {
        return "application/pdf";
    }
    if head.starts_with(b"PK\x03\x04") {
        return "application/zip";
    }
    if head.starts_with(&[0x1f, 0x8b]) {
        return "application/gzip";
    }
    if head.starts_with(b"#!") {
        return "text/x-script";
    }
    if head.len() > 262 && head.get(257..262) == Some(b"ustar") {
        return "application/x-tar";
    }
    if head.is_empty() {
        return "application/x-zerosize";
    }
    if core::str::from_utf8(head).is_ok() || head.iter().all(|b| *b >= 0x20 || matches!(b, b'\n' | b'\r' | b'\t')) {
        let lower = path.to_lowercase();
        return match lower.rsplit_once('.').map(|(_, x)| x).unwrap_or("") {
            "md" => "text/markdown",
            "rs" => "text/rust",
            "c" | "h" => "text/x-c",
            "toml" => "application/toml",
            "json" => "application/json",
            "html" | "htm" => "text/html",
            "desktop" => "application/x-desktop",
            "conf" | "cfg" | "ini" => "text/x-config",
            _ => "text/plain",
        };
    }
    "application/octet-stream"
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn image_size(head: &[u8]) -> Option<(u32, u32)> {
    if head.starts_with(b"\x89PNG") {
        return Some((be32(head, 16)?, be32(head, 20)?));
    }
    if head.starts_with(b"GIF8") {
        return Some((le16(head, 6)? as u32, le16(head, 8)? as u32));
    }
    if head.starts_with(b"BM") {
        return Some((le32(head, 18)?, (le32(head, 22)? as i32).unsigned_abs()));
    }
    if head.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < head.len() {
            if head[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = head[i + 1];
            let len = u16::from_be_bytes([head[i + 2], head[i + 3]]) as usize;
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                let h = u16::from_be_bytes([head[i + 5], head[i + 6]]) as u32;
                let w = u16::from_be_bytes([head[i + 7], head[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

fn elf_details(head: &[u8], out: &mut Vec<(String, String)>) {
    let class = match head.get(4) {
        Some(2) => "64-bit",
        Some(1) => "32-bit",
        _ => "unknown class",
    };
    let endian = if head.get(5) == Some(&2) { "big-endian" } else { "little-endian" };
    let kind = match le16(head, 16) {
        Some(1) => "relocatable object",
        Some(2) => "executable",
        Some(3) => "shared object (library or PIE program)",
        Some(4) => "core dump",
        _ => "unknown",
    };
    let machine = match le16(head, 18) {
        Some(0x3e) => "x86-64",
        Some(0x03) => "i386",
        Some(0xb7) => "AArch64",
        Some(0xf3) => "RISC-V",
        Some(0x28) => "ARM",
        Some(other) => return out.push((String::from("Machine"), format!("0x{:x}", other))),
        None => "unknown",
    };
    out.push((String::from("Format"), format!("ELF {} {}, {}", class, endian, kind)));
    out.push((String::from("Machine"), String::from(machine)));
    if head.get(4) != Some(&2) {
        return;
    }
    let (Some(phoff), Some(phentsize), Some(phnum)) = (le64(head, 32), le16(head, 54), le16(head, 56)) else {
        return;
    };
    let mut interp = None;
    let mut dynamic = false;
    let mut hamix_note = false;
    for i in 0..phnum as usize {
        let at = phoff as usize + i * phentsize as usize;
        let (Some(kind), Some(offset), Some(size)) = (le32(head, at), le64(head, at + 8), le64(head, at + 32)) else {
            break;
        };
        match kind {
            2 => dynamic = true,
            3 => {
                if let Some(bytes) = head.get(offset as usize..(offset + size) as usize) {
                    interp = Some(String::from_utf8_lossy(bytes).trim_end_matches('\0').into());
                }
            }
            4 => {
                if let Some(bytes) = head.get(offset as usize..(offset + size) as usize) {
                    hamix_note |= bytes.windows(5).any(|w| w == b"Hamix");
                }
            }
            _ => {}
        }
    }
    out.push((String::from("Linking"), String::from(if dynamic { "dynamic" } else { "static" })));
    if let Some(interp) = interp {
        out.push((String::from("Interpreter"), interp));
    }
    out.push((String::from("ABI"), String::from(if hamix_note { "HamixOS (hxlibc)" } else { "Linux / System V" })));
}

fn text_details(head: &[u8], complete: bool, out: &mut Vec<(String, String)>) {
    let Ok(body) = core::str::from_utf8(head) else {
        return;
    };
    let suffix = if complete { "" } else { "+" };
    out.push((String::from("Encoding"), String::from(if body.is_ascii() { "ASCII" } else { "UTF-8" })));
    out.push((String::from("Lines"), format!("{}{}", body.lines().count(), suffix)));
    out.push((String::from("Words"), format!("{}{}", body.split_whitespace().count(), suffix)));
    out.push((String::from("Characters"), format!("{}{}", body.chars().count(), suffix)));
    let endings = if body.contains("\r\n") { "Windows (CRLF)" } else if body.contains('\n') { "Unix (LF)" } else { "none" };
    out.push((String::from("Line endings"), String::from(endings)));
    if let Some(first) = body.lines().next().and_then(|l| l.strip_prefix("#!")) {
        out.push((String::from("Runs with"), String::from(first.trim())));
    }
}

fn name_of(list: &[(u32, String)], id: u32) -> String {
    list.iter().find(|(i, _)| *i == id).map(|(_, n)| format!("{} ({})", n, id)).unwrap_or_else(|| format!("{}", id))
}

fn major_minor(dev: u64) -> String {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    format!("{}:{}", major, minor)
}

fn field(label: &str, value: impl Into<String>) -> Element<Msg> {
    row![text(String::from(label)).small().tone(Tone::Dim).width(130u32), text(value.into()).selectable().wrap().width(Length::Fill)].spacing(12).align(Align::Start).width(Length::Fill).into()
}

fn section(name: &str, rows: Vec<Element<Msg>>) -> Element<Msg> {
    let mut body = column![text(String::from(name)).strong()].spacing(8).padding(14).width(Length::Fill).card();
    for r in rows {
        body = body.push(r);
    }
    body.into()
}

impl Properties {
    pub fn new(path: String) -> Properties {
        let link = hxport::symlink_metadata(&path);
        let meta = hxport::metadata(&path).or_else(|| link.clone());
        let target = link.as_ref().filter(|m| m.kind == FileKind::Symlink).and_then(|_| hxport::read_link(&path));
        let mut details = Vec::new();
        if let Some(m) = meta.as_ref().filter(|m| m.kind == FileKind::File) {
            let limit = 1 << 20;
            let head = hxport::read_head(&path, limit).unwrap_or_default();
            details.push((String::from("Content type"), String::from(mime_type(&path, &head, m))));
            if let Some((w, h)) = image_size(&head) {
                details.push((String::from("Dimensions"), format!("{} × {} pixels", w, h)));
                details.push((String::from("Megapixels"), format!("{}.{}", (w as u64 * h as u64) / 1_000_000, (w as u64 * h as u64) / 100_000 % 10)));
            } else if head.starts_with(b"\x7fELF") {
                elf_details(&head, &mut details);
            } else {
                text_details(&head, (m.size as usize) <= limit, &mut details);
            }
        }
        let scan = meta.as_ref().filter(|m| m.kind == FileKind::Dir).map(|_| Scan { pending: alloc::vec![path.clone()], files: 0, folders: 0, bytes: 0, disk: 0, unreadable: 0, done: false });
        Properties { path, link, meta, target, details, scan, users: hxport::user_names(), groups: hxport::group_names(), status: (String::new(), Tone::Dim) }
    }

    pub fn settings() -> Settings {
        Settings::new("files").icon("files").size(500, 640).min_size(420, 420).display(DisplayServer::Auto)
    }

    fn scan_step(&mut self) {
        let Some(scan) = self.scan.as_mut() else {
            return;
        };
        let mut budget = SCAN_STEP;
        while budget > 0 {
            let Some(dir) = scan.pending.pop() else {
                scan.done = true;
                return;
            };
            let Some(list) = hxport::entries(&dir) else {
                scan.unreadable += 1;
                continue;
            };
            for e in list {
                budget = budget.saturating_sub(1);
                let full = join(&dir, &e.name);
                let Some(m) = hxport::symlink_metadata(&full) else {
                    scan.unreadable += 1;
                    continue;
                };
                scan.disk += m.blocks * 512;
                if m.kind == FileKind::Dir {
                    scan.folders += 1;
                    scan.pending.push(full);
                } else {
                    scan.files += 1;
                    scan.bytes += m.size;
                }
            }
            if scan.files + scan.folders >= SCAN_LIMIT {
                scan.done = true;
                scan.pending.clear();
                return;
            }
        }
    }

    fn general(&self, m: &Metadata) -> Element<Msg> {
        let (_, kind) = kind_name(&self.path, m);
        let mut rows = alloc::vec![
            field("Name", base_name(&self.path)),
            field("Type", format!("{} ({})", kind, m.kind.label())),
            field("Location", parent(&self.path)),
            field("Full path", self.path.clone()),
        ];
        if let Some(t) = &self.target {
            rows.push(field("Link target", t.clone()));
            let broken = hxport::metadata(&self.path).is_none();
            if broken {
                rows.push(field("Link status", "broken: the target does not exist"));
            }
        }
        match &self.scan {
            Some(scan) => {
                let more = if scan.done { "" } else { "…" };
                rows.push(field("Contents", format!("{} files, {} folders{}", scan.files, scan.folders, more)));
                rows.push(field("Total size", format!("{} ({} bytes){}", hxport::human_size(scan.bytes), scan.bytes, more)));
                rows.push(field("On disk", format!("{}{}", hxport::human_size(scan.disk + m.blocks * 512), more)));
                if scan.unreadable > 0 {
                    rows.push(field("Unreadable", format!("{} items (no permission)", scan.unreadable)));
                }
                if scan.done && scan.files + scan.folders >= SCAN_LIMIT {
                    rows.push(field("Note", format!("counting stopped after {} items", SCAN_LIMIT)));
                }
            }
            None => {
                rows.push(field("Size", format!("{} ({} bytes)", hxport::human_size(m.size), m.size)));
                rows.push(field("On disk", format!("{} ({} blocks of 512 bytes)", hxport::human_size(m.blocks * 512), m.blocks)));
            }
        }
        if matches!(m.kind, FileKind::CharDevice | FileKind::BlockDevice) {
            rows.push(field("Device number", major_minor(m.rdev)));
        }
        section("General", rows)
    }

    fn permissions(&self, m: &Metadata) -> Element<Msg> {
        let mode = m.mode;
        let owner_can = hxport::uid() == 0 || hxport::uid() == m.uid;
        let grid_row = |who: &str, shift: u32| -> Element<Msg> {
            let bit = |b: u32, label: &str| checkbox(String::from(label), mode & (b << shift) != 0, move |on| Msg::Toggle(b << shift, on));
            row![text(String::from(who)).small().tone(Tone::Dim).width(130u32), bit(4, "Read"), bit(2, "Write"), bit(1, if m.kind == FileKind::Dir { "Enter" } else { "Execute" })].spacing(12).align(Align::Center).width(Length::Fill).into()
        };
        let can = |bit: i32, label: &'static str| if hxport::can_access(&self.path, bit) { Some(label) } else { None };
        let yours: Vec<&str> = [can(4, "read"), can(2, "write"), can(1, if m.kind == FileKind::Dir { "enter" } else { "run" })].into_iter().flatten().collect();
        let mut rows = alloc::vec![
            field("Owner", name_of(&self.users, m.uid)),
            field("Group", name_of(&self.groups, m.gid)),
            field("Mode", format!("{}   {:04o}", hxport::permission_string(mode), mode & 0o7777)),
            field("You can", if yours.is_empty() { String::from("nothing") } else { yours.join(", ") }),
            divider(),
            grid_row("Owner", 6),
            grid_row("Group", 3),
            grid_row("Others", 0),
            row![
                text("Special").small().tone(Tone::Dim).width(130u32),
                checkbox("Set user ID", mode & 0o4000 != 0, |on| Msg::Toggle(0o4000, on)),
                checkbox("Set group ID", mode & 0o2000 != 0, |on| Msg::Toggle(0o2000, on)),
                checkbox("Sticky", mode & 0o1000 != 0, |on| Msg::Toggle(0o1000, on)),
            ]
            .spacing(12)
            .align(Align::Center)
            .width(Length::Fill)
            .into(),
        ];
        if !owner_can {
            rows.push(text("Only the owner or root can change the permissions.").small().tone(Tone::Dim).wrap().into());
        }
        section("Permissions", rows)
    }

    fn times(&self, m: &Metadata) -> Element<Msg> {
        section(
            "Dates",
            alloc::vec![field("Modified", hxport::format_time(m.modified)), field("Accessed", hxport::format_time(m.accessed)), field("Status changed", hxport::format_time(m.changed))],
        )
    }

    fn storage(&self, m: &Metadata) -> Element<Msg> {
        let mut rows = Vec::new();
        if let Some(mount) = hxport::mount_of(&self.path) {
            rows.push(field("File system", format!("{} on {}", mount.fs_type, mount.point)));
            rows.push(field("Device", mount.device));
            let ro = mount.options.split(',').any(|o| o == "ro");
            rows.push(field("Mounted", String::from(if ro { "read-only" } else { "read-write" })));
        }
        if let Some(fs) = hxport::fs_stats(&self.path).filter(|f| f.blocks > 0) {
            let used = fs.used_bytes();
            let total = fs.total_bytes();
            rows.push(field("Capacity", hxport::human_size(total)));
            rows.push(field("Free", format!("{} available", hxport::human_size(fs.available_bytes()))));
            rows.push(row![text("Used").small().tone(Tone::Dim).width(130u32), progress(used as f32 / total.max(1) as f32).width(Length::Fill), text(format!("{}%", used * 100 / total.max(1))).small()].spacing(12).align(Align::Center).width(Length::Fill).into());
            if fs.files > 0 {
                rows.push(field("Inodes free", format!("{} of {}", fs.files_free, fs.files)));
            }
        }
        rows.push(field("Inode", format!("{}", m.inode)));
        rows.push(field("Hard links", format!("{}", m.links)));
        rows.push(field("Device id", major_minor(m.device)));
        rows.push(field("Block size", format!("{} bytes", m.block_size)));
        section("Storage", rows)
    }

    fn apply(&mut self, bit: u32, on: bool) {
        let Some(m) = &self.meta else {
            return;
        };
        let mode = if on { m.mode | bit } else { m.mode & !bit };
        if hxport::chmod(&self.path, mode & 0o7777) {
            self.meta = hxport::metadata(&self.path);
            self.status = (format!("Permissions changed to {:04o}", mode & 0o7777), Tone::Success);
        } else {
            self.status = (String::from("Could not change the permissions: operation not permitted"), Tone::Danger);
        }
    }
}

impl App for Properties {
    type Message = Msg;

    fn title(&self) -> String {
        format!("{} — Properties", base_name(&self.path))
    }

    fn view(&self) -> Element<Msg> {
        let Some(m) = &self.meta else {
            return column![title("Not found"), text(format!("{} does not exist or cannot be read.", self.path)).wrap().tone(Tone::Dim), button("Close").on_press(Msg::Close)].spacing(12).padding(20).width(Length::Fill).into();
        };
        let (glyph, kind) = kind_name(&self.path, m);
        let summary = match (&self.scan, self.link.as_ref().map(|l| l.kind)) {
            (_, Some(FileKind::Symlink)) => format!("Link to {}", self.target.clone().unwrap_or_default()),
            (Some(scan), _) => format!("{} · {} items", kind, scan.files + scan.folders),
            (None, _) => format!("{} · {}", kind, hxport::human_size(m.size)),
        };
        let header = row![
            icon(format!("files/{}", glyph)).size(56u32, 56u32),
            column![title(String::from(base_name(&self.path))), text(summary).tone(Tone::Dim).small()].spacing(4).width(Length::Fill),
        ]
        .spacing(16)
        .align(Align::Center)
        .padding(16)
        .width(Length::Fill);
        let mut body = column![self.general(m)].spacing(12).padding([0, 16]).width(Length::Fill);
        if !self.details.is_empty() {
            body = body.push(section("Details", self.details.iter().map(|(k, v)| field(k, v.clone())).collect()));
        }
        body = body.push(self.permissions(m)).push(self.times(m)).push(self.storage(m)).push(hxtoolkit::vspace(4));
        let footer = row![
            text(self.status.0.clone()).small().tone(self.status.1).width(Length::Fill),
            button("Open location").flat().on_press(Msg::OpenLocation),
            button("Copy path").flat().on_press(Msg::CopyPath),
            button("Open").on_press(Msg::Open),
            button("Close").primary().on_press(Msg::Close),
        ]
        .spacing(8)
        .align(Align::Center)
        .padding(12)
        .width(Length::Fill);
        column![header, divider(), scroll(body).height(Length::Fill).width(Length::Fill), divider(), footer].spacing(0).width(Length::Fill).height(Length::Fill).into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Scan => self.scan_step(),
            Msg::Toggle(bit, on) => self.apply(bit, on),
            Msg::Open => {
                let dir = self.meta.as_ref().map(|m| m.kind == FileKind::Dir).unwrap_or(false);
                if !open_default(&self.path, dir) {
                    self.status = (String::from("Could not open the file"), Tone::Danger);
                }
            }
            Msg::OpenLocation => {
                if !open_default(&parent(&self.path), true) {
                    self.status = (String::from("Could not open the folder"), Tone::Danger);
                }
            }
            Msg::CopyPath => {
                cx.copy(self.path.clone());
                self.status = (String::from("Path copied"), Tone::Dim);
            }
            Msg::Close => cx.quit(),
        }
    }

    fn tick(&self) -> Option<(u32, Msg)> {
        self.scan.as_ref().filter(|s| !s.done).map(|_| (30, Msg::Scan))
    }

    fn key(&self, press: &KeyPress) -> Option<Msg> {
        match press.key {
            Key::Escape => Some(Msg::Close),
            _ => None,
        }
    }
}
