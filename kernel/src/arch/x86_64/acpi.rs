use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Debug)]
pub struct Processor {
    pub apic_id: u8,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Madt {
    pub lapic_address: u64,
    pub processors: Vec<Processor>,
    pub ioapics: Vec<(u8, u32, u32)>,
}

#[derive(Clone, Debug, Default)]
pub struct AcpiInfo {
    pub revision: u8,
    pub oem: String,
    pub tables: Vec<String>,
    pub madt: Option<Madt>,
    pub pm1a_control: u32,
    pub pm1b_control: u32,
    pub slp_typa: Option<u16>,
    pub smi_command: u32,
    pub acpi_enable: u8,
    pub reset: Option<(u8, u64, u8)>,
}

static INFO: Mutex<Option<AcpiInfo>> = Mutex::new(None);

unsafe fn read_u8(addr: u64) -> u8 {
    unsafe { core::ptr::read_unaligned(addr as *const u8) }
}

unsafe fn read_u16(addr: u64) -> u16 {
    unsafe { core::ptr::read_unaligned(addr as *const u16) }
}

unsafe fn read_u32(addr: u64) -> u32 {
    unsafe { core::ptr::read_unaligned(addr as *const u32) }
}

unsafe fn read_u64(addr: u64) -> u64 {
    unsafe { core::ptr::read_unaligned(addr as *const u64) }
}

fn checksum_ok(addr: u64, len: usize) -> bool {
    let mut sum = 0u8;
    for i in 0..len as u64 {
        sum = sum.wrapping_add(unsafe { read_u8(addr + i) });
    }
    sum == 0
}

fn scan_rsdp() -> Option<[u8; 36]> {
    let ebda = (unsafe { read_u16(0x40E) } as u64) << 4;
    let ranges = [(ebda, ebda + 1024), (0xE0000u64, 0x100000u64)];
    for (start, end) in ranges {
        if start == 0 || end > 0x100000 {
            continue;
        }
        let mut addr = start & !0xF;
        while addr + 20 <= end {
            let sig = unsafe { core::slice::from_raw_parts(addr as *const u8, 8) };
            if sig == b"RSD PTR " && checksum_ok(addr, 20) {
                let mut out = [0u8; 36];
                unsafe { core::ptr::copy_nonoverlapping(addr as *const u8, out.as_mut_ptr(), 36) };
                return Some(out);
            }
            addr += 16;
        }
    }
    None
}

fn signature(addr: u64) -> [u8; 4] {
    let mut sig = [0u8; 4];
    for (i, b) in sig.iter_mut().enumerate() {
        *b = unsafe { read_u8(addr + i as u64) };
    }
    sig
}

fn table_valid(addr: u64) -> bool {
    if addr == 0 || addr >= (1u64 << 32) {
        return false;
    }
    let len = unsafe { read_u32(addr + 4) } as usize;
    (36..(1 << 20)).contains(&len) && checksum_ok(addr, len)
}

fn parse_madt(addr: u64) -> Madt {
    let len = unsafe { read_u32(addr + 4) } as u64;
    let mut madt = Madt { lapic_address: unsafe { read_u32(addr + 36) } as u64, ..Madt::default() };
    let mut off = 44u64;
    while off + 2 <= len {
        let kind = unsafe { read_u8(addr + off) };
        let entry_len = unsafe { read_u8(addr + off + 1) } as u64;
        if entry_len < 2 {
            break;
        }
        match kind {
            0 if entry_len >= 8 => {
                let apic_id = unsafe { read_u8(addr + off + 3) };
                let flags = unsafe { read_u32(addr + off + 4) };
                madt.processors.push(Processor { apic_id, enabled: flags & 1 != 0 });
            }
            1 if entry_len >= 12 => {
                let id = unsafe { read_u8(addr + off + 2) };
                let base = unsafe { read_u32(addr + off + 4) };
                let gsi = unsafe { read_u32(addr + off + 8) };
                madt.ioapics.push((id, base, gsi));
            }
            5 if entry_len >= 12 => {
                madt.lapic_address = unsafe { read_u64(addr + off + 4) };
            }
            _ => {}
        }
        off += entry_len;
    }
    madt
}

fn parse_s5(dsdt: u64) -> Option<u16> {
    if !table_valid(dsdt) {
        return None;
    }
    let len = unsafe { read_u32(dsdt + 4) } as u64;
    let mut i = 36u64;
    while i + 8 < len {
        if signature(dsdt + i) == *b"_S5_" {
            let mut p = dsdt + i + 4;
            if unsafe { read_u8(p) } != 0x12 {
                i += 1;
                continue;
            }
            p += 1;
            let pkg_len_bytes = ((unsafe { read_u8(p) } >> 6) & 3) as u64;
            p += 1 + pkg_len_bytes;
            p += 1;
            let mut value = unsafe { read_u8(p) } as u16;
            if value == 0x0A {
                p += 1;
                value = unsafe { read_u8(p) } as u16;
            }
            return Some(value);
        }
        i += 1;
    }
    None
}

fn parse_fadt(info: &mut AcpiInfo, addr: u64) {
    let len = unsafe { read_u32(addr + 4) };
    info.pm1a_control = unsafe { read_u32(addr + 64) };
    info.pm1b_control = unsafe { read_u32(addr + 68) };
    info.smi_command = unsafe { read_u32(addr + 48) };
    info.acpi_enable = unsafe { read_u8(addr + 52) };
    let mut dsdt = unsafe { read_u32(addr + 40) } as u64;
    if len >= 148 {
        let x_dsdt = unsafe { read_u64(addr + 140) };
        if x_dsdt != 0 && x_dsdt < (1u64 << 32) {
            dsdt = x_dsdt;
        }
    }
    info.slp_typa = parse_s5(dsdt);
    if len >= 129 {
        let flags = unsafe { read_u32(addr + 112) };
        if flags & (1 << 10) != 0 {
            let space = unsafe { read_u8(addr + 116) };
            let address = unsafe { read_u64(addr + 120) };
            let value = unsafe { read_u8(addr + 128) };
            info.reset = Some((space, address, value));
        }
    }
}

pub fn init() -> bool {
    let rsdp = crate::memory::RSDP.lock().or_else(scan_rsdp);
    let Some(rsdp) = rsdp else {
        return false;
    };
    let revision = rsdp[15];
    let oem = String::from_utf8_lossy(&rsdp[9..15]).trim().into();
    let rsdt = u32::from_le_bytes(rsdp[16..20].try_into().unwrap()) as u64;
    let xsdt = if revision >= 2 { u64::from_le_bytes(rsdp[24..32].try_into().unwrap()) } else { 0 };
    let (root, entry_size) = if xsdt != 0 && table_valid(xsdt) { (xsdt, 8) } else if table_valid(rsdt) { (rsdt, 4) } else { return false };
    let mut info = AcpiInfo { revision, oem, ..AcpiInfo::default() };
    let len = unsafe { read_u32(root + 4) } as u64;
    let count = (len - 36) / entry_size;
    for i in 0..count {
        let entry = root + 36 + i * entry_size;
        let table = if entry_size == 8 { unsafe { read_u64(entry) } } else { (unsafe { read_u32(entry) }) as u64 };
        if !table_valid(table) {
            continue;
        }
        let sig = signature(table);
        info.tables.push(String::from_utf8_lossy(&sig).into());
        match &sig {
            b"APIC" => info.madt = Some(parse_madt(table)),
            b"FACP" => parse_fadt(&mut info, table),
            _ => {}
        }
    }
    *INFO.lock() = Some(info);
    true
}

pub fn info() -> Option<AcpiInfo> {
    INFO.lock().clone()
}

pub fn madt() -> Option<Madt> {
    INFO.lock().as_ref().and_then(|i| i.madt.clone())
}

pub fn poweroff() {
    let Some(info) = info() else {
        return;
    };
    let Some(slp_typ) = info.slp_typa else {
        return;
    };
    if info.pm1a_control != 0 && super::inw(info.pm1a_control as u16) & 1 == 0 && info.smi_command != 0 && info.acpi_enable != 0 {
        super::outb(info.smi_command as u16, info.acpi_enable);
        for _ in 0..300 {
            if super::inw(info.pm1a_control as u16) & 1 != 0 {
                break;
            }
            super::pit_delay_us(1000);
        }
    }
    if info.pm1a_control != 0 {
        super::outw(info.pm1a_control as u16, (slp_typ << 10) | (1 << 13));
    }
    if info.pm1b_control != 0 {
        super::outw(info.pm1b_control as u16, (slp_typ << 10) | (1 << 13));
    }
}

pub fn reset() {
    if let Some((space, address, value)) = info().and_then(|i| i.reset) {
        match space {
            1 => super::outb(address as u16, value),
            0 if address < (1u64 << 32) => unsafe { core::ptr::write_volatile(address as *mut u8, value) },
            _ => {}
        }
    }
}
