use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{fs, sys, users};

use super::{parse_opts, Builtin};
use crate::shell::{write, Io, Shell};
use crate::{errln, outln};

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "ls", usage: "ls [-l] [-a] [-h] [-1] [path...]", help: "list directory contents", group: "files", run: ls },
        Builtin { name: "cp", usage: "cp [-r] [-a] src... dest", help: "copy files and directories", group: "files", run: cp },
        Builtin { name: "mv", usage: "mv src... dest", help: "move or rename", group: "files", run: mv },
        Builtin { name: "rm", usage: "rm [-r] [-f] path...", help: "remove files or directories", group: "files", run: rm },
        Builtin { name: "mkdir", usage: "mkdir [-p] dir...", help: "create directories", group: "files", run: mkdir },
        Builtin { name: "rmdir", usage: "rmdir dir...", help: "remove empty directories", group: "files", run: rmdir },
        Builtin { name: "touch", usage: "touch file...", help: "create empty files", group: "files", run: touch },
        Builtin { name: "chmod", usage: "chmod [-R] mode path...", help: "change permissions (octal or u+x style)", group: "files", run: chmod },
        Builtin { name: "chown", usage: "chown [-R] user path...", help: "change the owner (root)", group: "files", run: chown },
        Builtin { name: "tree", usage: "tree [dir]", help: "show a directory tree", group: "files", run: tree },
        Builtin { name: "stat", usage: "stat path...", help: "file details", group: "files", run: stat },
        Builtin { name: "find", usage: "find [dir] [-name pattern] [-type f|d]", help: "search for files", group: "files", run: find },
        Builtin { name: "du", usage: "du [-h] [-s] [path]", help: "disk usage of files", group: "files", run: du },
        Builtin { name: "basename", usage: "basename path", help: "last path component", group: "files", run: basename },
        Builtin { name: "dirname", usage: "dirname path", help: "path without the last component", group: "files", run: dirname },
        Builtin { name: "realpath", usage: "realpath path", help: "absolute path", group: "files", run: realpath },
    ]
}

pub fn absolute(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let cwd = sys::getcwd();
    if !path.starts_with('/') {
        parts.extend(cwd.split('/').filter(|p| !p.is_empty()));
    }
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{}{}", dir, name) } else { format!("{}/{}", dir, name) }
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 4 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}", bytes)
    } else if value >= 10.0 {
        format!("{}{}", (value + 0.5) as u64, UNITS[unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        format!("{}.{}{}", tenths / 10, tenths % 10, UNITS[unit])
    }
}

fn mode_string(kind: char, mode: u32) -> String {
    let mut s = String::new();
    s.push(match kind {
        'd' => 'd',
        'c' => 'c',
        'p' => 'p',
        _ => '-',
    });
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    if mode & 0o1000 != 0 {
        s.pop();
        s.push('t');
    }
    s
}

fn colored(name: &str, kind: char, mode: u32) -> String {
    match kind {
        'd' => format!("\x1b[94m{}\x1b[0m", name),
        'c' => format!("\x1b[93m{}\x1b[0m", name),
        'p' => format!("\x1b[96m{}\x1b[0m", name),
        _ if mode & 0o111 != 0 => format!("\x1b[92m{}\x1b[0m", name),
        _ if name.ends_with(".png") => format!("\x1b[95m{}\x1b[0m", name),
        _ if name.ends_with(".sh") => format!("\x1b[32m{}\x1b[0m", name),
        _ => String::from(name),
    }
}

fn ls(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let long = opts.has('l');
    let all = opts.has('a');
    let human_sizes = opts.has('h');
    let one = opts.has('1') || !sys::isatty(io.out);
    let targets = if opts.rest.is_empty() { alloc::vec![String::from(".")] } else { opts.rest.clone() };
    let width = sys::termsize(io.out).map(|(c, _)| c as usize).unwrap_or(80);
    let mut status = 0;
    for (ti, target) in targets.iter().enumerate() {
        let stat = match sys::stat(target) {
            Ok(s) => s,
            Err(e) => {
                errln!(io, "ls: {}: {}", target, sys::error_name(e));
                status = 1;
                continue;
            }
        };
        let mut entries: Vec<sys::DirEntry> = if stat.is_dir() {
            sys::read_dir(target).unwrap_or_default()
        } else {
            alloc::vec![sys::DirEntry {
                name: target.clone(),
                is_dir: false,
                size: stat.size,
                kind: if stat.kind == sys::KIND_DEVICE { 'c' } else { 'f' },
                mode: stat.mode,
                owner: stat.owner,
            }]
        };
        if !all {
            entries.retain(|e| !e.name.starts_with('.'));
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        if targets.len() > 1 && stat.is_dir() {
            if ti > 0 {
                outln!(io);
            }
            outln!(io, "{}:", target);
        }
        if long {
            let names: Vec<String> = entries.iter().map(|e| users::name_of(e.owner)).collect();
            let owner_w = names.iter().map(|n| n.len()).max().unwrap_or(4);
            let sizes: Vec<String> = entries.iter().map(|e| if human_sizes { human(e.size) } else { format!("{}", e.size) }).collect();
            let size_w = sizes.iter().map(|s| s.len()).max().unwrap_or(1);
            for (i, e) in entries.iter().enumerate() {
                outln!(io, "{} {:<ow$} {:>sw$} {}", mode_string(e.kind, e.mode), names[i], sizes[i], colored(&e.name, e.kind, e.mode), ow = owner_w, sw = size_w);
            }
        } else if one {
            for e in &entries {
                if sys::isatty(io.out) {
                    outln!(io, "{}", colored(&e.name, e.kind, e.mode));
                } else {
                    outln!(io, "{}", e.name);
                }
            }
        } else if !entries.is_empty() {
            let longest = entries.iter().map(|e| e.name.chars().count()).max().unwrap_or(1) + 2;
            let columns = (width / longest).max(1);
            let rows = entries.len().div_ceil(columns);
            for row in 0..rows {
                let mut line = String::new();
                for col in 0..columns {
                    if let Some(e) = entries.get(col * rows + row) {
                        let pad = longest - e.name.chars().count();
                        line.push_str(&colored(&e.name, e.kind, e.mode));
                        if col + 1 < columns {
                            for _ in 0..pad {
                                line.push(' ');
                            }
                        }
                    }
                }
                outln!(io, "{}", line.trim_end());
            }
        }
    }
    status
}

pub fn copy_tree(src: &str, dest: &str, preserve: bool, io: Io, count: &mut usize) -> bool {
    let stat = match sys::stat(src) {
        Ok(s) => s,
        Err(e) => {
            errln!(io, "cp: {}: {}", src, sys::error_name(e));
            return false;
        }
    };
    if stat.is_dir() {
        let r = sys::mkdir(dest);
        if r < 0 && r != -17 {
            errln!(io, "cp: {}: {}", dest, sys::error_name(r));
            return false;
        }
        if preserve {
            sys::chmod(dest, stat.mode);
            sys::chown(dest, stat.owner);
        }
        let mut ok = true;
        for entry in sys::read_dir(src).unwrap_or_default() {
            if entry.kind == 'c' || entry.kind == 'p' {
                continue;
            }
            ok &= copy_tree(&join(src, &entry.name), &join(dest, &entry.name), preserve, io, count);
        }
        return ok;
    }
    if stat.kind != sys::KIND_FILE {
        return true;
    }
    let Some(data) = fs::read(src) else {
        errln!(io, "cp: {}: cannot read", src);
        return false;
    };
    if !fs::write(dest, &data) {
        errln!(io, "cp: {}: cannot write", dest);
        return false;
    }
    if preserve {
        sys::chmod(dest, stat.mode);
        sys::chown(dest, stat.owner);
    } else if stat.mode & 0o111 != 0 {
        sys::chmod(dest, 0o755);
    }
    *count += 1;
    if *count % 32 == 0 {
        sys::yield_now();
    }
    true
}

fn target_path(src: &str, dest: &str, many: bool) -> String {
    let dest_is_dir = sys::stat(dest).map(|s| s.is_dir()).unwrap_or(false);
    if dest_is_dir || many {
        let name = src.trim_end_matches('/').rsplit('/').next().unwrap_or(src);
        join(dest, name)
    } else {
        String::from(dest)
    }
}

fn cp(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    if opts.rest.len() < 2 {
        errln!(io, "usage: cp [-r] [-a] src... dest");
        return 1;
    }
    let recursive = opts.has('r') || opts.has('R') || opts.has('a');
    let preserve = opts.has('a') || opts.has('p');
    let verbose = opts.has('v');
    let (sources, dest) = opts.rest.split_at(opts.rest.len() - 1);
    let dest = &dest[0];
    let mut status = 0;
    let mut count = 0usize;
    for src in sources {
        let is_dir = sys::stat(src).map(|s| s.is_dir()).unwrap_or(false);
        if is_dir && !recursive {
            errln!(io, "cp: {} is a directory (use -r)", src);
            status = 1;
            continue;
        }
        let target = if is_dir && !sys::stat(dest).map(|s| s.is_dir()).unwrap_or(false) { dest.clone() } else { target_path(src, dest, sources.len() > 1) };
        if !copy_tree(src, &target, preserve, io, &mut count) {
            status = 1;
        } else if verbose {
            outln!(io, "{} -> {}", src, target);
        }
    }
    status
}

fn mv(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    if opts.rest.len() < 2 {
        errln!(io, "usage: mv src... dest");
        return 1;
    }
    let (sources, dest) = opts.rest.split_at(opts.rest.len() - 1);
    let mut status = 0;
    for src in sources {
        let target = target_path(src, &dest[0], sources.len() > 1);
        let r = sys::rename(src, &target);
        if r < 0 {
            errln!(io, "mv: {}: {}", src, sys::error_name(r));
            status = 1;
        }
    }
    status
}

pub fn remove_tree(path: &str) -> i64 {
    if let Ok(stat) = sys::stat(path) {
        if stat.is_dir() {
            for entry in sys::read_dir(path).unwrap_or_default() {
                let child = join(path, &entry.name);
                if entry.kind == 'd' {
                    remove_tree(&child);
                }
            }
        }
    }
    sys::unlink(path)
}

fn rm(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let force = opts.has('f');
    let recursive = opts.has('r') || opts.has('R');
    if opts.rest.is_empty() && !force {
        errln!(io, "usage: rm [-r] [-f] path...");
        return 1;
    }
    let mut status = 0;
    for path in &opts.rest {
        let abs = absolute(path);
        if abs == "/" {
            errln!(io, "rm: refusing to remove /");
            status = 1;
            continue;
        }
        match sys::stat(path) {
            Ok(stat) if stat.is_dir() && !recursive => {
                let empty = sys::read_dir(path).map(|e| e.is_empty()).unwrap_or(false);
                if !empty {
                    errln!(io, "rm: {}: is a directory (use -r)", path);
                    status = 1;
                    continue;
                }
            }
            Ok(_) => {}
            Err(e) => {
                if !force {
                    errln!(io, "rm: {}: {}", path, sys::error_name(e));
                    status = 1;
                }
                continue;
            }
        }
        let r = if recursive { remove_tree(path) } else { sys::unlink(path) };
        if r < 0 && !force {
            errln!(io, "rm: {}: {}", path, sys::error_name(r));
            status = 1;
        }
    }
    status
}

fn mkdir(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["m"]);
    let parents = opts.has('p');
    let mode = opts.value("m").and_then(|m| u32::from_str_radix(&m, 8).ok());
    let mut status = 0;
    for dir in &opts.rest {
        if parents {
            let abs = absolute(dir);
            let mut acc = String::new();
            for part in abs.split('/').filter(|p| !p.is_empty()) {
                acc.push('/');
                acc.push_str(part);
                let r = sys::mkdir(&acc);
                if r < 0 && r != -17 {
                    errln!(io, "mkdir: {}: {}", acc, sys::error_name(r));
                    status = 1;
                    break;
                }
            }
        } else {
            let r = sys::mkdir(dir);
            if r < 0 {
                errln!(io, "mkdir: {}: {}", dir, sys::error_name(r));
                status = 1;
                continue;
            }
        }
        if let Some(m) = mode {
            sys::chmod(dir, m);
        }
    }
    status
}

fn rmdir(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut status = 0;
    for dir in &args[1..] {
        match sys::read_dir(dir) {
            Some(entries) if entries.is_empty() => {
                let r = sys::unlink(dir);
                if r < 0 {
                    errln!(io, "rmdir: {}: {}", dir, sys::error_name(r));
                    status = 1;
                }
            }
            Some(_) => {
                errln!(io, "rmdir: {}: directory not empty", dir);
                status = 1;
            }
            None => {
                errln!(io, "rmdir: {}: not a directory", dir);
                status = 1;
            }
        }
    }
    status
}

fn touch(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut status = 0;
    for path in &args[1..] {
        if sys::stat(path).is_ok() {
            continue;
        }
        let fd = sys::open_with(path, sys::O_WRONLY | sys::O_CREAT);
        if fd < 0 {
            errln!(io, "touch: {}: {}", path, sys::error_name(fd));
            status = 1;
        } else {
            sys::close(fd as u64);
        }
    }
    status
}

fn symbolic_mode(spec: &str, current: u32) -> Option<u32> {
    let mut mode = current;
    for clause in spec.split(',') {
        let op_pos = clause.find(|c| c == '+' || c == '-' || c == '=')?;
        let who = &clause[..op_pos];
        let op = clause.as_bytes()[op_pos];
        let perms = &clause[op_pos + 1..];
        let mut mask = 0u32;
        for p in perms.chars() {
            mask |= match p {
                'r' => 0o444,
                'w' => 0o222,
                'x' => 0o111,
                't' => 0o1000,
                _ => return None,
            };
        }
        let mut scope = 0u32;
        for w in who.chars() {
            scope |= match w {
                'u' => 0o4700,
                'g' => 0o2070,
                'o' => 0o1007,
                'a' => 0o7777,
                _ => return None,
            };
        }
        if who.is_empty() {
            scope = 0o7777;
        }
        let bits = mask & scope;
        match op {
            b'+' => mode |= bits,
            b'-' => mode &= !bits,
            _ => mode = (mode & !scope) | bits,
        }
    }
    Some(mode)
}

fn walk(path: &str, f: &mut dyn FnMut(&str)) {
    f(path);
    if sys::stat(path).map(|s| s.is_dir()).unwrap_or(false) {
        for entry in sys::read_dir(path).unwrap_or_default() {
            walk(&join(path, &entry.name), f);
        }
    }
}

fn chmod(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    if opts.rest.len() < 2 {
        errln!(io, "usage: chmod [-R] mode path...");
        return 1;
    }
    let spec = opts.rest[0].clone();
    let mut status = 0;
    for path in &opts.rest[1..] {
        let mut apply = |p: &str| {
            let current = sys::stat(p).map(|s| s.mode).unwrap_or(0o644);
            let mode = if spec.chars().all(|c| c.is_digit(8)) { u32::from_str_radix(&spec, 8).ok() } else { symbolic_mode(&spec, current) };
            match mode {
                Some(m) => {
                    let r = sys::chmod(p, m);
                    if r < 0 {
                        errln!(io, "chmod: {}: {}", p, sys::error_name(r));
                        status = 1;
                    }
                }
                None => {
                    errln!(io, "chmod: invalid mode {}", spec);
                    status = 1;
                }
            }
        };
        if opts.has('R') {
            walk(path, &mut apply);
        } else {
            apply(path);
        }
    }
    status
}

fn chown(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    if opts.rest.len() < 2 {
        errln!(io, "usage: chown [-R] user path...");
        return 1;
    }
    let owner = opts.rest[0].split(':').next().unwrap_or("");
    let uid = match owner.parse::<u32>() {
        Ok(u) => u,
        Err(_) => match users::find("", owner) {
            Some(u) => u.uid,
            None => {
                errln!(io, "chown: {}: no such user", owner);
                return 1;
            }
        },
    };
    let mut status = 0;
    for path in &opts.rest[1..] {
        let mut apply = |p: &str| {
            let r = sys::chown(p, uid);
            if r < 0 {
                errln!(io, "chown: {}: {}", p, sys::error_name(r));
                status = 1;
            }
        };
        if opts.has('R') {
            walk(path, &mut apply);
        } else {
            apply(path);
        }
    }
    status
}

fn tree_walk(path: &str, prefix: &str, io: Io, counts: &mut (usize, usize), depth: usize) {
    let mut entries = sys::read_dir(path).unwrap_or_default();
    entries.retain(|e| !e.name.starts_with('.'));
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, e) in entries.iter().enumerate() {
        let last = i + 1 == entries.len();
        outln!(io, "{}{}{}", prefix, if last { "`-- " } else { "|-- " }, colored(&e.name, e.kind, e.mode));
        if e.is_dir {
            counts.0 += 1;
            if depth < 12 {
                let next = format!("{}{}", prefix, if last { "    " } else { "|   " });
                tree_walk(&join(path, &e.name), &next, io, counts, depth + 1);
            }
        } else {
            counts.1 += 1;
        }
    }
}

fn tree(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let root = args.get(1).cloned().unwrap_or_else(|| String::from("."));
    outln!(io, "\x1b[94m{}\x1b[0m", root);
    let mut counts = (0, 0);
    tree_walk(&root, "", io, &mut counts, 0);
    outln!(io, "\n{} directories, {} files", counts.0, counts.1);
    0
}

fn stat(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut status = 0;
    for path in &args[1..] {
        match sys::stat(path) {
            Ok(s) => {
                let kind = match s.kind {
                    sys::KIND_DIR => "directory",
                    sys::KIND_DEVICE => "device",
                    sys::KIND_PROC => "kernel file",
                    _ => "regular file",
                };
                outln!(io, "  File: {}", absolute(path));
                outln!(io, "  Type: {}   Size: {} ({})", kind, s.size, human(s.size));
                outln!(io, "  Mode: {:04o} ({})   Owner: {} ({})", s.mode, mode_string(if s.is_dir() { 'd' } else { '-' }, s.mode), users::name_of(s.owner), s.owner);
                outln!(io, "  Volume: {}", if s.dev == 0 { String::from("root") } else { format!("mount #{}", s.dev) });
            }
            Err(e) => {
                errln!(io, "stat: {}: {}", path, sys::error_name(e));
                status = 1;
            }
        }
    }
    status
}

fn find(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let mut root = String::from(".");
    let mut name: Option<String> = None;
    let mut kind: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-name" | "-iname" => {
                name = args.get(i + 1).cloned();
                i += 2;
            }
            "-type" => {
                kind = args.get(i + 1).cloned();
                i += 2;
            }
            other => {
                root = String::from(other);
                i += 1;
            }
        }
    }
    let mut found = |path: &str| {
        let base = path.rsplit('/').next().unwrap_or(path);
        if let Some(pattern) = &name {
            if !crate::shell::matches_pattern(pattern, base) {
                return;
            }
        }
        if let Some(k) = &kind {
            let is_dir = sys::stat(path).map(|s| s.is_dir()).unwrap_or(false);
            if (k == "d") != is_dir {
                return;
            }
        }
        outln!(io, "{}", path);
    };
    walk(&root, &mut found);
    0
}

fn usage_of(path: &str) -> u64 {
    match sys::stat(path) {
        Ok(s) if s.is_dir() => sys::read_dir(path).unwrap_or_default().iter().map(|e| usage_of(&join(path, &e.name))).sum(),
        Ok(s) if s.kind == sys::KIND_FILE => s.size,
        _ => 0,
    }
}

fn du(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let targets = if opts.rest.is_empty() { alloc::vec![String::from(".")] } else { opts.rest.clone() };
    for target in targets {
        if !opts.has('s') && sys::stat(&target).map(|s| s.is_dir()).unwrap_or(false) {
            for e in sys::read_dir(&target).unwrap_or_default() {
                let p = join(&target, &e.name);
                let size = usage_of(&p);
                outln!(io, "{:>8}  {}", if opts.has('h') { human(size) } else { format!("{}", size.div_ceil(1024)) }, p);
            }
        }
        let size = usage_of(&target);
        outln!(io, "{:>8}  {}", if opts.has('h') { human(size) } else { format!("{}", size.div_ceil(1024)) }, target);
    }
    0
}

fn basename(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let path = args.get(1).map(|s| s.trim_end_matches('/')).unwrap_or("");
    let mut base = String::from(path.rsplit('/').next().unwrap_or(path));
    if let Some(suffix) = args.get(2) {
        if base.ends_with(suffix.as_str()) && base != *suffix {
            base.truncate(base.len() - suffix.len());
        }
    }
    outln!(io, "{}", base);
    0
}

fn dirname(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let path = args.get(1).map(|s| s.trim_end_matches('/')).unwrap_or("");
    match path.rfind('/') {
        Some(0) => outln!(io, "/"),
        Some(i) => outln!(io, "{}", &path[..i]),
        None => outln!(io, "."),
    }
    0
}

fn realpath(_: &mut Shell, args: &[String], io: Io) -> i32 {
    for path in &args[1..] {
        outln!(io, "{}", absolute(path));
    }
    let _ = write;
    0
}
