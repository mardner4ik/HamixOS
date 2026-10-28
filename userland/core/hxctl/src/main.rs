#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::{entry, env, eprintln, print, println, sys};

const CONTROL_SERVICE: &str = "hamix.svc";
const TIMEOUT_MS: i64 = 5000;

fn usage() -> i32 {
    println!("usage: hxctl status [SERVICE]       services and their state");
    println!("       hxctl start|stop|restart SERVICE");
    println!("       hxctl log [SERVICE] [LINES]  the journal of a service (hxsvc when omitted)");
    println!("       hxctl reload                 re-read /etc/hamix/services");
    println!("       hxctl linux on|off|status    the Linux compatibility layer");
    println!("       hxctl abi                    the Hamix ABI version of this kernel");
    println!("       hxctl prof start|stop|reset|report   the kernel sampling profiler (hxprof)");
    println!("       hxctl poweroff|reboot");
    2
}

fn request(text: &str) -> Result<String, String> {
    let pid = sys::service_lookup(CONTROL_SERVICE);
    if pid < 0 {
        return Err(String::from("hxsvc is not running (the system was started with init=kernel)"));
    }
    let r = sys::msg_send(pid, text.as_bytes());
    if r < 0 {
        return Err(format!("cannot reach hxsvc: {}", sys::error_name(r)));
    }
    let mut out = String::new();
    let mut buf = [0u8; 4096];
    loop {
        let Some((sender, n)) = sys::msg_recv(&mut buf, TIMEOUT_MS) else {
            return Err(String::from("hxsvc did not answer"));
        };
        if sender != pid || n == 0 {
            continue;
        }
        out.push_str(&String::from_utf8_lossy(&buf[1..n]));
        if buf[0] == b'E' {
            return Ok(out);
        }
    }
}

fn forward(text: &str) -> i32 {
    match request(text) {
        Ok(reply) => {
            print!("{}", reply);
            if reply.starts_with("permission denied") || reply.starts_with("cannot ") || reply.starts_with("no service") || reply.starts_with("unknown request") {
                1
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("hxctl: {}", e);
            1
        }
    }
}

fn linux(args: &[String]) -> i32 {
    let (_, info) = sys::abi_info();
    let field = |key: &str| info.lines().find_map(|l| l.strip_prefix(key).map(|v| String::from(v.trim()))).unwrap_or_default();
    match args.first().map(|s| s.as_str()) {
        None | Some("status") => {
            println!("Linux compatibility layer: {}", field("linux"));
            println!("Linux processes running:   {}", field("linux_processes"));
            0
        }
        Some(op @ ("on" | "off")) => {
            let on = op == "on";
            let r = sys::linux_control(if on { sys::LINUX_ON } else { sys::LINUX_OFF });
            if r < 0 {
                eprintln!("hxctl: {}", if r == -1 { "permission denied: run it as root (sudo hxctl linux ...)" } else { sys::error_name(r) });
                return 1;
            }
            if !on {
                println!("Linux compatibility layer is off; {} Linux process(es) stopped", r);
            } else {
                println!("Linux compatibility layer is on");
            }
            match request(&format!("layer linux {}", op)) {
                Ok(reply) => print!("{}", reply),
                Err(e) => eprintln!("hxctl: {}", e),
            }
            0
        }
        Some(other) => {
            eprintln!("hxctl: linux {}: expected on, off or status", other);
            2
        }
    }
}

fn main() -> i32 {
    let args: Vec<String> = env::args().iter().skip(1).cloned().collect();
    let Some(command) = args.first() else {
        return forward("status");
    };
    let rest = &args[1..];
    match command.as_str() {
        "-h" | "--help" | "help" => {
            usage();
            0
        }
        "--version" => {
            println!("hxctl 0.1.0");
            0
        }
        "status" | "log" | "reload" | "poweroff" | "reboot" | "start" | "stop" | "restart" => {
            if matches!(command.as_str(), "start" | "stop" | "restart") && rest.len() != 1 {
                return usage();
            }
            let mut text = command.clone();
            for a in rest {
                text.push(' ');
                text.push_str(a);
            }
            forward(&text)
        }
        "linux" => linux(rest),
        "prof" => {
            let op = match rest.first().map(|s| s.as_str()) {
                Some("stop") => 0,
                Some("start") => 1,
                Some("reset") => 2,
                Some("report") | None => {
                    print!("{}", hxstd::fs::read_to_string("/proc/hxprof").unwrap_or_default());
                    return 0;
                }
                Some(_) => return usage(),
            };
            let r = unsafe { sys::syscall3(9104, op, 0, 0) };
            if r < 0 {
                eprintln!("hxctl: prof: {}", sys::error_name(r));
                return 1;
            }
            println!("hxprof is {}", if r == 1 { "running" } else { "stopped" });
            0
        }
        "abi" => {
            let (version, info) = sys::abi_info();
            println!("Hamix ABI {}.{}", version >> 8, version & 0xFF);
            print!("{}", info);
            0
        }
        _ => usage(),
    }
}

entry!(main);
