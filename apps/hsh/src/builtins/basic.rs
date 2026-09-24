use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::{fs, sys};

use super::{parse_opts, Builtin};
use crate::shell::{write, Flow, Io, Shell};
use crate::{errln, out, outln};

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "cd", usage: "cd [dir]", help: "change the working directory", group: "shell", run: cd },
        Builtin { name: "pwd", usage: "pwd", help: "print the working directory", group: "shell", run: pwd },
        Builtin { name: "echo", usage: "echo [-n] [-e] text", help: "print text", group: "shell", run: echo },
        Builtin { name: "printf", usage: "printf format [args]", help: "formatted output (%s %d %%, \\n \\t)", group: "shell", run: printf },
        Builtin { name: "exit", usage: "exit [code]", help: "leave the shell", group: "shell", run: exit },
        Builtin { name: "logout", usage: "logout", help: "leave the login shell", group: "shell", run: exit },
        Builtin { name: "export", usage: "export NAME[=value]", help: "set a variable for this shell", group: "shell", run: export },
        Builtin { name: "unset", usage: "unset NAME", help: "remove a variable", group: "shell", run: unset },
        Builtin { name: "set", usage: "set [-e|-x|+e|+x]", help: "show variables or set shell options", group: "shell", run: set },
        Builtin { name: "source", usage: "source file [args]", help: "run a script in this shell", group: "shell", run: source },
        Builtin { name: ".", usage: ". file [args]", help: "same as source", group: "shell", run: source },
        Builtin { name: "sh", usage: "sh file.sh [args]", help: "run a shell script in a new hsh", group: "shell", run: sh },
        Builtin { name: "alias", usage: "alias [name=command]", help: "define or list aliases", group: "shell", run: alias },
        Builtin { name: "unalias", usage: "unalias name", help: "remove an alias", group: "shell", run: unalias },
        Builtin { name: "history", usage: "history [-c]", help: "command history", group: "shell", run: history },
        Builtin { name: "help", usage: "help [command]", help: "list commands or describe one", group: "shell", run: help },
        Builtin { name: "type", usage: "type name", help: "tell how a name would be run", group: "shell", run: which },
        Builtin { name: "which", usage: "which name", help: "locate a program", group: "shell", run: which },
        Builtin { name: "true", usage: "true", help: "do nothing, successfully", group: "shell", run: |_, _, _| 0 },
        Builtin { name: "false", usage: "false", help: "do nothing, unsuccessfully", group: "shell", run: |_, _, _| 1 },
        Builtin { name: ":", usage: ":", help: "do nothing", group: "shell", run: |_, _, _| 0 },
        Builtin { name: "test", usage: "test expression", help: "evaluate -e -f -d -z -n = != -eq -lt ...", group: "shell", run: test },
        Builtin { name: "[", usage: "[ expression ]", help: "same as test", group: "shell", run: test },
        Builtin { name: "read", usage: "read [-s] [-p prompt] NAME...", help: "read a line into variables", group: "shell", run: read },
        Builtin { name: "shift", usage: "shift [n]", help: "drop positional arguments", group: "shell", run: shift },
        Builtin { name: "sleep", usage: "sleep seconds", help: "pause (fractions allowed)", group: "shell", run: sleep },
        Builtin { name: "clear", usage: "clear", help: "clear the screen", group: "shell", run: |_, _, io| { write(io.out, "\x1b[2J\x1b[H"); 0 } },
        Builtin { name: "exec", usage: "exec program [args]", help: "run a program and exit with its status", group: "shell", run: exec },
        Builtin { name: "wait", usage: "wait [pid]", help: "wait for background jobs", group: "shell", run: wait },
        Builtin { name: "jobs", usage: "jobs", help: "list background jobs", group: "shell", run: jobs },
        Builtin { name: "env", usage: "env", help: "print exported environment variables", group: "shell", run: env },
        Builtin { name: "break", usage: "break", help: "leave a loop", group: "shell", run: |sh, _, _| { sh.flow = Flow::Break; 0 } },
        Builtin { name: "continue", usage: "continue", help: "next loop iteration", group: "shell", run: |sh, _, _| { sh.flow = Flow::Continue; 0 } },
        Builtin { name: "return", usage: "return [code]", help: "leave a function", group: "shell", run: ret },
        Builtin { name: "eval", usage: "eval text", help: "run text as a command", group: "shell", run: eval },
        Builtin { name: "loadconf", usage: "loadconf file", help: "load KEY=value lines as variables", group: "shell", run: loadconf },
        Builtin { name: "setconf", usage: "setconf file key value", help: "set key=value in a config file", group: "shell", run: setconf },
    ]
}

fn cd(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let target = match args.get(1).map(|s| s.as_str()) {
        None | Some("~") => sh.home(),
        Some("-") => sh.var("OLDPWD"),
        Some(dir) => String::from(dir),
    };
    let old = sys::getcwd();
    let r = sys::chdir(&target);
    if r < 0 {
        errln!(io, "cd: {}: {}", target, sys::error_name(r));
        return 1;
    }
    sh.set_var("OLDPWD", &old);
    0
}

fn pwd(_: &mut Shell, _: &[String], io: Io) -> i32 {
    outln!(io, "{}", sys::getcwd());
    0
}

pub fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('0') => out.push('\0'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn echo(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut newline = true;
    let mut escapes = false;
    let mut start = 1;
    while let Some(flag) = args.get(start) {
        match flag.as_str() {
            "-n" => newline = false,
            "-e" => escapes = true,
            "-ne" | "-en" => {
                newline = false;
                escapes = true;
            }
            _ => break,
        }
        start += 1;
    }
    let mut text = args[start.min(args.len())..].join(" ");
    if escapes {
        text = unescape(&text);
    }
    if newline {
        text.push('\n');
    }
    write(io.out, &text);
    0
}

fn printf(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(format) = args.get(1) else {
        errln!(io, "usage: printf format [args]");
        return 1;
    };
    let format = unescape(format);
    let mut values = args[2..].iter();
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut width = String::new();
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() || d == '-' {
                width.push(d);
                chars.next();
            } else {
                break;
            }
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some(spec @ ('s' | 'd')) => {
                let value = values.next().cloned().unwrap_or_default();
                let value = if spec == 'd' { value.trim().parse::<i64>().unwrap_or(0).to_string() } else { value };
                let w: usize = width.trim_start_matches('-').parse().unwrap_or(0);
                if width.starts_with('-') {
                    out.push_str(&format!("{:<w$}", value, w = w));
                } else {
                    out.push_str(&format!("{:>w$}", value, w = w));
                }
            }
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    write(io.out, &out);
    0
}

fn exit(sh: &mut Shell, args: &[String], _: Io) -> i32 {
    let code = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(sh.status);
    sh.exit = Some(code);
    code
}

fn export(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() == 1 {
        for entry in sh.environment() {
            outln!(io, "export {}", entry);
        }
        return 0;
    }
    for arg in &args[1..] {
        match arg.split_once('=') {
            Some((k, v)) => sh.set_var(k, v),
            None => {}
        }
        let name = arg.split('=').next().unwrap_or("").to_string();
        if !sh.exported.contains(&name) {
            sh.exported.push(name);
        }
    }
    0
}

fn unset(sh: &mut Shell, args: &[String], _: Io) -> i32 {
    for name in &args[1..] {
        sh.vars.remove(name);
        sh.functions.remove(name);
        sh.exported.retain(|e| e != name);
    }
    0
}

fn set(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() == 1 {
        return show_vars(sh, args, io);
    }
    for arg in &args[1..] {
        match arg.as_str() {
            "-e" => sh.errexit = true,
            "+e" => sh.errexit = false,
            "-x" => sh.xtrace = true,
            "+x" => sh.xtrace = false,
            "-ex" | "-xe" => {
                sh.errexit = true;
                sh.xtrace = true;
            }
            "--" => {}
            other => {
                errln!(io, "set: unknown option {}", other);
                return 1;
            }
        }
    }
    0
}

fn source(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(path) = args.get(1) else {
        errln!(io, "usage: source file [args]");
        return 1;
    };
    sh.run_script(path, &args[2..], io)
}

fn sh(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() < 2 {
        errln!(io, "usage: sh file.sh [args]");
        return 1;
    }
    let mut argv: Vec<String> = alloc::vec![String::from("hsh")];
    argv.extend_from_slice(&args[1..]);
    let argv: Vec<&str> = argv[1..].iter().map(|a| a.as_str()).collect();
    let pid = sh.spawn_with_env("/usr/bin/hsh", &argv, if sh.interactive { sys::SPAWN_FOREGROUND } else { 0 }, io);
    if pid < 0 {
        errln!(io, "sh: {}", sys::error_name(pid));
        return 127;
    }
    sh.wait_foreground(pid)
}

fn alias(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() == 1 {
        for (k, v) in &sh.aliases {
            outln!(io, "alias {}='{}'", k, v);
        }
        return 0;
    }
    for arg in &args[1..] {
        match arg.split_once('=') {
            Some((k, v)) => {
                sh.aliases.insert(String::from(k), String::from(v));
            }
            None => match sh.aliases.get(arg) {
                Some(v) => outln!(io, "alias {}='{}'", arg, v),
                None => {
                    errln!(io, "alias: {}: not found", arg);
                    return 1;
                }
            },
        }
    }
    0
}

fn unalias(sh: &mut Shell, args: &[String], _: Io) -> i32 {
    for name in &args[1..] {
        sh.aliases.remove(name);
    }
    0
}

fn history(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.get(1).map(|s| s.as_str()) == Some("-c") {
        sh.history.clear();
        return 0;
    }
    for (i, line) in sh.history.iter().enumerate() {
        outln!(io, "{:>5}  {}", i + 1, line);
    }
    0
}

fn help(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let table = super::table();
    if let Some(name) = args.get(1) {
        return match table.iter().find(|b| b.name == name) {
            Some(b) => {
                outln!(io, "\x1b[1m{}\x1b[0m -- {}", b.usage, b.help);
                0
            }
            None => {
                errln!(io, "help: no builtin named {}", name);
                1
            }
        };
    }
    let groups = [("shell", "Shell"), ("files", "Files and text"), ("system", "System"), ("users", "Users"), ("disk", "Disks and installation")];
    outln!(io, "\x1b[1mhsh\x1b[0m -- the HamixOS shell. `help <command>` shows usage; programs in $PATH run by name.");
    for (key, title) in groups {
        let names: Vec<&str> = table.iter().filter(|b| b.group == key || (key == "files" && b.group == "text")).map(|b| b.name).collect();
        outln!(io, "\n\x1b[93m{}\x1b[0m", title);
        let mut line = String::from(" ");
        for name in names {
            if line.len() + name.len() + 1 > 78 {
                outln!(io, "{}", line);
                line = String::from(" ");
            }
            line.push(' ');
            line.push_str(name);
        }
        if line.trim().len() > 0 {
            outln!(io, "{}", line);
        }
    }
    outln!(io, "\nSyntax: a; b   a && b   a || b   a | b   > >> < 2> 2>&1 &>   cmd &   $VAR ${{VAR:-default}} $(cmd)");
    outln!(io, "        if ...; then ...; elif ...; else ...; fi   while/until ...; do ...; done   for x in ...; do ...; done");
    outln!(io, "        name() {{ ... }}   # comments   scripts: hsh file.sh or ./file.sh with #!/usr/bin/hsh");
    0
}

fn which(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut status = 0;
    for name in &args[1..] {
        if sh.aliases.contains_key(name) {
            outln!(io, "{}: alias for '{}'", name, sh.aliases[name]);
        } else if sh.functions.contains_key(name) {
            outln!(io, "{}: shell function", name);
        } else if super::is_builtin(name) {
            outln!(io, "{}: hsh builtin", name);
        } else if let Some(path) = sh.resolve_program(name) {
            match crate::shell::binary_kind(&path) {
                Some(kind) if args[0] == "type" => outln!(io, "{} is {} ({})", name, path, kind),
                _ => outln!(io, "{}", path),
            }
        } else if let Some(cmd) = sys::cmd_list().into_iter().find(|c| &c.name == name) {
            outln!(io, "{}: registered command -> {} {}", name, cmd.path, cmd.args.join(" "));
        } else {
            errln!(io, "{}: not found", name);
            status = 1;
        }
    }
    status
}

fn test(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut items: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();
    if args[0] == "[" {
        if items.last() != Some(&"]") {
            errln!(io, "[: missing ]");
            return 2;
        }
        items.pop();
    }
    if evaluate(&items) { 0 } else { 1 }
}

fn evaluate(items: &[&str]) -> bool {
    if let Some(pos) = items.iter().position(|s| *s == "-o") {
        return evaluate(&items[..pos]) || evaluate(&items[pos + 1..]);
    }
    if let Some(pos) = items.iter().position(|s| *s == "-a") {
        return evaluate(&items[..pos]) && evaluate(&items[pos + 1..]);
    }
    if items.first() == Some(&"!") {
        return !evaluate(&items[1..]);
    }
    match items {
        [] => false,
        [single] => !single.is_empty(),
        [op, value] => {
            let stat = sys::stat(value);
            match *op {
                "-n" => !value.is_empty(),
                "-z" => value.is_empty(),
                "-e" => stat.is_ok(),
                "-f" => stat.map(|s| s.kind == sys::KIND_FILE).unwrap_or(false),
                "-d" => stat.map(|s| s.is_dir()).unwrap_or(false),
                "-s" => stat.map(|s| s.size > 0).unwrap_or(false),
                "-x" => stat.map(|s| s.mode & 0o111 != 0).unwrap_or(false),
                "-r" => fs::read_prefix(value, 1).is_some(),
                "-w" => stat.is_ok(),
                "-b" | "-c" => stat.map(|s| s.kind == sys::KIND_DEVICE).unwrap_or(false),
                _ => false,
            }
        }
        [a, op, b] => {
            let na = a.trim().parse::<i64>();
            let nb = b.trim().parse::<i64>();
            match *op {
                "=" | "==" => a == b,
                "!=" => a != b,
                "-eq" => na.is_ok() && na == nb,
                "-ne" => na != nb,
                "-lt" => matches!((na, nb), (Ok(x), Ok(y)) if x < y),
                "-le" => matches!((na, nb), (Ok(x), Ok(y)) if x <= y),
                "-gt" => matches!((na, nb), (Ok(x), Ok(y)) if x > y),
                "-ge" => matches!((na, nb), (Ok(x), Ok(y)) if x >= y),
                _ => false,
            }
        }
        _ => false,
    }
}

pub fn read_line_fd(fd: u64, secret: bool) -> Option<String> {
    if sys::isatty(fd) {
        let mut line = String::new();
        loop {
            let key = sys::read_key();
            match key {
                sys::Key::Enter => {
                    write(1, "\n");
                    return Some(line);
                }
                sys::Key::Backspace => {
                    if line.pop().is_some() && !secret {
                        write(1, "\x08 \x08");
                    }
                }
                sys::Key::Char(3) => {
                    write(1, "^C\n");
                    return None;
                }
                sys::Key::Char(4) if line.is_empty() => return None,
                sys::Key::Char(c) if c >= 0x20 => {
                    line.push(c as char);
                    if !secret {
                        let mut b = [0u8; 4];
                        write(1, (c as char).encode_utf8(&mut b));
                    }
                }
                _ => {}
            }
        }
    }
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = sys::read(fd, &mut byte);
        if n <= 0 {
            if bytes.is_empty() {
                return None;
            }
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        bytes.push(byte[0]);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn read(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["p"]);
    if let Some(prompt) = opts.value("p") {
        write(io.err, &prompt);
    }
    let Some(line) = read_line_fd(io.input, opts.has('s')) else {
        return 1;
    };
    let names = if opts.rest.is_empty() { alloc::vec![String::from("REPLY")] } else { opts.rest.clone() };
    let mut words: Vec<&str> = line.split_whitespace().collect();
    for (i, name) in names.iter().enumerate() {
        if i + 1 == names.len() {
            let value = if words.len() > i { words[i..].join(" ") } else { String::new() };
            sh.set_var(name, &value);
        } else {
            let value = if i < words.len() { words[i] } else { "" };
            sh.set_var(name, value);
        }
    }
    words.clear();
    0
}

fn shift(sh: &mut Shell, args: &[String], _: Io) -> i32 {
    let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    for _ in 0..n {
        if sh.args.len() > 1 {
            sh.args.remove(1);
        }
    }
    0
}

fn sleep(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(value) = args.get(1) else {
        errln!(io, "usage: sleep seconds");
        return 1;
    };
    let (whole, frac) = value.split_once('.').unwrap_or((value.as_str(), ""));
    let mut ms = whole.parse::<u64>().unwrap_or(0) * 1000;
    let frac: String = frac.chars().chain("000".chars()).take(3).collect();
    ms += frac.parse::<u64>().unwrap_or(0);
    if !sh.interactive || !sys::isatty(0) {
        sys::sleep_ms(ms);
        return 0;
    }
    let deadline = sys::uptime_ms() + ms;
    while sys::uptime_ms() < deadline {
        sys::sleep_ms((deadline - sys::uptime_ms()).min(50));
        while let Some(key) = sys::poll_key() {
            if key == sys::Key::Char(3) {
                write(1, "^C\n");
                return 130;
            }
        }
    }
    0
}

fn exec(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() < 2 {
        return 0;
    }
    let status = match sh.spawn_external(&args[1..], io, true) {
        Ok(pid) => sh.wait_foreground(pid),
        Err(e) => {
            errln!(io, "exec: {}", e);
            127
        }
    };
    sh.exit = Some(status);
    status
}

fn wait(sh: &mut Shell, args: &[String], _: Io) -> i32 {
    let targets: Vec<i64> = match args.get(1).and_then(|s| s.parse().ok()) {
        Some(pid) => alloc::vec![pid],
        None => sh.jobs.iter().map(|(p, _)| *p).collect(),
    };
    for pid in targets {
        while sys::proc_alive(pid) {
            sys::sleep_ms(50);
        }
    }
    sh.jobs.retain(|(p, _)| sys::proc_alive(*p));
    0
}

fn jobs(sh: &mut Shell, _: &[String], io: Io) -> i32 {
    sh.jobs.retain(|(p, _)| sys::proc_alive(*p));
    for (i, (pid, src)) in sh.jobs.iter().enumerate() {
        outln!(io, "[{}] {:>5} running  {}", i + 1, pid, src);
    }
    0
}

fn env(sh: &mut Shell, _: &[String], io: Io) -> i32 {
    for entry in sh.environment() {
        outln!(io, "{}", entry);
    }
    0
}

fn show_vars(sh: &mut Shell, _: &[String], io: Io) -> i32 {
    for (k, v) in &sh.vars {
        outln!(io, "{}={}", k, v);
    }
    0
}

fn ret(sh: &mut Shell, args: &[String], _: Io) -> i32 {
    sh.flow = Flow::Return;
    args.get(1).and_then(|s| s.parse().ok()).unwrap_or(sh.status)
}

fn eval(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let text = args[1..].join(" ");
    sh.run_source(&text, io)
}

pub fn parse_conf(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            let v = v.trim();
            let v = if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) { &v[1..v.len() - 1] } else { v };
            Some((String::from(k.trim()), String::from(v)))
        })
        .collect()
}

fn loadconf(sh: &mut Shell, args: &[String], io: Io) -> i32 {
    let Some(path) = args.get(1) else {
        errln!(io, "usage: loadconf file");
        return 1;
    };
    let Some(text) = fs::read_to_string(path) else {
        errln!(io, "loadconf: {}: cannot read", path);
        return 1;
    };
    for (k, v) in parse_conf(&text) {
        sh.set_var(&k, &v);
    }
    0
}

pub fn set_conf_value(path: &str, key: &str, value: &str) -> bool {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut out = String::new();
    let mut done = false;
    for line in text.lines() {
        if line.trim_start().split('=').next().map(|k| k.trim()) == Some(key) && !line.trim_start().starts_with('#') {
            out.push_str(&format!("{}={}\n", key, value));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !done {
        out.push_str(&format!("{}={}\n", key, value));
    }
    fs::write(path, out.as_bytes())
}

fn setconf(_: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() < 4 {
        errln!(io, "usage: setconf file key value");
        return 1;
    }
    if set_conf_value(&args[1], &args[2], &args[3..].join(" ")) {
        0
    } else {
        errln!(io, "setconf: cannot write {}", args[1]);
        1
    }
}

pub fn print_kv(io: Io, key: &str, value: &str) {
    out!(io, "  \x1b[90m{:<14}\x1b[0m {}\n", key, value);
}
