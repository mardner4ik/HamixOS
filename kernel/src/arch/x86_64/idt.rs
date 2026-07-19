use core::arch::asm;
use spin::Mutex;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    flags: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            flags: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    fn set_handler(&mut self, handler: u64, flags: u8, ist: u8) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector = 0x08;
        self.ist = ist;
        self.flags = flags;
        self.zero = 0;
    }
}

#[repr(C, packed)]
struct IdtPointer {
    size: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

pub static KEYBOARD_HANDLER: Mutex<Option<fn(u8)>> = Mutex::new(None);

#[repr(C)]
pub struct InterruptStackFrame {
    pub ip: u64,
    pub cs: u64,
    pub flags: u64,
    pub sp: u64,
    pub ss: u64,
}

unsafe extern "x86-interrupt" fn divide_by_zero(frame: InterruptStackFrame) {
    panic!("Division by zero at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn debug_exception(frame: InterruptStackFrame) {
    panic!("Debug exception at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn nmi(frame: InterruptStackFrame) {
    panic!("Non-maskable interrupt at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn breakpoint(frame: InterruptStackFrame) {
    panic!("Breakpoint at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn overflow(frame: InterruptStackFrame) {
    panic!("Overflow at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn bound_range(frame: InterruptStackFrame) {
    panic!("Bound range exceeded at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn invalid_opcode(frame: InterruptStackFrame) {
    panic!("Invalid opcode at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn device_not_available(frame: InterruptStackFrame) {
    panic!("Device not available at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn double_fault(frame: InterruptStackFrame, _ec: u64) -> ! {
    panic!("Double fault at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn invalid_tss(frame: InterruptStackFrame, ec: u64) {
    panic!("Invalid TSS at {:#x}, code={:#x}", frame.ip, ec);
}

unsafe extern "x86-interrupt" fn segment_not_present(frame: InterruptStackFrame, ec: u64) {
    panic!("Segment not present at {:#x}, code={:#x}", frame.ip, ec);
}

unsafe extern "x86-interrupt" fn stack_segment_fault(frame: InterruptStackFrame, ec: u64) {
    panic!("Stack segment fault at {:#x}, code={:#x}", frame.ip, ec);
}

unsafe extern "x86-interrupt" fn general_protection(frame: InterruptStackFrame, ec: u64) {
    panic!(
        "General protection fault at {:#x}, code={:#x}, cs={:#x}, ss={:#x}, rsp={:#x}, rflags={:#x}",
        frame.ip, ec, frame.cs, frame.ss, frame.sp, frame.flags
    );
}

unsafe extern "x86-interrupt" fn page_fault(frame: InterruptStackFrame, ec: u64) {
    unsafe {
        let addr: u64;
        asm!("mov {}, cr2", out(reg) addr, options(nomem, nostack));
        panic!("Page fault at {:#x} accessing {:#x}, code={:#x}", frame.ip, addr, ec);
    }
}

unsafe extern "x86-interrupt" fn fpu_error(frame: InterruptStackFrame) {
    panic!("x87 FPU error at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn alignment_check(frame: InterruptStackFrame, ec: u64) {
    panic!("Alignment check at {:#x}, code={:#x}", frame.ip, ec);
}

unsafe extern "x86-interrupt" fn machine_check(frame: InterruptStackFrame) -> ! {
    panic!("Machine check at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn simd_fp_exception(frame: InterruptStackFrame) {
    panic!("SIMD floating point exception at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn reserved_exception(frame: InterruptStackFrame) {
    panic!("Reserved/unhandled exception at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    use crate::arch::x86_64::{inb, outb};
    let scancode = inb(0x60);
    if let Some(handler) = *KEYBOARD_HANDLER.lock() {
        handler(scancode);
    }
    outb(0x20, 0x20);
}

unsafe extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
    use crate::arch::x86_64::outb;
    crate::task::tick();
    outb(0x20, 0x20);
}

unsafe extern "x86-interrupt" fn spurious_handler(_frame: InterruptStackFrame) {
    use crate::arch::x86_64::outb;
    outb(0x20, 0x20);
}

fn remap_pic() {
    use crate::arch::x86_64::{outb, io_wait};
    outb(0x20, 0x11); io_wait();
    outb(0xA0, 0x11); io_wait();
    outb(0x21, 0x20); io_wait();
    outb(0xA1, 0x28); io_wait();
    outb(0x21, 0x04); io_wait();
    outb(0xA1, 0x02); io_wait();
    outb(0x21, 0x01); io_wait();
    outb(0xA1, 0x01); io_wait();
    outb(0x21, 0xFC);
    outb(0xA1, 0xFF);
}

pub fn init() {
    unsafe {
        let idt_ptr = &raw mut IDT;

        for entry in (*idt_ptr).iter_mut() {
            entry.set_handler(reserved_exception as *const () as u64, 0x8E, 0);
        }

        (*idt_ptr)[0].set_handler(divide_by_zero as *const () as u64, 0x8E, 0);
        (*idt_ptr)[1].set_handler(debug_exception as *const () as u64, 0x8E, 0);
        (*idt_ptr)[2].set_handler(nmi as *const () as u64, 0x8E, 0);
        (*idt_ptr)[3].set_handler(breakpoint as *const () as u64, 0x8E, 0);
        (*idt_ptr)[4].set_handler(overflow as *const () as u64, 0x8E, 0);
        (*idt_ptr)[5].set_handler(bound_range as *const () as u64, 0x8E, 0);
        (*idt_ptr)[6].set_handler(invalid_opcode as *const () as u64, 0x8E, 0);
        (*idt_ptr)[7].set_handler(device_not_available as *const () as u64, 0x8E, 0);
        (*idt_ptr)[8].set_handler(
            double_fault as *const () as u64,
            0x8E,
            crate::arch::x86_64::gdt::DOUBLE_FAULT_IST_INDEX,
        );
        (*idt_ptr)[10].set_handler(invalid_tss as *const () as u64, 0x8E, 0);
        (*idt_ptr)[11].set_handler(segment_not_present as *const () as u64, 0x8E, 0);
        (*idt_ptr)[12].set_handler(stack_segment_fault as *const () as u64, 0x8E, 0);
        (*idt_ptr)[13].set_handler(general_protection as *const () as u64, 0x8E, 0);
        (*idt_ptr)[14].set_handler(page_fault as *const () as u64, 0x8E, 0);
        (*idt_ptr)[16].set_handler(fpu_error as *const () as u64, 0x8E, 0);
        (*idt_ptr)[17].set_handler(alignment_check as *const () as u64, 0x8E, 0);
        (*idt_ptr)[18].set_handler(machine_check as *const () as u64, 0x8E, 0);
        (*idt_ptr)[19].set_handler(simd_fp_exception as *const () as u64, 0x8E, 0);

        (*idt_ptr)[32].set_handler(timer_handler as *const () as u64, 0x8E, 0);
        (*idt_ptr)[33].set_handler(keyboard_handler as *const () as u64, 0x8E, 0);

        for i in 34..48usize {
            (*idt_ptr)[i].set_handler(spurious_handler as *const () as u64, 0x8E, 0);
        }

        let ptr = IdtPointer {
            size: (core::mem::size_of_val(&*(&raw const IDT)) - 1) as u16,
            base: &raw const IDT as u64,
        };

        asm!("lidt [{ptr}]", ptr = in(reg) &ptr, options(nostack));
    }

    remap_pic();
}
