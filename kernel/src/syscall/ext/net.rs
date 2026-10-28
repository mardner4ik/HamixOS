use super::*;

pub(in crate::syscall) const SYS_NET_STATUS: u64 = 9130;
pub(in crate::syscall) const SYS_NET_SET_MODE: u64 = 9131;
pub(in crate::syscall) const SYS_WIFI_SCAN: u64 = 9132;
pub(in crate::syscall) const SYS_WIFI_CONNECT: u64 = 9133;
pub(in crate::syscall) const SYS_WIFI_DISCONNECT: u64 = 9134;
pub(in crate::syscall) const SYS_NET_CONFIGURE: u64 = 9135;
pub(in crate::syscall) const SYS_WIFI_KNOWN: u64 = 9136;
pub(in crate::syscall) const SYS_WIFI_FORGET: u64 = 9137;
pub(in crate::syscall) const SYS_WIFI_AUTOJOIN: u64 = 9138;
pub(in crate::syscall) const SYS_NET_SOCKET: u64 = 9140;
pub(in crate::syscall) const SYS_NET_SOCKET_CONNECT: u64 = 9141;
pub(in crate::syscall) const SYS_NET_SOCKET_SEND: u64 = 9142;
pub(in crate::syscall) const SYS_NET_SOCKET_RECV: u64 = 9143;
pub(in crate::syscall) const SYS_NET_SOCKET_CLOSE: u64 = 9144;
pub(in crate::syscall) const SYS_RESOLVE: u64 = 9145;
pub(in crate::syscall) const SYS_PING: u64 = 9146;
pub(in crate::syscall) const SYS_NET_SOCKET_LISTEN: u64 = 9147;
pub(in crate::syscall) const SYS_NET_SOCKET_STATE: u64 = 9148;
pub(in crate::syscall) const SYS_NET_SOCKET_SENDTO: u64 = 9149;
pub(in crate::syscall) const SYS_NET_SOCKET_RECVFROM: u64 = 9150;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_NET_STATUS => copy_out(a1, a2, crate::net::status_text().as_bytes()),
        SYS_NET_SET_MODE => {
            if let Err(e) = require_root_or_console() {
                return Some(e);
            }
            match crate::net::set_mode(a1 as u8) {
                Ok(()) => 0,
                Err(_) => ENODEV,
            }
        }
        SYS_WIFI_SCAN => match crate::net::wifi_scan_text(a3 != 0) {
            Ok(text) => copy_out(a1, a2, text.as_bytes()),
            Err(_) => ENODEV,
        },
        SYS_WIFI_CONNECT => match (user_string(a1, a2), user_string(a3, a4)) {
            (Ok(ssid), Ok(pass)) => match crate::net::wifi_connect(&ssid, &pass) {
                Ok(()) => 0,
                Err("no Wi-Fi adapter") => ENODEV,
                Err(_) => EINVAL,
            },
            (Err(e), _) | (_, Err(e)) => e,
        },
        SYS_WIFI_DISCONNECT => match crate::net::wifi_disconnect() {
            Ok(()) => 0,
            Err(_) => ENODEV,
        },
        SYS_WIFI_KNOWN => copy_out(a1, a2, crate::net::wifi_known_text().as_bytes()),
        SYS_WIFI_FORGET => match user_string(a1, a2) {
            Ok(ssid) => {
                if task::with_current(|t| t.uid) != 0 {
                    EPERM
                } else if crate::net::wifi_forget(&ssid) {
                    0
                } else {
                    ENOENT
                }
            }
            Err(e) => e,
        },
        SYS_WIFI_AUTOJOIN => match user_string(a1, a2) {
            Ok(ssid) => {
                if task::with_current(|t| t.uid) != 0 {
                    EPERM
                } else if crate::net::known::set_automatic(&ssid, a3 != 0) {
                    0
                } else {
                    ENOENT
                }
            }
            Err(e) => e,
        },
        SYS_NET_CONFIGURE => {
            if let Err(e) = require_root() {
                return Some(e);
            }
            match user_string(a3, a4) {
                Ok(spec) => match crate::net::stack::configure(&spec) {
                    Ok(()) => 0,
                    Err(_) => EINVAL,
                },
                Err(e) => e,
            }
        }
        SYS_NET_SOCKET => crate::net::stack::socket(a1, a2 as u16),
        SYS_NET_SOCKET_CONNECT => crate::net::stack::connect(a1, ((a2 >> 16) as u32).to_be_bytes(), a2 as u16, a3),
        SYS_NET_SOCKET_LISTEN => crate::net::stack::listen(a1, a2 as u16),
        SYS_NET_SOCKET_STATE => crate::net::stack::state(a1),
        SYS_NET_SOCKET_SEND => match user_slice(a2, a3.min(1 << 20)) {
            Ok(data) => match crate::memory::try_copy(data) {
                Ok(copy) => crate::net::stack::send(a1, &copy),
                Err(e) => e,
            },
            Err(e) => e,
        },
        SYS_NET_SOCKET_RECV => match crate::memory::try_zeroed(a3.min(1 << 20) as usize) {
            Ok(mut tmp) => {
                let n = crate::net::stack::recv(a1, &mut tmp, a4);
                if n > 0 { copy_out(a2, a3, &tmp[..n as usize]).min(n) } else { n }
            }
            Err(e) => e,
        },
        SYS_NET_SOCKET_SENDTO => match user_slice(a3, a4.min(65536)) {
            Ok(data) => {
                let copy = data.to_vec();
                crate::net::stack::send_to(a1, ((a2 >> 16) as u32).to_be_bytes(), a2 as u16, &copy)
            }
            Err(e) => e,
        },
        SYS_NET_SOCKET_RECVFROM => {
            let mut tmp = alloc::vec![0u8; a3.min(65536) as usize];
            match crate::net::stack::recv_from(a1, &mut tmp, a4) {
                Ok((n, ip, port)) => {
                    if a5 != 0 {
                        let mut meta = [0u8; 8];
                        meta[..4].copy_from_slice(&ip);
                        meta[4..6].copy_from_slice(&port.to_le_bytes());
                        copy_out(a5, 8, &meta);
                    }
                    copy_out(a2, a3, &tmp[..n]).min(n as i64)
                }
                Err(e) => e,
            }
        }
        SYS_NET_SOCKET_CLOSE => crate::net::stack::close(a1),
        SYS_RESOLVE => match user_string(a1, a2) {
            Ok(name) => match crate::net::stack::resolve(&name, 5000) {
                Ok(ip) => copy_out(a3, 4, &ip).min(0),
                Err(e) => e,
            },
            Err(e) => e,
        },
        SYS_PING => crate::net::stack::ping((a1 as u32).to_be_bytes(), a2 as u16, a3.clamp(100, 10_000)),
        _ => return None,
    })
}
