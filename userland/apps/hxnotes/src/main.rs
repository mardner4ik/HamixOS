#![cfg_attr(target_os = "hamix", no_std)]
#![cfg_attr(target_os = "hamix", no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxtoolkit::{button, column, editor, input, row, space, text, App, Context, EditAction, Element, MenuItem, Settings, Tone};

const DOC: &str = "doc";
const TEXT_FILTERS: [&str; 15] = ["txt", "md", "conf", "log", "rs", "sh", "json", "csv", "ini", "toml", "c", "h", "py", "html", "css"];

struct Notes {
    text: String,
    path: Option<String>,
    modified: bool,
    status: (String, Tone),
    cursor: (usize, usize),
    find: Option<String>,
    line_numbers: bool,
    highlight_line: bool,
    status_bar: bool,
}

#[derive(Clone)]
enum Msg {
    Edited(String),
    Cursor((usize, usize)),
    New,
    Open,
    Opened(Option<String>),
    Save,
    SaveAs,
    SavedAs(Option<String>),
    Quit,
    Edit(EditAction),
    Find,
    FindText(String),
    FindNext,
    Found(bool),
    CloseFind,
    ToggleLineNumbers,
    ToggleHighlight,
    ToggleStatusBar,
    Shortcuts,
    About,
    DiscardAndQuit(bool),
}

impl Notes {
    fn new() -> Notes {
        let mut notes = Notes {
            text: String::new(),
            path: None,
            modified: false,
            status: (String::from("New note"), Tone::Dim),
            cursor: (1, 1),
            find: None,
            line_numbers: true,
            highlight_line: true,
            status_bar: true,
        };
        if let Some(arg) = hxport::args().get(1) {
            let path = if arg.starts_with('/') { arg.clone() } else { format!("{}/{}", hxport::var("PWD").unwrap_or_default(), arg) };
            notes.open_path(&path);
        }
        notes
    }

    fn file_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.rsplit('/').next()).filter(|n| !n.is_empty()).map(String::from).unwrap_or_else(|| String::from("Untitled"))
    }

    fn open_path(&mut self, path: &str) {
        match hxport::read(path) {
            Some(bytes) if bytes.iter().take(4096).any(|b| *b == 0) => self.status = (format!("{} looks like a binary file", path), Tone::Danger),
            Some(bytes) => {
                self.text = String::from_utf8_lossy(&bytes).into_owned();
                self.path = Some(String::from(path));
                self.modified = false;
                self.status = (format!("Opened {}", path), Tone::Dim);
            }
            None if !hxport::exists(path) => {
                self.text.clear();
                self.path = Some(String::from(path));
                self.modified = false;
                self.status = (String::from("New file — it will be created when you save"), Tone::Dim);
            }
            None => self.status = (format!("Cannot open {}", path), Tone::Danger),
        }
    }

    fn start_dir(&self) -> String {
        match self.path.as_deref().and_then(|p| p.rfind('/').map(|i| (p, i))) {
            Some((p, i)) if i > 0 => String::from(&p[..i]),
            _ => format!("{}/Documents", hxport::home()),
        }
    }

    fn save(&mut self, cx: &mut Context<Msg>) {
        let Some(path) = self.path.clone() else {
            self.save_as(cx);
            return;
        };
        if let Some(i) = path.rfind('/') {
            if i > 0 && !hxport::exists(&path[..i]) {
                hxport::mkdir(&path[..i]);
            }
        }
        let mut body = self.text.clone();
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        if hxport::write(&path, body.as_bytes()) {
            self.modified = false;
            self.status = (format!("Saved {} ({})", path, hxport::human_size(body.len() as u64)), Tone::Success);
        } else {
            self.status = (format!("Cannot save {} — permission denied?", path), Tone::Danger);
        }
    }

    fn save_as(&mut self, cx: &mut Context<Msg>) {
        let suggested = self.path.clone().unwrap_or_else(|| format!("{}/note.txt", self.start_dir()));
        cx.save_file("Save the note as", suggested, Msg::SavedAs);
    }
}

impl App for Notes {
    type Message = Msg;

    fn title(&self) -> String {
        format!("{}{} — Notes", if self.modified { "• " } else { "" }, self.file_name())
    }

    fn view(&self) -> Element<Msg> {
        let mut page = column![].spacing(8).padding(12).fill();
        if let Some(query) = &self.find {
            page = page.push(
                row![
                    input(query.clone(), "Find in the note", Msg::FindText).on_submit(Msg::FindNext).focused(),
                    button("Next").on_press(Msg::FindNext),
                    button("Close").flat().on_press(Msg::CloseFind),
                ]
                .spacing(8),
            );
        }
        page = page.push(editor(self.text.clone(), Msg::Edited).id(DOC).monospace().line_numbers(self.line_numbers).highlight_line(self.highlight_line).on_cursor(Msg::Cursor));
        if self.status_bar {
            let lines = self.text.lines().count().max(1);
            page = page.push(row![
                text(self.status.0.clone()).small().tone(self.status.1),
                space(),
                text(format!("Ln {}, Col {} · {} lines · {}", self.cursor.0, self.cursor.1, lines, hxport::human_size(self.text.len() as u64))).small().tone(Tone::Faint),
            ]);
        }
        page.into()
    }

    fn update(&mut self, message: Msg, cx: &mut Context<Msg>) {
        match message {
            Msg::Edited(text) => {
                self.text = text;
                self.modified = true;
            }
            Msg::Cursor(pos) => self.cursor = pos,
            Msg::New => {
                self.text.clear();
                self.path = None;
                self.modified = false;
                self.status = (String::from("New note"), Tone::Dim);
            }
            Msg::Open => cx.open_file("Open a text file", &TEXT_FILTERS, Msg::Opened),
            Msg::Opened(Some(path)) => self.open_path(&path),
            Msg::Save => self.save(cx),
            Msg::SaveAs => self.save_as(cx),
            Msg::SavedAs(Some(path)) => {
                self.path = Some(path);
                self.save(cx);
            }
            Msg::Opened(None) | Msg::SavedAs(None) => {}
            Msg::Quit => {
                if self.modified {
                    cx.confirm("Discard the changes?", format!("“{}” has unsaved changes.", self.file_name()), Msg::DiscardAndQuit);
                } else {
                    cx.quit();
                }
            }
            Msg::DiscardAndQuit(true) => cx.quit(),
            Msg::DiscardAndQuit(false) => {}
            Msg::Edit(action) => cx.edit(DOC, action),
            Msg::Find => {
                if self.find.is_none() {
                    self.find = Some(String::new());
                }
            }
            Msg::FindText(query) => self.find = Some(query),
            Msg::FindNext => match self.find.clone().filter(|q| !q.is_empty()) {
                Some(query) => cx.find(DOC, query, Msg::Found),
                None => self.find = Some(String::new()),
            },
            Msg::Found(found) => {
                let query = self.find.clone().unwrap_or_default();
                self.status = if found { (format!("Found “{}”", query), Tone::Dim) } else { (format!("“{}” was not found", query), Tone::Warning) };
            }
            Msg::CloseFind => {
                self.find = None;
                cx.edit(DOC, EditAction::Focus);
            }
            Msg::ToggleLineNumbers => self.line_numbers = !self.line_numbers,
            Msg::ToggleHighlight => self.highlight_line = !self.highlight_line,
            Msg::ToggleStatusBar => self.status_bar = !self.status_bar,
            Msg::Shortcuts => cx.message("Keyboard shortcuts", "Ctrl+N new · Ctrl+O open · Ctrl+S save · Ctrl+Z / Ctrl+Y undo and redo · Ctrl+F find · Ctrl+G find next · Ctrl+Home / Ctrl+End go to the start or the end"),
            Msg::About => cx.message("About Notes", "Notes — a plain text editor for HamixOS, built with hxtoolkit."),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Msg>> {
        alloc::vec![
            MenuItem::submenu(
                "File",
                alloc::vec![
                    MenuItem::new("New", Msg::New).shortcut("Ctrl+N"),
                    MenuItem::new("Open…", Msg::Open).shortcut("Ctrl+O"),
                    MenuItem::separator(),
                    MenuItem::new("Save", Msg::Save).shortcut("Ctrl+S"),
                    MenuItem::new("Save as…", Msg::SaveAs).shortcut("Ctrl+Shift+S"),
                    MenuItem::separator(),
                    MenuItem::new("Quit", Msg::Quit).shortcut("Ctrl+Q"),
                ],
            ),
            MenuItem::submenu(
                "Edit",
                alloc::vec![
                    MenuItem::new("Undo", Msg::Edit(EditAction::Undo)).shortcut("Ctrl+Z"),
                    MenuItem::new("Redo", Msg::Edit(EditAction::Redo)).shortcut("Ctrl+Y"),
                    MenuItem::separator(),
                    MenuItem::new("Cut", Msg::Edit(EditAction::Cut)).shortcut("Ctrl+X"),
                    MenuItem::new("Copy", Msg::Edit(EditAction::Copy)).shortcut("Ctrl+C"),
                    MenuItem::new("Paste", Msg::Edit(EditAction::Paste)).shortcut("Ctrl+V"),
                    MenuItem::new("Delete", Msg::Edit(EditAction::Delete)),
                    MenuItem::separator(),
                    MenuItem::new("Select all", Msg::Edit(EditAction::SelectAll)).shortcut("Ctrl+A"),
                ],
            ),
            MenuItem::submenu(
                "Search",
                alloc::vec![
                    MenuItem::new("Find…", Msg::Find).shortcut("Ctrl+F"),
                    MenuItem::new("Find next", Msg::FindNext).shortcut("Ctrl+G"),
                    MenuItem::separator(),
                    MenuItem::new("Go to the start", Msg::Edit(EditAction::GoToStart)).shortcut("Ctrl+Home"),
                    MenuItem::new("Go to the end", Msg::Edit(EditAction::GoToEnd)).shortcut("Ctrl+End"),
                ],
            ),
            MenuItem::submenu(
                "View",
                alloc::vec![
                    MenuItem::new("Line numbers", Msg::ToggleLineNumbers).checked(self.line_numbers),
                    MenuItem::new("Highlight the current line", Msg::ToggleHighlight).checked(self.highlight_line),
                    MenuItem::new("Status bar", Msg::ToggleStatusBar).checked(self.status_bar),
                ],
            ),
            MenuItem::submenu("Help", alloc::vec![MenuItem::new("Keyboard shortcuts", Msg::Shortcuts), MenuItem::new("About Notes", Msg::About)]),
        ]
    }

    fn close_requested(&mut self, cx: &mut Context<Msg>) -> bool {
        if self.modified {
            cx.confirm("Discard the changes?", format!("“{}” has unsaved changes.", self.file_name()), Msg::DiscardAndQuit);
            false
        } else {
            true
        }
    }
}

fn run() -> i32 {
    hxtoolkit::run(Settings::new("notes").size(780, 560).min_size(420, 260), Notes::new())
}

#[cfg(target_os = "hamix")]
hxstd::entry!(run);

#[cfg(not(target_os = "hamix"))]
hxtoolkit::main!(run);
