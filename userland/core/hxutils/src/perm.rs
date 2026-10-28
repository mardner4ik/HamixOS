use alloc::string::String;
use alloc::vec::Vec;
use hxstd::permissions::{self, PERMISSIONS};
use hxstd::{sys, users};

use crate::{errln, outln, Io};

fn gid_of(names: &[(String, u32)], group: &str) -> Option<u32> {
    names.iter().find(|(n, _)| n == group).map(|(_, g)| *g)
}

pub fn perm_exec(args: &[String], io: Io) -> i32 {
    if args.len() < 3 || !permissions::valid_app_id(&args[1]) {
        errln!(io, "usage: hxperm-exec APP-ID PROGRAM [ARGS...]");
        return 2;
    }
    let (uid, gid) = (sys::getuid() as u32, sys::getgid());
    let granted = permissions::granted(&args[1]);
    let names = users::group_ids();
    let revoked: Vec<u32> = PERMISSIONS.iter().filter(|p| !granted.iter().any(|g| g == p.group)).filter_map(|p| gid_of(&names, p.group)).collect();
    let groups: Vec<u32> = sys::groups().into_iter().filter(|g| !revoked.contains(g)).collect();
    if sys::geteuid() == 0 {
        if sys::set_groups(&groups) < 0 || sys::setgid(gid) < 0 || sys::setuid(uid) < 0 {
            errln!(io, "hxperm-exec: cannot drop privileges");
            return 1;
        }
    } else if !revoked.is_empty() && sys::set_groups(&groups) < 0 {
        errln!(io, "hxperm-exec: not installed setuid, permissions are not enforced");
    }
    let env: Vec<String> = hxstd::env::vars().iter().map(|(k, v)| alloc::format!("{}={}", k, v)).collect();
    let r = sys::exec(&args[2], &args[2..], &env);
    errln!(io, "hxperm-exec: {}: {}", args[2], sys::error_name(r));
    127
}

fn become_root() -> bool {
    sys::geteuid() == 0 && sys::init_groups("root", 0) >= 0 && sys::setgid(0) >= 0 && sys::setuid(0) >= 0
}

fn permit(args: &[String], io: Io) -> i32 {
    let (Some(app), Some(group), Some(value)) = (args.first(), args.get(1), args.get(2)) else {
        errln!(io, "usage: hxsysconf permit APP-ID PERMISSION yes|no");
        return 2;
    };
    if permissions::find(group).is_none() {
        let known: Vec<&str> = PERMISSIONS.iter().map(|p| p.group).collect();
        errln!(io, "hxsysconf: unknown permission {} (known: {})", group, known.join(", "));
        return 2;
    }
    let allow = match value.as_str() {
        "yes" | "on" | "1" => true,
        "no" | "off" | "0" => false,
        _ => {
            errln!(io, "hxsysconf: expected yes or no");
            return 2;
        }
    };
    let declaration = permissions::declaration(app);
    if !allow && declaration.as_ref().map(|d| d.required.iter().any(|r| r == group)).unwrap_or(false) {
        errln!(io, "hxsysconf: {} cannot run without {}", app, group);
        return 1;
    }
    let requested = permissions::requested(app);
    if allow && !requested.iter().any(|r| r == group) {
        errln!(io, "hxsysconf: {} does not ask for {}", app, group);
        return 1;
    }
    let mut granted = permissions::granted(app);
    granted.retain(|g| g != group);
    if allow {
        granted.push(group.clone());
    }
    match permissions::store("", app, &granted) {
        Ok(()) => 0,
        Err(e) => {
            errln!(io, "hxsysconf: {}", e);
            1
        }
    }
}

fn show(args: &[String], io: Io) -> i32 {
    let apps: Vec<String> = match args.first() {
        Some(app) => alloc::vec![app.clone()],
        None => permissions::applications(),
    };
    for app in apps {
        let granted = permissions::granted(&app);
        let requested = permissions::requested(&app);
        if requested.is_empty() {
            continue;
        }
        let list: Vec<String> = requested.iter().map(|g| alloc::format!("{}{}", if granted.contains(g) { "+" } else { "-" }, g)).collect();
        outln!(io, "{} {}", app, list.join(" "));
    }
    0
}

const ALLOWED_CONF: &[&str] = &["/etc/hamix/login.conf", "/etc/hamix/linux.conf"];

pub fn sysconf(args: &[String], io: Io) -> i32 {
    let Some(command) = args.get(1) else {
        errln!(io, "usage: hxsysconf permit APP PERMISSION yes|no | reset APP | show [APP] | session ... | linux on|off | netconf ... | hostname NAME | setconf FILE KEY VALUE");
        return 2;
    };
    let rest = &args[2..];
    if command == "show" {
        return show(rest, io);
    }
    if !become_root() {
        errln!(io, "hxsysconf: must be installed setuid root (or run with sudo)");
        return 1;
    }
    let forward = |name: &str| -> Vec<String> { core::iter::once(String::from(name)).chain(rest.iter().cloned()).collect() };
    match command.as_str() {
        "permit" => permit(rest, io),
        "reset" => match rest.first() {
            Some(app) if permissions::valid_app_id(app) => {
                permissions::reset("", app);
                0
            }
            _ => {
                errln!(io, "usage: hxsysconf reset APP-ID");
                2
            }
        },
        "session" => crate::system::session(&forward("session"), io),
        "netconf" => crate::net::netconf(&forward("netconf"), io),
        "hostname" => crate::system::hostname(&forward("hostname"), io),
        "setconf" => {
            if !rest.first().map(|f| ALLOWED_CONF.contains(&f.as_str())).unwrap_or(false) {
                errln!(io, "hxsysconf: setconf is limited to {}", ALLOWED_CONF.join(", "));
                return 1;
            }
            crate::system::setconf(&forward("setconf"), io)
        }
        "linux" => {
            let (op, value) = match rest.first().map(|s| s.as_str()) {
                Some("on") => ("on", "yes"),
                Some("off") => ("off", "no"),
                _ => {
                    errln!(io, "usage: hxsysconf linux on|off");
                    return 2;
                }
            };
            let pid = sys::spawn("/usr/bin/hxctl", &["linux", op], 0);
            if pid < 0 || sys::waitpid(pid, false) != 0 {
                errln!(io, "hxsysconf: hxctl linux {} failed", op);
                return 1;
            }
            if crate::set_conf_value("/etc/hamix/linux.conf", "enabled", value) { 0 } else { 1 }
        }
        _ => {
            errln!(io, "hxsysconf: unknown command {}", command);
            2
        }
    }
}
