use alloc::format;
use alloc::string::String;

pub fn reset() {
    super::sbi::system_reset(1);
}

pub fn poweroff() {
    super::sbi::system_reset(0);
}

fn isa() -> String {
    crate::fdt::current()
        .and_then(|f| f.find("/cpus").and_then(|c| c.children().find(|n| n.str_property("device_type") == Some("cpu"))).and_then(|n| n.str_property("riscv,isa").map(String::from)))
        .unwrap_or_else(|| String::from("rv64imac"))
}

pub fn cpu_brand() -> String {
    let name = crate::fdt::current().and_then(|f| f.find("/cpus").and_then(|c| c.children().find(|n| n.str_property("device_type") == Some("cpu"))).and_then(|n| n.strings("compatible").next().map(String::from)));
    let isa = isa();
    let base = isa.split('_').next().unwrap_or("rv64");
    format!("{} {}", name.unwrap_or_else(|| String::from("riscv")), base)
}

pub fn cpuinfo_text() -> String {
    format!("processor\t: 0\nhart\t\t: 0\nisa\t\t: {}\nmmu\t\t: sv39\nmodel name\t: {}\nuarch\t\t: {}\n", isa(), cpu_brand(), cpu_brand())
}

pub fn cycles() -> u64 {
    super::timer::counter()
}

pub fn hw_random() -> Option<u64> {
    None
}
