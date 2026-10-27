use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::unix::{self, PollFd, POLLHUP, POLLIN};
use hamix_std::sys;
use hxproto::{Request, Title};

use crate::Bridge;

pub const XWAYLAND: &str = "/opt/linux/usr/bin/Xwayland";
pub const SOCKET_DIR: &str = "/tmp/.X11-unix";
pub const SOCKET: &str = "/tmp/.X11-unix/X0";
const XWAYLAND_IDLE_MS: u64 = 20000;

const ATOM_NAMES: [&str; 22] = [
    "WL_SURFACE_ID",
    "WM_PROTOCOLS",
    "WM_DELETE_WINDOW",
    "WM_TAKE_FOCUS",
    "_NET_WM_NAME",
    "UTF8_STRING",
    "WM_STATE",
    "_NET_WM_WINDOW_TYPE",
    "_NET_WM_WINDOW_TYPE_MENU",
    "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
    "_NET_WM_WINDOW_TYPE_POPUP_MENU",
    "_NET_WM_WINDOW_TYPE_TOOLTIP",
    "_NET_WM_WINDOW_TYPE_COMBO",
    "_NET_SUPPORTING_WM_CHECK",
    "_NET_WM_STATE",
    "_NET_WM_STATE_FULLSCREEN",
    "_NET_WM_STATE_MAXIMIZED_VERT",
    "_NET_WM_STATE_MAXIMIZED_HORZ",
    "_NET_ACTIVE_WINDOW",
    "_NET_SUPPORTED",
    "_NET_WM_WINDOW_TYPE_NOTIFICATION",
    "_NET_WM_PID",
];

const WL_SURFACE_ID: usize = 0;
const WM_PROTOCOLS: usize = 1;
const WM_DELETE_WINDOW: usize = 2;
const WM_TAKE_FOCUS: usize = 3;
const NET_WM_NAME: usize = 4;
const UTF8_STRING: usize = 5;
const WM_STATE: usize = 6;
const NET_WM_WINDOW_TYPE: usize = 7;
const NET_SUPPORTING_WM_CHECK: usize = 13;
const NET_WM_STATE: usize = 14;
const NET_WM_STATE_FULLSCREEN: usize = 15;
const NET_WM_STATE_MAXIMIZED_VERT: usize = 16;
const NET_WM_STATE_MAXIMIZED_HORZ: usize = 17;
const NET_ACTIVE_WINDOW: usize = 18;
const NET_SUPPORTED: usize = 19;
const NET_WM_PID: usize = 21;

const ATOM_ATOM: u32 = 4;
const ATOM_WINDOW: u32 = 33;
const ATOM_STRING: u32 = 31;
const ATOM_RESOURCE_MANAGER: u32 = 23;
const ATOM_WM_NAME: u32 = 39;
const ATOM_WM_CLASS: u32 = 67;
const ATOM_CARDINAL: u32 = 6;
const ATOM_WM_NORMAL_HINTS: u32 = 40;

const EVENT_SUBSTRUCTURE_NOTIFY: u32 = 0x80000;
const EVENT_SUBSTRUCTURE_REDIRECT: u32 = 0x100000;
const EVENT_PROPERTY_CHANGE: u32 = 0x400000;
const EVENT_FOCUS_CHANGE: u32 = 0x200000;
const CW_EVENT_MASK: u32 = 0x800;
const CW_OVERRIDE_REDIRECT: u32 = 0x200;

#[derive(Clone, Default)]
pub struct XWin {
    pub override_redirect: bool,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub mapped: bool,
    pub surface: Option<u32>,
    pub title: String,
    pub net_title: bool,
    pub delete: bool,
    pub take_focus: bool,
    pub min: (u32, u32),
    pub max: (u32, u32),
    pub class: String,
    pub instance: String,
    pub pid: u32,
}

#[derive(Clone, Copy)]
enum Pending {
    Title(u32, bool),
    Class(u32),
    Pid(u32),
    Protocols(u32),
    Hints(u32),
    Attributes(u32),
    Geometry(u32),
    Tree,
}

pub struct Xwm {
    pub fd: u64,
    inbuf: Vec<u8>,
    seq: u16,
    root: u32,
    next_id: u32,
    id_mask: u32,
    atoms: [u32; 22],
    pending: BTreeMap<u16, Pending>,
    replies: BTreeMap<u16, Vec<u8>>,
    pub windows: BTreeMap<u32, XWin>,
    pub cid: Option<u32>,
    pub pid: i64,
    check: u32,
    pub focus: u32,
    orphans: BTreeMap<u32, u32>,
    idle_since: u64,
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn i16_at(b: &[u8], i: usize) -> i16 {
    u16_at(b, i) as i16
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

struct Req {
    buf: Vec<u8>,
}

impl Req {
    fn new(opcode: u8, data: u8) -> Req {
        Req { buf: alloc::vec![opcode, data, 0, 0] }
    }

    fn u32(mut self, v: u32) -> Req {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn u16(mut self, v: u16) -> Req {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn bytes(mut self, data: &[u8]) -> Req {
        self.buf.extend_from_slice(data);
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        self
    }

    fn finish(mut self) -> Vec<u8> {
        let words = (self.buf.len() / 4) as u16;
        self.buf[2..4].copy_from_slice(&words.to_le_bytes());
        self.buf
    }
}

impl Xwm {
    fn write(&mut self, data: &[u8]) {
        let mut sent = 0;
        let mut attempts = 0;
        while sent < data.len() {
            let r = unix::send_with_fds(self.fd, &data[sent..], &[], unix::MSG_NOSIGNAL);
            if r < 0 {
                if r == unix::EAGAIN && attempts < 500 {
                    attempts += 1;
                    sys::sleep_ms(1);
                    continue;
                }
                return;
            }
            sent += r as usize;
        }
    }

    fn send(&mut self, request: Vec<u8>) -> u16 {
        self.write(&request);
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    fn atom(&self, index: usize) -> u32 {
        self.atoms[index]
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = (self.next_id & !self.id_mask) | ((self.next_id + 1) & self.id_mask);
        id
    }

    fn change_attributes(&mut self, window: u32, mask: u32, values: &[u32]) {
        let mut req = Req::new(2, 0).u32(window).u32(mask);
        for v in values {
            req = req.u32(*v);
        }
        self.send(req.finish());
    }

    pub fn configure(&mut self, window: u32, x: Option<i32>, y: Option<i32>, w: Option<u32>, h: Option<u32>) {
        let mut mask: u16 = 0;
        let mut values = Vec::new();
        if let Some(x) = x {
            mask |= 1;
            values.push(x as u32);
        }
        if let Some(y) = y {
            mask |= 2;
            values.push(y as u32);
        }
        if let Some(w) = w {
            mask |= 4;
            values.push(w.max(1));
        }
        if let Some(h) = h {
            mask |= 8;
            values.push(h.max(1));
        }
        if mask == 0 {
            return;
        }
        let mut req = Req::new(12, 0).u32(window).u16(mask).u16(0);
        for v in values {
            req = req.u32(v);
        }
        self.send(req.finish());
        if let Some(win) = self.windows.get_mut(&window) {
            if let Some(x) = x {
                win.x = x;
            }
            if let Some(y) = y {
                win.y = y;
            }
            if let Some(w) = w {
                win.w = w;
            }
            if let Some(h) = h {
                win.h = h;
            }
        }
    }

    fn map(&mut self, window: u32) {
        self.send(Req::new(8, 0).u32(window).finish());
    }

    fn unmap(&mut self, window: u32) {
        self.send(Req::new(10, 0).u32(window).finish());
    }

    fn change_property(&mut self, window: u32, property: u32, kind: u32, format: u8, data: &[u8]) {
        let units = data.len() as u32 / (format as u32 / 8);
        let req = Req::new(18, 0).u32(window).u32(property).u32(kind).u32(format as u32).u32(units).bytes(data).finish();
        self.send(req);
    }

    fn get_property(&mut self, window: u32, property: u32, kind: u32, what: Pending) {
        let seq = self.send(Req::new(20, 0).u32(window).u32(property).u32(kind).u32(0).u32(2048).finish());
        self.pending.insert(seq, what);
    }

    fn query(&mut self, request: Vec<u8>, what: Pending) {
        let seq = self.send(request);
        self.pending.insert(seq, what);
    }

    pub fn focus_window(&mut self, window: u32) {
        self.focus = window;
        let take = self.windows.get(&window).map(|w| w.take_focus).unwrap_or(false);
        self.send(Req::new(42, 1).u32(window).u32(0).finish());
        if take {
            let atoms = [self.atom(WM_TAKE_FOCUS), 0, 0, 0, 0];
            self.client_message(window, self.atom(WM_PROTOCOLS), atoms);
        }
        let data = window.to_le_bytes();
        let root = self.root;
        let active = self.atom(NET_ACTIVE_WINDOW);
        self.change_property(root, active, ATOM_WINDOW, 32, &data);
    }

    fn client_message(&mut self, window: u32, kind: u32, data: [u32; 5]) {
        let mut event = alloc::vec![33u8, 32, 0, 0];
        event.extend_from_slice(&window.to_le_bytes());
        event.extend_from_slice(&kind.to_le_bytes());
        for v in data {
            event.extend_from_slice(&v.to_le_bytes());
        }
        let req = Req::new(25, 0).u32(window).u32(0).bytes(&event).finish();
        self.send(req);
    }

    pub fn close(&mut self, window: u32) {
        let delete = self.windows.get(&window).map(|w| w.delete).unwrap_or(false);
        if delete {
            let data = [self.atom(WM_DELETE_WINDOW), 0, 0, 0, 0];
            self.client_message(window, self.atom(WM_PROTOCOLS), data);
        } else {
            self.send(Req::new(113, 0).u32(window).finish());
        }
    }

    fn manage(&mut self, window: u32) {
        self.change_attributes(window, CW_EVENT_MASK, &[EVENT_PROPERTY_CHANGE | EVENT_FOCUS_CHANGE]);
        self.get_property(window, ATOM_WM_NAME, 0, Pending::Title(window, false));
        let net_name = self.atom(NET_WM_NAME);
        self.get_property(window, net_name, 0, Pending::Title(window, true));
        let protocols = self.atom(WM_PROTOCOLS);
        self.get_property(window, protocols, ATOM_ATOM, Pending::Protocols(window));
        self.get_property(window, ATOM_WM_NORMAL_HINTS, 0, Pending::Hints(window));
        self.get_property(window, ATOM_WM_CLASS, ATOM_STRING, Pending::Class(window));
        let pid = self.atom(NET_WM_PID);
        self.get_property(window, pid, ATOM_CARDINAL, Pending::Pid(window));
        let state = self.atom(WM_STATE);
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        self.change_property(window, state, state, 32, &data);
    }
}

fn x_resources() -> String {
    let dark = hamix_std::env::var("HOME")
        .and_then(|home| hamix_std::fs::read_to_string(&alloc::format!("{}/.config/gtk-3.0/settings.ini", home)))
        .map(|text| text.contains("prefer-dark-theme=1"))
        .unwrap_or(false);
    let (bg, fg, base, select, select_fg) = if dark {
        ("#1b1e25", "#e8eaf0", "#23272f", "#5b8cff", "#ffffff")
    } else {
        ("#f6f7f9", "#1d2129", "#ffffff", "#2f6fed", "#ffffff")
    };
    alloc::format!(
        "*background:\t{bg}\n*foreground:\t{fg}\n*Text.background:\t{base}\n*selectBackground:\t{select}\n*selectForeground:\t{select_fg}\n*scheme:\tgtk+\nfltk.scheme:\tgtk+\nXft.dpi:\t96\nXft.antialias:\t1\nXft.hinting:\t1\nXft.hintstyle:\thintslight\nXft.rgba:\tnone\nXcursor.theme:\tAdwaita\n"
    )
}

fn decode_text(raw: &[u8]) -> String {
    let mut clean = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == 0x1B {
            i += 1;
            while i < raw.len() && (0x20..0x30).contains(&raw[i]) {
                i += 1;
            }
            i += 1;
            continue;
        }
        clean.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&clean).into_owned()
}

pub struct Parsed {
    replies: Vec<(u16, Vec<u8>)>,
    events: Vec<Vec<u8>>,
}

fn parse(inbuf: &mut Vec<u8>) -> Parsed {
    let mut out = Parsed { replies: Vec::new(), events: Vec::new() };
    loop {
        if inbuf.len() < 32 {
            break;
        }
        let size = if inbuf[0] == 1 { 32 + u32_at(inbuf, 4) as usize * 4 } else { 32 };
        if inbuf.len() < size {
            break;
        }
        let message: Vec<u8> = inbuf.drain(..size).collect();
        match message[0] {
            0 | 1 => out.replies.push((u16_at(&message, 2), message)),
            _ => out.events.push(message),
        }
    }
    out
}

impl Bridge {
    pub fn xwm_prepare(&mut self) {
        if self.xlisten.is_some() || self.xwm.is_some() || sys::stat(XWAYLAND).is_err() {
            return;
        }
        let now = sys::uptime_ms();
        if now.saturating_sub(self.xcheck) < 3000 && self.xcheck != 0 {
            return;
        }
        self.xcheck = now;
        sys::mkdir(SOCKET_DIR);
        sys::chmod(SOCKET_DIR, 0o1777);
        sys::unlink(SOCKET);
        sys::unlink("/tmp/.X0-lock");
        let fd = unix::listen_at(SOCKET, unix::SOCK_STREAM | unix::SOCK_NONBLOCK);
        if fd >= 0 {
            sys::chmod(SOCKET, 0o777);
            self.xlisten = Some(fd as u64);
        }
    }

    pub fn xwm_launch(&mut self) {
        let Some(listen) = self.xlisten else {
            return;
        };
        if self.xwm.is_some() {
            return;
        }
        sys::unlink("/tmp/.X0-lock");
        let log = sys::open_with("/tmp/xwayland.log", sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC);
        let args = [":0", "-rootless", "-shm", "-noreset", "-nolisten", "tcp", "-listenfd", "3"];
        let mut env: Vec<String> = hamix_std::env::vars().iter().filter(|(k, _)| k != "WAYLAND_SOCKET" && k != "WAYLAND_DISPLAY").map(|(k, v)| alloc::format!("{}={}", k, v)).collect();
        env.push(String::from("WAYLAND_SOCKET=0"));
        let err = if log >= 0 { Some(log as u64) } else { None };
        let Ok((ours, theirs)) = unix::socketpair(unix::SOCK_STREAM) else {
            return;
        };
        let pid = sys::spawn_fds_env(XWAYLAND, &args, Some(&env), sys::SPAWN_DETACH, [Some(theirs), err, err, Some(listen)]);
        sys::close(theirs);
        if log >= 0 {
            sys::close(log as u64);
        }
        if pid < 0 {
            sys::close(ours);
        } else {
            unix::set_nonblocking(ours, true);
        }
        let wayland_cid = if pid >= 0 { Some(self.add_client(ours)) } else { None };
        if pid < 0 {
            self.xwm_forget();
            self.xcheck = sys::uptime_ms();
            return;
        }
        let fd = unix::connect_to(SOCKET, unix::SOCK_STREAM);
        if fd < 0 {
            sys::kill(pid);
            self.xwm_forget();
            self.xcheck = sys::uptime_ms();
            return;
        }
        unix::set_nonblocking(fd as u64, true);
        self.xwm = Some(Xwm {
            fd: fd as u64,
            inbuf: Vec::new(),
            seq: 0,
            root: 0,
            next_id: 0,
            id_mask: 0,
            atoms: [0; 22],
            pending: BTreeMap::new(),
            replies: BTreeMap::new(),
            windows: BTreeMap::new(),
            cid: wayland_cid,
            pid,
            check: 0,
            focus: 0,
            orphans: BTreeMap::new(),
            idle_since: 0,
        });
        if !self.xwm_setup() {
            if let Some(xwm) = self.xwm.take() {
                sys::close(xwm.fd);
            }
            sys::kill(pid);
            self.xwm_forget();
            self.xcheck = sys::uptime_ms();
        }
    }

    fn xwm_service(&mut self, timeout: i32) {
        let Some(xfd) = self.xwm.as_ref().map(|x| x.fd) else {
            return;
        };
        let order: Vec<u32> = self.clients.keys().copied().collect();
        let mut fds = Vec::with_capacity(order.len() + 3);
        fds.push(PollFd { fd: xfd as i32, events: POLLIN, revents: 0 });
        fds.push(PollFd { fd: self.listen as i32, events: POLLIN, revents: 0 });
        fds.push(PollFd { fd: self.mailbox as i32, events: POLLIN, revents: 0 });
        for cid in order.iter() {
            fds.push(PollFd { fd: self.clients[cid].fd as i32, events: POLLIN, revents: 0 });
        }
        unix::poll(&mut fds, timeout);
        if fds[1].revents & POLLIN != 0 {
            self.accept();
        }
        self.pump_server();
        for (i, cid) in order.iter().enumerate() {
            if fds[i + 3].revents & (POLLIN | POLLHUP) != 0 {
                self.read_client(*cid);
            }
        }
        self.flush_frames();
        self.xwm_read(false);
    }

    fn xwm_wait(&mut self, seq: u16) -> Option<Vec<u8>> {
        let deadline = sys::uptime_ms() + 15000;
        loop {
            if let Some(reply) = self.xwm.as_mut()?.replies.remove(&seq) {
                return if reply[0] == 1 { Some(reply) } else { None };
            }
            if sys::uptime_ms() > deadline || !sys::proc_alive(self.xwm.as_ref()?.pid) {
                return None;
            }
            self.xwm_service(50);
        }
    }

    fn xwm_setup(&mut self) -> bool {
        {
            let xwm = self.xwm.as_mut().unwrap();
            let setup = [0x6Cu8, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            xwm.write(&setup);
        }
        let deadline = sys::uptime_ms() + 20000;
        let reply = loop {
            let Some(xwm) = self.xwm.as_mut() else {
                return false;
            };
            let mut buf = [0u8; 4096];
            match unix::recv_with_fds(xwm.fd, &mut buf, unix::MSG_DONTWAIT) {
                Ok((0, _)) => return false,
                Ok((n, fds)) => {
                    for fd in fds {
                        sys::close(fd);
                    }
                    xwm.inbuf.extend_from_slice(&buf[..n]);
                }
                Err(unix::EAGAIN) => {}
                Err(_) => return false,
            }
            if xwm.inbuf.len() >= 8 {
                let total = 8 + u16_at(&xwm.inbuf, 6) as usize * 4;
                if xwm.inbuf.len() >= total {
                    let reply: Vec<u8> = xwm.inbuf.drain(..total).collect();
                    break reply;
                }
            }
            if sys::uptime_ms() > deadline || !sys::proc_alive(xwm.pid) {
                return false;
            }
            self.xwm_service(50);
        };
        if reply[0] != 1 {
            return false;
        }
        let vendor_len = u16_at(&reply, 24) as usize;
        let formats = reply[29] as usize;
        let screen = 40 + pad4(vendor_len) + formats * 8;
        if reply.len() < screen + 40 {
            return false;
        }
        let root = u32_at(&reply, screen);
        {
            let xwm = self.xwm.as_mut().unwrap();
            xwm.root = root;
            xwm.next_id = u32_at(&reply, 12);
            xwm.id_mask = u32_at(&reply, 16);
        }
        let mut seqs = Vec::new();
        for name in ATOM_NAMES.iter() {
            let xwm = self.xwm.as_mut().unwrap();
            let req = Req::new(16, 0).u16(name.len() as u16).u16(0).bytes(name.as_bytes()).finish();
            seqs.push(xwm.send(req));
        }
        for (i, seq) in seqs.into_iter().enumerate() {
            let Some(reply) = self.xwm_wait(seq) else {
                return false;
            };
            self.xwm.as_mut().unwrap().atoms[i] = u32_at(&reply, 8);
        }
        let name = b"Composite";
        let seq = {
            let xwm = self.xwm.as_mut().unwrap();
            xwm.send(Req::new(98, 0).u16(name.len() as u16).u16(0).bytes(name).finish())
        };
        let Some(reply) = self.xwm_wait(seq) else {
            return false;
        };
        if reply[8] == 0 {
            return false;
        }
        let composite = reply[9];
        let xwm = self.xwm.as_mut().unwrap();
        xwm.send(Req::new(composite, 0).u32(0).u32(4).finish());
        xwm.send(Req::new(composite, 2).u32(root).u32(1).finish());
        xwm.change_attributes(root, CW_EVENT_MASK, &[EVENT_SUBSTRUCTURE_NOTIFY | EVENT_SUBSTRUCTURE_REDIRECT | EVENT_PROPERTY_CHANGE]);
        let check = xwm.alloc_id();
        xwm.check = check;
        xwm.send(Req::new(1, 0).u32(check).u32(root).u16(0).u16(0).u16(1).u16(1).u16(0).u16(2).u32(0).u32(CW_OVERRIDE_REDIRECT).u32(1).finish());
        let supporting = xwm.atom(NET_SUPPORTING_WM_CHECK);
        let net_name = xwm.atom(NET_WM_NAME);
        let utf8 = xwm.atom(UTF8_STRING);
        xwm.change_property(root, supporting, ATOM_WINDOW, 32, &check.to_le_bytes());
        xwm.change_property(check, supporting, ATOM_WINDOW, 32, &check.to_le_bytes());
        xwm.change_property(check, net_name, utf8, 8, b"Nook");
        let supported: Vec<u8> = [NET_WM_NAME, NET_WM_STATE, NET_WM_STATE_FULLSCREEN, NET_WM_STATE_MAXIMIZED_VERT, NET_WM_STATE_MAXIMIZED_HORZ, NET_ACTIVE_WINDOW, NET_SUPPORTING_WM_CHECK, NET_WM_WINDOW_TYPE]
            .iter()
            .flat_map(|i| xwm.atoms[*i].to_le_bytes())
            .collect();
        let supported_atom = xwm.atom(NET_SUPPORTED);
        xwm.change_property(root, supported_atom, ATOM_ATOM, 32, &supported);
        let resources = x_resources();
        xwm.change_property(root, ATOM_RESOURCE_MANAGER, ATOM_STRING, 8, resources.as_bytes());
        xwm.query(Req::new(15, 0).u32(root).finish(), Pending::Tree);
        true
    }

    pub fn xwm_read(&mut self, from_poll: bool) {
        let _ = from_poll;
        let parsed = {
            let Some(xwm) = self.xwm.as_mut() else {
                return;
            };
            let mut buf = [0u8; 8192];
            let mut dead = false;
            loop {
                match unix::recv_with_fds(xwm.fd, &mut buf, unix::MSG_DONTWAIT) {
                    Ok((0, _)) => {
                        dead = true;
                        break;
                    }
                    Ok((n, fds)) => {
                        for fd in fds {
                            sys::close(fd);
                        }
                        xwm.inbuf.extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
            if dead {
                let fd = xwm.fd;
                let pid = xwm.pid;
                self.xwm = None;
                sys::close(fd);
                sys::kill(pid);
                self.xwm_forget();
                return;
            }
            parse(&mut xwm.inbuf)
        };
        for (seq, reply) in parsed.replies {
            let pending = self.xwm.as_mut().and_then(|x| x.pending.remove(&seq));
            match pending {
                Some(what) => {
                    if reply[0] == 1 {
                        self.xwm_reply(what, &reply);
                    }
                }
                None => {
                    if let Some(xwm) = self.xwm.as_mut() {
                        xwm.replies.insert(seq, reply);
                    }
                }
            }
        }
        for event in parsed.events {
            self.xwm_event(&event);
        }
    }

    pub fn xwm_idle_check(&mut self) {
        let now = sys::uptime_ms();
        let Some(xwm) = self.xwm.as_mut() else {
            return;
        };
        if !xwm.windows.is_empty() {
            xwm.idle_since = 0;
            return;
        }
        if xwm.idle_since == 0 {
            xwm.idle_since = now;
            return;
        }
        if now.saturating_sub(xwm.idle_since) < XWAYLAND_IDLE_MS {
            return;
        }
        let Some(xwm) = self.xwm.take() else {
            return;
        };
        sys::close(xwm.fd);
        sys::kill(xwm.pid);
        if let Some(cid) = xwm.cid {
            if let Some(client) = self.clients.get_mut(&cid) {
                client.dead = true;
            }
        }
        sys::unlink("/tmp/.X0-lock");
    }

    fn xwm_forget(&mut self) {
        let listen = self.xlisten.take();
        if let Some(fd) = listen {
            sys::close(fd);
        }
        self.xcheck = 0;
    }

    fn xwm_reply(&mut self, what: Pending, reply: &[u8]) {
        match what {
            Pending::Title(window, net) => {
                let len = u32_at(reply, 16) as usize;
                if reply.len() < 32 + len {
                    return;
                }
                let text = decode_text(&reply[32..32 + len]);
                if text.is_empty() {
                    return;
                }
                let changed = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)).map(|w| {
                    if net || !w.net_title {
                        w.title = text.clone();
                        w.net_title |= net;
                        true
                    } else {
                        false
                    }
                });
                if changed == Some(true) {
                    self.xwm_sync_title(window);
                }
            }
            Pending::Class(window) => {
                let len = u32_at(reply, 16) as usize;
                if reply.len() < 32 + len {
                    return;
                }
                let mut parts = reply[32..32 + len].split(|b| *b == 0).map(|p| String::from_utf8_lossy(p).into_owned());
                let instance = parts.next().unwrap_or_default();
                let class = parts.next().unwrap_or_default();
                if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    w.instance = instance;
                    w.class = class;
                }
                self.xwm_sync_identity(window);
            }
            Pending::Pid(window) => {
                if u32_at(reply, 16) == 0 || reply.len() < 36 {
                    return;
                }
                let pid = u32_at(reply, 32);
                if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    w.pid = pid;
                }
                self.xwm_sync_identity(window);
            }
            Pending::Protocols(window) => {
                let count = u32_at(reply, 16) as usize;
                let (delete, take) = {
                    let Some(xwm) = self.xwm.as_ref() else {
                        return;
                    };
                    let atoms: Vec<u32> = (0..count).filter(|i| reply.len() >= 36 + i * 4).map(|i| u32_at(reply, 32 + i * 4)).collect();
                    (atoms.contains(&xwm.atom(WM_DELETE_WINDOW)), atoms.contains(&xwm.atom(WM_TAKE_FOCUS)))
                };
                if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    w.delete = delete;
                    w.take_focus = take;
                }
            }
            Pending::Hints(window) => {
                let count = u32_at(reply, 16) as usize;
                if count < 9 || reply.len() < 32 + 36 {
                    return;
                }
                let flags = u32_at(reply, 32);
                let min = if flags & 16 != 0 { (u32_at(reply, 52), u32_at(reply, 56)) } else { (0, 0) };
                let max = if flags & 32 != 0 { (u32_at(reply, 60), u32_at(reply, 64)) } else { (0, 0) };
                if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    w.min = min;
                    w.max = max;
                }
                if let Some(id) = self.xwm_nook_window(window) {
                    self.request(Request::SizeHints { window: id, min_width: min.0, min_height: min.1, max_width: max.0, max_height: max.1 });
                }
            }
            Pending::Attributes(window) => {
                let mapped = reply[26] == 2;
                let override_redirect = reply[27] != 0;
                let xwm = self.xwm.as_mut().unwrap();
                let entry = xwm.windows.entry(window).or_default();
                entry.override_redirect = override_redirect;
                if mapped {
                    entry.mapped = true;
                    if !override_redirect {
                        xwm.manage(window);
                    }
                    xwm.unmap(window);
                    xwm.map(window);
                }
            }
            Pending::Geometry(window) => {
                let (x, y, w, h) = (i16_at(reply, 12) as i32, i16_at(reply, 14) as i32, u16_at(reply, 16) as u32, u16_at(reply, 18) as u32);
                if let Some(win) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    win.x = x;
                    win.y = y;
                    win.w = w;
                    win.h = h;
                }
            }
            Pending::Tree => {
                let count = u16_at(reply, 16) as usize;
                let children: Vec<u32> = (0..count).filter(|i| reply.len() >= 36 + i * 4).map(|i| u32_at(reply, 32 + i * 4)).collect();
                let xwm = self.xwm.as_mut().unwrap();
                for child in children {
                    if child == xwm.check {
                        continue;
                    }
                    xwm.windows.entry(child).or_default();
                    xwm.query(Req::new(14, 0).u32(child).finish(), Pending::Geometry(child));
                    xwm.query(Req::new(3, 0).u32(child).finish(), Pending::Attributes(child));
                }
            }
        }
    }

    fn xwm_event(&mut self, e: &[u8]) {
        let code = e[0] & 0x7F;
        match code {
            16 => {
                let window = u32_at(e, 8);
                let xwm = self.xwm.as_mut().unwrap();
                if window == xwm.check {
                    return;
                }
                let win = XWin {
                    override_redirect: e[22] != 0,
                    x: i16_at(e, 12) as i32,
                    y: i16_at(e, 14) as i32,
                    w: u16_at(e, 16) as u32,
                    h: u16_at(e, 18) as u32,
                    ..XWin::default()
                };
                xwm.windows.insert(window, win);
                if let Some(sid) = xwm.orphans.remove(&window) {
                    self.xwm_associate(window, sid);
                }
            }
            17 => {
                let window = u32_at(e, 8);
                self.xwm_drop_surface(window);
                if let Some(xwm) = self.xwm.as_mut() {
                    xwm.windows.remove(&window);
                }
            }
            18 => {
                let window = u32_at(e, 8);
                self.xwm_drop_surface(window);
                if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    w.mapped = false;
                }
            }
            19 => {
                let window = u32_at(e, 8);
                let override_redirect = e[12] != 0;
                if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
                    w.mapped = true;
                    w.override_redirect = override_redirect;
                }
            }
            20 => {
                let window = u32_at(e, 8);
                let xwm = self.xwm.as_mut().unwrap();
                let entry = xwm.windows.entry(window).or_default();
                entry.override_redirect = false;
                xwm.manage(window);
                xwm.map(window);
            }
            22 => {
                let window = u32_at(e, 8);
                let (x, y, w, h) = (i16_at(e, 16) as i32, i16_at(e, 18) as i32, u16_at(e, 20) as u32, u16_at(e, 22) as u32);
                let popup = self.xwm.as_mut().and_then(|xw| xw.windows.get_mut(&window)).map(|win| {
                    let moved = (win.x, win.y) != (x, y);
                    win.x = x;
                    win.y = y;
                    win.w = w;
                    win.h = h;
                    win.override_redirect && moved
                });
                if popup == Some(true) {
                    if let Some(id) = self.xwm_nook_window(window) {
                        self.request(Request::MovePopup { window: id, x, y });
                    }
                }
            }
            23 => {
                let window = u32_at(e, 8);
                let mask = u16_at(e, 26);
                let (x, y, w, h) = (i16_at(e, 16) as i32, i16_at(e, 18) as i32, u16_at(e, 20) as u32, u16_at(e, 22) as u32);
                let placed = self.xwm_nook_window(window).is_some();
                let xwm = self.xwm.as_mut().unwrap();
                let managed = xwm.windows.get(&window).map(|w| !w.override_redirect).unwrap_or(true);
                let nx = if mask & 1 != 0 && (!managed || !placed) { Some(x) } else { None };
                let ny = if mask & 2 != 0 && (!managed || !placed) { Some(y) } else { None };
                let nw = if mask & 4 != 0 { Some(w) } else { None };
                let nh = if mask & 8 != 0 { Some(h) } else { None };
                xwm.configure(window, nx, ny, nw, nh);
                if placed && managed && (nx.is_none() && ny.is_none()) && (mask & 3) != 0 {
                    let pos = xwm.windows.get(&window).map(|w| (w.x, w.y));
                    if let Some((px, py)) = pos {
                        xwm.configure(window, Some(px), Some(py), None, None);
                    }
                }
            }
            28 => {
                let window = u32_at(e, 4);
                let atom = u32_at(e, 8);
                let xwm = self.xwm.as_mut().unwrap();
                if !xwm.windows.contains_key(&window) {
                    return;
                }
                if atom == ATOM_WM_NAME {
                    xwm.get_property(window, ATOM_WM_NAME, 0, Pending::Title(window, false));
                } else if atom == xwm.atom(NET_WM_NAME) {
                    let a = xwm.atom(NET_WM_NAME);
                    xwm.get_property(window, a, 0, Pending::Title(window, true));
                } else if atom == ATOM_WM_NORMAL_HINTS {
                    xwm.get_property(window, ATOM_WM_NORMAL_HINTS, 0, Pending::Hints(window));
                } else if atom == xwm.atom(WM_PROTOCOLS) {
                    let a = xwm.atom(WM_PROTOCOLS);
                    xwm.get_property(window, a, ATOM_ATOM, Pending::Protocols(window));
                }
            }
            33 => {
                let window = u32_at(e, 4);
                let kind = u32_at(e, 8);
                let (surface_atom, state_atom, fullscreen, max_v, max_h) = {
                    let xwm = self.xwm.as_ref().unwrap();
                    (xwm.atom(WL_SURFACE_ID), xwm.atom(NET_WM_STATE), xwm.atom(NET_WM_STATE_FULLSCREEN), xwm.atom(NET_WM_STATE_MAXIMIZED_VERT), xwm.atom(NET_WM_STATE_MAXIMIZED_HORZ))
                };
                if kind == surface_atom {
                    let sid = u32_at(e, 12);
                    let known = self.xwm.as_ref().map(|x| x.windows.contains_key(&window)).unwrap_or(false);
                    if known {
                        self.xwm_associate(window, sid);
                    } else if let Some(xwm) = self.xwm.as_mut() {
                        xwm.orphans.insert(window, sid);
                    }
                } else if kind == state_atom {
                    let action = u32_at(e, 12);
                    let first = u32_at(e, 16);
                    let second = u32_at(e, 20);
                    let wants = [first, second].iter().any(|a| *a == fullscreen || *a == max_v || *a == max_h);
                    if wants {
                        if let Some(id) = self.xwm_nook_window(window) {
                            let maximized = action != 0;
                            self.request(Request::SetMaximized { window: id, maximized: maximized as u32 });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn xwm_cid(&mut self) -> Option<u32> {
        let cid = self.xwm.as_ref()?.cid?;
        if self.clients.contains_key(&cid) { Some(cid) } else { None }
    }

    fn xwm_associate(&mut self, window: u32, sid: u32) {
        let Some(cid) = self.xwm_cid() else {
            if let Some(xwm) = self.xwm.as_mut() {
                xwm.orphans.insert(window, sid);
            }
            return;
        };
        let exists = self.clients.get(&cid).map(|c| c.surfaces.contains_key(&sid)).unwrap_or(false);
        if !exists {
            return;
        }
        if let Some(w) = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)) {
            w.surface = Some(sid);
        }
        let held = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)).and_then(|s| {
            s.xwin = Some(window);
            s.held.take()
        });
        if let Some(buffer) = held {
            self.show(cid, sid, buffer);
            self.post(cid, crate::wire::Message::new(buffer, 0).finish());
        }
        self.xwm_sync_identity(window);
    }

    fn xwm_drop_surface(&mut self, window: u32) {
        let Some(cid) = self.xwm.as_ref().and_then(|x| x.cid) else {
            return;
        };
        let sid = self.xwm.as_mut().and_then(|x| x.windows.get_mut(&window)).and_then(|w| w.surface.take());
        if let Some(sid) = sid {
            self.unmap(cid, sid);
            if let Some(s) = self.clients.get_mut(&cid).and_then(|c| c.surfaces.get_mut(&sid)) {
                s.xwin = None;
            }
        }
    }

    pub fn xwm_nook_window(&self, window: u32) -> Option<u32> {
        let xwm = self.xwm.as_ref()?;
        let sid = xwm.windows.get(&window)?.surface?;
        let cid = xwm.cid?;
        self.clients.get(&cid)?.surfaces.get(&sid)?.window.as_ref().map(|w| w.id)
    }

    fn xwm_sync_title(&mut self, window: u32) {
        let Some(title) = self.xwm.as_ref().and_then(|x| x.windows.get(&window)).map(|w| w.title.clone()) else {
            return;
        };
        if let Some(id) = self.xwm_nook_window(window) {
            self.request(Request::SetTitle { window: id, title: Title::new(&title) });
        }
    }

    pub fn xwm_sync_identity(&mut self, window: u32) {
        let Some((class, instance, pid)) = self.xwm.as_ref().and_then(|x| x.windows.get(&window)).map(|w| (w.class.clone(), w.instance.clone(), w.pid)) else {
            return;
        };
        if class.is_empty() && instance.is_empty() && pid == 0 {
            return;
        }
        if let Some(id) = self.xwm_nook_window(window) {
            let app_id = if instance.is_empty() || instance == class { class } else { alloc::format!("{}\n{}", class, instance) };
            self.request(Request::SetAppId { window: id, pid, app_id: Title::new(&app_id) });
        }
    }

    pub fn xwm_window_info(&self, window: u32) -> Option<XWin> {
        self.xwm.as_ref()?.windows.get(&window).cloned()
    }

    pub fn xwm_placed(&mut self, window: u32, x: i32, y: i32) {
        if let Some(xwm) = self.xwm.as_mut() {
            let managed = xwm.windows.get(&window).map(|w| !w.override_redirect).unwrap_or(false);
            if managed {
                xwm.configure(window, Some(x), Some(y), None, None);
            }
        }
    }

    pub fn xwm_resize(&mut self, window: u32, width: u32, height: u32) {
        if let Some(xwm) = self.xwm.as_mut() {
            xwm.configure(window, None, None, Some(width), Some(height));
        }
    }

    pub fn xwm_close(&mut self, window: u32) {
        if let Some(xwm) = self.xwm.as_mut() {
            xwm.close(window);
        }
    }

    pub fn xwm_focus(&mut self, window: u32) {
        if let Some(xwm) = self.xwm.as_mut() {
            if xwm.focus != window {
                xwm.focus_window(window);
            }
        }
    }
}
