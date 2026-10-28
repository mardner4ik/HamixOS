#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxdbus::{one_path, one_str, Bus, Message, Reader, Reply, Value, Writer, NOT_SUPPORTED, PROPERTIES, UNKNOWN_METHOD};
use hxstd::{entry, env, fs, sys, users};

const SOCKET: &str = "/run/dbus/system_bus_socket";
const NAME: &str = "org.freedesktop.login1";
const ROOT: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const SEAT: &str = "org.freedesktop.login1.Seat";
const USER: &str = "org.freedesktop.login1.User";
const SEAT_PATH: &str = "/org/freedesktop/login1/seat/seat0";
const NO_SUCH_SESSION: &str = "org.freedesktop.login1.NoSuchSession";
const NO_SUCH_USER: &str = "org.freedesktop.login1.NoSuchUser";

fn session_path(uid: u32) -> String {
    format!("{}/session/s{}", ROOT, uid)
}

fn user_path(uid: u32) -> String {
    format!("{}/user/_{}", ROOT, uid)
}

fn uid_of_pid(pid: u32) -> Option<u32> {
    let text = fs::read_to_string(&format!("/proc/{}/status", pid))?;
    text.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok())
}

fn session_uids() -> Vec<u32> {
    let mut uids: Vec<u32> = Vec::new();
    for entry in sys::read_dir("/proc").unwrap_or_default() {
        let Ok(pid) = entry.name.parse::<u32>() else {
            continue;
        };
        if let Some(uid) = uid_of_pid(pid) {
            if uid >= 1000 && uid < 60000 && !uids.contains(&uid) {
                uids.push(uid);
            }
        }
    }
    uids.sort();
    if uids.is_empty() {
        uids.push(0);
    }
    uids
}

fn active_uid() -> u32 {
    session_uids()[0]
}

fn session_uid(path: &str) -> Option<u32> {
    path.strip_prefix(ROOT)?.strip_prefix("/session/")?.strip_prefix('s')?.parse().ok().filter(|u| session_uids().contains(u))
}

fn user_uid(path: &str) -> Option<u32> {
    path.strip_prefix(ROOT)?.strip_prefix("/user/_")?.parse().ok().filter(|u| session_uids().contains(u))
}

fn session_id_uid(id: &str) -> Option<u32> {
    if id == "auto" || id == "self" {
        return Some(active_uid());
    }
    id.strip_prefix('s')?.parse().ok().filter(|u| session_uids().contains(u))
}

fn manager_props() -> Vec<(&'static str, Value)> {
    alloc::vec![
        ("IdleHint", Value::Bool(false)),
        ("IdleSinceHint", Value::U64(0)),
        ("BlockInhibited", Value::Str(String::new())),
        ("DelayInhibited", Value::Str(String::new())),
        ("PreparingForShutdown", Value::Bool(false)),
        ("PreparingForSleep", Value::Bool(false)),
        ("Docked", Value::Bool(false)),
        ("LidClosed", Value::Bool(false)),
        ("OnExternalPower", Value::Bool(true)),
        ("NAutoVTs", Value::U32(6)),
        ("KillUserProcesses", Value::Bool(false)),
    ]
}

fn session_props(uid: u32) -> Vec<(&'static str, Value)> {
    alloc::vec![
        ("Id", Value::Str(format!("s{}", uid))),
        ("User", Value::UidPath(uid, user_path(uid))),
        ("Name", Value::Str(users::name_of(uid))),
        ("Timestamp", Value::U64(0)),
        ("VTNr", Value::U32(1)),
        ("Seat", Value::IdPath(String::from("seat0"), String::from(SEAT_PATH))),
        ("TTY", Value::Str(String::from("tty1"))),
        ("Display", Value::Str(String::new())),
        ("Remote", Value::Bool(false)),
        ("Service", Value::Str(String::from("hxlogin"))),
        ("Desktop", Value::Str(String::from("nook"))),
        ("Leader", Value::U32(1)),
        ("Type", Value::Str(String::from("wayland"))),
        ("Class", Value::Str(String::from("user"))),
        ("Active", Value::Bool(uid == active_uid())),
        ("State", Value::Str(String::from(if uid == active_uid() { "active" } else { "online" }))),
        ("IdleHint", Value::Bool(false)),
        ("LockedHint", Value::Bool(false)),
    ]
}

fn seat_props() -> Vec<(&'static str, Value)> {
    let active = active_uid();
    alloc::vec![
        ("Id", Value::Str(String::from("seat0"))),
        ("ActiveSession", Value::IdPath(format!("s{}", active), session_path(active))),
        ("CanGraphical", Value::Bool(true)),
        ("CanTTY", Value::Bool(true)),
        ("Sessions", Value::IdPaths(session_uids().into_iter().map(|u| (format!("s{}", u), session_path(u))).collect())),
        ("IdleHint", Value::Bool(false)),
    ]
}

fn user_props(uid: u32) -> Vec<(&'static str, Value)> {
    let gid = users::find_uid("/", uid).map(|u| u.gid).unwrap_or(uid);
    alloc::vec![
        ("UID", Value::U32(uid)),
        ("GID", Value::U32(gid)),
        ("Name", Value::Str(users::name_of(uid))),
        ("State", Value::Str(String::from(if uid == active_uid() { "active" } else { "online" }))),
        ("RuntimePath", Value::Str(format!("/tmp/runtime-{}", uid))),
        ("Sessions", Value::IdPaths(alloc::vec![(format!("s{}", uid), session_path(uid))])),
        ("Display", Value::IdPath(format!("s{}", uid), session_path(uid))),
        ("Linger", Value::Bool(false)),
    ]
}

fn props_for(path: &str, interface: &str) -> Option<Vec<(&'static str, Value)>> {
    match interface {
        MANAGER if path == ROOT => Some(manager_props()),
        SESSION => session_uid(path).map(session_props),
        SEAT if path == SEAT_PATH => Some(seat_props()),
        USER => user_uid(path).map(user_props),
        _ => None,
    }
}

fn introspect(path: &str) -> Reply {
    let interface = if path == ROOT {
        MANAGER
    } else if path == SEAT_PATH {
        SEAT
    } else if session_uid(path).is_some() {
        SESSION
    } else if user_uid(path).is_some() {
        USER
    } else {
        ""
    };
    let children: &[&str] = if path == ROOT { &["seat", "session", "user"] } else { &[] };
    hxdbus::introspect(interface, props_for(path, interface), "", children)
}

fn power(action: &str) -> Reply {
    let pid = sys::spawn("/usr/bin/hxctl", &[action], 0);
    if pid < 0 {
        return Reply::Error("org.freedesktop.DBus.Error.Failed", format!("cannot run hxctl {}", action));
    }
    Reply::Empty
}

fn inhibit() -> Reply {
    match sys::pipe() {
        Ok((read_end, write_end)) => {
            sys::close(read_end);
            Reply::Fd(write_end)
        }
        Err(e) => Reply::Error("org.freedesktop.DBus.Error.Failed", format!("pipe failed ({})", e)),
    }
}

fn manager(m: &Message) -> Reply {
    let mut r = Reader::new(&m.body);
    match m.member.as_str() {
        "GetSession" => match session_id_uid(&r.str().unwrap_or_default()) {
            Some(uid) => one_path(&session_path(uid)),
            None => Reply::Error(NO_SUCH_SESSION, String::from("no such session")),
        },
        "GetSessionByPID" => {
            let pid = r.u32().unwrap_or(0);
            let uid = if pid == 0 { Some(active_uid()) } else { uid_of_pid(pid) };
            match uid.map(|u| if session_uids().contains(&u) { u } else { active_uid() }) {
                Some(uid) => one_path(&session_path(uid)),
                None => Reply::Error(NO_SUCH_SESSION, format!("PID {} does not belong to any known session", pid)),
            }
        }
        "GetUser" => {
            let uid = r.u32().unwrap_or(u32::MAX);
            if session_uids().contains(&uid) {
                one_path(&user_path(uid))
            } else {
                Reply::Error(NO_SUCH_USER, format!("user {} is not logged in", uid))
            }
        }
        "GetUserByPID" => {
            let pid = r.u32().unwrap_or(0);
            let uid = if pid == 0 { active_uid() } else { uid_of_pid(pid).unwrap_or(active_uid()) };
            one_path(&user_path(uid))
        }
        "GetSeat" => {
            let id = r.str().unwrap_or_default();
            if id == "seat0" || id == "auto" || id == "self" {
                one_path(SEAT_PATH)
            } else {
                Reply::Error("org.freedesktop.login1.NoSuchSeat", String::from("no such seat"))
            }
        }
        "ListSessions" => {
            let mut w = Writer::default();
            w.array(8, |w| {
                for uid in session_uids() {
                    w.pad(8);
                    w.str(&format!("s{}", uid));
                    w.u32(uid);
                    w.str(&users::name_of(uid));
                    w.str("seat0");
                    w.str(&session_path(uid));
                }
            });
            Reply::Body("a(susso)", w)
        }
        "ListUsers" => {
            let mut w = Writer::default();
            w.array(8, |w| {
                for uid in session_uids() {
                    w.pad(8);
                    w.u32(uid);
                    w.str(&users::name_of(uid));
                    w.str(&user_path(uid));
                }
            });
            Reply::Body("a(uso)", w)
        }
        "ListSeats" => {
            let mut w = Writer::default();
            w.array(8, |w| {
                w.pad(8);
                w.str("seat0");
                w.str(SEAT_PATH);
            });
            Reply::Body("a(so)", w)
        }
        "ListInhibitors" => {
            let mut w = Writer::default();
            w.array(8, |_| {});
            Reply::Body("a(ssssuu)", w)
        }
        "CanPowerOff" | "CanReboot" => one_str("yes"),
        "CanSuspend" | "CanHibernate" | "CanHybridSleep" | "CanSuspendThenHibernate" | "CanRebootToFirmwareSetup" => one_str("na"),
        "PowerOff" => power("poweroff"),
        "Reboot" => power("reboot"),
        "Inhibit" => inhibit(),
        "ActivateSession" | "ActivateSessionOnSeat" | "LockSession" | "UnlockSession" | "LockSessions" | "UnlockSessions" | "SetUserLinger" => Reply::Empty,
        _ => Reply::Error(NOT_SUPPORTED, format!("{} is not supported by hx-login1", m.member)),
    }
}

fn session(m: &Message) -> Reply {
    if session_uid(&m.path).is_none() {
        return Reply::Error(NO_SUCH_SESSION, String::from("no such session"));
    }
    match m.member.as_str() {
        "Activate" | "Lock" | "Unlock" | "SetIdleHint" | "SetLockedHint" | "SetType" | "SetDisplay" | "ReleaseControl" | "SetBrightness" | "Terminate" => Reply::Empty,
        _ => Reply::Error(NOT_SUPPORTED, format!("{} is not supported by hx-login1 (device access goes through the native display)", m.member)),
    }
}

fn properties(m: &Message) -> Reply {
    let interface = hxdbus::properties_interface(m);
    hxdbus::properties(m, props_for(&m.path, &interface))
}

fn handle(m: &Message) -> Reply {
    match m.interface.as_str() {
        "org.freedesktop.DBus.Introspectable" => introspect(&m.path),
        PROPERTIES => properties(m),
        MANAGER => manager(m),
        SESSION => session(m),
        SEAT | USER => Reply::Empty,
        "" if m.path == ROOT => manager(m),
        _ => Reply::Error(UNKNOWN_METHOD, format!("unknown interface {}", m.interface)),
    }
}

fn main() -> i32 {
    let once = env::args().iter().any(|a| a == "--once");
    hxdbus::run(SOCKET, NAME, "hx-login1", once, |_: &mut Bus, m: &Message| handle(m))
}

entry!(main);
