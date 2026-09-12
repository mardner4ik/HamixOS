use alloc::string::String;
use alloc::vec::Vec;
use core::slice;
use spin::Mutex;

use crate::drivers::tty::{CONSOLE, COLOR_FG};
use crate::fs;

const SYSCALL_STACK_SIZE: usize = 4096 * 4;

#[repr(C, align(16))]
struct SyscallStacks([[u8; SYSCALL_STACK_SIZE]; crate::vt::VT_COUNT]);

static mut SYSCALL_STACKS: SyscallStacks =
    SyscallStacks([[0u8; SYSCALL_STACK_SIZE]; crate::vt::VT_COUNT]);

pub(crate) fn syscall_stack_top(vt: usize) -> u64 {
    let base = unsafe {
        (&raw const SYSCALL_STACKS).cast::<u8>().add(vt * SYSCALL_STACK_SIZE) as u64
            + SYSCALL_STACK_SIZE as u64
    };
    base & !0xF
}

pub(crate) fn set_current_task(vt: usize) {
    unsafe { PER_CPU.kernel_rsp = syscall_stack_top(vt) };
}

#[repr(C)]
struct PerCpu {
    kernel_rsp: u64,
    user_rsp_scratch: u64,
}

static mut PER_CPU: PerCpu = PerCpu {
    kernel_rsp: 0,
    user_rsp_scratch: 0,
};

pub fn init() {
    use crate::arch::x86_64::{read_msr, write_msr};
    const IA32_EFER: u32 = 0xC0000080;
    const IA32_STAR: u32 = 0xC0000081;
    const IA32_LSTAR: u32 = 0xC0000082;
    const IA32_FMASK: u32 = 0xC0000084;
    const IA32_GS_BASE: u32 = 0xC0000101;
    const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;

    unsafe {
        PER_CPU.kernel_rsp = syscall_stack_top(0);
    }

    let efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer | 1);

    write_msr(IA32_STAR, (0x0008u64 << 32) | (0x0018u64 << 48));
    write_msr(IA32_LSTAR, syscall_entry as *const () as u64);
    write_msr(IA32_FMASK, 0x200);

    write_msr(IA32_GS_BASE, 0);
    write_msr(IA32_KERNEL_GS_BASE, (&raw const PER_CPU) as u64);
}



const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_FSTAT: u64 = 5;
const SYS_LSEEK: u64 = 8;
const SYS_MMAP: u64 = 9;
const SYS_MUNMAP: u64 = 11;
const SYS_BRK: u64 = 12;
const SYS_RT_SIGACTION: u64 = 13;
const SYS_RT_SIGPROCMASK: u64 = 14;
const SYS_IOCTL: u64 = 16;
const SYS_WRITEV: u64 = 20;
const SYS_GETUID: u64 = 102;
const SYS_GETEUID: u64 = 107;
const SYS_GETPID: u64 = 39;
const SYS_EXIT: u64 = 60;
const SYS_UNAME: u64 = 63;
const SYS_GETTID: u64 = 186;
const SYS_SET_TID_ADDRESS: u64 = 218;
const SYS_CLOCK_GETTIME: u64 = 228;
const SYS_ARCH_PRCTL: u64 = 158;
const SYS_EXIT_GROUP: u64 = 231;
const SYS_HAMIX_FBMAP: u64 = 9001;
const SYS_HAMIX_READKEY: u64 = 9002;
const SYS_HAMIX_TRUNCATE: u64 = 9003;
const SYS_HAMIX_MOUSE: u64 = 9004;
const SYS_HAMIX_POLLKEY: u64 = 9005;
const SYS_HAMIX_RELEASE_FB: u64 = 9006;

const ENOSYS: i64 = -38;
const EBADF: i64 = -9;
const ENOENT: i64 = -2;
const EINVAL: i64 = -22;
const EMFILE: i64 = -24;
const ENODEV: i64 = -19;

struct OpenFile {
    path: String,
    pos: usize,
}

const MAX_FDS: usize = 32;
const FIRST_USER_FD: usize = 3;
static FD_TABLE: Mutex<[Option<OpenFile>; MAX_FDS]> = Mutex::new([const { None }; MAX_FDS]);

const USER_HEAP_SIZE: usize = 16 * 1024 * 1024;
// Placed in its own 2MB-aligned linker section (see kernel/linker.ld) so
// that granting ring-3 access to it can never accidentally also expose
// page 0 or other kernel data at the same 2MB-page granularity.
#[unsafe(link_section = ".user_heap")]
static mut USER_HEAP: [u8; USER_HEAP_SIZE] = [0u8; USER_HEAP_SIZE];
static BRK: Mutex<usize> = Mutex::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn handle_syscall(syscall_number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    if crate::vt::kill_pending() {
        crate::vt::terminate_current_ring3();
    }
    crate::vt::service_pending_foreground_switch();
    match syscall_number {
        SYS_READ => unsafe { sys_read(arg1, arg2 as *mut u8, arg3 as usize) },
        SYS_WRITE => unsafe { sys_write(arg1, arg2 as *const u8, arg3 as usize) },
        SYS_OPEN => unsafe { sys_open(arg1 as *const u8) },
        SYS_CLOSE => sys_close(arg1),
        SYS_FSTAT => unsafe { sys_fstat(arg1, arg2 as *mut u8) },
        SYS_LSEEK => sys_lseek(arg1, arg2 as i64, arg3),
        SYS_MMAP => sys_mmap(arg2 as usize),
        SYS_MUNMAP => 0,
        SYS_BRK => sys_brk(arg1 as usize),
        SYS_RT_SIGACTION => 0,
        SYS_RT_SIGPROCMASK => 0,
        SYS_IOCTL => 0,
        SYS_WRITEV => unsafe { sys_writev(arg1, arg2 as *const u8, arg3 as usize) },
        SYS_GETUID => crate::users::ROOT_UID as i64,
        SYS_GETEUID => crate::users::ROOT_UID as i64,
        SYS_GETPID => 1,
        SYS_GETTID => 1,
        SYS_SET_TID_ADDRESS => 1,
        SYS_CLOCK_GETTIME => unsafe { sys_clock_gettime(arg2 as *mut u8) },
        SYS_UNAME => unsafe { sys_uname(arg1 as *mut u8) },
        SYS_ARCH_PRCTL => 0,
        SYS_HAMIX_FBMAP => unsafe { sys_hamix_fbmap(arg1 as *mut u8) },
        SYS_HAMIX_MOUSE => unsafe { sys_hamix_mouse(arg1 as *mut u8) },
        SYS_HAMIX_POLLKEY => sys_hamix_pollkey(),
        SYS_HAMIX_RELEASE_FB => {
            release_framebuffer();
            0
        }
        SYS_HAMIX_READKEY => sys_hamix_readkey(),
        SYS_HAMIX_TRUNCATE => unsafe { sys_hamix_truncate(arg1 as *const u8) },
        SYS_EXIT | SYS_EXIT_GROUP => {
            kernel_terminate_current_process(arg1 as i32);
            let vt = crate::vt::ring3_owner_or_current();
            crate::vt::release_ring3(vt);
            // Long-jumps straight back to whoever called
            // enter_user_mode_and_return (see task::usermode); does not
            // return here, so `syscall_entry`'s sysretq is never reached.
            unsafe { crate::task::usermode::resume_kernel(arg1 as i32, crate::vt::coro_slot_ptr(vt)) }
        }
        _ => ENOSYS,
    }
}

fn kernel_print_slice(slice: &[u8]) {
    if crate::drivers::video::text_mode::graphics_owned() {
        if let Ok(text) = core::str::from_utf8(slice) {
            crate::serial_print!("{}", text);
        }
        return;
    }
    let mut console = CONSOLE.lock();
    match core::str::from_utf8(slice) {
        Ok(s) => console.write_str_colored(s, COLOR_FG),
        Err(_) => {
            for &b in slice {
                console.write_char_colored(b as char, COLOR_FG);
            }
        }
    }
}

fn kernel_terminate_current_process(code: i32) {
    crate::serial_println!("process exited with code {}", code);
    crate::drivers::klog::log("process exited");
}

unsafe fn sys_write(fd: u64, buf: *const u8, count: usize) -> i64 {
    if buf.is_null() {
        return EINVAL;
    }
    let data = unsafe { slice::from_raw_parts(buf, count) };

    if fd == 1 || fd == 2 {
        kernel_print_slice(data);
        return count as i64;
    }

    let mut table = FD_TABLE.lock();
    if let Some(slot) = table.get_mut(fd as usize) {
        if let Some(file) = slot {
            if let Some(vfs) = fs::VFS.lock().as_mut() {
                let root = vfs.root_id();
                return match vfs.write(root, &file.path, data, true, crate::users::ROOT_UID) {
                    Ok(()) => {
                        file.pos += data.len();
                        data.len() as i64
                    }
                    Err(_) => EBADF,
                };
            }
        }
    }
    EBADF
}

unsafe fn sys_read(fd: u64, buf: *mut u8, count: usize) -> i64 {
    if buf.is_null() {
        return EINVAL;
    }
    let out = unsafe { slice::from_raw_parts_mut(buf, count) };

    if fd == 0 {
        use crate::drivers::input::keyboard::{self, Key};
        let mut n = 0usize;
        while n < count {
            match keyboard::read_key_blocking_ring3() {
                Key::Enter => {
                    out[n] = b'\n';
                    n += 1;
                    break;
                }
                Key::Char(ch) if ch.is_ascii() => {
                    out[n] = ch as u8;
                    n += 1;
                }
                _ => {}
            }
        }
        return n as i64;
    }

    let mut table = FD_TABLE.lock();
    if let Some(slot) = table.get_mut(fd as usize) {
        if let Some(file) = slot {
            if let Some(vfs) = fs::VFS.lock().as_ref() {
                let root = vfs.root_id();
                if let Ok(data) = vfs.read(root, &file.path) {
                    if file.pos >= data.len() {
                        return 0;
                    }
                    let n = (data.len() - file.pos).min(count);
                    out[..n].copy_from_slice(&data[file.pos..file.pos + n]);
                    file.pos += n;
                    return n as i64;
                }
            }
        }
    }
    EBADF
}

unsafe fn read_cstr(ptr: *const u8) -> String {
    let mut len = 0usize;
    unsafe {
        while len < 4096 && *ptr.add(len) != 0 {
            len += 1;
        }
        let bytes = slice::from_raw_parts(ptr, len);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

unsafe fn sys_open(path_ptr: *const u8) -> i64 {
    if path_ptr.is_null() {
        return EINVAL;
    }
    let path = unsafe { read_cstr(path_ptr) };

    let mut table = FD_TABLE.lock();
    let slot_idx = match table.iter().skip(FIRST_USER_FD).position(|f| f.is_none()) {
        Some(idx) => idx + FIRST_USER_FD,
        None => return EMFILE,
    };

    if let Some(vfs) = fs::VFS.lock().as_mut() {
        let root = vfs.root_id();
        if !vfs.exists(root, &path) && vfs.create_file(root, &path, Vec::new(), crate::users::ROOT_UID).is_err() {
            return ENOENT;
        }
    } else {
        return ENOENT;
    }

    table[slot_idx] = Some(OpenFile { path, pos: 0 });
    slot_idx as i64
}

unsafe fn sys_fstat(fd: u64, statbuf: *mut u8) -> i64 {
    if statbuf.is_null() {
        return EINVAL;
    }
    let mut table = FD_TABLE.lock();
    let size = match table.get_mut(fd as usize) {
        Some(Some(file)) => {
            if let Some(vfs) = fs::VFS.lock().as_ref() {
                let root = vfs.root_id();
                match vfs.read(root, &file.path) {
                    Ok(data) => data.len() as u64,
                    Err(_) => return ENOENT,
                }
            } else {
                return ENOENT;
            }
        }
        _ => return EBADF,
    };
    unsafe {
        core::ptr::write_bytes(statbuf, 0, 144);
        core::ptr::write_unaligned(statbuf.add(24) as *mut u32, 0o100644);
        core::ptr::write_unaligned(statbuf.add(16) as *mut u64, 1);
        core::ptr::write_unaligned(statbuf.add(48) as *mut u64, size);
        core::ptr::write_unaligned(statbuf.add(56) as *mut u64, 4096u64);
        core::ptr::write_unaligned(statbuf.add(64) as *mut u64, size.div_ceil(512));
    }
    0
}

fn sys_lseek(fd: u64, offset: i64, whence: u64) -> i64 {
    let mut table = FD_TABLE.lock();
    let file = match table.get_mut(fd as usize) {
        Some(Some(f)) => f,
        _ => return EBADF,
    };
    let len = if let Some(vfs) = fs::VFS.lock().as_ref() {
        let root = vfs.root_id();
        vfs.read(root, &file.path).map(|d| d.len()).unwrap_or(0)
    } else {
        0
    };
    let base: i64 = match whence {
        0 => 0,
        1 => file.pos as i64,
        2 => len as i64,
        _ => return EINVAL,
    };
    let new_pos = base + offset;
    if new_pos < 0 {
        return EINVAL;
    }
    file.pos = new_pos as usize;
    new_pos
}

unsafe fn sys_writev(fd: u64, iov: *const u8, iovcnt: usize) -> i64 {
    if iov.is_null() {
        return EINVAL;
    }
    let mut total: i64 = 0;
    for i in 0..iovcnt {
        let entry = unsafe { iov.add(i * 16) };
        let base = unsafe { core::ptr::read_unaligned(entry as *const u64) } as *const u8;
        let len = unsafe { core::ptr::read_unaligned(entry.add(8) as *const u64) } as usize;
        if len == 0 {
            continue;
        }
        let n = unsafe { sys_write(fd, base, len) };
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
    }
    total
}

unsafe fn sys_clock_gettime(ts: *mut u8) -> i64 {
    if ts.is_null() {
        return EINVAL;
    }
    let ticks = crate::task::uptime_ticks();
    let secs = (ticks / 100) as i64;
    let nanos = ((ticks % 100) * 10_000_000) as i64;
    unsafe {
        core::ptr::write_unaligned(ts as *mut i64, secs);
        core::ptr::write_unaligned(ts.add(8) as *mut i64, nanos);
    }
    0
}

unsafe fn sys_hamix_fbmap(buf_ptr: *mut u8) -> i64 {
    if buf_ptr.is_null() {
        return EINVAL;
    }
    let fb = match *crate::memory::FRAMEBUFFER.lock() {
        Some(fb) => fb,
        None => return ENODEV,
    };
    let region_len = (fb.pitch as u64) * (fb.height as u64);
    crate::arch::x86_64::paging::allow_user_access(fb.addr, region_len.max(1));
    crate::drivers::video::console_mouse::disable();
    crate::drivers::video::text_mode::set_graphics_owned(true);
    crate::drivers::input::mouse::drain_events();
    unsafe {
        core::ptr::write_unaligned(buf_ptr as *mut u64, fb.addr);
        core::ptr::write_unaligned(buf_ptr.add(8) as *mut u32, fb.pitch);
        core::ptr::write_unaligned(buf_ptr.add(12) as *mut u32, fb.width);
        core::ptr::write_unaligned(buf_ptr.add(16) as *mut u32, fb.height);
        core::ptr::write_unaligned(buf_ptr.add(20) as *mut u32, fb.bpp as u32);
    }
    0
}

/// Encodes one key press as an i64 so a single syscall return value can
/// carry it: printable/control ASCII as its byte value (Enter=10,
/// Backspace=8, Tab=9), navigation keys the plain `read()`/fd-0 path
/// silently drops (see sys_read above) as small negative codes. Blocks
/// until a key is available, same as the fd-0 read path does.
pub fn release_framebuffer() {
    if !crate::drivers::video::text_mode::graphics_owned() {
        return;
    }
    crate::drivers::video::text_mode::set_graphics_owned(false);
    crate::drivers::video::text_mode::fb_clear_full();
    crate::drivers::video::text_mode::fb_redraw_all();
    crate::drivers::video::console_mouse::enable();
}

unsafe fn sys_hamix_mouse(buf_ptr: *mut u8) -> i64 {
    if buf_ptr.is_null() {
        return EINVAL;
    }
    let state = crate::drivers::input::mouse::take_state();
    unsafe {
        core::ptr::write_unaligned(buf_ptr as *mut i32, state.x);
        core::ptr::write_unaligned(buf_ptr.add(4) as *mut i32, state.y);
        core::ptr::write_unaligned(buf_ptr.add(8) as *mut u32, state.buttons as u32);
        core::ptr::write_unaligned(buf_ptr.add(12) as *mut i32, state.wheel);
    }
    if crate::drivers::input::mouse::present() { 0 } else { ENODEV }
}

fn sys_hamix_pollkey() -> i64 {
    use crate::drivers::input::keyboard::{self, Key};
    if crate::vt::kill_pending() {
        crate::vt::terminate_current_ring3();
    }
    crate::vt::service_pending_foreground_switch();
    match keyboard::read_key() {
        None => -100,
        Some(Key::Char(ch)) if ch.is_ascii() => ch as i64,
        Some(Key::Char(_)) => -100,
        Some(Key::Ctrl(ch)) => (ch as u8 & 0x1F) as i64,
        Some(Key::Enter) => 10,
        Some(Key::Backspace) => 8,
        Some(Key::Tab) => 9,
        Some(Key::Up) => -1,
        Some(Key::Down) => -2,
        Some(Key::Left) => -3,
        Some(Key::Right) => -4,
        Some(Key::Home) => -5,
        Some(Key::End) => -6,
        Some(Key::Delete) => -7,
    }
}

fn sys_hamix_readkey() -> i64 {
    use crate::drivers::input::keyboard::{self, Key};
    loop {
        match keyboard::read_key_blocking_ring3() {
            Key::Char(ch) if ch.is_ascii() => return ch as i64,
            Key::Char(_) => continue,
            Key::Ctrl(ch) => return (ch as u8 & 0x1F) as i64,
            Key::Enter => return 10,
            Key::Backspace => return 8,
            Key::Tab => return 9,
            Key::Up => return -1,
            Key::Down => return -2,
            Key::Left => return -3,
            Key::Right => return -4,
            Key::Home => return -5,
            Key::End => return -6,
            Key::Delete => return -7,
        }
    }
}

/// sys_write always appends for regular files (see fs::Vfs::write) rather
/// than respecting the fd's seek position, so there's no way to *replace*
/// a file's contents through the normal open+write path. `hed`'s save
/// needs exactly that, so this gives it a minimal, narrowly-scoped way to
/// clear a file before appending the new contents, without changing
/// sys_write's existing (already relied upon) append behavior for anyone
/// else.
unsafe fn sys_hamix_truncate(path_ptr: *const u8) -> i64 {
    if path_ptr.is_null() {
        return EINVAL;
    }
    let path = unsafe { read_cstr(path_ptr) };
    if let Some(vfs) = fs::VFS.lock().as_mut() {
        let root = vfs.root_id();
        return match vfs.write(root, &path, &[], false, crate::users::ROOT_UID) {
            Ok(()) => 0,
            Err(_) => EBADF,
        };
    }
    ENODEV
}

fn sys_close(fd: u64) -> i64 {
    let idx = fd as usize;
    let mut table = FD_TABLE.lock();
    if idx < MAX_FDS && table[idx].is_some() {
        table[idx] = None;
        return 0;
    }
    EBADF
}

fn sys_brk(addr: usize) -> i64 {
    let base = core::ptr::addr_of!(USER_HEAP) as usize;
    let mut brk = BRK.lock();
    if *brk == 0 {
        *brk = base;
        // USER_HEAP lives in the kernel's own .bss, which boot.S maps
        // supervisor-only. Without this, the very first write a user
        // process makes into memory returned by brk() (e.g. any Vec/String
        // growth) takes a ring-3 protection page fault, since the page is
        // present but not marked user-accessible.
        crate::arch::x86_64::paging::allow_user_access(base as u64, USER_HEAP_SIZE as u64);
    }
    if addr == 0 {
        return *brk as i64;
    }
    if addr >= base && addr <= base + USER_HEAP_SIZE {
        *brk = addr;
    }
    *brk as i64
}

fn sys_mmap(len: usize) -> i64 {
    use core::alloc::Layout;
    let layout = match Layout::from_size_align(len.max(4096), 4096) {
        Ok(l) => l,
        Err(_) => return -1,
    };
    unsafe {
        let ptr = alloc::alloc::alloc_zeroed(layout);
        if ptr.is_null() { -1 } else { ptr as i64 }
    }
}

unsafe fn sys_uname(buf: *mut u8) -> i64 {
    if buf.is_null() {
        return EINVAL;
    }
    const FIELD: usize = 65;
    let fields: [&[u8]; 6] = [
        b"HamixOS",
        b"hamix",
        b"0.1.0",
        b"#1 SMP HamixOS",
        b"x86_64",
        b"hamix.localdomain",
    ];
    unsafe {
        core::ptr::write_bytes(buf, 0, FIELD * 6);
        for (i, field) in fields.iter().enumerate() {
            let dst = buf.add(i * FIELD);
            let n = field.len().min(FIELD - 1);
            core::ptr::copy_nonoverlapping(field.as_ptr(), dst, n);
        }
    }
    0
}

// IA32_FMASK (see init() above) clears IF the instant the `syscall`
// instruction executes, so we land here with interrupts hard-disabled.
// That's fine for a normal syscall that just does a bit of work and
// returns -- but several of our syscalls (SYS_READ on fd 0,
// SYS_HAMIX_READKEY) legitimately *block*, spinning on `hlt` until the
// keyboard IRQ delivers a scancode. `hlt` with IF=0 never wakes up: the
// keyboard interrupt can't fire, so the CPU halts forever. That's the
// root cause of `help`/`edit`/`startx` hanging the whole machine on a
// dead cursor -- every one of them ends up blocked on a key read inside
// a syscall. `sti` right after we're safely on our own kernel stack
// re-enables interrupts for the body of the syscall (so blocking reads
// can actually be woken up by IRQ1), and `cli` right before we hand the
// stack back to userspace closes the window again so a same-privilege
// interrupt can't land on a half-restored user stack. `sysretq` still
// restores the *user's* original RFLAGS (IF=1) from r11 regardless, so
// this doesn't change anything about the interrupt state the caller
// sees on return -- it only fixes what happens *during* the syscall.
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "swapgs",
        "mov gs:[8], rsp",
        "mov rsp, gs:[0]",
        "and rsp, -16",
        "push rcx",
        "push r11",
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbp",
        "push rbx",
        "push r9",
        "push r8",
        "push r10",
        "push rdx",
        "push rsi",
        "push rdi",
        "sub rsp, 512",
        "fxsave64 [rsp]",
        "mov rdi, rax",
        "mov rsi, [rsp + 512]",
        "mov rdx, [rsp + 520]",
        "mov rcx, [rsp + 528]",
        "sti",
        "call {handler}",
        "cli",
        "fxrstor64 [rsp]",
        "add rsp, 512",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop r10",
        "pop r8",
        "pop r9",
        "pop rbx",
        "pop rbp",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "pop r11",
        "pop rcx",
        "mov rsp, gs:[8]",
        "swapgs",
        "sysretq",
        handler = sym handle_syscall,
    );
}
