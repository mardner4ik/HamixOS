use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::util::*;

pub struct Call {
    pub nr: u64,
    pub name: String,
    pub status: String,
}

pub struct Abi {
    pub version: u32,
    pub core: Vec<Call>,
    pub hamix: Vec<Call>,
}

const DOC: &str = "docs/reference/hamix-abi.md";
const KERNEL_TABLE: &str = "kernel/src/syscall/core_table.rs";
const C_HEADER: &str = "sdk/hamix_abi/include/hamix/syscall.h";
const RUST_NR: &str = "hxlibc/rust/hxlibc-sys/src/nr.rs";
const HXLIBC_HEADER: &str = "hxlibc/sysdeps/hamix/include/hamix/syscall.h";
const HXLIBC_TABLE: &str = "hxlibc/sysdeps/hamix/include/hamix/syscall-table.h";

fn parse_row(line: &str) -> Option<Call> {
    let cells: Vec<&str> = line.trim().trim_matches('|').split(" | ").map(|c| c.trim()).collect();
    if cells.len() < 5 {
        return None;
    }
    let nr = cells[0].parse().ok()?;
    let name = cells[1];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        return None;
    }
    Some(Call { nr, name: String::from(name), status: String::from(cells[4]) })
}

pub fn load() -> Result<Abi> {
    let text = fs::read_to_string(root().join(DOC)).map_err(|e| format!("{}: {}", DOC, e))?;
    let mut abi = Abi { version: 0, core: Vec::new(), hamix: Vec::new() };
    let mut section = 0;
    for line in text.lines() {
        if line.starts_with("## 3.") {
            section = 3;
        } else if line.starts_with("## 4.") {
            section = 4;
        } else if line.starts_with("## ") {
            section = 0;
        }
        if let Some(rest) = line.split("HAMIX_ABI_VERSION = ").nth(1) {
            let hex = rest.trim_matches(|c: char| c == '`' || c == '.' || c.is_whitespace()).trim_start_matches("0x");
            abi.version = u32::from_str_radix(hex.split('`').next().unwrap_or(hex), 16).unwrap_or(0);
        }
        if !line.starts_with("| ") {
            continue;
        }
        if let Some(call) = parse_row(line) {
            match section {
                3 => abi.core.push(call),
                4 => abi.hamix.push(call),
                _ => {}
            }
        }
    }
    if abi.version == 0 {
        return Err(format!("{}: no `HAMIX_ABI_VERSION = 0x...` line", DOC));
    }
    let mut names = BTreeMap::new();
    for call in abi.core.iter().chain(abi.hamix.iter()) {
        if let Some(other) = names.insert(call.name.clone(), call.nr) {
            return Err(format!("{}: the name {} is used by both {} and {}", DOC, call.name, other, call.nr));
        }
    }
    for table in [&abi.core, &abi.hamix] {
        let mut seen = BTreeMap::new();
        for call in table {
            if let Some(other) = seen.insert(call.nr, call.name.clone()) {
                return Err(format!("{}: number {} is used by both {} and {}", DOC, call.nr, other, call.name));
            }
        }
    }
    Ok(abi)
}

fn kernel_table(abi: &Abi) -> String {
    let mut numbers: Vec<u64> = abi.core.iter().map(|c| c.nr).collect();
    numbers.sort();
    let mut out = String::from("pub fn allowed(number: u64) -> bool {\n    matches!(\n        number,\n");
    for chunk in numbers.chunks(16) {
        out.push_str("        ");
        out.push_str(&chunk.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" | "));
        out.push_str(if chunk.as_ptr_range().end == numbers.as_ptr_range().end { "\n" } else { " |\n" });
    }
    out.push_str("    )\n}\n");
    out
}

fn c_header(abi: &Abi) -> String {
    let mut out = String::from("#ifndef HAMIX_SYSCALL_H\n#define HAMIX_SYSCALL_H\n\n");
    out.push_str(&format!("#define HAMIX_ABI_VERSION 0x{:04x}\n#define HAMIX_ABI_LEGACY 1\n#define HAMIX_NOTE_TYPE 0x48584f53\n\n", abi.version));
    for call in abi.core.iter().chain(abi.hamix.iter()) {
        out.push_str(&format!("#define HAMIX_SYS_{} {}\n", call.name, call.nr));
    }
    out.push_str("\n#endif\n");
    out
}

fn c_table(abi: &Abi) -> String {
    let mut out = String::from("#ifndef HAMIX_SYSCALL_TABLE_H\n#define HAMIX_SYSCALL_TABLE_H\n\nstruct hamix_syscall_entry {\n    long nr;\n    const char *name;\n    int core;\n    int deprecated;\n};\n\nstatic const struct hamix_syscall_entry hamix_syscalls[] = {\n");
    for call in abi.core.iter() {
        out.push_str(&format!("    {{ {}, \"{}\", 1, {} }},\n", call.nr, call.name, (call.status == "deprecated") as u8));
    }
    for call in abi.hamix.iter() {
        out.push_str(&format!("    {{ {}, \"{}\", 0, {} }},\n", call.nr, call.name, (call.status == "deprecated") as u8));
    }
    out.push_str("};\n\n#define HAMIX_SYSCALL_COUNT (sizeof hamix_syscalls / sizeof hamix_syscalls[0])\n\n#endif\n");
    out
}

fn rust_numbers(abi: &Abi) -> String {
    let mut out = format!("pub const HAMIX_ABI_VERSION: u32 = 0x{:04x};\npub const HAMIX_ABI_LEGACY: u32 = 1;\n\n", abi.version);
    for call in abi.core.iter().chain(abi.hamix.iter()) {
        if call.status == "deprecated" {
            out.push_str("#[deprecated]\n");
        }
        out.push_str(&format!("pub const {}: u64 = {};\n", call.name, call.nr));
    }
    out
}

fn kernel_constants(path: &Path) -> Result<BTreeMap<String, u64>> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.split("const SYS_").nth(1) else {
            continue;
        };
        let Some((name, value)) = rest.split_once(": u64 = ") else {
            continue;
        };
        if let Ok(nr) = value.trim_end_matches(';').trim().parse::<u64>() {
            map.insert(String::from(name), nr);
        }
    }
    Ok(map)
}

fn check_kernel(abi: &Abi) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let mut native = BTreeMap::new();
    for entry in fs::read_dir(root().join("kernel/src/syscall/ext")).map_err(|e| e.to_string())?.filter_map(|e| e.ok()) {
        if entry.path().extension().map(|x| x == "rs").unwrap_or(false) {
            native.extend(kernel_constants(&entry.path())?);
        }
    }
    for call in &abi.hamix {
        match native.get(&call.name) {
            Some(nr) if *nr == call.nr => {}
            Some(nr) => problems.push(format!("{}: the document says {}, the kernel says {}", call.name, call.nr, nr)),
            None => problems.push(format!("{} ({}) is in the document but not in kernel/src/syscall/ext/", call.name, call.nr)),
        }
    }
    for (name, nr) in &native {
        if *nr >= 9000 && !abi.hamix.iter().any(|c| &c.name == name) {
            problems.push(format!("SYS_{} ({}) is in the kernel but not in {}", name, nr, DOC));
        }
    }
    let mut core_numbers: Vec<u64> = kernel_constants(&root().join("kernel/src/syscall/mod.rs"))?.values().copied().collect();
    core_numbers.extend(kernel_constants(&root().join("kernel/src/syscall/table.rs"))?.values().copied());
    for call in &abi.core {
        if !core_numbers.contains(&call.nr) {
            problems.push(format!("core call {} ({}) has no kernel implementation", call.name, call.nr));
        }
    }
    Ok(problems)
}

fn sync(path: &str, content: &str, check: bool, stale: &mut Vec<String>) -> Result {
    let full = root().join(path);
    let current = fs::read_to_string(&full).unwrap_or_default();
    if current == content {
        return Ok(());
    }
    if check {
        stale.push(String::from(path));
        return Ok(());
    }
    write(&full, content)?;
    say(&format!("abi: wrote {}", path));
    Ok(())
}

pub fn generate(check: bool) -> Result {
    let abi = load()?;
    let mut stale = Vec::new();
    sync(KERNEL_TABLE, &kernel_table(&abi), check, &mut stale)?;
    sync(C_HEADER, &c_header(&abi), check, &mut stale)?;
    sync(HXLIBC_HEADER, &c_header(&abi), check, &mut stale)?;
    sync(HXLIBC_TABLE, &c_table(&abi), check, &mut stale)?;
    sync(RUST_NR, &rust_numbers(&abi), check, &mut stale)?;
    let problems = check_kernel(&abi)?;
    for p in &problems {
        println!("abi: {}", p);
    }
    for s in &stale {
        println!("abi: {} is out of date, run `cargo xtask abi`", s);
    }
    if problems.is_empty() && stale.is_empty() {
        say(&format!(
            "abi: Hamix ABI {}.{} -- {} core + {} native calls ({} deprecated), kernel and generated files agree",
            abi.version >> 8,
            abi.version & 0xFF,
            abi.core.len(),
            abi.hamix.len(),
            abi.hamix.iter().filter(|c| c.status == "deprecated").count()
        ));
        Ok(())
    } else {
        Err(format!("abi: {} problem(s)", problems.len() + stale.len()))
    }
}
