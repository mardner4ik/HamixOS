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
pub static MOUSE_HANDLER: Mutex<Option<fn(u8)>> = Mutex::new(None);

#[repr(C)]
pub struct InterruptStackFrame {
    pub ip: u64,
    pub cs: u64,
    pub flags: u64,
    pub sp: u64,
    pub ss: u64,
}

macro_rules! isr_noerr {
    ($name:ident, $handler:path) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push rbp",
                "mov rbp, rsp",
                "push rax",
                "push rcx",
                "push rdx",
                "push rsi",
                "push rdi",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "sub rsp, 520",
                "and rsp, -16",
                "fxsave64 [rsp]",
                "lea rdi, [rbp + 8]",
                "xor esi, esi",
                "call {handler}",
                "fxrstor64 [rsp]",
                "lea rsp, [rbp - 72]",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rdi",
                "pop rsi",
                "pop rdx",
                "pop rcx",
                "pop rax",
                "pop rbp",
                "iretq",
                handler = sym $handler,
            );
        }
    };
}

macro_rules! isr_err {
    ($name:ident, $handler:path) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push rbp",
                "mov rbp, rsp",
                "push rax",
                "push rcx",
                "push rdx",
                "push rsi",
                "push rdi",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "sub rsp, 520",
                "and rsp, -16",
                "fxsave64 [rsp]",
                "lea rdi, [rbp + 16]",
                "mov rsi, [rbp + 8]",
                "call {handler}",
                "fxrstor64 [rsp]",
                "lea rsp, [rbp - 72]",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rdi",
                "pop rsi",
                "pop rdx",
                "pop rcx",
                "pop rax",
                "pop rbp",
                "add rsp, 8",
                "iretq",
                handler = sym $handler,
            );
        }
    };
}

extern "C" fn divide_by_zero(frame: &InterruptStackFrame, _ec: u64) {
    fault("Division by zero", frame, 0);
}

extern "C" fn debug_exception(frame: &InterruptStackFrame, _ec: u64) {
    fault("Debug exception", frame, 0);
}

extern "C" fn nmi(frame: &InterruptStackFrame, _ec: u64) {
    fault("Non-maskable interrupt", frame, 0);
}

extern "C" fn breakpoint(frame: &InterruptStackFrame, _ec: u64) {
    crate::serial_println!("breakpoint at {:#x}", frame.ip);
}

extern "C" fn overflow(frame: &InterruptStackFrame, _ec: u64) {
    fault("Overflow", frame, 0);
}

extern "C" fn bound_range(frame: &InterruptStackFrame, _ec: u64) {
    fault("Bound range exceeded", frame, 0);
}

extern "C" fn invalid_opcode(frame: &InterruptStackFrame, _ec: u64) {
    fault("Invalid opcode", frame, 0);
}

extern "C" fn device_not_available(frame: &InterruptStackFrame, _ec: u64) {
    fault("Device not available", frame, 0);
}

extern "C" fn double_fault(frame: &InterruptStackFrame, ec: u64) {
    panic!("Double fault at {:#x}, code={:#x}", frame.ip, ec);
}

extern "C" fn invalid_tss(frame: &InterruptStackFrame, ec: u64) {
    fault("Invalid TSS", frame, ec);
}

extern "C" fn segment_not_present(frame: &InterruptStackFrame, ec: u64) {
    fault("Segment not present", frame, ec);
}

extern "C" fn stack_segment_fault(frame: &InterruptStackFrame, ec: u64) {
    fault("Stack segment fault", frame, ec);
}

extern "C" fn general_protection(frame: &InterruptStackFrame, ec: u64) {
    fault("General protection fault", frame, ec);
}

extern "C" fn page_fault(frame: &InterruptStackFrame, ec: u64) {
    let addr: u64;
    unsafe { asm!("mov {}, cr2", out(reg) addr, options(nomem, nostack)) };
    crate::serial_println!(
        "#PF ip={:#x} cr2={:#x} ec={:#x} cs={:#x} rsp={:#x}",
        frame.ip, addr, ec, frame.cs, frame.sp
    );
    if frame.cs & 3 == 3 {
        kill_faulting_process("page fault");
    }
    panic!("Page fault at {:#x} accessing {:#x}, code={:#x}", frame.ip, addr, ec);
}

extern "C" fn fpu_error(frame: &InterruptStackFrame, _ec: u64) {
    fault("x87 FPU error", frame, 0);
}

extern "C" fn alignment_check(frame: &InterruptStackFrame, ec: u64) {
    fault("Alignment check", frame, ec);
}

extern "C" fn machine_check(frame: &InterruptStackFrame, _ec: u64) {
    panic!("Machine check at {:#x}", frame.ip);
}

extern "C" fn simd_fp_exception(frame: &InterruptStackFrame, _ec: u64) {
    fault("SIMD floating point exception", frame, 0);
}

extern "C" fn reserved_exception(frame: &InterruptStackFrame, _ec: u64) {
    fault("Reserved/unhandled exception", frame, 0);
}

fn fault(name: &str, frame: &InterruptStackFrame, ec: u64) {
    crate::serial_println!(
        "{} ip={:#x} ec={:#x} cs={:#x} ss={:#x} rsp={:#x} flags={:#x}",
        name, frame.ip, ec, frame.cs, frame.ss, frame.sp, frame.flags
    );
    if frame.cs & 3 == 3 {
        kill_faulting_process(name);
    }
    panic!("{} at {:#x}, code={:#x}", name, frame.ip, ec);
}

fn kill_faulting_process(name: &str) -> ! {
    crate::drivers::tty::println_colored(
        &alloc::format!("\nsegmentation fault ({})", name),
        crate::drivers::tty::COLOR_ERROR,
    );
    unsafe { asm!("swapgs", options(nomem, nostack)) };
    crate::vt::terminate_current_ring3_with(139);
}

extern "C" fn keyboard_handler(_frame: &InterruptStackFrame, _ec: u64) {
    use crate::arch::x86_64::{inb, outb};
    let scancode = inb(0x60);
    if let Some(handler) = *KEYBOARD_HANDLER.lock() {
        handler(scancode);
    }
    outb(0x20, 0x20);
}

extern "C" fn mouse_handler(_frame: &InterruptStackFrame, _ec: u64) {
    use crate::arch::x86_64::{inb, outb};
    let byte = inb(0x60);
    if let Some(handler) = *MOUSE_HANDLER.lock() {
        handler(byte);
    }
    outb(0xA0, 0x20);
    outb(0x20, 0x20);
}

extern "C" fn spurious_handler(_frame: &InterruptStackFrame, _ec: u64) {
    crate::arch::x86_64::outb(0x20, 0x20);
}

extern "C" fn spurious_slave_handler(_frame: &InterruptStackFrame, _ec: u64) {
    crate::arch::x86_64::outb(0xA0, 0x20);
    crate::arch::x86_64::outb(0x20, 0x20);
}

isr_noerr!(isr_divide_by_zero, divide_by_zero);
isr_noerr!(isr_debug, debug_exception);
isr_noerr!(isr_nmi, nmi);
isr_noerr!(isr_breakpoint, breakpoint);
isr_noerr!(isr_overflow, overflow);
isr_noerr!(isr_bound_range, bound_range);
isr_noerr!(isr_invalid_opcode, invalid_opcode);
isr_noerr!(isr_device_not_available, device_not_available);
isr_err!(isr_double_fault, double_fault);
isr_err!(isr_invalid_tss, invalid_tss);
isr_err!(isr_segment_not_present, segment_not_present);
isr_err!(isr_stack_segment, stack_segment_fault);
isr_err!(isr_general_protection, general_protection);
isr_err!(isr_page_fault, page_fault);
isr_noerr!(isr_fpu_error, fpu_error);
isr_err!(isr_alignment_check, alignment_check);
isr_noerr!(isr_machine_check, machine_check);
isr_noerr!(isr_simd_fp, simd_fp_exception);
isr_noerr!(isr_reserved, reserved_exception);
isr_noerr!(isr_keyboard, keyboard_handler);
isr_noerr!(isr_mouse, mouse_handler);
isr_noerr!(isr_spurious, spurious_handler);
isr_noerr!(isr_spurious_slave, spurious_slave_handler);

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
    outb(0x21, 0xF8);
    outb(0xA1, 0xEF);
}

pub fn init() {
    unsafe {
        let idt_ptr = &raw mut IDT;

        for entry in (*idt_ptr).iter_mut() {
            entry.set_handler(isr_reserved as *const () as u64, 0x8E, 0);
        }

        (*idt_ptr)[0].set_handler(isr_divide_by_zero as *const () as u64, 0x8E, 0);
        (*idt_ptr)[1].set_handler(isr_debug as *const () as u64, 0x8E, 0);
        (*idt_ptr)[2].set_handler(isr_nmi as *const () as u64, 0x8E, 0);
        (*idt_ptr)[3].set_handler(isr_breakpoint as *const () as u64, 0x8E, 0);
        (*idt_ptr)[4].set_handler(isr_overflow as *const () as u64, 0x8E, 0);
        (*idt_ptr)[5].set_handler(isr_bound_range as *const () as u64, 0x8E, 0);
        (*idt_ptr)[6].set_handler(isr_invalid_opcode as *const () as u64, 0x8E, 0);
        (*idt_ptr)[7].set_handler(isr_device_not_available as *const () as u64, 0x8E, 0);
        (*idt_ptr)[8].set_handler(
            isr_double_fault as *const () as u64,
            0x8E,
            crate::arch::x86_64::gdt::DOUBLE_FAULT_IST_INDEX,
        );
        (*idt_ptr)[10].set_handler(isr_invalid_tss as *const () as u64, 0x8E, 0);
        (*idt_ptr)[11].set_handler(isr_segment_not_present as *const () as u64, 0x8E, 0);
        (*idt_ptr)[12].set_handler(isr_stack_segment as *const () as u64, 0x8E, 0);
        (*idt_ptr)[13].set_handler(isr_general_protection as *const () as u64, 0x8E, 0);
        (*idt_ptr)[14].set_handler(isr_page_fault as *const () as u64, 0x8E, 0);
        (*idt_ptr)[16].set_handler(isr_fpu_error as *const () as u64, 0x8E, 0);
        (*idt_ptr)[17].set_handler(isr_alignment_check as *const () as u64, 0x8E, 0);
        (*idt_ptr)[18].set_handler(isr_machine_check as *const () as u64, 0x8E, 0);
        (*idt_ptr)[19].set_handler(isr_simd_fp as *const () as u64, 0x8E, 0);

        (*idt_ptr)[32].set_handler(crate::vt::timer_handler_entry(), 0x8E, 0);
        (*idt_ptr)[33].set_handler(isr_keyboard as *const () as u64, 0x8E, 0);
        (*idt_ptr)[44].set_handler(isr_mouse as *const () as u64, 0x8E, 0);

        for i in 34..40usize {
            (*idt_ptr)[i].set_handler(isr_spurious as *const () as u64, 0x8E, 0);
        }
        for i in 40..48usize {
            if i != 44 {
                (*idt_ptr)[i].set_handler(isr_spurious_slave as *const () as u64, 0x8E, 0);
            }
        }

        let ptr = IdtPointer {
            size: (core::mem::size_of_val(&*(&raw const IDT)) - 1) as u16,
            base: &raw const IDT as u64,
        };

        asm!("lidt [{ptr}]", ptr = in(reg) &ptr, options(nostack));
    }

    remap_pic();
}
