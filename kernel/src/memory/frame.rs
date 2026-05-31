use spin::Mutex;

const MAX_FRAMES: usize = 1 << 20;
const BITMAP_SIZE: usize = MAX_FRAMES / 64;
const PAGE_SIZE: usize = 4096;

pub struct BitmapFrameAllocator {
    bitmap: [u64; BITMAP_SIZE],
    next_free: usize,
    total_frames: usize,
    free_frames: usize,
}

impl BitmapFrameAllocator {
    pub const fn new() -> Self {
        Self {
            bitmap: [0u64; BITMAP_SIZE],
            next_free: 0,
            total_frames: 0,
            free_frames: 0,
        }
    }

    pub fn add_region(&mut self, base: usize, size: usize) {
        let start_frame = (base + PAGE_SIZE - 1) / PAGE_SIZE;
        let end_frame = (base + size) / PAGE_SIZE;
        for frame in start_frame..end_frame {
            if frame < MAX_FRAMES {
                self.set_free(frame);
                self.total_frames = self.total_frames.max(frame + 1);
                self.free_frames += 1;
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
        let start = self.next_free;
        let total = self.total_frames;
        let mut i = start;
        loop {
            if i >= total {
                i = 0;
            }
            if i == start && i != 0 {
                return None;
            }
            if self.is_free(i) {
                self.set_used(i);
                self.free_frames = self.free_frames.saturating_sub(1);
                self.next_free = (i + 1) % total;
                return Some(i * PAGE_SIZE);
            }
            i += 1;
            if total == 0 {
                return None;
            }
        }
    }

    pub fn free(&mut self, addr: usize) {
        let frame = addr / PAGE_SIZE;
        if frame < MAX_FRAMES && !self.is_free(frame) {
            self.set_free(frame);
            self.free_frames += 1;
        }
    }

    pub fn free_count(&self) -> usize {
        self.free_frames
    }

    pub fn total_count(&self) -> usize {
        self.total_frames
    }
}

static FRAME_ALLOCATOR: Mutex<BitmapFrameAllocator> = Mutex::new(BitmapFrameAllocator::new());

pub fn init(alloc: BitmapFrameAllocator) {
    *FRAME_ALLOCATOR.lock() = alloc;
}

pub fn alloc_frame() -> Option<usize> {
    FRAME_ALLOCATOR.lock().alloc()
}

pub fn free_frame(addr: usize) {
    FRAME_ALLOCATOR.lock().free(addr);
}

pub fn memory_info() -> (usize, usize) {
    let a = FRAME_ALLOCATOR.lock();
    (a.free_count() * 4096, a.total_count() * 4096)
}
