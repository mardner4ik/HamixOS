use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{println, sys};

use crate::db::{self, Db};

pub const SCRIPTS_DIR: &str = "/var/lib/pantry/scripts";
const SHELL: &str = "/opt/linux/bin/busybox";
const ENV: [&str; 5] = ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root", "LANG=C.UTF-8", "TERM=dumb", "SHELL=/bin/sh"];

fn script_path(pkg: &str, script: &str) -> String {
    format!("{}/{}/{}", SCRIPTS_DIR, pkg, script.trim_start_matches('.'))
}

pub fn forget(pkg: &str) {
    let dir = format!("{}/{}", SCRIPTS_DIR, pkg);
    if let Some(entries) = sys::read_dir(&dir) {
        for entry in entries {
            sys::unlink(&format!("{}/{}", dir, entry.name));
        }
    }
    sys::unlink(&dir);
}

pub fn store(pkg: &str, scripts: &[(String, Vec<u8>)]) {
    forget(pkg);
    for (name, body) in scripts {
        let path = script_path(pkg, name);
        if db::write_file(&path, body) {
            sys::chmod(&path, 0o755);
        }
    }
}

pub fn has(pkg: &str, script: &str) -> bool {
    sys::stat(&script_path(pkg, script)).is_ok()
}

pub fn run(pkg: &str, script: &str, args: &[&str]) -> bool {
    let path = script_path(pkg, script);
    if sys::stat(&path).is_err() {
        return true;
    }
    if sys::stat(SHELL).is_err() {
        println!("  cannot run {} of {}: no shell in the Linux sysroot", script, pkg);
        return false;
    }
    let mut argv: Vec<&str> = alloc::vec!["sh", path.as_str()];
    argv.extend_from_slice(args);
    let pid = sys::spawn_io_env(SHELL, &argv, Some(&ENV[..]), 0, None, None, None);
    if pid < 0 {
        println!("  cannot start {} of {}", script, pkg);
        return false;
    }
    let status = sys::waitpid(pid, false);
    if status != 0 {
        println!("  {} of {} exited with status {}", script, pkg, status);
        return false;
    }
    true
}

fn segment_matches(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => segment_matches(&pattern[1..], text) || (!text.is_empty() && segment_matches(pattern, &text[1..])),
        (Some(b'?'), Some(_)) => segment_matches(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => segment_matches(&pattern[1..], &text[1..]),
        _ => false,
    }
}

pub fn glob_matches(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    p.len() == t.len() && p.iter().zip(t.iter()).all(|(a, b)| segment_matches(a.as_bytes(), b.as_bytes()))
}

pub fn changed_dir(file: &str) -> String {
    let full = format!("/{}", file.trim_start_matches('/'));
    String::from(db::parent(&full))
}

pub fn run_triggers(db: &Db, changed: &BTreeSet<String>) {
    if changed.is_empty() {
        return;
    }
    for pkg in db.packages.iter().filter(|p| !p.triggers.is_empty() && has(&p.name, ".trigger")) {
        let dirs: Vec<&str> = changed.iter().filter(|dir| pkg.triggers.iter().any(|t| glob_matches(t, dir))).map(|d| d.as_str()).collect();
        if dirs.is_empty() {
            continue;
        }
        println!("Running the {} trigger", pkg.name);
        run(&pkg.name, ".trigger", &dirs);
    }
}
