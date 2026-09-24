use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{fs, sys, users};

use super::basic::print_kv;
use super::files::human;
use super::{parse_opts, Builtin};
use crate::shell::{hostname as read_hostname, write, Io, Shell};
use crate::{errln, outln};

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "uname", usage: "uname [-a]", help: "system name", group: "system", run: uname },
        Builtin { name: "whoami", usage: "whoami", help: "effective user name", group: "system", run: whoami },
        Builtin { name: "id", usage: "id", help: "user and group ids", group: "system", run: id },
        Builtin { name: "hostname", usage: "hostname [--root dir] [name]", help: "show or set the host name", group: "system", run: hostname },
        Builtin { name: "date", usage: "date [+%s]", help: "current date and time (UTC from the CMOS clock)", group: "system", run: date },
        Builtin { name: "uptime", usage: "uptime", help: "time since boot", group: "system", run: uptime },
        Builtin { name: "free", usage: "free [-h]", help: "memory usage", group: "system", run: free },
        Builtin { name: "ps", usage: "ps", help: "list processes", group: "system", run: ps },
        Builtin { name: "kill", usage: "kill pid...", help: "terminate processes", group: "system", run: kill },
        Builtin { name: "syscalltrace", usage: "syscalltrace [name]", help: "log the system calls of processes whose name contains NAME to the serial port (no name: stop)", group: "system", run: syscalltrace },
        Builtin { name: "dmesg", usage: "dmesg", help: "kernel log", group: "system", run: |_, _, io| cat_proc(io, "/proc/dmesg") },
        Builtin { name: "cpuinfo", usage: "cpuinfo", help: "processor information", group: "system", run: |_, _, io| cat_proc(io, "/proc/cpuinfo") },
        Builtin { name: "lsusb", usage: "lsusb", help: "USB controllers and devices", group: "system", run: lsusb },
        Builtin { name: "usb", usage: "usb", help: "same as lsusb", group: "system", run: lsusb },
        Builtin { name: "gpuinfo", usage: "gpuinfo", help: "graphics adapter and framebuffer", group: "system", run: gpuinfo },
        Builtin { name: "mouse", usage: "mouse", help: "pointer state", group: "system", run: |_, _, io| table_proc(io, "/proc/mouse") },
        Builtin { name: "drivers", usage: "drivers", help: "registered video drivers", group: "system", run: drivers },
        Builtin { name: "modules", usage: "modules", help: "loaded driver modules and their memory budget", group: "system", run: modules },
        Builtin { name: "modinfo", usage: "modinfo [NAME]", help: "details of a loaded driver module", group: "system", run: modinfo },
        Builtin { name: "interrupts", usage: "interrupts", help: "interrupt lines and their owners", group: "system", run: interrupts },
        Builtin { name: "version", usage: "version", help: "HamixOS version", group: "system", run: version },
        Builtin { name: "fetch", usage: "fetch", help: "system summary", group: "system", run: fetch },
        Builtin { name: "sync", usage: "sync", help: "flush filesystems to disk", group: "system", run: |_, _, _| { sys::sync(); 0 } },
        Builtin { name: "reboot", usage: "reboot", help: "restart the computer (root)", group: "system", run: |_, _, io| power(io, sys::POWER_REBOOT) },
        Builtin { name: "halt", usage: "halt", help: "stop the system (root)", group: "system", run: |_, _, io| power(io, sys::POWER_HALT) },
        Builtin { name: "poweroff", usage: "poweroff", help: "power off (root)", group: "system", run: |_, _, io| power(io, sys::POWER_OFF) },
        Builtin { name: "cmd", usage: "cmd [list | add name program [args] | rm name]", help: "dynamic command registry", group: "system", run: cmd },
        Builtin { name: "startx", usage: "startx", help: "start the Nook desktop", group: "system", run: startx },
        Builtin { name: "edit", usage: "edit file", help: "open a file in the hed editor", group: "system", run: edit },
        Builtin { name: "session", usage: "session [autostart on|off]", help: "login session settings", group: "system", run: session },
    ]
}

fn cat_proc(io: Io, path: &str) -> i32 {
    match fs::read_to_string(path) {
        Some(text) => {
            write(io.out, &text);
            0
        }
        None => {
            errln!(io, "{}: unavailable", path);
            1
        }
    }
}

fn table_proc(io: Io, path: &str) -> i32 {
    let Some(text) = fs::read_to_string(path) else {
        return 1;
    };
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('\t') {
            print_kv(io, k, v);
        }
    }
    0
}

fn uname(_: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.iter().any(|a| a == "-a") {
        outln!(io, "HamixOS {} 0.6.1 #1 {} hsh", read_hostname(), hamix_std::sys::ARCH);
    } else if args.iter().any(|a| a == "-r") {
        outln!(io, "0.6.1");
    } else {
        outln!(io, "HamixOS");
    }
    0
}

fn whoami(_: &mut Shell, _: &[String], io: Io) -> i32 {
    outln!(io, "{}", users::name_of(sys::geteuid() as u32));
    0
}

fn id(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let (uid, euid) = match args.get(1) {
        Some(name) => match users::find("", name) {
            Some(u) => (u.uid, u.uid),
            None => {
                errln!(io, "id: {}: no such user", name);
                return 1;
            }
        },
        None => (sys::getuid() as u32, sys::geteuid() as u32),
    };
    let name = users::name_of(uid);
    let mut line = format!("uid={}({}) gid={}({})", uid, name, uid, name);
    if euid != uid {
        line.push_str(&format!(" euid={}({})", euid, users::name_of(euid)));
    }
    if users::is_sudoer(&name) {
        line.push_str(" groups=sudo");
    }
    outln!(io, "{}", line);
    0
}

fn hostname(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root"]);
    let root = opts.value("root").unwrap_or_default();
    match opts.rest.first() {
        None => outln!(io, "{}", read_hostname()),
        Some(name) => {
            let valid = !name.is_empty() && name.len() <= 63 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if !valid {
                errln!(io, "hostname: invalid name");
                return 1;
            }
            let path = format!("{}/etc/hostname", root.trim_end_matches('/'));
            if !fs::write(&path, format!("{}\n", name).as_bytes()) {
                errln!(io, "hostname: cannot write {} (try sudo)", path);
                return 1;
            }
        }
    }
    0
}

pub fn civil(epoch: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (epoch / 86400) as i64;
    let secs = (epoch % 86400) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
}

fn date(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let now = sys::realtime().sec as u64;
    if args.get(1).map(|s| s.as_str()) == Some("+%s") {
        outln!(io, "{}", now);
        return 0;
    }
    let (y, m, d, hh, mm, ss) = civil(now);
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    outln!(io, "{} {} {:>2} {:02}:{:02}:{:02} UTC {}", DAYS[(now / 86400 % 7) as usize], MONTHS[(m - 1) as usize], d, hh, mm, ss, y);
    0
}

fn format_duration(ms: u64) -> String {
    let secs = ms / 1000;
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{}d {}h {}m", d, h, m)
    } else if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m {}s", m, s)
    }
}

fn syscalltrace(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let name = args.get(1).map(|s| s.as_str()).unwrap_or("");
    if sys::trace_syscalls(name) < 0 {
        errln!(io, "syscalltrace: only root can trace system calls");
        return 1;
    }
    0
}

fn uptime(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let info = sys::sysinfo();
    outln!(io, "up {}, {} processes", format_duration(info.uptime_ms), info.processes);
    0
}

fn free(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let info = sys::sysinfo();
    let h = args.iter().any(|a| a == "-h") || true;
    let f = |v: u64| if h { human(v) } else { format!("{}", v / 1024) };
    outln!(io, "\x1b[1m{:<8}{:>10}{:>10}{:>10}\x1b[0m", "", "total", "used", "free");
    outln!(io, "{:<8}{:>10}{:>10}{:>10}", "Mem:", f(info.mem_total), f(info.mem_used()), f(info.mem_free));
    outln!(io, "{:<8}{:>10}{:>10}{:>10}", "Kernel:", f(info.heap_total), f(info.heap_total - info.heap_free), f(info.heap_free));
    0
}

fn ps(_: &mut Shell, _: &[String], io: Io) -> i32 {
    outln!(io, "\x1b[1m{:>5} {:>5} {:<8} {:<2} {:<4} {:<6} {:>7} {:>8}  {}\x1b[0m", "PID", "PPID", "USER", "S", "TTY", "ABI", "HEAP", "CPU", "COMMAND");
    for p in sys::proc_list() {
        outln!(
            io,
            "{:>5} {:>5} {:<8} {:<2} tty{:<1} {:<6} {:>7} {:>7}s  {}",
            p.pid,
            p.parent,
            users::name_of(p.uid),
            p.state,
            p.tty,
            p.abi,
            human(p.heap_kb * 1024),
            p.cpu_ms / 1000,
            p.name
        );
    }
    0
}

fn kill(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut status = 0;
    for arg in args[1..].iter().filter(|a| !a.starts_with('-')) {
        match arg.parse::<i64>() {
            Ok(pid) => {
                let r = sys::kill(pid);
                if r < 0 {
                    errln!(io, "kill: {}: {}", pid, sys::error_name(r));
                    status = 1;
                }
            }
            Err(_) => {
                errln!(io, "kill: {}: not a pid", arg);
                status = 1;
            }
        }
    }
    status
}

fn lsusb(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let text = fs::read_to_string("/proc/usb").unwrap_or_default();
    let mut any = false;
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("controller") if f.len() >= 6 => {
                outln!(io, "\x1b[1m{:<5}\x1b[0m {}  {:<10} {}", f[1], f[2], f[4], f[5]);
                any = true;
            }
            Some("device") if f.len() >= 6 => {
                outln!(io, "  \x1b[92m{}\x1b[0m  port {:<2} {:<5} {}", f[1], f[3], f[4], f[5]);
            }
            _ => {}
        }
    }
    if !any {
        outln!(io, "no USB host controllers");
    }
    0
}

fn drivers(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let text = fs::read_to_string("/proc/drivers").unwrap_or_default();
    outln!(io, "\x1b[1m{:<26} {:<8} {:<10} {}\x1b[0m", "NAME", "VERSION", "KIND", "STATUS");
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 4 {
            outln!(io, "{:<26} {:<8} {:<10} {}", f[0], f[1], f[2], f[3]);
        }
    }
    gpu_lines(io);
    0
}

fn pci_ids_name(id: &str) -> Option<String> {
    let (vendor, device) = id.split_once(':')?;
    let text = ["/opt/linux/usr/share/hwdata/pci.ids", "/usr/share/hwdata/pci.ids", "/opt/linux/usr/share/misc/pci.ids"]
        .iter()
        .find_map(|p| fs::read_to_string(p))?;
    let mut inside = false;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with('\t') {
            inside = line.starts_with(vendor);
            continue;
        }
        if !inside || line.starts_with("\t\t") {
            continue;
        }
        let entry = line.trim_start();
        if entry.starts_with(device) {
            return entry.split_once("  ").map(|(_, name)| String::from(name.trim()));
        }
    }
    None
}

fn gpuinfo(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let text = fs::read_to_string("/proc/gpu").unwrap_or_default();
    let field = |key: &str| text.lines().find(|l| l.split('\t').next() == Some(key)).and_then(|l| l.split('\t').nth(1)).unwrap_or("");
    let id = field("pciid");
    let mut chipset = String::from(field("chipset"));
    if chipset.starts_with("Unknown vendor") || chipset.contains(&format!(" {}", id)) {
        if let Some(name) = pci_ids_name(id) {
            chipset = name;
        }
    }
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 2 {
            continue;
        }
        if f[0] == "chipset" {
            print_kv(io, f[0], &chipset);
        } else {
            print_kv(io, f[0], &f[1..].join("  "));
        }
    }
    0
}

fn gpu_lines(io: Io) {
    let text = fs::read_to_string("/proc/gpu").unwrap_or_default();
    let field = |key: &str| {
        text.lines()
            .find(|l| l.split('\t').next() == Some(key))
            .and_then(|l| l.split('\t').nth(1))
            .unwrap_or("")
    };
    let provider = field("provider");
    if provider.is_empty() || provider == "none" {
        outln!(io, "acceleration: none (the compositor draws and scans out on the CPU)");
        let reason = field("reason");
        if !reason.is_empty() {
            outln!(io, "  driver module failed: {}", reason);
        }
        return;
    }
    outln!(io, "acceleration: {} [{}]", provider, field("caps"));
    outln!(io, "  scanout flushes: {} ({} pixels transferred)", field("flushes"), field("pixels"));
    outln!(io, "  pointer: {} cursor", field("cursor"));
}

fn modinfo(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let wanted = args.get(1).cloned().unwrap_or_default();
    let text = fs::read_to_string("/proc/modules_detail").unwrap_or_default();
    let mut shown = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.first() != Some(&"module") || f.len() < 10 {
            continue;
        }
        if !wanted.is_empty() && f[1] != wanted {
            continue;
        }
        shown += 1;
        outln!(io, "\x1b[1m{}\x1b[0m", f[1]);
        outln!(io, "  version     {}", f[2]);
        outln!(io, "  memory      {} KiB of {} KiB allowed", f[3].parse::<u64>().unwrap_or(0) / 1024, f[4].parse::<u64>().unwrap_or(0) / 1024);
        outln!(io, "  signature   {}", f[5]);
        outln!(io, "  pages       {}", if f[6] == "wx" { "text read-only and executable, data no-execute" } else { "writable and executable" });
        outln!(io, "  state       {}", f[7]);
        outln!(io, "  interrupts  {}", f[8]);
        outln!(io, "  devices     {}", f[9]);
        if let Some(entries) = sys::read_dir(&format!("/sys/module/{}/parameters", f[1])) {
            let names: Vec<String> = entries.iter().filter(|e| e.name != "." && e.name != "..").map(|e| e.name.clone()).collect();
            if !names.is_empty() {
                outln!(io, "  parameters  {}", names.join(", "));
            }
        }
    }
    if shown == 0 {
        outln!(io, "{}", if wanted.is_empty() { String::from("no driver modules loaded") } else { format!("{}: not loaded", wanted) });
        return 1;
    }
    0
}

fn interrupts(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let text = fs::read_to_string("/proc/interrupts").unwrap_or_default();
    outln!(io, "\x1b[1m{:>6} {:<6} {:>12}  {}\x1b[0m", "VECTOR", "KIND", "COUNT", "OWNER");
    let mut rows = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 4 && f[0].parse::<u32>().is_ok() {
            outln!(io, "{:>6} {:<6} {:>12}  {}", f[0], f[1], f[2], f[3]);
            rows += 1;
        }
    }
    if rows == 0 {
        outln!(io, "no driver module has taken an interrupt line");
    }
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 2 && matches!(f[0], "timer" | "spurious" | "work" | "dropped") {
            outln!(io, "{:>6} {:<6} {:>12}", "", f[0], f[1]);
        }
    }
    0
}

fn modules(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let text = fs::read_to_string("/proc/modules").unwrap_or_default();
    let mut rows = 0;
    outln!(io, "\x1b[1m{:<20} {:>10} {:>10} {:>10} {:>8}\x1b[0m", "MODULE", "RESIDENT", "TEXT", "DATA", "DEVICES");
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.first() == Some(&"module") && f.len() >= 6 {
            outln!(io, "{:<20} {:>10} {:>10} {:>10} {:>8}", f[1], f[2], f[3], f[4], f[5]);
            rows += 1;
        }
    }
    if rows == 0 {
        outln!(io, "no driver modules loaded");
    }
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.first() == Some(&"device") && f.len() >= 5 {
            outln!(io, "  {} drives {} ({})", f[1], f[3], f[4]);
        }
    }
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.first() == Some(&"budget") && f.len() >= 3 {
            let used: u64 = f[1].parse().unwrap_or(0);
            let total: u64 = f[2].parse().unwrap_or(1);
            outln!(io, "budget: {} KiB of {} MiB used", used / 1024, total / (1024 * 1024));
        }
    }
    gpu_lines(io);
    0
}

fn version(_: &mut Shell, _: &[String], io: Io) -> i32 {
    outln!(io, "HamixOS 0.6.1 -- Rust kernel, hext filesystem, Nook desktop, hsh shell");
    0
}

fn fetch(_: &mut Shell, _: &[String], io: Io) -> i32 {
    let info = sys::sysinfo();
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .and_then(|t| t.lines().find(|l| l.starts_with("model name")).map(|l| String::from(l.split(':').nth(1).unwrap_or("").trim())))
        .unwrap_or_default();
    let gpu = fs::read_to_string("/proc/gpu").and_then(|t| t.lines().next().map(|l| String::from(l.split('\t').nth(1).unwrap_or("")))).unwrap_or_default();
    let root = fs::read_to_string("/proc/mounts").and_then(|t| t.lines().next().map(|l| String::from(l.split('\t').next().unwrap_or("")))).unwrap_or_default();
    let logo = [
        "\x1b[96m  _   _ \x1b[0m",
        "\x1b[96m | | | |\x1b[0m",
        "\x1b[96m | |_| |\x1b[0m",
        "\x1b[96m |  _  |\x1b[0m",
        "\x1b[96m | | | |\x1b[0m",
        "\x1b[96m |_| |_|\x1b[0m",
        "        ",
    ];
    let user = users::name_of(sys::geteuid() as u32);
    let lines = [
        format!("\x1b[1m{}@{}\x1b[0m", user, read_hostname()),
        format!("\x1b[96mOS\x1b[0m      HamixOS 0.6.1 {}", hamix_std::sys::ARCH),
        format!("\x1b[96mUptime\x1b[0m  {}", format_duration(info.uptime_ms)),
        format!("\x1b[96mShell\x1b[0m   hsh"),
        format!("\x1b[96mCPU\x1b[0m     {}", cpu),
        format!("\x1b[96mGPU\x1b[0m     {}", gpu),
        format!("\x1b[96mMemory\x1b[0m  {} / {}  root: {}", human(info.mem_used()), human(info.mem_total), root),
    ];
    for (i, line) in lines.iter().enumerate() {
        outln!(io, "{}   {}", logo.get(i).copied().unwrap_or("        "), line);
    }
    0
}

fn power(io: Io, mode: u64) -> i32 {
    let r = sys::power(mode);
    if r < 0 {
        errln!(io, "permission denied (try: sudo {})", match mode {
            sys::POWER_REBOOT => "reboot",
            sys::POWER_OFF => "poweroff",
            _ => "halt",
        });
        return 1;
    }
    0
}

fn cmd(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    match args.get(1).map(|s| s.as_str()).unwrap_or("list") {
        "list" | "ls" => {
            let list = sys::cmd_list();
            if list.is_empty() {
                outln!(io, "the command registry is empty");
            }
            for c in list {
                outln!(io, "{:<18} {} {}", c.name, c.path, c.args.join(" "));
            }
            0
        }
        "add" if args.len() >= 4 => {
            let program = sh.resolve_program(&args[3]).unwrap_or_else(|| args[3].clone());
            let r = sys::cmd_register(&args[2], &program, &args[4..]);
            if r < 0 {
                errln!(io, "cmd: {}", sys::error_name(r));
                return 1;
            }
            outln!(io, "cmd: '{}' now runs {}", args[2], program);
            0
        }
        "rm" | "remove" if args.len() >= 3 => {
            let r = sys::cmd_unregister(&args[2]);
            if r < 0 {
                errln!(io, "cmd: {}", sys::error_name(r));
                return 1;
            }
            0
        }
        _ => {
            errln!(io, "usage: cmd [list | add name program [args] | rm name]");
            1
        }
    }
}

fn run_foreground(sh: &mut Shell, program: &str, args: &[&str], io: Io) -> i32 {
    let pid = sh.spawn_with_env(program, args, sys::SPAWN_FOREGROUND, io);
    if pid < 0 {
        errln!(io, "{}: {}", program, sys::error_name(pid));
        return 127;
    }
    sh.wait_foreground(pid)
}

fn start_session_bus() {
    let uid = sys::getuid().max(0);
    let runtime = hamix_std::env::var("XDG_RUNTIME_DIR")
        .map(String::from)
        .unwrap_or_else(|| format!("/tmp/runtime-{}", uid));
    sys::mkdir(&runtime);
    let socket = format!("{}/bus", runtime);
    if sys::stat(&socket).is_ok() {
        return;
    }
    let daemon = "/opt/linux/usr/bin/dbus-daemon";
    if sys::stat(daemon).is_err() {
        return;
    }
    let address = format!("--address=unix:path={}", socket);
    sys::spawn(daemon, &["--session", address.as_str(), "--nopidfile"], sys::SPAWN_DETACH);
}

fn startx(sh: &mut Shell, _: &[String], io: Io) -> i32 {
    if sys::service_lookup("hxserver") > 0 {
        errln!(io, "startx: the desktop is already running");
        return 1;
    }
    start_session_bus();
    let desktop = fs::read_to_string("/etc/hamix/login.conf")
        .and_then(|t| super::basic::parse_conf(&t).into_iter().find(|(k, _)| k == "desktop").map(|(_, v)| v))
        .unwrap_or_else(|| String::from("/usr/bin/hxserver"));
    let status = run_foreground(sh, &desktop, &[], io);
    write(io.out, "\x1b[0m\x1b[2J\x1b[H");
    status
}

fn edit(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(file) = args.get(1) else {
        errln!(io, "usage: edit file");
        return 1;
    };
    let file = super::files::absolute(file);
    run_foreground(sh, "/usr/bin/hed", &[file.as_str()], io)
}

fn session(_: &mut Shell, args: &[String], io: Io) -> i32 {
    const CONF: &str = "/etc/hamix/login.conf";
    match (args.get(1).map(|s| s.as_str()), args.get(2).map(|s| s.as_str())) {
        (None, _) => {
            let text = fs::read_to_string(CONF).unwrap_or_default();
            for (k, v) in super::basic::parse_conf(&text) {
                print_kv(io, &k, &v);
            }
            0
        }
        (Some("autostart"), Some(value @ ("on" | "off"))) => {
            let v = if value == "on" { "yes" } else { "no" };
            if super::basic::set_conf_value(CONF, "autostart_desktop", v) {
                outln!(io, "Nook will {}start automatically after login", if v == "yes" { "" } else { "not " });
                0
            } else {
                errln!(io, "session: cannot write {} (try sudo)", CONF);
                1
            }
        }
        _ => {
            errln!(io, "usage: session [autostart on|off]");
            1
        }
    }
}
