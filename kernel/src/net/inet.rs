use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use spin::Mutex;

use smoltcp::iface::SocketHandle;
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Duration;
use smoltcp::wire::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address};

use super::stack::{self, Stack};
use crate::arch::without_interrupts;
use crate::task;

pub const ID_BASE: u32 = 0x4000_0000;

pub const EBADF: i64 = -9;
pub const EAGAIN: i64 = -11;
pub const EINVAL: i64 = -22;
pub const EPIPE: i64 = -32;
pub const EDESTADDRREQ: i64 = -89;
pub const EMSGSIZE: i64 = -90;
pub const EOPNOTSUPP: i64 = -95;
pub const EADDRINUSE: i64 = -98;
pub const ENETDOWN: i64 = -100;
pub const ENETUNREACH: i64 = -101;
pub const ECONNRESET: i64 = -104;
pub const EISCONN: i64 = -106;
pub const ENOTCONN: i64 = -107;
pub const ETIMEDOUT: i64 = -110;
pub const ECONNREFUSED: i64 = -111;
pub const EALREADY: i64 = -114;
pub const EINPROGRESS: i64 = -115;

const TCP_RX_BUFFER: usize = 256 * 1024;
const TCP_TX_BUFFER: usize = 128 * 1024;
const UDP_PACKETS: usize = 64;
const UDP_BYTES: usize = 128 * 1024;
const CONNECT_TIMEOUT_MS: u64 = 30_000;
const IDLE_TIMEOUT_S: u64 = 120;
const LINGER_MS: u64 = 20_000;
const MAX_BACKLOG: usize = 16;

pub type Endpoint = ([u8; 4], u16);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Fresh,
    Connecting,
    Connected,
    Listening,
    Failed,
}

struct Socket {
    refs: u32,
    proto: Proto,
    phase: Phase,
    handle: Option<SocketHandle>,
    generation: u64,
    backlog: Vec<SocketHandle>,
    bound: Option<Endpoint>,
    peer: Option<Endpoint>,
    error: i64,
    connect_started: u64,
    shut_read: bool,
    shut_write: bool,
    nodelay: bool,
    keepalive: bool,
    recv_timeout_ms: u64,
    send_timeout_ms: u64,
}

pub struct Ready {
    pub readable: bool,
    pub writable: bool,
    pub hangup: bool,
    pub error: bool,
    pub pending: usize,
}

pub struct Datagram {
    pub data: Vec<u8>,
    pub full_len: usize,
    pub from: Option<Endpoint>,
}

static SOCKETS: Mutex<BTreeMap<u32, Socket>> = Mutex::new(BTreeMap::new());
static NEXT_ID: AtomicU32 = AtomicU32::new(ID_BASE + 1);
static OPEN: AtomicUsize = AtomicUsize::new(0);

pub fn is_inet(id: u32) -> bool {
    id >= ID_BASE
}

pub fn open_count() -> usize {
    OPEN.load(Ordering::Relaxed)
}

fn with_table<R>(f: impl FnOnce(&mut BTreeMap<u32, Socket>) -> R) -> R {
    without_interrupts(|| f(&mut SOCKETS.lock()))
}

fn with_socket<R>(id: u32, f: impl FnOnce(&mut Socket) -> Result<R, i64>) -> Result<R, i64> {
    with_table(|table| match table.get_mut(&id) {
        Some(sock) => f(sock),
        None => Err(EBADF),
    })
}

fn endpoint_of(ep: IpEndpoint) -> Endpoint {
    match ep.addr {
        IpAddress::Ipv4(v4) => (v4.octets(), ep.port),
        #[allow(unreachable_patterns)]
        _ => ([0; 4], ep.port),
    }
}

fn ip_endpoint(ep: Endpoint) -> IpEndpoint {
    IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from(ep.0)), ep.1)
}

fn live(stack: &Stack, sock: &Socket) -> Option<SocketHandle> {
    if sock.generation == stack.generation { sock.handle } else { None }
}

fn new_tcp(sock: &Socket) -> tcp::Socket<'static> {
    let mut tcp = tcp::Socket::new(tcp::SocketBuffer::new(vec![0; TCP_RX_BUFFER]), tcp::SocketBuffer::new(vec![0; TCP_TX_BUFFER]));
    tcp.set_timeout(Some(Duration::from_secs(IDLE_TIMEOUT_S)));
    tcp.set_nagle_enabled(!sock.nodelay);
    if sock.keepalive {
        tcp.set_keep_alive(Some(Duration::from_secs(60)));
    }
    tcp
}

fn is_local(ip: [u8; 4], stack: &Stack) -> bool {
    ip[0] == 127 || stack.address.map(|a| a.address().octets() == ip).unwrap_or(false)
}

pub fn create(proto: Proto) -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_table(|table| {
        table.insert(
            id,
            Socket {
                refs: 1,
                proto,
                phase: Phase::Fresh,
                handle: None,
                generation: 0,
                backlog: Vec::new(),
                bound: None,
                peer: None,
                error: 0,
                connect_started: 0,
                shut_read: false,
                shut_write: false,
                nodelay: false,
                keepalive: false,
                recv_timeout_ms: 0,
                send_timeout_ms: 0,
            },
        )
    });
    OPEN.fetch_add(1, Ordering::Relaxed);
    id
}

pub fn retain(id: u32) {
    with_table(|table| {
        if let Some(sock) = table.get_mut(&id) {
            sock.refs += 1;
        }
    });
}

pub fn release(id: u32) {
    let gone = with_table(|table| {
        let last = match table.get_mut(&id) {
            Some(sock) => {
                sock.refs = sock.refs.saturating_sub(1);
                sock.refs == 0
            }
            None => false,
        };
        if last { table.remove(&id) } else { None }
    });
    let Some(sock) = gone else {
        return;
    };
    OPEN.fetch_sub(1, Ordering::Relaxed);
    stack::with_stack(|stack| {
        if sock.generation != stack.generation {
            return;
        }
        for h in sock.backlog.iter() {
            stack.sockets.get_mut::<tcp::Socket>(*h).abort();
            stack.sockets.remove(*h);
        }
        let Some(handle) = sock.handle else {
            return;
        };
        match sock.proto {
            Proto::Udp => {
                stack.sockets.remove(handle);
            }
            Proto::Tcp => {
                let tcp = stack.sockets.get_mut::<tcp::Socket>(handle);
                match tcp.state() {
                    tcp::State::Closed | tcp::State::Listen | tcp::State::TimeWait => {
                        stack.sockets.remove(handle);
                    }
                    tcp::State::SynSent | tcp::State::SynReceived => {
                        tcp.abort();
                        let deadline = task::uptime_ms() + 1000;
                        stack.lingering.push((handle, deadline));
                    }
                    _ => {
                        tcp.close();
                        let deadline = task::uptime_ms() + LINGER_MS;
                        stack.lingering.push((handle, deadline));
                    }
                }
            }
        }
    });
}

pub fn proto(id: u32) -> Option<Proto> {
    with_table(|table| table.get(&id).map(|s| s.proto))
}

fn port_taken(table: &BTreeMap<u32, Socket>, me: u32, proto: Proto, port: u16) -> bool {
    table.iter().any(|(id, s)| *id != me && s.proto == proto && s.bound.map(|b| b.1 == port).unwrap_or(false) && (proto == Proto::Udp || s.phase == Phase::Listening))
}

fn ensure_udp(stack: &mut Stack, sock: &mut Socket) -> Result<SocketHandle, i64> {
    if let Some(h) = live(stack, sock) {
        return Ok(h);
    }
    let port = match sock.bound {
        Some((_, p)) if p != 0 => p,
        _ => stack::next_port(stack),
    };
    let mut udp = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKETS], vec![0; UDP_BYTES]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKETS], vec![0; UDP_BYTES]),
    );
    udp.bind(IpListenEndpoint { addr: None, port }).map_err(|_| EINVAL)?;
    let handle = stack.sockets.add(udp);
    sock.handle = Some(handle);
    sock.generation = stack.generation;
    sock.bound = Some((sock.bound.map(|b| b.0).unwrap_or([0; 4]), port));
    Ok(handle)
}

pub fn bind(id: u32, ep: Endpoint) -> Result<(), i64> {
    with_table(|table| {
        let proto = table.get(&id).ok_or(EBADF)?.proto;
        if ep.1 != 0 && port_taken(table, id, proto, ep.1) {
            return Err(EADDRINUSE);
        }
        let sock = table.get_mut(&id).ok_or(EBADF)?;
        if sock.bound.is_some() && sock.handle.is_some() {
            return Err(EINVAL);
        }
        sock.bound = Some(ep);
        if sock.proto == Proto::Udp {
            stack::with_stack(|stack| ensure_udp(stack, sock).map(|_| ())).unwrap_or(Err(ENETDOWN))?;
        }
        Ok(())
    })
}

pub fn listen(id: u32, backlog: usize) -> Result<(), i64> {
    with_socket(id, |sock| {
        if sock.proto != Proto::Tcp {
            return Err(EOPNOTSUPP);
        }
        match sock.phase {
            Phase::Listening => return Ok(()),
            Phase::Fresh => {}
            _ => return Err(EINVAL),
        }
        stack::with_stack(|stack| {
            let port = match sock.bound {
                Some((_, p)) if p != 0 => p,
                _ => stack::next_port(stack),
            };
            sock.bound = Some((sock.bound.map(|b| b.0).unwrap_or([0; 4]), port));
            let count = backlog.clamp(1, MAX_BACKLOG);
            for _ in 0..count {
                let mut tcp = new_tcp(sock);
                tcp.listen(IpListenEndpoint { addr: None, port }).map_err(|_| EADDRINUSE)?;
                sock.backlog.push(stack.sockets.add(tcp));
            }
            sock.generation = stack.generation;
            sock.phase = Phase::Listening;
            Ok(())
        })
        .unwrap_or(Err(ENETDOWN))
    })
}

pub fn connect(id: u32, remote: Option<Endpoint>) -> Result<(), i64> {
    with_socket(id, |sock| match sock.proto {
        Proto::Udp => {
            sock.peer = remote;
            if remote.is_some() {
                stack::with_stack(|stack| ensure_udp(stack, sock).map(|_| ())).unwrap_or(Err(ENETDOWN))?;
            }
            Ok(())
        }
        Proto::Tcp => {
            match sock.phase {
                Phase::Connecting => return Err(EALREADY),
                Phase::Connected => return Err(EISCONN),
                Phase::Listening => return Err(EINVAL),
                Phase::Fresh | Phase::Failed => {}
            }
            let remote = remote.ok_or(EINVAL)?;
            stack::with_stack(|stack| {
                if stack.address.is_none() {
                    return Err(ENETUNREACH);
                }
                if is_local(remote.0, stack) {
                    return Err(ECONNREFUSED);
                }
                let local = match sock.bound {
                    Some((_, p)) if p != 0 => p,
                    _ => stack::next_port(stack),
                };
                let mut tcp = new_tcp(sock);
                let cx = stack.iface.context();
                tcp.connect(cx, ip_endpoint(remote), local).map_err(|_| ENETUNREACH)?;
                let handle = stack.sockets.add(tcp);
                sock.handle = Some(handle);
                sock.generation = stack.generation;
                sock.bound = Some((sock.bound.map(|b| b.0).unwrap_or([0; 4]), local));
                sock.peer = Some(remote);
                sock.phase = Phase::Connecting;
                sock.error = 0;
                sock.connect_started = task::uptime_ms();
                Err(EINPROGRESS)
            })
            .unwrap_or(Err(ENETDOWN))
        }
    })
}

fn refresh(stack: &mut Stack, sock: &mut Socket) {
    if sock.proto != Proto::Tcp || sock.phase != Phase::Connecting {
        return;
    }
    let Some(handle) = live(stack, sock) else {
        sock.phase = Phase::Failed;
        sock.error = ENETDOWN;
        return;
    };
    let tcp = stack.sockets.get_mut::<tcp::Socket>(handle);
    match tcp.state() {
        tcp::State::SynSent | tcp::State::SynReceived => {
            if task::uptime_ms().saturating_sub(sock.connect_started) >= CONNECT_TIMEOUT_MS {
                tcp.abort();
                sock.phase = Phase::Failed;
                sock.error = ETIMEDOUT;
            }
        }
        tcp::State::Closed => {
            sock.phase = Phase::Failed;
            sock.error = if task::uptime_ms().saturating_sub(sock.connect_started) >= CONNECT_TIMEOUT_MS { ETIMEDOUT } else { ECONNREFUSED };
        }
        _ => sock.phase = Phase::Connected,
    }
}

pub fn connect_result(id: u32) -> Option<i64> {
    with_socket(id, |sock| {
        stack::with_stack(|stack| refresh(stack, sock));
        Ok(match sock.phase {
            Phase::Connecting => None,
            Phase::Connected => Some(0),
            Phase::Failed => Some(core::mem::replace(&mut sock.error, 0)).map(|e| if e == 0 { ECONNREFUSED } else { e }),
            _ => Some(ENOTCONN),
        })
    })
    .unwrap_or(Some(EBADF))
}

pub fn accept(id: u32) -> Result<(u32, Endpoint), i64> {
    let taken = with_socket(id, |sock| {
        if sock.phase != Phase::Listening {
            return Err(EINVAL);
        }
        let nodelay = sock.nodelay;
        let keepalive = sock.keepalive;
        stack::with_stack(|stack| {
            if sock.generation != stack.generation {
                return Err(ENETDOWN);
            }
            let port = sock.bound.map(|b| b.1).unwrap_or(0);
            let mut found = None;
            for (i, h) in sock.backlog.iter().enumerate() {
                let tcp = stack.sockets.get_mut::<tcp::Socket>(*h);
                match tcp.state() {
                    tcp::State::Listen | tcp::State::SynReceived => {}
                    tcp::State::Closed | tcp::State::TimeWait => {
                        let _ = tcp.listen(IpListenEndpoint { addr: None, port });
                    }
                    _ => {
                        found = Some(i);
                        break;
                    }
                }
            }
            let Some(index) = found else {
                return Err(EAGAIN);
            };
            let handle = sock.backlog.remove(index);
            let peer = stack.sockets.get::<tcp::Socket>(handle).remote_endpoint().map(endpoint_of).unwrap_or(([0; 4], 0));
            let mut fresh = new_tcp(sock);
            if fresh.listen(IpListenEndpoint { addr: None, port }).is_ok() {
                sock.backlog.push(stack.sockets.add(fresh));
            }
            Ok((handle, peer, stack.generation, sock.bound, nodelay, keepalive))
        })
        .unwrap_or(Err(ENETDOWN))
    })?;
    let (handle, peer, generation, bound, nodelay, keepalive) = taken;
    let new_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_table(|table| {
        table.insert(
            new_id,
            Socket {
                refs: 1,
                proto: Proto::Tcp,
                phase: Phase::Connected,
                handle: Some(handle),
                generation,
                backlog: Vec::new(),
                bound,
                peer: Some(peer),
                error: 0,
                connect_started: 0,
                shut_read: false,
                shut_write: false,
                nodelay,
                keepalive,
                recv_timeout_ms: 0,
                send_timeout_ms: 0,
            },
        )
    });
    OPEN.fetch_add(1, Ordering::Relaxed);
    Ok((new_id, peer))
}

pub fn send(id: u32, data: &[u8], dest: Option<Endpoint>) -> Result<usize, i64> {
    with_socket(id, |sock| {
        stack::with_stack(|stack| {
            refresh(stack, sock);
            match sock.proto {
                Proto::Tcp => {
                    if sock.shut_write {
                        return Err(EPIPE);
                    }
                    match sock.phase {
                        Phase::Connected => {}
                        Phase::Connecting => return Err(EAGAIN),
                        Phase::Failed => return Err(if sock.error != 0 { core::mem::replace(&mut sock.error, 0) } else { EPIPE }),
                        _ => return Err(ENOTCONN),
                    }
                    let handle = live(stack, sock).ok_or(ECONNRESET)?;
                    let tcp = stack.sockets.get_mut::<tcp::Socket>(handle);
                    if !tcp.may_send() {
                        return Err(EPIPE);
                    }
                    if !tcp.can_send() {
                        return Err(EAGAIN);
                    }
                    match tcp.send_slice(data) {
                        Ok(0) if !data.is_empty() => Err(EAGAIN),
                        Ok(n) => Ok(n),
                        Err(_) => Err(EPIPE),
                    }
                }
                Proto::Udp => {
                    let target = dest.or(sock.peer).ok_or(EDESTADDRREQ)?;
                    let handle = ensure_udp(stack, sock)?;
                    let udp = stack.sockets.get_mut::<udp::Socket>(handle);
                    if data.len() > udp.payload_send_capacity() || data.len() > 65507 {
                        return Err(EMSGSIZE);
                    }
                    match udp.send_slice(data, ip_endpoint(target)) {
                        Ok(()) => Ok(data.len()),
                        Err(udp::SendError::BufferFull) => Err(EAGAIN),
                        Err(_) => Err(ENETUNREACH),
                    }
                }
            }
        })
        .unwrap_or(Err(ENETDOWN))
    })
}

pub fn recv(id: u32, max: usize, peek: bool) -> Result<Datagram, i64> {
    with_socket(id, |sock| {
        stack::with_stack(|stack| {
            refresh(stack, sock);
            match sock.proto {
                Proto::Tcp => {
                    match sock.phase {
                        Phase::Connected => {}
                        Phase::Connecting => return Err(EAGAIN),
                        Phase::Listening => return Err(EINVAL),
                        Phase::Failed => {
                            let e = core::mem::replace(&mut sock.error, 0);
                            return if e != 0 { Err(e) } else { Ok(Datagram { data: Vec::new(), full_len: 0, from: None }) };
                        }
                        Phase::Fresh => return Err(ENOTCONN),
                    }
                    if sock.shut_read {
                        return Ok(Datagram { data: Vec::new(), full_len: 0, from: None });
                    }
                    let Some(handle) = live(stack, sock) else {
                        return Err(ECONNRESET);
                    };
                    let tcp = stack.sockets.get_mut::<tcp::Socket>(handle);
                    if tcp.can_recv() {
                        let mut buf = vec![0u8; max.min(tcp.recv_queue()).max(1)];
                        let n = if peek { tcp.peek_slice(&mut buf) } else { tcp.recv_slice(&mut buf) }.unwrap_or(0);
                        buf.truncate(n);
                        return Ok(Datagram { full_len: n, data: buf, from: sock.peer });
                    }
                    if !tcp.may_recv() || tcp.state() == tcp::State::Closed {
                        return Ok(Datagram { data: Vec::new(), full_len: 0, from: None });
                    }
                    Err(EAGAIN)
                }
                Proto::Udp => {
                    let Some(handle) = live(stack, sock) else {
                        return Err(EAGAIN);
                    };
                    let peer = sock.peer;
                    let udp = stack.sockets.get_mut::<udp::Socket>(handle);
                    loop {
                        if !udp.can_recv() {
                            return Err(EAGAIN);
                        }
                        let (payload, from) = if peek {
                            let (p, meta) = udp.peek().map_err(|_| EAGAIN)?;
                            (p.to_vec(), endpoint_of(meta.endpoint))
                        } else {
                            let (p, meta) = udp.recv().map_err(|_| EAGAIN)?;
                            (p.to_vec(), endpoint_of(meta.endpoint))
                        };
                        if let Some(expected) = peer {
                            if expected != from && !peek {
                                continue;
                            }
                        }
                        let full_len = payload.len();
                        let mut data = payload;
                        data.truncate(max);
                        return Ok(Datagram { data, full_len, from: Some(from) });
                    }
                }
            }
        })
        .unwrap_or(Err(ENETDOWN))
    })
}

pub fn readiness(id: u32) -> Ready {
    let gone = Ready { readable: true, writable: true, hangup: true, error: true, pending: 0 };
    with_table(|table| {
        let Some(sock) = table.get_mut(&id) else {
            return gone;
        };
        stack::with_stack(|stack| {
            refresh(stack, sock);
            match (sock.proto, sock.phase) {
                (Proto::Tcp, Phase::Fresh) => Ready { readable: false, writable: true, hangup: true, error: false, pending: 0 },
                (Proto::Tcp, Phase::Connecting) => Ready { readable: false, writable: false, hangup: false, error: false, pending: 0 },
                (Proto::Tcp, Phase::Failed) => Ready { readable: true, writable: true, hangup: true, error: sock.error != 0, pending: 0 },
                (Proto::Tcp, Phase::Listening) => {
                    let ready = sock.generation == stack.generation
                        && sock.backlog.iter().any(|h| !matches!(stack.sockets.get::<tcp::Socket>(*h).state(), tcp::State::Listen | tcp::State::SynReceived | tcp::State::Closed | tcp::State::TimeWait));
                    Ready { readable: ready, writable: false, hangup: false, error: false, pending: 0 }
                }
                (Proto::Tcp, Phase::Connected) => {
                    let Some(handle) = live(stack, sock) else {
                        return Ready { readable: true, writable: true, hangup: true, error: true, pending: 0 };
                    };
                    let tcp = stack.sockets.get::<tcp::Socket>(handle);
                    let eof = !tcp.may_recv() || sock.shut_read;
                    let closed = tcp.state() == tcp::State::Closed;
                    Ready {
                        readable: tcp.can_recv() || eof || closed,
                        writable: tcp.can_send() || !tcp.may_send(),
                        hangup: closed || (eof && !tcp.may_send()),
                        error: false,
                        pending: tcp.recv_queue(),
                    }
                }
                (Proto::Udp, _) => match live(stack, sock) {
                    Some(handle) => {
                        let udp = stack.sockets.get::<udp::Socket>(handle);
                        Ready { readable: udp.can_recv(), writable: udp.can_send(), hangup: false, error: false, pending: udp.recv_queue() }
                    }
                    None => Ready { readable: false, writable: true, hangup: false, error: false, pending: 0 },
                },
            }
        })
        .unwrap_or(Ready { readable: sock.phase == Phase::Connected, writable: true, hangup: sock.phase == Phase::Connected, error: sock.phase == Phase::Connected, pending: 0 })
    })
}

pub fn shutdown(id: u32, how: u64) -> Result<(), i64> {
    with_socket(id, |sock| {
        if how > 2 {
            return Err(EINVAL);
        }
        if sock.proto == Proto::Tcp && sock.phase != Phase::Connected {
            return Err(ENOTCONN);
        }
        if how == 0 || how == 2 {
            sock.shut_read = true;
        }
        if (how == 1 || how == 2) && !sock.shut_write {
            sock.shut_write = true;
            if sock.proto == Proto::Tcp {
                stack::with_stack(|stack| {
                    if let Some(h) = live(stack, sock) {
                        stack.sockets.get_mut::<tcp::Socket>(h).close();
                    }
                });
            }
        }
        Ok(())
    })
}

pub fn names(id: u32) -> Result<(Endpoint, Option<Endpoint>), i64> {
    with_socket(id, |sock| {
        let own_ip = stack::ipv4().map(|a| a.0).unwrap_or([0; 4]);
        let local = stack::with_stack(|stack| match live(stack, sock) {
            Some(h) if sock.proto == Proto::Tcp => stack.sockets.get::<tcp::Socket>(h).local_endpoint().map(endpoint_of),
            Some(h) => {
                let port = stack.sockets.get::<udp::Socket>(h).endpoint().port;
                Some((if sock.peer.is_some() { own_ip } else { sock.bound.map(|b| b.0).unwrap_or([0; 4]) }, port))
            }
            None => None,
        })
        .flatten()
        .or(sock.bound)
        .unwrap_or(([0; 4], 0));
        let peer = match sock.proto {
            Proto::Tcp if sock.phase == Phase::Connected => sock.peer,
            Proto::Udp => sock.peer,
            _ => None,
        };
        Ok((local, peer))
    })
}

pub fn take_error(id: u32) -> i64 {
    with_socket(id, |sock| {
        stack::with_stack(|stack| refresh(stack, sock));
        Ok(core::mem::replace(&mut sock.error, 0))
    })
    .unwrap_or(EBADF)
}

pub fn is_listening(id: u32) -> bool {
    with_table(|table| table.get(&id).map(|s| s.phase == Phase::Listening).unwrap_or(false))
}

pub fn set_nodelay(id: u32, on: bool) {
    let _ = with_socket(id, |sock| {
        sock.nodelay = on;
        stack::with_stack(|stack| {
            if let (Proto::Tcp, Some(h)) = (sock.proto, live(stack, sock)) {
                stack.sockets.get_mut::<tcp::Socket>(h).set_nagle_enabled(!on);
            }
        });
        Ok(())
    });
}

pub fn nodelay(id: u32) -> bool {
    with_table(|table| table.get(&id).map(|s| s.nodelay).unwrap_or(false))
}

pub fn set_keepalive(id: u32, on: bool) {
    let _ = with_socket(id, |sock| {
        sock.keepalive = on;
        stack::with_stack(|stack| {
            if let (Proto::Tcp, Some(h)) = (sock.proto, live(stack, sock)) {
                stack.sockets.get_mut::<tcp::Socket>(h).set_keep_alive(if on { Some(Duration::from_secs(60)) } else { None });
            }
        });
        Ok(())
    });
}

pub fn keepalive(id: u32) -> bool {
    with_table(|table| table.get(&id).map(|s| s.keepalive).unwrap_or(false))
}

pub fn set_timeouts(id: u32, recv_ms: Option<u64>, send_ms: Option<u64>) {
    let _ = with_socket(id, |sock| {
        if let Some(r) = recv_ms {
            sock.recv_timeout_ms = r;
        }
        if let Some(s) = send_ms {
            sock.send_timeout_ms = s;
        }
        Ok(())
    });
}

pub fn timeouts(id: u32) -> (u64, u64) {
    with_table(|table| table.get(&id).map(|s| (s.recv_timeout_ms, s.send_timeout_ms)).unwrap_or((0, 0)))
}

pub fn kick() {
    stack::poll();
}
