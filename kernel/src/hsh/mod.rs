use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

use crate::drivers::input::keyboard::{self, Key};
use crate::drivers::tty::{
    CONSOLE, print_colored, println_colored,
    COLOR_FG, COLOR_PROMPT, COLOR_INPUT, COLOR_ERROR, COLOR_SUCCESS,
    COLOR_HEADER, COLOR_DIM, COLOR_WARN,
};
use crate::drivers::video::{intel_graphics, registry};
use crate::fs;

const MAX_HISTORY: usize = 64;

struct History {
    entries: Vec<String>,
}

impl History {
    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.entries.last().map(|s| s.as_str()) == Some(line) {
            return;
        }
        if self.entries.len() >= MAX_HISTORY {
            self.entries.remove(0);
        }
        self.entries.push(String::from(line));
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn get(&self, idx: usize) -> &str {
        self.entries[idx].as_str()
    }
}

fn redraw_line(buf: &mut String, new: &str, mask: bool) {
    for _ in 0..buf.chars().count() {
        CONSOLE.lock().write_char_colored('\x08', COLOR_INPUT);
    }
    buf.clear();
    buf.push_str(new);
    for ch in buf.chars() {
        let display = if mask { '*' } else { ch };
        CONSOLE.lock().write_char_colored(display, COLOR_INPUT);
    }
}

fn read_line(mask: bool, history: Option<&History>) -> String {
    let mut buf = String::new();
    let mut hist_idx = history.map(|h| h.len()).unwrap_or(0);

    loop {
        match keyboard::read_key_blocking() {
            Key::Enter => {
                CONSOLE.lock().write_char_colored('\n', COLOR_FG);
                break;
            }
            Key::Backspace => {
                if !buf.is_empty() {
                    buf.pop();
                    CONSOLE.lock().write_char_colored('\x08', COLOR_INPUT);
                }
            }
            Key::Char(ch) if ch.is_ascii() && !ch.is_control() => {
                buf.push(ch);
                let display = if mask { '*' } else { ch };
                CONSOLE.lock().write_char_colored(display, COLOR_INPUT);
            }
            Key::Up => {
                if let Some(h) = history {
                    if hist_idx > 0 {
                        hist_idx -= 1;
                        let line = h.get(hist_idx);
                        redraw_line(&mut buf, line, mask);
                    }
                }
            }
            Key::Down => {
                if let Some(h) = history {
                    if hist_idx < h.len() {
                        hist_idx += 1;
                        if hist_idx == h.len() {
                            redraw_line(&mut buf, "", mask);
                        } else {
                            let line = h.get(hist_idx);
                            redraw_line(&mut buf, line, mask);
                        }
                    }
                }
            }
            Key::Ctrl('c') => {
                CONSOLE.lock().write_str_colored("^C\n", COLOR_DIM);
                buf.clear();
                break;
            }
            _ => {}
        }
    }

    buf
}

fn print_banner(vt: usize) {
    let mut c = CONSOLE.lock();
    c.clear();
    c.write_str_colored("+--------------------------------------------------+\n", COLOR_HEADER);
    c.write_str_colored("|          HamixOS  v0.1.0  (x86_64)              |\n", COLOR_HEADER);
    c.write_str_colored("|     Unix-like kernel -- built in Rust            |\n", COLOR_DIM);
    c.write_str_colored("+--------------------------------------------------+\n", COLOR_HEADER);
    c.write_str_colored(&format!("HamixOS tty{}\n", vt + 1), COLOR_DIM);
    c.write_str_colored("\n", COLOR_FG);
}

fn check_credentials(user: &str, pass: &str) -> bool {
    crate::users::find_by_name(user).is_some() && crate::users::verify_password(user, pass)
}

/// Entry point for one virtual terminal's login/shell session (see vt.rs).
/// Each of the VT_COUNT workspaces -- switchable with Ctrl+Alt+F1..F6 --
/// runs its own independent copy of this, with its own login state, cwd,
/// and history; only the actual screen contents and (for at most one VT at
/// a time) a live ring-3 process are shared/exclusive resources.
pub fn run_login(vt: usize) -> ! {
    print_banner(vt);

    loop {
        print_colored("login: ", COLOR_PROMPT);
        let username = read_line(false, None);

        print_colored("password: ", COLOR_PROMPT);
        let password = read_line(true, None);

        if check_credentials(username.trim(), password.trim()) {
            println_colored("\nLogin successful.", COLOR_SUCCESS);
            run_shell(username.trim(), vt);
        } else {
            println_colored("\nLogin incorrect.", COLOR_ERROR);
            print_colored("\n", COLOR_FG);
        }
    }
}

fn home_dir_of(user: &str) -> String {
    crate::users::find_by_name(user)
        .map(|u| u.home)
        .unwrap_or_else(|| format!("/home/{}", user))
}

fn display_cwd(user: &str, cwd_path: &str) -> String {
    let home = home_dir_of(user);
    if cwd_path == home {
        String::from("~")
    } else if let Some(rest) = cwd_path.strip_prefix(&format!("{}/", home)) {
        format!("~/{}", rest)
    } else {
        String::from(cwd_path)
    }
}

fn run_shell(user: &str, vt: usize) -> ! {
    let mut history = History::new();
    let mut cwd_id = fs::VFS.lock().as_ref().map(|v| v.root_id()).unwrap_or(0);
    let mut cwd_path = String::from("/");
    let ruid = crate::users::find_by_name(user).map(|u| u.uid).unwrap_or(1000);

    {
        let mut c = CONSOLE.lock();
        c.write_str_colored("\nWelcome to hsh. Type 'help' for available commands.\n\n", COLOR_SUCCESS);
    }

    loop {
        let path_display = display_cwd(user, &cwd_path);
        let prompt = if ruid == 0 {
            format!("{}@hamix:{}# ", user, path_display)
        } else {
            format!("{}@hamix:{}$ ", user, path_display)
        };
        print_colored(&prompt, COLOR_PROMPT);

        let line = read_line(false, Some(&history));
        let line_str = line.trim();

        if line_str.is_empty() {
            continue;
        }

        history.push(line_str);

        if line_str == "logout" || line_str == "exit" {
            println_colored("\nLogged out.\n", COLOR_DIM);
            print_banner(vt);
            loop {
                print_colored("login: ", COLOR_PROMPT);
                let u = read_line(false, None);
                print_colored("password: ", COLOR_PROMPT);
                let p = read_line(true, None);
                if check_credentials(u.trim(), p.trim()) {
                    println_colored("\nLogin successful.", COLOR_SUCCESS);
                    run_shell(u.trim(), vt);
                } else {
                    println_colored("\nLogin incorrect.", COLOR_ERROR);
                    print_colored("\n", COLOR_FG);
                }
            }
        }

        dispatch(line_str, user, ruid, ruid, &mut cwd_id, &mut cwd_path, &mut history);
    }
}

fn dispatch(
    line_str: &str,
    user: &str,
    ruid: u32,
    euid: u32,
    cwd_id: &mut usize,
    cwd_path: &mut String,
    history: &mut History,
) {
    let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
    let cmd = parts[0];
    let args = parts.get(1).copied().unwrap_or("").trim();

    match cmd {
        "help"     => cmd_help(),
        "clear"    => { CONSOLE.lock().clear(); }
        "edit"     => cmd_edit(*cwd_id, cwd_path.as_str(), args),
        "echo"     => cmd_echo(*cwd_id, args, euid),
        "uname"    => cmd_uname(args),
        "whoami"   => println_colored(if euid == 0 { "root" } else { user }, COLOR_FG),
        "id"       => cmd_id(user, ruid, euid),
        "meminfo"  => cmd_meminfo(),
        "uptime"   => cmd_uptime(),
        "pwd"      => println_colored(cwd_path.as_str(), COLOR_FG),
        "ls"       => cmd_ls(*cwd_id, args),
        "cd"       => cmd_cd(cwd_id, cwd_path, args),
        "cat"      => cmd_cat(*cwd_id, args),
        "mkdir"    => cmd_mkdir(*cwd_id, args, euid),
        "touch"    => cmd_touch(*cwd_id, args, euid),
        "rm"       => cmd_rm(*cwd_id, args, euid),
        "chmod"    => cmd_chmod(*cwd_id, args, euid),
        "chown"    => cmd_chown(*cwd_id, args, euid),
        "tree"     => cmd_tree(*cwd_id, cwd_path.as_str()),
        "fb"       => cmd_fb(args),
        "gpuinfo"  => cmd_gpuinfo(),
        "drivers"  => cmd_drivers(),
        "usb"      => cmd_usb(),
        "mouse"    => cmd_mouse(),
        "ring3smoketest" => cmd_ring3_smoke_test(),
        "exec"     => cmd_exec(args),
        "startx"   => cmd_exec("/usr/bin/hxserver"),
        "diskls"   => cmd_diskls(),
        "diskcat"  => cmd_diskcat(args),
        "hostname" => println_colored("hamix", COLOR_FG),
        "date"     => println_colored("Tue Jan  1 00:00:00 UTC 2000", COLOR_FG),
        "sudo"     => cmd_sudo(args, user, ruid, cwd_id, cwd_path, history),
        "passwd"   => cmd_passwd(user, ruid, euid, args),
        "useradd"  => cmd_useradd(euid, args),
        "halt" | "poweroff" => cmd_halt(euid),
        "reboot"   => cmd_reboot(euid),
        "version"  => cmd_version(),
        "cpuinfo"  => cmd_cpuinfo(),
        "history"  => cmd_history(history),
        "dmesg"    => cmd_dmesg(),
        "hexdump"  => cmd_hexdump(args),
        "inport"   => cmd_inport(args),
        "outport"  => cmd_outport(args),
        "regs"     => cmd_regs(),
        "alloctest" => cmd_alloctest(args),
        "crash"    => cmd_crash(args),
        "logout" | "exit" => {}
        _ => {
            let msg = format!("{}: command not found\n", cmd);
            print_colored(&msg, COLOR_ERROR);
        }
    }
}

fn cmd_id(user: &str, ruid: u32, euid: u32) {
    let msg = if ruid == euid {
        format!("uid={}({}) gid={}({})\n", ruid, user, ruid, user)
    } else {
        format!("uid={}({}) euid={}(root)\n", ruid, user, euid)
    };
    print_colored(&msg, COLOR_FG);
}

fn cmd_sudo(
    args: &str,
    user: &str,
    ruid: u32,
    cwd_id: &mut usize,
    cwd_path: &mut String,
    history: &mut History,
) {
    if args.trim().is_empty() {
        println_colored("usage: sudo <command>", COLOR_ERROR);
        return;
    }
    if ruid == 0 {
        dispatch(args.trim(), user, ruid, 0, cwd_id, cwd_path, history);
        return;
    }
    if !crate::users::is_sudoer(user) {
        let msg = format!("{} is not in the sudoers file. This incident will be reported.\n", user);
        print_colored(&msg, COLOR_ERROR);
        return;
    }
    let prompt = format!("[sudo] password for {}: ", user);
    print_colored(&prompt, COLOR_PROMPT);
    let password = read_line(true, None);
    if crate::users::verify_password(user, password.trim()) {
        dispatch(args.trim(), user, ruid, 0, cwd_id, cwd_path, history);
    } else {
        println_colored("\nsudo: incorrect password", COLOR_ERROR);
    }
}

fn cmd_passwd(user: &str, ruid: u32, euid: u32, args: &str) {
    let target = if args.trim().is_empty() { user } else { args.trim() };
    if target != user && euid != 0 {
        println_colored("passwd: permission denied (try: sudo passwd <user>)", COLOR_ERROR);
        return;
    }
    if crate::users::find_by_name(target).is_none() {
        println_colored("passwd: no such user", COLOR_ERROR);
        return;
    }
    if target == user && euid != 0 {
        print_colored("Current password: ", COLOR_PROMPT);
        let current = read_line(true, None);
        if !crate::users::verify_password(user, current.trim()) {
            println_colored("\npasswd: authentication failure", COLOR_ERROR);
            return;
        }
        println_colored("", COLOR_FG);
    }
    let _ = ruid;
    print_colored("New password: ", COLOR_PROMPT);
    let new_pass = read_line(true, None);
    print_colored("\nRetype new password: ", COLOR_PROMPT);
    let confirm = read_line(true, None);
    if new_pass.trim() != confirm.trim() || new_pass.trim().is_empty() {
        println_colored("\npasswd: passwords do not match", COLOR_ERROR);
        return;
    }
    match crate::users::set_password(target, new_pass.trim()) {
        Ok(()) => println_colored("\npasswd: password updated successfully", COLOR_SUCCESS),
        Err(e) => println_colored(&format!("\npasswd: {}", e), COLOR_ERROR),
    }
}

fn cmd_useradd(euid: u32, args: &str) {
    if euid != 0 {
        println_colored("useradd: permission denied (try: sudo useradd <name>)", COLOR_ERROR);
        return;
    }
    let name = args.trim();
    if name.is_empty() {
        println_colored("usage: useradd <name>", COLOR_ERROR);
        return;
    }
    print_colored("New password: ", COLOR_PROMPT);
    let password = read_line(true, None);
    let uid = 1001 + name.len() as u32;
    match crate::users::add_user(name, uid, uid, password.trim()) {
        Ok(()) => println_colored("\nuseradd: user created", COLOR_SUCCESS),
        Err(e) => println_colored(&format!("\nuseradd: {}", e), COLOR_ERROR),
    }
}

fn cmd_help() {
    let mut text = String::new();
    text.push_str("Available commands:\n\n");
    let cmds = [
        ("help",       "show this help"),
        ("clear",      "clear the screen"),
        ("echo <msg>", "print a message"),
        ("uname [-a]", "kernel information"),
        ("whoami",     "current user name"),
        ("meminfo",    "memory usage"),
        ("uptime",     "system uptime"),
        ("pwd",        "current directory"),
        ("ls [path]",  "list directory entries"),
        ("cd <path>",  "change directory"),
        ("cat <path>", "print file contents"),
        ("mkdir <p>",  "create a directory"),
        ("touch <p>",  "create an empty file"),
        ("rm <path>",  "remove a file or empty dir"),
        ("chmod <mode> <p>", "change permission bits (octal)"),
        ("chown <user> <p>", "change file owner (root only)"),
        ("id",         "show current uid/euid"),
        ("sudo <cmd>", "run a command as root"),
        ("passwd [user]", "change a password"),
        ("useradd <name>", "create a new user (root only)"),
        ("tree",       "show directory tree from cwd"),
        ("echo a > f", "write/append output to a file"),
        ("fb <color>", "fill the linear framebuffer (intel-graphics-driver)"),
        ("gpuinfo",    "graphics chipset and framebuffer information"),
        ("drivers",    "list registered kernel drivers"),
        ("usb",        "list USB host controllers and attached devices"),
        ("mouse",      "pointer state; drag to select text, middle-click pastes"),
        ("ring3smoketest", "one-way jump to a tiny ring-3 stub (GDT/TSS/SYSCALL smoke test)"),
        ("exec <path>", "run a static ET_EXEC x86_64 binary and return here when it exits"),
        ("edit <path>", "open a file in the hed text editor"),
        ("startx",      "run the Nook desktop (Esc returns here; click Nook for the menu)"),
        ("Ctrl+Alt+F1..F6", "switch workspaces -- each keeps its own login/shell; a running exec/edit/startx session stays put and picks up right where you left it"),
        ("Ctrl+C",      "kill whatever exec/edit/startx session is currently running on this workspace"),
        ("diskls",     "list root dir of the ext4 disk image module"),
        ("diskcat <p>","read a file from the ext4 disk image module"),
        ("hostname",   "system hostname"),
        ("cpuinfo",    "processor information"),
        ("version",    "HamixOS version"),
        ("history",    "show command history"),
        ("logout",     "log out current user"),
        ("halt",       "halt the system"),
        ("reboot",     "reboot the system"),
    ];
    for (name, desc) in &cmds {
        let line = format!("  {:<14}  {}\n", name, desc);
        text.push_str(&line);
    }

    text.push_str("\nDeveloper tools:\n");
    let dev_cmds = [
        ("dmesg",                 "show kernel boot log"),
        ("hexdump <addr> [len]",  "dump raw memory (hex, default len=64)"),
        ("inport <port>",         "read a byte from an I/O port"),
        ("outport <port> <val>",  "write a byte to an I/O port"),
        ("regs",                  "dump cr0/cr3/cr4 control registers"),
        ("alloctest <bytes>",     "test the kernel heap allocator"),
        ("crash <div0|bp|ud|pf>", "trigger a CPU exception (tests the IDT)"),
    ];
    for (name, desc) in &dev_cmds {
        let line = format!("  {:<22}  {}\n", name, desc);
        text.push_str(&line);
    }

    // The full text doesn't fit on one 80x25 screen, which is exactly why
    // this now opens in hed (a scrolling view) instead of printing
    // straight to the console and scrolling most of it out of sight.
    open_in_hed("/run/help.txt", &text, true);
}

/// Claims the single system-wide ring-3 slot (see vt::try_claim_ring3),
/// runs the binary, and releases the slot again if it never made it into
/// ring 3 at all (a real exit already releases it from the SYS_EXIT
/// handler in syscall.rs, so this is just the defensive early-failure
/// path). Every place that execs a ring-3 binary -- exec, edit, startx,
/// and help's "open in hed" -- goes through this so two workspaces can
/// never end up with two live processes corrupting each other's memory.
fn run_ring3(path: &str) -> Result<i32, String> {
    let vt = crate::vt::current();
    if let Err(owner) = crate::vt::try_claim_ring3(vt) {
        return Err(format!(
            "a graphical/editor session is already open on tty{} -- switch there (Ctrl+Alt+F{}) or quit it first",
            owner + 1,
            owner + 1
        ));
    }
    let result = crate::task::elf::load_and_exec(path);
    if result.is_err() {
        crate::vt::release_ring3(vt);
    }
    result.map_err(String::from)
}

/// Writes `content` to `path` (as root, since /run is a system scratch
/// area not owned by any particular user) and writes the hed handoff file
/// (see apps/hed's doc comment -- HamixOS's exec has no argv, so this
/// fixed file is how hed learns what to open), then execs hed.
fn open_in_hed(path: &str, content: &str, view_only: bool) {
    let mut guard = fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        println_colored("edit: filesystem not mounted", COLOR_ERROR);
        return;
    };
    let root = vfs.root_id();
    if let Err(e) = vfs.write(root, path, content.as_bytes(), false, crate::users::ROOT_UID) {
        println_colored(&format!("edit: {}", e), COLOR_ERROR);
        return;
    }
    let handoff = format!("{}\n{}\n", path, if view_only { "view" } else { "edit" });
    if let Err(e) = vfs.write(root, "/run/hed_target", handoff.as_bytes(), false, crate::users::ROOT_UID) {
        println_colored(&format!("edit: {}", e), COLOR_ERROR);
        return;
    }
    drop(guard);

    match run_ring3("/usr/bin/hed") {
        Ok(_) => {
            // hed already cleared the screen with its own \x0C on the way
            // out; just repaint the shell's own prompt/banner state.
            CONSOLE.lock().clear();
        }
        Err(e) => println_colored(&format!("edit: {}", e), COLOR_ERROR),
    }
}

fn cmd_edit(cwd: usize, cwd_path: &str, args: &str) {
    let rel = args.trim();
    if rel.is_empty() {
        println_colored("usage: edit <path>", COLOR_ERROR);
        return;
    }
    let abs_path = if rel.starts_with('/') {
        rel.to_string()
    } else {
        format!("{}/{}", cwd_path.trim_end_matches('/'), rel)
    };

    // hed opens files itself (from the filesystem root, since a fresh
    // process has no inherited cwd), so unlike open_in_hed's help-text
    // case we don't pre-write the content -- just point it at the real
    // (already-resolved-to-absolute) path so saves land back in the
    // actual file rather than a throwaway copy.
    let mut guard = fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        println_colored("edit: filesystem not mounted", COLOR_ERROR);
        return;
    };
    let root = vfs.root_id();
    if !vfs.exists(cwd, rel) && vfs.create_file(cwd, rel, Vec::new(), crate::users::ROOT_UID).is_err() {
        println_colored("edit: could not create file", COLOR_ERROR);
        return;
    }
    let handoff = format!("{}\nedit\n", abs_path);
    if let Err(e) = vfs.write(root, "/run/hed_target", handoff.as_bytes(), false, crate::users::ROOT_UID) {
        println_colored(&format!("edit: {}", e), COLOR_ERROR);
        return;
    }
    drop(guard);

    match run_ring3("/usr/bin/hed") {
        Ok(_) => {
            CONSOLE.lock().clear();
        }
        Err(e) => println_colored(&format!("edit: {}", e), COLOR_ERROR),
    }
}

fn cmd_uname(args: &str) {
    if args.contains('a') {
        println_colored("HamixOS hamix 0.1.0 x86_64 GNU/Rust", COLOR_FG);
    } else {
        println_colored("HamixOS", COLOR_FG);
    }
}

fn cmd_meminfo() {
    use crate::memory::frame::memory_info;
    let (free, total) = memory_info();
    let used = total.saturating_sub(free);
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nMemory:\n", COLOR_HEADER);
    let line = format!("  Total : {:>8} KB\n", total / 1024);
    c.write_str_colored(&line, COLOR_FG);
    let line = format!("  Used  : {:>8} KB\n", used / 1024);
    c.write_str_colored(&line, COLOR_WARN);
    let line = format!("  Free  : {:>8} KB\n\n", free / 1024);
    c.write_str_colored(&line, COLOR_SUCCESS);
}

fn cmd_uptime() {
    let ticks = crate::task::uptime_ticks();
    let secs = ticks / 100;
    let msg = format!("up {} seconds ({} ticks)\n", secs, ticks);
    print_colored(&msg, COLOR_FG);
}

fn cmd_ls(cwd: usize, args: &str) {
    let target = args.trim();
    let listing = {
        let guard = fs::VFS.lock();
        guard.as_ref().and_then(|vfs| vfs.list(cwd, target).ok())
    };
    match listing {
        Some(mut entries) => {
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut c = CONSOLE.lock();
            for (name, is_dir) in entries {
                if is_dir {
                    c.write_str_colored(&format!("{}/  ", name), COLOR_PROMPT);
                } else {
                    c.write_str_colored(&format!("{}  ", name), COLOR_FG);
                }
            }
            c.write_str_colored("\n", COLOR_FG);
        }
        None => println_colored("ls: cannot access path", COLOR_ERROR),
    }
}

fn cmd_cd(cwd_id: &mut usize, cwd_path: &mut String, args: &str) {
    let target = if args.trim().is_empty() { "/" } else { args.trim() };
    let resolved = {
        let guard = fs::VFS.lock();
        guard.as_ref().and_then(|vfs| {
            let id = vfs.resolve(*cwd_id, target)?;
            if vfs.is_dir(id) { Some(id) } else { None }
        })
    };
    match resolved {
        Some(id) => {
            *cwd_id = id;
            *cwd_path = normalize_path(cwd_path, target);
        }
        None => println_colored("cd: no such directory", COLOR_ERROR),
    }
}

fn normalize_path(base: &str, rel: &str) -> String {
    let mut stack: Vec<String> = Vec::new();
    let start_abs = rel.starts_with('/');
    if !start_abs {
        for part in base.split('/') {
            if !part.is_empty() {
                stack.push(String::from(part));
            }
        }
    }
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => { stack.pop(); }
            p => stack.push(String::from(p)),
        }
    }
    if stack.is_empty() {
        String::from("/")
    } else {
        format!("/{}", stack.join("/"))
    }
}

fn cmd_cat(cwd: usize, args: &str) {
    let path = args.trim();
    if path.is_empty() {
        println_colored("usage: cat <path>", COLOR_ERROR);
        return;
    }
    let data = {
        let guard = fs::VFS.lock();
        guard.as_ref().and_then(|vfs| vfs.read(cwd, path).ok())
    };
    match data {
        Some(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            print_colored(&text, COLOR_FG);
            if !text.ends_with('\n') {
                print_colored("\n", COLOR_FG);
            }
        }
        None => println_colored("cat: no such file", COLOR_ERROR),
    }
}

fn cmd_mkdir(cwd: usize, args: &str, euid: u32) {
    let path = args.trim();
    if path.is_empty() {
        println_colored("usage: mkdir <path>", COLOR_ERROR);
        return;
    }
    let mut guard = fs::VFS.lock();
    match guard.as_mut().map(|vfs| vfs.mkdir(cwd, path, euid)) {
        Some(Ok(_)) => {}
        Some(Err(e)) => println_colored(&format!("mkdir: {}", e), COLOR_ERROR),
        None => println_colored("mkdir: filesystem not mounted", COLOR_ERROR),
    }
}

fn cmd_touch(cwd: usize, args: &str, euid: u32) {
    let path = args.trim();
    if path.is_empty() {
        println_colored("usage: touch <path>", COLOR_ERROR);
        return;
    }
    let mut guard = fs::VFS.lock();
    if let Some(vfs) = guard.as_mut() {
        if !vfs.exists(cwd, path) {
            if let Err(e) = vfs.create_file(cwd, path, Vec::new(), euid) {
                println_colored(&format!("touch: {}", e), COLOR_ERROR);
            }
        }
    }
}

fn cmd_rm(cwd: usize, args: &str, euid: u32) {
    let path = args.trim();
    if path.is_empty() {
        println_colored("usage: rm <path>", COLOR_ERROR);
        return;
    }
    let mut guard = fs::VFS.lock();
    match guard.as_mut().map(|vfs| vfs.remove(cwd, path, euid)) {
        Some(Ok(())) => {}
        Some(Err(e)) => println_colored(&format!("rm: {}", e), COLOR_ERROR),
        None => println_colored("rm: filesystem not mounted", COLOR_ERROR),
    }
}

fn cmd_echo(cwd: usize, args: &str, euid: u32) {
    if let Some(idx) = args.find('>') {
        let append = args[idx..].starts_with(">>");
        let text = args[..idx].trim();
        let path_start = idx + if append { 2 } else { 1 };
        let path = args[path_start..].trim();
        if path.is_empty() {
            println_colored("echo: missing target file", COLOR_ERROR);
            return;
        }
        let mut data = alloc::vec::Vec::from(text.as_bytes());
        data.push(b'\n');
        let mut guard = fs::VFS.lock();
        match guard.as_mut().map(|vfs| vfs.write(cwd, path, &data, append, euid)) {
            Some(Ok(())) => {}
            Some(Err(e)) => println_colored(&format!("echo: {}", e), COLOR_ERROR),
            None => println_colored("echo: filesystem not mounted", COLOR_ERROR),
        }
    } else {
        println_colored(args, COLOR_FG);
    }
}

fn cmd_chmod(cwd: usize, args: &str, euid: u32) {
    let mut parts = args.split_whitespace();
    let mode_str = parts.next();
    let path = parts.next();
    match (mode_str, path) {
        (Some(mode_str), Some(path)) => match u16::from_str_radix(mode_str, 8) {
            Ok(mode) => {
                let mut guard = fs::VFS.lock();
                match guard.as_mut().map(|vfs| vfs.chmod(cwd, path, euid, mode)) {
                    Some(Ok(())) => {}
                    Some(Err(e)) => println_colored(&format!("chmod: {}", e), COLOR_ERROR),
                    None => println_colored("chmod: filesystem not mounted", COLOR_ERROR),
                }
            }
            Err(_) => println_colored("chmod: mode must be octal, e.g. 755", COLOR_ERROR),
        },
        _ => println_colored("usage: chmod <octal-mode> <path>", COLOR_ERROR),
    }
}

fn cmd_chown(cwd: usize, args: &str, euid: u32) {
    let mut parts = args.split_whitespace();
    let owner_name = parts.next();
    let path = parts.next();
    match (owner_name, path) {
        (Some(owner_name), Some(path)) => match crate::users::find_by_name(owner_name) {
            Some(record) => {
                let mut guard = fs::VFS.lock();
                match guard.as_mut().map(|vfs| vfs.chown(cwd, path, euid, record.uid)) {
                    Some(Ok(())) => {}
                    Some(Err(e)) => println_colored(&format!("chown: {}", e), COLOR_ERROR),
                    None => println_colored("chown: filesystem not mounted", COLOR_ERROR),
                }
            }
            None => println_colored("chown: no such user", COLOR_ERROR),
        },
        _ => println_colored("usage: chown <user> <path>", COLOR_ERROR),
    }
}

fn cmd_tree(cwd: usize, cwd_path: &str) {
    println_colored(cwd_path, COLOR_HEADER);
    let guard = fs::VFS.lock();
    if let Some(vfs) = guard.as_ref() {
        tree_walk(vfs, cwd, 0);
    }
}

fn tree_walk(vfs: &fs::Vfs, dir: usize, depth: usize) {
    if depth > 6 {
        return;
    }
    if let Ok(mut entries) = vfs.list(dir, "") {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, is_dir) in entries {
            let indent = "  ".repeat(depth + 1);
            if is_dir {
                let line = format!("{}{}/", indent, name);
                println_colored(&line, COLOR_PROMPT);
                if let Some(child) = vfs.resolve(dir, &name) {
                    tree_walk(vfs, child, depth + 1);
                }
            } else {
                let line = format!("{}{}", indent, name);
                println_colored(&line, COLOR_FG);
            }
        }
    }
}

fn cmd_diskls() {
    let data = match fs::disk_image_slice() {
        Some(d) => d,
        None => {
            println_colored("diskls: no disk image module was loaded by GRUB", COLOR_ERROR);
            return;
        }
    };
    match fs::ext4::Ext4Fs::mount(data) {
        Ok(disk) => {
            let mut c = CONSOLE.lock();
            c.write_str_colored("\n/ (ext4)\n", COLOR_HEADER);
            for entry in disk.list_root() {
                let marker = if entry.is_dir { "/" } else { "" };
                let line = format!("  {}{}\n", entry.name, marker);
                c.write_str_colored(&line, COLOR_FG);
            }
            c.write_str_colored("\n", COLOR_FG);
        }
        Err(e) => println_colored(&format!("diskls: {}", e), COLOR_ERROR),
    }
}

fn cmd_diskcat(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        println_colored("usage: diskcat <path-on-ext4-image>", COLOR_ERROR);
        return;
    }
    let data = match fs::disk_image_slice() {
        Some(d) => d,
        None => {
            println_colored("diskcat: no disk image module was loaded by GRUB", COLOR_ERROR);
            return;
        }
    };
    match fs::ext4::Ext4Fs::mount(data) {
        Ok(disk) => match disk.find(path) {
            Some((ino, _)) => match disk.read_file(ino) {
                Some(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    print_colored(&text, COLOR_FG);
                    if !text.ends_with('\n') {
                        print_colored("\n", COLOR_FG);
                    }
                }
                None => println_colored("diskcat: could not read inode", COLOR_ERROR),
            },
            None => println_colored("diskcat: no such file", COLOR_ERROR),
        },
        Err(e) => println_colored(&format!("diskcat: {}", e), COLOR_ERROR),
    }
}

fn cmd_exec(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        println_colored("exec: usage: exec <path to static ET_EXEC binary>", COLOR_ERROR);
        return;
    }
    match run_ring3(path) {
        Ok(code) => {
            let msg = format!("exec: {} exited with code {}\n", path, code);
            println_colored(&msg, if code == 0 { COLOR_SUCCESS } else { COLOR_WARN });
        }
        Err(e) => println_colored(&format!("exec: {}", e), COLOR_ERROR),
    }
}

fn cmd_ring3_smoke_test() {
    println_colored(
        "ring3smoketest: jumping to ring 3 now -- this does not return, no scheduler yet.",
        COLOR_WARN,
    );
    println_colored(
        "ring3smoketest: watch for \"hello from ring3\" below; reboot afterwards.",
        COLOR_DIM,
    );
    crate::task::usermode::run_smoke_test();
}

fn cmd_fb(args: &str) {
    if !intel_graphics::available() {
        println_colored("fb: no linear framebuffer available (boot without VBE/gfxterm?)", COLOR_ERROR);
        return;
    }
    let (w, h) = intel_graphics::resolution().unwrap_or((0, 0));
    match args.trim() {
        "gradient" => {
            intel_graphics::fill_gradient();
        }
        "red" => intel_graphics::fill_screen(0x00FF0000),
        "green" => intel_graphics::fill_screen(0x0000FF00),
        "blue" => intel_graphics::fill_screen(0x000000FF),
        "black" => intel_graphics::fill_screen(0x00000000),
        _ => {
            let msg = format!("fb: {}x{} framebuffer ready. usage: fb <red|green|blue|black|gradient>", w, h);
            println_colored(&msg, COLOR_FG);
            return;
        }
    }
    let msg = format!("fb: filled {}x{} framebuffer", w, h);
    println_colored(&msg, COLOR_SUCCESS);
}

fn cmd_gpuinfo() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nGraphics:\n", COLOR_HEADER);
    let chipset_line = format!("  Chipset      : {}\n", intel_graphics::chipset_name());
    c.write_str_colored(&chipset_line, COLOR_FG);
    let codename_line = format!("  Codename     : {}\n", intel_graphics::chipset_codename());
    c.write_str_colored(&codename_line, COLOR_FG);
    let driver_line = format!(
        "  Driver       : {} {}\n",
        intel_graphics::driver_name(),
        intel_graphics::driver_version()
    );
    c.write_str_colored(&driver_line, COLOR_FG);
    if intel_graphics::available() {
        let (w, h) = intel_graphics::resolution().unwrap_or((0, 0));
        let res_line = format!("  Framebuffer  : {}x{}\n", w, h);
        c.write_str_colored(&res_line, COLOR_FG);
        c.write_str_colored("  Status       : ready\n\n", COLOR_SUCCESS);
    } else {
        c.write_str_colored("  Status       : no linear framebuffer reported by firmware\n\n", COLOR_WARN);
    }
}

fn cmd_drivers() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("\n", COLOR_FG);
    let header = format!("  {:<22}  {:<8}  {:<11}  {}\n", "NAME", "VERSION", "KIND", "STATUS");
    c.write_str_colored(&header, COLOR_HEADER);
    registry::for_each(|driver| {
        let status = if driver.is_ready() { "ready" } else { "not ready" };
        let line = format!(
            "  {:<22}  {:<8}  {:<11}  {}\n",
            driver.name(),
            driver.version(),
            driver.kind(),
            status
        );
        let color = if driver.is_ready() { COLOR_FG } else { COLOR_DIM };
        c.write_str_colored(&line, color);
    });
    c.write_str_colored("\n", COLOR_FG);
}

fn cmd_usb() {
    use crate::drivers::usb;

    let controllers = usb::controllers();
    let mut c = CONSOLE.lock();
    c.write_str_colored("\n", COLOR_FG);

    if controllers.is_empty() {
        c.write_str_colored("  no USB host controllers on the PCI bus\n\n", COLOR_DIM);
        return;
    }

    c.write_str_colored(
        &format!("  {:<20}  {:<9}  {:<7}  {}\n", "CONTROLLER", "PCI ID", "BASE", "STATE"),
        COLOR_HEADER,
    );
    for controller in &controllers {
        c.write_str_colored(
            &format!(
                "  {:<20}  {:04x}:{:04x}  {:#07x}  {}\n",
                controller.kind.name(),
                controller.device.vendor,
                controller.device.device,
                controller.base,
                if controller.note.is_empty() { "ready" } else { &controller.note }
            ),
            if controller.driven { COLOR_FG } else { COLOR_DIM },
        );
    }

    let devices = usb::devices();
    c.write_str_colored("\n", COLOR_FG);
    if devices.is_empty() {
        c.write_str_colored("  no USB devices attached\n\n", COLOR_DIM);
        return;
    }
    c.write_str_colored(
        &format!("  {:<5}  {:<5}  {:<9}  {:<9}  {}\n", "ADDR", "PORT", "ID", "SPEED", "CLASS"),
        COLOR_HEADER,
    );
    for device in &devices {
        c.write_str_colored(
            &format!(
                "  {:<5}  {:<5}  {:04x}:{:04x}  {:<9}  {}\n",
                device.address,
                device.port,
                device.vendor,
                device.product,
                if device.low_speed { "low" } else { "full" },
                match device.class {
                    usb::DeviceClass::HidMouse => "HID mouse",
                    usb::DeviceClass::HidKeyboard => "HID keyboard",
                    usb::DeviceClass::HidOther => "HID device",
                    usb::DeviceClass::MassStorage => "mass storage",
                    usb::DeviceClass::Hub => "hub",
                    usb::DeviceClass::Other => "other",
                }
            ),
            COLOR_FG,
        );
    }
    c.write_str_colored("\n", COLOR_FG);
}

fn cmd_mouse() {
    use crate::drivers::input::mouse;

    let state = mouse::state();
    let mut c = CONSOLE.lock();
    c.write_str_colored("\n", COLOR_FG);
    if !mouse::present() {
        c.write_str_colored("  no pointing device detected\n\n", COLOR_DIM);
        return;
    }
    c.write_str_colored(&format!("  position   {}, {}\n", state.x, state.y), COLOR_FG);
    c.write_str_colored(&format!("  buttons    {:03b}\n", state.buttons & 7), COLOR_FG);
    c.write_str_colored(&format!("  wheel      {}\n", state.wheel), COLOR_FG);
    c.write_str_colored(
        "  drag with the left button to select, middle button pastes\n\n",
        COLOR_DIM,
    );
}

fn cmd_halt(euid: u32) {
    if euid != 0 {
        println_colored("halt: permission denied (try: sudo halt)", COLOR_ERROR);
        return;
    }
    print_colored("\nSystem halted. Power off your machine.\n", COLOR_WARN);
    use crate::arch::x86_64::disable_interrupts;
    disable_interrupts();
    loop { crate::arch::x86_64::hlt(); }
}

fn cmd_reboot(euid: u32) {
    if euid != 0 {
        println_colored("reboot: permission denied (try: sudo reboot)", COLOR_ERROR);
        return;
    }
    print_colored("\nRebooting...\n", COLOR_WARN);
    use crate::arch::x86_64::{outb, disable_interrupts};
    disable_interrupts();
    outb(0x64, 0xFE);
    loop { crate::arch::x86_64::hlt(); }
}

fn cmd_version() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("HamixOS v0.1.0\n", COLOR_HEADER);
    c.write_str_colored("Kernel : Rust no_std, x86_64\n", COLOR_FG);
    c.write_str_colored("Shell  : hsh\n", COLOR_FG);
    c.write_str_colored("Boot   : GRUB2 Multiboot2\n", COLOR_FG);
    let info = crate::arch::x86_64::cpuid::identify();
    let cpu_line = if info.brand.is_empty() {
        format!("CPU    : {}\n\n", info.vendor)
    } else {
        format!("CPU    : {}\n\n", info.brand)
    };
    c.write_str_colored(&cpu_line, COLOR_FG);
}

fn cmd_cpuinfo() {
    let info = crate::arch::x86_64::cpuid::identify();
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nProcessor:\n", COLOR_HEADER);
    let name_line = if info.brand.is_empty() {
        format!("  Model        : {} (brand string unavailable)\n", info.vendor)
    } else {
        format!("  Model        : {}\n", info.brand)
    };
    c.write_str_colored(&name_line, COLOR_FG);
    c.write_str_colored(&format!("  Vendor       : {}\n", info.vendor), COLOR_FG);
    c.write_str_colored("  Architecture : x86_64\n", COLOR_FG);
    c.write_str_colored("  Mode         : 64-bit Long Mode\n", COLOR_FG);
    c.write_str_colored(
        &format!("  Family/Model : family {}, model {}, stepping {}\n", info.family, info.model, info.stepping),
        COLOR_FG,
    );
    let mut feats = alloc::vec::Vec::new();
    if info.features.fpu { feats.push("FPU"); }
    if info.features.mmx { feats.push("MMX"); }
    if info.features.sse { feats.push("SSE"); }
    if info.features.sse2 { feats.push("SSE2"); }
    if info.features.sse3 { feats.push("SSE3"); }
    if info.features.ssse3 { feats.push("SSSE3"); }
    if info.features.sse4_1 { feats.push("SSE4.1"); }
    if info.features.sse4_2 { feats.push("SSE4.2"); }
    if info.features.avx { feats.push("AVX"); }
    c.write_str_colored(&format!("  Features     : {}\n\n", feats.join(", ")), COLOR_FG);
}

fn cmd_history(history: &History) {
    let mut c = CONSOLE.lock();
    if history.len() == 0 {
        c.write_str_colored("\n(empty)\n\n", COLOR_DIM);
        return;
    }
    c.write_str_colored("\n", COLOR_FG);
    for i in 0..history.len() {
        let line = format!("  {:>4}  {}\n", i + 1, history.get(i));
        c.write_str_colored(&line, COLOR_FG);
    }
    c.write_str_colored("\n", COLOR_FG);
}

fn cmd_dmesg() {
    let mut c = CONSOLE.lock();
    c.write_str_colored("\n", COLOR_FG);
    crate::drivers::klog::for_each(|line| {
        let msg = format!("{}\n", line);
        c.write_str_colored(&msg, COLOR_FG);
    });
    c.write_str_colored("\n", COLOR_FG);
}

fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

fn cmd_hexdump(args: &str) {
    let mut parts = args.split_whitespace();
    let addr = match parts.next().and_then(parse_hex) {
        Some(a) => a as usize,
        None => {
            println_colored("usage: hexdump <hex_addr> [len]", COLOR_ERROR);
            return;
        }
    };
    let len = parts
        .next()
        .and_then(|s| usize::from_str_radix(s, 10).ok())
        .unwrap_or(64)
        .min(4096);

    let mut c = CONSOLE.lock();
    c.write_str_colored("\n", COLOR_FG);

    let mut off = 0usize;
    while off < len {
        let line_addr = addr + off;
        let mut hex_part = String::new();
        let mut ascii_part = String::new();
        let count = (len - off).min(16);

        for i in 0..count {
            let byte = unsafe { core::ptr::read_volatile((line_addr + i) as *const u8) };
            hex_part.push_str(&format!("{:02x} ", byte));
            let printable = byte >= 0x20 && byte < 0x7F;
            ascii_part.push(if printable { byte as char } else { '.' });
        }

        let line = format!("  {:016x}:  {:<48}|{}|\n", line_addr, hex_part, ascii_part);
        c.write_str_colored(&line, COLOR_FG);
        off += count;
    }
    c.write_str_colored("\n", COLOR_FG);
}

fn cmd_inport(args: &str) {
    let port = match parse_hex(args.split_whitespace().next().unwrap_or("")) {
        Some(p) => p as u16,
        None => {
            println_colored("usage: inport <hex_port>", COLOR_ERROR);
            return;
        }
    };
    let val = crate::arch::x86_64::inb(port);
    let msg = format!("port {:#06x} -> {:#04x}\n", port, val);
    print_colored(&msg, COLOR_FG);
}

fn cmd_outport(args: &str) {
    let mut parts = args.split_whitespace();
    let port = parts.next().and_then(parse_hex);
    let val = parts.next().and_then(parse_hex);
    match (port, val) {
        (Some(p), Some(v)) => {
            crate::arch::x86_64::outb(p as u16, v as u8);
            let msg = format!("port {:#06x} <- {:#04x}\n", p, v);
            print_colored(&msg, COLOR_SUCCESS);
        }
        _ => println_colored("usage: outport <hex_port> <hex_val>", COLOR_ERROR),
    }
}

fn cmd_regs() {
    let cr0: u64;
    let cr3: u64;
    let cr4: u64;
    unsafe {
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
    }
    let mut c = CONSOLE.lock();
    c.write_str_colored("\nControl registers:\n", COLOR_HEADER);
    c.write_str_colored(&format!("  CR0 : {:#018x}\n", cr0), COLOR_FG);
    c.write_str_colored(&format!("  CR3 : {:#018x}\n", cr3), COLOR_FG);
    c.write_str_colored(&format!("  CR4 : {:#018x}\n\n", cr4), COLOR_FG);
}

fn cmd_alloctest(args: &str) {
    let size = args
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1024);

    use core::alloc::Layout;
    let layout = match Layout::from_size_align(size, 8) {
        Ok(l) => l,
        Err(_) => {
            println_colored("invalid size", COLOR_ERROR);
            return;
        }
    };

    unsafe {
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            println_colored("allocation failed", COLOR_ERROR);
        } else {
            let msg = format!("allocated {} bytes at {:#x}\n", size, ptr as usize);
            print_colored(&msg, COLOR_SUCCESS);
            alloc::alloc::dealloc(ptr, layout);
            println_colored("freed ok", COLOR_SUCCESS);
        }
    }
}

fn cmd_crash(args: &str) {
    match args.trim() {
        "div0" => unsafe {
            core::arch::asm!(
                "xor edx, edx",
                "mov eax, 1",
                "div ecx",
                out("eax") _,
                out("edx") _,
                in("ecx") 0u32,
                options(nomem, nostack)
            );
        },
        "bp" => unsafe {
            core::arch::asm!("int3", options(nomem, nostack));
        },
        "ud" => unsafe {
            core::arch::asm!("ud2", options(nomem, nostack));
        },
        "pf" => unsafe {
            let bad = 0x0000_7000_0000_0000usize as *const u8;
            let _ = core::ptr::read_volatile(bad);
        },
        _ => println_colored("usage: crash <div0|bp|ud|pf>", COLOR_ERROR),
    }
}
