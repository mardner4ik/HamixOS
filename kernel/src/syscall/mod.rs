pub fn init() {
    use crate::arch::x86_64::{write_msr, read_msr};
    const IA32_EFER: u32 = 0xC0000080;
    const IA32_STAR: u32 = 0xC0000081;
    const IA32_LSTAR: u32 = 0xC0000082;
    const IA32_FMASK: u32 = 0xC0000084;

    let efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer | 1);

    write_msr(IA32_STAR, (0x0008u64 << 32) | (0x0018u64 << 48));
    write_msr(IA32_LSTAR, syscall_entry as *const () as u64);
    write_msr(IA32_FMASK, 0x200);
}

extern "C" fn syscall_entry() {
}
