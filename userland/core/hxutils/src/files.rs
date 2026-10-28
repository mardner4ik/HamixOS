use alloc::format;
use alloc::string::String;
use hxstd::sys;

use crate::outln;
use crate::{join, Io};

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

fn tree_walk(path: &str, prefix: &str, io: Io, counts: &mut (usize, usize), depth: usize, all: bool) {
    let mut entries = sys::read_dir(path).unwrap_or_default();
    entries.retain(|e| e.name != "." && e.name != ".." && (all || !e.name.starts_with('.')));
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, e) in entries.iter().enumerate() {
        let last = i + 1 == entries.len();
        outln!(io, "{}{}{}", prefix, if last { "`-- " } else { "|-- " }, colored(&e.name, e.kind, e.mode));
        if e.is_dir {
            counts.0 += 1;
            if depth < 12 {
                let next = format!("{}{}", prefix, if last { "    " } else { "|   " });
                tree_walk(&join(path, &e.name), &next, io, counts, depth + 1, all);
            }
        } else {
            counts.1 += 1;
        }
    }
}

pub fn tree(args: &[String], io: Io) -> i32 {
    let opts = crate::parse_opts(&args[1..], &[]);
    let root = opts.rest.first().cloned().unwrap_or_else(|| String::from("."));
    outln!(io, "\x1b[94m{}\x1b[0m", root);
    let mut counts = (0, 0);
    tree_walk(&root, "", io, &mut counts, 0, opts.has('a'));
    outln!(io, "\n{} directories, {} files", counts.0, counts.1);
    0
}
