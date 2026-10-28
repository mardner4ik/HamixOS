#![no_std]
#![no_main]

extern crate alloc;

mod config;
mod probe;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use config::{Config, Entry};
use hxstd::{entry, env, eprintln, fs, println};

const DEFAULT_FILE: &str = "/boot/hxboot.cfg";

fn usage() -> i32 {
    println!("usage: hxbootctl [-f FILE] COMMAND      (FILE: {} by default)", DEFAULT_FILE);
    println!();
    println!("  list                         the boot menu (default marked with *)");
    println!("  show N                       every line of entry N");
    println!("  default N|NAME               boot entry N when nobody presses a key");
    println!("  timeout SECONDS              how long the menu waits (0: only with Shift held)");
    println!("  add NAME KERNEL [CMDLINE..]  a new kernel entry");
    println!("  add-chain NAME DISK PART     boot another system: DISK is sda.. or a signature, PART 0 = whole disk");
    println!("  add-reboot [NAME]            a menu entry that restarts the computer");
    println!("  module N PATH NAME [--optional]   load PATH with entry N as boot module NAME");
    println!("  cmdline N [ARGS..]           replace the kernel command line of entry N");
    println!("  rename N NAME                change the title of entry N");
    println!("  remove N                     delete entry N");
    println!("  move N TO                    put entry N at position TO");
    println!("  probe [--add]                other operating systems on the disks (--add puts them into the menu)");
    println!("  auto on|off                  let hxboot look for other systems itself at every boot");
    println!("  check                        verify that kernels, modules and disks of all entries exist");
    println!("  edit                         open the file in $EDITOR (hed by default)");
    println!();
    println!("The file is plain text and may be edited by hand; see /usr/share/doc/hamix/hxboot.md.");
    2
}

fn load(file: &str) -> Result<Config, String> {
    match fs::read_to_string(file) {
        Some(text) => Ok(Config::parse(&text)),
        None => Err(format!("{}: cannot read it (this system may boot with GRUB or from read-only media; use -f FILE)", file)),
    }
}

fn save(file: &str, config: &Config) -> Result<(), String> {
    if let Some(old) = fs::read(file) {
        let _ = fs::write(&format!("{}.bak", file), &old);
    }
    if fs::write(file, config.text().as_bytes()) {
        Ok(())
    } else {
        Err(format!("{}: cannot write it (run as root: sudo hxbootctl ...)", file))
    }
}

fn list(config: &Config, file: &str) {
    let timeout = config.get("timeout").unwrap_or("3");
    let probe = matches!(config.get("probe"), Some("yes" | "on" | "true" | "1"));
    println!("{}: menu waits {} s, other systems found at boot: {}", file, timeout, if probe { "yes" } else { "no" });
    let default = config.default_index();
    for (i, e) in config.entries.iter().enumerate() {
        println!("{} {:>2}  {:<44} {}", if i == default { "*" } else { " " }, i + 1, e.name(), e.describe());
    }
    if config.entries.is_empty() {
        println!("  (no entries)");
    }
}

fn show(entry: &Entry, index: usize) {
    println!("entry {}:", index + 1);
    for line in &entry.lines {
        println!("  {}", line);
    }
}

fn root_of(file: &str) -> String {
    let normal = fs::normalize(file);
    let parent = match normal.rfind('/') {
        Some(i) => &normal[..i],
        None => "",
    };
    match parent.strip_suffix("/boot") {
        Some(root) => String::from(root),
        None => String::from(parent),
    }
}

fn check(config: &Config, file: &str) -> i32 {
    let root = root_of(file);
    let disks = probe::scan().ok();
    let mut problems = 0;
    for (i, e) in config.entries.iter().enumerate() {
        let mut notes: Vec<String> = Vec::new();
        match e.kind() {
            "kernel" => {
                let kernel = e.value("kernel").and_then(|v| v.split_whitespace().next()).unwrap_or("");
                if kernel.is_empty() {
                    notes.push(String::from("no kernel line"));
                } else if !fs::exists(&format!("{}{}", root, kernel)) {
                    notes.push(format!("{} is missing", kernel));
                }
                for (module, optional) in e.modules() {
                    let path = module.split_whitespace().next().unwrap_or("");
                    if !optional && !fs::exists(&format!("{}{}", root, path)) {
                        notes.push(format!("module {} is missing", path));
                    }
                }
            }
            "chain" => {
                let line = e.value("chain").unwrap_or("");
                let signature = line.split_whitespace().find_map(|w| w.strip_prefix("signature=")).and_then(|s| u32::from_str_radix(s, 16).ok());
                match (signature, &disks) {
                    (Some(sig), Some(found)) if !found.iter().any(|f| f.signature == sig) => notes.push(format!("no connected disk has the signature {:08x}", sig)),
                    (None, _) if !line.contains("drive=") => notes.push(String::from("chain needs signature= or drive=")),
                    _ => {}
                }
            }
            _ => {}
        }
        if notes.is_empty() {
            println!("  ok   {:>2}  {}", i + 1, e.name());
        } else {
            problems += 1;
            println!("  FAIL {:>2}  {}: {}", i + 1, e.name(), notes.join(", "));
        }
    }
    if config.entries.is_empty() {
        println!("  FAIL the menu has no entries");
        problems += 1;
    }
    if problems == 0 { 0 } else { 1 }
}

fn disk_signature(disk: &str) -> Result<u32, String> {
    if let Ok(sig) = u32::from_str_radix(disk.trim_start_matches("0x"), 16) {
        if disk.len() >= 8 {
            return Ok(sig);
        }
    }
    let mut mbr = [0u8; 512];
    if hxstd::sys::disk_read(disk.trim_start_matches("/dev/"), 0, &mut mbr) != 0 {
        return Err(format!("{}: cannot read the disk (run as root)", disk));
    }
    let sig = u32::from_le_bytes([mbr[440], mbr[441], mbr[442], mbr[443]]);
    if sig == 0 {
        return Err(format!("{} has no MBR disk signature; use drive=0x8N in the file by hand", disk));
    }
    Ok(sig)
}

fn run(file: &str, cmd: &str, args: &[String]) -> Result<i32, String> {
    let need = |n: usize| if args.len() < n { Err(String::from("missing arguments (hxbootctl help)")) } else { Ok(()) };
    match cmd {
        "list" => {
            list(&load(file)?, file);
            Ok(0)
        }
        "show" => {
            need(1)?;
            let config = load(file)?;
            let i = config.find(&args[0])?;
            show(&config.entries[i], i);
            Ok(0)
        }
        "check" => Ok(check(&load(file)?, file)),
        "probe" => {
            let found = probe::scan()?;
            if found.is_empty() {
                println!("no other operating system with a BIOS boot sector was found");
                return Ok(0);
            }
            let add = args.iter().any(|a| a == "--add");
            let mut config = if add { Some(load(file)?) } else { None };
            for f in &found {
                println!("  {:<36} chain {}", f.title(), f.chain());
                if let Some(config) = config.as_mut() {
                    let chain = f.chain();
                    if config.entries.iter().any(|e| e.value("chain") == Some(chain.as_str())) {
                        println!("      already in the menu");
                        continue;
                    }
                    let mut entry = Entry::new(&f.title());
                    entry.set("chain", &chain);
                    let at = config.insert(entry);
                    println!("      added as entry {}", at + 1);
                }
            }
            if let Some(config) = config {
                save(file, &config)?;
            }
            Ok(0)
        }
        "edit" => {
            let editor = hxstd::env::var("EDITOR").filter(|e| !e.is_empty()).unwrap_or("hed");
            let pid = hxstd::sys::spawn(editor, &[file], hxstd::sys::SPAWN_FOREGROUND);
            if pid < 0 {
                return Err(format!("cannot start {}", editor));
            }
            let status = hxstd::sys::waitpid(pid, false);
            if status != 0 {
                return Err(format!("{} exited with {}", editor, status));
            }
            let config = load(file)?;
            Ok(check(&config, file))
        }
        _ => {
            let mut config = load(file)?;
            match cmd {
                "default" => {
                    need(1)?;
                    let i = config.find(&args[0])?;
                    config.set("default", &format!("{}", i));
                }
                "timeout" => {
                    need(1)?;
                    let seconds: u32 = args[0].parse().map_err(|_| String::from("timeout needs a number of seconds"))?;
                    config.set("timeout", &format!("{}", seconds));
                }
                "auto" => {
                    need(1)?;
                    let on = match args[0].as_str() {
                        "on" | "yes" => true,
                        "off" | "no" => false,
                        _ => return Err(String::from("auto on|off")),
                    };
                    config.set("probe", if on { "yes" } else { "no" });
                }
                "add" => {
                    need(2)?;
                    let mut entry = Entry::new(&args[0]);
                    let mut line = args[1].clone();
                    for a in &args[2..] {
                        line.push(' ');
                        line.push_str(a);
                    }
                    entry.set("kernel", &line);
                    let at = config.insert(entry);
                    println!("added as entry {}", at + 1);
                }
                "add-chain" => {
                    need(3)?;
                    let sig = disk_signature(&args[1])?;
                    let part: u8 = args[2].parse().ok().filter(|&p: &u8| p <= 4).ok_or_else(|| String::from("PART is 0 (whole disk) or 1..4"))?;
                    let mut entry = Entry::new(&args[0]);
                    entry.set("chain", &format!("signature={:08x} partition={}", sig, part));
                    let at = config.insert(entry);
                    println!("added as entry {}", at + 1);
                }
                "add-reboot" => {
                    let name = args.first().map(|s| s.as_str()).unwrap_or("Reboot");
                    let mut entry = Entry::new(name);
                    entry.set("reboot", "");
                    config.entries.push(entry);
                }
                "module" => {
                    need(3)?;
                    let i = config.find(&args[0])?;
                    let optional = args.iter().any(|a| a == "--optional");
                    config.entries[i].add_module(&args[1], &args[2], optional);
                }
                "cmdline" => {
                    need(1)?;
                    let i = config.find(&args[0])?;
                    let kernel = config.entries[i].value("kernel").and_then(|v| v.split_whitespace().next()).map(String::from).ok_or_else(|| String::from("this entry has no kernel"))?;
                    let mut line = kernel;
                    for a in &args[1..] {
                        line.push(' ');
                        line.push_str(a);
                    }
                    config.entries[i].set("kernel", &line);
                }
                "rename" => {
                    need(2)?;
                    let i = config.find(&args[0])?;
                    config.entries[i].rename(&args[1..].join(" "));
                }
                "remove" => {
                    need(1)?;
                    let i = config.find(&args[0])?;
                    let default = config.default_index();
                    let gone = config.entries.remove(i);
                    if default >= config.entries.len() || default == i {
                        config.set("default", "0");
                    } else if default > i {
                        config.set("default", &format!("{}", default - 1));
                    }
                    println!("removed \"{}\"", gone.name());
                }
                "move" => {
                    need(2)?;
                    let from = config.find(&args[0])?;
                    let to: usize = args[1].parse().ok().filter(|&t: &usize| t >= 1 && t <= config.entries.len()).ok_or_else(|| String::from("TO is a position in the menu"))?;
                    let entry = config.entries.remove(from);
                    config.entries.insert(to - 1, entry);
                }
                _ => return Ok(usage()),
            }
            save(file, &config)?;
            list(&config, file);
            Ok(0)
        }
    }
}

fn main() -> i32 {
    let argv = env::args();
    let mut file = String::from(DEFAULT_FILE);
    let mut rest: Vec<String> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        if argv[i] == "-f" || argv[i] == "--file" {
            match argv.get(i + 1) {
                Some(f) => file = f.clone(),
                None => return usage(),
            }
            i += 2;
            continue;
        }
        rest.push(argv[i].clone());
        i += 1;
    }
    let cmd = rest.first().cloned().unwrap_or_else(|| String::from("list"));
    if matches!(cmd.as_str(), "help" | "-h" | "--help") {
        usage();
        return 0;
    }
    match run(&file, &cmd, &rest[1.min(rest.len())..]) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hxbootctl: {}", e);
            1
        }
    }
}

entry!(main);
