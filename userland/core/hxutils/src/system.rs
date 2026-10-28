use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{fs, sys, users};

use crate::print_kv;
use crate::human;
use crate::parse_opts;
use crate::{civil, format_duration, hostname as read_hostname, run_foreground, write, Io};
use crate::{errln, outln};

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

pub fn uname(args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let all = opts.has('a') || opts.long.iter().any(|l| l == "all");
    let host = read_hostname();
    let fields: [(char, &str); 5] = [('s', "HamixOS"), ('n', host.as_str()), ('r', "0.6.1"), ('v', "#1"), ('m', hxstd::sys::ARCH)];
    let mut shown: Vec<&str> = fields.iter().filter(|(c, _)| all || opts.has(*c)).map(|(_, v)| *v).collect();
    if all || opts.has('o') {
        shown.push("HamixOS");
    }
    if shown.is_empty() {
        shown.push("HamixOS");
    }
    outln!(io, "{}", shown.join(" "));
    0
}

pub fn dmesg(_: &[String], io: Io) -> i32 {
    cat_proc(io, "/proc/dmesg")
}

pub fn cpuinfo(_: &[String], io: Io) -> i32 {
    cat_proc(io, "/proc/cpuinfo")
}

pub fn mouse(_: &[String], io: Io) -> i32 {
    table_proc(io, "/proc/mouse")
}

pub fn sync(_: &[String], _: Io) -> i32 {
    sys::sync();
    0
}

pub fn reboot(_: &[String], io: Io) -> i32 {
    power(io, sys::POWER_REBOOT)
}

pub fn halt(_: &[String], io: Io) -> i32 {
    power(io, sys::POWER_HALT)
}

pub fn poweroff(_: &[String], io: Io) -> i32 {
    power(io, sys::POWER_OFF)
}

pub fn whoami(_: &[String], io: Io) -> i32 {
    outln!(io, "{}", users::name_of(sys::geteuid() as u32));
    0
}

pub fn id(args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let names = users::group_ids();
    let group_name = |gid: u32| names.iter().find(|(_, g)| *g == gid).map(|(n, _)| n.clone()).unwrap_or_else(|| format!("{}", gid));
    let (uid, euid, gid, egid, groups) = match opts.rest.first() {
        Some(name) => match users::find("", name) {
            Some(u) => {
                let mut groups = alloc::vec![u.gid];
                for g in users::member_of("", &u.name) {
                    if let Some((_, id)) = names.iter().find(|(n, _)| *n == g) {
                        if !groups.contains(id) {
                            groups.push(*id);
                        }
                    }
                }
                (u.uid, u.uid, u.gid, u.gid, groups)
            }
            None => {
                errln!(io, "id: {}: no such user", name);
                return 1;
            }
        },
        None => {
            let egid = sys::getegid();
            let mut groups = alloc::vec![egid];
            for g in sys::groups() {
                if !groups.contains(&g) {
                    groups.push(g);
                }
            }
            (sys::getuid() as u32, sys::geteuid() as u32, sys::getgid(), egid, groups)
        }
    };
    let real = opts.has('r');
    let named = opts.has('n');
    if opts.has('u') {
        let shown = if real { uid } else { euid };
        outln!(io, "{}", if named { users::name_of(shown) } else { format!("{}", shown) });
        return 0;
    }
    if opts.has('g') {
        let shown = if real { gid } else { egid };
        outln!(io, "{}", if named { group_name(shown) } else { format!("{}", shown) });
        return 0;
    }
    if opts.has('G') {
        let list: Vec<String> = groups.iter().map(|g| if named { group_name(*g) } else { format!("{}", g) }).collect();
        outln!(io, "{}", list.join(" "));
        return 0;
    }
    let mut line = format!("uid={}({}) gid={}({})", uid, users::name_of(uid), gid, group_name(gid));
    if euid != uid {
        line.push_str(&format!(" euid={}({})", euid, users::name_of(euid)));
    }
    if egid != gid {
        line.push_str(&format!(" egid={}({})", egid, group_name(egid)));
    }
    let list: Vec<String> = groups.iter().map(|g| format!("{}({})", g, group_name(*g))).collect();
    line.push_str(&format!(" groups={}", list.join(",")));
    outln!(io, "{}", line);
    0
}

pub fn hostname(args: &[String], io: Io) -> i32 {
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

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn strftime(format: &str, now: u64) -> String {
    let (y, m, d, hh, mm, ss) = civil(now);
    let day_of_year = now / 86400 - civil_days(y) + 1;
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{}", y)),
            Some('y') => out.push_str(&format!("{:02}", y % 100)),
            Some('m') => out.push_str(&format!("{:02}", m)),
            Some('d') => out.push_str(&format!("{:02}", d)),
            Some('e') => out.push_str(&format!("{:>2}", d)),
            Some('H') => out.push_str(&format!("{:02}", hh)),
            Some('M') => out.push_str(&format!("{:02}", mm)),
            Some('S') => out.push_str(&format!("{:02}", ss)),
            Some('s') => out.push_str(&format!("{}", now)),
            Some('j') => out.push_str(&format!("{:03}", day_of_year)),
            Some('a') => out.push_str(DAYS[(now / 86400 % 7) as usize]),
            Some('b') | Some('h') => out.push_str(MONTHS[(m - 1) as usize]),
            Some('F') => out.push_str(&format!("{}-{:02}-{:02}", y, m, d)),
            Some('T') => out.push_str(&format!("{:02}:{:02}:{:02}", hh, mm, ss)),
            Some('R') => out.push_str(&format!("{:02}:{:02}", hh, mm)),
            Some('D') => out.push_str(&format!("{:02}/{:02}/{:02}", m, d, y % 100)),
            Some('Z') => out.push_str("UTC"),
            Some('z') => out.push_str("+0000"),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

fn civil_days(year: i64) -> u64 {
    let y = year - 1;
    (y * 365 + y / 4 - y / 100 + y / 400 - 719162) as u64
}

pub fn date(args: &[String], io: Io) -> i32 {
    let mut now = sys::realtime().sec as u64;
    let mut format = String::from("%a %b %e %H:%M:%S %Z %Y");
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if let Some(f) = arg.strip_prefix('+') {
            format = String::from(f);
        } else if arg == "-u" || arg == "--utc" || arg == "-R" {
            if arg == "-R" {
                format = String::from("%a, %d %b %Y %H:%M:%S %z");
            }
        } else if arg == "-d" || arg == "--date" {
            match args.get(i + 1).and_then(|v| v.strip_prefix('@')).and_then(|v| v.parse().ok()) {
                Some(epoch) => now = epoch,
                None => {
                    errln!(io, "date: only -d @SECONDS is supported");
                    return 1;
                }
            }
            i += 1;
        } else if let Some(v) = arg.strip_prefix("--date=@") {
            now = v.parse().unwrap_or(now);
        } else {
            errln!(io, "date: setting the clock is not supported (usage: date [-u] [-d @SECONDS] [+FORMAT])");
            return 1;
        }
        i += 1;
    }
    outln!(io, "{}", strftime(&format, now));
    0
}

pub fn syscalltrace(args: &[String], io: Io) -> i32 {
    let name = args.get(1).map(|s| s.as_str()).unwrap_or("");
    if sys::trace_syscalls(name) < 0 {
        errln!(io, "syscalltrace: only root can trace system calls");
        return 1;
    }
    0
}

pub fn uptime(_: &[String], io: Io) -> i32 {
    let info = sys::sysinfo();
    outln!(io, "up {}, {} processes", format_duration(info.uptime_ms), info.processes);
    0
}

pub fn free(args: &[String], io: Io) -> i32 {
    let info = sys::sysinfo();
    let h = args.iter().any(|a| a == "-h") || true;
    let f = |v: u64| if h { human(v) } else { format!("{}", v / 1024) };
    outln!(io, "\x1b[1m{:<8}{:>10}{:>10}{:>10}\x1b[0m", "", "total", "used", "free");
    outln!(io, "{:<8}{:>10}{:>10}{:>10}", "Mem:", f(info.mem_total), f(info.mem_used()), f(info.mem_free));
    outln!(io, "{:<8}{:>10}{:>10}{:>10}", "Kernel:", f(info.heap_total), f(info.heap_total - info.heap_free), f(info.heap_free));
    0
}

pub fn ps(_: &[String], io: Io) -> i32 {
    outln!(io, "\x1b[1m{:>5} {:>5} {:<8} {:<2} {:<4} {:<6} {:>7} {:>8}  {}\x1b[0m", "PID", "PPID", "USER", "S", "TTY", "ABI", "HEAP", "CPU", "COMMAND");
    for p in sys::proc_list() {
        outln!(
            io,
            "{:>5} {:>5} {:<8} {:<2} {:<4} {:<6} {:>7} {:>7}s  {}",
            p.pid,
            p.parent,
            users::name_of(p.uid),
            p.state,
            if p.tty == 0 { String::from("?") } else { alloc::format!("tty{}", p.tty) },
            p.abi,
            human(p.heap_kb * 1024),
            p.cpu_ms / 1000,
            p.name
        );
    }
    0
}

pub fn kill(args: &[String], io: Io) -> i32 {
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

pub fn lsusb(_: &[String], io: Io) -> i32 {
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

pub fn drivers(_: &[String], io: Io) -> i32 {
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
    let text = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"]
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

pub fn gpuinfo(_: &[String], io: Io) -> i32 {
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

pub fn modinfo(args: &[String], io: Io) -> i32 {
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

pub fn interrupts(_: &[String], io: Io) -> i32 {
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

pub fn modules(_: &[String], io: Io) -> i32 {
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

pub fn version(_: &[String], io: Io) -> i32 {
    outln!(io, "HamixOS 0.6.1 -- Rust kernel, hext filesystem, Nook desktop, bash on hxlibc");
    0
}

pub fn fetch(_: &[String], io: Io) -> i32 {
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
        format!("\x1b[96mOS\x1b[0m      HamixOS 0.6.1 {}", hxstd::sys::ARCH),
        format!("\x1b[96mUptime\x1b[0m  {}", format_duration(info.uptime_ms)),
        format!("\x1b[96mShell\x1b[0m   {}", shell_name()),
        format!("\x1b[96mCPU\x1b[0m     {}", cpu),
        format!("\x1b[96mGPU\x1b[0m     {}", gpu),
        format!("\x1b[96mMemory\x1b[0m  {} / {}  root: {}", human(info.mem_used()), human(info.mem_total), root),
    ];
    for (i, line) in lines.iter().enumerate() {
        outln!(io, "{}   {}", logo.get(i).copied().unwrap_or("        "), line);
    }
    0
}

fn shell_name() -> String {
    let shell = hxstd::env::var("SHELL").unwrap_or(crate::SHELL);
    String::from(shell.rsplit('/').next().unwrap_or(shell))
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

pub fn cmd(args: &[String], io: Io) -> i32 {
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
            let program = crate::resolve_program(&args[3]).unwrap_or_else(|| args[3].clone());
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

pub const SESSION_HOOKS: &str = "/etc/hamix/session.d";

fn run_session_hooks() {
    let Some(entries) = sys::read_dir(SESSION_HOOKS) else {
        return;
    };
    let mut hooks: Vec<String> = entries.into_iter().filter(|e| !e.is_dir && e.name.ends_with(".start")).map(|e| format!("{}/{}", SESSION_HOOKS, e.name)).collect();
    hooks.sort();
    for hook in hooks {
        sys::spawn::<&str>(&hook, &[], sys::SPAWN_DETACH);
    }
}

const WAYLAND_SESSIONS: &str = "/usr/share/wayland-sessions";

const PERM_EXEC: &str = "/usr/libexec/hamix/hxperm-exec";
const NOOK_ID: &str = "org.hamix.Nook";

fn session_program(name: &str) -> Option<String> {
    if name.starts_with('/') {
        return Some(String::from(name));
    }
    if name == "nook" || name == "hxserver" {
        return Some(String::from("/usr/bin/hxserver"));
    }
    let text = fs::read_to_string(&format!("{}/{}.desktop", WAYLAND_SESSIONS, name))?;
    let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
    exec.split_whitespace().find(|word| !word.contains('=') && *word != "env").map(String::from)
}

fn session_names() -> Vec<String> {
    let mut names = alloc::vec![String::from("nook")];
    if let Some(entries) = sys::read_dir(WAYLAND_SESSIONS) {
        for e in entries {
            if let Some(n) = e.name.strip_suffix(".desktop").filter(|n| !names.iter().any(|known| known == n)) {
                names.push(String::from(n));
            }
        }
    }
    names
}

pub fn startx(args: &[String], io: Io) -> i32 {
    if sys::service_lookup("hxserver") > 0 {
        errln!(io, "startx: the desktop is already running");
        return 1;
    }
    let configured = fs::read_to_string("/etc/hamix/login.conf")
        .and_then(|t| crate::parse_conf(&t).into_iter().find(|(k, _)| k == "desktop").map(|(_, v)| v))
        .unwrap_or_else(|| String::from("/usr/bin/hxserver"));
    let wanted = args.get(1).cloned().unwrap_or(configured);
    let Some(desktop) = session_program(&wanted) else {
        errln!(io, "startx: no session called {} (available: {})", wanted, session_names().join(", "));
        return 1;
    };
    run_session_hooks();
    let status = if desktop == "/usr/bin/hxserver" && fs::exists(PERM_EXEC) { run_foreground(PERM_EXEC, &[NOOK_ID, &desktop], io) } else { run_foreground(&desktop, &[], io) };
    write(io.out, "\x1b[0m\x1b[2J\x1b[H");
    status
}

pub fn edit(args: &[String], io: Io) -> i32 {
    let Some(file) = args.get(1) else {
        errln!(io, "usage: edit file");
        return 1;
    };
    let file = crate::absolute(file);
    run_foreground("/usr/bin/hed", &[file.as_str()], io)
}

pub fn session(args: &[String], io: Io) -> i32 {
    const CONF: &str = "/etc/hamix/login.conf";
    match (args.get(1).map(|s| s.as_str()), args.get(2).map(|s| s.as_str())) {
        (None, _) => {
            let text = fs::read_to_string(CONF).unwrap_or_default();
            for (k, v) in crate::parse_conf(&text) {
                print_kv(io, &k, &v);
            }
            0
        }
        (Some("autostart"), Some(value @ ("on" | "off"))) => {
            let v = if value == "on" { "yes" } else { "no" };
            if crate::set_conf_value(CONF, "autostart_desktop", v) {
                outln!(io, "Nook will {}start automatically after login", if v == "yes" { "" } else { "not " });
                0
            } else {
                errln!(io, "session: cannot write {} (try sudo)", CONF);
                1
            }
        }
        (Some("list"), None) => {
            for name in session_names() {
                outln!(io, "{}", name);
            }
            0
        }
        (Some("desktop"), Some(name)) => {
            let Some(program) = session_program(name) else {
                errln!(io, "session: no session called {} (available: {})", name, session_names().join(", "));
                return 1;
            };
            if crate::set_conf_value(CONF, "desktop", &program) {
                outln!(io, "startx and autostart will run {}", program);
                0
            } else {
                errln!(io, "session: cannot write {} (try sudo)", CONF);
                1
            }
        }
        _ => {
            errln!(io, "usage: session [autostart on|off | desktop NAME | list]");
            1
        }
    }
}

pub fn setconf(args: &[String], io: Io) -> i32 {
    if args.len() < 4 {
        errln!(io, "usage: setconf file key value");
        return 1;
    }
    if crate::set_conf_value(&args[1], &args[2], &args[3..].join(" ")) {
        0
    } else {
        errln!(io, "setconf: cannot write {}", args[1]);
        1
    }
}

pub fn getconf(args: &[String], io: Io) -> i32 {
    if args.len() < 2 {
        errln!(io, "usage: hxconf file [key]");
        return 1;
    }
    let Some(text) = fs::read_to_string(&args[1]) else {
        errln!(io, "hxconf: {}: cannot read", args[1]);
        return 1;
    };
    let entries = crate::parse_conf(&text);
    match args.get(2) {
        Some(key) => match entries.iter().rev().find(|(k, _)| k == key) {
            Some((_, v)) => {
                outln!(io, "{}", v);
                0
            }
            None => 1,
        },
        None => {
            for (k, v) in entries {
                outln!(io, "{}={}", k, v);
            }
            0
        }
    }
}

const LINUX_ROOT: &str = "/linux";
const LINUX_PATH: [&str; 5] = ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];

pub fn linux(args: &[String], io: Io) -> i32 {
    if !fs::exists(&format!("{}/bin", LINUX_ROOT)) {
        errln!(io, "linux: the Linux environment is not installed ({} is missing)", LINUX_ROOT);
        return 1;
    }
    if args.get(1).map(|a| a == "-h" || a == "--help").unwrap_or(false) {
        outln!(io, "usage: linux [COMMAND [ARGS...]]");
        outln!(io, "Runs COMMAND inside the Linux environment: / is {}, while /home, /root, /tmp, /run,", LINUX_ROOT);
        outln!(io, "/dev, /proc, /sys, /mnt and /media are shared and the HamixOS tree is visible as /hamix.");
        outln!(io, "Without a command it starts the Linux shell (busybox ash).");
        return 0;
    }
    let (name, rest): (String, Vec<String>) = match args.get(1) {
        Some(command) => (command.clone(), args[2..].to_vec()),
        None => (String::from("/bin/sh"), alloc::vec![String::from("-l")]),
    };
    let program = if name.starts_with('/') {
        format!("{}{}", LINUX_ROOT, name)
    } else {
        match LINUX_PATH.iter().map(|d| format!("{}{}/{}", LINUX_ROOT, d, name)).find(|p| fs::exists(p)) {
            Some(p) => p,
            None => {
                errln!(io, "linux: {}: not found in the Linux environment (install it with: sudo pantry install {})", name, name);
                return 127;
            }
        }
    };
    let mut argv = alloc::vec![name.clone()];
    argv.extend(rest);
    let mut env: Vec<String> = crate::environment().into_iter().filter(|kv| !kv.starts_with("PS1=")).collect();
    env.push(String::from("PS1=[linux] \\u@\\h:\\w\\$ "));
    let r = sys::exec(&program, &argv, &env);
    errln!(io, "linux: cannot start {}: {}", program, sys::error_name(r));
    126
}

const LOCALES: &[&str] = &["C", "C.UTF-8", "POSIX", "en_US.UTF-8", "en_GB.UTF-8", "uk_UA.UTF-8", "de_DE.UTF-8", "fr_FR.UTF-8", "es_ES.UTF-8", "pl_PL.UTF-8"];
const CATEGORIES: &[&str] = &["LC_CTYPE", "LC_NUMERIC", "LC_TIME", "LC_COLLATE", "LC_MONETARY", "LC_MESSAGES", "LC_PAPER", "LC_NAME", "LC_ADDRESS", "LC_TELEPHONE", "LC_MEASUREMENT", "LC_IDENTIFICATION"];

pub fn locale(args: &[String], io: Io) -> i32 {
    match args.get(1).map(|s| s.as_str()) {
        Some("-a") | Some("--all-locales") => {
            for name in LOCALES {
                outln!(io, "{}", name);
            }
            0
        }
        Some("-m") | Some("--charmaps") => {
            outln!(io, "UTF-8");
            outln!(io, "ANSI_X3.4-1968");
            0
        }
        Some("-h") | Some("--help") => {
            outln!(io, "usage: locale [-a | -m]");
            0
        }
        Some(other) if other.starts_with('-') => {
            errln!(io, "locale: unknown option {}", other);
            1
        }
        Some(_) => {
            outln!(io, "UTF-8");
            0
        }
        None => {
            let lang = hxstd::env::var("LANG").unwrap_or("");
            let all = hxstd::env::var("LC_ALL").unwrap_or("");
            outln!(io, "LANG={}", lang);
            for category in CATEGORIES {
                let value = if !all.is_empty() { all } else { hxstd::env::var(category).unwrap_or(if lang.is_empty() { "POSIX" } else { lang }) };
                outln!(io, "{}=\"{}\"", category, value);
            }
            outln!(io, "LC_ALL={}", all);
            0
        }
    }
}
