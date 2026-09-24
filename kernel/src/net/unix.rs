use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::task::{self, OpenFile, WAIT_PIPE};

pub const SOCK_STREAM: u8 = 1;
pub const SOCK_DGRAM: u8 = 2;
pub const SOCK_SEQPACKET: u8 = 5;

pub const EAGAIN: i64 = -11;
pub const EINVAL: i64 = -22;
pub const EPIPE: i64 = -32;
pub const EMSGSIZE: i64 = -90;
pub const EPROTOTYPE: i64 = -91;
pub const EOPNOTSUPP: i64 = -95;
pub const EADDRINUSE: i64 = -98;
pub const EISCONN: i64 = -106;
pub const ENOTCONN: i64 = -107;
pub const ECONNREFUSED: i64 = -111;
pub const EDESTADDRREQ: i64 = -89;

const CAPACITY: usize = 256 * 1024;
const MAX_DATAGRAM: usize = 212 * 1024;
const MAX_BACKLOG: usize = 128;

#[derive(Clone, Copy, Default)]
pub struct Cred {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

struct Packet {
    data: Vec<u8>,
    offset: usize,
    fds: Vec<OpenFile>,
    from: Option<String>,
    sender: Cred,
}

enum Link {
    Fresh,
    Listening(VecDeque<u32>, usize),
    Connected(u32),
    Closed,
}

struct Sock {
    kind: u8,
    refs: u32,
    link: Link,
    name: Option<String>,
    peer_name: Option<String>,
    rx: VecDeque<Packet>,
    rx_bytes: usize,
    rd_shut: bool,
    wr_shut: bool,
    cred: Cred,
    peer_cred: Cred,
    dgram_peer: Option<u32>,
    passcred: bool,
}

impl Sock {
    fn new(kind: u8, cred: Cred) -> Sock {
        Sock {
            kind,
            refs: 1,
            link: Link::Fresh,
            name: None,
            peer_name: None,
            rx: VecDeque::new(),
            rx_bytes: 0,
            rd_shut: false,
            wr_shut: false,
            cred,
            peer_cred: Cred::default(),
            dgram_peer: None,
            passcred: false,
        }
    }
}

struct Table {
    socks: BTreeMap<u32, Sock>,
    names: BTreeMap<String, u32>,
}

static TABLE: Mutex<Table> = Mutex::new(Table { socks: BTreeMap::new(), names: BTreeMap::new() });
static NEXT: AtomicU32 = AtomicU32::new(1);

fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    without_interrupts(|| f(&mut TABLE.lock()))
}

pub fn inflight_file_nodes(out: &mut alloc::collections::BTreeSet<usize>) {
    with_table(|t| {
        for sock in t.socks.values() {
            for packet in sock.rx.iter() {
                for file in packet.fds.iter() {
                    if let OpenFile::File { node, .. } = file {
                        out.insert(*node);
                    }
                }
            }
        }
    })
}

fn wake() {
    task::wake_all(WAIT_PIPE);
}

fn release_files(files: Vec<OpenFile>) {
    for file in files {
        if file.release() {
            crate::fs::request_sync();
        }
    }
}

pub fn create(kind: u8, cred: Cred) -> u32 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    with_table(|t| t.socks.insert(id, Sock::new(kind, cred)));
    id
}

pub fn pair(kind: u8, cred: Cred) -> (u32, u32) {
    let a = NEXT.fetch_add(1, Ordering::Relaxed);
    let b = NEXT.fetch_add(1, Ordering::Relaxed);
    with_table(|t| {
        let mut sa = Sock::new(kind, cred);
        let mut sb = Sock::new(kind, cred);
        sa.link = Link::Connected(b);
        sb.link = Link::Connected(a);
        sa.peer_cred = cred;
        sb.peer_cred = cred;
        t.socks.insert(a, sa);
        t.socks.insert(b, sb);
    });
    (a, b)
}

pub fn retain(id: u32) {
    with_table(|t| {
        if let Some(s) = t.socks.get_mut(&id) {
            s.refs += 1;
        }
    });
}

fn detach(t: &mut Table, id: u32, files: &mut Vec<OpenFile>, orphans: &mut Vec<u32>) {
    let Some(sock) = t.socks.remove(&id) else {
        return;
    };
    if let Some(name) = &sock.name {
        if t.names.get(name) == Some(&id) {
            t.names.remove(name);
        }
    }
    for packet in sock.rx {
        files.extend(packet.fds);
    }
    match sock.link {
        Link::Connected(peer) => {
            if let Some(p) = t.socks.get_mut(&peer) {
                if matches!(p.link, Link::Connected(x) if x == id) {
                    p.link = Link::Closed;
                }
            }
        }
        Link::Listening(backlog, _) => orphans.extend(backlog),
        _ => {}
    }
    for other in t.socks.values_mut() {
        if other.dgram_peer == Some(id) {
            other.dgram_peer = None;
        }
    }
}

pub fn release(id: u32) {
    let mut files = Vec::new();
    with_table(|t| {
        let Some(sock) = t.socks.get_mut(&id) else {
            return;
        };
        sock.refs = sock.refs.saturating_sub(1);
        if sock.refs > 0 {
            return;
        }
        let mut orphans = Vec::new();
        detach(t, id, &mut files, &mut orphans);
        while let Some(orphan) = orphans.pop() {
            detach(t, orphan, &mut files, &mut orphans);
        }
    });
    release_files(files);
    wake();
}

pub fn kind(id: u32) -> Option<u8> {
    with_table(|t| t.socks.get(&id).map(|s| s.kind))
}

pub fn bind(id: u32, name: &str) -> Result<(), i64> {
    with_table(|t| {
        if t.names.contains_key(name) {
            return Err(EADDRINUSE);
        }
        let sock = t.socks.get_mut(&id).ok_or(EINVAL)?;
        if sock.name.is_some() {
            return Err(EINVAL);
        }
        sock.name = Some(String::from(name));
        t.names.insert(String::from(name), id);
        Ok(())
    })
}

pub fn is_bound_name(name: &str) -> bool {
    with_table(|t| t.names.contains_key(name))
}

pub fn forget_name(name: &str) {
    with_table(|t| {
        if let Some(id) = t.names.remove(name) {
            if let Some(sock) = t.socks.get_mut(&id) {
                sock.name = None;
            }
        }
    });
}

pub fn listen(id: u32, backlog: usize) -> Result<(), i64> {
    with_table(|t| {
        let sock = t.socks.get_mut(&id).ok_or(EINVAL)?;
        if sock.kind == SOCK_DGRAM {
            return Err(EOPNOTSUPP);
        }
        match &mut sock.link {
            Link::Fresh => {
                if sock.name.is_none() {
                    return Err(EINVAL);
                }
                sock.link = Link::Listening(VecDeque::new(), backlog.clamp(1, MAX_BACKLOG));
                Ok(())
            }
            Link::Listening(_, max) => {
                *max = backlog.clamp(1, MAX_BACKLOG);
                Ok(())
            }
            _ => Err(EINVAL),
        }
    })
}

pub fn connect(id: u32, name: &str, cred: Cred) -> Result<(), i64> {
    let result = with_table(|t| {
        let target = *t.names.get(name).ok_or(ECONNREFUSED)?;
        let my_kind = t.socks.get(&id).ok_or(EINVAL)?.kind;
        let (target_kind, listener_cred, backlog_full, listening) = {
            let target_sock = t.socks.get(&target).ok_or(ECONNREFUSED)?;
            match &target_sock.link {
                Link::Listening(q, max) => (target_sock.kind, target_sock.cred, q.len() >= *max, true),
                _ => (target_sock.kind, target_sock.cred, false, false),
            }
        };
        if target_kind != my_kind {
            return Err(EPROTOTYPE);
        }
        if my_kind == SOCK_DGRAM {
            let sock = t.socks.get_mut(&id).ok_or(EINVAL)?;
            sock.dgram_peer = Some(target);
            sock.peer_name = Some(String::from(name));
            return Ok(());
        }
        match t.socks.get(&id).map(|s| &s.link) {
            Some(Link::Fresh) => {}
            Some(Link::Connected(_)) => return Err(EISCONN),
            _ => return Err(EINVAL),
        }
        if !listening {
            return Err(ECONNREFUSED);
        }
        if backlog_full {
            return Err(EAGAIN);
        }
        let server_id = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut server = Sock::new(my_kind, listener_cred);
        server.link = Link::Connected(id);
        server.name = None;
        server.peer_cred = cred;
        let client_name = t.socks.get(&id).and_then(|s| s.name.clone());
        server.peer_name = client_name;
        t.socks.insert(server_id, server);
        if let Some(Link::Listening(q, _)) = t.socks.get_mut(&target).map(|s| &mut s.link) {
            q.push_back(server_id);
        }
        let client = t.socks.get_mut(&id).ok_or(EINVAL)?;
        client.link = Link::Connected(server_id);
        client.peer_cred = listener_cred;
        client.peer_name = Some(String::from(name));
        Ok(())
    });
    if result.is_ok() {
        wake();
    }
    result
}

pub fn accept(id: u32) -> Result<u32, i64> {
    with_table(|t| {
        let sock = t.socks.get_mut(&id).ok_or(EINVAL)?;
        match &mut sock.link {
            Link::Listening(q, _) => q.pop_front().ok_or(EAGAIN),
            _ => Err(EINVAL),
        }
    })
}

pub fn send(id: u32, data: &[u8], fds: &mut Vec<OpenFile>, to: Option<&str>) -> Result<usize, i64> {
    let result = with_table(|t| {
        let sock = t.socks.get(&id).ok_or(EINVAL)?;
        if sock.wr_shut {
            return Err(EPIPE);
        }
        let from = sock.name.clone();
        let kind = sock.kind;
        let sender = sender_cred(sock.cred.gid);
        let dest = match (&sock.link, kind) {
            (link, SOCK_DGRAM) => match (to, link) {
                (Some(name), _) => *t.names.get(name).ok_or(ECONNREFUSED)?,
                (None, Link::Connected(peer)) => *peer,
                (None, Link::Closed) => return Err(ECONNREFUSED),
                (None, _) => sock.dgram_peer.ok_or(EDESTADDRREQ)?,
            },
            (Link::Connected(peer), _) => {
                if to.is_some() {
                    return Err(EISCONN);
                }
                *peer
            }
            (Link::Closed, _) => return Err(EPIPE),
            _ => return Err(ENOTCONN),
        };
        let peer = t.socks.get_mut(&dest).ok_or(if kind == SOCK_DGRAM { ECONNREFUSED } else { EPIPE })?;
        if peer.kind != kind {
            return Err(EPROTOTYPE);
        }
        if peer.rd_shut {
            return Err(EPIPE);
        }
        if kind == SOCK_STREAM && data.is_empty() {
            return Ok(0);
        }
        let room = CAPACITY.saturating_sub(peer.rx_bytes);
        let take = if kind == SOCK_STREAM {
            if room == 0 && !data.is_empty() {
                return Err(EAGAIN);
            }
            data.len().min(room)
        } else {
            if data.len() > MAX_DATAGRAM {
                return Err(EMSGSIZE);
            }
            if room < data.len() && peer.rx_bytes > 0 {
                return Err(EAGAIN);
            }
            data.len()
        };
        peer.rx_bytes += take;
        peer.rx.push_back(Packet { data: data[..take].to_vec(), offset: 0, fds: core::mem::take(fds), from: if kind == SOCK_DGRAM { from } else { None }, sender });
        Ok(take)
    });
    if result.is_ok() {
        wake();
    }
    result
}

pub struct Received {
    pub data: Vec<u8>,
    pub fds: Vec<OpenFile>,
    pub from: Option<String>,
    pub full_len: usize,
    pub cred: Option<Cred>,
}

fn sender_cred(gid: u32) -> Cred {
    let (pid, leader, uid) = crate::task::with_current(|t| (t.pid, t.leader, t.uid));
    Cred { pid: if leader != 0 { leader } else { pid }, uid, gid }
}

pub fn set_passcred(id: u32, on: bool) -> bool {
    with_table(|t| match t.socks.get_mut(&id) {
        Some(sock) => {
            sock.passcred = on;
            true
        }
        None => false,
    })
}

pub fn recv(id: u32, max: usize, peek: bool) -> Result<Received, i64> {
    let result = with_table(|t| {
        let sock = t.socks.get(&id).ok_or(EINVAL)?;
        let kind = sock.kind;
        let eof = sock.rd_shut
            || match &sock.link {
                Link::Closed => true,
                Link::Connected(peer) => t.socks.get(peer).map(|p| p.wr_shut).unwrap_or(true),
                Link::Fresh if kind != SOCK_DGRAM => return Err(ENOTCONN),
                Link::Listening(..) => return Err(ENOTCONN),
                _ => false,
            };
        let sock = t.socks.get_mut(&id).ok_or(EINVAL)?;
        if sock.rx.is_empty() {
            return if eof { Ok(Received { data: Vec::new(), fds: Vec::new(), from: None, full_len: 0, cred: None }) } else { Err(EAGAIN) };
        }
        let passcred = sock.passcred;
        let mut out = Received { data: Vec::new(), fds: Vec::new(), from: None, full_len: 0, cred: None };
        if passcred {
            out.cred = sock.rx.front().map(|p| p.sender);
        }
        if kind != SOCK_STREAM {
            let packet = sock.rx.front_mut().unwrap();
            let n = packet.data.len().min(max);
            out.data.extend_from_slice(&packet.data[..n]);
            out.full_len = packet.data.len();
            out.from = packet.from.clone();
            if !peek {
                let packet = sock.rx.pop_front().unwrap();
                sock.rx_bytes -= packet.data.len();
                out.fds = packet.fds;
            }
            return Ok(out);
        }
        let mut index = 0;
        while out.data.len() < max {
            let Some(packet) = sock.rx.get_mut(index) else {
                break;
            };
            if !packet.fds.is_empty() && !out.data.is_empty() {
                break;
            }
            let available = &packet.data[packet.offset..];
            let n = available.len().min(max - out.data.len());
            out.data.extend_from_slice(&available[..n]);
            let had_fds = !packet.fds.is_empty();
            if peek {
                index += 1;
                if had_fds {
                    break;
                }
                continue;
            }
            out.fds.extend(core::mem::take(&mut packet.fds));
            packet.offset += n;
            sock.rx_bytes -= n;
            if packet.offset >= packet.data.len() {
                sock.rx.pop_front();
            }
            if had_fds {
                break;
            }
        }
        out.full_len = out.data.len();
        Ok(out)
    });
    if matches!(result, Ok(_)) && !peek {
        wake();
    }
    result
}

pub fn shutdown(id: u32, how: u64) -> Result<(), i64> {
    let result = with_table(|t| {
        let sock = t.socks.get_mut(&id).ok_or(EINVAL)?;
        if !matches!(sock.link, Link::Connected(_) | Link::Closed) && sock.kind != SOCK_DGRAM {
            return Err(ENOTCONN);
        }
        match how {
            0 => sock.rd_shut = true,
            1 => sock.wr_shut = true,
            2 => {
                sock.rd_shut = true;
                sock.wr_shut = true;
            }
            _ => return Err(EINVAL),
        }
        Ok(())
    });
    wake();
    result
}

pub struct Readiness {
    pub readable: bool,
    pub writable: bool,
    pub hangup: bool,
    pub pending: usize,
}

pub fn readiness(id: u32) -> Readiness {
    with_table(|t| {
        let Some(sock) = t.socks.get(&id) else {
            return Readiness { readable: true, writable: false, hangup: true, pending: 0 };
        };
        let pending = sock.rx_bytes;
        match &sock.link {
            Link::Listening(q, _) => Readiness { readable: !q.is_empty(), writable: false, hangup: false, pending: q.len() },
            Link::Connected(peer) => {
                let p = t.socks.get(peer);
                let peer_wr_shut = p.map(|p| p.wr_shut).unwrap_or(true);
                let room = p.map(|p| !p.rd_shut && p.rx_bytes < CAPACITY).unwrap_or(false);
                Readiness {
                    readable: !sock.rx.is_empty() || sock.rd_shut || peer_wr_shut,
                    writable: room && !sock.wr_shut,
                    hangup: p.is_none() || (sock.rd_shut && sock.wr_shut),
                    pending,
                }
            }
            Link::Closed => Readiness { readable: true, writable: false, hangup: true, pending },
            Link::Fresh => Readiness {
                readable: !sock.rx.is_empty(),
                writable: sock.kind == SOCK_DGRAM,
                hangup: sock.kind != SOCK_DGRAM,
                pending,
            },
        }
    })
}

pub fn names(id: u32) -> (Option<String>, Option<String>) {
    with_table(|t| t.socks.get(&id).map(|s| (s.name.clone(), s.peer_name.clone())).unwrap_or((None, None)))
}

pub fn peer_cred(id: u32) -> Cred {
    with_table(|t| t.socks.get(&id).map(|s| s.peer_cred).unwrap_or_default())
}

pub fn is_listening(id: u32) -> bool {
    with_table(|t| t.socks.get(&id).map(|s| matches!(s.link, Link::Listening(..))).unwrap_or(false))
}

pub fn is_connected(id: u32) -> bool {
    with_table(|t| t.socks.get(&id).map(|s| matches!(s.link, Link::Connected(_))).unwrap_or(false))
}
