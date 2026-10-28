#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hxstd::{entry, env, fs, println, sys};

const INIT_D: &str = "/linux/etc/init.d";
const UNITS: &str = "/etc/hamix/services";
const PREFIX: &str = "openrc-";
const STATUS_EVERY_MS: u64 = 5000;

fn usage() -> i32 {
    println!("usage: hxsvc-openrc sync            create hxsvc units for the OpenRC scripts in {}", INIT_D);
    println!("       hxsvc-openrc run SCRIPT      start an OpenRC service and stay while it runs (used by the units)");
    println!("       hxsvc-openrc list            show the OpenRC scripts and their units");
    2
}

fn scripts() -> Vec<String> {
    let mut names: Vec<String> = sys::read_dir(INIT_D)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir && !e.name.starts_with('.') && e.name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
        .map(|e| e.name)
        .collect();
    names.sort();
    names
}

fn description(script: &str) -> String {
    let text = fs::read_prefix(&format!("{}/{}", INIT_D, script), 4096).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    text.lines()
        .find_map(|l| l.trim().strip_prefix("description=").map(|d| String::from(d.trim_matches('"'))))
        .unwrap_or_else(|| format!("OpenRC service {}", script))
}

fn sync() -> i32 {
    let mut created = 0;
    let mut removed = 0;
    let present = scripts();
    for name in present.iter() {
        let unit = format!("{}/{}{}.svc", UNITS, PREFIX, name);
        let text = format!("description = {}\nexec = /usr/libexec/hamix/hxsvc-openrc run {}\nstart = manual\nrestart = on-failure\nlayer = linux\n", description(name), name);
        if fs::read_to_string(&unit).as_deref() != Some(text.as_str()) {
            if fs::write(&unit, text.as_bytes()) {
                created += 1;
            }
        }
    }
    for entry in sys::read_dir(UNITS).unwrap_or_default() {
        let Some(name) = entry.name.strip_prefix(PREFIX).and_then(|n| n.strip_suffix(".svc")) else {
            continue;
        };
        if !present.iter().any(|p| p == name) {
            sys::unlink(&format!("{}/{}", UNITS, entry.name));
            removed += 1;
        }
    }
    println!("hxsvc-openrc: {} unit(s) written, {} removed", created, removed);
    if created + removed > 0 && sys::service_lookup("hamix.svc") > 0 {
        let pid = sys::spawn("/usr/bin/hxctl", &["reload"], 0);
        if pid > 0 {
            sys::waitpid(pid, false);
        }
    }
    0
}

fn script_action(script: &str, action: &str) -> i64 {
    let path = format!("{}/{}", INIT_D, script);
    let pid = sys::spawn(&path, &[action], 0);
    if pid < 0 {
        return pid;
    }
    sys::waitpid(pid, false)
}

fn run(script: &str) -> i32 {
    let code = script_action(script, "start");
    if code != 0 {
        println!("hxsvc-openrc: {} start failed ({})", script, code);
        return 1;
    }
    println!("hxsvc-openrc: {} started", script);
    loop {
        sys::sleep_ms(STATUS_EVERY_MS);
        let status = script_action(script, "status");
        if status != 0 {
            println!("hxsvc-openrc: {} is no longer running ({})", script, status);
            return 1;
        }
    }
}

fn list() -> i32 {
    for name in scripts() {
        let unit = format!("{}/{}{}.svc", UNITS, PREFIX, name);
        println!("{:<24} {}", name, if fs::exists(&unit) { unit } else { String::from("(no unit, run hxsvc-openrc sync)") });
    }
    0
}

fn main() -> i32 {
    let args = env::args();
    match (args.get(1).map(|s| s.as_str()), args.get(2)) {
        (Some("sync"), _) => sync(),
        (Some("run"), Some(script)) => run(script),
        (Some("list"), _) => list(),
        _ => usage(),
    }
}

entry!(main);
