use alloc::vec::Vec;
use spin::Mutex;

use super::Pid;
use crate::arch::paging::{self, PAGE_SIZE};
use crate::arch::without_interrupts;
use crate::memory::{frame, FramebufferInfo};

struct DisplayState {
    owner: Option<(Pid, usize)>,
    shadow: Vec<usize>,
}

static STATE: Mutex<DisplayState> = Mutex::new(DisplayState { owner: None, shadow: Vec::new() });

fn framebuffer() -> Option<FramebufferInfo> {
    *crate::memory::FRAMEBUFFER.lock()
}

fn pages(fb: &FramebufferInfo) -> u64 {
    let buffers = crate::drivers::video::gpu::buffers() as u64;
    if buffers > 1 {
        return crate::drivers::video::gpu::buffer_stride(fb) * buffers / PAGE_SIZE;
    }
    fb.byte_len().div_ceil(PAGE_SIZE)
}

fn show_console_buffer() {
    if crate::drivers::video::gpu::front() != 0 {
        crate::drivers::video::gpu::page_flip(0);
    }
}

pub fn owner() -> Option<Pid> {
    without_interrupts(|| STATE.lock().owner.map(|(pid, _)| pid))
}

pub fn suspended() -> bool {
    without_interrupts(|| !STATE.lock().shadow.is_empty())
}

pub fn owner_vt() -> Option<usize> {
    without_interrupts(|| STATE.lock().owner.map(|(_, vt)| vt))
}

fn remap(pid: Pid, targets: &[u64], wc: bool) {
    super::with_task(pid, |task| {
        if let Some(aspace) = task.aspace.as_mut() {
            for (i, phys) in targets.iter().enumerate() {
                aspace.map_shared(paging::USER_FB_BASE + i as u64 * PAGE_SIZE, *phys, wc);
            }
        }
    });
    paging::flush_tlb();
}

pub fn map(pid: Pid, vt: usize) -> Result<FramebufferInfo, i64> {
    let fb = framebuffer().ok_or(-19i64)?;
    let mut state = without_interrupts(|| STATE.lock().owner);
    if let Some((owner, _)) = state {
        if owner != pid && super::exists(owner) {
            return Err(-16);
        }
        if owner != pid {
            release(owner);
            state = None;
        }
    }
    let _ = state;
    let count = pages(&fb);
    let ok = super::with_task(pid, |task| {
        let Some(aspace) = task.aspace.as_mut() else {
            return false;
        };
        for i in 0..count {
            if !aspace.map_shared(paging::USER_FB_BASE + i * PAGE_SIZE, fb.addr + i * PAGE_SIZE, true) {
                return false;
            }
        }
        true
    })
    .unwrap_or(false);
    if !ok {
        return Err(-12);
    }
    without_interrupts(|| STATE.lock().owner = Some((pid, vt)));
    crate::drivers::video::console_mouse::disable();
    crate::drivers::video::text_mode::set_graphics_owned(vt == crate::drivers::video::text_mode::foreground());
    crate::drivers::input::mouse::drain_events();
    Ok(FramebufferInfo { addr: paging::USER_FB_BASE, ..fb })
}

pub fn refresh_owner() {
    let Some(fb) = framebuffer() else {
        return;
    };
    let (owner, suspended) = without_interrupts(|| {
        let state = STATE.lock();
        (state.owner, !state.shadow.is_empty())
    });
    let Some((pid, _)) = owner else {
        return;
    };
    if suspended {
        return;
    }
    let targets: Vec<u64> = (0..pages(&fb)).map(|i| fb.addr + i * PAGE_SIZE).collect();
    remap(pid, &targets, true);
}

pub fn release(pid: Pid) {
    let released = without_interrupts(|| {
        let mut state = STATE.lock();
        match state.owner {
            Some((owner, _)) if owner == pid => {
                state.owner = None;
                Some(core::mem::take(&mut state.shadow))
            }
            _ => None,
        }
    });
    let Some(shadow) = released else {
        return;
    };
    if let Some(fb) = framebuffer() {
        super::with_task(pid, |task| {
            if let Some(aspace) = task.aspace.as_mut() {
                aspace.unmap_range(paging::USER_FB_BASE, pages(&fb) * PAGE_SIZE);
            }
        });
        paging::flush_tlb();
    }
    for f in shadow {
        frame::free_frame(f);
    }
    show_console_buffer();
    crate::drivers::video::text_mode::set_graphics_owned(false);
    crate::drivers::video::text_mode::fb_clear_full();
    crate::drivers::video::text_mode::fb_redraw_all();
    crate::drivers::video::console_mouse::enable();
}

pub fn on_vt_switch(target: usize) -> bool {
    let Some(fb) = framebuffer() else {
        return false;
    };
    let (owner, suspended) = without_interrupts(|| {
        let state = STATE.lock();
        (state.owner, !state.shadow.is_empty())
    });
    let Some((pid, vt)) = owner else {
        return false;
    };
    let count = pages(&fb) as usize;
    if vt != target && !suspended {
        let mut shadow = Vec::with_capacity(count);
        for _ in 0..count {
            match frame::alloc_frame() {
                Some(f) => shadow.push(f),
                None => {
                    for f in shadow {
                        frame::free_frame(f);
                    }
                    return false;
                }
            }
        }
        for (i, f) in shadow.iter().enumerate() {
            unsafe {
                core::ptr::copy_nonoverlapping((fb.addr + i as u64 * PAGE_SIZE) as *const u8, *f as *mut u8, PAGE_SIZE as usize);
            }
        }
        let targets: Vec<u64> = shadow.iter().map(|f| *f as u64).collect();
        remap(pid, &targets, false);
        without_interrupts(|| STATE.lock().shadow = shadow);
        show_console_buffer();
        crate::drivers::video::text_mode::set_graphics_owned(false);
        false
    } else if vt == target && suspended {
        let shadow = without_interrupts(|| core::mem::take(&mut STATE.lock().shadow));
        crate::drivers::video::text_mode::set_graphics_owned(true);
        for (i, f) in shadow.iter().enumerate() {
            unsafe {
                core::ptr::copy_nonoverlapping(*f as *const u8, (fb.addr + i as u64 * PAGE_SIZE) as *mut u8, PAGE_SIZE as usize);
            }
        }
        let targets: Vec<u64> = (0..count as u64).map(|i| fb.addr + i * PAGE_SIZE).collect();
        remap(pid, &targets, true);
        for f in shadow {
            frame::free_frame(f);
        }
        crate::drivers::video::gpu::mark_dirty(0, 0, fb.width, fb.height);
        crate::drivers::video::gpu::flush_pending();
        true
    } else {
        vt == target
    }
}
