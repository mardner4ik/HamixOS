#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::{entry, println, sys, users};

const IDLE_EXIT_MS: i64 = 60_000;
const FAILURE_DELAY_MS: u64 = 800;
const TICKET_SECONDS: u64 = 300;
const EACCES: i64 = -13;
const EINVAL: i64 = -22;

fn real_uid(pid: i64) -> Option<u32> {
    let status = hxstd::fs::read_to_string(&format!("/proc/{}/status", pid))?;
    status.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok())
}

fn sender_identity(pid: i64) -> Option<(u32, String)> {
    let uid = real_uid(pid)?;
    Some((uid, users::name_of(uid)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Session {
    uid: u32,
    tty: u32,
    parent: i64,
}

struct Remembered {
    session: Session,
    expires_ms: u64,
}

fn session_of(pid: i64) -> Option<Session> {
    let p = sys::proc_list().into_iter().find(|p| p.pid == pid)?;
    Some(Session { uid: real_uid(pid)?, tty: p.tty, parent: p.parent })
}

fn remember(cache: &mut Vec<Remembered>, sender: i64) {
    let Some(session) = session_of(sender) else {
        return;
    };
    cache.retain(|r| r.session != session);
    cache.push(Remembered { session, expires_ms: sys::uptime_ms() + TICKET_SECONDS * 1000 });
}

fn renew(cache: &mut Vec<Remembered>, sender: i64) -> i64 {
    let now = sys::uptime_ms();
    cache.retain(|r| r.expires_ms > now);
    let Some(session) = session_of(sender) else {
        return EACCES;
    };
    if !cache.iter().any(|r| r.session == session) || !users::is_sudoer(&users::name_of(session.uid)) {
        return EACCES;
    }
    0
}

fn forget(cache: &mut Vec<Remembered>, sender: i64) -> i64 {
    if let Some(session) = session_of(sender) {
        cache.retain(|r| r.session.uid != session.uid || r.session.tty != session.tty);
    }
    0
}

fn auth(sender: i64, user: &str, password: &str) -> i64 {
    if !users::verify_password(user, password) {
        sys::sleep_ms(FAILURE_DELAY_MS);
        return EACCES;
    }
    let Some((uid, _)) = sender_identity(sender) else {
        return EACCES;
    };
    let own = users::find("/", user).map(|u| u.uid == uid).unwrap_or(false);
    if user == "root" || (own && users::is_sudoer(user)) {
        0
    } else {
        1
    }
}

fn passwd(sender: i64, old: &str, new: &str) -> i64 {
    let Some((_, name)) = sender_identity(sender) else {
        return EACCES;
    };
    if new.is_empty() || new.len() > 64 {
        return EINVAL;
    }
    if !users::verify_password(&name, old) {
        sys::sleep_ms(FAILURE_DELAY_MS);
        return EACCES;
    }
    match users::set_password("", &name, new) {
        Ok(()) => {
            sys::sync();
            0
        }
        Err(_) => -5,
    }
}

fn main() -> i32 {
    if sys::getuid() != 0 {
        println!("hx-authd: must run as root");
        return 1;
    }
    if sys::service_register(sys::AUTH_SERVICE) < 0 {
        println!("hx-authd: {} is already served", sys::AUTH_SERVICE);
        return 1;
    }
    let mut buf = [0u8; 512];
    let mut cache: Vec<Remembered> = Vec::new();
    loop {
        let Some((sender, n)) = sys::msg_recv(&mut buf, IDLE_EXIT_MS) else {
            let now = sys::uptime_ms();
            cache.retain(|r| r.expires_ms > now);
            if cache.is_empty() {
                return 0;
            }
            continue;
        };
        if sender <= 0 {
            continue;
        }
        let fields: Vec<&str> = buf[..n].split(|b| *b == 0).map(|f| core::str::from_utf8(f).unwrap_or("")).collect();
        let result = match fields.as_slice() {
            ["auth", user, password] => {
                let result = auth(sender, user, password);
                if result == 0 && *user != "root" {
                    remember(&mut cache, sender);
                }
                result
            }
            ["renew"] => renew(&mut cache, sender),
            ["forget"] => forget(&mut cache, sender),
            ["passwd", old, new] => passwd(sender, old, new),
            _ => EINVAL,
        };
        sys::msg_send(sender, format!("{}", result).as_bytes());
    }
}

entry!(main);
