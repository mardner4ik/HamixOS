use std::fs;
use std::path::{Path, PathBuf};

use crate::util::*;

const NATIVE_ROOTS: [&str; 7] = ["kernel/src", "userland", "crates", "hxlibc/rust", "sdk", "modules", "data"];
const SKIP_DIRS: [&str; 2] = ["target", "vendor"];
const LINUX_KERNEL_PART: [&str; 1] = ["kernel/src/linux"];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = file_name(&path);
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn rel(path: &Path) -> String {
    path.strip_prefix(root()).unwrap_or(path).display().to_string()
}

struct Finding {
    file: String,
    line: usize,
    rule: &'static str,
    text: String,
}

fn scan_manifest(path: &Path, findings: &mut Vec<Finding>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for (i, line) in text.lines().enumerate() {
        if line.contains("layers/linux") {
            findings.push(Finding { file: rel(path), line: i + 1, rule: "native-depends-on-linux-layer", text: String::from(line.trim()) });
        }
    }
}

fn scan_source(path: &Path, findings: &mut Vec<Finding>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for (i, line) in text.lines().enumerate() {
        if line.contains("linux_bridge") {
            findings.push(Finding { file: rel(path), line: i + 1, rule: "linux-bridge-in-native-code", text: String::from(line.trim()) });
        }
        if line.contains("/opt/linux") && !line.contains("LEGACY_SYSROOT") {
            findings.push(Finding { file: rel(path), line: i + 1, rule: "opt-linux-path", text: String::from(line.trim()) });
        }
    }
}

pub fn lint_layers() -> Result {
    let root = root();
    let mut files = Vec::new();
    for dir in NATIVE_ROOTS {
        walk(&root.join(dir), &mut files);
    }
    let mut findings = Vec::new();
    for file in &files {
        let r = rel(file);
        if LINUX_KERNEL_PART.iter().any(|p| r.starts_with(p)) {
            continue;
        }
        let name = file_name(file);
        if name == "Cargo.toml" || name == "meson.build" {
            scan_manifest(file, &mut findings);
        } else if matches!(file.extension().and_then(|e| e.to_str()), Some("rs") | Some("c") | Some("h") | Some("cpp") | Some("sh") | Some("hsh") | Some("desktop") | Some("conf")) {
            scan_source(file, &mut findings);
        }
    }
    let workspace = fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    for (i, line) in workspace.lines().enumerate() {
        if line.contains("layers/linux") && !line.trim_start().starts_with('"') {
            findings.push(Finding { file: String::from("Cargo.toml"), line: i + 1, rule: "native-depends-on-linux-layer", text: String::from(line.trim()) });
        }
    }
    if findings.is_empty() {
        say(&format!("lint-layers: {} files checked, the native side does not depend on layers/linux", files.len()));
        return Ok(());
    }
    for f in &findings {
        println!("{}:{}: [{}] {}", f.file, f.line, f.rule, f.text);
    }
    Err(format!("lint-layers: {} violation(s) of the one-way layer rule", findings.len()))
}

const UNSAFE_BASELINE: &str = "tools/xtask/unsafe-baseline.txt";

fn unsafe_groups() -> Vec<(String, PathBuf)> {
    let mut groups = Vec::new();
    for dir in subdirs(&root().join("kernel/src")) {
        groups.push((format!("kernel/{}", file_name(&dir)), dir));
    }
    groups.push((String::from("kernel/*.rs"), root().join("kernel/src")));
    for base in ["modules", "crates", "userland/libs", "hxlibc/rust"] {
        for dir in subdirs(&root().join(base)) {
            groups.push((format!("{}/{}", base, file_name(&dir)), dir));
        }
    }
    for role in ["apps", "core", "desktop", "services"] {
        groups.push((format!("userland/{}", role), root().join("userland").join(role)));
    }
    groups
}

fn count_unsafe(dir: &Path, top_only: bool) -> usize {
    let mut files = Vec::new();
    if top_only {
        if let Ok(entries) = fs::read_dir(dir) {
            files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_file()));
        }
    } else {
        walk(dir, &mut files);
    }
    files
        .iter()
        .filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false))
        .filter_map(|p| fs::read_to_string(p).ok())
        .map(|text| {
            text.lines()
                .map(|l| l.split("//").next().unwrap_or(""))
                .map(|l| l.matches("unsafe ").count() + l.matches("unsafe{").count())
                .sum::<usize>()
        })
        .sum()
}

pub fn lint_unsafe(update: bool) -> Result {
    let counts: Vec<(String, usize)> = unsafe_groups().into_iter().map(|(name, dir)| {
        let top = name == "kernel/*.rs";
        (name, count_unsafe(&dir, top))
    }).filter(|(_, n)| *n > 0).collect();
    let baseline_path = root().join(UNSAFE_BASELINE);
    if update || !baseline_path.exists() {
        let text: String = counts.iter().map(|(name, n)| format!("{} {}\n", name, n)).collect();
        write(&baseline_path, &text)?;
        say(&format!("lint-unsafe: baseline written to {} ({} uses of unsafe)", UNSAFE_BASELINE, counts.iter().map(|(_, n)| n).sum::<usize>()));
        return Ok(());
    }
    let baseline: Vec<(String, usize)> = fs::read_to_string(&baseline_path)
        .map_err(|e| e.to_string())?
        .lines()
        .filter_map(|l| l.rsplit_once(' ').and_then(|(name, n)| n.parse().ok().map(|n| (String::from(name), n))))
        .collect();
    let mut grown = Vec::new();
    let mut total = 0;
    for (name, n) in &counts {
        total += n;
        let allowed = baseline.iter().find(|(b, _)| b == name).map(|(_, n)| *n).unwrap_or(0);
        if *n > allowed {
            grown.push(format!("{}: {} (baseline {})", name, n, allowed));
        }
    }
    let mut sorted = counts.clone();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, n) in sorted.iter().take(12) {
        println!("{:>6}  {}", n, name);
    }
    if grown.is_empty() {
        say(&format!("lint-unsafe: {} uses of unsafe, none above the baseline", total));
        Ok(())
    } else {
        Err(format!("lint-unsafe: more unsafe than the baseline allows (justify it in review, then `cargo xtask lint-unsafe --update`):\n    {}", grown.join("\n    ")))
    }
}
