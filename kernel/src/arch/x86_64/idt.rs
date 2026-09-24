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
    if frame.cs & 3 == 3 {
        kill_faulting_process("breakpoint trap", frame.sp);
    }
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
    let user_mode = frame.cs & 3 == 3;
    let interruptible = user_mode || frame.flags & (1 << 9) != 0;
    if ec & 1 == 0 && interruptible && crate::arch::paging::is_user_range(addr, 1) && crate::task::current_pid() != 0 && crate::task::is_user_process() {
        crate::arch::enable_interrupts();
        let had_lock = crate::task::bkl::held();
        if !had_lock {
            crate::task::bkl::acquire();
        }
        let handled = crate::syscall::linux::base::lazy_fault(addr);
        if !had_lock {
            crate::task::bkl::release();
        }
        crate::arch::disable_interrupts();
        if handled {
            return;
        }
    }
    crate::serial_println!(
        "#PF ip={:#x} cr2={:#x} ec={:#x} cs={:#x} rsp={:#x}",
        frame.ip, addr, ec, frame.cs, frame.sp
    );
    if frame.cs & 3 == 3 {
        kill_faulting_process("page fault", frame.sp);
    }
    if crate::arch::paging::is_user_range(addr, 1) && crate::task::current_pid() != 0 && crate::task::is_user_process() {
        kill_faulting_process("bad user pointer", 0);
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
        kill_faulting_process(name, frame.sp);
    }
    panic!("{} at {:#x}, code={:#x}", name, frame.ip, ec);
}

fn kill_faulting_process(name: &str, sp: u64) -> ! {
    crate::task::bkl::acquire_from_trap();
    let pid = crate::task::current_pid();
    if sp != 0 {
        let words = crate::syscall::user_stack_words(sp, 32);
        let text: alloc::vec::Vec<alloc::string::String> = words.iter().map(|w| alloc::format!("{:x}", w)).collect();
        crate::serial_println!("fault: pid {} stack at {:#x}: {}", pid, sp, text.join(" "));
    }
    let message = alloc::format!("\n\x1b[91msegmentation fault: pid {} ({})\x1b[0m\n", pid, name);
    crate::syscall::emit(2, message.as_bytes());
    let leader = crate::task::current_pid();
    crate::task::with_task(leader, |t| t.exit_signal = 11);
    crate::task::kill(leader, 139);
    crate::task::exit_current(139);
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

extern "C" fn wake_handler(_frame: &InterruptStackFrame, _ec: u64) {
    crate::arch::x86_64::lapic::eoi();
}

extern "C" fn apic_spurious_handler(_frame: &InterruptStackFrame, _ec: u64) {}

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
isr_noerr!(isr_wake, wake_handler);
isr_noerr!(isr_apic_error, wake_handler);
isr_noerr!(isr_apic_spurious, apic_spurious_handler);


macro_rules! device_vectors {
    ($(($entry:ident, $name:ident, $vector:literal)),* $(,)?) => {
        $(
            extern "C" fn $name(_frame: &InterruptStackFrame, _ec: u64) {
                crate::drivers::irq::dispatch($vector);
            }
            isr_noerr!($entry, $name);
        )*
        unsafe fn install_device_vectors(idt: *mut [IdtEntry; 256]) {
            unsafe { $( (*idt)[$vector as usize].set_handler($entry as *const () as u64, 0x8E, 0); )* }
        }
    };
}

device_vectors! {
    (isr_device_22, device_22, 34),
    (isr_device_23, device_23, 35),
    (isr_device_24, device_24, 36),
    (isr_device_25, device_25, 37),
    (isr_device_26, device_26, 38),
    (isr_device_27, device_27, 39),
    (isr_device_28, device_28, 40),
    (isr_device_29, device_29, 41),
    (isr_device_2a, device_2a, 42),
    (isr_device_2b, device_2b, 43),
    (isr_device_2d, device_2d, 45),
    (isr_device_2e, device_2e, 46),
    (isr_device_2f, device_2f, 47),
    (isr_device_50, device_50, 80),
    (isr_device_51, device_51, 81),
    (isr_device_52, device_52, 82),
    (isr_device_53, device_53, 83),
    (isr_device_54, device_54, 84),
    (isr_device_55, device_55, 85),
    (isr_device_56, device_56, 86),
    (isr_device_57, device_57, 87),
    (isr_device_58, device_58, 88),
    (isr_device_59, device_59, 89),
    (isr_device_5a, device_5a, 90),
    (isr_device_5b, device_5b, 91),
    (isr_device_5c, device_5c, 92),
    (isr_device_5d, device_5d, 93),
    (isr_device_5e, device_5e, 94),
    (isr_device_5f, device_5f, 95),
    (isr_device_60, device_60, 96),
    (isr_device_61, device_61, 97),
    (isr_device_62, device_62, 98),
    (isr_device_63, device_63, 99),
    (isr_device_64, device_64, 100),
    (isr_device_65, device_65, 101),
    (isr_device_66, device_66, 102),
    (isr_device_67, device_67, 103),
    (isr_device_68, device_68, 104),
    (isr_device_69, device_69, 105),
    (isr_device_6a, device_6a, 106),
    (isr_device_6b, device_6b, 107),
    (isr_device_6c, device_6c, 108),
    (isr_device_6d, device_6d, 109),
    (isr_device_6e, device_6e, 110),
    (isr_device_6f, device_6f, 111),
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
        (*idt_ptr)[3].set_handler(isr_breakpoint as *const () as u64, 0xEE, 0);
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

        (*idt_ptr)[32].set_handler(crate::task::switch::timer_entry as *const () as u64, 0x8E, 0);
        (*idt_ptr)[33].set_handler(isr_keyboard as *const () as u64, 0x8E, 0);
        (*idt_ptr)[44].set_handler(isr_mouse as *const () as u64, 0x8E, 0);

        install_device_vectors(idt_ptr);

        (*idt_ptr)[crate::arch::x86_64::lapic::TIMER_VECTOR as usize].set_handler(crate::task::switch::lapic_timer_entry as *const () as u64, 0x8E, 0);
        (*idt_ptr)[crate::arch::x86_64::lapic::WAKE_VECTOR as usize].set_handler(isr_wake as *const () as u64, 0x8E, 0);
        (*idt_ptr)[crate::arch::x86_64::lapic::ERROR_VECTOR as usize].set_handler(isr_apic_error as *const () as u64, 0x8E, 0);
        (*idt_ptr)[crate::arch::x86_64::lapic::SPURIOUS_VECTOR as usize].set_handler(isr_apic_spurious as *const () as u64, 0x8E, 0);
    }

    load();
    remap_pic();
}

pub fn load() {
    unsafe {
        let ptr = IdtPointer {
            size: (core::mem::size_of_val(&*(&raw const IDT)) - 1) as u16,
            base: &raw const IDT as u64,
        };
        asm!("lidt [{ptr}]", ptr = in(reg) &ptr, options(nostack));
    }
}
