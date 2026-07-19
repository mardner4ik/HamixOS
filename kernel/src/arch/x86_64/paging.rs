#[allow(dead_code)]
pub fn init() {
}

/// Flip the U/S bit on the 2MB-huge-page PDE(s) covering [addr, addr+len).
///
/// HamixOS today has a single identity-mapped kernel address space built by
/// boot.S out of 2MB pages with U/S left clear (supervisor-only). There is
/// no per-process page table yet (that lands with the ELF loader), so this
/// is a deliberately blunt, temporary tool: it grants user-mode access to
/// the *entire* 2MB region(s) the target range falls in, not just the exact
/// bytes requested. Only use it for the ring-3 smoke test; real user
/// processes need their own address space, not a hole punched in the
/// kernel's.
pub fn allow_user_access(addr: u64, len: u64) {
    const PAGE_2M: u64 = 0x20_0000;
    const PHYS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

    unsafe {
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        let p4 = (cr3 & !0xFFF) as *mut u64;

        let start = addr & !(PAGE_2M - 1);
        let end = (addr + len + PAGE_2M - 1) & !(PAGE_2M - 1);
        let mut cur = start;

        while cur < end {
            let p4_idx = ((cur >> 39) & 0x1FF) as usize;
            let p4e = core::ptr::read_volatile(p4.add(p4_idx));
            if p4e & 1 == 0 {
                cur += PAGE_2M;
                continue;
            }

            let p3 = (p4e & PHYS_MASK) as *mut u64;
            let p3_idx = ((cur >> 30) & 0x1FF) as usize;
            let p3e = core::ptr::read_volatile(p3.add(p3_idx));
            if p3e & 1 == 0 {
                cur += PAGE_2M;
                continue;
            }

            let p2 = (p3e & PHYS_MASK) as *mut u64;
            let p2_idx = ((cur >> 21) & 0x1FF) as usize;
            let p2e = core::ptr::read_volatile(p2.add(p2_idx));
            core::ptr::write_volatile(p2.add(p2_idx), p2e | 0b100);

            cur += PAGE_2M;
        }

        core::arch::asm!(
            "mov {tmp}, cr3",
            "mov cr3, {tmp}",
            tmp = out(reg) _,
            options(nostack),
        );
    }
}
