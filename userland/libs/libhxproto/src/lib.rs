#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, c_int, c_void, CStr};
use core::sync::atomic::{AtomicUsize, Ordering};

use proto::client::Window;
use proto::{Event, Request, Text, Title};

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn aligned_alloc(align: usize, size: usize) -> *mut c_void;
    fn abort() -> !;
}

struct Malloc;

const MALLOC_ALIGN: usize = 16;

unsafe impl GlobalAlloc for Malloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGN {
            malloc(layout.size()) as *mut u8
        } else {
            aligned_alloc(layout.align(), layout.size().next_multiple_of(layout.align())) as *mut u8
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        free(ptr as *mut c_void)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGN {
            return realloc(ptr as *mut c_void, new_size) as *mut u8;
        }
        let new = self.alloc(Layout::from_size_align_unchecked(new_size, layout.align()));
        if !new.is_null() {
            core::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
            self.dealloc(ptr, layout);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Malloc = Malloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { abort() }
}

const ERRORS: [&CStr; 6] = [c"no error", c"hxserver is not running", c"cannot allocate the window buffer", c"hxserver rejected the window", c"hxserver did not answer", c"invalid argument"];

static LAST_ERROR: AtomicUsize = AtomicUsize::new(0);

fn fail(message: &str) {
    let index = match message {
        "hxserver is not running" | "hxserver exited" => 1,
        "cannot allocate the window buffer" => 2,
        "hxserver rejected the window" | "hxserver refused the connection" => 3,
        "hxserver did not answer" => 4,
        _ => 5,
    };
    LAST_ERROR.store(index, Ordering::Relaxed);
}

pub struct HxWindow {
    inner: Window,
}

#[repr(C)]
pub struct HxEvent {
    kind: u32,
    window: u32,
    x: i32,
    y: i32,
    buttons: u32,
    mouse_kind: u32,
    wheel: i32,
    key: i32,
    mods: u32,
    width: u32,
    height: u32,
    flag: u32,
}

const EVENT_MOUSE: u32 = 1;
const EVENT_KEY: u32 = 2;
const EVENT_CLOSE: u32 = 3;
const EVENT_FOCUS: u32 = 4;
const EVENT_RESIZE: u32 = 5;
const EVENT_THEME: u32 = 6;
const EVENT_SCREEN: u32 = 7;
const EVENT_PLACED: u32 = 8;
const EVENT_OTHER: u32 = 99;

fn text_of(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    String::from(unsafe { CStr::from_ptr(ptr) }.to_string_lossy())
}

fn window<'a>(ptr: *mut HxWindow) -> Option<&'a mut HxWindow> {
    unsafe { ptr.as_mut() }
}

fn window_ref<'a>(ptr: *const HxWindow) -> Option<&'a HxWindow> {
    unsafe { ptr.as_ref() }
}

fn fill(event: Event, out: &mut HxEvent) {
    *out = HxEvent { kind: EVENT_OTHER, window: 0, x: 0, y: 0, buttons: 0, mouse_kind: 0, wheel: 0, key: 0, mods: 0, width: 0, height: 0, flag: 0 };
    match event {
        Event::Mouse { window, x, y, buttons, kind, wheel } => {
            out.kind = EVENT_MOUSE;
            out.window = window;
            out.x = x;
            out.y = y;
            out.buttons = buttons;
            out.mouse_kind = kind;
            out.wheel = wheel;
        }
        Event::Key { window, code, mods } => {
            out.kind = EVENT_KEY;
            out.window = window;
            out.key = code;
            out.mods = mods;
        }
        Event::Close { window } => {
            out.kind = EVENT_CLOSE;
            out.window = window;
        }
        Event::Focus { window, focused } => {
            out.kind = EVENT_FOCUS;
            out.window = window;
            out.flag = focused as u32;
        }
        Event::Resize { window, width, height } => {
            out.kind = EVENT_RESIZE;
            out.window = window;
            out.width = width;
            out.height = height;
        }
        Event::Theme { light } => {
            out.kind = EVENT_THEME;
            out.flag = light as u32;
        }
        Event::Screen { width, height } => {
            out.kind = EVENT_SCREEN;
            out.width = width;
            out.height = height;
        }
        Event::Placed { window, x, y } => {
            out.kind = EVENT_PLACED;
            out.window = window;
            out.x = x;
            out.y = y;
        }
        _ => {}
    }
}

#[no_mangle]
pub extern "C" fn hx_version() -> *const c_char {
    c"0.1.0".as_ptr()
}

#[no_mangle]
pub extern "C" fn hx_available() -> c_int {
    proto::client::server_running() as c_int
}

#[no_mangle]
pub extern "C" fn hx_last_error() -> *const c_char {
    ERRORS[LAST_ERROR.load(Ordering::Relaxed).min(ERRORS.len() - 1)].as_ptr()
}

#[no_mangle]
pub extern "C" fn hx_window_create(title: *const c_char, width: u32, height: u32) -> *mut HxWindow {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        fail("invalid argument");
        return core::ptr::null_mut();
    }
    match Window::open(&text_of(title), width, height) {
        Ok(inner) => {
            LAST_ERROR.store(0, Ordering::Relaxed);
            Box::into_raw(Box::new(HxWindow { inner }))
        }
        Err(e) => {
            fail(e);
            core::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn hx_window_destroy(ptr: *mut HxWindow) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

#[no_mangle]
pub extern "C" fn hx_window_pixels(ptr: *mut HxWindow) -> *mut u32 {
    window(ptr).map(|w| w.inner.buffer().as_mut_ptr()).unwrap_or(core::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn hx_window_width(ptr: *const HxWindow) -> u32 {
    window_ref(ptr).map(|w| w.inner.width).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn hx_window_height(ptr: *const HxWindow) -> u32 {
    window_ref(ptr).map(|w| w.inner.height).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn hx_window_id(ptr: *const HxWindow) -> u32 {
    window_ref(ptr).map(|w| w.inner.id).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn hx_window_fd(ptr: *const HxWindow) -> c_int {
    window_ref(ptr).map(|w| w.inner.conn.fd()).unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn hx_window_present(ptr: *mut HxWindow) -> c_int {
    match window(ptr) {
        Some(w) => {
            w.inner.present();
            0
        }
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn hx_window_present_rect(ptr: *mut HxWindow, x: u32, y: u32, width: u32, height: u32) -> c_int {
    let Some(w) = window(ptr) else {
        return -1;
    };
    let x = x.min(w.inner.width);
    let y = y.min(w.inner.height);
    let width = width.min(w.inner.width - x);
    let height = height.min(w.inner.height - y);
    if w.inner.conn.send(Request::Present { window: w.inner.id, x, y, width, height }) { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn hx_window_resize(ptr: *mut HxWindow, width: u32, height: u32) -> c_int {
    window(ptr).map(|w| w.inner.resize(width, height) as c_int).unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn hx_window_set_title(ptr: *mut HxWindow, title: *const c_char) {
    if let Some(w) = window(ptr) {
        w.inner.set_title(&text_of(title));
    }
}

#[no_mangle]
pub extern "C" fn hx_window_set_app_id(ptr: *mut HxWindow, app_id: *const c_char) {
    if let Some(w) = window(ptr) {
        w.inner.set_app_id(&text_of(app_id));
    }
}

#[no_mangle]
pub extern "C" fn hx_window_set_icon(ptr: *mut HxWindow, name: *const c_char) {
    if let Some(w) = window(ptr) {
        w.inner.set_icon(&text_of(name));
    }
}

#[no_mangle]
pub extern "C" fn hx_window_set_cursor(ptr: *mut HxWindow, cursor: u32) {
    if let Some(w) = window(ptr) {
        w.inner.set_cursor(cursor);
    }
}

#[no_mangle]
pub extern "C" fn hx_window_set_size_hints(ptr: *mut HxWindow, min_width: u32, min_height: u32, max_width: u32, max_height: u32) {
    if let Some(w) = window(ptr) {
        w.inner.size_hints((min_width, min_height), (max_width, max_height));
    }
}

#[no_mangle]
pub extern "C" fn hx_window_set_maximized(ptr: *mut HxWindow, maximized: c_int) {
    if let Some(w) = window(ptr) {
        w.inner.conn.send(Request::SetMaximized { window: w.inner.id, maximized: (maximized != 0) as u32 });
    }
}

#[no_mangle]
pub extern "C" fn hx_window_minimize(ptr: *mut HxWindow) {
    if let Some(w) = window(ptr) {
        w.inner.conn.send(Request::Minimize { window: w.inner.id });
    }
}

#[no_mangle]
pub extern "C" fn hx_window_notify(ptr: *mut HxWindow, title: *const c_char, body: *const c_char) {
    if let Some(w) = window(ptr) {
        w.inner.conn.send(Request::Notify { title: Title::new(&text_of(title)), body: Title::new(&text_of(body)) });
    }
}

#[no_mangle]
pub extern "C" fn hx_window_copy_text(ptr: *mut HxWindow, text: *const c_char) {
    if let Some(w) = window(ptr) {
        w.inner.conn.send(Request::SetSelection { mime: Title::new("text/plain"), data: Text::new(&text_of(text)) });
    }
}

#[no_mangle]
pub extern "C" fn hx_window_next_event(ptr: *mut HxWindow, out: *mut HxEvent, timeout_ms: c_int) -> c_int {
    let (Some(w), Some(out)) = (window(ptr), unsafe { out.as_mut() }) else {
        return -1;
    };
    match w.inner.next_event(timeout_ms as i64) {
        Some(event) => {
            fill(event, out);
            1
        }
        None if !w.inner.conn.server_alive() => -1,
        None => 0,
    }
}
