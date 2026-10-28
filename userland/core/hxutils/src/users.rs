use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{sys, users};

use crate::read_line_fd;
use crate::parse_opts;
use crate::{write, Io};
use crate::{errln, outln};

fn prompt_password(io: Io, prompt: &str) -> Option<String> {
    if sys::isatty(io.input) {
        write(io.err, prompt);
    }
    read_line_fd(io.input, true)
}

fn root_environment(user: &str, login: bool) -> Vec<String> {
    let mut env: Vec<String> = crate::environment()
        .into_iter()
        .filter(|kv| !["HOME=", "USER=", "LOGNAME=", "MAIL=", "SUDO_USER=", "SUDO_UID="].iter().any(|k| kv.starts_with(k)))
        .filter(|kv| !login || kv.starts_with("TERM=") || kv.starts_with("DISPLAY=") || kv.starts_with("WAYLAND_DISPLAY=") || kv.starts_with("LANG="))
        .collect();
    if login || !env.iter().any(|kv| kv.starts_with("PATH=")) {
        env.retain(|kv| !kv.starts_with("PATH="));
        env.push(format!("PATH={}", crate::DEFAULT_PATH));
    }
    env.push(String::from("HOME=/root"));
    env.push(String::from("USER=root"));
    env.push(String::from("LOGNAME=root"));
    env.push(format!("SHELL={}", crate::SHELL));
    env.push(format!("SUDO_USER={}", user));
    env.push(format!("SUDO_UID={}", sys::getuid()));
    env
}

fn spawn_as_root(program: &str, args: &[String], env: &[String], io: Io) -> Result<i32, i64> {
    if sys::geteuid() != 0 {
        return Err(-1);
    }
    let pid = crate::spawn_env(program, args, env, sys::SPAWN_ROOT | sys::SPAWN_FOREGROUND, io);
    if pid < 0 {
        return Err(pid);
    }
    Ok(crate::wait(pid))
}

fn authenticate(io: Io, user: &str) -> Result<(), i32> {
    if sys::getuid() == 0 || sys::auth_request(&["renew"]) == 0 {
        return Ok(());
    }
    if !users::is_sudoer(user) && hxstd::fs::read_to_string("/etc/sudoers").is_some() {
        errln!(io, "{} is not allowed to use sudo", user);
        return Err(1);
    }
    for attempt in 0..3 {
        let Some(password) = prompt_password(io, &format!("[sudo] password for {}: ", user)) else {
            return Err(1);
        };
        match sys::auth(user, &password) {
            0 => return Ok(()),
            1 => {
                errln!(io, "{} is not in the sudoers file", user);
                return Err(1);
            }
            -2 => {
                errln!(io, "sudo: the authentication service (hx-authd) is not running");
                return Err(1);
            }
            _ => {
                if attempt < 2 {
                    errln!(io, "Sorry, try again.");
                }
            }
        }
    }
    errln!(io, "sudo: 3 incorrect password attempts");
    Err(1)
}

const SUDO_USAGE: &str = "usage: sudo [-u root] [-i | -s] [-k | -v] [--] command [args]";

pub fn sudo(args: &[String], io: Io) -> i32 {
    let mut rest = &args[1..];
    let (mut login, mut shell, mut validate, mut forget) = (false, false, false, false);
    while let Some(arg) = rest.first() {
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        if arg == "--" {
            rest = &rest[1..];
            break;
        }
        match arg.as_str() {
            "-u" | "--user" => {
                if rest.get(1).map(|u| u != "root" && u != "0").unwrap_or(true) {
                    errln!(io, "sudo: only -u root is supported");
                    return 1;
                }
                rest = &rest[1..];
            }
            "-i" | "--login" => login = true,
            "-s" | "--shell" => shell = true,
            "-v" | "--validate" => validate = true,
            "-k" | "-K" | "--reset-timestamp" | "--remove-timestamp" => forget = true,
            "-E" | "--preserve-env" | "-H" | "--set-home" | "-S" | "--stdin" | "-n" | "--non-interactive" => {}
            "-h" | "--help" => {
                outln!(io, "{}", SUDO_USAGE);
                return 0;
            }
            other => {
                errln!(io, "sudo: unknown option {}\n{}", other, SUDO_USAGE);
                return 1;
            }
        }
        rest = &rest[1..];
    }
    if forget {
        sys::auth_request(&["forget"]);
        if rest.is_empty() && !validate && !login && !shell {
            return 0;
        }
    }
    if rest.is_empty() && !validate && !login && !shell {
        errln!(io, "{}", SUDO_USAGE);
        return 1;
    }
    let user = users::name_of(sys::getuid() as u32);
    if let Err(code) = authenticate(io, &user) {
        return code;
    }
    if validate && rest.is_empty() {
        return 0;
    }
    let env = root_environment(&user, login);
    let (program, argv): (String, Vec<String>) = if login || shell {
        let mut argv = Vec::new();
        if login {
            argv.push(String::from("-l"));
        }
        if !rest.is_empty() {
            argv.push(String::from("-c"));
            argv.push(rest.iter().map(|a| crate::quote(a)).collect::<Vec<_>>().join(" "));
        }
        (String::from(crate::SHELL), argv)
    } else {
        match crate::command_line(rest) {
            Some(found) => found,
            None => {
                errln!(io, "sudo: {}: command not found", rest[0]);
                return 127;
            }
        }
    };
    if login {
        sys::chdir("/root");
    }
    match spawn_as_root(&program, &argv, &env, io) {
        Ok(code) => code,
        Err(e) => {
            errln!(io, "sudo: {}: {}", rest.first().map(|s| s.as_str()).unwrap_or(program.as_str()), sys::error_name(e));
            1
        }
    }
}

pub fn su(args: &[String], io: Io) -> i32 {
    let command = args.iter().position(|a| a == "-c").and_then(|i| args.get(i + 1)).cloned();
    let login = args[1..].iter().any(|a| a == "-" || a == "-l" || a == "--login");
    if let Some(name) = args[1..].iter().find(|a| !a.starts_with('-') && Some(*a) != command.as_ref()) {
        if name != "root" {
            errln!(io, "su: only switching to root is supported; log in as {} on another console (Ctrl+Alt+F2)", name);
            return 1;
        }
    }
    let mut argv: Vec<String> = Vec::new();
    if login {
        argv.push(String::from("-l"));
    }
    if let Some(c) = &command {
        argv.push(String::from("-c"));
        argv.push(c.clone());
    }
    if sys::getuid() != 0 {
        let Some(password) = prompt_password(io, "Password: ") else {
            return 1;
        };
        if sys::auth("root", &password) != 0 {
            errln!(io, "su: authentication failure");
            return 1;
        }
    }
    let user = users::name_of(sys::getuid() as u32);
    let env = root_environment(&user, login);
    if login {
        sys::chdir("/root");
    }
    match spawn_as_root(crate::SHELL, &argv, &env, io) {
        Ok(code) => code,
        Err(e) => {
            errln!(io, "su: {}", sys::error_name(e));
            1
        }
    }
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

pub fn passwd(args: &[String], io: Io) -> i32 {
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

fn root_of(opts: &crate::Opts) -> String {
    opts.value("root").map(|r| String::from(r.trim_end_matches('/'))).unwrap_or_default()
}

pub fn useradd(args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root", "s", "G", "p", "shell", "groups", "password"]);
    let Some(name) = opts.rest.first() else {
        errln!(io, "usage: useradd [--root dir] [-s shell] [-G sudo] [-p password] name");
        return 1;
    };
    let root = root_of(&opts);
    let shell = opts.value("s").or_else(|| opts.value("shell")).unwrap_or_else(|| String::from(crate::SHELL));
    let groups = opts.value("G").or_else(|| opts.value("groups")).unwrap_or_default();
    let sudo = groups.split(',').any(|g| g == "sudo" || g == "wheel");
    match users::add_user(&root, name, &shell, sudo) {
        Ok(user) => {
            for g in groups.split(',').filter(|g| !g.is_empty() && *g != "sudo" && *g != "wheel") {
                if let Err(e) = users::set_member(&root, g, name, true) {
                    errln!(io, "useradd: {}", e);
                }
            }
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

pub fn userdel(args: &[String], io: Io) -> i32 {
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
            crate::remove_tree(&format!("{}{}", root, home));
        }
    }
    0
}

pub fn usermod(args: &[String], io: Io) -> i32 {
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
        let wanted: Vec<&str> = groups.split(',').filter(|g| !g.is_empty()).collect();
        let sudo = wanted.iter().any(|g| *g == "sudo" || *g == "wheel");
        if sudo || !opts.has('a') {
            if let Err(e) = users::set_sudo(&root, name, sudo) {
                errln!(io, "usermod: {}", e);
                return 1;
            }
        }
        if !opts.has('a') {
            for g in users::member_of(&root, name) {
                if !wanted.contains(&g.as_str()) && !(sudo && users::ADMIN_GROUPS.contains(&g.as_str())) {
                    let _ = users::set_member(&root, &g, name, false);
                }
            }
        }
        for g in wanted.iter().filter(|g| **g != "sudo" && **g != "wheel") {
            if let Err(e) = users::set_member(&root, g, name, true) {
                errln!(io, "usermod: {}", e);
                return 1;
            }
        }
    }
    0
}

pub fn chpasswd(args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["root"]);
    let root = root_of(&opts);
    let pairs: Vec<(String, String)> = if opts.rest.len() >= 2 {
        alloc::vec![(opts.rest[0].clone(), opts.rest[1..].join(" "))]
    } else {
        let data = crate::read_all(io.input);
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

pub fn list_users(_: &[String], io: Io) -> i32 {
    outln!(io, "\x1b[1m{:<14} {:>6}  {:<18} {:<16} {}\x1b[0m", "USER", "UID", "HOME", "SHELL", "ADMIN");
    for u in users::users("") {
        outln!(io, "{:<14} {:>6}  {:<18} {:<16} {}", u.name, u.uid, u.home, u.shell, if users::is_sudoer(&u.name) { "yes" } else { "" });
    }
    0
}
