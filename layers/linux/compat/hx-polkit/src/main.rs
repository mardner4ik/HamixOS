#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxdbus::{Basic, Bus, Message, Reader, Reply, Value, Writer, NOT_SUPPORTED, PROPERTIES, UNKNOWN_METHOD};
use hxstd::{entry, env, fs, users};

const SOCKET: &str = "/run/dbus/system_bus_socket";
const NAME: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY: &str = "org.freedesktop.PolicyKit1.Authority";

const ALLOW_ACTIVE: &[&str] = &[
    "org.freedesktop.login1.power-off",
    "org.freedesktop.login1.reboot",
    "org.freedesktop.login1.suspend",
    "org.freedesktop.login1.hibernate",
    "org.freedesktop.login1.lock-sessions",
    "org.freedesktop.login1.set-self-linger",
    "org.freedesktop.udisks2.filesystem-mount",
    "org.freedesktop.udisks2.encrypted-unlock",
    "org.freedesktop.udisks2.eject-media",
    "org.freedesktop.upower.",
    "org.freedesktop.NetworkManager.network-control",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Yes,
    Challenge,
    No,
}

pub fn decide(uid: Option<u32>, action: &str) -> Verdict {
    let Some(uid) = uid else {
        return Verdict::No;
    };
    if uid == 0 {
        return Verdict::Yes;
    }
    if ALLOW_ACTIVE.iter().any(|a| if a.ends_with('.') { action.starts_with(a) } else { action == *a }) {
        return Verdict::Yes;
    }
    if users::is_sudoer(&users::name_of(uid)) {
        Verdict::Challenge
    } else {
        Verdict::No
    }
}

fn uid_of_pid(pid: i64) -> Option<u32> {
    let text = fs::read_to_string(&format!("/proc/{}/status", pid))?;
    text.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok())
}

fn subject_uid(bus: &mut Bus, kind: &str, details: &[(String, Basic)]) -> Option<u32> {
    let get = |key: &str| details.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    match kind {
        "unix-process" => match get("uid").and_then(|v| v.int()) {
            Some(uid) if uid >= 0 => Some(uid as u32),
            _ => get("pid").and_then(|v| v.int()).and_then(uid_of_pid),
        },
        "system-bus-name" => {
            let name = get("name")?;
            hxdbus::unix_user_of(bus, name.text()?)
        }
        "unix-session" => get("session-id")?.text()?.strip_prefix('s')?.parse().ok(),
        _ => None,
    }
}

fn authority_props() -> Vec<(&'static str, Value)> {
    alloc::vec![
        ("BackendName", Value::Str(String::from("hx-polkit"))),
        ("BackendVersion", Value::Str(String::from("0.1"))),
        ("BackendFeatures", Value::U32(0)),
    ]
}

fn check_authorization(bus: &mut Bus, m: &Message) -> Reply {
    let mut r = Reader::new(&m.body);
    r.start_struct();
    let kind = r.str().unwrap_or_default();
    let details = r.dict_sv().unwrap_or_default();
    let action = r.str().unwrap_or_default();
    let uid = subject_uid(bus, &kind, &details);
    let verdict = decide(uid, &action);
    let mut w = Writer::default();
    w.pad(8);
    w.bool(verdict == Verdict::Yes);
    w.bool(verdict == Verdict::Challenge);
    w.array(8, |w| {
        if verdict == Verdict::Challenge {
            w.pad(8);
            w.str("polkit.message");
            w.str("HamixOS has no polkit authentication agent yet; use sudo");
        }
    });
    Reply::Body("(bba{ss})", w)
}

fn empty_array(signature: &'static str, align: usize) -> Reply {
    let mut w = Writer::default();
    w.array(align, |_| {});
    Reply::Body(signature, w)
}

fn handle(bus: &mut Bus, m: &Message) -> Reply {
    if m.path != AUTHORITY_PATH {
        return match m.interface.as_str() {
            "org.freedesktop.DBus.Introspectable" => hxdbus::introspect("", None, "", if m.path == "/" { &["org"] } else { &[] }),
            _ => Reply::Error(UNKNOWN_METHOD, format!("no object {}", m.path)),
        };
    }
    match m.interface.as_str() {
        "org.freedesktop.DBus.Introspectable" => hxdbus::introspect(
            AUTHORITY,
            Some(authority_props()),
            "  <method name=\"CheckAuthorization\"><arg type=\"(sa{sv})\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"a{ss}\" direction=\"in\"/><arg type=\"u\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"(bba{ss})\" direction=\"out\"/></method>\n  <method name=\"EnumerateActions\"><arg type=\"s\" direction=\"in\"/><arg type=\"a(ssssssuuua{ss})\" direction=\"out\"/></method>\n",
            &[],
        ),
        PROPERTIES => {
            let interface = hxdbus::properties_interface(m);
            hxdbus::properties(m, if interface == AUTHORITY { Some(authority_props()) } else { None })
        }
        AUTHORITY | "" => match m.member.as_str() {
            "CheckAuthorization" => check_authorization(bus, m),
            "EnumerateActions" => empty_array("a(ssssssuuua{ss})", 8),
            "EnumerateTemporaryAuthorizations" => empty_array("a(ss(sa{sv})tt)", 8),
            "CancelCheckAuthorization" | "RegisterAuthenticationAgent" | "RegisterAuthenticationAgentWithOptions" | "UnregisterAuthenticationAgent"
            | "AuthenticationAgentResponse" | "AuthenticationAgentResponse2" | "RevokeTemporaryAuthorizations" | "RevokeTemporaryAuthorizationById" => Reply::Empty,
            other => Reply::Error(NOT_SUPPORTED, format!("{} is not supported by hx-polkit", other)),
        },
        _ => Reply::Error(UNKNOWN_METHOD, format!("unknown interface {}", m.interface)),
    }
}

fn main() -> i32 {
    let once = env::args().iter().any(|a| a == "--once");
    hxdbus::run(SOCKET, NAME, "hx-polkit", once, handle)
}

entry!(main);
