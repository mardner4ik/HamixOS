#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxdbus::{one_path, one_str, Bus, Message, Reply, Value, Writer, NOT_SUPPORTED, PROPERTIES, UNKNOWN_METHOD};
use hxstd::{entry, env, sys};

const SOCKET: &str = "/run/dbus/system_bus_socket";
const NAME: &str = "org.freedesktop.UPower";
const ROOT: &str = "/org/freedesktop/UPower";
const UPOWER: &str = "org.freedesktop.UPower";
const DEVICE: &str = "org.freedesktop.UPower.Device";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const LINE_POWER: &str = "/org/freedesktop/UPower/devices/line_power_AC";

const TYPE_LINE_POWER: u32 = 1;
const TYPE_BATTERY: u32 = 2;
const LEVEL_NONE: u32 = 1;

fn daemon_props() -> Vec<(&'static str, Value)> {
    alloc::vec![
        ("DaemonVersion", Value::Str(String::from("1.90.2"))),
        ("OnBattery", Value::Bool(false)),
        ("LidIsClosed", Value::Bool(false)),
        ("LidIsPresent", Value::Bool(false)),
    ]
}

fn device_props(kind: u32, native: &str, online: bool, present: bool) -> Vec<(&'static str, Value)> {
    let now = sys::realtime().sec as u64;
    alloc::vec![
        ("NativePath", Value::Str(String::from(native))),
        ("Vendor", Value::Str(String::new())),
        ("Model", Value::Str(String::new())),
        ("Serial", Value::Str(String::new())),
        ("UpdateTime", Value::U64(now)),
        ("Type", Value::U32(kind)),
        ("PowerSupply", Value::Bool(true)),
        ("HasHistory", Value::Bool(false)),
        ("HasStatistics", Value::Bool(false)),
        ("Online", Value::Bool(online)),
        ("Energy", Value::F64(0.0)),
        ("EnergyEmpty", Value::F64(0.0)),
        ("EnergyFull", Value::F64(0.0)),
        ("EnergyFullDesign", Value::F64(0.0)),
        ("EnergyRate", Value::F64(0.0)),
        ("Voltage", Value::F64(0.0)),
        ("ChargeCycles", Value::I32(-1)),
        ("Luminosity", Value::F64(0.0)),
        ("TimeToEmpty", Value::I64(0)),
        ("TimeToFull", Value::I64(0)),
        ("Percentage", Value::F64(0.0)),
        ("Temperature", Value::F64(0.0)),
        ("IsPresent", Value::Bool(present)),
        ("State", Value::U32(0)),
        ("IsRechargeable", Value::Bool(false)),
        ("Capacity", Value::F64(0.0)),
        ("Technology", Value::U32(0)),
        ("WarningLevel", Value::U32(LEVEL_NONE)),
        ("BatteryLevel", Value::U32(LEVEL_NONE)),
        ("IconName", Value::Str(String::from(if kind == TYPE_LINE_POWER { "ac-adapter-symbolic" } else { "" }))),
    ]
}

fn props_for(path: &str, interface: &str) -> Option<Vec<(&'static str, Value)>> {
    match (path, interface) {
        (ROOT, UPOWER) => Some(daemon_props()),
        (DISPLAY, DEVICE) => Some(device_props(TYPE_BATTERY, "", false, false)),
        (LINE_POWER, DEVICE) => Some(device_props(TYPE_LINE_POWER, "AC", true, true)),
        _ => None,
    }
}

fn interface_of(path: &str) -> &'static str {
    match path {
        ROOT => UPOWER,
        DISPLAY | LINE_POWER => DEVICE,
        _ => "",
    }
}

fn introspect(path: &str) -> Reply {
    let interface = interface_of(path);
    let methods = match interface {
        UPOWER => "  <method name=\"EnumerateDevices\"><arg type=\"ao\" direction=\"out\"/></method>\n  <method name=\"GetDisplayDevice\"><arg type=\"o\" direction=\"out\"/></method>\n  <method name=\"GetCriticalAction\"><arg type=\"s\" direction=\"out\"/></method>\n",
        DEVICE => "  <method name=\"Refresh\"/>\n  <method name=\"GetHistory\"><arg type=\"s\" direction=\"in\"/><arg type=\"u\" direction=\"in\"/><arg type=\"u\" direction=\"in\"/><arg type=\"a(udu)\" direction=\"out\"/></method>\n  <method name=\"GetStatistics\"><arg type=\"s\" direction=\"in\"/><arg type=\"a(dd)\" direction=\"out\"/></method>\n",
        _ => "",
    };
    let children: &[&str] = match path {
        ROOT => &["devices"],
        "/org/freedesktop/UPower/devices" => &["DisplayDevice", "line_power_AC"],
        _ => &[],
    };
    hxdbus::introspect(interface, props_for(path, interface), methods, children)
}

fn empty_array(signature: &'static str, align: usize) -> Reply {
    let mut w = Writer::default();
    w.array(align, |_| {});
    Reply::Body(signature, w)
}

fn handle(m: &Message) -> Reply {
    match m.interface.as_str() {
        "org.freedesktop.DBus.Introspectable" => introspect(&m.path),
        PROPERTIES => {
            let interface = hxdbus::properties_interface(m);
            hxdbus::properties(m, props_for(&m.path, &interface))
        }
        UPOWER | "" if m.path == ROOT => match m.member.as_str() {
            "EnumerateDevices" => {
                let mut w = Writer::default();
                w.value(&Value::Paths(alloc::vec![String::from(LINE_POWER)]));
                Reply::Body("ao", w)
            }
            "GetDisplayDevice" => one_path(DISPLAY),
            "GetCriticalAction" => one_str("PowerOff"),
            other => Reply::Error(NOT_SUPPORTED, format!("{} is not supported by hx-upower", other)),
        },
        DEVICE | "" if interface_of(&m.path) == DEVICE => match m.member.as_str() {
            "Refresh" => Reply::Empty,
            "GetHistory" => empty_array("a(udu)", 8),
            "GetStatistics" => empty_array("a(dd)", 8),
            other => Reply::Error(NOT_SUPPORTED, format!("{} is not supported by hx-upower", other)),
        },
        _ => Reply::Error(UNKNOWN_METHOD, format!("unknown interface {} on {}", m.interface, m.path)),
    }
}

fn main() -> i32 {
    let once = env::args().iter().any(|a| a == "--once");
    hxdbus::run(SOCKET, NAME, "hx-upower", once, |_: &mut Bus, m: &Message| handle(m))
}

entry!(main);
