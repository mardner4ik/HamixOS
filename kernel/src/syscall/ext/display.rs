use super::*;

pub(in crate::syscall) const SYS_HAMIX_FBMAP: u64 = 9001;
pub(in crate::syscall) const SYS_HAMIX_READKEY: u64 = 9002;
pub(in crate::syscall) const SYS_HAMIX_MOUSE: u64 = 9004;
pub(in crate::syscall) const SYS_HAMIX_POLLKEY: u64 = 9005;
pub(in crate::syscall) const SYS_HAMIX_RELEASE_FB: u64 = 9006;
pub(in crate::syscall) const SYS_DISPLAY_INFO: u64 = 9160;
pub(in crate::syscall) const SYS_DISPLAY_SET: u64 = 9161;
pub(in crate::syscall) const SYS_DISPLAY_CONFIRM: u64 = 9162;
pub(in crate::syscall) const SYS_DISPLAY_REVERT: u64 = 9163;
pub(in crate::syscall) const SYS_FB_GENERATION: u64 = 9164;
pub(in crate::syscall) const SYS_DISPLAY_EDID: u64 = 9165;
pub(in crate::syscall) const SYS_DISPLAY_OUTPUT: u64 = 9166;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_FBMAP => sys_fbmap(a1),
        SYS_HAMIX_RELEASE_FB => {
            task::display::release(task::current_pid());
            0
        }
        SYS_HAMIX_MOUSE => sys_mouse(a1),
        SYS_HAMIX_POLLKEY => poll_key().unwrap_or(-100),
        SYS_HAMIX_READKEY => read_key_code(),
        SYS_DISPLAY_INFO => copy_out(a1, a2, crate::drivers::video::modes::info_text().as_bytes()),
        SYS_DISPLAY_SET => {
            if let Err(e) = require_root_or_console() {
                return Some(e);
            }
            crate::drivers::video::modes::set_mode((a1 >> 32) as u32, a1 as u32, a2 as u32, a3 != 0)
        }
        SYS_DISPLAY_CONFIRM => crate::drivers::video::modes::confirm(),
        SYS_DISPLAY_REVERT => crate::drivers::video::modes::revert(),
        SYS_FB_GENERATION => crate::drivers::video::modes::generation() as i64,
        SYS_DISPLAY_EDID => {
            let raw = crate::drivers::video::gpu::edid(a1 as u32);
            if raw.is_empty() {
                return Some(ENOENT);
            }
            copy_out(a2, a3, &raw)
        }
        SYS_DISPLAY_OUTPUT => {
            if let Err(e) = require_root_or_console() {
                return Some(e);
            }
            crate::drivers::video::gpu::set_output(a1 as u32, (a2 >> 32) as u32, a2 as u32) as i64
        }
        _ => return None,
    })
}

pub(in crate::syscall) fn sys_fbmap(buf: u64) -> i64 {
    let out = match user_slice_mut(buf, 24) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (pid, vt, _, _) = current_identity();
    match task::display::map(pid, vt) {
        Ok(fb) => {
            out[0..8].copy_from_slice(&fb.addr.to_le_bytes());
            out[8..12].copy_from_slice(&fb.pitch.to_le_bytes());
            out[12..16].copy_from_slice(&fb.width.to_le_bytes());
            out[16..20].copy_from_slice(&fb.height.to_le_bytes());
            out[20..24].copy_from_slice(&(fb.bpp as u32).to_le_bytes());
            0
        }
        Err(e) => e,
    }
}

pub(in crate::syscall) fn sys_mouse(buf: u64) -> i64 {
    let out = match user_slice_mut(buf, 16) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (pid, vt, _, _) = current_identity();
    let mut state = crate::drivers::input::mouse::take_state();
    if !crate::tty::vt::input_allowed(pid, vt) {
        state.buttons = 0;
        state.wheel = 0;
        state.presses = [0; 3];
    }
    out[0..4].copy_from_slice(&state.x.to_le_bytes());
    out[4..8].copy_from_slice(&state.y.to_le_bytes());
    let packed = state.buttons as u32 | (state.presses[0].min(15) as u32) << 8 | (state.presses[1].min(15) as u32) << 12 | (state.presses[2].min(15) as u32) << 16;
    out[8..12].copy_from_slice(&packed.to_le_bytes());
    out[12..16].copy_from_slice(&state.wheel.to_le_bytes());
    if crate::drivers::input::mouse::present() { 0 } else { ENODEV }
}
