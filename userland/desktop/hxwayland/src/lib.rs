#![no_std]

extern crate alloc;

mod keys;
mod wire;
mod xwm;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::unix::{self, PollFd, POLLHUP, POLLIN, POLLOUT};
use hxstd::{env, fs, println, sys};
use hxproto::{Event, Request, Title, MESSAGE_MAX};
use wire::{Message, Reader};

pub trait Link {
    fn request(&mut self, request: Request);
    fn receive(&mut self, timeout_ms: i64) -> Option<Event>;
    fn alive(&self) -> bool;
    fn mailbox(&self) -> u64;
    fn trusted_peer(&self, _pid: u32) -> bool {
        false
    }
    fn direct(&self) -> bool {
        false
    }
    fn attach(&mut self, _attach: Attach) {}
}

pub struct Attach {
    pub window: u32,
    pub pixels: *const u32,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub opaque: bool,
    pub damage: (u32, u32, u32, u32),
}

struct Mapping {
    ptr: *mut u8,
    size: usize,
}

impl Mapping {
    fn new(fd: u64, size: usize) -> Rc<Mapping> {
        let actual = sys::fstat_size(fd).max(0) as usize;
        let ptr = if size > 0 && size <= actual {
            unix::map_shared(fd, size as u64, 0).unwrap_or(core::ptr::null_mut())
        } else {
            core::ptr::null_mut()
        };
        Rc::new(Mapping { ptr, size: if ptr.is_null() { 0 } else { size } })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unix::unmap(self.ptr, self.size as u64);
        }
    }
}

pub struct IpcLink {
    pub server: i64,
    pub mailbox: u64,
}

impl Link for IpcLink {
    fn request(&mut self, request: Request) {
        let mut buf = [0u8; MESSAGE_MAX];
        let n = request.encode(&mut buf);
        sys::msg_send(self.server, &buf[..n]);
    }

    fn receive(&mut self, timeout_ms: i64) -> Option<Event> {
        let mut buf = [0u8; MESSAGE_MAX];
        let deadline = sys::uptime_ms() + timeout_ms.max(0) as u64;
        loop {
            let left = deadline.saturating_sub(sys::uptime_ms()) as i64;
            let (sender, len) = sys::msg_recv(&mut buf, left)?;
            if sender == self.server {
                if let Some(event) = Event::decode(&buf[..len]) {
                    return Some(event);
                }
            }
            if sys::uptime_ms() >= deadline {
                return None;
            }
        }
    }

    fn alive(&self) -> bool {
        sys::proc_alive(self.server)
    }

    fn mailbox(&self) -> u64 {
        self.mailbox
    }
}

const DISPLAY: u32 = 1;
const FRAME_MS: u64 = 16;
const F_SEAL_SHRINK: u32 = 0x0002;
const SERVER_DECORATION_MODE: u32 = 2;
const PANEL_H: i32 = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Display,
    Registry,
    Callback,
    Compositor,
    Region,
    Surface,
    Shm,
    Pool,
    Buffer,
    WmBase,
    Positioner,
    XdgSurface,
    Toplevel,
    Popup,
    Seat,
    Pointer,
    Keyboard,
    Touch,
    Output,
    Subcompositor,
    Subsurface,
    DataDeviceManager,
    DataDevice,
    DataSource,
    Decoration,
    DecorationManager,
    ServerDecoration,
    ServerDecorationManager,
    LayerShell,
    LayerSurface,
    ToplevelManager,
    ToplevelHandle,
    DataOffer,
    Viewporter,
    Viewport,
    FractionalScaleManager,
    FractionalScale,
    XdgOutputManager,
    XdgOutput,
    HamixShell,
}

const GLOBALS: [(&str, u32, Kind); 15] = [
    ("wl_compositor", 5, Kind::Compositor),
    ("wl_shm", 1, Kind::Shm),
    ("wl_seat", 7, Kind::Seat),
    ("wl_output", 4, Kind::Output),
    ("xdg_wm_base", 5, Kind::WmBase),
    ("wl_subcompositor", 1, Kind::Subcompositor),
    ("wl_data_device_manager", 3, Kind::DataDeviceManager),
    ("zxdg_decoration_manager_v1", 1, Kind::DecorationManager),
    ("org_kde_kwin_server_decoration_manager", 1, Kind::ServerDecorationManager),
    ("zwlr_layer_shell_v1", 4, Kind::LayerShell),
    ("zwlr_foreign_toplevel_manager_v1", 3, Kind::ToplevelManager),
    ("wp_viewporter", 1, Kind::Viewporter),
    ("wp_fractional_scale_manager_v1", 1, Kind::FractionalScaleManager),
    ("zxdg_output_manager_v1", 3, Kind::XdgOutputManager),
    ("hamix_shell_v1", 1, Kind::HamixShell),
];

fn privileged_global(kind: Kind) -> bool {
    matches!(kind, Kind::LayerShell | Kind::ToplevelManager | Kind::HamixShell)
}

const SERVER_ID_BASE: u32 = 0xFF00_0000;

#[derive(Clone)]
struct ForeignToplevel {
    state: u32,
    title: String,
    app_id: String,
}

#[derive(Clone, Copy)]
struct LayerRole {
    object: u32,
    layer: u32,
    anchor: u32,
    exclusive: i32,
    margins: [i32; 4],
    size: (u32, u32),
    keyboard: u32,
    configured: bool,
    dirty: bool,
    sent: (u32, u32),
}

struct Pool {
    live: bool,
    fd: u64,
    map: Rc<Mapping>,
}

struct Buffer {
    pool: u32,
    offset: usize,
    width: i32,
    height: i32,
    stride: i32,
    alpha: bool,
}

struct Window {
    id: u32,
    shm: u32,
    pixels: *mut u32,
    capacity: usize,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Default)]
struct Positioner {
    size: (i32, i32),
    rect: (i32, i32, i32, i32),
    anchor: u32,
    gravity: u32,
    constraint: u32,
    offset: (i32, i32),
}

#[derive(Clone, Copy)]
struct PopupRole {
    object: u32,
    parent: u32,
    positioner: Positioner,
    x: i32,
    y: i32,
    grab: bool,
    order: u32,
    configured: bool,
    done: bool,
}

#[derive(Default)]
struct Surface {
    pending: Option<u32>,
    attached: bool,
    pending_frames: Vec<u32>,
    frames: Vec<u32>,
    xdg: Option<u32>,
    toplevel: Option<u32>,
    title: String,
    app_id: String,
    configured: bool,
    window: Option<Window>,
    closed: bool,
    min: (u32, u32),
    max: (u32, u32),
    popup: Option<PopupRole>,
    geometry: Option<(i32, i32, i32, i32)>,
    xwin: Option<u32>,
    held: Option<u32>,
    damage: Option<(i32, i32, i32, i32)>,
    csd: bool,
    decoration: bool,
    layer: Option<LayerRole>,
    viewport_src: Option<(i32, i32, i32, i32)>,
    viewport_dst: Option<(i32, i32)>,
    shown: Option<(u32, Rc<Mapping>)>,
    scratch: Vec<u32>,
}

struct Client {
    fd: u64,
    inbuf: Vec<u8>,
    fds: VecDeque<u64>,
    objects: BTreeMap<u32, (Kind, u32)>,
    surfaces: BTreeMap<u32, Surface>,
    pools: BTreeMap<u32, Pool>,
    next_pool: u32,
    buffers: BTreeMap<u32, Buffer>,
    xdg_to_surface: BTreeMap<u32, u32>,
    positioners: BTreeMap<u32, Positioner>,
    pointers: Vec<u32>,
    keyboards: Vec<u32>,
    outputs: Vec<(u32, u32)>,
    dead: bool,
    decoration_manager: bool,
    pid: u32,
    trusted: bool,
    next_server_id: u32,
    toplevel_managers: Vec<u32>,
    handles: Vec<(u32, u32)>,
    shells: Vec<u32>,
    sources: BTreeMap<u32, Vec<String>>,
    data_devices: Vec<u32>,
    offers: Vec<u32>,
    out: VecDeque<(Vec<u8>, Vec<u64>)>,
    out_bytes: usize,
    xdg_outputs: Vec<(u32, u32, u32)>,
}

pub struct Bridge<L: Link> {
    pub link: L,
    listen: u64,
    clients: BTreeMap<u32, Client>,
    next_client: u32,
    windows: BTreeMap<u32, (u32, u32)>,
    serial: u32,
    keymap: Vec<u8>,
    pointer_focus: Option<(u32, u32)>,
    keyboard_focus: Option<(u32, u32)>,
    screen: (u32, u32),
    started: u64,
    last_frame: u64,
    backlog: VecDeque<Event>,
    placed: BTreeMap<u32, (i32, i32)>,
    popup_order: u32,
    xlisten: Option<u64>,
    xwm: Option<xwm::Xwm>,
    xcheck: u64,
    toplevels: BTreeMap<u32, ForeignToplevel>,
    subscribed: bool,
    clipboard: Option<(String, Vec<u8>)>,
    clipboard_owner: Option<(u32, u32)>,
    path: String,
    geometry: BTreeMap<u32, (i32, i32, i32, i32)>,
}

const MAX_CLIENT_OBJECTS: usize = 65_536;
const MAX_CLIENT_INPUT: usize = 1 << 20;
const MAX_CLIENT_MESSAGE: usize = 4 << 10;
const MAX_CLIENT_FDS: usize = 256;
const MAX_CLIENT_POOL_BYTES: usize = 512 << 20;
const OUT_LIMIT: usize = 4 << 20;

fn client_pool_bytes(client: &Client) -> usize {
    client.pools.values().fold(0usize, |total, pool| total.saturating_add(pool.map.size))
}

fn send(client: &mut Client, data: Vec<u8>, fds: &[u64]) {
    if client.dead {
        return;
    }
    let mut data = data;
    if client.out.is_empty() {
        let r = unix::send_with_fds(client.fd, &data, fds, unix::MSG_DONTWAIT);
        if r >= 0 && r as usize == data.len() {
            return;
        }
        if r < 0 && r != unix::EAGAIN {
            client.dead = true;
            return;
        }
        if r > 0 {
            data.drain(..r as usize);
            client.out_bytes += data.len();
            client.out.push_back((data, Vec::new()));
            return;
        }
    }
    let owned: Vec<u64> = fds.iter().map(|fd| sys::dup(*fd)).filter(|fd| *fd >= 0).map(|fd| fd as u64).collect();
    client.out_bytes += data.len();
    client.out.push_back((data, owned));
    if client.out_bytes > OUT_LIMIT {
        client.dead = true;
    }
}

fn flush_out(client: &mut Client) {
    while let Some((data, fds)) = client.out.front_mut() {
        if client.dead {
            return;
        }
        let r = unix::send_with_fds(client.fd, data, fds, unix::MSG_DONTWAIT);
        if r == unix::EAGAIN {
            return;
        }
        if r < 0 {
            client.dead = true;
            return;
        }
        for fd in fds.drain(..) {
            sys::close(fd);
        }
        let n = r as usize;
        client.out_bytes = client.out_bytes.saturating_sub(n);
        if n < data.len() {
            data.drain(..n);
            return;
        }
        client.out.pop_front();
    }
}

impl<L: Link> Bridge<L> {
    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }

    fn now(&self) -> u32 {
        (sys::uptime_ms() - self.started) as u32
    }

    fn request(&mut self, request: Request) {
        self.link.request(request);
    }

    fn accept(&mut self) {
        loop {
            let fd = unix::accept(self.listen, unix::SOCK_NONBLOCK);
            if fd < 0 {
                return;
            }
            self.add_client(fd as u64);
        }
    }

    fn add_client(&mut self, fd: u64) -> u32 {
        let pid = unix::peer_cred(fd).map(|cred| cred.pid).unwrap_or(0);
        let trusted = pid > 0 && self.link.trusted_peer(pid);
        {
            let id = self.next_client;
            self.next_client += 1;
            let mut objects = BTreeMap::new();
            objects.insert(DISPLAY, (Kind::Display, 0));
            self.clients.insert(
                id,
                Client {
                    fd,
                    inbuf: Vec::new(),
                    fds: VecDeque::new(),
                    objects,
                    surfaces: BTreeMap::new(),
                    pools: BTreeMap::new(),
                    next_pool: 1,
                    buffers: BTreeMap::new(),
                    xdg_to_surface: BTreeMap::new(),
                    positioners: BTreeMap::new(),
                    pointers: Vec::new(),
                    keyboards: Vec::new(),
                    outputs: Vec::new(),
                    dead: false,
                    decoration_manager: false,
                    pid,
                    trusted,
                    next_server_id: SERVER_ID_BASE,
                    toplevel_managers: Vec::new(),
                    handles: Vec::new(),
                    shells: Vec::new(),
                    sources: BTreeMap::new(),
                    data_devices: Vec::new(),
                    offers: Vec::new(),
                    out: VecDeque::new(),
                    out_bytes: 0,
                    xdg_outputs: Vec::new(),
                },
            );
            id
        }
    }

    fn read_client(&mut self, cid: u32) {
        let mut buf = [0u8; 8192];
        loop {
            let Some(client) = self.clients.get_mut(&cid) else {
                return;
            };
            match unix::recv_with_fds(client.fd, &mut buf, unix::MSG_DONTWAIT) {
                Ok((0, _)) => {
                    client.dead = true;
                    break;
                }
                Ok((n, fds)) => {
                    if client.inbuf.len().saturating_add(n) > MAX_CLIENT_INPUT
                        || client.fds.len().saturating_add(fds.len()) > MAX_CLIENT_FDS
                    {
                        for fd in fds {
                            sys::close(fd);
                        }
                        for fd in client.fds.drain(..) {
                            sys::close(fd);
                        }
                        client.dead = true;
                        break;
                    }
                    client.inbuf.extend_from_slice(&buf[..n]);
                    client.fds.extend(fds);
                }
                Err(unix::EAGAIN) => break,
                Err(_) => {
                    client.dead = true;
                    break;
                }
            }
        }
        loop {
            let (header, message) = {
                let Some(client) = self.clients.get_mut(&cid) else {
                    return;
                };
                if client.inbuf.len() < wlwire::HEADER_SIZE {
                    break;
                }
                let Some(header) = wlwire::decode_header(&client.inbuf) else {
                    client.dead = true;
                    break;
                };
                if header.size > MAX_CLIENT_MESSAGE {
                    client.dead = true;
                    break;
                }
                if client.inbuf.len() < header.size {
                    break;
                }
                let msg: Vec<u8> = client.inbuf.drain(..header.size).collect();
                (header, msg)
            };
            self.dispatch(cid, header.object, header.opcode, &message[wlwire::HEADER_SIZE..]);
        }
    }

    fn take_fd(&mut self, cid: u32) -> Option<u64> {
        self.clients.get_mut(&cid)?.fds.pop_front()
    }

    fn post(&mut self, cid: u32, data: Vec<u8>) {
        if let Some(client) = self.clients.get_mut(&cid) {
            send(client, data, &[]);
        }
    }

    fn post_fd(&mut self, cid: u32, data: Vec<u8>, fd: u64) {
        if let Some(client) = self.clients.get_mut(&cid) {
            send(client, data, &[fd]);
        }
    }

    fn add(&mut self, cid: u32, id: u32, kind: Kind, extra: u32) {
        if let Some(client) = self.clients.get_mut(&cid) {
            if id == 0 || client.objects.contains_key(&id) || client.objects.len() >= MAX_CLIENT_OBJECTS {
                client.dead = true;
                return;
            }
            client.objects.insert(id, (kind, extra));
        }
    }

    fn destroy(&mut self, cid: u32, id: u32) {
        if let Some(client) = self.clients.get_mut(&cid) {
            client.objects.remove(&id);
        }
        self.post(cid, Message::new(DISPLAY, 1).u32(id).finish());
    }

    fn dispatch(&mut self, cid: u32, object: u32, opcode: u16, args: &[u8]) {
        if self.clients.get(&cid).map(|client| client.dead).unwrap_or(true) {
            return;
        }
        let Some(&(kind, extra)) = self.clients.get(&cid).and_then(|c| c.objects.get(&object)) else {
            return;
        };
        let mut r = Reader::new(args);
        match (kind, opcode) {
            (Kind::Display, 0) => {
                let callback = r.u32();
                let serial = self.next_serial();
                self.post(cid, Message::new(callback, 0).u32(serial).finish());
                self.post(cid, Message::new(DISPLAY, 1).u32(callback).finish());
            }
            (Kind::Display, 1) => {
                let registry = r.u32();
                self.add(cid, registry, Kind::Registry, 0);
                let trusted = self.clients.get(&cid).map(|client| client.trusted).unwrap_or(false);
                for (i, (name, version, kind)) in GLOBALS.iter().enumerate() {
                    if privileged_global(*kind) && !trusted {
                        continue;
                    }
                    self.post(cid, Message::new(registry, 0).u32(i as u32 + 1).string(name).u32(*version).finish());
                }
            }
            (Kind::Registry, 0) => {
                let name = r.u32();
                let _interface = r.string();
                let version = r.u32();
                let id = r.u32();
                let Some((_, _, kind)) = GLOBALS.get(name.wrapping_sub(1) as usize) else {
                    return;
                };
                if privileged_global(*kind) && !self.clients.get(&cid).map(|client| client.trusted).unwrap_or(false) {
                    if let Some(client) = self.clients.get_mut(&cid) {
                        client.dead = true;
                    }
                    return;
                }
                self.add(cid, id, *kind, version);
                self.bound(cid, id, *kind, version);
            }
            (Kind::Callback, _) => {}
            (Kind::Compositor, 0) => {
                let id = r.u32();
                self.add(cid, id, Kind::Surface, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.surfaces.insert(id, Surface::default());
                }
            }
            (Kind::Compositor, 1) => {
                let id = r.u32();
                self.add(cid, id, Kind::Region, 0);
            }
            (Kind::Region, 0) => self.destroy(cid, object),
            (Kind::Region, _) => {}
            (Kind::Surface, op) => self.surface_request(cid, object, op, &mut r),
            (Kind::Shm, 0) => {
                let id = r.u32();
                let size = r.i32().max(0) as usize;
                let Some(fd) = self.take_fd(cid) else {
                    return;
                };
                let used = self.clients.get(&cid).map(client_pool_bytes).unwrap_or(MAX_CLIENT_POOL_BYTES);
                if size == 0 || used.saturating_add(size) > MAX_CLIENT_POOL_BYTES {
                    sys::close(fd);
                    if let Some(client) = self.clients.get_mut(&cid) {
                        client.dead = true;
                    }
                    return;
                }
                let seals = unix::get_seals(fd);
                if seals >= 0 && (seals as u32 & F_SEAL_SHRINK) == 0 && unix::add_seals(fd, F_SEAL_SHRINK) < 0 {
                    sys::close(fd);
                    if let Some(client) = self.clients.get_mut(&cid) {
                        client.dead = true;
                    }
                    return;
                }
                let map = Mapping::new(fd, size);
                if map.ptr.is_null() {
                    sys::close(fd);
                    if let Some(client) = self.clients.get_mut(&cid) {
                        client.dead = true;
                    }
                    return;
                }
                let Some(client) = self.clients.get_mut(&cid) else {
                    sys::close(fd);
                    return;
                };
                let key = client.next_pool;
                client.next_pool = client.next_pool.wrapping_add(1).max(1);
                client.pools.insert(key, Pool { live: true, fd, map });
                self.add(cid, id, Kind::Pool, key);
            }
            (Kind::Shm, _) => self.destroy(cid, object),
            (Kind::Pool, 0) => {
                let id = r.u32();
                let offset = r.i32().max(0) as usize;
                let width = r.i32();
                let height = r.i32();
                let stride = r.i32();
                let format = r.u32();
                self.add(cid, id, Kind::Buffer, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.buffers.insert(id, Buffer { pool: extra, offset, width, height, stride, alpha: format == 0 });
                }
            }
            (Kind::Pool, 1) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    if !client.buffers.values().any(|b| b.pool == extra) {
                        if let Some(pool) = client.pools.remove(&extra) {
                            release_pool(pool);
                        }
                    } else if let Some(pool) = client.pools.get_mut(&extra) {
                        pool.live = false;
                    }
                }
                self.destroy(cid, object);
            }
            (Kind::Pool, 2) => {
                let size = r.i32().max(0) as usize;
                if let Some(client) = self.clients.get_mut(&cid) {
                    let used = client_pool_bytes(client);
                    if let Some(pool) = client.pools.get_mut(&extra) {
                        if size > pool.map.size {
                            if used.saturating_add(size - pool.map.size) > MAX_CLIENT_POOL_BYTES {
                                client.dead = true;
                                return;
                            }
                            pool.map = Mapping::new(pool.fd, size);
                            if pool.map.ptr.is_null() {
                                client.dead = true;
                            }
                        }
                    }
                }
            }
            (Kind::Buffer, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    if let Some(buffer) = client.buffers.remove(&object) {
                        let pool_dead = !client.pools.get(&buffer.pool).map(|p| p.live).unwrap_or(false) && !client.buffers.values().any(|b| b.pool == buffer.pool);
                        if pool_dead {
                            if let Some(pool) = client.pools.remove(&buffer.pool) {
                                release_pool(pool);
                            }
                        }
                    }
                }
                self.destroy(cid, object);
            }
            (Kind::WmBase, 0) => self.destroy(cid, object),
            (Kind::WmBase, 1) => {
                let id = r.u32();
                self.add(cid, id, Kind::Positioner, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.positioners.insert(id, Positioner::default());
                }
            }
            (Kind::WmBase, 2) => {
                let id = r.u32();
                let surface = r.u32();
                self.add(cid, id, Kind::XdgSurface, surface);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.xdg_to_surface.insert(id, surface);
                    if let Some(s) = client.surfaces.get_mut(&surface) {
                        s.xdg = Some(id);
                    }
                }
            }
            (Kind::WmBase, _) => {}
            (Kind::Positioner, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.positioners.remove(&object);
                }
                self.destroy(cid, object);
            }
            (Kind::Positioner, op) => {
                let a = r.i32();
                let b = r.i32();
                let c = r.i32();
                let d = r.i32();
                if let Some(pos) = self.clients.get_mut(&cid).and_then(|cl| cl.positioners.get_mut(&object)) {
                    match op {
                        1 => pos.size = (a, b),
                        2 => pos.rect = (a, b, c, d),
                        3 => pos.anchor = a as u32,
                        4 => pos.gravity = a as u32,
                        5 => pos.constraint = a as u32,
                        6 => pos.offset = (a, b),
                        _ => {}
                    }
                }
            }
            (Kind::XdgSurface, 0) => self.destroy(cid, object),
            (Kind::XdgSurface, 1) => {
                let id = r.u32();
                self.add(cid, id, Kind::Toplevel, extra);
                if let Some(client) = self.clients.get_mut(&cid) {
                    if let Some(s) = client.surfaces.get_mut(&extra) {
                        s.toplevel = Some(id);
                    }
                }
            }
            (Kind::XdgSurface, 2) => {
                let id = r.u32();
                let parent_xdg = r.u32();
                let positioner = r.u32();
                self.add(cid, id, Kind::Popup, extra);
                self.popup_order += 1;
                let order = self.popup_order;
                if let Some(client) = self.clients.get_mut(&cid) {
                    let parent = client.xdg_to_surface.get(&parent_xdg).copied().unwrap_or(0);
                    let positioner = client.positioners.get(&positioner).copied().unwrap_or_default();
                    if let Some(s) = client.surfaces.get_mut(&extra) {
                        s.popup = Some(PopupRole { object: id, parent, positioner, x: 0, y: 0, grab: false, order, configured: false, done: false });
                        s.closed = false;
                    }
                }
            }
            (Kind::XdgSurface, 3) => {
                let x = r.i32();
                let y = r.i32();
                let w = r.i32();
                let h = r.i32();
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)) {
                    s.geometry = if w > 0 && h > 0 { Some((x, y, w, h)) } else { None };
                }
            }
            (Kind::XdgSurface, _) => {}
            (Kind::Toplevel, 0) => {
                self.unmap(cid, extra);
                if let Some(client) = self.clients.get_mut(&cid) {
                    if let Some(s) = client.surfaces.get_mut(&extra) {
                        s.toplevel = None;
                        s.configured = false;
                    }
                }
                self.destroy(cid, object);
            }
            (Kind::Toplevel, 2) | (Kind::Toplevel, 3) => {
                let text = r.string();
                let window = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)).map(|s| {
                    if opcode == 2 {
                        s.title = text.clone();
                    } else {
                        s.app_id = text.clone();
                    }
                    (s.window.as_ref().map(|w| w.id), s.title.clone())
                });
                if let Some((Some(id), title)) = window {
                    if opcode == 2 && !title.is_empty() {
                        self.request(Request::SetTitle { window: id, title: Title::new(&title) });
                    }
                    if opcode == 3 {
                        self.announce_identity(cid, extra);
                    }
                }
            }
            (Kind::Toplevel, 7) | (Kind::Toplevel, 8) => {
                let w = r.i32().max(0) as u32;
                let h = r.i32().max(0) as u32;
                let hints = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)).map(|s| {
                    if opcode == 7 {
                        s.max = (w, h);
                    } else {
                        s.min = (w, h);
                    }
                    (s.window.as_ref().map(|w| w.id), s.min, s.max)
                });
                if let Some((Some(id), min, max)) = hints {
                    self.request(Request::SizeHints { window: id, min_width: min.0, min_height: min.1, max_width: max.0, max_height: max.1 });
                }
            }
            (Kind::Toplevel, 5) | (Kind::Toplevel, 6) | (Kind::Toplevel, 13) => {
                let id = self.clients.get(&cid).and_then(|c| c.surfaces.get(&extra)).and_then(|s| s.window.as_ref().map(|w| w.id));
                if let Some(id) = id {
                    match opcode {
                        5 => self.request(Request::BeginMove { window: id }),
                        6 => {
                            let _seat = r.u32();
                            let _serial = r.u32();
                            let edges = r.u32();
                            let mut mask = 0u32;
                            if edges & 1 != 0 {
                                mask |= 4;
                            }
                            if edges & 2 != 0 {
                                mask |= 8;
                            }
                            if edges & 4 != 0 {
                                mask |= 1;
                            }
                            if edges & 8 != 0 {
                                mask |= 2;
                            }
                            self.request(Request::BeginResize { window: id, edges: mask });
                        }
                        _ => self.request(Request::Minimize { window: id }),
                    }
                }
            }
            (Kind::Toplevel, 9) | (Kind::Toplevel, 10) => {
                let id = self.clients.get(&cid).and_then(|c| c.surfaces.get(&extra)).and_then(|s| s.window.as_ref().map(|w| w.id));
                if let Some(id) = id {
                    self.request(Request::SetMaximized { window: id, maximized: (opcode == 9) as u32 });
                }
            }
            (Kind::Toplevel, _) => {}
            (Kind::Popup, 0) => {
                self.unmap(cid, extra);
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)) {
                    s.popup = None;
                }
                self.destroy(cid, object);
            }
            (Kind::Popup, 1) => {
                if let Some(role) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)).and_then(|s| s.popup.as_mut()) {
                    role.grab = true;
                }
            }
            (Kind::Popup, 2) => {
                let positioner = r.u32();
                let token = r.u32();
                let found = self.clients.get_mut(&cid).and_then(|c| {
                    let pos = c.positioners.get(&positioner).copied();
                    c.surfaces.get_mut(&extra).and_then(|s| s.popup.as_mut()).map(|role| {
                        if let Some(pos) = pos {
                            role.positioner = pos;
                        }
                    })
                });
                if found.is_some() {
                    self.post(cid, Message::new(object, 2).u32(token).finish());
                    self.configure_popup(cid, extra);
                    let target = self.clients.get(&cid).and_then(|c| c.surfaces.get(&extra)).and_then(|s| Some((s.window.as_ref()?.id, s.popup?)));
                    if let Some((window, role)) = target {
                        let (_, x, y) = self.popup_parent(cid, role.parent, role.x, role.y);
                        self.request(Request::MovePopup { window, x, y });
                    }
                }
            }
            (Kind::Popup, _) => {}
            (Kind::Seat, 0) => {
                let id = r.u32();
                self.add(cid, id, Kind::Pointer, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.pointers.push(id);
                }
            }
            (Kind::Seat, 1) => {
                let id = r.u32();
                self.add(cid, id, Kind::Keyboard, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.keyboards.push(id);
                }
                self.send_keymap(cid, id);
            }
            (Kind::Seat, 2) => {
                let id = r.u32();
                self.add(cid, id, Kind::Touch, 0);
            }
            (Kind::Seat, 3) => self.destroy(cid, object),
            (Kind::Pointer, 1) | (Kind::Keyboard, 0) | (Kind::Touch, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.pointers.retain(|p| *p != object);
                    client.keyboards.retain(|k| *k != object);
                }
                self.destroy(cid, object);
            }
            (Kind::Pointer, _) | (Kind::Keyboard, _) | (Kind::Touch, _) => {}
            (Kind::Output, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.outputs.retain(|o| o.0 != object);
                }
                self.destroy(cid, object);
            }
            (Kind::Viewporter, 0) | (Kind::FractionalScaleManager, 0) | (Kind::FractionalScale, 0) | (Kind::XdgOutputManager, 0) => self.destroy(cid, object),
            (Kind::Viewporter, 1) => {
                let id = r.u32();
                let surface = r.u32();
                self.add(cid, id, Kind::Viewport, surface);
            }
            (Kind::Viewport, 0) => {
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)) {
                    s.viewport_src = None;
                    s.viewport_dst = None;
                }
                self.destroy(cid, object);
            }
            (Kind::Viewport, 1) => {
                let rect = (r.i32(), r.i32(), r.i32(), r.i32());
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)) {
                    s.viewport_src = if rect == (-256, -256, -256, -256) || rect.2 <= 0 || rect.3 <= 0 { None } else { Some(rect) };
                }
            }
            (Kind::Viewport, 2) => {
                let size = (r.i32(), r.i32());
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)) {
                    s.viewport_dst = if size.0 <= 0 || size.1 <= 0 { None } else { Some(size) };
                }
            }
            (Kind::FractionalScaleManager, 1) => {
                let id = r.u32();
                let surface = r.u32();
                self.add(cid, id, Kind::FractionalScale, surface);
                self.post(cid, Message::new(id, 0).u32(120).finish());
            }
            (Kind::XdgOutputManager, 1) => {
                let id = r.u32();
                let output = r.u32();
                self.add(cid, id, Kind::XdgOutput, output);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.xdg_outputs.push((id, output, extra));
                }
                self.advertise_xdg_output(cid, id, output, extra);
            }
            (Kind::XdgOutput, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.xdg_outputs.retain(|o| o.0 != object);
                }
                self.destroy(cid, object);
            }
            (Kind::Subcompositor, 1) => {
                let id = r.u32();
                self.add(cid, id, Kind::Subsurface, 0);
            }
            (Kind::Subcompositor, 0) | (Kind::Subsurface, 0) => self.destroy(cid, object),
            (Kind::DataDeviceManager, 0) => {
                let id = r.u32();
                self.add(cid, id, Kind::DataSource, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.sources.insert(id, Vec::new());
                }
            }
            (Kind::DataDeviceManager, 1) => {
                let id = r.u32();
                self.add(cid, id, Kind::DataDevice, 0);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.data_devices.push(id);
                }
            }
            (Kind::DataSource, 0) => {
                let mime = r.string();
                if let Some(list) = self.clients.get_mut(&cid).and_then(|c| c.sources.get_mut(&object)) {
                    list.push(mime);
                }
            }
            (Kind::DataSource, 1) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.sources.remove(&object);
                }
                if self.clipboard_owner == Some((cid, object)) {
                    self.clipboard_owner = None;
                }
                self.destroy(cid, object);
            }
            (Kind::DataDevice, 1) => {
                let source = r.u32();
                self.take_selection(cid, source);
            }
            (Kind::DataDevice, 2) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.data_devices.retain(|d| *d != object);
                }
                self.destroy(cid, object);
            }
            (Kind::DataOffer, 1) => {
                let _mime = r.string();
                if let Some(fd) = self.take_fd(cid) {
                    if let Some((_, data)) = self.clipboard.as_ref() {
                        let mut done = 0;
                        while done < data.len() {
                            let n = sys::write(fd, &data[done..]);
                            if n <= 0 {
                                break;
                            }
                            done += n as usize;
                        }
                    }
                    sys::close(fd);
                }
            }
            (Kind::DataOffer, 2) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.offers.retain(|o| *o != object);
                }
                self.destroy(cid, object);
            }
            (Kind::DataOffer, _) => {}
            (Kind::DecorationManager, 0) => self.destroy(cid, object),
            (Kind::DecorationManager, 1) => {
                let id = r.u32();
                let toplevel = r.u32();
                let sid = self.clients.get(&cid).and_then(|c| c.objects.get(&toplevel)).map(|&(_, sid)| sid).unwrap_or(0);
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)) {
                    s.decoration = true;
                }
                self.add(cid, id, Kind::Decoration, sid);
                self.post(cid, Message::new(id, 0).u32(2).finish());
            }
            (Kind::Decoration, 0) => self.destroy(cid, object),
            (Kind::Decoration, _) => {
                let mode = if opcode == 1 { r.u32() } else { 2 };
                let mode = if mode == 1 { 1 } else { 2 };
                self.post(cid, Message::new(object, 0).u32(mode).finish());
                self.set_decoration(cid, extra, mode == 1);
            }
            (Kind::ServerDecorationManager, 0) => {
                let id = r.u32();
                self.add(cid, id, Kind::ServerDecoration, 0);
                self.post(cid, Message::new(id, 0).u32(SERVER_DECORATION_MODE).finish());
            }
            (Kind::LayerShell, 0) => {
                let id = r.u32();
                let surface = r.u32();
                let _output = r.u32();
                let layer = r.u32().min(3);
                self.add(cid, id, Kind::LayerSurface, surface);
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&surface)) {
                    s.layer = Some(LayerRole { object: id, layer, anchor: 0, exclusive: 0, margins: [0; 4], size: (0, 0), keyboard: 0, configured: false, dirty: false, sent: (0, 0) });
                }
            }
            (Kind::LayerShell, _) => self.destroy(cid, object),
            (Kind::LayerSurface, 7) => {
                self.unmap(cid, extra);
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&extra)) {
                    s.layer = None;
                }
                self.destroy(cid, object);
            }
            (Kind::LayerSurface, 5) => {
                let popup = r.u32();
                let target = self.clients.get(&cid).and_then(|c| c.objects.get(&popup)).map(|&(_, sid)| sid);
                if let Some(psid) = target {
                    if let Some(role) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&psid)).and_then(|s| s.popup.as_mut()) {
                        role.parent = extra;
                    }
                }
            }
            (Kind::LayerSurface, 6) => {}
            (Kind::LayerSurface, op) => {
                let a = r.i32();
                let b = r.i32();
                let c = r.i32();
                let d = r.i32();
                if let Some(role) = self.clients.get_mut(&cid).and_then(|cl| cl.surfaces.get_mut(&extra)).and_then(|s| s.layer.as_mut()) {
                    match op {
                        0 => role.size = (a.max(0) as u32, b.max(0) as u32),
                        1 => role.anchor = a as u32 & 15,
                        2 => role.exclusive = a,
                        3 => role.margins = [a, b, c, d],
                        4 => role.keyboard = a as u32,
                        8 => role.layer = (a as u32).min(3),
                        _ => {}
                    }
                    role.dirty = true;
                }
            }
            (Kind::HamixShell, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.shells.retain(|s| *s != object);
                }
                self.destroy(cid, object);
            }
            (Kind::ToplevelManager, 0) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.toplevel_managers.retain(|m| *m != object);
                }
                self.post(cid, Message::new(object, 1).finish());
            }
            (Kind::ToplevelHandle, 7) => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.handles.retain(|(h, _)| *h != object);
                }
                self.destroy(cid, object);
            }
            (Kind::ToplevelHandle, op) => {
                let action = match op {
                    0 | 1 => Some(hxproto::ACTION_TOGGLE_MAXIMIZE),
                    2 => Some(hxproto::ACTION_MINIMIZE),
                    3 | 4 => Some(hxproto::ACTION_ACTIVATE),
                    5 => Some(hxproto::ACTION_CLOSE),
                    _ => None,
                };
                let state = self.toplevels.get(&extra).map(|t| t.state).unwrap_or(0);
                let redundant = (op == 0 && state & hxproto::TOPLEVEL_MAXIMIZED != 0) || (op == 1 && state & hxproto::TOPLEVEL_MAXIMIZED == 0);
                if let (Some(action), false) = (action, redundant) {
                    self.request(Request::ToplevelAction { toplevel: extra, action });
                }
            }
            (Kind::ServerDecoration, 0) => self.destroy(cid, object),
            (Kind::ServerDecoration, _) => {
                self.post(cid, Message::new(object, 0).u32(SERVER_DECORATION_MODE).finish());
            }
            _ => {}
        }
    }

    fn bound(&mut self, cid: u32, id: u32, kind: Kind, version: u32) {
        match kind {
            Kind::ServerDecorationManager => {
                self.post(cid, Message::new(id, 0).u32(SERVER_DECORATION_MODE).finish());
            }
            Kind::DecorationManager => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.decoration_manager = true;
                }
            }
            Kind::HamixShell => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.shells.push(id);
                }
                let known: Vec<(u32, u32)> = self.clients.get(&cid).map(|c| c.handles.clone()).unwrap_or_default();
                for (handle, toplevel) in known {
                    self.send_geometry(cid, handle, toplevel);
                }
            }
            Kind::ToplevelManager => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.toplevel_managers.push(id);
                }
                if !self.subscribed {
                    self.subscribed = true;
                    self.request(Request::Hello { version: hxproto::VERSION });
                    self.request(Request::SubscribeToplevels);
                }
                let known: Vec<u32> = self.toplevels.keys().copied().collect();
                for toplevel in known {
                    self.announce_toplevel(cid, id, toplevel);
                }
            }
            Kind::Shm => {
                self.post(cid, Message::new(id, 0).u32(0).finish());
                self.post(cid, Message::new(id, 0).u32(1).finish());
            }
            Kind::Seat => {
                self.post(cid, Message::new(id, 0).u32(3).finish());
                if version >= 2 {
                    self.post(cid, Message::new(id, 1).string("seat0").finish());
                }
            }
            Kind::Output => {
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.outputs.push((id, version));
                }
                self.advertise_output(cid, id, version);
            }
            _ => {}
        }
    }

    fn advertise_output(&mut self, cid: u32, id: u32, version: u32) {
        let (w, h) = self.screen;
        self.post(cid, Message::new(id, 0).i32(0).i32(0).i32((w as i32) * 254 / 960).i32((h as i32) * 254 / 960).i32(0).string("HamixOS").string("Nook").i32(0).finish());
        self.post(cid, Message::new(id, 1).u32(3).i32(w as i32).i32(h as i32).i32(60000).finish());
        if version >= 2 {
            self.post(cid, Message::new(id, 3).i32(1).finish());
        }
        if version >= 4 {
            self.post(cid, Message::new(id, 4).string("NOOK-1").finish());
            self.post(cid, Message::new(id, 5).string("Nook desktop").finish());
        }
        if version >= 2 {
            self.post(cid, Message::new(id, 2).finish());
        }
    }

    fn advertise_xdg_output(&mut self, cid: u32, id: u32, output: u32, version: u32) {
        let (w, h) = self.screen;
        self.post(cid, Message::new(id, 0).i32(0).i32(0).finish());
        self.post(cid, Message::new(id, 1).i32(w as i32).i32(h as i32).finish());
        if version >= 2 {
            self.post(cid, Message::new(id, 3).string("NOOK-1").finish());
            self.post(cid, Message::new(id, 4).string("Nook desktop").finish());
        }
        if version < 3 {
            self.post(cid, Message::new(id, 2).finish());
            return;
        }
        let output_version = self.clients.get(&cid).and_then(|c| c.outputs.iter().find(|o| o.0 == output).map(|o| o.1)).unwrap_or(0);
        if output_version >= 2 {
            self.post(cid, Message::new(output, 2).finish());
        }
    }

    fn screen_changed(&mut self, width: u32, height: u32) {
        if (width, height) == self.screen || width == 0 || height == 0 {
            return;
        }
        self.screen = (width, height);
        let targets: Vec<(u32, u32, u32)> = self.clients.iter().flat_map(|(cid, c)| c.outputs.iter().map(move |(id, v)| (*cid, *id, *v))).collect();
        for (cid, id, version) in targets {
            self.advertise_output(cid, id, version);
        }
        let xdg: Vec<(u32, u32, u32, u32)> = self.clients.iter().flat_map(|(cid, c)| c.xdg_outputs.iter().map(move |(id, o, v)| (*cid, *id, *o, *v))).collect();
        for (cid, id, output, version) in xdg {
            self.advertise_xdg_output(cid, id, output, version);
        }
        println!("hxwayland: screen is now {}x{}", width, height);
    }

    fn send_keymap(&mut self, cid: u32, keyboard: u32) {
        if self.keymap.is_empty() {
            let fd = unix::memfd_create("keymap-empty");
            if fd >= 0 {
                self.post_fd(cid, Message::new(keyboard, 0).u32(0).u32(0).finish(), fd as u64);
                sys::close(fd as u64);
            }
            return;
        }
        let fd = unix::memfd_create("xkb-keymap");
        if fd < 0 {
            return;
        }
        let fd = fd as u64;
        unix::ftruncate(fd, self.keymap.len() as u64);
        unix::pwrite(fd, &self.keymap, 0);
        let size = self.keymap.len() as u32;
        self.post_fd(cid, Message::new(keyboard, 0).u32(1).u32(size).finish(), fd);
        sys::close(fd);
        self.post(cid, Message::new(keyboard, 5).i32(25).i32(500).finish());
    }

    fn surface_request(&mut self, cid: u32, id: u32, opcode: u16, r: &mut Reader) {
        match opcode {
            0 => {
                self.unmap(cid, id);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.surfaces.remove(&id);
                }
                self.destroy(cid, id);
            }
            1 => {
                let buffer = r.u32();
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&id)) {
                    s.pending = if buffer == 0 { None } else { Some(buffer) };
                    s.attached = true;
                }
            }
            3 => {
                let callback = r.u32();
                self.add(cid, callback, Kind::Callback, 0);
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&id)) {
                    s.pending_frames.push(callback);
                }
            }
            2 | 9 => {
                let (x, y, w, h) = (r.i32(), r.i32(), r.i32(), r.i32());
                if w > 0 && h > 0 {
                    if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&id)) {
                        s.damage = Some(match s.damage {
                            Some((ax, ay, aw, ah)) => {
                                let x0 = ax.min(x);
                                let y0 = ay.min(y);
                                let x1 = ax.saturating_add(aw).max(x.saturating_add(w));
                                let y1 = ay.saturating_add(ah).max(y.saturating_add(h));
                                (x0, y0, x1 - x0, y1 - y0)
                            }
                            None => (x, y, w, h),
                        });
                    }
                }
            }
            6 => self.commit(cid, id),
            _ => {}
        }
    }

    fn commit(&mut self, cid: u32, id: u32) {
        let Some(client) = self.clients.get_mut(&cid) else {
            return;
        };
        let Some(surface) = client.surfaces.get_mut(&id) else {
            return;
        };
        let mut pending = core::mem::take(&mut surface.pending_frames);
        surface.frames.append(&mut pending);
        surface.pending_frames = pending;
        let attached = core::mem::replace(&mut surface.attached, false);
        let buffer = if attached { surface.pending.take() } else { None };
        let popup_pending = surface.popup.map(|p| !p.configured).unwrap_or(false);
        if popup_pending {
            let _ = surface;
            self.configure_popup(cid, id);
            if buffer.is_none() {
                return;
            }
        }
        let Some(client) = self.clients.get_mut(&cid) else {
            return;
        };
        let Some(surface) = client.surfaces.get_mut(&id) else {
            return;
        };
        if let Some(layer) = surface.layer.filter(|l| !l.configured || l.dirty) {
            self.configure_layer(cid, id, layer);
            if buffer.is_none() {
                return;
            }
        }
        let Some(client) = self.clients.get_mut(&cid) else {
            return;
        };
        let Some(surface) = client.surfaces.get_mut(&id) else {
            return;
        };
        let is_popup = surface.popup.is_some();
        let role = surface.toplevel.is_some() || is_popup || surface.xwin.is_some() || surface.layer.is_some();
        if surface.toplevel.is_some() && !surface.configured {
            surface.configured = true;
            let toplevel = surface.toplevel.unwrap();
            let xdg = surface.xdg.unwrap_or(0);
            let serial = self.next_serial();
            self.post(cid, Message::new(toplevel, 0).i32(0).i32(0).array(&4u32.to_le_bytes()).finish());
            self.post(cid, Message::new(xdg, 0).u32(serial).finish());
            if buffer.is_none() {
                return;
            }
        }
        if attached && buffer.is_none() {
            self.unmap(cid, id);
            return;
        }
        let Some(buffer_id) = buffer else {
            return;
        };
        if !role && self.xwm.as_ref().and_then(|x| x.cid) == Some(cid) {
            let previous = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&id)).and_then(|s| s.held.replace(buffer_id));
            if let Some(previous) = previous {
                self.post(cid, Message::new(previous, 0).finish());
            }
            return;
        }
        if role && self.show(cid, id, buffer_id) {
            return;
        }
        self.post(cid, Message::new(buffer_id, 0).finish());
        let _ = is_popup;
    }

    fn show(&mut self, cid: u32, sid: u32, buffer_id: u32) -> bool {
        let (width, height, closed, origin, popup, damage, before, scaling) = {
            let Some(client) = self.clients.get_mut(&cid) else {
                return false;
            };
            let Some(buffer) = client.buffers.get(&buffer_id) else {
                return false;
            };
            let (pw, ph) = (buffer.width.max(1), buffer.height.max(1));
            let Some(surface) = client.surfaces.get_mut(&sid) else {
                return false;
            };
            let (bw, bh) = match (surface.viewport_dst, surface.viewport_src) {
                (Some((w, h)), _) => (w, h),
                (None, Some((_, _, w, h))) => ((w >> 8).max(1), (h >> 8).max(1)),
                _ => (pw, ph),
            };
            let scaling = if surface.viewport_dst.is_some() || surface.viewport_src.is_some() {
                let (sx, sy, sw, sh) = surface.viewport_src.unwrap_or((0, 0, pw << 8, ph << 8));
                Some((sx as i64, sy as i64, sw as i64, sh as i64, bw as i64, bh as i64))
            } else {
                None
            };
            let (gx, gy, gw, gh) = match surface.geometry {
                Some((x, y, w, h)) => {
                    let x = x.clamp(0, bw - 1);
                    let y = y.clamp(0, bh - 1);
                    (x, y, w.min(bw - x).max(1), h.min(bh - y).max(1))
                }
                None => (0, 0, bw, bh),
            };
            let closed = surface.closed || surface.popup.map(|p| p.done).unwrap_or(false);
            let damage = surface.damage.take();
            let before = surface.window.as_ref().map(|w| (w.id, w.width, w.height));
            (gw as u32, gh as u32, closed, (gx as usize, gy as usize), surface.popup.is_some(), damage, before, scaling)
        };
        if closed {
            return false;
        }
        let xinfo = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.xwin).and_then(|w| self.xwm_window_info(w));
        let layered = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).map(|s| s.layer.is_some()).unwrap_or(false);
        let popup_window = popup || layered || xinfo.as_ref().map(|i| i.override_redirect).unwrap_or(false);
        if layered {
            self.ensure_layer(cid, sid, width, height);
        }
        match xinfo {
            _ if layered => {}
            Some(info) if info.override_redirect => self.ensure_xpopup(cid, sid, width, height, info.x, info.y),
            Some(info) => {
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)) {
                    if !info.title.is_empty() {
                        s.title = info.title.clone();
                    }
                    s.min = info.min;
                    s.max = info.max;
                }
                self.ensure_window(cid, sid, width, height);
            }
            None if popup => self.ensure_popup(cid, sid, width, height),
            None => self.ensure_window(cid, sid, width, height),
        }
        if self.link.direct() && !layered {
            return self.show_direct(cid, sid, buffer_id, origin, popup_window, damage, before, scaling);
        }
        let Some(client) = self.clients.get_mut(&cid) else {
            return false;
        };
        let Some(buffer) = client.buffers.get(&buffer_id) else {
            return false;
        };
        let Some(pool) = client.pools.get(&buffer.pool) else {
            return false;
        };
        let Some(surface) = client.surfaces.get_mut(&sid) else {
            return false;
        };
        let Some(window) = surface.window.as_mut() else {
            return false;
        };
        if pool.map.ptr.is_null() {
            return false;
        }
        let (keep, force) = if popup_window { if buffer.alpha { (0xFFFF_FFFFu32, 0u32) } else { (0x00FF_FFFF, 0xFF00_0000) } } else { (0x00FF_FFFF, 0) };
        let stride = buffer.stride.max(0) as usize;
        if let Some((sx, sy, sw, sh, lw, lh)) = scaling {
            let (pw, ph) = (buffer.width.max(1) as i64, buffer.height.max(1) as i64);
            let (ww, wh) = (window.width as usize, window.height as usize);
            for y in 0..wh {
                let ly = (y + origin.1) as i64;
                let by = ((sy + ly * sh / lh) >> 8).clamp(0, ph - 1) as usize;
                for x in 0..ww {
                    let lx = (x + origin.0) as i64;
                    let bx = ((sx + lx * sw / lw) >> 8).clamp(0, pw - 1) as usize;
                    let at = buffer.offset + by * stride + bx * 4;
                    if at + 4 > pool.map.size {
                        continue;
                    }
                    unsafe {
                        let pixel = *(pool.map.ptr.add(at) as *const u32);
                        *window.pixels.add(y * ww + x) = (pixel & keep) | force;
                    }
                }
            }
            let id = window.id;
            self.request(Request::Present { window: id, x: 0, y: 0, width: ww as u32, height: wh as u32 });
            return false;
        }
        let rows = (buffer.height.max(0) as usize).saturating_sub(origin.1).min(window.height as usize);
        let cols = (buffer.width.max(0) as usize).saturating_sub(origin.0).min(window.width as usize);
        let whole = before != Some((window.id, window.width, window.height));
        let (x0, y0, x1, y1) = match damage.filter(|_| !whole) {
            Some((dx, dy, dw, dh)) => {
                let x0 = (dx - origin.0 as i32).max(0) as usize;
                let y0 = (dy - origin.1 as i32).max(0) as usize;
                let x1 = (dx.saturating_add(dw) - origin.0 as i32).max(0) as usize;
                let y1 = (dy.saturating_add(dh) - origin.1 as i32).max(0) as usize;
                (x0.min(cols), y0.min(rows), x1.min(cols), y1.min(rows))
            }
            None => (0, 0, cols, rows),
        };
        if x0 >= x1 || y0 >= y1 {
            return false;
        }
        let count = x1 - x0;
        let plain = keep == 0xFFFF_FFFF && force == 0;
        for y in y0..y1 {
            let start = buffer.offset + (y + origin.1) * stride + (x0 + origin.0) * 4;
            if start + count * 4 > pool.map.size {
                break;
            }
            unsafe {
                let src = core::slice::from_raw_parts(pool.map.ptr.add(start) as *const u32, count);
                let dst = core::slice::from_raw_parts_mut(window.pixels.add(y * window.width as usize + x0), count);
                if plain {
                    dst.copy_from_slice(src);
                } else {
                    for (out, pixel) in dst.iter_mut().zip(src.iter()) {
                        *out = (*pixel & keep) | force;
                    }
                }
            }
        }
        let id = window.id;
        self.request(Request::Present { window: id, x: x0 as u32, y: y0 as u32, width: (x1 - x0) as u32, height: (y1 - y0) as u32 });
        false
    }

    fn show_direct(&mut self, cid: u32, sid: u32, buffer_id: u32, origin: (usize, usize), popup: bool, damage: Option<(i32, i32, i32, i32)>, before: Option<(u32, u32, u32)>, scaling: Option<(i64, i64, i64, i64, i64, i64)>) -> bool {
        let Some(client) = self.clients.get_mut(&cid) else {
            return false;
        };
        let Some(buffer) = client.buffers.get(&buffer_id) else {
            return false;
        };
        let Some(pool) = client.pools.get(&buffer.pool) else {
            return false;
        };
        let map = pool.map.clone();
        let (bw, bh, stride, offset, alpha) = (buffer.width.max(0) as usize, buffer.height.max(0) as usize, buffer.stride.max(0) as usize, buffer.offset, buffer.alpha);
        let Some(surface) = client.surfaces.get_mut(&sid) else {
            return false;
        };
        let Some(window) = surface.window.as_ref() else {
            return false;
        };
        let (id, ww, wh) = (window.id, window.width as usize, window.height as usize);
        if map.ptr.is_null() || ww == 0 || wh == 0 || stride % 4 != 0 {
            return false;
        }
        let opaque = popup && !alpha;
        if let Some((sx, sy, sw, sh, lw, lh)) = scaling {
            let (pw, ph) = (bw.max(1) as i64, bh.max(1) as i64);
            surface.scratch.resize(ww * wh, 0);
            for y in 0..wh {
                let ly = (y + origin.1) as i64;
                let by = ((sy + ly * sh / lh) >> 8).clamp(0, ph - 1) as usize;
                for x in 0..ww {
                    let lx = (x + origin.0) as i64;
                    let bx = ((sx + lx * sw / lw) >> 8).clamp(0, pw - 1) as usize;
                    let at = offset + by * stride + bx * 4;
                    if at + 4 <= map.size {
                        surface.scratch[y * ww + x] = unsafe { *(map.ptr.add(at) as *const u32) };
                    }
                }
            }
            let pixels = surface.scratch.as_ptr();
            surface.shown = None;
            self.link.attach(Attach { window: id, pixels, stride: ww as u32, width: ww as u32, height: wh as u32, opaque, damage: (0, 0, ww as u32, wh as u32) });
            return false;
        }
        let rows = bh.saturating_sub(origin.1).min(wh);
        let cols = bw.saturating_sub(origin.0).min(ww);
        if rows == 0 || cols == 0 {
            return false;
        }
        let start = offset + origin.1 * stride + origin.0 * 4;
        let end = start + (rows - 1) * stride + cols * 4;
        if end > map.size {
            return false;
        }
        let whole = before != Some((id, ww as u32, wh as u32));
        let area = match damage.filter(|_| !whole) {
            Some((dx, dy, dw, dh)) => {
                let x0 = (dx - origin.0 as i32).max(0).min(cols as i32) as u32;
                let y0 = (dy - origin.1 as i32).max(0).min(rows as i32) as u32;
                let x1 = (dx.saturating_add(dw) - origin.0 as i32).max(0).min(cols as i32) as u32;
                let y1 = (dy.saturating_add(dh) - origin.1 as i32).max(0).min(rows as i32) as u32;
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            None => (0, 0, cols as u32, rows as u32),
        };
        let pixels = unsafe { map.ptr.add(start) as *const u32 };
        let previous = surface.shown.replace((buffer_id, map));
        self.link.attach(Attach { window: id, pixels, stride: (stride / 4) as u32, width: cols as u32, height: rows as u32, opaque, damage: area });
        if let Some((old, _)) = previous {
            if old != buffer_id {
                self.post(cid, Message::new(old, 0).finish());
            }
        }
        true
    }

    fn announce_identity(&mut self, cid: u32, sid: u32) {
        let Some(client) = self.clients.get(&cid) else {
            return;
        };
        let Some(surface) = client.surfaces.get(&sid) else {
            return;
        };
        if let Some(xwin) = surface.xwin {
            self.xwm_sync_identity(xwin);
            return;
        }
        let Some(window) = surface.window.as_ref().map(|w| w.id) else {
            return;
        };
        if surface.app_id.is_empty() && client.pid == 0 {
            return;
        }
        let app_id = Title::new(&surface.app_id);
        let pid = client.pid;
        self.request(Request::SetAppId { window, pid, app_id });
    }

    fn ensure_window(&mut self, cid: u32, sid: u32, width: u32, height: u32) {
        let (existing, title) = {
            let Some(surface) = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)) else {
                return;
            };
            let title = if !surface.title.is_empty() { surface.title.clone() } else if !surface.app_id.is_empty() { surface.app_id.clone() } else { String::from("Wayland") };
            (surface.window.as_ref().map(|w| (w.id, w.shm, w.capacity, w.width, w.height)), title)
        };
        match existing {
            Some((_, _, _, w, h)) if (w, h) == (width, height) => {}
            Some(_) if self.link.direct() => {
                if let Some(w) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.as_mut()) {
                    w.width = width;
                    w.height = height;
                }
            }
            Some((window, shm, capacity, _, _)) => {
                let bytes = width as usize * height as usize * 4;
                let (new_shm, pixels, cap) = if bytes <= capacity {
                    (shm, self.window_pixels(cid, sid), capacity)
                } else {
                    let Some((id, ptr)) = self.window_buffer(bytes) else {
                        return;
                    };
                    (id, ptr, bytes)
                };
                self.request(Request::Attach { window, shm: new_shm, width, height });
                if new_shm != shm {
                    release_shm(shm);
                }
                if let Some(w) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.as_mut()) {
                    w.shm = new_shm;
                    w.pixels = pixels;
                    w.capacity = cap;
                    w.width = width;
                    w.height = height;
                }
            }
            None => {
                let bytes = width as usize * height as usize * 4;
                let Some((shm, pixels)) = self.window_buffer(bytes) else {
                    return;
                };
                self.request(Request::CreateWindow { shm, width, height, title: Title::new(&title) });
                let Some(window) = self.wait_created() else {
                    release_shm(shm);
                    return;
                };
                self.windows.insert(window, (cid, sid));
                let manager = self.clients.get(&cid).map(|c| c.decoration_manager).unwrap_or(false);
                let hints = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).map(|s| {
                    s.window = Some(Window { id: window, shm, pixels, capacity: bytes, width, height });
                    if manager && !s.decoration && s.toplevel.is_some() {
                        s.csd = true;
                    }
                    (s.min, s.max, s.csd)
                });
                if let Some((_, _, true)) = hints {
                    self.request(Request::SetFrame { window, decorated: 0 });
                }
                self.request(Request::TrackPosition { window });
                let hints = hints.map(|(min, max, _)| (min, max));
                if let Some((min, max)) = hints {
                    if min != (0, 0) || max != (0, 0) {
                        self.request(Request::SizeHints { window, min_width: min.0, min_height: min.1, max_width: max.0, max_height: max.1 });
                    }
                }
                self.announce_identity(cid, sid);
            }
        }
    }

    fn set_decoration(&mut self, cid: u32, sid: u32, csd: bool) {
        let state = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).map(|s| {
            let changed = s.csd != csd;
            s.csd = csd;
            (changed, s.window.as_ref().map(|w| w.id), s.toplevel, s.xdg, s.configured)
        });
        let Some((changed, window, toplevel, xdg, configured)) = state else {
            return;
        };
        if let (true, Some(window)) = (changed, window) {
            self.request(Request::SetFrame { window, decorated: (!csd) as u32 });
        }
        if let (true, Some(toplevel), Some(xdg)) = (configured, toplevel, xdg) {
            let serial = self.next_serial();
            self.post(cid, Message::new(toplevel, 0).i32(0).i32(0).array(&4u32.to_le_bytes()).finish());
            self.post(cid, Message::new(xdg, 0).u32(serial).finish());
        }
    }

    fn window_xwin(&self, window: u32) -> Option<u32> {
        let &(cid, sid) = self.windows.get(&window)?;
        self.clients.get(&cid)?.surfaces.get(&sid)?.xwin
    }

    fn surface_origin(&self, cid: u32, sid: u32) -> Option<(i32, i32)> {
        let window = self.clients.get(&cid)?.surfaces.get(&sid)?.window.as_ref()?.id;
        self.placed.get(&window).copied()
    }

    fn configure_popup(&mut self, cid: u32, sid: u32) {
        let Some((role, parent)) = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.popup).map(|r| (r, r.parent)) else {
            return;
        };
        let pos = role.positioner;
        let (mut w, mut h) = (pos.size.0.max(1), pos.size.1.max(1));
        let (sw, sh) = (self.screen.0 as i32, self.screen.1 as i32);
        if pos.constraint & 32 != 0 && h > sh - PANEL_H {
            h = sh - PANEL_H;
        }
        if pos.constraint & 16 != 0 && w > sw {
            w = sw;
        }
        let place = move |anchor: u32, gravity: u32| -> (i32, i32) {
            let (ax, ay, aw, ah) = pos.rect;
            let px = match anchor {
                3 | 5 | 6 => ax,
                4 | 7 | 8 => ax + aw,
                _ => ax + aw / 2,
            };
            let py = match anchor {
                1 | 5 | 7 => ay,
                2 | 6 | 8 => ay + ah,
                _ => ay + ah / 2,
            };
            let x = match gravity {
                3 | 5 | 6 => px - w,
                4 | 7 | 8 => px,
                _ => px - w / 2,
            };
            let y = match gravity {
                1 | 5 | 7 => py - h,
                2 | 6 | 8 => py,
                _ => py - h / 2,
            };
            (x + pos.offset.0, y + pos.offset.1)
        };
        let flip_v = |v: u32| match v {
            1 => 2,
            2 => 1,
            5 => 6,
            6 => 5,
            7 => 8,
            8 => 7,
            o => o,
        };
        let flip_h = |v: u32| match v {
            3 => 4,
            4 => 3,
            5 => 7,
            7 => 5,
            6 => 8,
            8 => 6,
            o => o,
        };
        let (mut x, mut y) = place(pos.anchor, pos.gravity);
        let origin = self.surface_origin(cid, parent);
        if let Some((ox, oy)) = origin {
            if pos.constraint & 8 != 0 && (oy + y + h > sh || oy + y < PANEL_H) {
                let (_, fy) = place(flip_v(pos.anchor), flip_v(pos.gravity));
                if oy + fy >= PANEL_H && oy + fy + h <= sh {
                    y = fy;
                }
            }
            if pos.constraint & 4 != 0 && (ox + x + w > sw || ox + x < 0) {
                let (fx, _) = place(flip_h(pos.anchor), flip_h(pos.gravity));
                if ox + fx >= 0 && ox + fx + w <= sw {
                    x = fx;
                }
            }
            if ox + x + w > sw {
                x = sw - w - ox;
            }
            if ox + x < 0 {
                x = -ox;
            }
            if oy + y + h > sh {
                y = sh - h - oy;
            }
            if oy + y < PANEL_H {
                y = PANEL_H - oy;
            }
        }
        let xdg = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| {
            let role = s.popup.as_mut()?;
            role.x = x;
            role.y = y;
            role.configured = true;
            s.xdg
        });
        let serial = self.next_serial();
        self.post(cid, Message::new(role.object, 0).i32(x).i32(y).i32(w).i32(h).finish());
        if let Some(xdg) = xdg {
            self.post(cid, Message::new(xdg, 0).u32(serial).finish());
        }
    }

    fn ensure_popup(&mut self, cid: u32, sid: u32, width: u32, height: u32) {
        let Some((existing, role)) = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| Some((s.window.as_ref().map(|w| (w.id, w.width, w.height)), s.popup?))) else {
            return;
        };
        if let Some((window, w, h)) = existing {
            if (w, h) == (width, height) {
                return;
            }
            let old = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.take());
            self.request(Request::Destroy { window });
            if let Some(old) = old {
                release_shm(old.shm);
            }
            self.windows.remove(&window);
            self.placed.remove(&window);
        }
        let (parent_window, px, py) = self.popup_parent(cid, role.parent, role.x, role.y);
        let bytes = width as usize * height as usize * 4;
        let Some((shm, pixels)) = self.window_buffer(bytes) else {
            return;
        };
        self.request(Request::CreatePopup { shm, width, height, parent: parent_window, x: px, y: py });
        let Some(window) = self.wait_created() else {
            release_shm(shm);
            return;
        };
        self.request(Request::TrackPosition { window });
        self.windows.insert(window, (cid, sid));
        if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)) {
            s.window = Some(Window { id: window, shm, pixels, capacity: bytes, width, height });
        }
    }

    fn ensure_xpopup(&mut self, cid: u32, sid: u32, width: u32, height: u32, x: i32, y: i32) {
        let existing = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.window.as_ref().map(|w| (w.id, w.width, w.height)));
        if let Some((window, w, h)) = existing {
            if (w, h) == (width, height) {
                return;
            }
            let old = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.take());
            self.request(Request::Destroy { window });
            if let Some(old) = old {
                release_shm(old.shm);
            }
            self.windows.remove(&window);
        }
        let bytes = width as usize * height as usize * 4;
        let Some((shm, pixels)) = self.window_buffer(bytes) else {
            return;
        };
        self.request(Request::CreatePopup { shm, width, height, parent: 0, x, y });
        let Some(window) = self.wait_created() else {
            release_shm(shm);
            return;
        };
        self.windows.insert(window, (cid, sid));
        if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)) {
            s.window = Some(Window { id: window, shm, pixels, capacity: bytes, width, height });
        }
    }

    fn layer_size(&self, role: &LayerRole) -> (u32, u32) {
        let (sw, sh) = (self.screen.0 as i32, self.screen.1 as i32);
        let [top, right, bottom, left] = role.margins;
        let w = if role.size.0 == 0 && role.anchor & 12 == 12 { (sw - left - right).max(1) as u32 } else { role.size.0.max(1) };
        let h = if role.size.1 == 0 && role.anchor & 3 == 3 { (sh - top - bottom).max(1) as u32 } else { role.size.1.max(1) };
        (w, h)
    }

    fn configure_layer(&mut self, cid: u32, sid: u32, role: LayerRole) {
        let size = self.layer_size(&role);
        let window = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.window.as_ref()).map(|w| w.id);
        if let (Some(window), true) = (window, role.configured) {
            self.request(Request::ConfigureLayer { window, anchor: role.anchor, exclusive: role.exclusive, margins: role.margins });
        }
        if let Some(r) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.layer.as_mut()) {
            r.configured = true;
            r.dirty = false;
            if r.sent == size {
                return;
            }
            r.sent = size;
        }
        let serial = self.next_serial();
        self.post(cid, Message::new(role.object, 0).u32(serial).u32(size.0).u32(size.1).finish());
    }

    fn ensure_layer(&mut self, cid: u32, sid: u32, width: u32, height: u32) {
        let Some((existing, role)) = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| Some((s.window.as_ref().map(|w| (w.id, w.shm, w.capacity, w.width, w.height)), s.layer?))) else {
            return;
        };
        match existing {
            Some((_, _, _, w, h)) if (w, h) == (width, height) => {}
            Some((window, shm, capacity, _, _)) => {
                let bytes = width as usize * height as usize * 4;
                let (new_shm, pixels, cap) = if bytes <= capacity {
                    (shm, self.window_pixels(cid, sid), capacity)
                } else {
                    let Some((id, ptr)) = alloc_buffer(bytes) else {
                        return;
                    };
                    (id, ptr, bytes)
                };
                self.request(Request::Attach { window, shm: new_shm, width, height });
                if new_shm != shm {
                    release_shm(shm);
                }
                if let Some(w) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.as_mut()) {
                    w.shm = new_shm;
                    w.pixels = pixels;
                    w.capacity = cap;
                    w.width = width;
                    w.height = height;
                }
            }
            None => {
                let bytes = width as usize * height as usize * 4;
                let Some((shm, pixels)) = alloc_buffer(bytes) else {
                    return;
                };
                self.request(Request::CreateLayer { shm, width, height, layer: role.layer, anchor: role.anchor, exclusive: role.exclusive, margins: role.margins, keyboard: (role.keyboard != 0) as u32 });
                let Some(window) = self.wait_created() else {
                    release_shm(shm);
                    return;
                };
                self.windows.insert(window, (cid, sid));
                if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)) {
                    s.window = Some(Window { id: window, shm, pixels, capacity: bytes, width, height });
                }
            }
        }
    }

    fn popup_parent(&self, cid: u32, parent_sid: u32, x: i32, y: i32) -> (u32, i32, i32) {
        let Some(surface) = self.clients.get(&cid).and_then(|c| c.surfaces.get(&parent_sid)) else {
            return (0, x, y);
        };
        let window = surface.window.as_ref().map(|w| w.id).unwrap_or(0);
        if surface.layer.is_some() {
            let (ox, oy) = self.placed.get(&window).copied().unwrap_or((0, 0));
            return (0, ox + x, oy + y);
        }
        (window, x, y)
    }

    fn toplevel_states(state: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for (bit, value) in [(hxproto::TOPLEVEL_MAXIMIZED, 0u32), (hxproto::TOPLEVEL_MINIMIZED, 1), (hxproto::TOPLEVEL_ACTIVE, 2)] {
            if state & bit != 0 {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out
    }

    fn describe_toplevel(&mut self, cid: u32, handle: u32, toplevel: u32) {
        let Some(t) = self.toplevels.get(&toplevel).cloned() else {
            return;
        };
        self.post(cid, Message::new(handle, 0).string(&t.title).finish());
        self.post(cid, Message::new(handle, 1).string(&t.app_id).finish());
        self.post(cid, Message::new(handle, 4).array(&Self::toplevel_states(t.state)).finish());
        self.post(cid, Message::new(handle, 5).finish());
    }

    fn announce_toplevel(&mut self, cid: u32, manager: u32, toplevel: u32) {
        let Some(client) = self.clients.get_mut(&cid) else {
            return;
        };
        let handle = client.next_server_id;
        client.next_server_id += 1;
        client.handles.push((handle, toplevel));
        client.objects.insert(handle, (Kind::ToplevelHandle, toplevel));
        let outputs: Vec<u32> = client.outputs.iter().map(|o| o.0).collect();
        self.post(cid, Message::new(manager, 0).u32(handle).finish());
        for output in outputs {
            self.post(cid, Message::new(handle, 2).u32(output).finish());
        }
        self.describe_toplevel(cid, handle, toplevel);
        self.send_geometry(cid, handle, toplevel);
    }

    fn send_geometry(&mut self, cid: u32, handle: u32, toplevel: u32) {
        let Some(&(x, y, w, h)) = self.geometry.get(&toplevel) else {
            return;
        };
        let shells = self.clients.get(&cid).map(|c| c.shells.clone()).unwrap_or_default();
        for shell in shells {
            self.post(cid, Message::new(shell, 0).u32(handle).i32(x).i32(y).i32(w).i32(h).finish());
        }
    }

    fn geometry_changed(&mut self, toplevel: u32, area: (i32, i32, i32, i32)) {
        self.geometry.insert(toplevel, area);
        let targets: Vec<(u32, Vec<u32>)> = self.clients.iter().filter(|(_, c)| !c.shells.is_empty()).map(|(cid, c)| (*cid, c.handles.iter().filter(|(_, t)| *t == toplevel).map(|(h, _)| *h).collect())).collect();
        for (cid, handles) in targets {
            for handle in handles {
                self.send_geometry(cid, handle, toplevel);
            }
        }
    }

    fn toplevel_changed(&mut self, toplevel: u32, state: u32, title: String, app_id: String) {
        let fresh = !self.toplevels.contains_key(&toplevel);
        self.toplevels.insert(toplevel, ForeignToplevel { state, title, app_id });
        let targets: Vec<(u32, Vec<u32>, Vec<u32>)> = self
            .clients
            .iter()
            .filter(|(_, c)| !c.toplevel_managers.is_empty())
            .map(|(cid, c)| (*cid, c.toplevel_managers.clone(), c.handles.iter().filter(|(_, t)| *t == toplevel).map(|(h, _)| *h).collect()))
            .collect();
        for (cid, managers, handles) in targets {
            if fresh || handles.is_empty() {
                for manager in managers {
                    self.announce_toplevel(cid, manager, toplevel);
                }
            } else {
                for handle in handles {
                    self.describe_toplevel(cid, handle, toplevel);
                }
            }
        }
    }

    fn toplevel_closed(&mut self, toplevel: u32) {
        self.toplevels.remove(&toplevel);
        let targets: Vec<(u32, Vec<u32>)> = self.clients.iter().map(|(cid, c)| (*cid, c.handles.iter().filter(|(_, t)| *t == toplevel).map(|(h, _)| *h).collect())).collect();
        for (cid, handles) in targets {
            for handle in handles {
                self.post(cid, Message::new(handle, 6).finish());
            }
        }
    }

    fn window_buffer(&self, bytes: usize) -> Option<(u32, *mut u32)> {
        if self.link.direct() {
            return Some((0, core::ptr::null_mut()));
        }
        alloc_buffer(bytes)
    }

    fn window_pixels(&self, cid: u32, sid: u32) -> *mut u32 {
        self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.window.as_ref()).map(|w| w.pixels).unwrap_or(core::ptr::null_mut())
    }

    fn wait_created(&mut self) -> Option<u32> {
        for _ in 0..100 {
            match self.link.receive(50) {
                Some(Event::Created { window }) => return Some(window),
                Some(Event::Rejected { .. }) => return None,
                Some(other) => self.backlog.push_back(other),
                None => {
                    if !self.link.alive() {
                        return None;
                    }
                }
            }
        }
        None
    }

    fn unmap(&mut self, cid: u32, sid: u32) {
        let window = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.take());
        if let Some(window) = window {
            self.request(Request::Destroy { window: window.id });
            release_shm(window.shm);
            self.windows.remove(&window.id);
            self.placed.remove(&window.id);
        }
        let shown = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.shown.take());
        if let Some((buffer, _)) = shown {
            if self.clients.get(&cid).map(|c| c.buffers.contains_key(&buffer)).unwrap_or(false) {
                self.post(cid, Message::new(buffer, 0).finish());
            }
        }
        if self.pointer_focus.map(|(c, s)| c == cid && s == sid).unwrap_or(false) {
            self.pointer_focus = None;
        }
        if self.keyboard_focus.map(|(c, s)| c == cid && s == sid).unwrap_or(false) {
            self.keyboard_focus = None;
        }
    }

    fn surface_visible(
        surface: &Surface,
        placed: &BTreeMap<u32, (i32, i32)>,
        toplevels: &BTreeMap<u32, ForeignToplevel>,
        screen: (u32, u32),
    ) -> bool {
        let Some(window) = surface.window.as_ref() else {
            return false;
        };
        if let Some(toplevel) = toplevels.get(&window.id) {
            if toplevel.state & hxproto::TOPLEVEL_MINIMIZED != 0 {
                return false;
            }
        }
        let Some(&(x, y)) = placed.get(&window.id) else {
            return false;
        };
        let (w, h) = (window.width.min(i32::MAX as u32) as i32, window.height.min(i32::MAX as u32) as i32);
        w > 0 && h > 0 && x < screen.0 as i32 && y < screen.1 as i32 && x.saturating_add(w) > 0 && y.saturating_add(h) > 0
    }

    fn flush_frames(&mut self) {
        let now = sys::uptime_ms();
        if now.saturating_sub(self.last_frame) < FRAME_MS {
            return;
        }
        self.last_frame = now;
        let time = self.now();
        let placed = &self.placed;
        let toplevels = &self.toplevels;
        let screen = self.screen;
        let mut callbacks: Vec<(u32, u32)> = Vec::new();
        for (cid, client) in self.clients.iter_mut() {
            for surface in client.surfaces.values_mut() {
                if Self::surface_visible(surface, placed, toplevels, screen) {
                    callbacks.extend(core::mem::take(&mut surface.frames).into_iter().map(|callback| (*cid, callback)));
                }
            }
        }
        for (cid, callback) in callbacks {
            self.post(cid, Message::new(callback, 0).u32(time).finish());
            self.destroy(cid, callback);
        }
    }

    fn frames_waiting(&self) -> bool {
        let placed = &self.placed;
        let toplevels = &self.toplevels;
        let screen = self.screen;
        self.clients.values().any(|client| {
            client.surfaces.values().any(|surface| !surface.frames.is_empty() && Self::surface_visible(surface, placed, toplevels, screen))
        })
    }

    fn server_event(&mut self, event: Event) {
        match event {
            Event::Mouse { window, x, y, kind, wheel, .. } => self.pointer_event(window, x, y, kind, wheel),
            Event::Key { window, code, mods } => self.key_event(window, code, mods),
            Event::Focus { window, focused } => {
                let Some(&(cid, sid)) = self.windows.get(&window) else {
                    return;
                };
                let ids = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).map(|s| (s.toplevel, s.xdg));
                if let Some((Some(toplevel), Some(xdg))) = ids {
                    let serial = self.next_serial();
                    let states: Vec<u8> = if focused { 4u32.to_le_bytes().to_vec() } else { Vec::new() };
                    self.post(cid, Message::new(toplevel, 0).i32(0).i32(0).array(&states).finish());
                    self.post(cid, Message::new(xdg, 0).u32(serial).finish());
                }
                if focused {
                    if let Some(xwin) = self.window_xwin(window) {
                        self.xwm_focus(xwin);
                    }
                    self.keyboard_enter(cid, sid);
                } else if self.keyboard_focus == Some((cid, sid)) {
                    self.keyboard_leave();
                }
            }
            Event::Resize { window, width, height } => {
                let Some(&(cid, sid)) = self.windows.get(&window) else {
                    return;
                };
                if let Some(xwin) = self.window_xwin(window) {
                    self.xwm_resize(xwin, width, height);
                    return;
                }
                let ids = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).map(|s| (s.toplevel, s.xdg));
                if let Some((Some(toplevel), Some(xdg))) = ids {
                    let serial = self.next_serial();
                    let states: Vec<u8> = 4u32.to_le_bytes().to_vec();
                    self.post(cid, Message::new(toplevel, 0).i32(width as i32).i32(height as i32).array(&states).finish());
                    self.post(cid, Message::new(xdg, 0).u32(serial).finish());
                }
            }
            Event::Screen { width, height } => {
                self.screen_changed(width, height);
            }
            Event::ToplevelState { toplevel, state, title, app_id, .. } => {
                self.toplevel_changed(toplevel, state, String::from(title.as_str()), String::from(app_id.as_str()));
            }
            Event::ToplevelClosed { toplevel } => {
                self.geometry.remove(&toplevel);
                self.toplevel_closed(toplevel);
            }
            Event::ToplevelGeometry { toplevel, x, y, width, height } => self.geometry_changed(toplevel, (x, y, width, height)),
            Event::LayerConfigured { window, x, y, width, height } => {
                self.placed.insert(window, (x, y));
                let Some(&(cid, sid)) = self.windows.get(&window) else {
                    return;
                };
                let role = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.layer);
                if let Some(role) = role {
                    if role.sent != (width, height) {
                        if let Some(r) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.layer.as_mut()) {
                            r.sent = (width, height);
                        }
                        let serial = self.next_serial();
                        self.post(cid, Message::new(role.object, 0).u32(serial).u32(width).u32(height).finish());
                    }
                }
            }
            Event::Placed { window, x, y } => {
                self.placed.insert(window, (x, y));
                if let Some(xwin) = self.window_xwin(window) {
                    self.xwm_placed(xwin, x, y);
                }
            }
            Event::Close { window } => {
                if let Some(xwin) = self.window_xwin(window) {
                    let (cid, sid) = self.windows[&window];
                    let popup = self.xwm_window_info(xwin).map(|w| w.override_redirect).unwrap_or(false);
                    if !popup {
                        self.xwm_close(xwin);
                    }
                    self.windows.remove(&window);
                    self.placed.remove(&window);
                    if let Some(w) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| s.window.take()) {
                        release_shm(w.shm);
                    }
                    return;
                }
                let Some((cid, sid)) = self.windows.remove(&window) else {
                    return;
                };
                self.placed.remove(&window);
                let popup = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| {
                    let role = s.popup.as_mut()?;
                    role.done = true;
                    let object = role.object;
                    if let Some(w) = s.window.take() {
                        release_shm(w.shm);
                    }
                    Some(object)
                });
                if let Some(object) = popup {
                    self.post(cid, Message::new(object, 1).finish());
                    if self.pointer_focus == Some((cid, sid)) {
                        self.pointer_focus = None;
                    }
                    if self.keyboard_focus == Some((cid, sid)) {
                        self.keyboard_leave();
                    }
                    return;
                }
                let toplevel = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| {
                    if let Some(w) = s.window.take() {
                        release_shm(w.shm);
                    }
                    s.closed = true;
                    s.toplevel
                });
                if let Some(toplevel) = toplevel {
                    self.post(cid, Message::new(toplevel, 1).finish());
                }
                if self.pointer_focus == Some((cid, sid)) {
                    self.pointer_focus = None;
                }
                if self.keyboard_focus == Some((cid, sid)) {
                    self.keyboard_focus = None;
                }
            }
            _ => {}
        }
    }

    fn pointer_event(&mut self, window: u32, x: i32, y: i32, kind: u32, wheel: i32) {
        let Some(&(cid, sid)) = self.windows.get(&window) else {
            return;
        };
        let (gx, gy) = self.clients.get(&cid).and_then(|c| c.surfaces.get(&sid)).and_then(|s| s.geometry).map(|g| (g.0, g.1)).unwrap_or((0, 0));
        let (x, y) = if kind == hxproto::MOUSE_LEAVE { (x, y) } else { (x + gx, y + gy) };
        let pointers = self.clients.get(&cid).map(|c| c.pointers.clone()).unwrap_or_default();
        if kind == hxproto::MOUSE_LEAVE {
            if self.pointer_focus == Some((cid, sid)) {
                let serial = self.next_serial();
                for p in pointers.iter() {
                    self.post(cid, Message::new(*p, 1).u32(serial).u32(sid).finish());
                    self.post(cid, Message::new(*p, 5).finish());
                }
                self.pointer_focus = None;
            }
            return;
        }
        if self.pointer_focus != Some((cid, sid)) {
            if let Some((old_cid, old_sid)) = self.pointer_focus {
                let old = self.clients.get(&old_cid).map(|c| c.pointers.clone()).unwrap_or_default();
                let serial = self.next_serial();
                for p in old {
                    self.post(old_cid, Message::new(p, 1).u32(serial).u32(old_sid).finish());
                    self.post(old_cid, Message::new(p, 5).finish());
                }
            }
            let serial = self.next_serial();
            for p in pointers.iter() {
                self.post(cid, Message::new(*p, 0).u32(serial).u32(sid).fixed(x).fixed(y).finish());
            }
            self.pointer_focus = Some((cid, sid));
        }
        let time = self.now();
        for p in pointers.iter() {
            match kind {
                hxproto::MOUSE_MOVE => {
                    self.post(cid, Message::new(*p, 2).u32(time).fixed(x).fixed(y).finish());
                }
                hxproto::MOUSE_PRESS | hxproto::MOUSE_RELEASE => {
                    let button = match wheel {
                        2 => 0x111,
                        4 => 0x112,
                        _ => 0x110,
                    };
                    let serial = self.next_serial();
                    self.post(cid, Message::new(*p, 2).u32(time).fixed(x).fixed(y).finish());
                    self.post(cid, Message::new(*p, 3).u32(serial).u32(time).u32(button).u32((kind == hxproto::MOUSE_PRESS) as u32).finish());
                }
                hxproto::MOUSE_WHEEL => {
                    self.post(cid, Message::new(*p, 4).u32(time).u32(0).fixed(wheel * 10).finish());
                }
                _ => {}
            }
            self.post(cid, Message::new(*p, 5).finish());
        }
    }

    fn text_mime(mimes: &[String]) -> Option<String> {
        ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"].iter().find(|m| mimes.iter().any(|x| x == *m)).map(|m| String::from(*m)).or_else(|| mimes.first().cloned())
    }

    fn take_selection(&mut self, cid: u32, source: u32) {
        if let Some((ocid, osrc)) = self.clipboard_owner.take() {
            if (ocid, osrc) != (cid, source) {
                self.post(ocid, Message::new(osrc, 2).finish());
            }
        }
        if source == 0 {
            return;
        }
        let Some(mimes) = self.clients.get(&cid).and_then(|c| c.sources.get(&source)).cloned() else {
            return;
        };
        let Some(mime) = Self::text_mime(&mimes) else {
            return;
        };
        let Ok((read_end, write_end)) = sys::pipe_flags(0) else {
            return;
        };
        self.post_fd(cid, Message::new(source, 1).string(&mime).finish(), write_end);
        sys::close(write_end);
        sys::set_nonblocking(read_end);
        let mut data = Vec::new();
        let deadline = sys::uptime_ms() + 1000;
        let mut buf = [0u8; 4096];
        loop {
            let n = sys::read(read_end, &mut buf);
            if n > 0 {
                data.extend_from_slice(&buf[..n as usize]);
                if data.len() > 1 << 20 {
                    break;
                }
                continue;
            }
            if n == 0 || sys::uptime_ms() > deadline {
                break;
            }
            sys::sleep_ms(2);
        }
        sys::close(read_end);
        self.clipboard_owner = Some((cid, source));
        let text = String::from_utf8_lossy(&data).into_owned();
        self.request(Request::SetSelection { mime: Title::new("text/plain;charset=utf-8"), data: hxproto::Text::new(&text) });
        self.clipboard = Some((String::from("text/plain;charset=utf-8"), data));
    }

    fn refresh_clipboard(&mut self) {
        self.request(Request::GetSelection { mime: Title::new("") });
        let deadline = sys::uptime_ms() + 300;
        while sys::uptime_ms() < deadline {
            let Some(event) = self.link.receive(50) else {
                continue;
            };
            match Some(event) {
                Some(Event::Selection { mime, data }) => {
                    if !mime.as_str().is_empty() && self.clipboard_owner.is_none() {
                        self.clipboard = Some((String::from(mime.as_str()), data.as_str().as_bytes().to_vec()));
                    }
                    return;
                }
                Some(other) => self.backlog.push_back(other),
                None => {}
            }
        }
    }

    fn offer_selection(&mut self, cid: u32) {
        let devices = self.clients.get(&cid).map(|c| c.data_devices.clone()).unwrap_or_default();
        if devices.is_empty() {
            return;
        }
        if self.clipboard_owner.map(|(c, _)| c != cid).unwrap_or(true) {
            self.refresh_clipboard();
        }
        for device in devices {
            if self.clipboard.is_none() {
                self.post(cid, Message::new(device, 5).u32(0).finish());
                continue;
            }
            let Some(client) = self.clients.get_mut(&cid) else {
                return;
            };
            let offer = client.next_server_id;
            client.next_server_id += 1;
            client.offers.push(offer);
            client.objects.insert(offer, (Kind::DataOffer, 0));
            self.post(cid, Message::new(device, 0).u32(offer).finish());
            for mime in ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "STRING", "TEXT"] {
                self.post(cid, Message::new(offer, 0).string(mime).finish());
            }
            self.post(cid, Message::new(device, 5).u32(offer).finish());
        }
    }

    fn keyboard_enter(&mut self, cid: u32, sid: u32) {
        if self.keyboard_focus == Some((cid, sid)) {
            return;
        }
        self.offer_selection(cid);
        self.keyboard_leave();
        let keyboards = self.clients.get(&cid).map(|c| c.keyboards.clone()).unwrap_or_default();
        let serial = self.next_serial();
        for k in keyboards {
            self.post(cid, Message::new(k, 1).u32(serial).u32(sid).array(&[]).finish());
            self.post(cid, Message::new(k, 4).u32(serial).u32(0).u32(0).u32(0).u32(0).finish());
        }
        self.keyboard_focus = Some((cid, sid));
    }

    fn keyboard_leave(&mut self) {
        let Some((cid, sid)) = self.keyboard_focus.take() else {
            return;
        };
        let keyboards = self.clients.get(&cid).map(|c| c.keyboards.clone()).unwrap_or_default();
        let serial = self.next_serial();
        for k in keyboards {
            self.post(cid, Message::new(k, 2).u32(serial).u32(sid).finish());
        }
    }

    fn key_event(&mut self, window: u32, code: i32, mods: u32) {
        let Some(&(cid, sid)) = self.windows.get(&window) else {
            return;
        };
        let sid = self
            .clients
            .get(&cid)
            .and_then(|c| c.surfaces.iter().filter(|(_, s)| s.window.is_some() && s.popup.map(|p| p.grab && !p.done).unwrap_or(false)).max_by_key(|(_, s)| s.popup.map(|p| p.order).unwrap_or(0)).map(|(id, _)| *id))
            .unwrap_or(sid);
        self.keyboard_enter(cid, sid);
        let Some((key, xkb_mods)) = keys::translate(code, mods) else {
            return;
        };
        let keyboards = self.clients.get(&cid).map(|c| c.keyboards.clone()).unwrap_or_default();
        let time = self.now();
        for k in keyboards {
            let serial = self.next_serial();
            if xkb_mods != 0 {
                self.post(cid, Message::new(k, 4).u32(serial).u32(xkb_mods).u32(0).u32(0).u32(0).finish());
            }
            self.post(cid, Message::new(k, 3).u32(serial).u32(time).u32(key).u32(1).finish());
            let serial = self.next_serial();
            self.post(cid, Message::new(k, 3).u32(serial).u32(time + 1).u32(key).u32(0).finish());
            if xkb_mods != 0 {
                let serial = self.next_serial();
                self.post(cid, Message::new(k, 4).u32(serial).u32(0).u32(0).u32(0).u32(0).finish());
            }
        }
    }

    fn reap(&mut self) {
        let dead: Vec<u32> = self.clients.iter().filter(|(_, c)| c.dead).map(|(id, _)| *id).collect();
        for cid in dead {
            let surfaces: Vec<u32> = self.clients.get(&cid).map(|c| c.surfaces.keys().copied().collect()).unwrap_or_default();
            for sid in surfaces {
                self.unmap(cid, sid);
            }
            if let Some(client) = self.clients.remove(&cid) {
                for (_, pool) in client.pools {
                    release_pool(pool);
                }
                for fd in client.fds {
                    sys::close(fd);
                }
                for (_, fds) in client.out {
                    for fd in fds {
                        sys::close(fd);
                    }
                }
                sys::close(client.fd);
            }
        }
    }

    pub fn pump_server(&mut self) {
        while let Some(event) = self.backlog.pop_front() {
            self.server_event(event);
        }
        while let Some(event) = self.link.receive(0) {
            self.server_event(event);
        }
    }

    pub fn deliver(&mut self, event: Event) {
        while let Some(old) = self.backlog.pop_front() {
            self.server_event(old);
        }
        self.server_event(event);
    }

    pub fn start(link: L) -> Result<Bridge<L>, String> {
        let dir = runtime_dir();
        let mut partial = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            partial.push('/');
            partial.push_str(part);
            sys::mkdir(&partial);
        }
        sys::chmod(&dir, 0o700);
        let name = env::var("WAYLAND_DISPLAY").unwrap_or("wayland-0");
        let path = alloc::format!("{}/{}", dir, name);
        sys::unlink(&path);
        let listen = unix::listen_at(&path, unix::SOCK_STREAM | unix::SOCK_NONBLOCK);
        if listen < 0 {
            return Err(alloc::format!("cannot listen on {}: {}", path, sys::error_name(listen)));
        }
        let keymap = fs::read("/usr/share/hamix/xkb/us.xkb").map(|mut k| {
            k.push(0);
            k
        });
        let info = hxstd::display::info();
        let screen = if info.current.width > 0 { (info.current.width, info.current.height) } else { (1024, 768) };
        println!("hxwayland: listening on {}", path);
        Ok(Bridge {
            link,
            listen: listen as u64,
            clients: BTreeMap::new(),
            next_client: 1,
            windows: BTreeMap::new(),
            serial: 0,
            keymap: keymap.unwrap_or_default(),
            pointer_focus: None,
            keyboard_focus: None,
            screen,
            started: sys::uptime_ms(),
            last_frame: 0,
            backlog: VecDeque::new(),
            placed: BTreeMap::new(),
            popup_order: 0,
            xlisten: None,
            xwm: None,
            xcheck: 0,
            toplevels: BTreeMap::new(),
            subscribed: false,
            clipboard: None,
            clipboard_owner: None,
            path,
            geometry: BTreeMap::new(),
        })
    }

    pub fn fds(&mut self) -> Vec<(i32, bool)> {
        self.xwm_prepare();
        let mut fds = alloc::vec![(self.listen as i32, false)];
        let xlisten = if self.xwm.is_none() { self.xlisten } else { None };
        fds.push((xlisten.map(|f| f as i32).unwrap_or(-1), false));
        fds.push((self.xwm.as_ref().map(|x| x.fd as i32).unwrap_or(-1), false));
        for client in self.clients.values() {
            fds.push((client.fd as i32, !client.out.is_empty()));
        }
        fds
    }

    pub fn timeout_ms(&self) -> Option<u64> {
        if self.frames_waiting() {
            Some(FRAME_MS)
        } else if !self.backlog.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    pub fn service(&mut self, wait_ms: i32) {
        let order: Vec<u32> = self.clients.keys().copied().collect();
        let mut fds: Vec<PollFd> = Vec::with_capacity(order.len() + 3);
        let xlisten = if self.xwm.is_none() { self.xlisten } else { None };
        let xfd = self.xwm.as_ref().map(|x| x.fd);
        fds.push(PollFd { fd: self.listen as i32, events: POLLIN, revents: 0 });
        fds.push(PollFd { fd: xlisten.map(|f| f as i32).unwrap_or(-1), events: POLLIN, revents: 0 });
        fds.push(PollFd { fd: xfd.map(|f| f as i32).unwrap_or(-1), events: POLLIN, revents: 0 });
        for cid in order.iter() {
            let client = &self.clients[cid];
            let events = if client.out.is_empty() { POLLIN } else { POLLIN | POLLOUT };
            fds.push(PollFd { fd: client.fd as i32, events, revents: 0 });
        }
        unix::poll(&mut fds, wait_ms);
        if fds[0].revents & POLLIN != 0 {
            self.accept();
        }
        self.pump_server();
        for (i, cid) in order.iter().enumerate() {
            let revents = fds[i + 3].revents;
            if revents & (POLLIN | POLLHUP) != 0 {
                self.read_client(*cid);
            }
            if revents & POLLOUT != 0 {
                if let Some(client) = self.clients.get_mut(cid) {
                    flush_out(client);
                }
            }
        }
        if xlisten.is_some() && fds[1].revents & POLLIN != 0 {
            self.xwm_launch();
        }
        if xfd.is_some() && fds[2].revents & (POLLIN | POLLHUP) != 0 {
            self.xwm_read(true);
        }
        self.flush_frames();
        self.xwm_idle_check();
        self.reap();
    }

    pub fn shutdown(&mut self) {
        sys::unlink(&self.path);
    }
}

fn release_pool(pool: Pool) {
    sys::close(pool.fd);
}

fn release_shm(shm: u32) {
    if shm != 0 {
        sys::shm_release(shm);
    }
}

fn alloc_buffer(bytes: usize) -> Option<(u32, *mut u32)> {
    let shm = sys::shm_create(bytes.max(4));
    if shm < 0 {
        return None;
    }
    let (ptr, _) = sys::shm_map(shm as u32)?;
    Some((shm as u32, ptr as *mut u32))
}

fn runtime_dir() -> String {
    if let Some(dir) = env::var("XDG_RUNTIME_DIR") {
        return String::from(dir);
    }
    alloc::format!("/tmp/runtime-{}", sys::getuid())
}
