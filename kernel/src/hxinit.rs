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

fn print(unit: &Unit) {
    let line = if unit.detail.is_empty() {
        format!("{} {}\n", unit.status.badge(), unit.title)
    } else {
        format!("{} {} \x1b[90m-\x1b[0m {}\n", unit.status.badge(), unit.title, unit.detail)
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

pub fn run(name: &'static str, title: &'static str, f: impl FnOnce() -> Outcome) -> Status {
    let started = crate::task::uptime_ms();
    let (status, detail) = f();
    let millis = crate::task::uptime_ms().saturating_sub(started);
    store(Unit { name, title, status, detail, millis });
    status
}

pub fn run_quiet(name: &'static str, title: &'static str, f: impl FnOnce() -> Outcome) -> Status {
    run(name, title, || without_interrupts(f))
}

pub fn console_ready() {
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
    use crate::{drivers, fs, task, users};

    console_ready();

    run("rtc", "Real-time clock", || {
        drivers::rtc::init();
        ok(drivers::rtc::format_datetime(drivers::rtc::now()))
    });
    #[cfg(target_arch = "x86_64")]
    run("acpi", "ACPI tables", || {
        if crate::arch::x86_64::acpi::init() {
            let info = crate::arch::x86_64::acpi::info().unwrap_or_default();
            ok(format!("{} rev {}, {}", info.oem, info.revision, info.tables.join(" ")))
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
    run("kmods", "Early driver modules", || {
        let Some(pack) = crate::memory::find_module("kmods") else {
            return skip("no module pack from the boot loader");
        };
        let data = unsafe { core::slice::from_raw_parts(pack.start as usize as *const u8, pack.len()) };
        crate::module::kpi::scan_windows();
        let (count, names) = crate::module::load_boot_pack(data);
        crate::memory::release_module("kmods");
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
            None => fail("no root filesystem"),
        },
    });
    run("devices", "Device nodes", || {
        fs::create_volatile_nodes();
        ok("/dev /proc")
    });
    run("accounts", "User accounts", || {
        users::init();
        ok("")
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
    run("login", "Login on tty1", || {
        crate::vt::start_shell(0);
        ok("")
    });
    loop {
        task::sleep_ticks(task::TICK_HZ);
        drivers::video::modes::tick();
    }
}

pub fn start() {
    crate::task::spawn_kernel_thread_on("hxinit", 0, main_thread, 0, Some(0));
}
