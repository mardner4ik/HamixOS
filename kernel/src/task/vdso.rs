use crate::arch::paging::{AddressSpace, PAGE_SIZE};

#[cfg(target_arch = "x86_64")]
pub const BASE: u64 = 0x0000_009F_FFFF_F000;

#[cfg(target_arch = "x86_64")]
const PAGE: usize = PAGE_SIZE as usize;

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
fn symbol(page: &mut [u8; PAGE], offset: usize, name: u32, value: u64) {
    put32(page, offset, name);
    page[offset + 4] = 0x12;
    put16(page, offset + 6, 1);
    put64(page, offset + 8, value);
    put64(page, offset + 16, 8);
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
    symbol(&mut page, 0x358, 1, 0x500);
    symbol(&mut page, 0x370, 22, 0x510);
    symbol(&mut page, 0x388, 42, 0x520);

    put32(&mut page, 0x3c0, 1);
    put32(&mut page, 0x3c4, 4);
    put32(&mut page, 0x3c8, 1);
    put32(&mut page, 0x3cc, 0);
    put32(&mut page, 0x3d0, 2);
    put32(&mut page, 0x3d4, 3);
    put32(&mut page, 0x3d8, 0);

    page[0x500..0x507].copy_from_slice(&[0xb8, 228, 0, 0, 0, 0x0f, 0x05]);
    page[0x507] = 0xc3;
    page[0x510..0x517].copy_from_slice(&[0xb8, 96, 0, 0, 0, 0x0f, 0x05]);
    page[0x517] = 0xc3;
    page[0x520..0x527].copy_from_slice(&[0xb8, 201, 0, 0, 0, 0x0f, 0x05]);
    page[0x527] = 0xc3;
    page
}

#[cfg(target_arch = "x86_64")]
pub fn install(aspace: &mut AddressSpace) -> bool {
    if aspace.occupied(BASE) || !aspace.alloc_range(BASE, PAGE_SIZE) {
        return false;
    }
    let page = image();
    if !aspace.write_bytes(BASE, &page) {
        return false;
    }
    aspace.protect_range(BASE, PAGE_SIZE, crate::arch::pte::PROT_READ | crate::arch::pte::PROT_EXEC) == 1
}

#[cfg(not(target_arch = "x86_64"))]
pub fn install(_aspace: &mut AddressSpace) -> bool {
    true
}
