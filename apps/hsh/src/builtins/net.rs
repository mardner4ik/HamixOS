use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{net, sys};

use super::{parse_opts, Builtin};
use crate::shell::{write, Io, Shell};
use crate::{errln, outln};

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "ip", usage: "ip [addr]", help: "network adapters, addresses and link state", group: "network", run: ip },
        Builtin { name: "ifconfig", usage: "ifconfig", help: "same as ip", group: "network", run: ip },
        Builtin { name: "netmode", usage: "netmode [ethernet | wifi | off]", help: "show or switch the active network connection", group: "network", run: netmode },
        Builtin { name: "netconf", usage: "netconf dhcp | static ADDR/PREFIX [GATEWAY] [DNS]", help: "configure the active adapter (root)", group: "network", run: netconf },
        Builtin { name: "ping", usage: "ping [-c count] host", help: "send ICMP echo requests", group: "network", run: ping },
        Builtin { name: "nslookup", usage: "nslookup host", help: "resolve a host name with DNS", group: "network", run: nslookup },
        Builtin { name: "wifi", usage: "wifi [scan | connect SSID [PASSWORD] | disconnect]", help: "Wi-Fi networks", group: "network", run: wifi },
        Builtin { name: "wget", usage: "wget [-O file] http://host[:port]/path", help: "download over HTTP", group: "network", run: wget },
    ]
}

fn ip(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let status = net::status();
    if !status.available {
        errln!(io, "ip: networking is not available");
        return 1;
    }
    outln!(io, "mode: {}", status.mode);
    if status.interfaces.is_empty() {
        outln!(io, "no network adapters");
    }
    for i in status.interfaces {
        outln!(io, "\x1b[1m{}\x1b[0m  {}  {}", i.name, i.kind, i.driver);
        outln!(io, "    ether {}  link {}", i.mac, if i.link { "up" } else { "down" });
        if !i.address.is_empty() {
            outln!(io, "    inet {}  gateway {}  dns {}", i.address, if i.gateway.is_empty() { "-" } else { &i.gateway }, if i.dns.is_empty() { "-" } else { &i.dns });
        }
        if !i.ssid.is_empty() {
            outln!(io, "    ssid {}  signal {}%", i.ssid, i.signal);
        }
        outln!(io, "    state {}  rx {} packets  tx {} packets", i.state, i.rx_packets, i.tx_packets);
    }
    0
}

fn netmode(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(mode) = args.get(1) else {
        outln!(io, "{}", net::status().mode);
        return 0;
    };
    let value = match mode.as_str() {
        "ethernet" | "eth" | "lan" => net::MODE_ETHERNET,
        "wifi" | "wlan" | "wireless" => net::MODE_WIFI,
        "off" | "none" => net::MODE_OFF,
        _ => {
            errln!(io, "usage: netmode [ethernet | wifi | off]");
            return 2;
        }
    };
    let r = net::set_mode(value);
    if r < 0 {
        errln!(io, "netmode: {}", sys::error_name(r));
        return 1;
    }
    0
}

fn netconf(_: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() < 2 {
        errln!(io, "usage: netconf dhcp | static ADDR/PREFIX [GATEWAY] [DNS]");
        return 2;
    }
    let spec = args[1..].join(" ");
    let r = net::configure("", &spec);
    if r < 0 {
        errln!(io, "netconf: {}", sys::error_name(r));
        return 1;
    }
    0
}

fn resolve(io: &Io, host: &str) -> Option<[u8; 4]> {
    match net::resolve(host) {
        Ok(ip) => Some(ip),
        Err(e) => {
            errln!(io, "cannot resolve {}: {}", host, sys::error_name(e));
            None
        }
    }
}

fn ping(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["c"]);
    let Some(host) = opts.rest.first() else {
        errln!(io, "usage: ping [-c count] host");
        return 2;
    };
    let count: u16 = opts.value("c").and_then(|v| v.parse().ok()).unwrap_or(4);
    let Some(ip) = resolve(&io, host) else {
        return 1;
    };
    outln!(io, "PING {} ({})", host, net::format_ipv4(ip));
    let mut received = 0;
    let mut total = 0i64;
    for seq in 1..=count {
        let rtt = net::ping(ip, seq, 2000);
        if rtt >= 0 {
            received += 1;
            total += rtt;
            outln!(io, "reply from {}: seq={} time={} ms", net::format_ipv4(ip), seq, rtt);
        } else {
            outln!(io, "seq={}: {}", seq, sys::error_name(rtt));
        }
        if sys::poll_key().map(|k| k.code() == 3).unwrap_or(false) {
            break;
        }
        if seq < count {
            sys::sleep_ms(if rtt >= 0 { (1000 - rtt).max(0) as u64 } else { 200 });
        }
    }
    outln!(io, "--- {} ping statistics: {} sent, {} received{}", host, count, received, if received > 0 { format!(", avg {} ms", total / received as i64) } else { String::new() });
    if received > 0 { 0 } else { 1 }
}

fn nslookup(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(host) = args.get(1) else {
        errln!(io, "usage: nslookup host");
        return 2;
    };
    match resolve(&io, host) {
        Some(ip) => {
            outln!(io, "{} has address {}", host, net::format_ipv4(ip));
            0
        }
        None => 1,
    }
}

fn wifi(_: &mut Shell, args: &[String], io: Io) -> i32 {
    match args.get(1).map(|s| s.as_str()) {
        None | Some("scan") | Some("list") => {
            let networks = net::wifi_scan(args.get(1).is_some());
            if networks.is_empty() {
                let status = net::status();
                if !status.has_kind("wifi") {
                    errln!(io, "wifi: no Wi-Fi adapter");
                    return 1;
                }
                outln!(io, "no networks found yet (try: wifi scan)");
                return 0;
            }
            outln!(io, "{:<28} {:>7} {:>4} {:<6} BSSID", "SSID", "SIGNAL", "CH", "SEC");
            for n in networks {
                outln!(io, "{:<28} {:>6}% {:>4} {:<6} {}{}", n.ssid, n.signal, n.channel, n.security, n.bssid, if n.connected { "  *" } else { "" });
            }
            0
        }
        Some("connect") => {
            let Some(ssid) = args.get(2) else {
                errln!(io, "usage: wifi connect SSID [PASSWORD]");
                return 2;
            };
            let password = args.get(3).cloned().unwrap_or_default();
            let r = net::wifi_connect(ssid, &password);
            if r < 0 {
                errln!(io, "wifi: {}", sys::error_name(r));
                return 1;
            }
            outln!(io, "connecting to {}…", ssid);
            0
        }
        Some("disconnect") => {
            net::wifi_disconnect();
            0
        }
        Some(other) => {
            errln!(io, "wifi: unknown command {}", other);
            2
        }
    }
}

fn deliver(fd: i64, data: &[u8], body: &mut Vec<u8>) -> bool {
    if fd < 0 {
        body.extend_from_slice(data);
        return true;
    }
    let mut done = 0;
    while done < data.len() {
        let n = sys::write(fd as u64, &data[done..]);
        if n <= 0 {
            return false;
        }
        done += n as usize;
    }
    true
}

fn wget(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["O"]);
    let Some(url) = opts.rest.first() else {
        errln!(io, "usage: wget [-O file] http://host[:port]/path");
        return 2;
    };
    let Some(rest) = url.strip_prefix("http://") else {
        errln!(io, "wget: only http:// URLs are supported");
        return 2;
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, p.parse().unwrap_or(80)),
        None => (authority, 80u16),
    };
    let Some(ip) = resolve(&io, host) else {
        return 1;
    };
    let mut stream = match net::TcpStream::connect(ip, port, 10_000) {
        Ok(s) => s,
        Err(e) => {
            errln!(io, "wget: cannot connect to {}:{}: {}", host, port, sys::error_name(e));
            return 1;
        }
    };
    let request = format!("GET {} HTTP/1.0\r\nHost: {}\r\nUser-Agent: HamixOS-wget/0.5\r\nConnection: close\r\n\r\n", path, host);
    if stream.send(request.as_bytes()) < 0 {
        errln!(io, "wget: send failed");
        return 1;
    }
    let target = opts.value("O").filter(|f| f != "-");
    let mut head_bytes = Vec::new();
    let mut body = Vec::new();
    let mut header_done = false;
    let mut fd = -1i64;
    let mut received = 0usize;
    let mut buf = alloc::vec![0u8; 64 * 1024];
    loop {
        let n = stream.recv(&mut buf, 15_000);
        if n <= 0 {
            break;
        }
        let data = &buf[..n as usize];
        if !header_done {
            head_bytes.extend_from_slice(data);
            let Some(split) = head_bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            header_done = true;
            let rest = head_bytes.split_off(split + 4);
            head_bytes.truncate(split);
            if let Some(file) = &target {
                fd = sys::create(file);
                if fd < 0 {
                    errln!(io, "wget: cannot write {}", file);
                    return 1;
                }
            }
            received += rest.len();
            if !deliver(fd, &rest, &mut body) {
                errln!(io, "wget: write failed");
                sys::close(fd as u64);
                return 1;
            }
            continue;
        }
        received += data.len();
        if !deliver(fd, data, &mut body) {
            errln!(io, "wget: write failed (disk full?)");
            sys::close(fd as u64);
            return 1;
        }
    }
    if fd >= 0 {
        sys::close(fd as u64);
    }
    if !header_done {
        errln!(io, "wget: malformed response ({} bytes)", head_bytes.len());
        return 1;
    }
    let head = String::from_utf8_lossy(&head_bytes).into_owned();
    let status = head.lines().next().unwrap_or("");
    errln!(io, "{} ({} bytes)", status, received);
    if fd < 0 {
        write(io.out, &String::from_utf8_lossy(&body));
    }
    if status.contains(" 200") { 0 } else { 1 }
}
