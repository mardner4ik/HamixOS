#![no_std]
#![no_main]

#[macro_use]
mod console;
mod acpi;
mod bios;
mod config;
mod disk;
mod elf;
mod iso9660;
mod layout;
mod mb2;
mod memmap;
mod menu;
mod probe;
mod vbe;
mod ventoy;

use core::panic::PanicInfo;

use config::{Config, Entry, Kind};
use disk::Disk;
use iso9660::Volume;

core::arch::global_asm!(include_str!("entry.S"), options(att_syntax));
core::arch::global_asm!(include_str!("mbr.S"), options(att_syntax));
core::arch::global_asm!(include_str!("chain.S"), options(att_syntax));

unsafe extern "C" {
    fn hxboot_enter(entry: u32, info: u32) -> !;
    fn hxboot_chain(drive: u32, partition_entry: u32) -> !;
}

#[unsafe(no_mangle)]
pub extern "C" fn hxboot_main(drive: u32, _file_lba: u32, sector_shift: u32) -> ! {
    console::init();
    console::banner();
    let disk = Disk::new(drive as u8, sector_shift);
    println!("boot drive {:#04x}, {} byte sectors", disk.drive(), disk.sector_size());
    let volume = match Volume::open(&disk) {
        Ok(v) => v,
        Err(e) => fail(e),
    };
    let text = match volume.read_small("/boot/hxboot.cfg", layout::CONFIG, layout::CONFIG_SIZE) {
        Ok(t) => t,
        Err(e) => fail(e),
    };
    let mut config = Config::parse(text);
    if !config.ventoy_param.is_empty() {
        let placed = volume.lookup(config.ventoy_param).and_then(|f| if f.size <= ventoy::INJECT_MAX { volume.read(&f, 0, ventoy::PARAM_ADDRESS as *mut u8, f.size as usize) } else { Err("too large") });
        if let Err(e) = placed {
            println!("ventoy-param {}: {}", config.ventoy_param, e);
        }
    }
    let ventoy = ventoy::find();
    if ventoy.is_some() {
        println!("Ventoy detected: the image will be opened by the kernel directly");
    }
    if config.probe {
        probe::add_other_systems(&mut config, disk.drive());
    }
    if config.count == 0 {
        fail("/boot/hxboot.cfg has no entries");
    }
    let choice = menu::run(&config);
    let entry = config.entries[choice.index];
    console::clear();
    println!("Booting {}", entry.name);
    match entry.kind {
        Kind::Kernel => boot(&volume, &entry, choice.cmdline.unwrap_or(entry.cmdline), ventoy),
        Kind::Chain { drive, signature, partition } => chain(drive, signature, partition),
        Kind::Reboot => reboot(),
    }
}

fn chain(drive: u8, signature: u32, partition: u8) -> ! {
    let drive = if signature != 0 {
        match probe::find_drive(signature) {
            Some(d) => d,
            None => fail("the disk of this entry is not connected"),
        }
    } else {
        drive
    };
    if !probe::read_mbr(drive, layout::MBR_COPY) {
        fail("the disk has no readable boot sector");
    }
    let mut si = 0;
    if partition == 0 {
        unsafe { core::ptr::copy_nonoverlapping(layout::MBR_COPY as *const u8, layout::CHAIN as *mut u8, 512) };
    } else {
        let entry = layout::MBR_COPY + 446 + 16 * (partition as u32 - 1);
        let start = bios::read_u32(entry + 8);
        let disk = Disk::new(drive, 9);
        if start == 0 || disk.read(start as u64 * 512, layout::CHAIN as *mut u8, 512).is_err() || bios::read_u16(layout::CHAIN + 510) != 0xaa55 {
            fail("the partition has no boot sector");
        }
        for p in 0..4 {
            let flag = layout::MBR_COPY + 446 + 16 * p;
            bios::bytes_mut(flag, 1)[0] = if flag == entry { 0x80 } else { 0 };
        }
        si = entry;
    }
    unsafe { hxboot_chain(drive as u32, si) }
}

fn reboot() -> ! {
    bios::outb(0xcf9, 0x02);
    bios::outb(0xcf9, 0x06);
    for _ in 0..100000 {
        if bios::inb(0x64) & 0x02 == 0 {
            break;
        }
    }
    bios::outb(0x64, 0xfe);
    fail("the computer did not restart; switch it off")
}

fn boot(volume: &Volume, entry: &Entry, cmdline: &str, ventoy: Option<ventoy::Found>) -> ! {
    let kernel = match elf::load(volume, entry.kernel) {
        Ok(k) => k,
        Err(e) => fail(e),
    };
    let mut next = align(kernel.end, 4096);
    let mut modules = [mb2::Module::EMPTY; config::MAX_MODULES];
    let mut count = 0;
    for module in &entry.modules[..entry.module_count] {
        let file = match volume.lookup(module.path) {
            Ok(f) => f,
            Err(e) if module.optional => {
                println!("  {} skipped ({})", module.path, e);
                continue;
            }
            Err(e) => fail(e),
        };
        if !memmap::fits(next, file.size) {
            if module.optional {
                println!("  {} skipped (not enough memory)", module.path);
                continue;
            }
            fail("not enough memory for the boot modules");
        }
        println!("  {} ({} KiB)", module.path, file.size / 1024);
        if let Err(e) = volume.read(&file, 0, next as *mut u8, file.size as usize) {
            if module.optional {
                println!("  {} skipped ({})", module.path, e);
                continue;
            }
            fail(e);
        }
        modules[count] = mb2::Module { start: next, end: next + file.size, name: module.name };
        count += 1;
        next = align(next + file.size, 4096);
    }
    let rsdp = acpi::find_rsdp();
    let framebuffer = if entry.text { None } else { vbe::set_best_mode() };
    let map = memmap::read();
    let info = mb2::build(cmdline, &modules[..count], map, framebuffer, rsdp, ventoy);
    unsafe { hxboot_enter(kernel.entry, info) }
}

fn align(value: u32, to: u32) -> u32 {
    (value + to - 1) & !(to - 1)
}

pub fn fail(reason: &str) -> ! {
    console::error(reason);
    loop {
        unsafe { core::arch::asm!("cli", "hlt") };
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("hxboot panic: {}", info);
    fail("internal error")
}
