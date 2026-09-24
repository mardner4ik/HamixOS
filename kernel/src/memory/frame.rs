use spin::Mutex;

use crate::arch::without_interrupts;

const MAX_FRAMES: usize = 1 << 24;
const BITMAP_SIZE: usize = MAX_FRAMES / 64;
pub const PAGE_SIZE: usize = 4096;

pub struct BitmapFrameAllocator {
    bitmap: [u64; BITMAP_SIZE],
    next_free: usize,
    limit: usize,
    total_frames: usize,
    free_frames: usize,
}

impl BitmapFrameAllocator {
    pub const fn new() -> Self {
        Self {
            bitmap: [0u64; BITMAP_SIZE],
            next_free: 0,
            limit: 0,
            total_frames: 0,
            free_frames: 0,
        }
    }

    pub fn add_region(&mut self, base: usize, size: usize) {
        let start_frame = base.div_ceil(PAGE_SIZE);
        let end_frame = ((base + size) / PAGE_SIZE).min(MAX_FRAMES);
        for frame in start_frame..end_frame {
            if !self.is_free(frame) {
                self.set_free(frame);
                self.free_frames += 1;
                self.total_frames += 1;
            }
            self.limit = self.limit.max(frame + 1);
        }
    }

    pub fn reserve_region(&mut self, base: usize, size: usize) {
        let start_frame = base / PAGE_SIZE;
        let end_frame = (base + size).div_ceil(PAGE_SIZE).min(MAX_FRAMES);
        for frame in start_frame..end_frame {
            if self.is_free(frame) {
                self.set_used(frame);
                self.free_frames -= 1;
            }
        }
    }

    fn set_free(&mut self, frame: usize) {
        self.bitmap[frame / 64] |= 1 << (frame % 64);
    }

    fn set_used(&mut self, frame: usize) {
        self.bitmap[frame / 64] &= !(1 << (frame % 64));
    }

    fn is_free(&self, frame: usize) -> bool {
        self.bitmap[frame / 64] & (1 << (frame % 64)) != 0
    }

    pub fn alloc(&mut self) -> Option<usize> {
        if self.free_frames == 0 || self.limit == 0 {
            return None;
        }
        let words = self.limit.div_ceil(64);
        let start_word = (self.next_free / 64).min(words - 1);
        for step in 0..=words {
            let word = (start_word + step) % words;
            let bits = self.bitmap[word];
            if bits == 0 {
                continue;
            }
            let frame = word * 64 + bits.trailing_zeros() as usize;
            if frame >= self.limit {
                continue;
            }
            self.set_used(frame);
            self.free_frames -= 1;
            self.next_free = frame + 1;
            return Some(frame * PAGE_SIZE);
        }
        None
    }

    pub fn alloc_contiguous(&mut self, count: usize, below: usize) -> Option<usize> {
        let limit = self.limit.min(below / PAGE_SIZE);
        let mut run = 0usize;
        let mut frame = limit;
        while frame > 0 {
            frame -= 1;
            if self.is_free(frame) {
                run += 1;
                if run == count {
                    for f in frame..frame + count {
                        self.set_used(f);
                    }
                    self.free_frames -= count;
                    return Some(frame * PAGE_SIZE);
                }
            } else {
                run = 0;
            }
        }
        None
    }

    pub fn free(&mut self, addr: usize) {
        let frame = addr / PAGE_SIZE;
        if frame < MAX_FRAMES && !self.is_free(frame) {
            self.set_free(frame);
            self.free_frames += 1;
            if frame < self.next_free {
                self.next_free = frame;
            }
        }
    }
}

pub static FRAME_ALLOCATOR: Mutex<BitmapFrameAllocator> = Mutex::new(BitmapFrameAllocator::new());

pub fn init() {}

pub fn add_region(base: usize, size: usize) {
    without_interrupts(|| FRAME_ALLOCATOR.lock().add_region(base, size));
}

pub fn reserve_region(base: usize, size: usize) {
    without_interrupts(|| FRAME_ALLOCATOR.lock().reserve_region(base, size));
}

pub fn alloc_frame() -> Option<usize> {
    without_interrupts(|| FRAME_ALLOCATOR.lock().alloc())
}

pub fn alloc_zeroed_frame() -> Option<usize> {
    let frame = alloc_frame()?;
    unsafe { core::ptr::write_bytes(frame as *mut u8, 0, PAGE_SIZE) };
    Some(frame)
}

pub fn alloc_contiguous(count: usize, below: usize) -> Option<usize> {
    without_interrupts(|| FRAME_ALLOCATOR.lock().alloc_contiguous(count, below))
}

pub fn free_frame(addr: usize) {
    without_interrupts(|| FRAME_ALLOCATOR.lock().free(addr));
}

pub fn memory_info() -> (usize, usize) {
    without_interrupts(|| {
        let a = FRAME_ALLOCATOR.lock();
        (a.free_frames * PAGE_SIZE, a.total_frames * PAGE_SIZE)
    })
}
