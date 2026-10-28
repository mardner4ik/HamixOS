use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::element::Element;
use crate::keys::{KeyPress, Shortcut};

pub trait App: Sized + 'static {
    type Message: Clone + 'static;

    fn title(&self) -> String;

    fn view(&self) -> Element<Self::Message>;

    fn update(&mut self, message: Self::Message, cx: &mut Context<Self::Message>);

    fn key(&self, _press: &KeyPress) -> Option<Self::Message> {
        None
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        Vec::new()
    }

    fn tick(&self) -> Option<(u32, Self::Message)> {
        None
    }

    fn close_requested(&mut self, _cx: &mut Context<Self::Message>) -> bool {
        true
    }

    fn started(&mut self, _cx: &mut Context<Self::Message>) {}

    fn watch(&self) -> Vec<(i32, Self::Message)> {
        Vec::new()
    }

    fn appearance(&mut self, _dark: bool) {}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayServer {
    #[default]
    Auto,
    Hxserver,
    Wayland,
}

impl DisplayServer {
    pub fn name(self) -> &'static str {
        match self {
            DisplayServer::Auto => "auto",
            DisplayServer::Hxserver => "hxserver",
            DisplayServer::Wayland => "wayland",
        }
    }

    pub fn parse(text: &str) -> Option<DisplayServer> {
        match text.trim() {
            "auto" | "" => Some(DisplayServer::Auto),
            "hxserver" | "nook" => Some(DisplayServer::Hxserver),
            "wayland" => Some(DisplayServer::Wayland),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub app_id: String,
    pub icon: Option<String>,
    pub size: (u32, u32),
    pub min_size: Option<(u32, u32)>,
    pub resizable: bool,
    pub display: DisplayServer,
}

impl Settings {
    pub fn new(app_id: impl Into<String>) -> Settings {
        Settings { app_id: app_id.into(), icon: None, size: (640, 480), min_size: None, resizable: true, display: DisplayServer::Auto }
    }

    pub fn display(mut self, server: DisplayServer) -> Settings {
        self.display = server;
        self
    }

    pub fn hxserver_only(self) -> Settings {
        self.display(DisplayServer::Hxserver)
    }

    pub fn wayland_only(self) -> Settings {
        self.display(DisplayServer::Wayland)
    }

    pub fn size(mut self, width: u32, height: u32) -> Settings {
        self.size = (width, height);
        self
    }

    pub fn min_size(mut self, width: u32, height: u32) -> Settings {
        self.min_size = Some((width, height));
        self
    }

    pub fn fixed(mut self) -> Settings {
        self.resizable = false;
        self
    }

    pub fn icon(mut self, name: impl Into<String>) -> Settings {
        self.icon = Some(name.into());
        self
    }
}

pub struct MenuItem<M> {
    pub label: String,
    pub shortcut: Option<Shortcut>,
    pub message: Option<M>,
    pub checked: Option<bool>,
    pub separator: bool,
    pub children: Vec<MenuItem<M>>,
}

impl<M> MenuItem<M> {
    pub fn new(label: impl Into<String>, message: M) -> MenuItem<M> {
        MenuItem { label: label.into(), shortcut: None, message: Some(message), checked: None, separator: false, children: Vec::new() }
    }

    pub fn separator() -> MenuItem<M> {
        MenuItem { label: String::new(), shortcut: None, message: None, checked: None, separator: true, children: Vec::new() }
    }

    pub fn submenu(label: impl Into<String>, children: Vec<MenuItem<M>>) -> MenuItem<M> {
        MenuItem { label: label.into(), shortcut: None, message: None, checked: None, separator: false, children }
    }

    pub fn shortcut(mut self, text: &str) -> MenuItem<M> {
        self.shortcut = Shortcut::parse(text);
        self
    }

    pub fn checked(mut self, checked: bool) -> MenuItem<M> {
        self.checked = Some(checked);
        self
    }

    pub fn disabled(mut self) -> MenuItem<M> {
        self.message = None;
        self
    }
}

pub type Reply<T, M> = Box<dyn FnOnce(T) -> M>;

pub enum FileDialog {
    Open,
    Save,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditAction {
    SelectAll,
    Cut,
    Copy,
    Paste,
    Delete,
    Undo,
    Redo,
    GoToStart,
    GoToEnd,
    Focus,
}

pub enum Command<M> {
    Edit { id: String, action: EditAction },
    Find { id: String, query: String, reply: Reply<bool, M> },
    Quit,
    After { ms: u32, message: M },
    Copy(String),
    Paste(Reply<Option<String>, M>),
    ChooseFile { kind: FileDialog, title: String, filters: Vec<String>, start: String, reply: Reply<Option<String>, M> },
    Message { title: String, text: String },
    Confirm { title: String, text: String, reply: Reply<bool, M> },
    Notify { title: String, body: String },
    Resize { width: u32, height: u32 },
    SetWallpaper { path: String, reply: Reply<bool, M> },
    ShowMenu(Vec<MenuItem<M>>),
    ResetScroll,
}

pub struct Context<M> {
    pub(crate) commands: Vec<Command<M>>,
}

impl<M> Default for Context<M> {
    fn default() -> Self {
        Context { commands: Vec::new() }
    }
}

impl<M> Context<M> {
    pub fn new() -> Context<M> {
        Context::default()
    }

    pub fn take(&mut self) -> Vec<Command<M>> {
        core::mem::take(&mut self.commands)
    }

    pub fn edit(&mut self, id: impl Into<String>, action: EditAction) {
        self.commands.push(Command::Edit { id: id.into(), action });
    }

    pub fn find(&mut self, id: impl Into<String>, query: impl Into<String>, reply: impl FnOnce(bool) -> M + 'static) {
        self.commands.push(Command::Find { id: id.into(), query: query.into(), reply: Box::new(reply) });
    }

    pub fn quit(&mut self) {
        self.commands.push(Command::Quit);
    }

    pub fn after(&mut self, ms: u32, message: M) {
        self.commands.push(Command::After { ms, message });
    }

    pub fn copy(&mut self, text: impl Into<String>) {
        self.commands.push(Command::Copy(text.into()));
    }

    pub fn reset_scroll(&mut self) {
        self.commands.push(Command::ResetScroll);
    }

    pub fn paste(&mut self, reply: impl FnOnce(Option<String>) -> M + 'static) {
        self.commands.push(Command::Paste(Box::new(reply)));
    }

    pub fn open_file(&mut self, title: impl Into<String>, filters: &[&str], reply: impl FnOnce(Option<String>) -> M + 'static) {
        self.file_dialog(FileDialog::Open, title.into(), filters, String::new(), reply);
    }

    pub fn save_file(&mut self, title: impl Into<String>, suggested: impl Into<String>, reply: impl FnOnce(Option<String>) -> M + 'static) {
        self.file_dialog(FileDialog::Save, title.into(), &[], suggested.into(), reply);
    }

    pub fn choose_folder(&mut self, title: impl Into<String>, reply: impl FnOnce(Option<String>) -> M + 'static) {
        self.file_dialog(FileDialog::Folder, title.into(), &[], String::new(), reply);
    }

    fn file_dialog(&mut self, kind: FileDialog, title: String, filters: &[&str], start: String, reply: impl FnOnce(Option<String>) -> M + 'static) {
        self.commands.push(Command::ChooseFile { kind, title, filters: filters.iter().map(|f| String::from(*f)).collect(), start, reply: Box::new(reply) });
    }

    pub fn message(&mut self, title: impl Into<String>, text: impl Into<String>) {
        self.commands.push(Command::Message { title: title.into(), text: text.into() });
    }

    pub fn confirm(&mut self, title: impl Into<String>, text: impl Into<String>, reply: impl FnOnce(bool) -> M + 'static) {
        self.commands.push(Command::Confirm { title: title.into(), text: text.into(), reply: Box::new(reply) });
    }

    pub fn notify(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.commands.push(Command::Notify { title: title.into(), body: body.into() });
    }

    pub fn set_wallpaper(&mut self, path: impl Into<String>, reply: impl FnOnce(bool) -> M + 'static) {
        self.commands.push(Command::SetWallpaper { path: path.into(), reply: Box::new(reply) });
    }

    pub fn show_menu(&mut self, items: Vec<MenuItem<M>>) {
        if !items.is_empty() {
            self.commands.push(Command::ShowMenu(items));
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.commands.push(Command::Resize { width, height });
    }
}

pub fn shortcut_message<M: Clone>(menu: &[MenuItem<M>], press: &KeyPress) -> Option<M> {
    for item in menu {
        if item.shortcut.map(|s| s.matches(press)).unwrap_or(false) {
            if let Some(m) = &item.message {
                return Some(m.clone());
            }
        }
        if let Some(m) = shortcut_message(&item.children, press) {
            return Some(m);
        }
    }
    None
}
