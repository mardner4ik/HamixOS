use core::arch::asm;

#[repr(C, packed)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl GdtEntry {
    const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_mid: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        }
    }

    const fn new(base: u32, limit: u32, access: u8, gran: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_mid: ((base >> 16) & 0xFF) as u8,
            access,
            granularity: ((limit >> 16) & 0x0F) as u8 | (gran & 0xF0),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }
}

#[repr(C, packed)]
struct GdtPointer {
    size: u16,
    base: u64,
}

static GDT: [GdtEntry; 5] = [
    GdtEntry::null(),
    GdtEntry::new(0, 0xFFFFF, 0x9A, 0xA0),
    GdtEntry::new(0, 0xFFFFF, 0x92, 0xC0),
    GdtEntry::new(0, 0xFFFFF, 0xFA, 0xA0),
    GdtEntry::new(0, 0xFFFFF, 0xF2, 0xC0),
];

pub fn init() {
    let ptr = GdtPointer {
        size: (core::mem::size_of_val(&GDT) - 1) as u16,
        base: GDT.as_ptr() as u64,
    };

    unsafe {
        asm!(
            "lgdt [{ptr}]",
            "push 0x08",
            "lea {tmp}, [rip + 1f]",
            "push {tmp}",
            "retfq",
            "1:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            ptr = in(reg) &ptr,
            tmp = lateout(reg) _,
            options(att_syntax)
        );
    }
}
