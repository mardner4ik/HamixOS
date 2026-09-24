#![no_std]

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
const RELOAD: u32 = 7;
const SET_ICON: u32 = 8;
const ATTACH: u32 = 9;
const SIZE_HINTS: u32 = 10;
const DISPLAY_CHANGED: u32 = 11;
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
    Reload,
    SetIcon { window: u32, name: Title },
    Attach { window: u32, shm: u32, width: u32, height: u32 },
    SizeHints { window: u32, min_width: u32, min_height: u32, max_width: u32, max_height: u32 },
    DisplayChanged,
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
            Request::Reload => {
                w.u32(RELOAD);
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
            Request::DisplayChanged => {
                w.u32(DISPLAY_CHANGED);
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
            RELOAD => Request::Reload,
            SET_ICON => Request::SetIcon { window: r.u32()?, name: r.title()? },
            SET_APP_ID => Request::SetAppId { window: r.u32()?, pid: r.u32()?, app_id: r.title()? },
            ATTACH => Request::Attach { window: r.u32()?, shm: r.u32()?, width: r.u32()?, height: r.u32()? },
            SIZE_HINTS => Request::SizeHints { window: r.u32()?, min_width: r.u32()?, min_height: r.u32()?, max_width: r.u32()?, max_height: r.u32()? },
            DISPLAY_CHANGED => Request::DisplayChanged,
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
            _ => return None,
        })
    }
}
