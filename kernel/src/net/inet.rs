use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use spin::Mutex;

use smoltcp::iface::SocketHandle;
use smoltcp::socket::{icmp, tcp, udp};
use smoltcp::time::Duration;
use smoltcp::wire::{Icmpv4Packet, Icmpv4Repr, IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address};

use super::stack::{self, Stack};
use crate::arch::without_interrupts;
use crate::task;

pub const ID_BASE: u32 = 0x4000_0000;

pub use crate::errno::EBADF;
pub use crate::errno::EAGAIN;
pub use crate::errno::EINVAL;
pub use crate::errno::EPIPE;
pub use crate::errno::EDESTADDRREQ;
pub use crate::errno::EMSGSIZE;
pub use crate::errno::EOPNOTSUPP;
pub use crate::errno::EADDRINUSE;
pub use crate::errno::ENETDOWN;
pub use crate::errno::ENETUNREACH;
pub use crate::errno::ECONNRESET;
pub use crate::errno::EISCONN;
pub use crate::errno::ENOTCONN;
pub use crate::errno::ETIMEDOUT;
pub use crate::errno::ECONNREFUSED;
pub use crate::errno::EALREADY;
pub use crate::errno::EINPROGRESS;

const TCP_RX_BUFFER: usize = 256 * 1024;
const TCP_TX_BUFFER: usize = 128 * 1024;
const UDP_PACKETS: usize = 64;
const UDP_BYTES: usize = 128 * 1024;
const CONNECT_TIMEOUT_MS: u64 = 30_000;
const IDLE_TIMEOUT_S: u64 = 120;
const LINGER_MS: u64 = 20_000;
const MAX_BACKLOG: usize = 16;
const LOOP_BUFFER: usize = 256 * 1024;
const LOOP_PORT_BASE: u16 = 40000;

pub type Endpoint = ([u8; 4], u16);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Udp,
    Icmp,
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
    loop_peer: Option<u32>,
    loop_rx: VecDeque<u8>,
    loop_eof: bool,
    loop_pending: Vec<(u32, Endpoint)>,
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
static LOOP_PORT: AtomicU32 = AtomicU32::new(0);
static ICMP_IDENT: AtomicU32 = AtomicU32::new(0x4858);
const ICMP_PACKETS: usize = 16;
const ICMP_BYTES: usize = 16 * 1024;

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

fn is_local_address(ip: [u8; 4]) -> bool {
    ip[0] == 127 || stack::ipv4().map(|a| a.0 == ip).unwrap_or(false)
}

fn loop_port() -> u16 {
    LOOP_PORT_BASE + (LOOP_PORT.fetch_add(1, Ordering::Relaxed) % 20000) as u16
}

fn fresh_socket(proto: Proto, phase: Phase) -> Socket {
    Socket {
        refs: 1,
        proto,
        phase,
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
        loop_peer: None,
        loop_rx: VecDeque::new(),
        loop_eof: false,
        loop_pending: Vec::new(),
    }
}

fn connect_loopback(id: u32, remote: Endpoint) -> Result<(), i64> {
    let server = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let result = with_table(|table| {
        let me = table.get(&id).ok_or(EBADF)?;
        match me.phase {
            Phase::Connected => return Err(EISCONN),
            Phase::Listening => return Err(EINVAL),
            _ => {}
        }
        let local = (remote.0, me.bound.map(|b| b.1).filter(|p| *p != 0).unwrap_or_else(loop_port));
        let listener = table
            .iter()
            .find(|(_, s)| s.proto == Proto::Tcp && s.phase == Phase::Listening && s.bound.map(|b| b.1 == remote.1 && (b.0 == [0; 4] || b.0 == remote.0 || (b.0[0] == 127 && remote.0[0] == 127))).unwrap_or(false))
            .map(|(k, _)| *k)
            .ok_or(ECONNREFUSED)?;
        let mut peer = fresh_socket(Proto::Tcp, Phase::Connected);
        peer.bound = Some(remote);
        peer.peer = Some(local);
        peer.loop_peer = Some(id);
        table.insert(server, peer);
        if let Some(l) = table.get_mut(&listener) {
            l.loop_pending.push((server, local));
        }
        let me = table.get_mut(&id).ok_or(EBADF)?;
        me.phase = Phase::Connected;
        me.bound = Some(local);
        me.peer = Some(remote);
        me.loop_peer = Some(server);
        me.loop_eof = false;
        me.error = 0;
        Ok(())
    });
    if result.is_ok() {
        OPEN.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub fn create(proto: Proto) -> u32 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_table(|table| {
        table.insert(id, fresh_socket(proto, Phase::Fresh))
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
        let removed = if last { table.remove(&id) } else { None };
        if let Some(sock) = removed.as_ref() {
            if let Some(peer) = sock.loop_peer.and_then(|p| table.get_mut(&p)) {
                peer.loop_eof = true;
                peer.loop_peer = None;
            }
            for (pending, _) in sock.loop_pending.iter() {
                table.remove(pending);
            }
        }
        removed
    });
    let Some(sock) = gone else {
        return;
    };
    OPEN.fetch_sub(1, Ordering::Relaxed);
    if sock.handle.is_none() && sock.backlog.is_empty() {
        return;
    }
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
            Proto::Udp | Proto::Icmp => {
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

fn ensure_icmp(stack: &mut Stack, sock: &mut Socket) -> Result<SocketHandle, i64> {
    if let Some(h) = live(stack, sock) {
        return Ok(h);
    }
    let ident = match sock.bound {
        Some((_, p)) if p != 0 => p,
        _ => ICMP_IDENT.fetch_add(1, Ordering::Relaxed) as u16,
    };
    let mut socket = icmp::Socket::new(
        icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; ICMP_PACKETS], vec![0; ICMP_BYTES]),
        icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; ICMP_PACKETS], vec![0; ICMP_BYTES]),
    );
    socket.bind(icmp::Endpoint::Ident(ident)).map_err(|_| EINVAL)?;
    let handle = stack.sockets.add(socket);
    sock.handle = Some(handle);
    sock.generation = stack.generation;
    sock.bound = Some((sock.bound.map(|b| b.0).unwrap_or([0; 4]), ident));
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
        if sock.proto == Proto::Icmp {
            stack::with_stack(|stack| ensure_icmp(stack, sock).map(|_| ())).unwrap_or(Err(ENETDOWN))?;
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
        let listened = stack::with_stack(|stack| {
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
        });
        match listened {
            Some(result) => result,
            None => {
                let port = sock.bound.map(|b| b.1).filter(|p| *p != 0).unwrap_or_else(loop_port);
                sock.bound = Some((sock.bound.map(|b| b.0).unwrap_or([0; 4]), port));
                sock.phase = Phase::Listening;
                Ok(())
            }
        }
    })
}

pub fn connect(id: u32, remote: Option<Endpoint>) -> Result<(), i64> {
    if let Some(target) = remote {
        if proto(id) == Some(Proto::Tcp) && is_local_address(target.0) {
            return connect_loopback(id, target);
        }
    }
    with_socket(id, |sock| match sock.proto {
        Proto::Icmp => {
            sock.peer = remote;
            Ok(())
        }
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
    let looped = with_socket(id, |sock| {
        if sock.phase != Phase::Listening {
            return Err(EINVAL);
        }
        Ok(if sock.loop_pending.is_empty() { None } else { Some(sock.loop_pending.remove(0)) })
    })?;
    if let Some(connection) = looped {
        return Ok(connection);
    }
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
                loop_peer: None,
                loop_rx: VecDeque::new(),
                loop_eof: false,
                loop_pending: Vec::new(),
            },
        )
    });
    OPEN.fetch_add(1, Ordering::Relaxed);
    Ok((new_id, peer))
}

fn loop_send(id: u32, data: &[u8]) -> Option<Result<usize, i64>> {
    with_table(|table| {
        let me = table.get(&id)?;
        if me.phase != Phase::Connected || (me.loop_peer.is_none() && !me.loop_eof) {
            return None;
        }
        if me.shut_write {
            return Some(Err(EPIPE));
        }
        let Some(peer_id) = me.loop_peer else {
            return Some(Err(EPIPE));
        };
        let Some(peer) = table.get_mut(&peer_id) else {
            return Some(Err(EPIPE));
        };
        if peer.shut_read {
            return Some(Ok(data.len()));
        }
        let room = LOOP_BUFFER.saturating_sub(peer.loop_rx.len());
        if room == 0 {
            return Some(Err(EAGAIN));
        }
        let n = room.min(data.len());
        peer.loop_rx.extend(&data[..n]);
        Some(Ok(n))
    })
}

fn loop_recv(id: u32, max: usize, peek: bool) -> Option<Result<Datagram, i64>> {
    with_table(|table| {
        let me = table.get_mut(&id)?;
        if me.phase != Phase::Connected || (me.loop_peer.is_none() && !me.loop_eof) {
            return None;
        }
        if me.shut_read {
            return Some(Ok(Datagram { data: Vec::new(), full_len: 0, from: None }));
        }
        if me.loop_rx.is_empty() {
            return Some(if me.loop_eof { Ok(Datagram { data: Vec::new(), full_len: 0, from: None }) } else { Err(EAGAIN) });
        }
        let n = max.max(1).min(me.loop_rx.len());
        let data: Vec<u8> = if peek { me.loop_rx.iter().take(n).copied().collect() } else { me.loop_rx.drain(..n).collect() };
        Some(Ok(Datagram { full_len: data.len(), data, from: me.peer }))
    })
}

pub fn send(id: u32, data: &[u8], dest: Option<Endpoint>) -> Result<usize, i64> {
    if let Some(result) = loop_send(id, data) {
        return result;
    }
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
                Proto::Icmp => {
                    let target = dest.or(sock.peer).ok_or(EDESTADDRREQ)?;
                    if data.len() < 8 || data[0] != 8 {
                        return Err(EINVAL);
                    }
                    let handle = ensure_icmp(stack, sock)?;
                    let ident = sock.bound.map(|b| b.1).unwrap_or(0);
                    let seq_no = u16::from_be_bytes([data[6], data[7]]);
                    let repr = Icmpv4Repr::EchoRequest { ident, seq_no, data: &data[8..] };
                    let caps = smoltcp::phy::ChecksumCapabilities::default();
                    let socket = stack.sockets.get_mut::<icmp::Socket>(handle);
                    match socket.send(repr.buffer_len(), IpAddress::Ipv4(Ipv4Address::from(target.0))) {
                        Ok(buf) => {
                            repr.emit(&mut Icmpv4Packet::new_unchecked(buf), &caps);
                            Ok(data.len())
                        }
                        Err(icmp::SendError::BufferFull) => Err(EAGAIN),
                        Err(_) => Err(ENETUNREACH),
                    }
                }
            }
        })
        .unwrap_or(Err(ENETDOWN))
    })
}

pub fn recv(id: u32, max: usize, peek: bool) -> Result<Datagram, i64> {
    if let Some(result) = loop_recv(id, max, peek) {
        return result;
    }
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
                Proto::Icmp => {
                    let Some(handle) = live(stack, sock) else {
                        return Err(EAGAIN);
                    };
                    let socket = stack.sockets.get_mut::<icmp::Socket>(handle);
                    if !socket.can_recv() {
                        return Err(EAGAIN);
                    }
                    if peek {
                        return Err(EOPNOTSUPP);
                    }
                    let (payload, from) = socket.recv().map(|(p, addr)| (p.to_vec(), addr)).map_err(|_| EAGAIN)?;
                    let IpAddress::Ipv4(v4) = from;
                    let full_len = payload.len();
                    let mut data = payload;
                    data.truncate(max);
                    Ok(Datagram { data, full_len, from: Some((v4.octets(), 0)) })
                }
            }
        })
        .unwrap_or(Err(ENETDOWN))
    })
}

pub fn readiness(id: u32) -> Ready {
    let gone = Ready { readable: true, writable: true, hangup: true, error: true, pending: 0 };
    with_table(|table| {
        let looped = match table.get(&id) {
            None => return gone,
            Some(sock) if sock.phase == Phase::Connected && (sock.loop_peer.is_some() || sock.loop_eof) => {
                let room = sock.loop_peer.and_then(|p| table.get(&p)).map(|p| p.loop_rx.len() < LOOP_BUFFER).unwrap_or(true);
                Some(Ready {
                    readable: !sock.loop_rx.is_empty() || sock.loop_eof || sock.shut_read,
                    writable: room,
                    hangup: sock.loop_eof && sock.loop_peer.is_none(),
                    error: false,
                    pending: sock.loop_rx.len(),
                })
            }
            Some(sock) if sock.phase == Phase::Listening && !sock.loop_pending.is_empty() => Some(Ready { readable: true, writable: false, hangup: false, error: false, pending: 0 }),
            Some(_) => None,
        };
        if let Some(ready) = looped {
            return ready;
        }
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
                (Proto::Icmp, _) => match live(stack, sock) {
                    Some(handle) => {
                        let socket = stack.sockets.get::<icmp::Socket>(handle);
                        Ready { readable: socket.can_recv(), writable: socket.can_send(), hangup: false, error: false, pending: 0 }
                    }
                    None => Ready { readable: false, writable: true, hangup: false, error: false, pending: 0 },
                },
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
            if sock.loop_peer.is_some() {
                return Ok(());
            }
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
            Some(_) if sock.proto == Proto::Icmp => sock.bound,
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
            Proto::Udp | Proto::Icmp => sock.peer,
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
