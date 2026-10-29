use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use super::{Message, Pid, State, WAIT_MSG};
use crate::arch::paging::PAGE_SIZE;
use crate::arch::without_interrupts;
use crate::memory::frame;

pub const MAX_MESSAGE: usize = 4096;
const MAILBOX_LIMIT: usize = 512;

pub use crate::errno::ESRCH;
pub use crate::errno::EAGAIN;
pub use crate::errno::ENOMEM;
pub use crate::errno::EACCES;
pub use crate::errno::EEXIST;
pub use crate::errno::EINVAL;
pub use crate::errno::ENOENT;

struct Segment {
    frames: Vec<usize>,
    len: u64,
    refs: u32,
    owner: Pid,
    seals: u32,
}

const F_SEAL_SHRINK: u32 = 0x0002;
const EPERM: i64 = -1;

static SERVICES: Mutex<BTreeMap<String, Pid>> = Mutex::new(BTreeMap::new());
static ACTIVATORS: Mutex<BTreeMap<String, Pid>> = Mutex::new(BTreeMap::new());
const ACTIVATION_TIMEOUT_MS: u64 = 5000;
static SEGMENTS: Mutex<BTreeMap<u32, Segment>> = Mutex::new(BTreeMap::new());
static NEXT_SEGMENT: AtomicU32 = AtomicU32::new(0x5100);

pub fn send(from: Pid, to: Pid, data: &[u8]) -> i64 {
    if data.len() > MAX_MESSAGE {
        return EINVAL;
    }
    let message = Message { sender: from, data: data.to_vec() };
    let mut woke = false;
    let result = super::with_tasks(|tasks| match tasks.get_mut(&to) {
        Some(task) if task.state != State::Zombie => {
            if task.mailbox.len() >= MAILBOX_LIMIT {
                return EAGAIN;
            }
            task.mailbox.push_back(message);
            if task.state == State::Blocked && task.wait & WAIT_MSG != 0 {
                woke = true;
            }
            0
        }
        _ => ESRCH,
    });
    if woke {
        super::wake_task(to, WAIT_MSG);
    }
    result
}

pub fn take_message() -> Option<Message> {
    super::with_current(|task| task.mailbox.pop_front())
}

pub fn take_message_from(sender: Pid) -> Option<Message> {
    super::with_current(|task| {
        let index = task.mailbox.iter().position(|m| m.sender == sender)?;
        task.mailbox.remove(index)
    })
}

pub fn has_message() -> bool {
    super::with_current(|task| !task.mailbox.is_empty())
}

pub fn register_service(name: &str, pid: Pid) -> i64 {
    if name.is_empty() || name.len() > 64 {
        return EINVAL;
    }
    without_interrupts(|| {
        let mut services = SERVICES.lock();
        if let Some(existing) = services.get(name) {
            if *existing != pid && super::exists(*existing) {
                return EEXIST;
            }
        }
        services.insert(String::from(name), pid);
        0
    })
}

pub fn register_activator(name: &str, pid: Pid) -> i64 {
    if name.is_empty() || name.len() > 64 {
        return EINVAL;
    }
    without_interrupts(|| ACTIVATORS.lock().insert(String::from(name), pid));
    0
}

pub fn lookup_service_activating(name: &str) -> i64 {
    let found = lookup_service(name);
    if found != ENOENT {
        return found;
    }
    let activator = without_interrupts(|| ACTIVATORS.lock().get(name).copied());
    let Some(activator) = activator.filter(|p| super::exists(*p)) else {
        return ENOENT;
    };
    let request = alloc::format!("activate {}", name);
    if send(0, activator, request.as_bytes()) < 0 {
        return ENOENT;
    }
    let deadline = super::uptime_ms() + ACTIVATION_TIMEOUT_MS;
    while super::uptime_ms() < deadline {
        super::sleep_ticks((super::TICK_HZ / 50).max(1));
        super::check_killed();
        let found = lookup_service(name);
        if found != ENOENT {
            return found;
        }
    }
    ENOENT
}

pub fn lookup_service(name: &str) -> i64 {
    let pid = without_interrupts(|| SERVICES.lock().get(name).copied());
    match pid {
        Some(pid) if super::exists(pid) => pid as i64,
        _ => ENOENT,
    }
}

pub fn shm_create(owner: Pid, len: u64) -> i64 {
    if len == 0 || len > 256 * 1024 * 1024 {
        return EINVAL;
    }
    let count = len.div_ceil(PAGE_SIZE) as usize;
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        match frame::alloc_zeroed_frame() {
            Some(f) => frames.push(f),
            None => {
                for f in frames {
                    frame::free_frame(f);
                }
                return ENOMEM;
            }
        }
    }
    let id = NEXT_SEGMENT.fetch_add(1, Ordering::Relaxed);
    without_interrupts(|| SEGMENTS.lock().insert(id, Segment { frames, len, refs: 0, owner, seals: 0 }));
    id as i64
}

pub fn shm_map(pid: Pid, id: u32) -> Result<(u64, u64), i64> {
    let display = super::display::owner();
    let (frames, len) = without_interrupts(|| {
        let mut segments = SEGMENTS.lock();
        let segment = segments.get_mut(&id).ok_or(ENOENT)?;
        if segment.owner != pid && display != Some(pid) {
            return Err(EACCES);
        }
        segment.refs += 1;
        Ok((segment.frames.clone(), segment.len))
    })?;
    let mapped = super::with_task(pid, |task| {
        let Some(aspace) = task.aspace.as_mut() else {
            return None;
        };
        let base = task.shm_next;
        for (i, f) in frames.iter().enumerate() {
            if !aspace.map_shared(base + i as u64 * PAGE_SIZE, *f as u64, false) {
                return None;
            }
        }
        task.shm_next = base + (frames.len() as u64 + 1) * PAGE_SIZE;
        task.shm.push((id, base, len));
        Some(base)
    })
    .flatten();
    match mapped {
        Some(base) => Ok((base, len)),
        None => {
            drop_ref(id);
            Err(ENOMEM)
        }
    }
}

pub fn memfd_create(owner: Pid) -> u32 {
    let id = NEXT_SEGMENT.fetch_add(1, Ordering::Relaxed);
    without_interrupts(|| SEGMENTS.lock().insert(id, Segment { frames: Vec::new(), len: 0, refs: 1, owner, seals: 0 }));
    id
}

pub fn retain(id: u32) {
    without_interrupts(|| {
        if let Some(segment) = SEGMENTS.lock().get_mut(&id) {
            segment.refs += 1;
        }
    });
}

pub fn release(id: u32) {
    drop_ref(id);
}

pub fn shm_len(id: u32) -> Option<u64> {
    without_interrupts(|| SEGMENTS.lock().get(&id).map(|s| s.len))
}

pub fn shm_add_seals(id: u32, seals: u32) -> i64 {
    if seals & !F_SEAL_SHRINK != 0 {
        return EINVAL;
    }
    without_interrupts(|| match SEGMENTS.lock().get_mut(&id) {
        Some(segment) => {
            segment.seals |= seals;
            0
        }
        None => ENOENT,
    })
}

pub fn shm_seals(id: u32) -> Option<u32> {
    without_interrupts(|| SEGMENTS.lock().get(&id).map(|segment| segment.seals))
}

pub fn shm_set_len(id: u32, len: u64) -> i64 {
    if len > 1 << 30 {
        return EINVAL;
    }
    let wanted = len.div_ceil(PAGE_SIZE) as usize;
    let have = without_interrupts(|| SEGMENTS.lock().get(&id).map(|s| s.frames.len()));
    let Some(have) = have else {
        return ENOENT;
    };
    let mut extra = Vec::new();
    for _ in have..wanted {
        match frame::alloc_zeroed_frame() {
            Some(f) => extra.push(f),
            None => {
                for f in extra {
                    frame::free_frame(f);
                }
                return ENOMEM;
            }
        }
    }
    let (leftover, error) = without_interrupts(|| {
        let mut segments = SEGMENTS.lock();
        let Some(segment) = segments.get_mut(&id) else {
            return (extra, ENOENT);
        };
        if segment.len > len && segment.seals & F_SEAL_SHRINK != 0 {
            return (extra, EPERM);
        }
        if segment.len > len {
            let keep = len as usize;
            for (i, f) in segment.frames.iter().enumerate() {
                let page_start = i * PAGE_SIZE as usize;
                let page_end = page_start + PAGE_SIZE as usize;
                if page_end <= keep {
                    continue;
                }
                let from = keep.saturating_sub(page_start);
                unsafe { core::ptr::write_bytes((*f + from) as *mut u8, 0, PAGE_SIZE as usize - from) };
            }
        }
        segment.frames.extend(extra);
        segment.len = len;
        (Vec::new(), 0)
    });
    for f in leftover {
        frame::free_frame(f);
    }
    error
}

pub fn shm_frames(id: u32) -> Option<(Vec<usize>, u64)> {
    without_interrupts(|| SEGMENTS.lock().get(&id).map(|s| (s.frames.clone(), s.len)))
}

pub fn shm_io(id: u32, offset: u64, buf: &mut [u8], write: bool) -> Result<usize, i64> {
    without_interrupts(|| {
        let segments = SEGMENTS.lock();
        let segment = segments.get(&id).ok_or(ENOENT)?;
        let len = segment.len;
        if offset >= len {
            return Ok(0);
        }
        let n = (len - offset).min(buf.len() as u64) as usize;
        let mut done = 0usize;
        while done < n {
            let pos = offset as usize + done;
            let frame = segment.frames[pos / PAGE_SIZE as usize];
            let within = pos % PAGE_SIZE as usize;
            let step = (PAGE_SIZE as usize - within).min(n - done);
            unsafe {
                let ptr = (frame + within) as *mut u8;
                if write {
                    core::ptr::copy_nonoverlapping(buf.as_ptr().add(done), ptr, step);
                } else {
                    core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr().add(done), step);
                }
            }
            done += step;
        }
        Ok(n)
    })
}

pub fn shm_map_at(pid: Pid, id: u32, fixed: Option<u64>, offset: u64, len: u64) -> Result<u64, i64> {
    let first = (offset / PAGE_SIZE) as usize;
    let count = len.div_ceil(PAGE_SIZE) as usize;
    let frames = without_interrupts(|| {
        let mut segments = SEGMENTS.lock();
        let segment = segments.get_mut(&id)?;
        if first + count > segment.frames.len() {
            return None;
        }
        segment.refs += 1;
        Some(segment.frames[first..first + count].to_vec())
    })
    .ok_or(EINVAL)?;
    let mapped = super::with_task(pid, |task| {
        let base = fixed.unwrap_or(task.shm_next);
        if fixed.is_none() && base + (count as u64 + 1) * PAGE_SIZE > crate::arch::paging::USER_FB_BASE {
            return None;
        }
        let aspace = task.aspace.as_mut()?;
        if fixed.is_some() {
            aspace.unmap_range(base, count as u64 * PAGE_SIZE);
        }
        for (i, f) in frames.iter().enumerate() {
            if !aspace.map_shared(base + i as u64 * PAGE_SIZE, *f as u64, false) {
                return None;
            }
        }
        if fixed.is_none() {
            task.shm_next = base + (count as u64 + 1) * PAGE_SIZE;
        }
        task.shm.push((id, base, count as u64 * PAGE_SIZE));
        Some(base)
    })
    .flatten();
    crate::memory::tlb::sync();
    match mapped {
        Some(base) => Ok(base),
        None => {
            drop_ref(id);
            Err(ENOMEM)
        }
    }
}

pub fn unmap_range(pid: Pid, start: u64, len: u64) -> bool {
    let end = start + len;
    let removed: Vec<u32> = super::with_task(pid, |task| {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < task.shm.len() {
            let (id, base, size) = task.shm[i];
            if base >= start && base + size <= end {
                if let Some(aspace) = task.aspace.as_mut() {
                    aspace.unmap_range(base, size);
                }
                task.shm.remove(i);
                removed.push(id);
            } else {
                i += 1;
            }
        }
        removed
    })
    .unwrap_or_default();
    crate::memory::tlb::sync();
    let any = !removed.is_empty();
    for id in removed {
        drop_ref(id);
    }
    any
}

fn drop_ref(id: u32) {
    let freed = without_interrupts(|| {
        let mut segments = SEGMENTS.lock();
        let segment = segments.get_mut(&id)?;
        segment.refs = segment.refs.saturating_sub(1);
        if segment.refs == 0 {
            return segments.remove(&id).map(|s| s.frames);
        }
        None
    });
    if let Some(frames) = freed {
        for f in frames {
            frame::free_frame(f);
        }
    }
}

pub fn shm_release(pid: Pid, id: u32) -> i64 {
    let entry = super::with_task(pid, |task| {
        let index = task.shm.iter().position(|(sid, _, _)| *sid == id)?;
        let (_, base, len) = task.shm.remove(index);
        if let Some(aspace) = task.aspace.as_mut() {
            aspace.unmap_range(base, len);
        }
        Some(())
    })
    .flatten();
    crate::memory::tlb::sync();
    match entry {
        Some(()) => {
            drop_ref(id);
            0
        }
        None => ENOENT,
    }
}

pub fn cleanup(pid: Pid) {
    without_interrupts(|| SERVICES.lock().retain(|_, owner| *owner != pid));
    let mapped: Vec<u32> = super::with_task(pid, |task| {
        let ids: Vec<u32> = task.shm.iter().map(|(id, _, _)| *id).collect();
        task.shm.clear();
        ids
    })
    .unwrap_or_default();
    let mut to_free = Vec::new();
    without_interrupts(|| {
        let mut segments = SEGMENTS.lock();
        for id in mapped {
            if let Some(segment) = segments.get_mut(&id) {
                segment.refs = segment.refs.saturating_sub(1);
            }
        }
        let dead: Vec<u32> = segments
            .iter()
            .filter(|(_, s)| s.refs == 0 && (s.owner == pid || !super::exists(s.owner)))
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            if let Some(segment) = segments.remove(&id) {
                to_free.push(segment.frames);
            }
        }
    });
    for frames in to_free {
        for f in frames {
            frame::free_frame(f);
        }
    }
}
