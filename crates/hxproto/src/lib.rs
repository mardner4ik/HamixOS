#![no_std]

#[cfg(feature = "client")]
extern crate alloc;

#[cfg(feature = "client")]
pub mod client;

pub const SERVICE: &str = "hxserver";
pub const TITLE_MAX: usize = 110;
pub const TEXT_MAX: usize = 1024;
pub const MESSAGE_MAX: usize = 2400;

pub const PICK_OPEN_FILE: u32 = 0;
pub const PICK_FOLDER: u32 = 1;
pub const PICK_SAVE_FILE: u32 = 2;

pub const PICK_OK: u32 = 0;
pub const PICK_CANCELLED: u32 = 1;
pub const PICK_FAILED: u32 = 2;

pub const MOUSE_MOVE: u32 = 0;
pub const MOUSE_PRESS: u32 = 1;
pub const MOUSE_RELEASE: u32 = 2;
pub const MOUSE_WHEEL: u32 = 3;
pub const MOUSE_LEAVE: u32 = 4;

pub const VERSION: u32 = 2;
pub const FEATURE_LAYERS: u32 = 1;
pub const FEATURE_TOPLEVELS: u32 = 2;
pub const FEATURE_WORKSPACES: u32 = 4;
pub const FEATURE_SELECTION: u32 = 8;
pub const FEATURE_DND: u32 = 16;
pub const FEATURE_OUTPUT: u32 = 32;

pub const LAYER_BACKGROUND: u32 = 0;
pub const LAYER_BOTTOM: u32 = 1;
pub const LAYER_TOP: u32 = 2;
pub const LAYER_OVERLAY: u32 = 3;

pub const ANCHOR_TOP: u32 = 1;
pub const ANCHOR_BOTTOM: u32 = 2;
pub const ANCHOR_LEFT: u32 = 4;
pub const ANCHOR_RIGHT: u32 = 8;

pub const TOPLEVEL_ACTIVE: u32 = 1;
pub const TOPLEVEL_MINIMIZED: u32 = 2;
pub const TOPLEVEL_MAXIMIZED: u32 = 4;

pub const ACTION_ACTIVATE: u32 = 0;
pub const ACTION_MINIMIZE: u32 = 1;
pub const ACTION_CLOSE: u32 = 2;
pub const ACTION_TOGGLE_MAXIMIZE: u32 = 3;

pub const SHELL_LOGOUT: u32 = 0;
pub const SHELL_REBOOT: u32 = 1;
pub const SHELL_SHUTDOWN: u32 = 2;
pub const SHELL_ABOUT: u32 = 3;

pub const CURSOR_ARROW: u32 = 0;
pub const CURSOR_TEXT: u32 = 1;
pub const CURSOR_HAND: u32 = 2;
pub const CURSOR_MOVE: u32 = 3;
pub const CURSOR_RESIZE_NS: u32 = 4;
pub const CURSOR_RESIZE_EW: u32 = 5;
pub const CURSOR_RESIZE_NWSE: u32 = 6;
pub const CURSOR_RESIZE_NESW: u32 = 7;

const CREATE_WINDOW: u32 = 1;
const PRESENT: u32 = 2;
const DESTROY: u32 = 3;
const SET_TITLE: u32 = 4;
const SET_CURSOR: u32 = 5;
const NOTIFY: u32 = 6;
const SET_WALLPAPER: u32 = 7;
const SET_ICON: u32 = 8;
const ATTACH: u32 = 9;
const SIZE_HINTS: u32 = 10;
const CHOOSE_FILE: u32 = 12;
const PICKER_RESULT: u32 = 13;
const SET_MAXIMIZED: u32 = 14;
const CREATE_POPUP: u32 = 15;
const MOVE_POPUP: u32 = 16;
const TRACK_POSITION: u32 = 17;
const SET_FRAME: u32 = 18;
const BEGIN_MOVE: u32 = 19;
const BEGIN_RESIZE: u32 = 20;
const MINIMIZE: u32 = 21;
const SET_APP_ID: u32 = 22;
const HELLO: u32 = 30;
const CREATE_LAYER: u32 = 31;
const CONFIGURE_LAYER: u32 = 32;
const SUBSCRIBE_TOPLEVELS: u32 = 33;
const TOPLEVEL_ACTION: u32 = 34;
const SET_SELECTION: u32 = 35;
const GET_SELECTION: u32 = 36;
const SWITCH_WORKSPACE: u32 = 37;
const MOVE_TO_WORKSPACE: u32 = 38;
const START_DRAG: u32 = 39;
const SHELL_ACTION: u32 = 40;
const TOAST: u32 = 41;
const TOPLEVEL_RECTANGLE: u32 = 42;

const CREATED: u32 = 100;
const MOUSE: u32 = 101;
const KEY: u32 = 102;
const CLOSE: u32 = 103;
const FOCUS: u32 = 104;
const REJECTED: u32 = 105;
const RESIZE: u32 = 106;
const FILE_CHOSEN: u32 = 107;
const THEME: u32 = 108;
const PLACED: u32 = 109;
const SCREEN: u32 = 110;
const WELCOME: u32 = 111;
const TOPLEVEL_STATE: u32 = 112;
const TOPLEVEL_CLOSED: u32 = 113;
const SELECTION: u32 = 114;
const WORKSPACES: u32 = 115;
const OUTPUT: u32 = 116;
const DROP: u32 = 117;
const LAYER_CONFIGURED: u32 = 118;
const WALLPAPER_RESULT: u32 = 119;
const TOPLEVEL_GEOMETRY: u32 = 120;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Title {
    bytes: [u8; TITLE_MAX],
    len: u8,
}

impl Title {
    pub fn new(text: &str) -> Self {
        let mut bytes = [0u8; TITLE_MAX];
        let mut len = 0usize;
        for ch in text.chars() {
            let mut tmp = [0u8; 4];
            let encoded = ch.encode_utf8(&mut tmp).as_bytes();
            if len + encoded.len() > TITLE_MAX {
                break;
            }
            bytes[len..len + encoded.len()].copy_from_slice(encoded);
            len += encoded.len();
        }
        Self { bytes, len: len as u8 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Text {
    bytes: [u8; TEXT_MAX],
    len: u16,
}

impl Text {
    pub fn new(text: &str) -> Self {
        let mut bytes = [0u8; TEXT_MAX];
        let mut len = 0usize;
        for ch in text.chars() {
            let mut tmp = [0u8; 4];
            let encoded = ch.encode_utf8(&mut tmp).as_bytes();
            if len + encoded.len() > TEXT_MAX {
                break;
            }
            bytes[len..len + encoded.len()].copy_from_slice(encoded);
            len += encoded.len();
        }
        Self { bytes, len: len as u16 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

#[derive(Clone, Copy)]
pub enum Request {
    CreateWindow { shm: u32, width: u32, height: u32, title: Title },
    Present { window: u32, x: u32, y: u32, width: u32, height: u32 },
    Destroy { window: u32 },
    SetTitle { window: u32, title: Title },
    SetCursor { window: u32, cursor: u32 },
    Notify { title: Title, body: Title },
    SetWallpaper { request: u32, path: Text },
    SetIcon { window: u32, name: Title },
    Attach { window: u32, shm: u32, width: u32, height: u32 },
    SizeHints { window: u32, min_width: u32, min_height: u32, max_width: u32, max_height: u32 },
    ChooseFile { request: u32, mode: u32, title: Title, filters: Text, start: Text },
    PickerResult { token: u32, status: u32, path: Text },
    SetMaximized { window: u32, maximized: u32 },
    CreatePopup { shm: u32, width: u32, height: u32, parent: u32, x: i32, y: i32 },
    MovePopup { window: u32, x: i32, y: i32 },
    TrackPosition { window: u32 },
    SetFrame { window: u32, decorated: u32 },
    BeginMove { window: u32 },
    BeginResize { window: u32, edges: u32 },
    Minimize { window: u32 },
    SetAppId { window: u32, pid: u32, app_id: Title },
    Hello { version: u32 },
    CreateLayer { shm: u32, width: u32, height: u32, layer: u32, anchor: u32, exclusive: i32, margins: [i32; 4], keyboard: u32 },
    ConfigureLayer { window: u32, anchor: u32, exclusive: i32, margins: [i32; 4] },
    SubscribeToplevels,
    ToplevelAction { toplevel: u32, action: u32 },
    SetSelection { mime: Title, data: Text },
    GetSelection { mime: Title },
    SwitchWorkspace { index: u32 },
    MoveToWorkspace { window: u32, index: u32 },
    StartDrag { window: u32, mime: Title, data: Text },
    ShellAction { action: u32 },
    Toast { title: Title, body: Title, icon: Title },
    ToplevelRectangle { toplevel: u32, x: i32, y: i32, width: i32, height: i32 },
}

#[derive(Clone, Copy)]
pub enum Event {
    Created { window: u32 },
    Mouse { window: u32, x: i32, y: i32, buttons: u32, kind: u32, wheel: i32 },
    Key { window: u32, code: i32, mods: u32 },
    Close { window: u32 },
    Focus { window: u32, focused: bool },
    Rejected { reason: u32 },
    Resize { window: u32, width: u32, height: u32 },
    FileChosen { request: u32, status: u32, path: Text },
    Theme { light: bool },
    Placed { window: u32, x: i32, y: i32 },
    Screen { width: u32, height: u32 },
    Welcome { version: u32, features: u32 },
    ToplevelState { toplevel: u32, state: u32, workspace: u32, title: Title, app_id: Title },
    ToplevelClosed { toplevel: u32 },
    Selection { mime: Title, data: Text },
    Workspaces { current: u32, count: u32 },
    Output { width: u32, height: u32, scale: u32 },
    Drop { window: u32, x: i32, y: i32, mime: Title, data: Text },
    LayerConfigured { window: u32, x: i32, y: i32, width: u32, height: u32 },
    WallpaperResult { request: u32, accepted: bool },
    ToplevelGeometry { toplevel: u32, x: i32, y: i32, width: i32, height: i32 },
}

struct Writer<'a> {
    buf: &'a mut [u8; MESSAGE_MAX],
    len: usize,
}

impl Writer<'_> {
    fn u32(&mut self, v: u32) -> &mut Self {
        self.buf[self.len..self.len + 4].copy_from_slice(&v.to_le_bytes());
        self.len += 4;
        self
    }

    fn i32(&mut self, v: i32) -> &mut Self {
        self.u32(v as u32)
    }

    fn title(&mut self, t: &Title) -> &mut Self {
        self.buf[self.len] = t.len;
        let n = t.len as usize;
        self.buf[self.len + 1..self.len + 1 + n].copy_from_slice(&t.bytes[..n]);
        self.len += 1 + n;
        self
    }

    fn text(&mut self, t: &Text) -> &mut Self {
        let n = t.len as usize;
        self.buf[self.len..self.len + 2].copy_from_slice(&t.len.to_le_bytes());
        self.buf[self.len + 2..self.len + 2 + n].copy_from_slice(&t.bytes[..n]);
        self.len += 2 + n;
        self
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let bytes = self.buf.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    fn title(&mut self) -> Option<Title> {
        let len = (*self.buf.get(self.pos)? as usize).min(TITLE_MAX);
        let bytes = self.buf.get(self.pos + 1..self.pos + 1 + len)?;
        self.pos += 1 + len;
        let mut title = Title { bytes: [0; TITLE_MAX], len: len as u8 };
        title.bytes[..len].copy_from_slice(bytes);
        Some(title)
    }

    fn text(&mut self) -> Option<Text> {
        let raw = self.buf.get(self.pos..self.pos + 2)?;
        let len = (u16::from_le_bytes([raw[0], raw[1]]) as usize).min(TEXT_MAX);
        let bytes = self.buf.get(self.pos + 2..self.pos + 2 + len)?;
        self.pos += 2 + len;
        let mut text = Text { bytes: [0; TEXT_MAX], len: len as u16 };
        text.bytes[..len].copy_from_slice(bytes);
        Some(text)
    }
}

impl Request {
    pub fn encode(&self, buf: &mut [u8; MESSAGE_MAX]) -> usize {
        let mut w = Writer { buf, len: 0 };
        match self {
            Request::CreateWindow { shm, width, height, title } => {
                w.u32(CREATE_WINDOW).u32(*shm).u32(*width).u32(*height).title(title);
            }
            Request::Present { window, x, y, width, height } => {
                w.u32(PRESENT).u32(*window).u32(*x).u32(*y).u32(*width).u32(*height);
            }
            Request::Destroy { window } => {
                w.u32(DESTROY).u32(*window);
            }
            Request::SetTitle { window, title } => {
                w.u32(SET_TITLE).u32(*window).title(title);
            }
            Request::SetCursor { window, cursor } => {
                w.u32(SET_CURSOR).u32(*window).u32(*cursor);
            }
            Request::Notify { title, body } => {
                w.u32(NOTIFY).title(title).title(body);
            }
            Request::SetWallpaper { request, path } => {
                w.u32(SET_WALLPAPER).u32(*request).text(path);
            }
            Request::SetIcon { window, name } => {
                w.u32(SET_ICON).u32(*window).title(name);
            }
            Request::SetAppId { window, pid, app_id } => {
                w.u32(SET_APP_ID).u32(*window).u32(*pid).title(app_id);
            }
            Request::Attach { window, shm, width, height } => {
                w.u32(ATTACH).u32(*window).u32(*shm).u32(*width).u32(*height);
            }
            Request::SizeHints { window, min_width, min_height, max_width, max_height } => {
                w.u32(SIZE_HINTS).u32(*window).u32(*min_width).u32(*min_height).u32(*max_width).u32(*max_height);
            }
            Request::ChooseFile { request, mode, title, filters, start } => {
                w.u32(CHOOSE_FILE).u32(*request).u32(*mode).title(title).text(filters).text(start);
            }
            Request::PickerResult { token, status, path } => {
                w.u32(PICKER_RESULT).u32(*token).u32(*status).text(path);
            }
            Request::SetMaximized { window, maximized } => {
                w.u32(SET_MAXIMIZED).u32(*window).u32(*maximized);
            }
            Request::CreatePopup { shm, width, height, parent, x, y } => {
                w.u32(CREATE_POPUP).u32(*shm).u32(*width).u32(*height).u32(*parent).i32(*x).i32(*y);
            }
            Request::MovePopup { window, x, y } => {
                w.u32(MOVE_POPUP).u32(*window).i32(*x).i32(*y);
            }
            Request::TrackPosition { window } => {
                w.u32(TRACK_POSITION).u32(*window);
            }
            Request::SetFrame { window, decorated } => {
                w.u32(SET_FRAME).u32(*window).u32(*decorated);
            }
            Request::BeginMove { window } => {
                w.u32(BEGIN_MOVE).u32(*window);
            }
            Request::BeginResize { window, edges } => {
                w.u32(BEGIN_RESIZE).u32(*window).u32(*edges);
            }
            Request::Minimize { window } => {
                w.u32(MINIMIZE).u32(*window);
            }
            Request::Hello { version } => {
                w.u32(HELLO).u32(*version);
            }
            Request::CreateLayer { shm, width, height, layer, anchor, exclusive, margins, keyboard } => {
                w.u32(CREATE_LAYER).u32(*shm).u32(*width).u32(*height).u32(*layer).u32(*anchor).i32(*exclusive).i32(margins[0]).i32(margins[1]).i32(margins[2]).i32(margins[3]).u32(*keyboard);
            }
            Request::ConfigureLayer { window, anchor, exclusive, margins } => {
                w.u32(CONFIGURE_LAYER).u32(*window).u32(*anchor).i32(*exclusive).i32(margins[0]).i32(margins[1]).i32(margins[2]).i32(margins[3]);
            }
            Request::SubscribeToplevels => {
                w.u32(SUBSCRIBE_TOPLEVELS);
            }
            Request::ToplevelAction { toplevel, action } => {
                w.u32(TOPLEVEL_ACTION).u32(*toplevel).u32(*action);
            }
            Request::SetSelection { mime, data } => {
                w.u32(SET_SELECTION).title(mime).text(data);
            }
            Request::GetSelection { mime } => {
                w.u32(GET_SELECTION).title(mime);
            }
            Request::SwitchWorkspace { index } => {
                w.u32(SWITCH_WORKSPACE).u32(*index);
            }
            Request::MoveToWorkspace { window, index } => {
                w.u32(MOVE_TO_WORKSPACE).u32(*window).u32(*index);
            }
            Request::StartDrag { window, mime, data } => {
                w.u32(START_DRAG).u32(*window).title(mime).text(data);
            }
            Request::ShellAction { action } => {
                w.u32(SHELL_ACTION).u32(*action);
            }
            Request::Toast { title, body, icon } => {
                w.u32(TOAST).title(title).title(body).title(icon);
            }
            Request::ToplevelRectangle { toplevel, x, y, width, height } => {
                w.u32(TOPLEVEL_RECTANGLE).u32(*toplevel).i32(*x).i32(*y).i32(*width).i32(*height);
            }
        }
        w.len
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { buf: bytes, pos: 0 };
        Some(match r.u32()? {
            CREATE_WINDOW => Request::CreateWindow { shm: r.u32()?, width: r.u32()?, height: r.u32()?, title: r.title()? },
            PRESENT => Request::Present { window: r.u32()?, x: r.u32()?, y: r.u32()?, width: r.u32()?, height: r.u32()? },
            DESTROY => Request::Destroy { window: r.u32()? },
            SET_TITLE => Request::SetTitle { window: r.u32()?, title: r.title()? },
            SET_CURSOR => Request::SetCursor { window: r.u32()?, cursor: r.u32()? },
            NOTIFY => Request::Notify { title: r.title()?, body: r.title()? },
            SET_WALLPAPER => Request::SetWallpaper { request: r.u32()?, path: r.text()? },
            SET_ICON => Request::SetIcon { window: r.u32()?, name: r.title()? },
            SET_APP_ID => Request::SetAppId { window: r.u32()?, pid: r.u32()?, app_id: r.title()? },
            ATTACH => Request::Attach { window: r.u32()?, shm: r.u32()?, width: r.u32()?, height: r.u32()? },
            SIZE_HINTS => Request::SizeHints { window: r.u32()?, min_width: r.u32()?, min_height: r.u32()?, max_width: r.u32()?, max_height: r.u32()? },
            CHOOSE_FILE => Request::ChooseFile { request: r.u32()?, mode: r.u32()?, title: r.title()?, filters: r.text()?, start: r.text()? },
            PICKER_RESULT => Request::PickerResult { token: r.u32()?, status: r.u32()?, path: r.text()? },
            SET_MAXIMIZED => Request::SetMaximized { window: r.u32()?, maximized: r.u32()? },
            CREATE_POPUP => Request::CreatePopup { shm: r.u32()?, width: r.u32()?, height: r.u32()?, parent: r.u32()?, x: r.i32()?, y: r.i32()? },
            MOVE_POPUP => Request::MovePopup { window: r.u32()?, x: r.i32()?, y: r.i32()? },
            TRACK_POSITION => Request::TrackPosition { window: r.u32()? },
            SET_FRAME => Request::SetFrame { window: r.u32()?, decorated: r.u32()? },
            BEGIN_MOVE => Request::BeginMove { window: r.u32()? },
            BEGIN_RESIZE => Request::BeginResize { window: r.u32()?, edges: r.u32()? },
            MINIMIZE => Request::Minimize { window: r.u32()? },
            HELLO => Request::Hello { version: r.u32()? },
            CREATE_LAYER => Request::CreateLayer {
                shm: r.u32()?,
                width: r.u32()?,
                height: r.u32()?,
                layer: r.u32()?,
                anchor: r.u32()?,
                exclusive: r.i32()?,
                margins: [r.i32()?, r.i32()?, r.i32()?, r.i32()?],
                keyboard: r.u32()?,
            },
            CONFIGURE_LAYER => Request::ConfigureLayer { window: r.u32()?, anchor: r.u32()?, exclusive: r.i32()?, margins: [r.i32()?, r.i32()?, r.i32()?, r.i32()?] },
            SUBSCRIBE_TOPLEVELS => Request::SubscribeToplevels,
            TOPLEVEL_ACTION => Request::ToplevelAction { toplevel: r.u32()?, action: r.u32()? },
            SET_SELECTION => Request::SetSelection { mime: r.title()?, data: r.text()? },
            GET_SELECTION => Request::GetSelection { mime: r.title()? },
            SWITCH_WORKSPACE => Request::SwitchWorkspace { index: r.u32()? },
            MOVE_TO_WORKSPACE => Request::MoveToWorkspace { window: r.u32()?, index: r.u32()? },
            START_DRAG => Request::StartDrag { window: r.u32()?, mime: r.title()?, data: r.text()? },
            SHELL_ACTION => Request::ShellAction { action: r.u32()? },
            TOAST => Request::Toast { title: r.title()?, body: r.title()?, icon: r.title()? },
            TOPLEVEL_RECTANGLE => Request::ToplevelRectangle { toplevel: r.u32()?, x: r.i32()?, y: r.i32()?, width: r.i32()?, height: r.i32()? },
            _ => return None,
        })
    }
}

impl Event {
    pub fn encode(&self, buf: &mut [u8; MESSAGE_MAX]) -> usize {
        let mut w = Writer { buf, len: 0 };
        match self {
            Event::Created { window } => {
                w.u32(CREATED).u32(*window);
            }
            Event::Mouse { window, x, y, buttons, kind, wheel } => {
                w.u32(MOUSE).u32(*window).i32(*x).i32(*y).u32(*buttons).u32(*kind).i32(*wheel);
            }
            Event::Key { window, code, mods } => {
                w.u32(KEY).u32(*window).i32(*code).u32(*mods);
            }
            Event::Close { window } => {
                w.u32(CLOSE).u32(*window);
            }
            Event::Focus { window, focused } => {
                w.u32(FOCUS).u32(*window).u32(*focused as u32);
            }
            Event::Rejected { reason } => {
                w.u32(REJECTED).u32(*reason);
            }
            Event::Resize { window, width, height } => {
                w.u32(RESIZE).u32(*window).u32(*width).u32(*height);
            }
            Event::FileChosen { request, status, path } => {
                w.u32(FILE_CHOSEN).u32(*request).u32(*status).text(path);
            }
            Event::Theme { light } => {
                w.u32(THEME).u32(*light as u32);
            }
            Event::Placed { window, x, y } => {
                w.u32(PLACED).u32(*window).i32(*x).i32(*y);
            }
            Event::Screen { width, height } => {
                w.u32(SCREEN).u32(*width).u32(*height);
            }
            Event::Welcome { version, features } => {
                w.u32(WELCOME).u32(*version).u32(*features);
            }
            Event::ToplevelState { toplevel, state, workspace, title, app_id } => {
                w.u32(TOPLEVEL_STATE).u32(*toplevel).u32(*state).u32(*workspace).title(title).title(app_id);
            }
            Event::ToplevelClosed { toplevel } => {
                w.u32(TOPLEVEL_CLOSED).u32(*toplevel);
            }
            Event::Selection { mime, data } => {
                w.u32(SELECTION).title(mime).text(data);
            }
            Event::Workspaces { current, count } => {
                w.u32(WORKSPACES).u32(*current).u32(*count);
            }
            Event::Output { width, height, scale } => {
                w.u32(OUTPUT).u32(*width).u32(*height).u32(*scale);
            }
            Event::Drop { window, x, y, mime, data } => {
                w.u32(DROP).u32(*window).i32(*x).i32(*y).title(mime).text(data);
            }
            Event::LayerConfigured { window, x, y, width, height } => {
                w.u32(LAYER_CONFIGURED).u32(*window).i32(*x).i32(*y).u32(*width).u32(*height);
            }
            Event::WallpaperResult { request, accepted } => {
                w.u32(WALLPAPER_RESULT).u32(*request).u32(*accepted as u32);
            }
            Event::ToplevelGeometry { toplevel, x, y, width, height } => {
                w.u32(TOPLEVEL_GEOMETRY).u32(*toplevel).i32(*x).i32(*y).i32(*width).i32(*height);
            }
        }
        w.len
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { buf: bytes, pos: 0 };
        Some(match r.u32()? {
            CREATED => Event::Created { window: r.u32()? },
            MOUSE => Event::Mouse { window: r.u32()?, x: r.i32()?, y: r.i32()?, buttons: r.u32()?, kind: r.u32()?, wheel: r.i32()? },
            KEY => Event::Key { window: r.u32()?, code: r.i32()?, mods: r.u32().unwrap_or(0) },
            CLOSE => Event::Close { window: r.u32()? },
            FOCUS => Event::Focus { window: r.u32()?, focused: r.u32()? != 0 },
            REJECTED => Event::Rejected { reason: r.u32()? },
            RESIZE => Event::Resize { window: r.u32()?, width: r.u32()?, height: r.u32()? },
            FILE_CHOSEN => Event::FileChosen { request: r.u32()?, status: r.u32()?, path: r.text()? },
            THEME => Event::Theme { light: r.u32()? != 0 },
            PLACED => Event::Placed { window: r.u32()?, x: r.i32()?, y: r.i32()? },
            SCREEN => Event::Screen { width: r.u32()?, height: r.u32()? },
            WELCOME => Event::Welcome { version: r.u32()?, features: r.u32()? },
            TOPLEVEL_STATE => Event::ToplevelState { toplevel: r.u32()?, state: r.u32()?, workspace: r.u32()?, title: r.title()?, app_id: r.title()? },
            TOPLEVEL_CLOSED => Event::ToplevelClosed { toplevel: r.u32()? },
            SELECTION => Event::Selection { mime: r.title()?, data: r.text()? },
            WORKSPACES => Event::Workspaces { current: r.u32()?, count: r.u32()? },
            OUTPUT => Event::Output { width: r.u32()?, height: r.u32()?, scale: r.u32()? },
            DROP => Event::Drop { window: r.u32()?, x: r.i32()?, y: r.i32()?, mime: r.title()?, data: r.text()? },
            LAYER_CONFIGURED => Event::LayerConfigured { window: r.u32()?, x: r.i32()?, y: r.i32()?, width: r.u32()?, height: r.u32()? },
            WALLPAPER_RESULT => Event::WallpaperResult { request: r.u32()?, accepted: r.u32()? != 0 },
            TOPLEVEL_GEOMETRY => Event::ToplevelGeometry { toplevel: r.u32()?, x: r.i32()?, y: r.i32()?, width: r.i32()?, height: r.i32()? },
            _ => return None,
        })
    }
}
