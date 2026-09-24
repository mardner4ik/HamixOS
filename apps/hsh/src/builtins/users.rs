use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{sys, users};

use super::basic::read_line_fd;
use super::{parse_opts, Builtin};
use crate::shell::{write, Io, Shell};
use crate::{errln, outln};

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "sudo", usage: "sudo command [args]", help: "run a command as root", group: "users", run: sudo },
        Builtin { name: "su", usage: "su [-c command]", help: "become root (asks for the root password)", group: "users", run: su },
        Builtin { name: "passwd", usage: "passwd [user]", help: "change a password", group: "users", run: passwd },
        Builtin { name: "useradd", usage: "useradd [--root dir] [-s shell] [-G sudo] [-p password] name", help: "create a user (root)", group: "users", run: useradd },
        Builtin { name: "userdel", usage: "userdel [--root dir] [-r] name", help: "delete a user (root)", group: "users", run: userdel },
        Builtin { name: "usermod", usage: "usermod [-aG sudo] [-s shell] name", help: "change a user (root)", group: "users", run: usermod },
        Builtin { name: "chpasswd", usage: "chpasswd [--root dir] [user password]", help: "set passwords (or user:password lines from input)", group: "users", run: chpasswd },
        Builtin { name: "users", usage: "users", help: "list accounts", group: "users", run: list_users },
    ]
}

pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c)) {
        return String::from(arg);
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

fn prompt_password(io: Io, prompt: &str) -> Option<String> {
    write(io.err, prompt);
    read_line_fd(io.input, true)
}

fn spawn_root(sh: &mut Shell, args: &[String], io: Io) -> Result<i32, i64> {
    let name = &args[0];
    let builtin = super::is_builtin(name) || sh.functions.contains_key(name) || sh.aliases.contains_key(name);
    let (program, argv): (String, Vec<String>) = if builtin {
        let text = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        (String::from("/usr/bin/hsh"), alloc::vec![String::from("-c"), text])
    } else {
        match sh.resolve_program(name) {
            Some(path) => {
                let is_elf = hamix_std::fs::read_prefix(&path, 4).map(|h| h.starts_with(b"\x7fELF")).unwrap_or(false);
                if is_elf {
                    (path, args[1..].to_vec())
                } else {
                    let mut a = alloc::vec![path];
                    a.extend_from_slice(&args[1..]);
                    (String::from("/usr/bin/hsh"), a)
                }
            }
            None => {
                errln!(io, "sudo: {}: command not found", name);
                return Ok(127);
            }
        }
    };
    let argv: Vec<&str> = argv.iter().map(|a| a.as_str()).collect();
    let pid = sh.spawn_with_env(&program, &argv, sys::SPAWN_ROOT | sys::SPAWN_FOREGROUND, io);
    if pid < 0 {
        return Err(pid);
    }
    Ok(sh.wait_foreground(pid))
}

fn sudo(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut rest = &args[1..];
    while rest.first().map(|a| a.starts_with('-')).unwrap_or(false) {
        rest = &rest[1..];
    }
    if rest.is_empty() {
        errln!(io, "usage: sudo command [args]");
        return 1;
    }
    let rest = rest.to_vec();
    if sys::geteuid() == 0 {
        return crate::run_argv(sh, &rest, io);
    }
    match spawn_root(sh, &rest, io) {
        Ok(code) => return code,
        Err(-1) => {}
        Err(e) => {
            errln!(io, "sudo: {}", sys::error_name(e));
            return 1;
        }
    }
    let user = users::name_of(sys::getuid() as u32);
    if !users::is_sudoer(&user) && hamix_std::fs::read_to_string("/etc/sudoers").is_some() {
        errln!(io, "{} is not allowed to use sudo", user);
        return 1;
    }
    for attempt in 0..3 {
        let Some(password) = prompt_password(io, &format!("[sudo] password for {}: ", user)) else {
            return 1;
        };
        match sys::auth(&user, &password) {
            0 => {
                return match spawn_root(sh, &rest, io) {
                    Ok(code) => code,
                    Err(e) => {
                        errln!(io, "sudo: {}", sys::error_name(e));
                        1
                    }
                };
            }
            1 => {
                errln!(io, "{} is not in the sudoers file", user);
                return 1;
            }
            _ => {
                if attempt < 2 {
                    errln!(io, "Sorry, try again.");
                }
            }
        }
    }
    errln!(io, "sudo: 3 incorrect password attempts");
    1
}

fn su(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let command = args.iter().position(|a| a == "-c").and_then(|i| args.get(i + 1)).cloned();
    if let Some(name) = args[1..].iter().find(|a| !a.starts_with('-') && Some(*a) != command.as_ref()) {
        if name != "root" {
            errln!(io, "su: only switching to root is supported; log in as {} on another console (Ctrl+Alt+F2)", name);
            return 1;
        }
    }
    let argv: Vec<String> = match &command {
        Some(c) => alloc::vec![String::from("-c"), c.clone()],
        None => alloc::vec![String::from("-l")],
    };
    if sys::geteuid() != 0 {
        let Some(password) = prompt_password(io, "Password: ") else {
            return 1;
        };
        if sys::auth("root", &password) != 0 {
            errln!(io, "su: authentication failure");
            return 1;
        }
    }
    let pid = sys::spawn_io("/usr/bin/hsh", &argv, sys::SPAWN_ROOT | sys::SPAWN_FOREGROUND, Some(io.input), Some(io.out), Some(io.err));
    if pid < 0 {
        errln!(io, "su: {}", sys::error_name(pid));
        return 1;
    }
    sh.wait_foreground(pid)
}

fn read_new_password(io: Io) -> Option<String> {
    let first = prompt_password(io, "New password: ")?;
    let second = prompt_password(io, "Retype new password: ")?;
    if first != second {
        errln!(io, "passwd: passwords do not match");
        return None;
    }
    if first.is_empty() {
        errln!(io, "passwd: empty passwords are not allowed");
        return None;
    }
    Some(first)
}

fn passwd(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let me = users::name_of(sys::getuid() as u32);
    let target = args.get(1).cloned().unwrap_or_else(|| me.clone());
    if users::find("", &target).is_none() {
        errln!(io, "passwd: user '{}' does not exist", target);
        return 1;
    }
    if sys::geteuid() != 0 {
        if target != me {
            errln!(io, "passwd: you may only change your own password (try: sudo passwd {})", target);
            return 1;
        }
        let Some(current) = prompt_password(io, "Current password: ") else {
            return 1;
        };
        if sys::auth(&me, &current) < 0 {
            errln!(io, "passwd: authentication failure");
            return 1;
        }
        let Some(new) = read_new_password(io) else {
            return 1;
        };
        let r = sys::set_own_password(&current, &new);
        if r < 0 {
            errln!(io, "passwd: {}", sys::error_name(r));
            return 1;
        }
        outln!(io, "passwd: password updated");
        return 0;
    }
    let Some(new) = read_new_password(io) else {
        return 1;
    };
    match users::set_password("", &target, &new) {
        Ok(()) => {
            outln!(io, "passwd: password for {} updated", target);
            0
        }
        Err(e) => {
            errln!(io, "passwd: {}", e);
            1
        }
    }
}

fn root_of(opts: &super::Opts) -> String {
    opts.value("root").map(|r| String::from(r.trim_end_matches('/'))).unwrap_or_default()
}

fn useradd(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root", "s", "G", "p", "shell", "groups", "password"]);
    let Some(name) = opts.rest.first() else {
        errln!(io, "usage: useradd [--root dir] [-s shell] [-G sudo] [-p password] name");
        return 1;
    };
    let root = root_of(&opts);
    let shell = opts.value("s").or_else(|| opts.value("shell")).unwrap_or_else(|| String::from("/usr/bin/hsh"));
    let groups = opts.value("G").or_else(|| opts.value("groups")).unwrap_or_default();
    let sudo = groups.split(',').any(|g| g == "sudo" || g == "wheel");
    match users::add_user(&root, name, &shell, sudo) {
        Ok(user) => {
            if let Some(password) = opts.value("p").or_else(|| opts.value("password")) {
                if let Err(e) = users::set_password(&root, name, &password) {
                    errln!(io, "useradd: {}", e);
                    return 1;
                }
            }
            outln!(io, "useradd: created {} (uid {}, home {}{})", user.name, user.uid, root, user.home);
            0
        }
        Err(e) => {
            errln!(io, "useradd: {}", e);
            1
        }
    }
}

fn userdel(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root"]);
    let Some(name) = opts.rest.first() else {
        errln!(io, "usage: userdel [--root dir] [-r] name");
        return 1;
    };
    let root = root_of(&opts);
    let home = users::find(&root, name).map(|u| u.home);
    if let Err(e) = users::remove_user(&root, name) {
        errln!(io, "userdel: {}", e);
        return 1;
    }
    if opts.has('r') {
        if let Some(home) = home {
            super::files::remove_tree(&format!("{}{}", root, home));
        }
    }
    0
}

fn usermod(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root", "G", "s"]);
    let Some(name) = opts.rest.first() else {
        errln!(io, "usage: usermod [-aG sudo] [-s shell] name");
        return 1;
    };
    let root = root_of(&opts);
    let mut list = users::users(&root);
    let Some(user) = list.iter_mut().find(|u| &u.name == name) else {
        errln!(io, "usermod: no such user");
        return 1;
    };
    if let Some(shell) = opts.value("s") {
        user.shell = shell;
        if let Err(e) = users::write_passwd(&root, &list) {
            errln!(io, "usermod: {}", e);
            return 1;
        }
    }
    if let Some(groups) = opts.value("G") {
        let sudo = groups.split(',').any(|g| g == "sudo" || g == "wheel");
        if sudo || !opts.has('a') {
            if let Err(e) = users::set_sudo(&root, name, sudo) {
                errln!(io, "usermod: {}", e);
                return 1;
            }
        }
    }
    if root.is_empty() {
        sys::users_reload();
    }
    0
}

fn chpasswd(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root"]);
    let root = root_of(&opts);
    let pairs: Vec<(String, String)> = if opts.rest.len() >= 2 {
        alloc::vec![(opts.rest[0].clone(), opts.rest[1..].join(" "))]
    } else {
        let data = super::text::read_all(io.input);
        String::from_utf8_lossy(&data)
            .lines()
            .filter_map(|l| l.split_once(':').map(|(u, p)| (String::from(u), String::from(p))))
            .collect()
    };
    let mut status = 0;
    for (user, password) in pairs {
        if users::find(&root, &user).is_none() {
            errln!(io, "chpasswd: {}: no such user", user);
            status = 1;
            continue;
        }
        if let Err(e) = users::set_password(&root, &user, &password) {
            errln!(io, "chpasswd: {}", e);
            status = 1;
        }
    }
    status
}

fn list_users(_: &mut Shell, _: &[String], io: Io) -> i32 {
    outln!(io, "\x1b[1m{:<14} {:>6}  {:<18} {:<16} {}\x1b[0m", "USER", "UID", "HOME", "SHELL", "ADMIN");
    for u in users::users("") {
        outln!(io, "{:<14} {:>6}  {:<18} {:<16} {}", u.name, u.uid, u.home, u.shell, if users::is_sudoer(&u.name) { "yes" } else { "" });
    }
    0
}
