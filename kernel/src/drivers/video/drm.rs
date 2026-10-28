use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::without_interrupts;
use crate::usercopy::{copy_out, user_slice};
use crate::errno::{EACCES, EAGAIN, EINVAL, ENODEV, ENOENT, ENOMEM};
use crate::task::{self, ipc, Abi, Pid};

const CONNECTOR: u32 = 31;
const ENCODER: u32 = 32;
const CRTC: u32 = 33;
const PLANE: u32 = 34;
const FIRST_FB: u32 = 100;
const PROP_DPMS: u32 = 60;
const PROP_EDID: u32 = 61;
const PROP_TYPE: u32 = 62;
const BLOB_EDID: u32 = 70;
const OBJECT_CONNECTOR: u32 = 0xc0c0_c0c0;
const OBJECT_PLANE: u32 = 0xeeee_eeee;
const PROP_ENUM: u32 = 1 << 3;
const PROP_BLOB: u32 = 1 << 4;
const PROP_IMMUTABLE: u32 = 1 << 2;
const MAP_SHIFT: u64 = 28;

const XRGB8888: u32 = 0x3432_5258;
const ARGB8888: u32 = 0x3432_5241;
const RGB888: u32 = 0x3432_4752;
const BGR888: u32 = 0x3432_4742;

const EVENT_FLIP_COMPLETE: u32 = 2;
const PAGE_FLIP_EVENT: u32 = 1;
use crate::errno::EOPNOTSUPP;
use crate::errno::ENXIO;

struct Dumb {
    segment: u32,
    size: u64,
}

struct Framebuffer {
    file: u32,
    segment: u32,
    width: u32,
    height: u32,
    pitch: u32,
    offset: u32,
    bytes: u32,
}

struct Pending {
    ready: u64,
    data: [u8; 32],
}

struct File {
    handles: BTreeMap<u32, Dumb>,
    next_handle: u32,
    events: VecDeque<Pending>,
}

#[derive(Clone, Copy)]
struct Scanout {
    fb: u32,
    x: u32,
    y: u32,
    pid: Pid,
}

struct Device {
    files: BTreeMap<u32, File>,
    fbs: BTreeMap<u32, Framebuffer>,
    next_fb: u32,
    scanout: Option<Scanout>,
    sequence: u32,
    dpms: u64,
}

static DEVICE: Mutex<Device> = Mutex::new(Device { files: BTreeMap::new(), fbs: BTreeMap::new(), next_fb: FIRST_FB, scanout: None, sequence: 0, dpms: 0 });

fn with<R>(f: impl FnOnce(&mut Device) -> R) -> R {
    without_interrupts(|| f(&mut DEVICE.lock()))
}

fn r32(s: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(s[at..at + 4].try_into().unwrap())
}

fn r64(s: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(s[at..at + 8].try_into().unwrap())
}

fn w16(s: &mut [u8], at: usize, v: u16) {
    s[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn w32(s: &mut [u8], at: usize, v: u32) {
    s[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn w64(s: &mut [u8], at: usize, v: u64) {
    s[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn screen() -> Option<(u32, u32)> {
    let fb = (*crate::memory::FRAMEBUFFER.lock())?;
    Some((fb.width, fb.height))
}

fn modeinfo(width: u32, height: u32) -> [u8; 68] {
    let mut m = [0u8; 68];
    let htotal = width + 160;
    let vtotal = height + 23;
    w32(&mut m, 0, htotal * vtotal / 1000 * 60);
    w16(&mut m, 4, width as u16);
    w16(&mut m, 6, (width + 48) as u16);
    w16(&mut m, 8, (width + 80) as u16);
    w16(&mut m, 10, htotal as u16);
    w16(&mut m, 14, height as u16);
    w16(&mut m, 16, (height + 3) as u16);
    w16(&mut m, 18, (height + 7) as u16);
    w16(&mut m, 20, vtotal as u16);
    w32(&mut m, 24, 60);
    w32(&mut m, 28, 0);
    w32(&mut m, 32, 0x48);
    let name = format!("{}x{}", width, height);
    let n = name.len().min(31);
    m[36..36 + n].copy_from_slice(&name.as_bytes()[..n]);
    m
}

pub fn open(flags: u64) -> i64 {
    let abi = task::with_current(|t| t.abi);
    if (abi == Abi::Linux && !crate::linux::linux_enabled()) || !abi.posix() {
        return ENODEV;
    }
    if screen().is_none() {
        return ENODEV;
    }
    crate::syscall::special::create_drm(flags)
}

pub fn opened(id: u32) {
    with(|d| d.files.insert(id, File { handles: BTreeMap::new(), next_handle: 1, events: VecDeque::new() }));
}

pub fn closed(id: u32) {
    let segments = with(|d| {
        let mut segments = Vec::new();
        if let Some(file) = d.files.remove(&id) {
            segments.extend(file.handles.values().map(|h| h.segment));
        }
        let owned: Vec<u32> = d.fbs.iter().filter(|(_, fb)| fb.file == id).map(|(k, _)| *k).collect();
        for key in owned {
            if let Some(fb) = d.fbs.remove(&key) {
                segments.push(fb.segment);
            }
            if d.scanout.map(|s| s.fb) == Some(key) {
                d.scanout = None;
            }
        }
        segments
    });
    for s in segments {
        ipc::release(s);
    }
}

pub fn readable(id: u32) -> bool {
    let now = task::ticks();
    with(|d| d.files.get(&id).map(|f| f.events.front().map(|e| e.ready <= now).unwrap_or(false)).unwrap_or(false))
}

pub fn read(fd: u64, id: u32, buf: u64, count: u64) -> i64 {
    if count < 32 {
        return EINVAL;
    }
    loop {
        let now = task::ticks();
        let ready: Vec<[u8; 32]> = with(|d| {
            let mut out = Vec::new();
            if let Some(file) = d.files.get_mut(&id) {
                while (out.len() as u64 + 1) * 32 <= count && file.events.front().map(|e| e.ready <= now).unwrap_or(false) {
                    out.push(file.events.pop_front().unwrap().data);
                }
            }
            out
        });
        if !ready.is_empty() {
            let bytes: Vec<u8> = ready.iter().flat_map(|e| e.iter().copied()).collect();
            let r = copy_out(buf, bytes.len() as u64, &bytes);
            return if r < 0 { r } else { bytes.len() as i64 };
        }
        if crate::syscall::fd_nonblock(fd) {
            return EAGAIN;
        }
        if crate::syscall::signal::pending() {
            return crate::syscall::signal::EINTR;
        }
        let next = with(|d| d.files.get(&id).and_then(|f| f.events.front().map(|e| e.ready)));
        let slice = next.map(|r| r.saturating_sub(now)).unwrap_or(task::TICK_HZ / 10).clamp(1, task::TICK_HZ / 10);
        crate::tty::sleep_for_events(slice);
    }
}

pub fn map_target(id: u32, offset: u64) -> Option<(u32, u64)> {
    let handle = (offset >> MAP_SHIFT) as u32;
    let within = offset & ((1 << MAP_SHIFT) - 1);
    with(|d| d.files.get(&id)?.handles.get(&handle).filter(|h| within < h.size).map(|h| (h.segment, within)))
}

pub fn refresh() {
    present();
}

fn present() {
    present_rects(None);
}

fn present_rects(clips: Option<&[(u32, u32, u32, u32)]>) {
    let Some(fb) = *crate::memory::FRAMEBUFFER.lock() else {
        return;
    };
    let Some((scanout, src)) = with(|d| {
        let s = d.scanout?;
        let f = d.fbs.get(&s.fb)?;
        ipc::retain(f.segment);
        Some((s, (f.segment, f.width, f.height, f.pitch, f.offset, f.bytes)))
    }) else {
        return;
    };
    let (segment, width, height, pitch, offset, bytes) = src;
    let visible = task::display::owner() == Some(scanout.pid) && !task::display::suspended() && fb.bpp == 32;
    if visible {
        let rows = height.saturating_sub(scanout.y).min(fb.height);
        let cols = width.saturating_sub(scanout.x).min(fb.width);
        let full = [(0u32, 0u32, cols, rows)];
        let rects: &[(u32, u32, u32, u32)] = match clips {
            Some(c) if !c.is_empty() => c,
            _ => &full,
        };
        let mut row = alloc::vec![0u8; cols as usize * 4];
        let pages = ipc::shm_frames(segment);
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for &(x1, y1, x2, y2) in rects {
            let (x1, y1, x2, y2) = (x1.min(cols), y1.min(rows), x2.min(cols), y2.min(rows));
            if x1 >= x2 || y1 >= y2 {
                continue;
            }
            let pixels = (x2 - x1) as usize;
            let span = pixels * bytes as usize;
            for y in y1..y2 {
                let from = offset as u64 + (scanout.y + y) as u64 * pitch as u64 + (scanout.x + x1) as u64 * bytes as u64;
                let target = (fb.addr + y as u64 * fb.pitch as u64 + x1 as u64 * 4) as *mut u8;
                if bytes == 4 {
                    if let Some((frames, len)) = pages.as_ref() {
                        if from + span as u64 > *len {
                            break;
                        }
                        let mut done = 0usize;
                        while done < span {
                            let pos = from as usize + done;
                            let page = frames[pos / 4096];
                            let within = pos % 4096;
                            let step = (4096 - within).min(span - done);
                            unsafe { core::ptr::copy_nonoverlapping((page + within) as *const u8, target.add(done), step) };
                            done += step;
                        }
                        continue;
                    }
                }
                if ipc::shm_io(segment, from, &mut row[..span], false).is_err() {
                    break;
                }
                if bytes == 4 {
                    unsafe {
                        core::ptr::copy_nonoverlapping(row.as_ptr(), target, span);
                    }
                } else {
                    let out = unsafe { core::slice::from_raw_parts_mut(target as *mut u32, pixels) };
                    for (i, px) in out.iter_mut().enumerate() {
                        let p = &row[i * 3..i * 3 + 3];
                        *px = p[0] as u32 | (p[1] as u32) << 8 | (p[2] as u32) << 16;
                    }
                }
            }
            min_x = min_x.min(x1);
            min_y = min_y.min(y1);
            max_x = max_x.max(x2);
            max_y = max_y.max(y2);
        }
        if crate::drivers::video::gpu::front() != 0 {
            crate::drivers::video::gpu::page_flip(0);
        }
        if max_x > min_x && max_y > min_y {
            crate::drivers::video::gpu::mark_dirty(min_x, min_y, max_x - min_x, max_y - min_y);
        }
        crate::drivers::video::gpu::flush_pending();
    }
    ipc::release(segment);
}

fn version(s: &mut [u8]) -> i64 {
    w32(s, 0, 1);
    w32(s, 4, 0);
    w32(s, 8, 0);
    for (at, text) in [(16usize, "hamix"), (32, "20260928"), (48, "Hamix DRM compatibility")] {
        let len = r64(s, at);
        let ptr = r64(s, at + 8);
        if ptr != 0 && len > 0 {
            let n = (len as usize).min(text.len());
            if copy_out(ptr, n as u64, &text.as_bytes()[..n]) < 0 {
                return -14;
            }
        }
        w64(s, at, text.len() as u64);
    }
    0
}

fn get_cap(s: &mut [u8]) -> i64 {
    let value = match r64(s, 0) {
        0x1 => 1,
        0x2 => 0,
        0x3 => 24,
        0x4 => 1,
        0x5 => 0,
        0x6 => 1,
        0x7 => 0,
        0x8 | 0x9 => 64,
        0x10 => 0,
        0x11 => 0,
        0x12 => 1,
        0x13 | 0x14 => 0,
        _ => return EINVAL,
    };
    w64(s, 8, value);
    0
}

fn set_client_cap(s: &[u8]) -> i64 {
    match r64(s, 0) {
        1 | 2 | 4 => 0,
        _ => EOPNOTSUPP,
    }
}

fn put_ids(ptr: u64, capacity: u32, ids: &[u32]) -> i64 {
    if ptr != 0 && capacity as usize >= ids.len() {
        let bytes: Vec<u8> = ids.iter().flat_map(|i| i.to_le_bytes()).collect();
        if copy_out(ptr, bytes.len() as u64, &bytes) < 0 {
            return -14;
        }
    }
    0
}

fn get_resources(s: &mut [u8]) -> i64 {
    let (width, height) = screen().unwrap_or((0, 0));
    let fbs: Vec<u32> = with(|d| d.fbs.keys().copied().collect());
    let lists: [(usize, usize, &[u32]); 4] = [(0, 32, &fbs), (8, 36, &[CRTC]), (16, 40, &[CONNECTOR]), (24, 44, &[ENCODER])];
    for (ptr, count, ids) in lists {
        let r = put_ids(r64(s, ptr), r32(s, count), ids);
        if r < 0 {
            return r;
        }
        w32(s, count, ids.len() as u32);
    }
    w32(s, 48, 1);
    w32(s, 52, width.max(8192));
    w32(s, 56, 1);
    w32(s, 60, height.max(8192));
    0
}

fn get_connector(s: &mut [u8]) -> i64 {
    if r32(s, 48) != CONNECTOR {
        return ENOENT;
    }
    let (width, height) = screen().unwrap_or((0, 0));
    let modes_ptr = r64(s, 8);
    if modes_ptr != 0 && r32(s, 32) >= 1 && copy_out(modes_ptr, 68, &modeinfo(width, height)) < 0 {
        return -14;
    }
    let r = put_ids(r64(s, 0), r32(s, 40), &[ENCODER]);
    if r < 0 {
        return r;
    }
    if r32(s, 36) >= 2 {
        let dpms = with(|d| d.dpms);
        let r = put_ids(r64(s, 16), 2, &[PROP_DPMS, PROP_EDID]);
        if r < 0 {
            return r;
        }
        let values: Vec<u8> = [dpms, BLOB_EDID as u64].iter().flat_map(|v| v.to_le_bytes()).collect();
        if r64(s, 24) != 0 && copy_out(r64(s, 24), 16, &values) < 0 {
            return -14;
        }
    }
    w32(s, 32, 1);
    w32(s, 36, 2);
    w32(s, 40, 1);
    w32(s, 44, ENCODER);
    w32(s, 52, 15);
    w32(s, 56, 1);
    w32(s, 60, 1);
    w32(s, 64, width * 265 / 1000);
    w32(s, 68, height * 265 / 1000);
    w32(s, 72, 1);
    0
}

fn get_encoder(s: &mut [u8]) -> i64 {
    if r32(s, 0) != ENCODER {
        return ENOENT;
    }
    w32(s, 4, 5);
    w32(s, 8, CRTC);
    w32(s, 12, 1);
    w32(s, 16, 0);
    0
}

fn get_crtc(s: &mut [u8]) -> i64 {
    if r32(s, 12) != CRTC {
        return ENOENT;
    }
    let (width, height) = screen().unwrap_or((0, 0));
    let scanout = with(|d| d.scanout);
    w32(s, 16, scanout.map(|s| s.fb).unwrap_or(0));
    w32(s, 20, scanout.map(|s| s.x).unwrap_or(0));
    w32(s, 24, scanout.map(|s| s.y).unwrap_or(0));
    w32(s, 28, 0);
    w32(s, 32, scanout.is_some() as u32);
    s[36..104].copy_from_slice(&if scanout.is_some() { modeinfo(width, height) } else { [0u8; 68] });
    0
}

fn claim_display() -> Result<Pid, i64> {
    let (pid, vt) = task::with_current(|t| (t.pid, t.vt));
    if task::display::owner() == Some(pid) {
        return Ok(pid);
    }
    task::display::map(pid, vt).map(|_| pid).map_err(|_| EACCES)
}

fn set_crtc(s: &[u8]) -> i64 {
    if r32(s, 12) != CRTC {
        return ENOENT;
    }
    let fb_id = r32(s, 16);
    let mode_valid = r32(s, 32) != 0;
    if fb_id == 0 || !mode_valid {
        with(|d| d.scanout = None);
        return 0;
    }
    let (width, height) = screen().unwrap_or((0, 0));
    let hdisplay = u16::from_le_bytes([s[40], s[41]]) as u32;
    let vdisplay = u16::from_le_bytes([s[50], s[51]]) as u32;
    if (hdisplay, vdisplay) != (width, height) {
        return EINVAL;
    }
    if r32(s, 8) > 1 {
        return EINVAL;
    }
    let (x, y) = (r32(s, 20), r32(s, 24));
    let fb = if fb_id == u32::MAX { with(|d| d.scanout.map(|s| s.fb)) } else { Some(fb_id) };
    let Some(fb) = fb else {
        return EINVAL;
    };
    let fits = with(|d| d.fbs.get(&fb).map(|f| f.width >= x + width && f.height >= y + height));
    match fits {
        None => return ENOENT,
        Some(false) => return -28,
        Some(true) => {}
    }
    let pid = match claim_display() {
        Ok(pid) => pid,
        Err(e) => return e,
    };
    with(|d| d.scanout = Some(Scanout { fb, x, y, pid }));
    present();
    0
}

fn page_flip(id: u32, s: &[u8]) -> i64 {
    if r32(s, 0) != CRTC {
        return ENOENT;
    }
    let fb = r32(s, 4);
    let flags = r32(s, 8);
    let user_data = r64(s, 16);
    let now = task::ticks();
    let frame = task::TICK_HZ / 60;
    let result = with(|d| {
        if !d.fbs.contains_key(&fb) {
            return Err(ENOENT);
        }
        let Some(scanout) = d.scanout.as_mut() else {
            return Err(EINVAL);
        };
        if flags & PAGE_FLIP_EVENT != 0 && d.files.get(&id).map(|f| f.events.iter().any(|e| e.ready > now)).unwrap_or(false) {
            return Err(-16);
        }
        scanout.fb = fb;
        d.sequence = d.sequence.wrapping_add(1);
        Ok(d.sequence)
    });
    let sequence = match result {
        Ok(s) => s,
        Err(e) => return e,
    };
    present();
    if flags & PAGE_FLIP_EVENT != 0 {
        let ready = (now / frame + 1) * frame;
        let mut data = [0u8; 32];
        w32(&mut data, 0, EVENT_FLIP_COMPLETE);
        w32(&mut data, 4, 32);
        w64(&mut data, 8, user_data);
        let ms = ready * 1000 / task::TICK_HZ;
        w32(&mut data, 16, (ms / 1000) as u32);
        w32(&mut data, 20, ((ms % 1000) * 1000) as u32);
        w32(&mut data, 24, sequence);
        w32(&mut data, 28, CRTC);
        with(|d| {
            if let Some(file) = d.files.get_mut(&id) {
                file.events.push_back(Pending { ready, data });
            }
        });
        crate::task::wake_all(task::WAIT_PIPE);
    }
    0
}

fn dirty_fb(s: &[u8]) -> i64 {
    let fb = r32(s, 0);
    let (known, shown) = with(|d| (d.fbs.contains_key(&fb), d.scanout.map(|s| s.fb) == Some(fb)));
    if !known {
        return ENOENT;
    }
    if !shown {
        return 0;
    }
    let count = (r32(s, 12) as usize).min(256);
    let ptr = r64(s, 16);
    if count == 0 || ptr == 0 {
        present();
        return 0;
    }
    let raw = match user_slice(ptr, count as u64 * 8) {
        Ok(b) => b.to_vec(),
        Err(e) => return e,
    };
    let clips: Vec<(u32, u32, u32, u32)> = raw
        .chunks_exact(8)
        .map(|c| (u16::from_le_bytes([c[0], c[1]]) as u32, u16::from_le_bytes([c[2], c[3]]) as u32, u16::from_le_bytes([c[4], c[5]]) as u32, u16::from_le_bytes([c[6], c[7]]) as u32))
        .collect();
    present_rects(Some(&clips));
    0
}

fn create_dumb(id: u32, s: &mut [u8]) -> i64 {
    let (height, width, bpp) = (r32(s, 0), r32(s, 4), r32(s, 8));
    if width == 0 || height == 0 || width > 16384 || height > 16384 || !(bpp == 32 || bpp == 24 || bpp == 16 || bpp == 8) {
        return EINVAL;
    }
    let pitch = (width * bpp.div_ceil(8) + 63) & !63;
    let size = (pitch as u64 * height as u64 + 4095) & !4095;
    let segment = ipc::memfd_create(task::current_pid());
    if ipc::shm_set_len(segment, size) < 0 {
        ipc::release(segment);
        return ENOMEM;
    }
    let handle = with(|d| {
        let file = d.files.get_mut(&id)?;
        let handle = file.next_handle;
        file.next_handle += 1;
        file.handles.insert(handle, Dumb { segment, size });
        Some(handle)
    });
    let Some(handle) = handle else {
        ipc::release(segment);
        return EINVAL;
    };
    w32(s, 16, handle);
    w32(s, 20, pitch);
    w64(s, 24, size);
    0
}

fn map_dumb(id: u32, s: &mut [u8]) -> i64 {
    let handle = r32(s, 0);
    let known = with(|d| d.files.get(&id).map(|f| f.handles.contains_key(&handle)).unwrap_or(false));
    if !known {
        return ENOENT;
    }
    w64(s, 8, (handle as u64) << MAP_SHIFT);
    0
}

fn destroy_dumb(id: u32, handle: u32) -> i64 {
    let segment = with(|d| d.files.get_mut(&id)?.handles.remove(&handle).map(|h| h.segment));
    match segment {
        Some(segment) => {
            ipc::release(segment);
            0
        }
        None => ENOENT,
    }
}

fn add_fb(id: u32, handle: u32, width: u32, height: u32, pitch: u32, offset: u32, bytes: u32) -> Result<u32, i64> {
    if width == 0 || height == 0 || pitch < width * bytes {
        crate::debug_println!("drm: framebuffer {}x{} with pitch {} is too narrow", width, height, pitch);
        return Err(EINVAL);
    }
    with(|d| {
        let dumb = d.files.get(&id).and_then(|f| f.handles.get(&handle)).ok_or(ENOENT)?;
        if offset as u64 + pitch as u64 * height as u64 > dumb.size {
            return Err(EINVAL);
        }
        let segment = dumb.segment;
        ipc::retain(segment);
        let fb = d.next_fb;
        d.next_fb += 1;
        d.fbs.insert(fb, Framebuffer { file: id, segment, width, height, pitch, offset, bytes });
        if d.fbs.values().filter(|f| f.file == id).count() == 1 {
            crate::drivers::klog::log(&alloc::format!("drm: pid {} framebuffer {}x{}, {} bytes per pixel", task::current_pid(), width, height, bytes));
        }
        Ok(fb)
    })
}

fn add_fb1(id: u32, s: &mut [u8]) -> i64 {
    let (width, height, pitch, bpp, depth, handle) = (r32(s, 4), r32(s, 8), r32(s, 12), r32(s, 16), r32(s, 20), r32(s, 24));
    if !((bpp == 32 && (depth == 24 || depth == 32)) || (bpp == 24 && depth == 24)) {
        crate::debug_println!("drm: ADDFB refused bpp {} depth {} ({}x{} pitch {})", bpp, depth, width, height, pitch);
        return EINVAL;
    }
    match add_fb(id, handle, width, height, pitch, 0, bpp / 8) {
        Ok(fb) => {
            w32(s, 0, fb);
            0
        }
        Err(e) => e,
    }
}

fn add_fb2(id: u32, s: &mut [u8]) -> i64 {
    let (width, height, format, flags) = (r32(s, 4), r32(s, 8), r32(s, 12), r32(s, 16));
    let bytes = match format {
        XRGB8888 | ARGB8888 => 4,
        RGB888 | BGR888 => 3,
        _ => 0,
    };
    if bytes == 0 {
        crate::debug_println!("drm: ADDFB2 refused format {:#x} ({}x{})", format, width, height);
        return EINVAL;
    }
    if flags & 2 != 0 && s.len() >= 80 && r64(s, 72) != 0 {
        return EINVAL;
    }
    match add_fb(id, r32(s, 20), width, height, r32(s, 36), r32(s, 52), bytes) {
        Ok(fb) => {
            w32(s, 0, fb);
            0
        }
        Err(e) => e,
    }
}

fn rm_fb(id: u32, fb: u32) -> i64 {
    let segment = with(|d| {
        if d.fbs.get(&fb).map(|f| f.file) != Some(id) {
            return None;
        }
        if d.scanout.map(|s| s.fb) == Some(fb) {
            d.scanout = None;
        }
        d.fbs.remove(&fb).map(|f| f.segment)
    });
    match segment {
        Some(segment) => {
            ipc::release(segment);
            0
        }
        None => ENOENT,
    }
}

fn get_fb(s: &mut [u8]) -> i64 {
    let fb = with(|d| d.fbs.get(&r32(s, 0)).map(|f| (f.width, f.height, f.pitch, f.bytes)));
    let Some((width, height, pitch, bytes)) = fb else {
        return ENOENT;
    };
    w32(s, 4, width);
    w32(s, 8, height);
    w32(s, 12, pitch);
    w32(s, 16, bytes * 8);
    w32(s, 20, 24);
    w32(s, 24, 0);
    0
}

fn plane_resources(s: &mut [u8]) -> i64 {
    let r = put_ids(r64(s, 0), r32(s, 8), &[PLANE]);
    if r < 0 {
        return r;
    }
    w32(s, 8, 1);
    0
}

fn get_plane(s: &mut [u8]) -> i64 {
    if r32(s, 0) != PLANE {
        return ENOENT;
    }
    let scanout = with(|d| d.scanout);
    let r = put_ids(r64(s, 24), r32(s, 20), &[XRGB8888, ARGB8888]);
    if r < 0 {
        return r;
    }
    w32(s, 4, if scanout.is_some() { CRTC } else { 0 });
    w32(s, 8, scanout.map(|s| s.fb).unwrap_or(0));
    w32(s, 12, 1);
    w32(s, 16, 0);
    w32(s, 20, 2);
    0
}

fn edid_blob() -> Vec<u8> {
    if let Some(c) = crate::drivers::video::gpu::connectors().into_iter().find(|c| c.status != 0 && c.edid_len > 0) {
        let raw = crate::drivers::video::gpu::edid(c.id);
        if raw.len() >= 128 {
            return raw;
        }
    }
    let (width, height) = screen().unwrap_or((1024, 768));
    let mut e = alloc::vec![0u8; 128];
    e[0..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
    let id = (((b'H' - b'@') as u16) << 10) | (((b'M' - b'@') as u16) << 5) | ((b'X' - b'@') as u16);
    e[8..10].copy_from_slice(&id.to_be_bytes());
    e[10..12].copy_from_slice(&1u16.to_le_bytes());
    e[12..16].copy_from_slice(&1u32.to_le_bytes());
    e[16] = 1;
    e[17] = 36;
    e[18] = 1;
    e[19] = 4;
    e[20] = 0xa5;
    e[21] = (width * 265 / 10000) as u8;
    e[22] = (height * 265 / 10000) as u8;
    e[23] = 120;
    e[24] = 0x06;
    for block in 0..4 {
        let at = 54 + block * 18;
        e[at..at + 18].copy_from_slice(&[0, 0, 0, 0x10, 0, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20]);
    }
    e[54..72].copy_from_slice(&[0, 0, 0, 0xfc, 0, b'H', b'a', b'm', b'i', b'x', b'O', b'S', b' ', b'D', b'i', b's', b'p', b'\n']);
    let sum = e[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    e[127] = 0u8.wrapping_sub(sum);
    e
}

fn get_property(s: &mut [u8]) -> i64 {
    let id = r32(s, 16);
    let (name, flags, enums): (&str, u32, &[(u64, &str)]) = match id {
        PROP_DPMS => ("DPMS", PROP_ENUM, &[(0, "On"), (1, "Standby"), (2, "Suspend"), (3, "Off")]),
        PROP_EDID => ("EDID", PROP_BLOB | PROP_IMMUTABLE, &[]),
        PROP_TYPE => ("type", PROP_ENUM | PROP_IMMUTABLE, &[(0, "Overlay"), (1, "Primary"), (2, "Cursor")]),
        _ => return ENOENT,
    };
    if !enums.is_empty() && r32(s, 56) as usize >= enums.len() && r32(s, 60) as usize >= enums.len() {
        let values: Vec<u8> = enums.iter().flat_map(|(v, _)| v.to_le_bytes()).collect();
        if r64(s, 0) != 0 && copy_out(r64(s, 0), values.len() as u64, &values) < 0 {
            return -14;
        }
        let mut blobs = alloc::vec![0u8; enums.len() * 40];
        for (i, (v, n)) in enums.iter().enumerate() {
            blobs[i * 40..i * 40 + 8].copy_from_slice(&v.to_le_bytes());
            blobs[i * 40 + 8..i * 40 + 8 + n.len()].copy_from_slice(n.as_bytes());
        }
        if r64(s, 8) != 0 && copy_out(r64(s, 8), blobs.len() as u64, &blobs) < 0 {
            return -14;
        }
    }
    w32(s, 20, flags);
    s[24..56].fill(0);
    s[24..24 + name.len()].copy_from_slice(name.as_bytes());
    w32(s, 56, enums.len() as u32);
    w32(s, 60, enums.len() as u32);
    0
}

fn set_connector_property(s: &[u8]) -> i64 {
    if r32(s, 12) != CONNECTOR {
        return ENOENT;
    }
    match r32(s, 8) {
        PROP_DPMS => {
            let value = r64(s, 0);
            if value > 3 {
                return EINVAL;
            }
            with(|d| d.dpms = value);
            0
        }
        _ => EINVAL,
    }
}

fn get_blob(s: &mut [u8]) -> i64 {
    if r32(s, 0) != BLOB_EDID {
        return ENOENT;
    }
    let blob = edid_blob();
    let capacity = r32(s, 4) as usize;
    let ptr = r64(s, 8);
    if ptr != 0 && capacity >= blob.len() && copy_out(ptr, blob.len() as u64, &blob) < 0 {
        return -14;
    }
    w32(s, 4, blob.len() as u32);
    0
}

fn object_properties(s: &mut [u8]) -> i64 {
    let (object, kind) = (r32(s, 20), r32(s, 24));
    let props: Vec<(u32, u64)> = match (kind, object) {
        (OBJECT_CONNECTOR, CONNECTOR) => alloc::vec![(PROP_DPMS, with(|d| d.dpms)), (PROP_EDID, BLOB_EDID as u64)],
        (OBJECT_PLANE, PLANE) => alloc::vec![(PROP_TYPE, 1)],
        (_, CONNECTOR | CRTC | ENCODER | PLANE) | (0, _) => Vec::new(),
        _ => return ENOENT,
    };
    if r32(s, 16) as usize >= props.len() && !props.is_empty() {
        let ids: Vec<u32> = props.iter().map(|p| p.0).collect();
        let r = put_ids(r64(s, 0), props.len() as u32, &ids);
        if r < 0 {
            return r;
        }
        let values: Vec<u8> = props.iter().flat_map(|p| p.1.to_le_bytes()).collect();
        if r64(s, 8) != 0 && copy_out(r64(s, 8), values.len() as u64, &values) < 0 {
            return -14;
        }
    }
    w32(s, 16, props.len() as u32);
    0
}

fn wait_vblank(s: &mut [u8]) -> i64 {
    let frame = task::TICK_HZ / 60;
    let now = task::ticks();
    let target = (now / frame + 1) * frame;
    while task::ticks() < target {
        if crate::syscall::signal::pending() {
            return crate::syscall::signal::EINTR;
        }
        crate::tty::sleep_for_events((target - task::ticks()).max(1));
    }
    let sequence = with(|d| {
        d.sequence = d.sequence.wrapping_add(1);
        d.sequence
    });
    let ms = task::ticks() * 1000 / task::TICK_HZ;
    w32(s, 4, sequence);
    w64(s, 8, ms / 1000);
    w64(s, 16, (ms % 1000) * 1000);
    0
}

pub fn ioctl(id: u32, request: u64, arg: u64) -> i64 {
    if (request >> 8) & 0xFF != 0x64 {
        return crate::syscall::ENOTTY;
    }
    let nr = request & 0xFF;
    let size = ((request >> 16) & 0x3FFF) as usize;
    let needed = match nr {
        0x00 => 64,
        0x02 => 4,
        0x09 => 8,
        0x0C | 0x0D => 16,
        0x1E | 0x1F => 0,
        0x3A => 24,
        0xA0 => 64,
        0xA1 | 0xA2 => 104,
        0xA4 | 0xA5 => 32,
        0xA6 => 20,
        0xA7 => 80,
        0xAA => 64,
        0xAB | 0xAC => 16,
        0xBA => 24,
        0xAD | 0xAE => 28,
        0xAF => 4,
        0xB0 => 24,
        0xB1 => 24,
        0xB2 => 32,
        0xB3 => 16,
        0xB4 => 4,
        0xB5 => 16,
        0xB6 => 32,
        0xB8 => 72,
        0xB9 => 28,
        0xA3 | 0xBB => return ENXIO,
        _ => return EINVAL,
    };
    if size < needed {
        return EINVAL;
    }
    let mut s = if size == 0 {
        Vec::new()
    } else {
        match user_slice(arg, size as u64) {
            Ok(b) => b.to_vec(),
            Err(e) => return e,
        }
    };
    let r = match nr {
        0x00 => version(&mut s),
        0x02 => {
            w32(&mut s, 0, 0x4841_4d58);
            0
        }
        0x09 => destroy_dumb(id, r32(&s, 0)),
        0x0C => get_cap(&mut s),
        0x0D => set_client_cap(&s),
        0x1E | 0x1F => 0,
        0x3A => wait_vblank(&mut s),
        0xA0 => get_resources(&mut s),
        0xA1 => get_crtc(&mut s),
        0xA2 => set_crtc(&s),
        0xA6 => get_encoder(&mut s),
        0xA7 => get_connector(&mut s),
        0xAD => get_fb(&mut s),
        0xAE => add_fb1(id, &mut s),
        0xAF => rm_fb(id, r32(&s, 0)),
        0xB0 => page_flip(id, &s),
        0xB1 => dirty_fb(&s),
        0xB2 => create_dumb(id, &mut s),
        0xB3 => map_dumb(id, &mut s),
        0xB4 => destroy_dumb(id, r32(&s, 0)),
        0xB5 => plane_resources(&mut s),
        0xB6 => get_plane(&mut s),
        0xB8 => add_fb2(id, &mut s),
        0xA4 | 0xA5 => EINVAL,
        0xAA => get_property(&mut s),
        0xAB => set_connector_property(&s),
        0xAC => get_blob(&mut s),
        0xB9 => object_properties(&mut s),
        0xBA => 0,
        _ => EINVAL,
    };
    if r == 0 && request & (2 << 30) != 0 && size > 0 {
        let w = copy_out(arg, size as u64, &s);
        if w < 0 {
            return w;
        }
    }
    r
}
