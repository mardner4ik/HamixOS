use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

const EMPTY: u8 = 0;
const BUSY: u8 = 1;
const READY: u8 = 2;

pub struct OnceLock<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}
unsafe impl<T: Send> Send for OnceLock<T> {}

impl<T> OnceLock<T> {
    pub const fn new() -> Self {
        OnceLock { state: AtomicU8::new(EMPTY), value: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    pub fn get(&self) -> Option<&T> {
        if self.state.load(Ordering::Acquire) == READY {
            Some(unsafe { (*self.value.get()).assume_init_ref() })
        } else {
            None
        }
    }

    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        if let Some(v) = self.get() {
            return v;
        }
        if self.state.compare_exchange(EMPTY, BUSY, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            unsafe { (*self.value.get()).write(f()) };
            self.state.store(READY, Ordering::Release);
        } else {
            while self.state.load(Ordering::Acquire) != READY {
                core::hint::spin_loop();
            }
        }
        unsafe { (*self.value.get()).assume_init_ref() }
    }
}
