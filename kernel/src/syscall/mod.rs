use alloc::string::String;
use alloc::vec::Vec;
use core::slice;
use spin::Mutex;

use crate::drivers::tty::{CONSOLE, COLOR_FG};
use crate::fs;

const SYSCALL_STACK_SIZE: usize = 4096 * 4;
static mut SYSCALL_STACK: [u8; SYSCALL_STACK_SIZE] = [0u8; SYSCALL_STACK_SIZE];

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
        let stack_top = (&raw const SYSCALL_STACK) as u64 + SYSCALL_STACK_SIZE as u64;
        PER_CPU.kernel_rsp = stack_top;
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
const SYS_MMAP: u64 = 9;
const SYS_BRK: u64 = 12;
const SYS_IOCTL: u64 = 16;
const SYS_GETPID: u64 = 39;
const SYS_EXIT: u64 = 60;
const SYS_UNAME: u64 = 63;
const SYS_ARCH_PRCTL: u64 = 158;
const SYS_EXIT_GROUP: u64 = 231;

const ENOSYS: i64 = -38;
const EBADF: i64 = -9;
const ENOENT: i64 = -2;
const EINVAL: i64 = -22;
const EMFILE: i64 = -24;

struct OpenFile {
    path: String,
    pos: usize,
}

const MAX_FDS: usize = 32;
static FD_TABLE: Mutex<[Option<OpenFile>; MAX_FDS]> = Mutex::new([const { None }; MAX_FDS]);

const USER_HEAP_SIZE: usize = 4 * 1024 * 1024;
static mut USER_HEAP: [u8; USER_HEAP_SIZE] = [0u8; USER_HEAP_SIZE];
static BRK: Mutex<usize> = Mutex::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "C" fn handle_syscall(syscall_number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    match syscall_number {
        SYS_READ => unsafe { sys_read(arg1, arg2 as *mut u8, arg3 as usize) },
        SYS_WRITE => unsafe { sys_write(arg1, arg2 as *const u8, arg3 as usize) },
        SYS_OPEN => unsafe { sys_open(arg1 as *const u8) },
        SYS_CLOSE => sys_close(arg1),
        SYS_MMAP => sys_mmap(arg2 as usize),
        SYS_BRK => sys_brk(arg1 as usize),
        SYS_IOCTL => 0,
        SYS_GETPID => 1,
        SYS_UNAME => unsafe { sys_uname(arg1 as *mut u8) },
        SYS_ARCH_PRCTL => 0,
        SYS_EXIT | SYS_EXIT_GROUP => {
            kernel_terminate_current_process(arg1 as i32);
            0
        }
        _ => ENOSYS,
    }
}

fn kernel_print_slice(slice: &[u8]) {
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
            match keyboard::read_key_blocking() {
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
    let slot_idx = match table.iter().position(|f| f.is_none()) {
        Some(idx) => idx,
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

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "swapgs",
        "mov gs:[8], rsp",
        "mov rsp, gs:[0]",
        "push rcx",
        "push r11",
        "mov r8, rdx",
        "mov r9, rsi",
        "mov rsi, rdi",
        "mov rdi, rax",
        "mov rdx, r9",
        "mov rcx, r8",
        "call {handler}",
        "pop r11",
        "pop rcx",
        "mov rsp, gs:[8]",
        "swapgs",
        "sysretq",
        handler = sym handle_syscall,
    );
}
