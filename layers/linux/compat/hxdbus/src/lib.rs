#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::{sys, unix};

pub const METHOD_CALL: u8 = 1;
pub const METHOD_RETURN: u8 = 2;
pub const ERROR: u8 = 3;
pub const NO_REPLY_EXPECTED: u8 = 1;

const PATH: u8 = 1;
const INTERFACE: u8 = 2;
const MEMBER: u8 = 3;
const ERROR_NAME: u8 = 4;
const REPLY_SERIAL: u8 = 5;
const DESTINATION: u8 = 6;
const SENDER: u8 = 7;
const SIGNATURE: u8 = 8;
const UNIX_FDS: u8 = 9;

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn pad(&mut self, n: usize) {
        while self.buf.len() % n != 0 {
            self.buf.push(0);
        }
    }

    pub fn byte(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u32(&mut self, v: u32) {
        self.pad(4);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.pad(8);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn bool(&mut self, v: bool) {
        self.u32(v as u32);
    }

    pub fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }

    pub fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }

    pub fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }

    pub fn str(&mut self, v: &str) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v.as_bytes());
        self.buf.push(0);
    }

    pub fn sig(&mut self, v: &str) {
        self.buf.push(v.len() as u8);
        self.buf.extend_from_slice(v.as_bytes());
        self.buf.push(0);
    }

    pub fn array(&mut self, align: usize, f: impl FnOnce(&mut Writer)) {
        self.pad(4);
        let at = self.buf.len();
        self.buf.extend_from_slice(&[0; 4]);
        self.pad(align);
        let start = self.buf.len();
        f(self);
        let len = (self.buf.len() - start) as u32;
        self.buf[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }

    pub fn value(&mut self, v: &Value) {
        match v {
            Value::Bool(b) => self.bool(*b),
            Value::Byte(n) => self.byte(*n),
            Value::I32(n) => self.i32(*n),
            Value::I64(n) => self.i64(*n),
            Value::F64(n) => self.f64(*n),
            Value::Path(p) => self.str(p),
            Value::Paths(list) => self.array(4, |w| {
                for p in list {
                    w.str(p);
                }
            }),
            Value::U32(n) => self.u32(*n),
            Value::U64(n) => self.u64(*n),
            Value::Str(s) => self.str(s),
            Value::UidPath(u, p) => {
                self.pad(8);
                self.u32(*u);
                self.str(p);
            }
            Value::IdPath(i, p) => {
                self.pad(8);
                self.str(i);
                self.str(p);
            }
            Value::IdPaths(list) => self.array(8, |w| {
                for (i, p) in list {
                    w.pad(8);
                    w.str(i);
                    w.str(p);
                }
            }),
        }
    }

    pub fn variant(&mut self, v: &Value) {
        self.sig(v.signature());
        self.value(v);
    }
}

pub enum Value {
    Bool(bool),
    Byte(u8),
    I32(i32),
    I64(i64),
    F64(f64),
    Path(String),
    Paths(Vec<String>),
    U32(u32),
    U64(u64),
    Str(String),
    UidPath(u32, String),
    IdPath(String, String),
    IdPaths(Vec<(String, String)>),
}

impl Value {
    pub fn signature(&self) -> &'static str {
        match self {
            Value::Bool(_) => "b",
            Value::Byte(_) => "y",
            Value::I32(_) => "i",
            Value::I64(_) => "x",
            Value::F64(_) => "d",
            Value::Path(_) => "o",
            Value::Paths(_) => "ao",
            Value::U32(_) => "u",
            Value::U64(_) => "t",
            Value::Str(_) => "s",
            Value::UidPath(..) => "(uo)",
            Value::IdPath(..) => "(so)",
            Value::IdPaths(_) => "a(so)",
        }
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    fn align(&mut self, n: usize) {
        self.pos = (self.pos + n - 1) / n * n;
    }

    pub fn byte(&mut self) -> Option<u8> {
        let v = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.align(4);
        let v = u32::from_le_bytes(self.data.get(self.pos..self.pos + 4)?.try_into().ok()?);
        self.pos += 4;
        Some(v)
    }

    pub fn str(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        let bytes = self.data.get(self.pos..self.pos + len)?;
        self.pos += len + 1;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn sig(&mut self) -> Option<String> {
        let len = self.byte()? as usize;
        let bytes = self.data.get(self.pos..self.pos + len)?;
        self.pos += len + 1;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn u64(&mut self) -> Option<u64> {
        self.align(8);
        let v = u64::from_le_bytes(self.data.get(self.pos..self.pos + 8)?.try_into().ok()?);
        self.pos += 8;
        Some(v)
    }

    pub fn start_struct(&mut self) {
        self.align(8);
    }

    pub fn basic(&mut self, code: u8) -> Option<Basic> {
        Some(match code {
            b'y' => Basic::Int(self.byte()? as i64),
            b'b' => Basic::Int(self.u32()? as i64),
            b'n' | b'q' => {
                self.align(2);
                let v = u16::from_le_bytes(self.data.get(self.pos..self.pos + 2)?.try_into().ok()?);
                self.pos += 2;
                Basic::Int(if code == b'n' { v as i16 as i64 } else { v as i64 })
            }
            b'i' => Basic::Int(self.u32()? as i32 as i64),
            b'u' | b'h' => Basic::Int(self.u32()? as i64),
            b'x' | b't' | b'd' => Basic::Int(self.u64()? as i64),
            b's' | b'o' => Basic::Str(self.str()?),
            b'g' => Basic::Str(self.sig()?),
            _ => return None,
        })
    }

    pub fn dict_sv(&mut self) -> Option<Vec<(String, Basic)>> {
        let len = self.u32()? as usize;
        self.align(8);
        let end = self.pos + len;
        let mut out = Vec::new();
        while self.pos < end {
            self.align(8);
            let key = self.str()?;
            let signature = self.sig()?;
            let code = *signature.as_bytes().first()?;
            if signature.len() != 1 {
                return None;
            }
            out.push((key, self.basic(code)?));
        }
        self.pos = end;
        Some(out)
    }
}

#[derive(Clone, Debug)]
pub enum Basic {
    Int(i64),
    Str(String),
}

impl Basic {
    pub fn int(&self) -> Option<i64> {
        match self {
            Basic::Int(v) => Some(*v),
            Basic::Str(_) => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Basic::Str(s) => Some(s),
            Basic::Int(_) => None,
        }
    }
}

#[derive(Default)]
pub struct Message {
    pub kind: u8,
    pub flags: u8,
    pub serial: u32,
    pub path: String,
    pub interface: String,
    pub member: String,
    pub error: String,
    pub destination: String,
    pub sender: String,
    pub signature: String,
    pub reply_serial: u32,
    pub body: Vec<u8>,
    pub fds: Vec<u64>,
}

impl Message {
    pub fn call(destination: &str, path: &str, interface: &str, member: &str) -> Message {
        Message { kind: METHOD_CALL, destination: destination.into(), path: path.into(), interface: interface.into(), member: member.into(), ..Default::default() }
    }

    pub fn reply_to(call: &Message) -> Message {
        Message { kind: METHOD_RETURN, reply_serial: call.serial, destination: call.sender.clone(), ..Default::default() }
    }

    pub fn error_to(call: &Message, name: &str, text: &str) -> Message {
        let mut w = Writer::default();
        w.str(text);
        Message { kind: ERROR, reply_serial: call.serial, destination: call.sender.clone(), error: name.into(), signature: "s".into(), body: w.buf, ..Default::default() }
    }

    pub fn with_body(mut self, signature: &str, body: Writer) -> Message {
        self.signature = signature.into();
        self.body = body.buf;
        self
    }

    fn encode(&self, serial: u32) -> Vec<u8> {
        let mut w = Writer::default();
        w.byte(b'l');
        w.byte(self.kind);
        w.byte(self.flags);
        w.byte(1);
        w.u32(self.body.len() as u32);
        w.u32(serial);
        let fields: [(u8, &str, &str); 8] = [
            (PATH, "o", &self.path),
            (INTERFACE, "s", &self.interface),
            (MEMBER, "s", &self.member),
            (ERROR_NAME, "s", &self.error),
            (DESTINATION, "s", &self.destination),
            (SENDER, "s", &self.sender),
            (SIGNATURE, "g", &self.signature),
            (0, "", ""),
        ];
        let reply_serial = self.reply_serial;
        let fd_count = self.fds.len() as u32;
        w.array(8, |w| {
            for (code, sig, value) in fields {
                if code == 0 || value.is_empty() {
                    continue;
                }
                w.pad(8);
                w.byte(code);
                w.sig(sig);
                if sig == "g" {
                    w.sig(value);
                } else {
                    w.str(value);
                }
            }
            for (code, value) in [(REPLY_SERIAL, reply_serial), (UNIX_FDS, fd_count)] {
                if value != 0 {
                    w.pad(8);
                    w.byte(code);
                    w.sig("u");
                    w.u32(value);
                }
            }
        });
        w.pad(8);
        w.buf.extend_from_slice(&self.body);
        w.buf
    }

    fn decode(raw: &[u8]) -> Option<(Message, u32)> {
        if raw.first() != Some(&b'l') {
            return None;
        }
        let mut r = Reader::new(raw);
        r.pos = 1;
        let kind = r.byte()?;
        let flags = r.byte()?;
        r.byte()?;
        let body_len = r.u32()? as usize;
        let serial = r.u32()?;
        let fields_len = r.u32()? as usize;
        let end = 16 + fields_len;
        let mut m = Message { kind, flags, serial, ..Default::default() };
        let mut fds = 0;
        while r.pos < end {
            r.align(8);
            let code = r.byte()?;
            let sig = r.sig()?;
            match sig.as_str() {
                "s" | "o" => {
                    let v = r.str()?;
                    match code {
                        PATH => m.path = v,
                        INTERFACE => m.interface = v,
                        MEMBER => m.member = v,
                        ERROR_NAME => m.error = v,
                        DESTINATION => m.destination = v,
                        SENDER => m.sender = v,
                        _ => {}
                    }
                }
                "g" => {
                    let v = r.sig()?;
                    if code == SIGNATURE {
                        m.signature = v;
                    }
                }
                "u" => {
                    let v = r.u32()?;
                    match code {
                        REPLY_SERIAL => m.reply_serial = v,
                        UNIX_FDS => fds = v,
                        _ => {}
                    }
                }
                _ => return None,
            }
        }
        let start = (end + 7) / 8 * 8;
        m.body = raw.get(start..start + body_len)?.to_vec();
        Some((m, fds))
    }
}

pub fn total_len(raw: &[u8]) -> Option<usize> {
    if raw.len() < 16 {
        return None;
    }
    let body = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let fields = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
    Some((16 + fields + 7) / 8 * 8 + body)
}

pub struct Bus {
    fd: u64,
    serial: u32,
    inbuf: Vec<u8>,
    infds: VecDeque<u64>,
    pending: VecDeque<Message>,
    pub unique: String,
}

fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{:02x}", b)).collect()
}

impl Bus {
    fn line(&mut self) -> Option<String> {
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            if sys::read(self.fd, &mut byte) != 1 {
                return None;
            }
            out.push(byte[0]);
            if out.ends_with(b"\r\n") {
                out.truncate(out.len() - 2);
                return Some(String::from_utf8_lossy(&out).into_owned());
            }
            if out.len() > 4096 {
                return None;
            }
        }
    }

    pub fn connect(path: &str) -> Result<Bus, String> {
        let fd = unix::connect_to(path, unix::SOCK_STREAM);
        if fd < 0 {
            return Err(format!("cannot connect to {} ({})", path, fd));
        }
        let mut bus = Bus { fd: fd as u64, serial: 0, inbuf: Vec::new(), infds: VecDeque::new(), pending: VecDeque::new(), unique: String::new() };
        let uid = format!("{}", sys::getuid());
        sys::write(bus.fd, b"\0");
        sys::write(bus.fd, format!("AUTH EXTERNAL {}\r\n", hex(&uid)).as_bytes());
        match bus.line() {
            Some(l) if l.starts_with("OK ") => {}
            other => return Err(format!("the bus refused EXTERNAL authentication ({:?})", other)),
        }
        sys::write(bus.fd, b"NEGOTIATE_UNIX_FD\r\n");
        if !bus.line().map(|l| l.starts_with("AGREE_UNIX_FD")).unwrap_or(false) {
            return Err(String::from("the bus does not pass file descriptors"));
        }
        sys::write(bus.fd, b"BEGIN\r\n");
        let hello = bus.call(Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello"))?;
        bus.unique = Reader::new(&hello.body).str().unwrap_or_default();
        Ok(bus)
    }

    pub fn send(&mut self, message: &Message) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        let raw = message.encode(self.serial);
        let mut sent = 0;
        while sent < raw.len() {
            let r = unix::send_with_fds(self.fd, &raw[sent..], if sent == 0 { &message.fds } else { &[] }, 0);
            if r <= 0 {
                break;
            }
            sent += r as usize;
        }
        self.serial
    }

    fn read_one(&mut self) -> Option<Message> {
        loop {
            if let Some(total) = total_len(&self.inbuf) {
                if self.inbuf.len() >= total {
                    let raw: Vec<u8> = self.inbuf.drain(..total).collect();
                    let (mut m, fds) = Message::decode(&raw)?;
                    for _ in 0..fds {
                        if let Some(fd) = self.infds.pop_front() {
                            m.fds.push(fd);
                        }
                    }
                    return Some(m);
                }
            }
            let mut buf = [0u8; 8192];
            match unix::recv_with_fds(self.fd, &mut buf, 0) {
                Ok((0, _)) | Err(_) => return None,
                Ok((n, fds)) => {
                    self.inbuf.extend_from_slice(&buf[..n]);
                    self.infds.extend(fds);
                }
            }
        }
    }

    pub fn next(&mut self) -> Option<Message> {
        if let Some(m) = self.pending.pop_front() {
            return Some(m);
        }
        self.read_one()
    }

    pub fn call(&mut self, message: Message) -> Result<Message, String> {
        let serial = self.send(&message);
        loop {
            let m = self.read_one().ok_or_else(|| String::from("the bus closed the connection"))?;
            if m.reply_serial == serial && (m.kind == METHOD_RETURN || m.kind == ERROR) {
                if m.kind == ERROR {
                    return Err(format!("{}: {}", m.error, Reader::new(&m.body).str().unwrap_or_default()));
                }
                return Ok(m);
            }
            self.pending.push_back(m);
        }
    }
}

pub const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
pub const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
pub const NOT_SUPPORTED: &str = "org.freedesktop.DBus.Error.NotSupported";
pub const FAILED: &str = "org.freedesktop.DBus.Error.Failed";

pub enum Reply {
    Empty,
    Body(&'static str, Writer),
    Fd(u64),
    Error(&'static str, String),
}

pub fn one_str(value: &str) -> Reply {
    let mut w = Writer::default();
    w.str(value);
    Reply::Body("s", w)
}

pub fn one_path(value: &str) -> Reply {
    let mut w = Writer::default();
    w.str(value);
    Reply::Body("o", w)
}

pub fn properties(m: &Message, props: Option<Vec<(&'static str, Value)>>) -> Reply {
    let mut r = Reader::new(&m.body);
    let interface = r.str().unwrap_or_default();
    let Some(props) = props else {
        return Reply::Error("org.freedesktop.DBus.Error.UnknownInterface", format!("{} has no interface {}", m.path, interface));
    };
    match m.member.as_str() {
        "Get" => {
            let name = r.str().unwrap_or_default();
            match props.iter().find(|(n, _)| *n == name) {
                Some((_, value)) => {
                    let mut w = Writer::default();
                    w.variant(value);
                    Reply::Body("v", w)
                }
                None => Reply::Error("org.freedesktop.DBus.Error.UnknownProperty", format!("no property {}", name)),
            }
        }
        "GetAll" => {
            let mut w = Writer::default();
            w.array(8, |w| {
                for (name, value) in props.iter() {
                    w.pad(8);
                    w.str(name);
                    w.variant(value);
                }
            });
            Reply::Body("a{sv}", w)
        }
        "Set" => Reply::Error("org.freedesktop.DBus.Error.PropertyReadOnly", String::from("properties are read-only")),
        _ => Reply::Error(UNKNOWN_METHOD, format!("unknown method {}", m.member)),
    }
}

pub fn properties_interface(m: &Message) -> String {
    Reader::new(&m.body).str().unwrap_or_default()
}

pub fn introspect(interface: &str, props: Option<Vec<(&'static str, Value)>>, methods: &str, children: &[&str]) -> Reply {
    let mut xml = String::from("<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n");
    xml.push_str(" <interface name=\"org.freedesktop.DBus.Peer\"><method name=\"Ping\"/></interface>\n");
    xml.push_str(" <interface name=\"org.freedesktop.DBus.Introspectable\"><method name=\"Introspect\"><arg type=\"s\" direction=\"out\"/></method></interface>\n");
    xml.push_str(" <interface name=\"org.freedesktop.DBus.Properties\"><method name=\"Get\"><arg type=\"s\" direction=\"in\"/><arg type=\"s\" direction=\"in\"/><arg type=\"v\" direction=\"out\"/></method><method name=\"GetAll\"><arg type=\"s\" direction=\"in\"/><arg type=\"a{sv}\" direction=\"out\"/></method></interface>\n");
    if !interface.is_empty() {
        xml.push_str(&format!(" <interface name=\"{}\">\n", interface));
        xml.push_str(methods);
        if let Some(props) = props {
            for (name, value) in props {
                xml.push_str(&format!("  <property name=\"{}\" type=\"{}\" access=\"read\"/>\n", name, value.signature()));
            }
        }
        xml.push_str(" </interface>\n");
    }
    for child in children {
        xml.push_str(&format!(" <node name=\"{}\"/>\n", child));
    }
    xml.push_str("</node>\n");
    one_str(&xml)
}

pub fn serve(bus: &mut Bus, mut handle: impl FnMut(&mut Bus, &Message) -> Reply) {
    while let Some(m) = bus.next() {
        for fd in m.fds.iter() {
            sys::close(*fd);
        }
        if m.kind != METHOD_CALL {
            continue;
        }
        let reply = if m.interface == "org.freedesktop.DBus.Peer" { Reply::Empty } else { handle(bus, &m) };
        if m.flags & NO_REPLY_EXPECTED != 0 {
            if let Reply::Fd(fd) = reply {
                sys::close(fd);
            }
            continue;
        }
        let out = match reply {
            Reply::Empty => Message::reply_to(&m),
            Reply::Body(sig, w) => Message::reply_to(&m).with_body(sig, w),
            Reply::Fd(fd) => {
                let mut w = Writer::default();
                w.u32(0);
                let mut out = Message::reply_to(&m).with_body("h", w);
                out.fds.push(fd);
                out
            }
            Reply::Error(name, text) => Message::error_to(&m, name, &text),
        };
        bus.send(&out);
        for fd in out.fds {
            sys::close(fd);
        }
    }
}

pub fn unix_user_of(bus: &mut Bus, sender: &str) -> Option<u32> {
    let mut w = Writer::default();
    w.str(sender);
    let request = Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixUser").with_body("s", w);
    let reply = bus.call(request).ok()?;
    if reply.kind != METHOD_RETURN {
        return None;
    }
    Reader::new(&reply.body).u32()
}

pub fn run(socket: &str, name: &str, program: &str, once: bool, mut handle: impl FnMut(&mut Bus, &Message) -> Reply) -> i32 {
    let mut waited = false;
    loop {
        if !crate_fs_exists(socket) {
            if once {
                hxstd::println!("{}: no system bus at {}", program, socket);
                return 1;
            }
            if !waited {
                hxstd::println!("{}: waiting for the system bus at {}", program, socket);
                waited = true;
            }
            sys::sleep_ms(5000);
            continue;
        }
        let mut bus = match Bus::connect(socket) {
            Ok(bus) => bus,
            Err(e) => {
                hxstd::println!("{}: {}", program, e);
                if once {
                    return 1;
                }
                sys::sleep_ms(5000);
                continue;
            }
        };
        let mut w = Writer::default();
        w.str(name);
        w.u32(4);
        let request = Message::call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName").with_body("su", w);
        match bus.call(request) {
            Ok(reply) if Reader::new(&reply.body).u32() == Some(1) => hxstd::println!("{}: {} on the system bus as {}", program, name, bus.unique),
            Ok(_) => {
                hxstd::println!("{}: {} is already owned by another service", program, name);
                return 0;
            }
            Err(e) => {
                hxstd::println!("{}: cannot own {}: {}", program, name, e);
                return 1;
            }
        }
        serve(&mut bus, &mut handle);
        hxstd::println!("{}: the system bus went away", program);
        if once {
            return 0;
        }
        sys::sleep_ms(2000);
    }
}

fn crate_fs_exists(path: &str) -> bool {
    hxstd::fs::exists(path)
}
