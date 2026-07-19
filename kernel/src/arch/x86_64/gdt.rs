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

#[repr(C, packed)]
struct Tss {
    reserved0: u32,
    rsp: [u64; 3],
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    iomap_base: u16,
}

impl Tss {
    const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp: [0; 3],
            reserved1: 0,
            ist: [0; 7],
            reserved2: 0,
            reserved3: 0,
            iomap_base: core::mem::size_of::<Tss>() as u16,
        }
    }
}

#[repr(C)]
struct GdtWithTss {
    entries: [GdtEntry; 6],
    tss_descriptor: [u64; 2],
}

#[allow(dead_code)]
pub const KERNEL_CS: u16 = 0x08;
#[allow(dead_code)]
pub const KERNEL_DS: u16 = 0x10;
#[allow(dead_code)]
pub const USER_CS: u16 = 0x28 | 3;
#[allow(dead_code)]
pub const USER_DS: u16 = 0x20 | 3;
#[allow(dead_code)]
pub const TSS_SEL: u16 = 0x30;

const DF_STACK_SIZE: usize = 4096 * 5;
static mut DOUBLE_FAULT_STACK: [u8; DF_STACK_SIZE] = [0u8; DF_STACK_SIZE];

const KERNEL_STACK_SIZE: usize = 4096 * 5;
static mut KERNEL_STACK: [u8; KERNEL_STACK_SIZE] = [0u8; KERNEL_STACK_SIZE];

static mut TSS: Tss = Tss::new();

static mut GDT: GdtWithTss = GdtWithTss {
    entries: [
        GdtEntry::null(),
        GdtEntry::new(0, 0xFFFFF, 0x9A, 0xA0),
        GdtEntry::new(0, 0xFFFFF, 0x92, 0x80),
        GdtEntry::new(0, 0xFFFFF, 0xFA, 0xC0),
        GdtEntry::new(0, 0xFFFFF, 0xF2, 0x80),
        GdtEntry::new(0, 0xFFFFF, 0xFA, 0xA0),
    ],
    tss_descriptor: [0, 0],
};

pub const DOUBLE_FAULT_IST_INDEX: u8 = 1;

pub fn init() {
    unsafe {
        let df_top = (&raw const DOUBLE_FAULT_STACK) as u64 + DF_STACK_SIZE as u64;
        let kstack_top = (&raw const KERNEL_STACK) as u64 + KERNEL_STACK_SIZE as u64;
        TSS.ist[(DOUBLE_FAULT_IST_INDEX - 1) as usize] = df_top;
        TSS.rsp[0] = kstack_top;

        let base = (&raw const TSS) as u64;
        let limit = (core::mem::size_of::<Tss>() - 1) as u64;
        let access: u64 = 0x89;
        let flags: u64 = 0x0;

        let low = (limit & 0xFFFF)
            | ((base & 0xFFFFFF) << 16)
            | (access << 40)
            | (((limit >> 16) & 0xF) << 48)
            | ((flags & 0xF) << 52)
            | (((base >> 24) & 0xFF) << 56);
        let high = (base >> 32) & 0xFFFFFFFF;

        GDT.tss_descriptor[0] = low;
        GDT.tss_descriptor[1] = high;
    }

    let gdt_base = (&raw const GDT) as u64;
    let gdt_size = core::mem::size_of::<GdtWithTss>() as u64;

    let ptr = GdtPointer {
        size: (gdt_size - 1) as u16,
        base: gdt_base,
    };

    unsafe {
        asm!(
            "lgdt [{ptr}]",
            "push 0x08",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            "mov ax, 0x30",
            "ltr ax",
            ptr = in(reg) &ptr,
            tmp = lateout(reg) _,
        );
    }
}
