use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{fs, sys};

use super::{parse_opts, Builtin};
use crate::shell::{write, Io, Shell};
use crate::{errln, outln};

pub fn commands() -> Vec<Builtin> {
    alloc::vec![
        Builtin { name: "cat", usage: "cat [-n] [file...]", help: "print files (or standard input)", group: "text", run: cat },
        Builtin { name: "grep", usage: "grep [-i] [-v] [-n] [-c] pattern [file...]", help: "print matching lines", group: "text", run: grep },
        Builtin { name: "head", usage: "head [-n N] [file]", help: "first lines", group: "text", run: head },
        Builtin { name: "tail", usage: "tail [-n N] [file]", help: "last lines", group: "text", run: tail },
        Builtin { name: "wc", usage: "wc [-l] [-w] [-c] [file...]", help: "count lines, words, bytes", group: "text", run: wc },
        Builtin { name: "sort", usage: "sort [-r] [-n] [-u] [file]", help: "sort lines", group: "text", run: sort },
        Builtin { name: "uniq", usage: "uniq [-c] [file]", help: "collapse repeated lines", group: "text", run: uniq },
        Builtin { name: "tee", usage: "tee [-a] file...", help: "copy input to files and output", group: "text", run: tee },
        Builtin { name: "xxd", usage: "xxd [file]", help: "hex dump", group: "text", run: xxd },
        Builtin { name: "write", usage: "write file text...", help: "write text into a file", group: "text", run: write_file },
    ]
}

pub fn read_all(fd: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = sys::read(fd, &mut buf);
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    out
}

fn inputs(files: &[String], io: Io, name: &str) -> Result<Vec<(String, Vec<u8>)>, i32> {
    if files.is_empty() {
        return Ok(alloc::vec![(String::from("-"), read_all(io.input))]);
    }
    let mut out = Vec::new();
    for file in files {
        if file == "-" {
            out.push((file.clone(), read_all(io.input)));
            continue;
        }
        match sys::stat(file) {
            Ok(s) if s.is_dir() => {
                errln!(io, "{}: {}: is a directory", name, file);
                return Err(1);
            }
            Err(e) => {
                errln!(io, "{}: {}: {}", name, file, sys::error_name(e));
                return Err(1);
            }
            _ => {}
        }
        match fs::read(file) {
            Some(data) => out.push((file.clone(), data)),
            None => {
                errln!(io, "{}: {}: permission denied", name, file);
                return Err(1);
            }
        }
    }
    Ok(out)
}

fn cat(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let data = match inputs(&opts.rest, io, "cat") {
        Ok(d) => d,
        Err(c) => return c,
    };
    for (_, bytes) in data {
        if opts.has('n') {
            let text = String::from_utf8_lossy(&bytes);
            for (i, line) in text.lines().enumerate() {
                outln!(io, "{:>6}  {}", i + 1, line);
            }
        } else {
            let mut done = 0;
            while done < bytes.len() {
                let n = sys::write(io.out, &bytes[done..]);
                if n <= 0 {
                    break;
                }
                done += n as usize;
            }
        }
    }
    0
}

fn grep(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let Some(pattern) = opts.rest.first() else {
        errln!(io, "usage: grep [-i] [-v] [-n] [-c] pattern [file...]");
        return 2;
    };
    let ignore = opts.has('i');
    let needle = if ignore { pattern.to_lowercase() } else { pattern.clone() };
    let data = match inputs(&opts.rest[1..], io, "grep") {
        Ok(d) => d,
        Err(c) => return c,
    };
    let many = data.len() > 1;
    let tty = sys::isatty(io.out);
    let mut matched = 0;
    for (name, bytes) in data {
        let text = String::from_utf8_lossy(&bytes);
        let mut count = 0;
        for (i, line) in text.lines().enumerate() {
            let hay = if ignore { line.to_lowercase() } else { String::from(line) };
            let hit = hay.contains(&needle) != opts.has('v');
            if !hit {
                continue;
            }
            count += 1;
            matched += 1;
            if opts.has('c') || opts.has('q') {
                continue;
            }
            let mut shown = String::from(line);
            if tty && !opts.has('v') && !ignore && !needle.is_empty() {
                shown = shown.replace(needle.as_str(), &format!("\x1b[91m{}\x1b[0m", needle));
            }
            let prefix = if many { format!("\x1b[95m{}\x1b[0m:", name) } else { String::new() };
            if opts.has('n') {
                outln!(io, "{}{}:{}", prefix, i + 1, shown);
            } else {
                outln!(io, "{}{}", prefix, shown);
            }
        }
        if opts.has('c') {
            if many {
                outln!(io, "{}:{}", name, count);
            } else {
                outln!(io, "{}", count);
            }
        }
    }
    if matched > 0 { 0 } else { 1 }
}

fn count_arg(opts: &super::Opts, default: usize) -> usize {
    opts.value("n").and_then(|v| v.parse().ok()).or_else(|| opts.rest.iter().find(|a| a.starts_with('-')).and_then(|a| a[1..].parse().ok())).unwrap_or(default)
}

fn head(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["n"]);
    let n = count_arg(&opts, 10);
    let files: Vec<String> = opts.rest.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    let data = match inputs(&files, io, "head") {
        Ok(d) => d,
        Err(c) => return c,
    };
    for (_, bytes) in data {
        for line in String::from_utf8_lossy(&bytes).lines().take(n) {
            outln!(io, "{}", line);
        }
    }
    0
}

fn tail(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &["n"]);
    let n = count_arg(&opts, 10);
    let files: Vec<String> = opts.rest.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    let data = match inputs(&files, io, "tail") {
        Ok(d) => d,
        Err(c) => return c,
    };
    for (_, bytes) in data {
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines[lines.len().saturating_sub(n)..] {
            outln!(io, "{}", line);
        }
    }
    0
}

fn wc(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let data = match inputs(&opts.rest, io, "wc") {
        Ok(d) => d,
        Err(c) => return c,
    };
    let all = !opts.has('l') && !opts.has('w') && !opts.has('c');
    for (name, bytes) in data {
        let text = String::from_utf8_lossy(&bytes);
        let mut parts = Vec::new();
        if all || opts.has('l') {
            parts.push(format!("{:>7}", bytes.iter().filter(|b| **b == b'\n').count()));
        }
        if all || opts.has('w') {
            parts.push(format!("{:>7}", text.split_whitespace().count()));
        }
        if all || opts.has('c') {
            parts.push(format!("{:>7}", bytes.len()));
        }
        if name != "-" {
            parts.push(name);
        }
        outln!(io, "{}", parts.join(" "));
    }
    0
}

fn sort(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let data = match inputs(&opts.rest, io, "sort") {
        Ok(d) => d,
        Err(c) => return c,
    };
    let mut lines: Vec<String> = Vec::new();
    for (_, bytes) in data {
        lines.extend(String::from_utf8_lossy(&bytes).lines().map(String::from));
    }
    if opts.has('n') {
        lines.sort_by(|a, b| {
            let x = a.trim().split_whitespace().next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
            let y = b.trim().split_whitespace().next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
            x.cmp(&y)
        });
    } else {
        lines.sort();
    }
    if opts.has('r') {
        lines.reverse();
    }
    if opts.has('u') {
        lines.dedup();
    }
    for line in lines {
        outln!(io, "{}", line);
    }
    0
}

fn uniq(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let data = match inputs(&opts.rest, io, "uniq") {
        Ok(d) => d,
        Err(c) => return c,
    };
    for (_, bytes) in data {
        let text = String::from_utf8_lossy(&bytes);
        let mut previous: Option<&str> = None;
        let mut count = 0;
        let flush = |line: Option<&str>, count: usize| {
            if let Some(l) = line {
                if opts.has('c') {
                    outln!(io, "{:>7} {}", count, l);
                } else {
                    outln!(io, "{}", l);
                }
            }
        };
        for line in text.lines() {
            if Some(line) == previous {
                count += 1;
            } else {
                flush(previous, count);
                previous = Some(line);
                count = 1;
            }
        }
        flush(previous, count);
    }
    0
}

fn tee(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let opts = parse_opts(&args[1..], &[]);
    let data = read_all(io.input);
    let mut status = 0;
    for file in &opts.rest {
        let ok = if opts.has('a') { fs::append(file, &data) } else { fs::write(file, &data) };
        if !ok {
            errln!(io, "tee: {}: cannot write", file);
            status = 1;
        }
    }
    sys::write(io.out, &data);
    status
}

fn xxd(_: &mut Shell, args: &[String], io: Io) -> i32 {
    let data = match inputs(&args[1..], io, "xxd") {
        Ok(d) => d,
        Err(c) => return c,
    };
    for (_, bytes) in data {
        for (row, chunk) in bytes.chunks(16).enumerate() {
            let mut hex = String::new();
            let mut ascii = String::new();
            for (i, b) in chunk.iter().enumerate() {
                hex.push_str(&format!("{:02x}", b));
                if i % 2 == 1 {
                    hex.push(' ');
                }
                ascii.push(if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' });
            }
            outln!(io, "{:08x}: {:<40} {}", row * 16, hex, ascii);
        }
    }
    0
}

fn write_file(_: &mut Shell, args: &[String], io: Io) -> i32 {
    if args.len() < 2 {
        errln!(io, "usage: write file text...");
        return 1;
    }
    let mut text = args[2..].join(" ");
    text.push('\n');
    if fs::write(&args[1], text.as_bytes()) {
        0
    } else {
        errln!(io, "write: {}: cannot write", args[1]);
        let _ = write;
        1
    }
}
