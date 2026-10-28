use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::drivers::video::text_mode::TEXT_CONSOLE;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    pub fn word(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skip => "skip",
        }
    }

    fn badge(&self) -> &'static str {
        match self {
            Status::Ok => "[  \x1b[92mOK\x1b[0m  ]",
            Status::Warn => "[ \x1b[93mWARN\x1b[0m ]",
            Status::Fail => "[\x1b[91mFAILED\x1b[0m]",
            Status::Skip => "[ \x1b[90mSKIP\x1b[0m ]",
        }
    }
}

#[derive(Clone)]
pub struct Unit {
    pub name: &'static str,
    pub title: &'static str,
    pub status: Status,
    pub detail: String,
    pub millis: u64,
}

pub type Outcome = (Status, String);

static UNITS: Mutex<Vec<Unit>> = Mutex::new(Vec::new());
static CONSOLE_READY: Mutex<bool> = Mutex::new(false);
static RUNNING: Mutex<Option<&'static str>> = Mutex::new(None);
static DETAIL: Mutex<String> = Mutex::new(String::new());
static STARTED_MS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static WARNED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

const SLOW_MS: u64 = 10_000;

pub fn watch() {
    use core::sync::atomic::Ordering;
    let Some(running) = RUNNING.try_lock() else {
        return;
    };
    let Some(title) = *running else {
        return;
    };
    drop(running);
    let spent = crate::task::uptime_ms().saturating_sub(STARTED_MS.load(Ordering::Relaxed));
    if spent < SLOW_MS || WARNED.swap(true, Ordering::Relaxed) {
        return;
    }
    let detail = DETAIL.try_lock().map(|d| d.clone()).unwrap_or_default();
    let hint = match detail.strip_prefix("starting ") {
        Some(module) => format!("the {} driver does not answer: reboot, pick \"safe graphics, no driver modules\" in the boot menu or add blacklist={} to the kernel command line", module, module),
        None => String::from("reboot and pick \"safe graphics, no driver modules\" in the boot menu"),
    };
    let line = format!("\r\x1b[K[ \x1b[93mSLOW\x1b[0m ] {} has been running for {} s{}\n         {}\n", title, spent / 1000, if detail.is_empty() { String::new() } else { format!(" ({})", detail) }, hint);
    crate::serial_println!("hxinit: slow {} {} ms {}", title, spent, detail);
    if let Some(mut console) = TEXT_CONSOLE.try_lock() {
        console.write_bytes_to(0, line.as_bytes());
    }
}

pub fn ok(detail: impl Into<String>) -> Outcome {
    (Status::Ok, detail.into())
}

pub fn warn(detail: impl Into<String>) -> Outcome {
    (Status::Warn, detail.into())
}

pub fn fail(detail: impl Into<String>) -> Outcome {
    (Status::Fail, detail.into())
}

pub fn skip(detail: impl Into<String>) -> Outcome {
    (Status::Skip, detail.into())
}

const PENDING: &str = "[ \x1b[96m....\x1b[0m ]";

fn print(unit: &Unit) {
    let line = if unit.detail.is_empty() {
        format!("\r\x1b[K{} {}\n", unit.status.badge(), unit.title)
    } else {
        format!("\r\x1b[K{} {} \x1b[90m-\x1b[0m {}\n", unit.status.badge(), unit.title, unit.detail)
    };
    TEXT_CONSOLE.lock().write_bytes_to(0, line.as_bytes());
}

fn store(unit: Unit) {
    let line = format!("hxinit: {} {} ({}) {}", unit.status.word(), unit.name, unit.millis, unit.detail);
    if *CONSOLE_READY.lock() && crate::drivers::video::text_mode::serial_mirror() {
        crate::drivers::klog::record(&line);
    } else {
        crate::drivers::klog::log(&line);
    }
    if *CONSOLE_READY.lock() {
        print(&unit);
    }
    without_interrupts(|| UNITS.lock().push(unit));
}

pub fn record(name: &'static str, title: &'static str, outcome: Outcome) {
    store(Unit { name, title, status: outcome.0, detail: outcome.1, millis: 0 });
}

fn announce(title: &'static str, detail: &str) {
    if !*CONSOLE_READY.lock() {
        return;
    }
    let line = if detail.is_empty() { format!("\r\x1b[K{} {}", PENDING, title) } else { format!("\r\x1b[K{} {} \x1b[90m-\x1b[0m {}", PENDING, title, detail) };
    TEXT_CONSOLE.lock().write_bytes_to(0, line.as_bytes());
}

pub fn progress(detail: &str) {
    let title = *RUNNING.lock();
    if let Some(title) = title {
        crate::serial_println!("hxinit: progress {}", detail);
        *DETAIL.lock() = String::from(detail);
        announce(title, detail);
    }
}

pub fn run(name: &'static str, title: &'static str, f: impl FnOnce() -> Outcome) -> Status {
    let started = crate::task::uptime_ms();
    crate::serial_println!("hxinit: start {}", name);
    STARTED_MS.store(started, core::sync::atomic::Ordering::Relaxed);
    WARNED.store(false, core::sync::atomic::Ordering::Relaxed);
    DETAIL.lock().clear();
    *RUNNING.lock() = Some(title);
    announce(title, "");
    let (status, detail) = f();
    *RUNNING.lock() = None;
    let millis = crate::task::uptime_ms().saturating_sub(started);
    store(Unit { name, title, status, detail, millis });
    status
}

pub fn run_quiet(name: &'static str, title: &'static str, f: impl FnOnce() -> Outcome) -> Status {
    run(name, title, || without_interrupts(f))
}

pub fn console_ready() {
    crate::prof::init();
    if crate::memory::cmdline_value("console").as_deref() == Some("ttyS0") {
        crate::drivers::video::text_mode::set_serial_mirror(true);
    }
    *CONSOLE_READY.lock() = true;
    TEXT_CONSOLE.lock().write_bytes_to(0, b"\x1b[0m\x0C\x1b[1;36mhxinit\x1b[0m \x1b[90mHamixOS system initialisation\x1b[0m\n\n");
    let units = without_interrupts(|| UNITS.lock().clone());
    for unit in units.iter() {
        print(unit);
    }
}

pub fn units() -> Vec<Unit> {
    without_interrupts(|| UNITS.lock().clone())
}

pub fn status_text() -> String {
    let mut out = String::new();
    for unit in units() {
        out.push_str(&format!("{}\t{}\t{}\t{}\t{}\n", unit.name, unit.status.word(), unit.millis, unit.title, unit.detail));
    }
    out
}

pub fn summary() -> (usize, usize, usize) {
    let units = units();
    let failed = units.iter().filter(|u| u.status == Status::Fail).count();
    let warned = units.iter().filter(|u| u.status == Status::Warn).count();
    (units.len(), warned, failed)
}

extern "C" fn main_thread(_: u64) -> ! {
    use crate::{drivers, fs, task};

    console_ready();

    run("rtc", "Real-time clock", || {
        drivers::rtc::init();
        ok(drivers::rtc::format_datetime(drivers::rtc::now()))
    });
    #[cfg(target_arch = "x86_64")]
    run("acpi", "ACPI tables", || {
        if crate::arch::x86_64::acpi::init() {
            let info = crate::arch::x86_64::acpi::info().unwrap_or_default();
            let ioapics = crate::arch::x86_64::ioapic::summary();
            let pins: u32 = ioapics.iter().map(|(_, _, pins)| pins).sum();
            let routing = if ioapics.is_empty() { String::from("no IOAPIC, PIC only") } else { format!("{} IOAPIC, {} GSIs", ioapics.len(), pins) };
            ok(format!("{} rev {}, {}, {}", info.oem, info.revision, info.tables.join(" "), routing))
        } else {
            warn("no RSDP, using legacy power control")
        }
    });
    #[cfg(not(target_arch = "x86_64"))]
    run("devicetree", "Device tree", || match crate::fdt::current() {
        Some(fdt) => ok(format!("{}, {} bytes", fdt.root().and_then(|r| r.str_property("model")).unwrap_or("unknown machine"), fdt.total_size())),
        None => fail("no device tree"),
    });
    run("pci", "PCI bus", || {
        let devices = drivers::pci::devices();
        drivers::virtio::probe_all();
        ok(format!("{} functions", devices.len()))
    });
    if let Some(line) = drivers::virtio::gpu::init() {
        record("virtio-gpu", "Virtio display", ok(line));
    }
    run_quiet("graphics", "Graphics", || {
        drivers::video::modes::init();
        match drivers::video::resolution() {
            Some((w, h)) => ok(format!("{}, {}x{}, {}", drivers::video::sysfs::display_name(), w, h, drivers::video::modes::backend_name())),
            None => warn("no linear framebuffer, text console only"),
        }
    });
    run("vfs", "Virtual filesystem", || {
        fs::init();
        ok("")
    });
    run("initfs", "Boot drivers (initfs)", || {
        let Some((label, pack)) = ["initfs", "kmods"].iter().find_map(|label| crate::memory::find_module(label).map(|m| (*label, m))) else {
            return skip("no initfs from the boot loader");
        };
        let data = unsafe { core::slice::from_raw_parts(pack.start as usize as *const u8, pack.len()) };
        crate::module::kpi::scan_windows();
        let (count, names) = crate::module::load_boot_pack(data);
        crate::memory::release_module(label);
        if crate::module::disabled() {
            skip("disabled by 'nomodules' on the kernel command line")
        } else if count == 0 {
            skip("no early module matches this hardware")
        } else {
            ok(format!("{} ({} KiB)", names.join(", "), crate::module::resident_bytes() / 1024))
        }
    });
    run("storage", "Storage controllers", || {
        drivers::block::init();
        let disks = drivers::block::disks();
        if disks.is_empty() {
            warn("no disks")
        } else {
            ok(disks.iter().map(|d| format!("{} {} ({})", d.name, d.model.trim(), d.bus)).collect::<Vec<_>>().join(", "))
        }
    });
    run("rootfs", "Root filesystem", || match fs::hextfs::mount_root() {
        Some(description) => ok(String::from(description.trim_start_matches("hext: "))),
        None => match crate::memory::find_module("initramfs") {
            Some(module) => {
                fs::load_initramfs(module.start as usize, module.len());
                warn(format!("initramfs {} KiB (no hext volume)", module.len() / 1024))
            }
            None => {
                let reason = root_failure_reason();
                *ROOT_FAILURE.lock() = Some(reason.clone());
                fail(format!("no root filesystem: {}", reason))
            }
        },
    });
    run("devices", "Device nodes", || {
        fs::create_volatile_nodes();
        ok("/dev /proc")
    });
    run("linux", "Linux compatibility layer", || match crate::linux::init() {
        (true, detail) => ok(detail),
        (false, reason) => skip(format!("off ({})", reason)),
    });
    if crate::arch::io::PORTS {
        run_quiet("keyboard", "PS/2 keyboard", || {
            drivers::input::keyboard::init();
            ok("")
        });
        run_quiet("mouse", "Pointing device", || {
            drivers::input::mouse::init();
            drivers::video::console_mouse::enable();
            if drivers::input::mouse::present() { ok("PS/2") } else { skip("no PS/2 mouse") }
        });
    }
    run("vinput", "Virtio input devices", || {
        let names = drivers::virtio::input::init();
        if names.is_empty() {
            skip("none")
        } else {
            drivers::video::console_mouse::enable();
            ok(names.join(", "))
        }
    });
    run("usb", "USB host controllers", || {
        drivers::usb::init();
        let controllers = drivers::usb::controllers();
        if controllers.is_empty() {
            skip("none")
        } else {
            let devices = drivers::usb::devices();
            ok(format!("{} controller(s), {} device(s)", controllers.len(), devices.len()))
        }
    });
    run("modules", "Loadable driver modules", || {
        crate::module::kpi::scan_windows();
        let loaded = crate::module::autoload();
        let failed = crate::module::failures();
        let failed_text = failed.iter().map(|f| format!("{} failed: {}", f.name, f.reason)).collect::<Vec<_>>().join("; ");
        if crate::module::disabled() {
            skip("disabled by 'nomodules' on the kernel command line")
        } else if loaded == 0 && failed.is_empty() {
            skip("no module in /lib/modules matches this hardware")
        } else if !failed.is_empty() {
            warn(format!("{} loaded; {}", loaded, failed_text))
        } else {
            ok(format!("{} loaded, {} KiB of the {} MiB budget", loaded, crate::module::resident_bytes() / 1024, crate::module::BUDGET_BYTES / (1024 * 1024)))
        }
    });
    run("sysfs", "Device attributes", || {
        drivers::video::sysfs::populate();
        crate::module::publish_sysfs();
        fs::refresh_block_nodes();
        ok(format!("{} PCI function(s) in /sys/bus/pci/devices", drivers::pci::devices().len()))
    });
    run("gpu", "Display acceleration", || {
        use drivers::video::gpu;
        if !gpu::active() {
            return match crate::module::display_failure() {
                Some(reason) => warn(format!("no accelerated display provider ({})", reason)),
                None => skip("no accelerated display provider"),
            };
        }
        gpu::start_daemon();
        ok(format!("{} ({})", gpu::name(), gpu::cap_names(gpu::caps()).join(", ")))
    });
    run("registry", "Command registry", || {
        task::registry::load();
        ok("")
    });
    run("hextd", "Filesystem sync service", || {
        fs::hextfs::start_daemon();
        ok("")
    });
    run("usbd", "USB hotplug service", || {
        drivers::usb::start_daemon();
        ok("")
    });
    run("smp", "Processors", || {
        let cpus = crate::arch::smp::start_aps(task::TICK_HZ);
        drivers::video::sysfs::populate_cpus(cpus.max(1));
        let brand = crate::arch::platform::cpu_brand();
        if cpus > 1 { ok(format!("{} cores online, {}", cpus, brand.trim())) } else { ok(format!("1 core, {}", brand.trim())) }
    });
    run("display", "Display mode", || match drivers::video::modes::apply_saved() {
        None => skip("no saved mode in /etc/hamix/display.conf"),
        Some(Ok(mode)) => ok(mode),
        Some(Err(e)) => warn(e),
    });
    crate::net::init_units();
    crate::drivers::audio::init_unit();
    let (total, warned, failed) = summary();
    let line = format!(
        "\n\x1b[1;36mhxinit\x1b[0m {} units, \x1b[93m{} warning{}\x1b[0m, \x1b[91m{} failed\x1b[0m, {} ms\n\n",
        total,
        warned,
        if warned == 1 { "" } else { "s" },
        failed,
        task::uptime_ms()
    );
    TEXT_CONSOLE.lock().write_bytes_to(0, line.as_bytes());
    if crate::memory::cmdline_value("hxpanic").as_deref() == Some("boot") {
        panic!("test panic requested with hxpanic=boot on the kernel command line");
    }
    explain_root_failure();
    run("init", "Init (PID 1)", start_init);
    loop {
        task::sleep_ticks(task::TICK_HZ);
        drivers::video::modes::tick();
    }
}

fn start_init() -> Outcome {
    let exists = |p: &str| {
        let guard = crate::fs::VFS.lock();
        guard.as_ref().and_then(|v| v.resolve(0, p).map(|id| !v.is_dir(id))).unwrap_or(false)
    };
    let path = crate::memory::cmdline_value("init").unwrap_or_else(|| String::from(DEFAULT_INITS.iter().find(|p| exists(p)).copied().unwrap_or(DEFAULT_INITS[0])));
    let present = path != "kernel" && exists(&path);
    if !present {
        crate::tty::vt::start_shell(0);
        return if path == "kernel" { ok("emergency console on tty1 (init=kernel)") } else { warn(format!("{} not found, emergency console on tty1", path)) };
    }
    match crate::task::elf::spawn_init(&path, &[]) {
        Ok(pid) => ok(format!("{} (pid {})", path, pid)),
        Err(e) => {
            crate::tty::vt::start_shell(0);
            warn(format!("{}: {}, emergency console on tty1", path, e))
        }
    }
}

static ROOT_FAILURE: Mutex<Option<String>> = Mutex::new(None);

fn root_failure_reason() -> String {
    let spec = crate::memory::cmdline_value("root").unwrap_or_else(|| String::from("auto"));
    let image = crate::memory::find_module("hext").is_some();
    let disks = crate::drivers::block::disks();
    let optical: Vec<String> = disks.iter().filter(|d| d.name.starts_with("sr")).map(|d| d.name.clone()).collect();
    let usb: Vec<String> = disks.iter().filter(|d| d.bus == "usb").map(|d| d.name.clone()).collect();
    let usb_hint = match crate::drivers::usb::controllers().iter().filter(|c| c.driven).count() {
        0 => "The kernel drives USB sticks only on USB 2.0 controllers (EHCI) and found none (USB 3.0-only computers need xHCI, not supported yet): choose the boot menu entry \"copy the whole system into RAM\" (the BIOS reads the stick then) or boot from a DVD",
        _ => "The USB stick did not answer the kernel: choose the boot menu entry \"copy the whole system into RAM\" (the BIOS reads the stick then), write the stick again with dd, or boot from a DVD; `hwreport` shows the USB controllers and hubs that were found",
    };
    let listed = if disks.is_empty() { String::from("no disks or CD/DVD drives were found") } else { format!("found {}", disks.iter().map(|d| format!("{} ({}, {})", d.name, d.model.trim(), d.bus)).collect::<Vec<_>>().join(", ")) };
    match spec.as_str() {
        "ram" if !image => format!("the boot loader did not load /boot/hext.img ({}); use the menu entry \"Live\" instead", listed),
        "live" | "cdrom" | "auto" if !image && optical.is_empty() && usb.is_empty() => format!(
            "the boot loader did not load /boot/hext.img and no CD/DVD drive or USB stick was found ({}). {}",
            listed, usb_hint
        ),
        "live" | "cdrom" | "auto" if !image && optical.is_empty() => format!(
            "the boot loader did not load /boot/hext.img and the USB stick {} does not hold it ({}). Checked: {}. The stick may be written with dd, or hold the .iso file on FAT/exFAT (Ventoy) if the file is not fragmented",
            usb.join(", "), listed, crate::fs::hextfs::media_report().join("; ")
        ),
        "live" | "cdrom" if !image => format!("no CD/DVD drive could be read ({}): the drive may be in a mode the kernel does not drive yet; try the menu entry \"copy the whole system into RAM\"", listed),
        "live" | "ram" => format!("/boot/hext.img was loaded but is not a valid HamixOS image ({}); the download or the stick may be damaged", listed),
        other => format!("no disk with a HamixOS filesystem matches root={} ({})", other, listed),
    }
}

fn explain_root_failure() {
    let Some(reason) = ROOT_FAILURE.lock().clone() else {
        return;
    };
    let text = format!(
        "\n\x1b[1;91mThe system could not start: no root filesystem.\x1b[0m\n\x1b[93m{}.\x1b[0m\nPlease take a photo of this screen.\n\n",
        reason
    );
    TEXT_CONSOLE.lock().write_bytes_to(0, text.as_bytes());
}

pub const DEFAULT_INITS: [&str; 2] = ["/usr/libexec/hamix/hxsvc", "/usr/bin/hxsvc"];

pub fn start() {
    crate::task::spawn_kernel_thread_on("hxinit", 0, main_thread, 0, Some(0));
}
