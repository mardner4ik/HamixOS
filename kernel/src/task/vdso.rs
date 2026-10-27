use crate::arch::paging::{AddressSpace, PAGE_SIZE};

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".section .rodata.hx_vdso,\"a\",@progbits",
    ".balign 16",
    ".global hx_vdso_clock_start",
    "hx_vdso_clock_start:",
    "cmp $0, %edi",
    "jne 1f",
    "mov $1, %r9d",
    "jmp 2f",
    "1:",
    "xor %r9d, %r9d",
    "2:",
    "movabs $0x0000009ffffff000, %r8",
    "mov 8(%r8), %rcx",
    "lfence",
    "rdtsc",
    "shl $32, %rdx",
    "or %rdx, %rax",
    "sub %rcx, %rax",
    "mov 16(%r8), %rcx",
    "test %rcx, %rcx",
    "jz 3f",
    "mov $1000000, %r10",
    "mul %r10",
    "div %rcx",
    "test %r9d, %r9d",
    "jz 4f",
    "mov 24(%r8), %rcx",
    "test %rcx, %rcx",
    "jz 3f",
    "imul $1000000000, %rcx, %rcx",
    "add %rcx, %rax",
    "4:",
    "xor %edx, %edx",
    "mov $1000000000, %rcx",
    "div %rcx",
    "mov %rax, (%rsi)",
    "mov %rdx, 8(%rsi)",
    "xor %eax, %eax",
    "ret",
    "3:",
    "mov $228, %eax",
    "syscall",
    "ret",
    ".global hx_vdso_clock_end",
    "hx_vdso_clock_end:",
    ".global hx_vdso_gettimeofday_start",
    "hx_vdso_gettimeofday_start:",
    "mov %rdi, %r11",
    "mov %rsi, %r9",
    "movabs $0x0000009ffffff000, %r8",
    "mov 8(%r8), %rcx",
    "lfence",
    "rdtsc",
    "shl $32, %rdx",
    "or %rdx, %rax",
    "sub %rcx, %rax",
    "mov 16(%r8), %rcx",
    "test %rcx, %rcx",
    "jz 5f",
    "mov $1000000, %r10",
    "mul %r10",
    "div %rcx",
    "mov 24(%r8), %rcx",
    "test %rcx, %rcx",
    "jz 5f",
    "imul $1000000000, %rcx, %rcx",
    "add %rcx, %rax",
    "xor %edx, %edx",
    "mov $1000000000, %rcx",
    "div %rcx",
    "test %r11, %r11",
    "jz 6f",
    "mov %rax, (%r11)",
    "mov %rdx, %rax",
    "xor %edx, %edx",
    "mov $1000, %rcx",
    "div %rcx",
    "mov %rax, 8(%r11)",
    "6:",
    "test %r9, %r9",
    "jz 7f",
    "movq $0, (%r9)",
    "7:",
    "xor %eax, %eax",
    "ret",
    "5:",
    "mov %r11, %rdi",
    "mov %r9, %rsi",
    "mov $96, %eax",
    "syscall",
    "ret",
    ".global hx_vdso_gettimeofday_end",
    "hx_vdso_gettimeofday_end:",
    ".global hx_vdso_time_start",
    "hx_vdso_time_start:",
    "mov %rdi, %r11",
    "movabs $0x0000009ffffff000, %r8",
    "mov 8(%r8), %rcx",
    "lfence",
    "rdtsc",
    "shl $32, %rdx",
    "or %rdx, %rax",
    "sub %rcx, %rax",
    "mov 16(%r8), %rcx",
    "test %rcx, %rcx",
    "jz 8f",
    "mov $1000000, %r10",
    "mul %r10",
    "div %rcx",
    "mov 24(%r8), %rcx",
    "test %rcx, %rcx",
    "jz 8f",
    "imul $1000000000, %rcx, %rcx",
    "add %rcx, %rax",
    "xor %edx, %edx",
    "mov $1000000000, %rcx",
    "div %rcx",
    "test %r11, %r11",
    "jz 9f",
    "mov %rax, (%r11)",
    "9:",
    "ret",
    "8:",
    "mov %r11, %rdi",
    "mov $201, %eax",
    "syscall",
    "ret",
    ".global hx_vdso_time_end",
    "hx_vdso_time_end:",
    options(att_syntax)
);

#[cfg(target_arch = "x86_64")]
pub const BASE: u64 = 0x0000_009F_FFFF_E000;

#[cfg(target_arch = "x86_64")]
const PAGE: usize = PAGE_SIZE as usize;

#[cfg(target_arch = "x86_64")]
const VVAR: u64 = BASE + PAGE_SIZE;

#[cfg(target_arch = "x86_64")]
const CLOCK_CODE: usize = 0x500;

#[cfg(target_arch = "x86_64")]
const GETTIMEOFDAY_CODE: usize = 0x600;

#[cfg(target_arch = "x86_64")]
const TIME_CODE: usize = 0x700;

#[cfg(target_arch = "x86_64")]
static VVAR_FRAME: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[cfg(target_arch = "x86_64")]
unsafe extern "C" {
    static hx_vdso_clock_start: u8;
    static hx_vdso_clock_end: u8;
    static hx_vdso_gettimeofday_start: u8;
    static hx_vdso_gettimeofday_end: u8;
    static hx_vdso_time_start: u8;
    static hx_vdso_time_end: u8;
}

#[cfg(target_arch = "x86_64")]
fn copy_code(page: &mut [u8; PAGE], offset: usize, start: *const u8, end: *const u8) {
    let len = end as usize - start as usize;
    unsafe { core::ptr::copy_nonoverlapping(start, page.as_mut_ptr().add(offset), len) };
}

#[cfg(target_arch = "x86_64")]
fn shared_vvar() -> Option<usize> {
    use core::sync::atomic::Ordering;
    let existing = VVAR_FRAME.load(Ordering::Acquire);
    if existing != 0 {
        return Some(existing);
    }
    let frame = crate::memory::frame::alloc_zeroed_frame()?;
    let (base, khz) = crate::arch::x86_64::vdso_clock_data();
    unsafe {
        let data = frame as *mut u64;
        data.add(1).write(base);
        data.add(2).write(khz);
        data.add(3).write(crate::drivers::rtc::boot_epoch());
    }
    match VVAR_FRAME.compare_exchange(0, frame, Ordering::Release, Ordering::Acquire) {
        Ok(_) => Some(frame),
        Err(winner) => {
            crate::memory::frame::free_frame(frame);
            Some(winner)
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn put16(page: &mut [u8; PAGE], offset: usize, value: u16) {
    page[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[cfg(target_arch = "x86_64")]
fn put32(page: &mut [u8; PAGE], offset: usize, value: u32) {
    page[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(target_arch = "x86_64")]
fn put64(page: &mut [u8; PAGE], offset: usize, value: u64) {
    page[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(target_arch = "x86_64")]
fn dynamic(page: &mut [u8; PAGE], offset: usize, tag: i64, value: u64) {
    put64(page, offset, tag as u64);
    put64(page, offset + 8, value);
}

#[cfg(target_arch = "x86_64")]
fn symbol(page: &mut [u8; PAGE], offset: usize, name: u32, value: u64, size: u64) {
    put32(page, offset, name);
    page[offset + 4] = 0x12;
    put16(page, offset + 6, 1);
    put64(page, offset + 8, value);
    put64(page, offset + 16, size);
}

#[cfg(target_arch = "x86_64")]
fn image() -> [u8; PAGE] {
    let mut page = [0u8; PAGE];
    page[0..4].copy_from_slice(b"\x7fELF");
    page[4] = 2;
    page[5] = 1;
    page[6] = 1;
    put16(&mut page, 16, 3);
    put16(&mut page, 18, 62);
    put32(&mut page, 20, 1);
    put64(&mut page, 32, 64);
    put16(&mut page, 52, 64);
    put16(&mut page, 54, 56);
    put16(&mut page, 56, 2);
    put16(&mut page, 58, 64);

    put32(&mut page, 64, 1);
    put32(&mut page, 68, 5);
    put64(&mut page, 72, 0);
    put64(&mut page, 80, 0);
    put64(&mut page, 88, 0);
    put64(&mut page, 96, PAGE_SIZE);
    put64(&mut page, 104, PAGE_SIZE);
    put64(&mut page, 112, PAGE_SIZE);

    put32(&mut page, 120, 2);
    put32(&mut page, 124, 4);
    put64(&mut page, 128, 0x200);
    put64(&mut page, 136, 0x200);
    put64(&mut page, 144, 0x200);
    put64(&mut page, 152, 96);
    put64(&mut page, 160, 96);
    put64(&mut page, 168, 8);

    dynamic(&mut page, 0x200, 4, 0x3c0);
    dynamic(&mut page, 0x210, 5, 0x300);
    dynamic(&mut page, 0x220, 10, 54);
    dynamic(&mut page, 0x230, 6, 0x340);
    dynamic(&mut page, 0x240, 11, 24);
    dynamic(&mut page, 0x250, 0, 0);

    let strings = b"\0__vdso_clock_gettime\0__vdso_gettimeofday\0__vdso_time\0";
    page[0x300..0x300 + strings.len()].copy_from_slice(strings);
    let clock_start = &raw const hx_vdso_clock_start;
    let clock_end = &raw const hx_vdso_clock_end;
    let gettimeofday_start = &raw const hx_vdso_gettimeofday_start;
    let gettimeofday_end = &raw const hx_vdso_gettimeofday_end;
    let time_start = &raw const hx_vdso_time_start;
    let time_end = &raw const hx_vdso_time_end;
    let clock_len = clock_end as usize - clock_start as usize;
    let gettimeofday_len = gettimeofday_end as usize - gettimeofday_start as usize;
    let time_len = time_end as usize - time_start as usize;
    copy_code(&mut page, CLOCK_CODE, clock_start, clock_end);
    copy_code(&mut page, GETTIMEOFDAY_CODE, gettimeofday_start, gettimeofday_end);
    copy_code(&mut page, TIME_CODE, time_start, time_end);
    symbol(&mut page, 0x358, 1, CLOCK_CODE as u64, clock_len as u64);
    symbol(&mut page, 0x370, 22, GETTIMEOFDAY_CODE as u64, gettimeofday_len as u64);
    symbol(&mut page, 0x388, 42, TIME_CODE as u64, time_len as u64);

    put32(&mut page, 0x3c0, 1);
    put32(&mut page, 0x3c4, 4);
    put32(&mut page, 0x3c8, 1);
    put32(&mut page, 0x3cc, 0);
    put32(&mut page, 0x3d0, 2);
    put32(&mut page, 0x3d4, 3);
    put32(&mut page, 0x3d8, 0);

    page
}

#[cfg(target_arch = "x86_64")]
pub fn install(aspace: &mut AddressSpace) -> bool {
    if aspace.occupied(BASE) || aspace.occupied(VVAR) || !aspace.alloc_range(BASE, PAGE_SIZE) {
        return false;
    }
    let page = image();
    if !aspace.write_bytes(BASE, &page) {
        return false;
    }
    let Some(vvar) = shared_vvar() else {
        return false;
    };
    if !aspace.map_shared(VVAR, vvar as u64, false) {
        return false;
    }
    aspace.protect_range(BASE, PAGE_SIZE, crate::arch::pte::PROT_READ | crate::arch::pte::PROT_EXEC) == 1
}

#[cfg(not(target_arch = "x86_64"))]
pub fn install(_aspace: &mut AddressSpace) -> bool {
    true
}
