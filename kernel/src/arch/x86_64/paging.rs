#[allow(dead_code)]
pub fn init() {
}

/// Flip the U/S bit on the 2MB-huge-page PDE(s) covering [addr, addr+len),
/// and on the PML4E/PDPTE above them.
///
/// x86 paging ANDs the U/S bit down the whole walk: a user-mode access is
/// only allowed if PML4E, PDPTE, *and* the final PDE/PTE all have U/S=1.
/// boot.S's identity map builds every PML4E/PDPTE with flags `0x3`
/// (present+writable, U/S clear), so setting U/S only on the leaf PDE is
/// not enough -- the intermediate levels still deny ring-3 access even
/// though the final entry says it's allowed. This grants U/S on every
/// level of the path, not just the leaf.
///
/// HamixOS today has a single identity-mapped kernel address space built by
/// boot.S out of 2MB pages with U/S left clear (supervisor-only). There is
/// no per-process page table yet (that lands with the ELF loader), so this
/// is a deliberately blunt, temporary tool: it grants user-mode access to
/// the *entire* 2MB region(s) the target range falls in, not just the exact
/// bytes requested, and it never revokes U/S from the PML4E/PDPTE it
/// touches once granted (they cover 512GB/1GB respectively, so revoking
/// per-caller would need reference counting this bridge stage doesn't have
/// yet). Only use it for the ring-3 smoke test; real user processes need
/// their own address space, not a hole punched in the kernel's.
pub fn allow_user_access(addr: u64, len: u64) {
    const PAGE_2M: u64 = 0x20_0000;
    const PHYS_MASK: u64 = 0x000F_FFFF_FFFF_F000;
    const USER_BIT: u64 = 0b100;

    unsafe {
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        let p4 = (cr3 & !0xFFF) as *mut u64;

        let start = addr & !(PAGE_2M - 1);
        let end = (addr + len + PAGE_2M - 1) & !(PAGE_2M - 1);
        let mut cur = start;

        crate::serial_println!(
            "paging: allow_user_access addr={:#x} len={:#x} start={:#x} end={:#x}",
            addr,
            len,
            start,
            end
        );

        while cur < end {
            let p4_idx = ((cur >> 39) & 0x1FF) as usize;
            let p4e = core::ptr::read_volatile(p4.add(p4_idx));
            if p4e & 1 == 0 {
                crate::serial_println!("paging: p4[{}] not present, skipping {:#x}", p4_idx, cur);
                cur += PAGE_2M;
                continue;
            }
            if p4e & USER_BIT == 0 {
                core::ptr::write_volatile(p4.add(p4_idx), p4e | USER_BIT);
            }

            let p3 = (p4e & PHYS_MASK) as *mut u64;
            let p3_idx = ((cur >> 30) & 0x1FF) as usize;
            let p3e = core::ptr::read_volatile(p3.add(p3_idx));
            if p3e & 1 == 0 {
                crate::serial_println!("paging: p3[{}] not present, skipping {:#x}", p3_idx, cur);
                cur += PAGE_2M;
                continue;
            }
            if p3e & USER_BIT == 0 {
                core::ptr::write_volatile(p3.add(p3_idx), p3e | USER_BIT);
            }

            let p2 = (p3e & PHYS_MASK) as *mut u64;
            let p2_idx = ((cur >> 21) & 0x1FF) as usize;
            let p2e = core::ptr::read_volatile(p2.add(p2_idx));
            if p2e & 1 == 0 {
                crate::serial_println!("paging: p2[{}] not present, skipping {:#x}", p2_idx, cur);
                cur += PAGE_2M;
                continue;
            }
            core::ptr::write_volatile(p2.add(p2_idx), p2e | USER_BIT);

            let readback = core::ptr::read_volatile(p2.add(p2_idx));
            crate::serial_println!(
                "paging: punched {:#x} p4[{}]={:#x} p3[{}]={:#x} p2[{}]={:#x}",
                cur,
                p4_idx,
                core::ptr::read_volatile(p4.add(p4_idx)),
                p3_idx,
                core::ptr::read_volatile(p3.add(p3_idx)),
                p2_idx,
                readback
            );

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
