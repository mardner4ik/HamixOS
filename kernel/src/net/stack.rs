use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{dhcpv4, dns, icmp, tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint, Ipv4Address, Ipv4Cidr};

use super::{with_devices, NetDevice};
use crate::task::{self, Pid};

pub const EINVAL: i64 = -22;
pub const EBADF: i64 = -9;
pub const ENETDOWN: i64 = -100;
pub const ETIMEDOUT: i64 = -110;
pub const ECONNREFUSED: i64 = -111;
pub const EHOSTUNREACH: i64 = -113;
pub const ENOTCONN: i64 = -107;
pub const EAGAIN: i64 = -11;
pub const ENOENT: i64 = -2;

const LINK_DOWN_GRACE_MS: u64 = 3000;
const DHCP_RETRY_MS: u64 = 20000;

static GENERATION: AtomicU64 = AtomicU64::new(1);
static CHANGED: AtomicBool = AtomicBool::new(false);
static DNS_DIRTY: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub struct Snapshot {
    pub address: String,
    pub gateway: String,
    pub dns: String,
    pub state: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SockKind {
    Tcp,
    Udp,
}

struct UserSocket {
    owner: Pid,
    handle: SocketHandle,
    kind: SockKind,
}

pub(super) struct Stack {
    device: usize,
    pub(super) iface: Interface,
    pub(super) sockets: SocketSet<'static>,
    pub(super) generation: u64,
    pub(super) lingering: Vec<(SocketHandle, u64)>,
    dhcp: Option<SocketHandle>,
    dns: SocketHandle,
    pub(super) address: Option<Ipv4Cidr>,
    gateway: Option<Ipv4Address>,
    dns_servers: Vec<Ipv4Address>,
    static_config: bool,
    user: BTreeMap<u32, UserSocket>,
    next_handle: u32,
    next_port: u16,
    link: bool,
    link_raw: bool,
    link_changed_at: u64,
    dhcp_restart_at: u64,
}

static STACK: Mutex<Option<Stack>> = Mutex::new(None);
static ACTIVE: Mutex<Option<usize>> = Mutex::new(None);

fn now() -> Instant {
    Instant::from_millis(task::uptime_ms() as i64)
}

struct Phy<'a> {
    dev: &'a mut dyn NetDevice,
}

struct Rx(Vec<u8>);

struct Tx<'a> {
    dev: &'a mut dyn NetDevice,
}

fn dhcp_message_name(kind: u8) -> &'static str {
    match kind {
        1 => "DISCOVER",
        2 => "OFFER",
        3 => "REQUEST",
        4 => "DECLINE",
        5 => "ACK",
        6 => "NAK",
        7 => "RELEASE",
        8 => "INFORM",
        _ => "message",
    }
}

fn trace_dhcp(frame: &[u8], outgoing: bool) {
    if frame.len() < 14 + 20 + 8 + 240 || frame[12..14] != [0x08, 0x00] {
        return;
    }
    let ip = &frame[14..];
    let ihl = (ip[0] & 0x0F) as usize * 4;
    if ip[9] != 17 || ip.len() < ihl + 8 + 240 {
        return;
    }
    let udp = &ip[ihl..];
    let (src, dst) = (u16::from_be_bytes([udp[0], udp[1]]), u16::from_be_bytes([udp[2], udp[3]]));
    if !((src == 68 && dst == 67) || (src == 67 && dst == 68)) {
        return;
    }
    let dhcp = &udp[8..];
    if dhcp[236..240] != [99, 130, 83, 99] {
        return;
    }
    let mut kind = 0u8;
    let mut i = 240;
    while i + 1 < dhcp.len() {
        let code = dhcp[i];
        if code == 255 {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = dhcp[i + 1] as usize;
        if code == 53 && len >= 1 && i + 2 < dhcp.len() {
            kind = dhcp[i + 2];
            break;
        }
        i += 2 + len;
    }
    let offered = Ipv4Address::new(dhcp[16], dhcp[17], dhcp[18], dhcp[19]);
    let from = Ipv4Address::new(ip[12], ip[13], ip[14], ip[15]);
    let to_mac = super::dma::mac_text(&[frame[0], frame[1], frame[2], frame[3], frame[4], frame[5]]);
    if outgoing {
        crate::drivers::klog::log(&format!("net: DHCP {} sent (xid {:02x}{:02x}{:02x}{:02x})", dhcp_message_name(kind), dhcp[4], dhcp[5], dhcp[6], dhcp[7]));
    } else {
        crate::drivers::klog::log(&format!("net: DHCP {} received from {} to {} offering {} (xid {:02x}{:02x}{:02x}{:02x})", dhcp_message_name(kind), from, to_mac, offered, dhcp[4], dhcp[5], dhcp[6], dhcp[7]));
    }
}

impl RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        trace_dhcp(&buffer, true);
        self.dev.transmit(&buffer);
        result
    }
}

impl Device for Phy<'_> {
    type RxToken<'a> = Rx where Self: 'a;
    type TxToken<'a> = Tx<'a> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.dev.receive()?;
        trace_dhcp(&packet, false);
        Some((Rx(packet), Tx { dev: self.dev }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(Tx { dev: self.dev })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps.max_burst_size = Some(16);
        caps
    }
}

pub(super) fn with_stack<R>(f: impl FnOnce(&mut Stack) -> R) -> Option<R> {
    crate::arch::without_interrupts(|| STACK.lock().as_mut().map(f))
}

pub fn active_device() -> Option<usize> {
    *ACTIVE.lock()
}

pub fn rebuild(device: Option<usize>) {
    crate::arch::without_interrupts(|| *STACK.lock() = None);
    *ACTIVE.lock() = device;
    let Some(index) = device else {
        return;
    };
    let stack = with_devices(|devices| {
        let dev = devices.get_mut(index)?;
        let mac = dev.mac();
        let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
        config.random_seed = crate::task::ticks() ^ crate::drivers::rtc::now() ^ u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0, 0]);
        let mut phy = Phy { dev: dev.as_mut() };
        let iface = Interface::new(config, &mut phy, now());
        let mut sockets = SocketSet::new(Vec::new());
        let dhcp = sockets.add(dhcpv4::Socket::new());
        let dns = sockets.add(dns::Socket::new(&[], Vec::new()));
        Some(Stack {
            device: index,
            iface,
            sockets,
            generation: GENERATION.fetch_add(1, Ordering::Relaxed) + 1,
            lingering: Vec::new(),
            dhcp: Some(dhcp),
            dns,
            address: None,
            gateway: None,
            dns_servers: Vec::new(),
            static_config: false,
            user: BTreeMap::new(),
            next_handle: 1,
            next_port: 49152 + (crate::task::ticks() % 4096) as u16,
            link: false,
            link_raw: false,
            link_changed_at: 0,
            dhcp_restart_at: 0,
        })
    });
    crate::arch::without_interrupts(|| *STACK.lock() = stack);
}

fn apply_address(stack: &mut Stack, address: Option<Ipv4Cidr>, gateway: Option<Ipv4Address>, dns_servers: Vec<Ipv4Address>) {
    stack.address = address;
    stack.gateway = gateway;
    stack.iface.update_ip_addrs(|addrs| {
        addrs.clear();
        if let Some(cidr) = address {
            let _ = addrs.push(IpCidr::Ipv4(cidr));
        }
    });
    stack.iface.routes_mut().remove_default_ipv4_route();
    if let Some(router) = gateway {
        let _ = stack.iface.routes_mut().add_default_ipv4_route(router);
    }
    if stack.dns_servers != dns_servers && !dns_servers.is_empty() {
        DNS_DIRTY.store(true, Ordering::Relaxed);
    }
    let servers: Vec<IpAddress> = dns_servers.iter().map(|s| IpAddress::Ipv4(*s)).collect();
    stack.sockets.get_mut::<dns::Socket>(stack.dns).update_servers(&servers);
    stack.dns_servers = dns_servers;
}

pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

pub fn take_dns_dirty() -> bool {
    DNS_DIRTY.swap(false, Ordering::Relaxed)
}

pub fn dns_servers() -> Vec<[u8; 4]> {
    with_stack(|stack| stack.dns_servers.iter().map(|d| d.octets()).collect()).unwrap_or_default()
}

fn reap_lingering(stack: &mut Stack, now_ms: u64) {
    let mut i = 0;
    while i < stack.lingering.len() {
        let (handle, deadline) = stack.lingering[i];
        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        match socket.state() {
            tcp::State::Closed | tcp::State::TimeWait => {
                stack.sockets.remove(handle);
                stack.lingering.swap_remove(i);
                continue;
            }
            _ if now_ms >= deadline => {
                socket.abort();
                stack.lingering[i].1 = u64::MAX;
            }
            _ => {}
        }
        i += 1;
    }
}

pub fn ipv4() -> Option<([u8; 4], u8)> {
    with_stack(|stack| stack.address.map(|a| (a.address().octets(), a.prefix_len()))).flatten()
}

pub fn config() -> Option<Snapshot> {
    with_stack(|stack| {
        let state = if !stack.link {
            String::from("cable unplugged")
        } else if stack.address.is_none() && !stack.link_raw {
            String::from("link is unstable, reconnecting…")
        } else if stack.address.is_some() {
            String::from(if stack.static_config { "configured (static)" } else { "configured by DHCP" })
        } else {
            String::from("obtaining an address (DHCP)…")
        };
        Snapshot {
            address: stack.address.map(|a| format!("{}/{}", a.address(), a.prefix_len())).unwrap_or_default(),
            gateway: stack.gateway.map(|g| format!("{}", g)).unwrap_or_default(),
            dns: stack.dns_servers.iter().map(|d| format!("{}", d)).collect::<Vec<_>>().join(","),
            state,
        }
    })
}

pub fn poll() -> bool {
    let Some(index) = active_device() else {
        with_devices(|devices| {
            for dev in devices.iter_mut() {
                dev.poll();
            }
        });
        return false;
    };
    with_devices(|devices| {
        for (i, dev) in devices.iter_mut().enumerate() {
            if i != index {
                dev.poll();
            }
        }
        let Some(dev) = devices.get_mut(index) else {
            return false;
        };
        dev.poll();
        let link = dev.link_up();
        crate::arch::without_interrupts(|| {
            let mut guard = STACK.lock();
            let Some(stack) = guard.as_mut() else {
                return false;
            };
            if stack.device != index {
                return false;
            }
            let now_ms = task::uptime_ms();
            if link != stack.link_raw {
                stack.link_raw = link;
                stack.link_changed_at = now_ms;
            }
            let settled = link || now_ms.saturating_sub(stack.link_changed_at) >= LINK_DOWN_GRACE_MS;
            if settled && link != stack.link {
                stack.link = link;
                if let Some(h) = stack.dhcp {
                    stack.sockets.get_mut::<dhcpv4::Socket>(h).reset();
                }
                if link {
                    stack.dhcp_restart_at = now_ms + DHCP_RETRY_MS;
                } else if !stack.static_config {
                    apply_address(stack, None, None, Vec::new());
                }
            }
            if !stack.link {
                while dev.receive().is_some() {}
                return false;
            }
            if stack.address.is_none() && !stack.static_config && stack.dhcp_restart_at != 0 && now_ms >= stack.dhcp_restart_at {
                stack.dhcp_restart_at = now_ms + DHCP_RETRY_MS;
                if let Some(h) = stack.dhcp {
                    stack.sockets.get_mut::<dhcpv4::Socket>(h).reset();
                }
            }
            let mut phy = Phy { dev: dev.as_mut() };
            let result = stack.iface.poll(now(), &mut phy, &mut stack.sockets);
            if matches!(result, smoltcp::iface::PollResult::SocketStateChanged) {
                CHANGED.store(true, Ordering::Relaxed);
            }
            if !stack.lingering.is_empty() {
                reap_lingering(stack, now_ms);
            }
            if let Some(handle) = stack.dhcp {
                let event = match stack.sockets.get_mut::<dhcpv4::Socket>(handle).poll() {
                    Some(dhcpv4::Event::Configured(config)) => Some(Some((config.address, config.router, config.dns_servers.iter().copied().collect::<Vec<Ipv4Address>>()))),
                    Some(dhcpv4::Event::Deconfigured) => Some(None),
                    None => None,
                };
                match event {
                    Some(Some((address, router, servers))) => {
                        crate::drivers::klog::log(&format!("net: DHCP address {} router {:?} dns {:?}", address, router, servers));
                        stack.dhcp_restart_at = 0;
                        apply_address(stack, Some(address), router, servers);
                    }
                    Some(None) => {
                        stack.dhcp_restart_at = task::uptime_ms() + DHCP_RETRY_MS;
                        apply_address(stack, None, None, Vec::new());
                    }
                    None => {}
                }
            }
            matches!(result, smoltcp::iface::PollResult::SocketStateChanged) || !stack.user.is_empty() || !stack.lingering.is_empty()
        })
    })
}

pub fn configure(spec: &str) -> Result<(), &'static str> {
    with_stack(|stack| {
        let words: Vec<&str> = spec.split_whitespace().collect();
        match words.first().copied() {
            Some("dhcp") => {
                if stack.dhcp.is_none() {
                    stack.dhcp = Some(stack.sockets.add(dhcpv4::Socket::new()));
                }
                stack.static_config = false;
                apply_address(stack, None, None, Vec::new());
                Ok(())
            }
            Some("static") if words.len() >= 2 => {
                let (ip, prefix) = words[1].split_once('/').unwrap_or((words[1], "24"));
                let ip: Ipv4Address = ip.parse().map_err(|_| "bad address")?;
                let prefix: u8 = prefix.parse().map_err(|_| "bad prefix")?;
                let gateway = words.get(2).and_then(|g| g.parse().ok());
                let dns_servers: Vec<Ipv4Address> = words.get(3).map(|d| d.split(',').filter_map(|s| s.parse().ok()).collect()).unwrap_or_default();
                if let Some(h) = stack.dhcp.take() {
                    stack.sockets.remove(h);
                }
                stack.static_config = true;
                apply_address(stack, Some(Ipv4Cidr::new(ip, prefix)), gateway, dns_servers);
                Ok(())
            }
            _ => Err("usage: dhcp | static ADDRESS/PREFIX [GATEWAY] [DNS,DNS]"),
        }
    })
    .unwrap_or(Err("network is off"))
}

fn wait_until<R>(deadline: u64, mut step: impl FnMut(&mut Stack) -> Option<R>) -> Result<R, i64> {
    loop {
        match with_stack(|stack| step(stack)) {
            None => return Err(ENETDOWN),
            Some(Some(r)) => return Ok(r),
            Some(None) => {}
        }
        if task::uptime_ms() >= deadline {
            return Err(ETIMEDOUT);
        }
        poll();
        task::sleep_ticks(1);
        if task::with_current(|t| t.killed.is_some()) {
            return Err(-4);
        }
    }
}

pub fn resolve(name: &str, timeout_ms: u64) -> Result<[u8; 4], i64> {
    if let Ok(ip) = name.parse::<Ipv4Address>() {
        return Ok(ip.octets());
    }
    let deadline = task::uptime_ms() + timeout_ms;
    let query = with_stack(|stack| {
        if stack.dns_servers.is_empty() {
            return Err(EHOSTUNREACH);
        }
        let cx = stack.iface.context();
        stack.sockets.get_mut::<dns::Socket>(stack.dns).start_query(cx, name, smoltcp::wire::DnsQueryType::A).map_err(|_| EINVAL)
    })
    .ok_or(ENETDOWN)??;
    wait_until(deadline, |stack| {
        match stack.sockets.get_mut::<dns::Socket>(stack.dns).get_query_result(query) {
            Ok(addrs) => Some(addrs.iter().find_map(|a| match a {
                IpAddress::Ipv4(v4) => Some(Ok(v4.octets())),
                #[allow(unreachable_patterns)]
                _ => None,
            }).unwrap_or(Err(ENOENT))),
            Err(dns::GetQueryResultError::Pending) => None,
            Err(_) => Some(Err(ENOENT)),
        }
    })?
}

pub fn ping(ip: [u8; 4], sequence: u16, timeout_ms: u64) -> i64 {
    let target = Ipv4Address::from(ip);
    let ident = 0x4858u16 ^ (task::current_pid() as u16);
    let started = task::uptime_ms();
    let handle = with_stack(|stack| {
        if stack.address.is_none() {
            return Err(ENETDOWN);
        }
        let mut socket = icmp::Socket::new(icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; 4], vec![0; 1024]), icmp::PacketBuffer::new(vec![icmp::PacketMetadata::EMPTY; 4], vec![0; 1024]));
        socket.bind(icmp::Endpoint::Ident(ident)).map_err(|_| EINVAL)?;
        let handle = stack.sockets.add(socket);
        let repr = Icmpv4Repr::EchoRequest { ident, seq_no: sequence, data: b"HamixOS ping payload 0123456789" };
        let caps = smoltcp::phy::ChecksumCapabilities::default();
        let socket = stack.sockets.get_mut::<icmp::Socket>(handle);
        match socket.send(repr.buffer_len(), IpAddress::Ipv4(target)) {
            Ok(buf) => {
                let mut packet = Icmpv4Packet::new_unchecked(buf);
                repr.emit(&mut packet, &caps);
                Ok(handle)
            }
            Err(_) => {
                stack.sockets.remove(handle);
                Err(EAGAIN)
            }
        }
    });
    let handle = match handle {
        Some(Ok(h)) => h,
        Some(Err(e)) => return e,
        None => return ENETDOWN,
    };
    let result = wait_until(started + timeout_ms, |stack| {
        let socket = stack.sockets.get_mut::<icmp::Socket>(handle);
        while socket.can_recv() {
            let Ok((payload, from)) = socket.recv() else {
                break;
            };
            if from != IpAddress::Ipv4(target) {
                continue;
            }
            let Ok(packet) = Icmpv4Packet::new_checked(payload) else {
                continue;
            };
            if let Ok(Icmpv4Repr::EchoReply { ident: i, seq_no, .. }) = Icmpv4Repr::parse(&packet, &smoltcp::phy::ChecksumCapabilities::ignored()) {
                if i == ident && seq_no == sequence {
                    return Some(());
                }
            }
        }
        None
    });
    with_stack(|stack| stack.sockets.remove(handle));
    match result {
        Ok(()) => (task::uptime_ms() - started) as i64,
        Err(e) => e,
    }
}

pub fn socket(kind: u64, port: u16) -> i64 {
    let owner = task::current_pid();
    with_stack(|stack| {
        let (handle, kind) = match kind {
            1 => (stack.sockets.add(tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 65536]), tcp::SocketBuffer::new(vec![0; 65536]))), SockKind::Tcp),
            2 => {
                let mut socket = udp::Socket::new(udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 32], vec![0; 32768]), udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 32], vec![0; 32768]));
                let local = if port != 0 { port } else { next_port(stack) };
                if socket.bind(local).is_err() {
                    return EINVAL;
                }
                (stack.sockets.add(socket), SockKind::Udp)
            }
            _ => return EINVAL,
        };
        let id = stack.next_handle;
        stack.next_handle += 1;
        stack.user.insert(id, UserSocket { owner, handle, kind });
        id as i64
    })
    .unwrap_or(ENETDOWN)
}

pub(super) fn next_port(stack: &mut Stack) -> u16 {
    stack.next_port = if stack.next_port >= 65000 { 49152 } else { stack.next_port + 1 };
    stack.next_port
}

fn lookup(stack: &Stack, id: u64) -> Result<(SocketHandle, SockKind), i64> {
    let owner = task::current_pid();
    match stack.user.get(&(id as u32)) {
        Some(s) if s.owner == owner => Ok((s.handle, s.kind)),
        _ => Err(EBADF),
    }
}

pub fn connect(id: u64, ip: [u8; 4], port: u16, timeout_ms: u64) -> i64 {
    let deadline = task::uptime_ms() + timeout_ms.clamp(100, 120_000);
    let setup = with_stack(|stack| {
        let (handle, kind) = lookup(stack, id)?;
        if kind != SockKind::Tcp {
            return Err(EINVAL);
        }
        if stack.address.is_none() {
            return Err(ENETDOWN);
        }
        let local = next_port(stack);
        let cx = stack.iface.context();
        stack.sockets.get_mut::<tcp::Socket>(handle).connect(cx, IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from(ip)), port), local).map_err(|_| EINVAL)?;
        Ok(handle)
    });
    let handle = match setup {
        Some(Ok(h)) => h,
        Some(Err(e)) => return e,
        None => return ENETDOWN,
    };
    match wait_until(deadline, |stack| {
        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        match socket.state() {
            tcp::State::Established => Some(0),
            tcp::State::Closed => Some(ECONNREFUSED),
            _ => None,
        }
    }) {
        Ok(v) => v,
        Err(e) => e,
    }
}

pub fn listen(id: u64, port: u16) -> i64 {
    with_stack(|stack| {
        let (handle, kind) = match lookup(stack, id) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if kind != SockKind::Tcp {
            return EINVAL;
        }
        match stack.sockets.get_mut::<tcp::Socket>(handle).listen(IpListenEndpoint { addr: None, port }) {
            Ok(()) => 0,
            Err(_) => EINVAL,
        }
    })
    .unwrap_or(ENETDOWN)
}

pub fn send(id: u64, data: &[u8]) -> i64 {
    let deadline = task::uptime_ms() + 30_000;
    let result = wait_until(deadline, |stack| {
        let (handle, _) = match lookup(stack, id) {
            Ok(v) => v,
            Err(e) => return Some(e),
        };
        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        if !socket.may_send() {
            return Some(ENOTCONN);
        }
        if socket.can_send() {
            return Some(match socket.send_slice(data) {
                Ok(n) => n as i64,
                Err(_) => ENOTCONN,
            });
        }
        None
    });
    poll();
    match result {
        Ok(v) => v,
        Err(e) => e,
    }
}

pub fn recv(id: u64, out: &mut [u8], timeout_ms: u64) -> i64 {
    let deadline = task::uptime_ms() + timeout_ms;
    match wait_until(deadline, |stack| {
        let (handle, _) = match lookup(stack, id) {
            Ok(v) => v,
            Err(e) => return Some(e),
        };
        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        if socket.can_recv() {
            return Some(match socket.recv_slice(out) {
                Ok(n) => n as i64,
                Err(_) => 0,
            });
        }
        if !socket.may_recv() && socket.state() != tcp::State::Listen && socket.state() != tcp::State::SynReceived {
            return Some(0);
        }
        None
    }) {
        Ok(v) => v,
        Err(ETIMEDOUT) => EAGAIN,
        Err(e) => e,
    }
}

pub fn send_to(id: u64, ip: [u8; 4], port: u16, data: &[u8]) -> i64 {
    let r = with_stack(|stack| {
        let (handle, kind) = lookup(stack, id)?;
        if kind != SockKind::Udp {
            return Err(EINVAL);
        }
        let endpoint = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::from(ip)), port);
        stack.sockets.get_mut::<udp::Socket>(handle).send_slice(data, endpoint).map(|_| data.len() as i64).map_err(|_| EAGAIN)
    });
    poll();
    match r {
        Some(Ok(n)) => n,
        Some(Err(e)) => e,
        None => ENETDOWN,
    }
}

pub fn recv_from(id: u64, out: &mut [u8], timeout_ms: u64) -> Result<(usize, [u8; 4], u16), i64> {
    let deadline = task::uptime_ms() + timeout_ms;
    wait_until(deadline, |stack| {
        let (handle, _) = match lookup(stack, id) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let socket = stack.sockets.get_mut::<udp::Socket>(handle);
        if socket.can_recv() {
            return Some(match socket.recv_slice(out) {
                Ok((n, meta)) => {
                    let ip = match meta.endpoint.addr {
                        IpAddress::Ipv4(v4) => v4.octets(),
                        #[allow(unreachable_patterns)]
                        _ => [0; 4],
                    };
                    Ok((n, ip, meta.endpoint.port))
                }
                Err(_) => Err(EAGAIN),
            });
        }
        None
    })
    .map_err(|e| if e == ETIMEDOUT { EAGAIN } else { e })?
}

pub fn close(id: u64) -> i64 {
    with_stack(|stack| {
        let (handle, kind) = match lookup(stack, id) {
            Ok(v) => v,
            Err(e) => return e,
        };
        stack.user.remove(&(id as u32));
        match kind {
            SockKind::Tcp => {
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                socket.abort();
            }
            SockKind::Udp => {}
        }
        stack.sockets.remove(handle);
        0
    })
    .unwrap_or(0)
}

pub fn state(id: u64) -> i64 {
    with_stack(|stack| match lookup(stack, id) {
        Ok((handle, SockKind::Tcp)) => stack.sockets.get::<tcp::Socket>(handle).state() as i64,
        Ok(_) => 0,
        Err(e) => e,
    })
    .unwrap_or(ENETDOWN)
}

pub fn close_all(pid: Pid) {
    with_stack(|stack| {
        let ids: Vec<u32> = stack.user.iter().filter(|(_, s)| s.owner == pid).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(s) = stack.user.remove(&id) {
                if s.kind == SockKind::Tcp {
                    stack.sockets.get_mut::<tcp::Socket>(s.handle).abort();
                }
                stack.sockets.remove(s.handle);
            }
        }
    });
}
