use crate::sys::{self, syscall3};

pub const SYS_HAMIX_TERMGEN: u64 = 9097;

pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 25;

pub fn size_of(fd: u64) -> Option<(u16, u16)> {
    sys::termsize(fd)
}

pub fn size() -> (u16, u16) {
    size_of(1).or_else(|| size_of(0)).unwrap_or((DEFAULT_COLS, DEFAULT_ROWS))
}

pub fn columns() -> usize {
    size().0 as usize
}

pub fn rows() -> usize {
    size().1 as usize
}

pub fn generation_of(fd: u64) -> i64 {
    unsafe { syscall3(SYS_HAMIX_TERMGEN, fd, 0, 0) }
}

pub fn generation() -> i64 {
    let value = generation_of(1);
    if value < 0 { generation_of(0) } else { value }
}

pub struct Size {
    pub cols: u16,
    pub rows: u16,
    generation: i64,
}

impl Default for Size {
    fn default() -> Self {
        Self::new()
    }
}

impl Size {
    pub fn new() -> Size {
        let (cols, rows) = size();
        Size { cols, rows, generation: generation() }
    }

    pub fn changed(&mut self) -> bool {
        let generation = generation();
        let (cols, rows) = size();
        if generation == self.generation && cols == self.cols && rows == self.rows {
            return false;
        }
        self.generation = generation;
        self.cols = cols;
        self.rows = rows;
        true
    }

    pub fn cols(&self) -> usize {
        self.cols as usize
    }

    pub fn rows(&self) -> usize {
        self.rows as usize
    }
}
