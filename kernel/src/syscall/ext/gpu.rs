use super::*;

pub(in crate::syscall) const SYS_GPU_CAPS: u64 = 9180;
pub(in crate::syscall) const SYS_GPU_FLUSH: u64 = 9181;
pub(in crate::syscall) const SYS_GPU_FILL: u64 = 9182;
pub(in crate::syscall) const SYS_GPU_COPY: u64 = 9183;
pub(in crate::syscall) const SYS_GPU_CURSOR_SET: u64 = 9184;
pub(in crate::syscall) const SYS_GPU_CURSOR_MOVE: u64 = 9185;
pub(in crate::syscall) const SYS_GPU_CURSOR_HIDE: u64 = 9186;
pub(in crate::syscall) const SYS_GPU_FLIP: u64 = 9187;
pub(in crate::syscall) const SYS_GPU_VBLANK: u64 = 9188;
pub(in crate::syscall) const SYS_GPU_BUFFERS: u64 = 9189;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_GPU_CAPS => crate::drivers::video::gpu::caps() as i64,
        SYS_GPU_FLUSH => sys_gpu_flush(a1, a2),
        SYS_GPU_FILL => sys_gpu_fill(a1, a2, a3),
        SYS_GPU_COPY => sys_gpu_copy(a1, a2, a3),
        SYS_GPU_CURSOR_SET => sys_gpu_cursor_set(a1, a2, a3),
        SYS_GPU_CURSOR_MOVE => sys_gpu_cursor(a1, false),
        SYS_GPU_CURSOR_HIDE => sys_gpu_cursor(0, true),
        SYS_GPU_FLIP => {
            if !gpu_allowed() {
                return Some(EACCES);
            }
            crate::drivers::video::gpu::page_flip(a1 as u32) as i64
        }
        SYS_GPU_VBLANK => crate::drivers::video::gpu::wait_vblank(a1.clamp(1, 1000) as u32) as i64,
        SYS_GPU_BUFFERS => crate::drivers::video::gpu::buffers() as i64,
        _ => return None,
    })
}

pub(in crate::syscall) fn gpu_allowed() -> bool {
    task::display::owner() == Some(task::current_pid())
}

pub(in crate::syscall) fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

pub(in crate::syscall) fn sys_gpu_flush(at: u64, size: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (x, y) = unpack(at);
    let (w, h) = unpack(size);
    crate::drivers::video::gpu::flush(x, y, w, h) as i64
}

pub(in crate::syscall) fn sys_gpu_fill(at: u64, size: u64, color: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (x, y) = unpack(at);
    let (w, h) = unpack(size);
    crate::drivers::video::gpu::fill(x, y, w, h, color as u32) as i64
}

pub(in crate::syscall) fn sys_gpu_copy(src: u64, dst: u64, size: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (sx, sy) = unpack(src);
    let (dx, dy) = unpack(dst);
    let (w, h) = unpack(size);
    crate::drivers::video::gpu::copy(sx, sy, dx, dy, w, h) as i64
}

pub(in crate::syscall) fn sys_gpu_cursor_set(buf: u64, size: u64, hot: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (w, h) = unpack(size);
    let (hx, hy) = unpack(hot);
    if w == 0 || h == 0 || w > crate::drivers::video::gpu::CURSOR_MAX || h > crate::drivers::video::gpu::CURSOR_MAX {
        return EINVAL;
    }
    let bytes = match user_slice(buf, w as u64 * h as u64 * 4) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let mut pixels = alloc::vec![0u32; (w * h) as usize];
    for (dst, chunk) in pixels.iter_mut().zip(bytes.chunks_exact(4)) {
        *dst = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    crate::drivers::video::gpu::cursor_set(&pixels, w, h, hx, hy) as i64
}

pub(in crate::syscall) fn sys_gpu_cursor(at: u64, hide: bool) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    if hide {
        return crate::drivers::video::gpu::cursor_hide() as i64;
    }
    let (x, y) = unpack(at);
    crate::drivers::video::gpu::cursor_move(x as i32, y as i32) as i64
}
