#![allow(dead_code)]

pub mod bit;
pub mod fft;
pub mod mdct;

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

pub mod io {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Error(pub &'static str);

    impl Error {
        pub fn other(message: &'static str) -> Error {
            Error(message)
        }
    }

    pub type Result<T> = core::result::Result<T, Error>;
}

pub mod errors {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        Decode(&'static str),
        Unsupported(&'static str),
        Bits(&'static str),
    }

    impl From<super::io::Error> for Error {
        fn from(e: super::io::Error) -> Error {
            Error::Bits(e.0)
        }
    }

    pub type Result<T> = core::result::Result<T, Error>;

    pub fn decode_error<T>(message: &'static str) -> Result<T> {
        Err(Error::Decode(message))
    }

    pub fn unsupported_error<T>(message: &'static str) -> Result<T> {
        Err(Error::Unsupported(message))
    }
}

pub mod bits {
    #[inline(always)]
    pub fn sign_extend_leq32_to_i32(value: u32, width: u32) -> i32 {
        (value.wrapping_shl(32 - width) as i32).wrapping_shr(32 - width)
    }

    #[inline(always)]
    pub fn sign_extend_leq64_to_i64(value: u64, width: u32) -> i64 {
        (value.wrapping_shl(64 - width) as i64).wrapping_shr(64 - width)
    }
}

pub mod complex {
    use core::ops::{Add, Mul, Sub};

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Complex<T> {
        pub re: T,
        pub im: T,
    }

    impl Complex<f32> {
        #[inline(always)]
        pub const fn new(re: f32, im: f32) -> Self {
            Complex { re, im }
        }

        #[inline(always)]
        pub fn conj(&self) -> Self {
            Complex { re: self.re, im: -self.im }
        }

        #[inline(always)]
        pub fn scale(&self, k: f32) -> Self {
            Complex { re: self.re * k, im: self.im * k }
        }
    }

    impl Add for Complex<f32> {
        type Output = Self;
        #[inline(always)]
        fn add(self, o: Self) -> Self {
            Complex { re: self.re + o.re, im: self.im + o.im }
        }
    }

    impl Sub for Complex<f32> {
        type Output = Self;
        #[inline(always)]
        fn sub(self, o: Self) -> Self {
            Complex { re: self.re - o.re, im: self.im - o.im }
        }
    }

    impl Mul for Complex<f32> {
        type Output = Self;
        #[inline(always)]
        fn mul(self, o: Self) -> Self {
            Complex { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
        }
    }

    impl Mul<f32> for Complex<f32> {
        type Output = Self;
        #[inline(always)]
        fn mul(self, k: f32) -> Self {
            Complex { re: self.re * k, im: self.im * k }
        }
    }
}

pub trait FloatExt: Sized {
    fn powf(self, e: Self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn sqrt(self) -> Self;
}

impl FloatExt for f32 {
    fn powf(self, e: f32) -> f32 {
        libm::powf(self, e)
    }
    fn sin(self) -> f32 {
        libm::sinf(self)
    }
    fn cos(self) -> f32 {
        libm::cosf(self)
    }
    fn sqrt(self) -> f32 {
        libm::sqrtf(self)
    }
}

impl FloatExt for f64 {
    fn powf(self, e: f64) -> f64 {
        libm::pow(self, e)
    }
    fn sin(self) -> f64 {
        libm::sin(self)
    }
    fn cos(self) -> f64 {
        libm::cos(self)
    }
    fn sqrt(self) -> f64 {
        libm::sqrt(self)
    }
}

pub struct Lazy<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
    init: fn() -> T,
}

unsafe impl<T: Send + Sync> Sync for Lazy<T> {}

impl<T> Lazy<T> {
    pub const fn new(init: fn() -> T) -> Self {
        Lazy { state: AtomicU8::new(0), value: UnsafeCell::new(MaybeUninit::uninit()), init }
    }

    pub fn get(&self) -> &T {
        if self.state.load(Ordering::Acquire) != 2 {
            if self.state.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                unsafe { (*self.value.get()).write((self.init)()) };
                self.state.store(2, Ordering::Release);
            } else {
                while self.state.load(Ordering::Acquire) != 2 {
                    core::hint::spin_loop();
                }
            }
        }
        unsafe { (*self.value.get()).assume_init_ref() }
    }
}

impl<T> core::ops::Deref for Lazy<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.get()
    }
}
