use std::fs;
use std::time::Duration;

use crate::image::{Arch, Options, Profile};
use crate::util::*;

const ALPINE: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.22/main/x86_64";
const MUSL_PACKAGES: [&str; 3] = ["musl-1.2.5-r12", "musl-dev-1.2.5-r12", "linux-headers-6.14.2-r0"];

fn musl_root() -> Result {
    let root_dir = root().join("target/musl-root");
    if root_dir.join("usr/lib/libc.a").exists() {
        return Ok(());
    }
    mkdirs(&root_dir)?;
    for pkg in MUSL_PACKAGES {
        let apk = root().join("target").join(format!("{}.apk", pkg));
        if !apk.exists() {
            run(command("curl").args(["-fsSL", "-o"]).arg(&apk).arg(format!("{}/{}.apk", ALPINE, pkg)))?;
        }
        let _ = command("tar").arg("xzf").arg(&apk).arg("-C").arg(&root_dir).stderr(std::process::Stdio::null()).status();
    }
    Ok(())
}

fn results(log: &str, marker: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in log.lines() {
        if line.contains(marker) {
            inside = true;
            continue;
        }
        if inside && line.starts_with('{') {
            out.push(String::from(line.trim()));
            if line.contains("\"done\"") {
                break;
            }
        }
    }
    out
}

fn value(line: &str, key: &str) -> Option<f64> {
    let rest = line.split(&format!("\"{}\":", key)).nth(1)?;
    rest.split(|c| c == ',' || c == '}').next()?.trim().trim_matches('"').parse().ok()
}

fn name(line: &str) -> Option<String> {
    let rest = line.split("\"test\":\"").nth(1)?;
    Some(String::from(rest.split('"').next()?))
}

pub fn kbench(build: bool) -> Result {
    musl_root()?;
    if build {
        crate::image::build(&Options { arch: Arch::X86_64, profile: Profile::Full, desktop: crate::image::Desktop::All, extra: None })?;
    }
    let outcome = crate::qemu::boot_script(Profile::Full, "/usr/lib/hamix/tests/bench-kbench.sh", "1G", Duration::from_secs(900))?;
    let dir = out_dir().join("bench");
    mkdirs(&dir)?;
    fs::write(dir.join("kbench.log"), &outcome.log).map_err(|e| e.to_string())?;
    let hx = results(&outcome.log, "kbench: hxlibc");
    let musl = results(&outcome.log, "kbench: musl");
    let mut json = String::from("{\n  \"hamix_hxlibc\": [\n");
    json.push_str(&hx.iter().map(|l| format!("    {}", l)).collect::<Vec<_>>().join(",\n"));
    json.push_str("\n  ],\n  \"hamix_linux_musl\": [\n");
    json.push_str(&musl.iter().map(|l| format!("    {}", l)).collect::<Vec<_>>().join(",\n"));
    json.push_str("\n  ]\n}\n");
    fs::write(dir.join("kbench.json"), &json).map_err(|e| e.to_string())?;
    println!("{:<24} {:>14} {:>14}", "test", "hxlibc", "linux/musl");
    for line in &hx {
        let Some(test) = name(line) else {
            continue;
        };
        let other = musl.iter().find(|l| name(l).as_deref() == Some(test.as_str()));
        let fmt = |l: &str| value(l, "us_per_op").map(|v| format!("{:.3} us", v)).or_else(|| value(l, "mib_per_s").map(|v| format!("{:.0} MiB/s", v))).unwrap_or_default();
        println!("{:<24} {:>14} {:>14}", test, fmt(line), other.map(|l| fmt(l)).unwrap_or_default());
    }
    if hx.is_empty() {
        return Err(format!("no kbench results, see {}", dir.join("kbench.log").display()));
    }
    say(&format!("kbench: results in {}", dir.join("kbench.json").display()));
    Ok(())
}

pub fn membench(build: bool) -> Result {
    if build {
        crate::image::build(&Options { arch: Arch::X86_64, profile: Profile::Full, desktop: crate::image::Desktop::All, extra: None })?;
    }
    let outcome = crate::qemu::boot_script(Profile::Full, "/usr/lib/hamix/tests/bench-membench.sh", "1G", Duration::from_secs(300))?;
    let dir = out_dir().join("bench");
    mkdirs(&dir)?;
    let desktop = fs::read_to_string(out_dir().join("rootfs/etc/hamix/desktop")).map(|d| String::from(d.trim())).unwrap_or_else(|_| String::from("all"));
    let iso = fs::metadata(crate::qemu::iso_path(Profile::Full)).map(|m| m.len() / 1024).unwrap_or(0);
    fs::write(dir.join("membench.log"), &outcome.log).map_err(|e| e.to_string())?;
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for line in outcome.log.lines() {
        if let Some(stage) = line.strip_prefix("membench: ") {
            let stage = String::from(stage.trim());
            if sections.last().map(|(s, _)| s != &stage).unwrap_or(true) {
                sections.push((stage, Vec::new()));
            }
        } else if let Some((_, lines)) = sections.last_mut() {
            let key = line.split(':').next().unwrap_or("");
            if ["MemTotal", "MemUsed", "FileCache", "KernelHeap", "Vmalloc"].contains(&key) && !lines.iter().any(|l: &String| l.starts_with(key)) {
                lines.push(String::from(line.trim()));
            }
        }
    }
    let kib = |lines: &[String], key: &str| lines.iter().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let mut json = format!("{{\n  \"desktop\": \"{}\",\n  \"iso_kib\": {},\n", desktop, iso);
    let mut rows = Vec::new();
    for (stage, lines) in &sections {
        if lines.is_empty() {
            continue;
        }
        let used = kib(lines, "MemUsed").saturating_sub(kib(lines, "FileCache"));
        rows.push(format!(
            "  \"{}\": {{\"used_kib\": {}, \"kernel_heap_kib\": {}, \"vmalloc_kib\": {}, \"file_cache_kib\": {}}}",
            stage,
            used,
            kib(lines, "KernelHeap"),
            kib(lines, "Vmalloc"),
            kib(lines, "FileCache")
        ));
        println!("{:<20} used {:>5} MiB  kernel heap {:>4} MiB  vmalloc {:>4} MiB  page cache {:>4} MiB", stage, used / 1024, kib(lines, "KernelHeap") / 1024, kib(lines, "Vmalloc") / 1024, kib(lines, "FileCache") / 1024);
    }
    json.push_str(&rows.join(",\n"));
    json.push_str("\n}\n");
    fs::write(dir.join("membench.json"), &json).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("membench-{}.json", desktop)), &json).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Err(format!("no membench results, see {}", dir.join("membench.log").display()));
    }
    say(&format!("membench: results in {}", dir.join("membench.json").display()));
    Ok(())
}

fn print_profile(functions: &mut Vec<(String, u64)>) {
    functions.sort_by(|a, b| b.1.cmp(&a.1));
    let total: u64 = functions.iter().map(|(_, c)| c).sum::<u64>().max(1);
    for (name, count) in functions.iter().take(30) {
        println!("{:>6} {:>5.1}%  {}", count, *count as f64 * 100.0 / total as f64, name.chars().take(110).collect::<String>());
    }
    functions.clear();
}

pub fn symbolize(path: &str) -> Result {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {}", path, e))?;
    let kernel = Arch::X86_64.kernel_bin();
    let mut functions: Vec<(String, u64)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("prof: ") {
            print_profile(&mut functions);
            println!("== {}", rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("hxprof ") {
            println!("hxprof {}", rest);
            continue;
        }
        let mut f = line.split_whitespace();
        if f.next() != Some("k") {
            continue;
        }
        let (Some(ip), Some(count)) = (f.next(), f.next().and_then(|c| c.parse::<u64>().ok())) else {
            continue;
        };
        let name = output(command("llvm-symbolizer").arg(format!("--obj={}", kernel.display())).arg("-C").arg(ip)).unwrap_or_default();
        let name = String::from(name.lines().next().unwrap_or(ip));
        match functions.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 += count,
            None => functions.push((name, count)),
        }
    }
    print_profile(&mut functions);
    Ok(())
}
