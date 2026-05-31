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

    fn set_handler(&mut self, handler: u64, flags: u8) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector = 0x08;
        self.ist = 0;
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

macro_rules! make_isr {
    ($name:ident, $num:expr, $body:block) => {
        unsafe extern "x86-interrupt" fn $name(_frame: InterruptStackFrame) $body
    };
}

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

unsafe extern "x86-interrupt" fn double_fault(frame: InterruptStackFrame, _ec: u64) -> ! {
    panic!("Double fault at {:#x}", frame.ip);
}

unsafe extern "x86-interrupt" fn general_protection(frame: InterruptStackFrame, ec: u64) {
    panic!("General protection fault at {:#x}, code={:#x}", frame.ip, ec);
}

unsafe extern "x86-interrupt" fn page_fault(frame: InterruptStackFrame, ec: u64) {
    let addr: u64;
    asm!("mov {}, cr2", out(reg) addr, options(nomem, nostack));
    panic!("Page fault at {:#x} accessing {:#x}, code={:#x}", frame.ip, addr, ec);
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
        IDT[0].set_handler(divide_by_zero as u64, 0x8E);
        IDT[8].set_handler(double_fault as u64, 0x8E);
        IDT[13].set_handler(general_protection as u64, 0x8E);
        IDT[14].set_handler(page_fault as u64, 0x8E);
        IDT[32].set_handler(timer_handler as u64, 0x8E);
        IDT[33].set_handler(keyboard_handler as u64, 0x8E);
        IDT[39].set_handler(spurious_handler as u64, 0x8E);

        for i in 34..39usize {
            IDT[i].set_handler(spurious_handler as u64, 0x8E);
        }
        for i in 40..48usize {
            IDT[i].set_handler(spurious_handler as u64, 0x8E);
        }

        let ptr = IdtPointer {
            size: (core::mem::size_of_val(&IDT) - 1) as u16,
            base: IDT.as_ptr() as u64,
        };

        asm!("lidt [{ptr}]", ptr = in(reg) &ptr, options(nostack));
    }

    remap_pic();

    use crate::arch::x86_64::enable_interrupts;
    enable_interrupts();
}
