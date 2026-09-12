pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_WRITEV: u64 = 20;
pub const SYS_GETUID: u64 = 102;
pub const SYS_GETEUID: u64 = 107;
pub const SYS_GETPID: u64 = 39;
pub const SYS_EXIT: u64 = 60;
pub const SYS_UNAME: u64 = 63;
pub const SYS_CLOCK_GETTIME: u64 = 228;
pub const SYS_EXIT_GROUP: u64 = 231;
pub const SYS_HAMIX_FBMAP: u64 = 9001;
pub const SYS_HAMIX_MOUSE: u64 = 9004;
pub const SYS_HAMIX_POLLKEY: u64 = 9005;
pub const SYS_HAMIX_RELEASE_FB: u64 = 9006;
pub const SYS_HAMIX_READKEY: u64 = 9002;
pub const SYS_HAMIX_TRUNCATE: u64 = 9003;

#[inline(always)]
unsafe fn syscall3(num: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
unsafe fn syscall1(num: u64, a1: u64) -> i64 {
    unsafe { syscall3(num, a1, 0, 0) }
}

#[inline(always)]
unsafe fn syscall0(num: u64) -> i64 {
    unsafe { syscall3(num, 0, 0, 0) }
}

pub fn write(fd: u64, buf: &[u8]) -> i64 {
    unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64) }
}

pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
    unsafe { syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn open(path: &str) -> i64 {
    let mut tmp = [0u8; 256];
    let bytes = path.as_bytes();
    let n = bytes.len().min(tmp.len() - 1);
    tmp[..n].copy_from_slice(&bytes[..n]);
    unsafe { syscall1(SYS_OPEN, tmp.as_ptr() as u64) }
}

/// Clears a file's contents so a following write() (which always appends,
/// see the kernel-side doc comment) effectively replaces them. Needed for
/// `hed`'s save; see SYS_HAMIX_TRUNCATE.
pub fn truncate(path: &str) -> i64 {
    let mut tmp = [0u8; 256];
    let bytes = path.as_bytes();
    let n = bytes.len().min(tmp.len() - 1);
    tmp[..n].copy_from_slice(&bytes[..n]);
    unsafe { syscall1(SYS_HAMIX_TRUNCATE, tmp.as_ptr() as u64) }
}

pub fn close(fd: u64) -> i64 {
    unsafe { syscall1(SYS_CLOSE, fd) }
}

pub fn lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    unsafe { syscall3(SYS_LSEEK, fd, offset as u64, whence) }
}

pub fn fstat_size(fd: u64) -> i64 {
    let mut buf = [0u8; 144];
    let r = unsafe { syscall3(SYS_FSTAT, fd, buf.as_mut_ptr() as u64, 0) };
    if r < 0 {
        return r;
    }
    i64::from_ne_bytes(buf[48..56].try_into().unwrap())
}

pub fn mmap_anon(len: usize) -> i64 {
    unsafe { syscall3(SYS_MMAP, 0, len as u64, 0) }
}

pub fn munmap(addr: u64, len: u64) -> i64 {
    unsafe { syscall3(SYS_MUNMAP, addr, len, 0) }
}

pub fn brk(addr: usize) -> i64 {
    unsafe { syscall1(SYS_BRK, addr as u64) }
}

pub fn getuid() -> i64 {
    unsafe { syscall0(SYS_GETUID) }
}

pub fn geteuid() -> i64 {
    unsafe { syscall0(SYS_GETEUID) }
}

pub fn getpid() -> i64 {
    unsafe { syscall0(SYS_GETPID) }
}

#[derive(Clone, Copy, Default)]
pub struct Timespec {
    pub sec: i64,
    pub nsec: i64,
}

pub fn clock_gettime() -> Timespec {
    let mut buf = [0u8; 16];
    unsafe { syscall3(SYS_CLOCK_GETTIME, 0, buf.as_mut_ptr() as u64, 0) };
    Timespec {
        sec: i64::from_ne_bytes(buf[0..8].try_into().unwrap()),
        nsec: i64::from_ne_bytes(buf[8..16].try_into().unwrap()),
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct HamixFbInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
}

pub fn fbmap(info: &mut HamixFbInfo) -> i64 {
    unsafe { syscall1(SYS_HAMIX_FBMAP, info as *mut HamixFbInfo as u64) }
}

pub const MOUSE_LEFT: u32 = 1;
pub const MOUSE_RIGHT: u32 = 2;
pub const MOUSE_MIDDLE: u32 = 4;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub buttons: u32,
    pub wheel: i32,
}

pub fn mouse(state: &mut MouseState) -> i64 {
    unsafe { syscall1(SYS_HAMIX_MOUSE, state as *mut MouseState as u64) }
}

pub fn release_framebuffer() {
    unsafe { syscall0(SYS_HAMIX_RELEASE_FB) };
}

pub fn poll_key() -> Option<Key> {
    let code = unsafe { syscall0(SYS_HAMIX_POLLKEY) };
    if code == -100 {
        return None;
    }
    Some(decode_key(code))
}

/// One decoded key press: printable ASCII and Enter/Backspace/Tab come
/// back as their byte value, navigation keys (which the generic
/// read(fd=0, ...) path can't represent in a byte stream) as small
/// negative codes. See `Key` for the full mapping. Blocks until a key is
/// available.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
}

pub fn read_key() -> Key {
    decode_key(unsafe { syscall0(SYS_HAMIX_READKEY) })
}

fn decode_key(code: i64) -> Key {
    match code {
        10 => Key::Enter,
        8 => Key::Backspace,
        9 => Key::Tab,
        -1 => Key::Up,
        -2 => Key::Down,
        -3 => Key::Left,
        -4 => Key::Right,
        -5 => Key::Home,
        -6 => Key::End,
        -7 => Key::Delete,
        c if (0..=255).contains(&c) => Key::Char(c as u8),
        _ => Key::Char(0),
    }
}

pub fn exit(code: i32) -> ! {
    unsafe {
        syscall1(SYS_EXIT, code as u32 as u64);
    }
    loop {
        unsafe { core::arch::asm!("pause", options(nomem, nostack)) };
    }
}
