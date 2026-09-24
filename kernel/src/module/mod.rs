pub mod classes;
pub mod digest;
pub mod elf;
pub mod image;
pub mod kpi;
mod reloc;
pub mod symbols;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use image::Image;

pub const BUDGET_BYTES: usize = 20 * 1024 * 1024;

type Init = extern "C" fn() -> i32;
type Exit = extern "C" fn();

pub const LEVEL_ERR: u32 = 0;
pub const LEVEL_WARN: u32 = 1;
pub const LEVEL_INFO: u32 = 2;
pub const LEVEL_DEBUG: u32 = 3;

pub struct Module {
    pub name: String,
    pub image: Image,
    pub exported: BTreeMap<String, u64>,
    pub exit: Option<Exit>,
    pub suspend: Option<Exit>,
    pub resume: Option<Init>,
    pub version: String,
    pub limit: usize,
    pub params: BTreeMap<String, u64>,
    pub signature: &'static str,
    pub stalled: bool,
    pub suspended: bool,
    pub level: u32,
}

pub struct Info {
    pub name: String,
    pub bytes: usize,
    pub text_bytes: usize,
    pub data_bytes: usize,
    pub devices: usize,
    pub version: String,
    pub limit: usize,
    pub allocated: usize,
    pub signature: &'static str,
    pub stalled: bool,
    pub suspended: bool,
    pub protected: bool,
    pub interrupts: u64,
}

static MODULES: Mutex<Vec<Module>> = Mutex::new(Vec::new());
static MISSING: Mutex<Option<String>> = Mutex::new(None);
static FAILURES: Mutex<Vec<Failure>> = Mutex::new(Vec::new());

#[derive(Clone)]
pub struct Failure {
    pub name: String,
    pub reason: String,
    pub display: bool,
}

pub fn failures() -> Vec<Failure> {
    FAILURES.lock().clone()
}

pub fn display_failure() -> Option<String> {
    FAILURES.lock().iter().find(|f| f.display).map(|f| alloc::format!("{}: {}", f.name, f.reason))
}

fn record_failure(name: &str, reason: String, display: bool) {
    let mut list = FAILURES.lock();
    list.retain(|f| f.name != name);
    list.push(Failure { name: String::from(name), reason, display });
}

fn forget_failure(name: &str) {
    FAILURES.lock().retain(|f| f.name != name);
}

pub(super) fn record_missing(name: String) {
    *MISSING.lock() = Some(name);
}

pub fn last_missing_symbol() -> Option<String> {
    MISSING.lock().clone()
}

pub fn resident_bytes() -> usize {
    MODULES.lock().iter().map(|m| m.image.len()).sum::<usize>() + kpi::allocated_bytes()
}

pub fn budget_left() -> isize {
    BUDGET_BYTES as isize - resident_bytes() as isize
}

pub fn loaded() -> Vec<Info> {
    let claims = kpi::CLAIMS.lock();
    let lines = crate::drivers::irq::lines();
    MODULES
        .lock()
        .iter()
        .map(|m| Info {
            name: m.name.clone(),
            bytes: m.image.len(),
            text_bytes: m.image.text_bytes,
            data_bytes: m.image.data_bytes,
            devices: claims.iter().filter(|c| c.module == m.name).count(),
            version: m.version.clone(),
            limit: m.limit,
            allocated: kpi::allocated_by(&m.name),
            signature: m.signature,
            stalled: m.stalled,
            suspended: m.suspended,
            protected: m.image.protected(),
            interrupts: lines.iter().filter(|(_, owner, _, _)| *owner == m.name).map(|(_, _, count, _)| *count).sum(),
        })
        .collect()
}

pub fn limit_of(name: &str) -> usize {
    MODULES.lock().iter().find(|m| m.name == name).map(|m| m.limit).unwrap_or(BUDGET_BYTES)
}

pub fn level_of(name: &str) -> u32 {
    MODULES.lock().iter().find(|m| m.name == name).map(|m| m.level).unwrap_or(LEVEL_INFO)
}

pub fn param_of(name: &str, key: &str) -> Option<u64> {
    MODULES.lock().iter().find(|m| m.name == name).and_then(|m| m.params.get(key).copied())
}

pub fn mark_stalled(name: &str, spent_ms: u64) {
    let known = {
        let mut modules = MODULES.lock();
        match modules.iter_mut().find(|m| m.name == name) {
            Some(module) if !module.stalled => {
                module.stalled = true;
                true
            }
            _ => false,
        }
    };
    if known {
        crate::drivers::klog::log(&alloc::format!("module {}: a callback took {} ms, the module is being stopped", name, spent_ms));
    }
}

pub fn stalled() -> Vec<String> {
    MODULES.lock().iter().filter(|m| m.stalled).map(|m| m.name.clone()).collect()
}

pub fn drop_stalled() {
    for name in stalled() {
        crate::drivers::irq::release(&name);
        kpi::drop_claims(&name);
        let _ = unload(&name);
    }
}

fn parse_config(name: &str, packed: Option<&str>) -> (usize, BTreeMap<String, u64>, u32) {
    let mut limit = BUDGET_BYTES;
    let mut params = BTreeMap::new();
    let mut level = LEVEL_INFO;
    let path = alloc::format!("/lib/modules/{}.conf", name);
    let Some(text) = packed.map(String::from).or_else(|| read_file(&path).and_then(|b| String::from_utf8(b).ok())) else {
        return (limit, params, level);
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let parsed = if let Some(hex) = value.strip_prefix("0x") { u64::from_str_radix(hex, 16).ok() } else { value.parse::<u64>().ok() };
        match key {
            "limit" => {
                if let Some(v) = parsed {
                    limit = (v as usize).min(BUDGET_BYTES);
                }
            }
            "loglevel" => {
                level = match value {
                    "err" => LEVEL_ERR,
                    "warn" => LEVEL_WARN,
                    "debug" => LEVEL_DEBUG,
                    _ => LEVEL_INFO,
                };
            }
            _ => {
                let value = match parsed {
                    Some(v) => v,
                    None => match value {
                        "on" | "true" | "yes" => 1,
                        "off" | "false" | "no" => 0,
                        _ => continue,
                    },
                };
                params.insert(String::from(key), value);
            }
        }
    }
    (limit, params, level)
}

fn check_signature(name: &str, blob: &[u8], packed: Option<&str>) -> &'static str {
    let path = alloc::format!("/lib/modules/{}.sig", name);
    let Some(text) = packed.map(String::from).or_else(|| read_file(&path).and_then(|b| String::from_utf8(b).ok())) else {
        return "unsigned";
    };
    let wanted = text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    let actual = digest::hex(&digest::sha256(blob));
    if wanted == actual {
        "ok"
    } else {
        crate::drivers::klog::log(&alloc::format!("module {}: signature does not match the file ({}...)", name, &actual[..16]));
        "bad"
    }
}

pub fn is_loaded(name: &str) -> bool {
    MODULES.lock().iter().any(|m| m.name == name)
}

pub fn load(name: &str, blob: &[u8]) -> Result<(), &'static str> {
    load_packed(name, blob, None, None)
}

pub fn load_packed(name: &str, blob: &[u8], conf: Option<&str>, sig: Option<&str>) -> Result<(), &'static str> {
    if is_loaded(name) {
        return Err("module: already loaded");
    }
    *MISSING.lock() = None;
    let signature = check_signature(name, blob, sig);
    let mut loaded = elf::load(blob)?;
    if resident_bytes() + loaded.image.len() > BUDGET_BYTES {
        return Err("module: loading it would exceed the 20 MiB module budget");
    }
    let init = loaded.functions.get("hamix_module_init").copied();
    let exit = loaded.functions.get("hamix_module_exit").copied();
    let suspend = loaded.functions.get("hamix_module_suspend").copied();
    let resume = loaded.functions.get("hamix_module_resume").copied();
    let version = loaded
        .exported
        .get("hamix_module_version")
        .map(|addr| unsafe { version_string(*addr) })
        .unwrap_or_else(|| String::from("0"));
    let (limit, params, level) = parse_config(name, conf);
    loaded.image.protect();
    let module = Module {
        name: name.to_string(),
        image: loaded.image,
        exported: loaded.exported,
        exit: exit.map(|addr| unsafe { core::mem::transmute::<u64, Exit>(addr) }),
        suspend: suspend.map(|addr| unsafe { core::mem::transmute::<u64, Exit>(addr) }),
        resume: resume.map(|addr| unsafe { core::mem::transmute::<u64, Init>(addr) }),
        version,
        limit,
        params,
        signature,
        stalled: false,
        suspended: false,
        level,
    };
    MODULES.lock().push(module);

    kpi::begin(name);
    let status = match init {
        Some(addr) => {
            let init: Init = unsafe { core::mem::transmute(addr) };
            init()
        }
        None => 0,
    };
    kpi::end();
    if status != 0 {
        crate::drivers::irq::release(name);
        kpi::drop_claims(name);
        kpi::forget(name);
        MODULES.lock().retain(|m| m.name != name);
        return Err("module: its init function reported a failure");
    }
    forget_failure(name);
    crate::drivers::klog::log(&alloc::format!("module {} loaded ({} KiB resident)", name, resident_bytes() / 1024));
    Ok(())
}

unsafe fn version_string(addr: u64) -> String {
    let mut out = String::new();
    unsafe {
        let mut at = addr as *const u8;
        for _ in 0..32 {
            let byte = core::ptr::read_volatile(at);
            if byte == 0 || !byte.is_ascii_graphic() {
                break;
            }
            out.push(byte as char);
            at = at.add(1);
        }
    }
    if out.is_empty() {
        String::from("0")
    } else {
        out
    }
}

pub fn suspend_all() -> usize {
    let hooks: Vec<(String, Option<Exit>)> = MODULES.lock().iter().filter(|m| !m.suspended).map(|m| (m.name.clone(), m.suspend)).collect();
    let mut count = 0;
    for (name, hook) in hooks {
        if let Some(hook) = hook {
            kpi::begin(&name);
            hook();
            kpi::end();
        }
        if let Some(module) = MODULES.lock().iter_mut().find(|m| m.name == name) {
            module.suspended = true;
        }
        count += 1;
    }
    count
}

pub fn resume_all() -> usize {
    let hooks: Vec<(String, Option<Init>)> = MODULES.lock().iter().filter(|m| m.suspended).map(|m| (m.name.clone(), m.resume)).collect();
    let mut count = 0;
    for (name, hook) in hooks {
        let ok = match hook {
            Some(hook) => {
                kpi::begin(&name);
                let status = hook();
                kpi::end();
                status == 0
            }
            None => true,
        };
        if let Some(module) = MODULES.lock().iter_mut().find(|m| m.name == name) {
            module.suspended = false;
            module.stalled = module.stalled || !ok;
        }
        count += 1;
    }
    count
}

pub fn unload(name: &str) -> Result<(), &'static str> {
    let exit = {
        let modules = MODULES.lock();
        let module = modules.iter().find(|m| m.name == name).ok_or("module: not loaded")?;
        module.exit
    };
    if let Some(exit) = exit {
        kpi::begin(name);
        exit();
        kpi::end();
    }
    crate::drivers::irq::release(name);
    kpi::drop_claims(name);
    kpi::forget(name);
    classes::forget(name);
    crate::drivers::block::forget_module(name);
    crate::net::module::forget_module(name);
    MODULES.lock().retain(|m| m.name != name);
    crate::drivers::klog::log(&alloc::format!("module {} unloaded ({} KiB resident)", name, resident_bytes() / 1024));
    Ok(())
}

pub fn lookup(name: &str) -> Option<u64> {
    if let Some(addr) = symbols::lookup(name) {
        return Some(addr);
    }
    MODULES.lock().iter().find_map(|m| m.exported.get(name).copied())
}

pub fn publish_sysfs() {
    let entries: Vec<(String, String, usize, usize, &'static str, bool, bool, BTreeMap<String, u64>)> = MODULES
        .lock()
        .iter()
        .map(|m| (m.name.clone(), m.version.clone(), m.limit, m.image.len(), m.signature, m.image.protected(), m.suspended, m.params.clone()))
        .collect();
    let claims = kpi::CLAIMS.lock().iter().map(|c| (c.module.clone(), c.name.clone(), c.handle.vendor, c.handle.device_id)).collect::<Vec<_>>();
    let mut guard = crate::fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let tracking = vfs.tracking;
    vfs.tracking = false;
    let _ = vfs.mkdir_all("/sys/module", 0);
    for (name, version, limit, bytes, signature, protected, suspended, params) in entries {
        let base = alloc::format!("/sys/module/{}", name);
        let _ = vfs.mkdir_all(&base, 0);
        let _ = vfs.write(0, &alloc::format!("{}/version", base), version.as_bytes(), false, 0);
        let _ = vfs.write(0, &alloc::format!("{}/coresize", base), alloc::format!("{}\n", bytes).as_bytes(), false, 0);
        let _ = vfs.write(0, &alloc::format!("{}/limit", base), alloc::format!("{}\n", limit).as_bytes(), false, 0);
        let _ = vfs.write(0, &alloc::format!("{}/signature", base), alloc::format!("{}\n", signature).as_bytes(), false, 0);
        let _ = vfs.write(0, &alloc::format!("{}/protection", base), if protected { b"wx\n".as_slice() } else { b"rwx\n".as_slice() }, false, 0);
        let _ = vfs.write(0, &alloc::format!("{}/state", base), if suspended { b"suspended\n".as_slice() } else { b"live\n".as_slice() }, false, 0);
        let _ = vfs.mkdir_all(&alloc::format!("{}/parameters", base), 0);
        for (key, value) in params {
            let _ = vfs.write(0, &alloc::format!("{}/parameters/{}", base, key), alloc::format!("{}\n", value).as_bytes(), false, 0);
        }
        let mut devices = String::new();
        for (owner, device, vendor, id) in claims.iter() {
            if *owner == name {
                devices.push_str(&alloc::format!("{} {:04x}:{:04x}\n", device, vendor, id));
            }
        }
        let _ = vfs.write(0, &alloc::format!("{}/devices", base), devices.as_bytes(), false, 0);
    }
    vfs.tracking = tracking;
}

pub fn poll_devices() {
    kpi::poll_all();
    crate::drivers::irq::run_work();
    drop_stalled();
}

pub fn claims() -> Vec<(String, u32, String, u16, u16)> {
    kpi::CLAIMS
        .lock()
        .iter()
        .map(|c| (c.module.clone(), c.class, c.name.clone(), c.handle.vendor, c.handle.device_id))
        .collect()
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let mut guard = crate::fs::VFS.lock();
    guard.as_mut().and_then(|v| v.read(0, path).ok())
}

fn list_dir(path: &str) -> Vec<String> {
    let mut guard = crate::fs::VFS.lock();
    match guard.as_mut().and_then(|v| v.list(0, path).ok()) {
        Some(entries) => entries.into_iter().map(|(name, _)| name).collect(),
        None => Vec::new(),
    }
}

pub fn load_from(path: &str) -> Result<(), &'static str> {
    let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".ko").trim_end_matches(".o").to_string();
    let blob = read_file(path).ok_or("module: cannot read the module file")?;
    load(&name, &blob)
}

pub fn disabled() -> bool {
    crate::memory::cmdline().split_whitespace().any(|word| word == "nomodules")
}

fn blacklisted(stem: &str) -> bool {
    crate::memory::cmdline_value("blacklist")
        .map(|list| list.split(',').any(|name| name.trim() == stem))
        .unwrap_or(false)
}

pub fn load_boot_pack(archive: &[u8]) -> (usize, Vec<String>) {
    if disabled() {
        return (0, Vec::new());
    }
    let entries = crate::fs::tar::parse(archive);
    let find = |wanted: &str| entries.iter().find(|e| e.name.trim_start_matches("./") == wanted).map(|e| e.data);
    let devices = crate::drivers::pci::devices();
    let mut loaded = Vec::new();
    for entry in entries.iter() {
        let path = entry.name.trim_start_matches("./");
        let Some(stem) = path.strip_prefix("lib/modules/").and_then(|n| n.strip_suffix(".ko")) else {
            continue;
        };
        if is_loaded(stem) || blacklisted(stem) {
            continue;
        }
        let text = |ext: &str| find(&alloc::format!("lib/modules/{}.{}", stem, ext)).map(|d| String::from_utf8_lossy(d).into_owned());
        let Some(ids) = text("ids") else {
            continue;
        };
        let Some(class) = ids_match(&ids, &devices) else {
            continue;
        };
        let conf = text("conf");
        if !conf.as_deref().map(|c| c.lines().any(|l| l.split('#').next().unwrap_or("").trim().replace(' ', "") == "early=1")).unwrap_or(false) {
            continue;
        }
        let sig = text("sig");
        match load_packed(stem, entry.data, conf.as_deref(), sig.as_deref()) {
            Ok(()) => loaded.push(String::from(stem)),
            Err(e) => {
                let reason = match (last_missing_symbol(), kpi::last_line(stem)) {
                    (Some(symbol), _) => alloc::format!("{} ({})", e, symbol),
                    (None, Some(line)) => line,
                    (None, None) => String::from(e),
                };
                crate::drivers::klog::log(&alloc::format!("module {}: {}", stem, reason));
                record_failure(stem, reason, class == 0x03);
            }
        }
    }
    (loaded.len(), loaded)
}

pub fn autoload() -> usize {
    if disabled() {
        crate::drivers::klog::log("module: autoload skipped, 'nomodules' is on the kernel command line");
        return 0;
    }
    let mut loaded = 0;
    let entries = list_dir("/lib/modules");
    let devices = crate::drivers::pci::devices();
    for entry in entries {
        if !entry.ends_with(".ko") {
            continue;
        }
        let stem = entry.trim_end_matches(".ko");
        if is_loaded(stem) {
            continue;
        }
        if blacklisted(stem) {
            crate::drivers::klog::log(&alloc::format!("module {}: blacklisted on the kernel command line", stem));
            continue;
        }
        let Some(class) = wants(stem, &devices) else {
            continue;
        };
        let path = alloc::format!("/lib/modules/{}", entry);
        match load_from(&path) {
            Ok(()) => loaded += 1,
            Err(e) => {
                let reason = match (last_missing_symbol(), kpi::last_line(stem)) {
                    (Some(symbol), _) => alloc::format!("{} ({})", e, symbol),
                    (None, Some(line)) => String::from(line.strip_prefix(stem).map(|l| l.trim_start_matches(':').trim()).unwrap_or(&line)),
                    (None, None) => String::from(e),
                };
                crate::drivers::klog::log(&alloc::format!("module {}: {}", stem, reason));
                record_failure(stem, reason, class == 0x03);
            }
        }
    }
    loaded
}

fn class_match(spec: &str, devices: &[crate::drivers::pci::PciDevice]) -> Option<u8> {
    let parts: Vec<&str> = spec.split(':').collect();
    let byte = |i: usize| parts.get(i).and_then(|p| if *p == "*" { Some(None) } else { u8::from_str_radix(p, 16).ok().map(Some) });
    let (class, subclass, prog_if) = (byte(0)??, byte(1).unwrap_or(None), byte(2).unwrap_or(None));
    devices
        .iter()
        .find(|d| d.class == class && subclass.map(|s| s == d.subclass).unwrap_or(true) && prog_if.map(|p| p == d.prog_if).unwrap_or(true))
        .map(|d| d.class)
}

pub fn ids_match(text: &str, devices: &[crate::drivers::pci::PciDevice]) -> Option<u8> {
    let published = classes::published();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(spec) = line.strip_prefix("class:") {
            if let Some(class) = class_match(spec.trim(), devices) {
                return Some(class);
            }
            continue;
        }
        if let Some((bus, rest)) = line.split_once(' ') {
            let Some((vendor, device)) = rest.trim().split_once(':') else {
                continue;
            };
            let (Ok(vendor), Ok(device)) = (u16::from_str_radix(vendor.trim(), 16), u16::from_str_radix(device.trim(), 16)) else {
                continue;
            };
            if published.iter().any(|p| classes::bus_name(p.bus) == bus && p.vendor == vendor && p.device == device) {
                return Some(0xFE);
            }
            continue;
        }
        let Some((vendor, device)) = line.split_once(':') else {
            continue;
        };
        let (Ok(vendor), Ok(device)) = (u16::from_str_radix(vendor.trim(), 16), u16::from_str_radix(device.trim(), 16)) else {
            continue;
        };
        if let Some(found) = devices.iter().find(|d| d.vendor == vendor && d.device == device) {
            return Some(found.class);
        }
    }
    None
}

fn wants(stem: &str, devices: &[crate::drivers::pci::PciDevice]) -> Option<u8> {
    let path = alloc::format!("/lib/modules/{}.ids", stem);
    let text = read_file(&path).and_then(|b| String::from_utf8(b).ok())?;
    if let Some(class) = ids_match(&text, devices) {
        return Some(class);
    }
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((vendor, device)) = line.split_once(':') else {
            continue;
        };
        let (Ok(vendor), Ok(device)) = (u16::from_str_radix(vendor.trim(), 16), u16::from_str_radix(device.trim(), 16)) else {
            continue;
        };
        if let Some(found) = devices.iter().find(|d| d.vendor == vendor && d.device == device) {
            return Some(found.class);
        }
    }
    None
}
