use core::ffi::{c_char, c_int, c_long, c_void};

pub const SIGHUP: c_int = 1;
pub const SIGKILL: c_int = 9;
pub const SIGPIPE: c_int = 13;
pub const SIGCHLD: c_int = 17;
pub const SIGTERM: c_int = 15;
pub const SIG_IGN: usize = 1;

pub const EPERM: c_int = 1;
pub const EINTR: c_int = 4;
pub const EAGAIN: c_int = 11;

pub const WNOHANG: c_int = 1;

pub const CLOCK_REALTIME: c_int = 0;
pub const CLOCK_MONOTONIC: c_int = 1;

pub const PROT_READ: c_int = 1;
pub const PROT_WRITE: c_int = 2;
pub const MAP_PRIVATE: c_int = 2;
pub const MAP_ANONYMOUS: c_int = 0x20;
pub const MAP_FAILED: *mut c_void = !0usize as *mut c_void;

pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;
pub const O_NONBLOCK: c_int = 0o4000;

pub const TIOCGWINSZ: c_long = 0x5413;
pub const TIOCSWINSZ: c_long = 0x5414;
pub const TIOCSCTTY: c_long = 0x540E;
pub const O_NOCTTY: c_int = 0o400;

pub const AF_INET: c_int = 2;
pub const SOCK_STREAM: c_int = 1;
pub const IPPROTO_ICMP: c_int = 1;
pub const POLLOUT: i16 = 4;
pub const SO_ERROR: c_int = 4;
pub const EINPROGRESS: c_int = 115;
pub const ETIMEDOUT: c_int = 110;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct sockaddr_in {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

#[repr(C)]
pub struct addrinfo {
    pub ai_flags: c_int,
    pub ai_family: c_int,
    pub ai_socktype: c_int,
    pub ai_protocol: c_int,
    pub ai_addrlen: u32,
    pub ai_addr: *mut sockaddr_in,
    pub ai_canonname: *mut c_char,
    pub ai_next: *mut addrinfo,
}

pub const RB_AUTOBOOT: c_int = 0x0123_4567;
pub const RB_HALT_SYSTEM: c_int = 0xCDEF_0123u32 as c_int;
pub const RB_POWER_OFF: c_int = 0x4321_FEDC;
pub const MNT_DETACH: c_int = 2;

pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFBLK: u32 = 0o060000;
pub const S_IFLNK: u32 = 0o120000;

pub const DT_DIR: u8 = 4;

pub const AF_UNIX: c_int = 1;
pub const SOCK_DGRAM: c_int = 2;
pub const SOCK_NONBLOCK: c_int = 0o4000;
pub const SOCK_CLOEXEC: c_int = 0o2000000;
pub const SOL_SOCKET: c_int = 1;
pub const SO_PASSCRED: c_int = 16;
pub const SCM_CREDENTIALS: c_int = 2;
pub const MSG_DONTWAIT: c_int = 0x40;
pub const MSG_NOSIGNAL: c_int = 0x4000;
pub const POLLIN: i16 = 1;
pub const O_RDWR: c_int = 2;
pub const O_CREAT: c_int = 0o100;
pub const O_EXCL: c_int = 0o200;
pub const O_CLOEXEC: c_int = 0o2000000;
pub const MAP_SHARED: c_int = 1;

#[repr(C)]
pub struct sockaddr_un {
    pub sun_family: u16,
    pub sun_path: [u8; 108],
}

#[repr(C)]
pub struct iovec {
    pub iov_base: *mut c_void,
    pub iov_len: usize,
}

#[repr(C)]
pub struct msghdr {
    pub msg_name: *mut c_void,
    pub msg_namelen: u32,
    pub msg_iov: *mut iovec,
    pub msg_iovlen: usize,
    pub msg_control: *mut c_void,
    pub msg_controllen: usize,
    pub msg_flags: c_int,
}

#[repr(C)]
pub struct pollfd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: timespec,
    pub st_mtime: timespec,
    pub st_ctime: timespec,
    pub reserved: [i64; 3],
}

#[cfg(not(target_arch = "x86_64"))]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub pad1: u64,
    pub st_size: i64,
    pub st_blksize: i32,
    pub pad2: i32,
    pub st_blocks: i64,
    pub st_atime: timespec,
    pub st_mtime: timespec,
    pub st_ctime: timespec,
    pub reserved: [u32; 2],
}

impl stat {
    pub fn zeroed() -> stat {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct statvfs {
    pub f_bsize: u64,
    pub f_frsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_favail: u64,
    pub f_fsid: u64,
    pub f_flag: u64,
    pub f_namemax: u64,
    pub spare: [u64; 6],
}

impl statvfs {
    pub fn zeroed() -> statvfs {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    pub tm_gmtoff: c_long,
    pub tm_zone: *const c_char,
}

impl tm {
    pub fn zeroed() -> tm {
        unsafe { core::mem::zeroed() }
    }
}

#[repr(C)]
pub struct dirent {
    pub d_ino: u64,
    pub d_off: i64,
    pub d_reclen: u16,
    pub d_type: u8,
    pub d_name: [c_char; 256],
}

pub enum DIR {}

#[repr(C, align(8))]
pub struct posix_spawn_file_actions_t {
    pub opaque: [u64; 16],
}

#[repr(C, align(8))]
pub struct posix_spawnattr_t {
    pub opaque: [u64; 64],
}

impl posix_spawnattr_t {
    pub fn zeroed() -> posix_spawnattr_t {
        posix_spawnattr_t { opaque: [0; 64] }
    }
}

pub const POSIX_SPAWN_SETPGROUP: i16 = 2;
pub const POSIX_SPAWN_SETSID: i16 = 128;

impl posix_spawn_file_actions_t {
    pub fn zeroed() -> posix_spawn_file_actions_t {
        posix_spawn_file_actions_t { opaque: [0; 16] }
    }
}

unsafe extern "C" {
    pub fn crypt(key: *const c_char, setting: *const c_char) -> *mut c_char;
    pub fn strlen(s: *const c_char) -> usize;
    pub fn malloc(size: usize) -> *mut c_void;
    pub fn calloc(count: usize, size: usize) -> *mut c_void;
    pub fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    pub fn free(ptr: *mut c_void);
    pub fn aligned_alloc(align: usize, size: usize) -> *mut c_void;
    pub fn exit(code: c_int) -> !;
    pub fn _exit(code: c_int) -> !;
    pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    pub fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn dup(fd: c_int) -> c_int;
    pub fn dup2(fd: c_int, target: c_int) -> c_int;
    pub fn lseek(fd: c_int, offset: i64, whence: c_int) -> i64;
    pub fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    pub fn ioctl(fd: c_int, request: c_long, ...) -> c_int;
    pub fn isatty(fd: c_int) -> c_int;
    pub fn pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    pub fn unlink(path: *const c_char) -> c_int;
    pub fn rmdir(path: *const c_char) -> c_int;
    pub fn mkdir(path: *const c_char, mode: u32) -> c_int;
    pub fn chdir(path: *const c_char) -> c_int;
    pub fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char;
    pub fn rename(from: *const c_char, to: *const c_char) -> c_int;
    pub fn chmod(path: *const c_char, mode: u32) -> c_int;
    pub fn chown(path: *const c_char, owner: u32, group: u32) -> c_int;
    pub fn truncate(path: *const c_char, length: i64) -> c_int;
    pub fn stat(path: *const c_char, buf: *mut stat) -> c_int;
    pub fn fstat(fd: c_int, buf: *mut stat) -> c_int;
    pub fn lstat(path: *const c_char, buf: *mut stat) -> c_int;
    pub fn readlink(path: *const c_char, buf: *mut c_char, size: usize) -> isize;
    pub fn statvfs(path: *const c_char, buf: *mut statvfs) -> c_int;
    pub fn localtime_r(time: *const i64, out: *mut tm) -> *mut tm;
    pub fn fstatat(dirfd: c_int, path: *const c_char, buf: *mut stat, flags: c_int) -> c_int;
    pub fn opendir(path: *const c_char) -> *mut DIR;
    pub fn readdir(dir: *mut DIR) -> *mut dirent;
    pub fn closedir(dir: *mut DIR) -> c_int;
    pub fn dirfd(dir: *mut DIR) -> c_int;
    pub fn sync();
    pub fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, offset: i64) -> *mut c_void;
    pub fn munmap(addr: *mut c_void, len: usize) -> c_int;
    pub fn getpid() -> c_int;
    pub fn getppid() -> c_int;
    pub fn getuid() -> u32;
    pub fn geteuid() -> u32;
    pub fn setuid(uid: u32) -> c_int;
    pub fn setgid(gid: u32) -> c_int;
    pub fn access(path: *const c_char, mode: c_int) -> c_int;
    pub fn kill(pid: c_int, sig: c_int) -> c_int;
    pub fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    pub fn sched_yield() -> c_int;
    pub fn nanosleep(request: *const timespec, remaining: *mut timespec) -> c_int;
    pub fn clock_gettime(clock: c_int, out: *mut timespec) -> c_int;
    pub fn signal(sig: c_int, handler: usize) -> usize;
    pub fn fflush(stream: *mut c_void) -> c_int;
    pub fn posix_spawnattr_init(attr: *mut posix_spawnattr_t) -> c_int;
    pub fn posix_spawnattr_destroy(attr: *mut posix_spawnattr_t) -> c_int;
    pub fn posix_spawnattr_setflags(attr: *mut posix_spawnattr_t, flags: i16) -> c_int;
    pub fn posix_spawnattr_setpgroup(attr: *mut posix_spawnattr_t, group: c_int) -> c_int;
    pub fn posix_spawn(pid: *mut c_int, path: *const c_char, actions: *const posix_spawn_file_actions_t, attr: *const posix_spawnattr_t, argv: *const *const c_char, envp: *const *const c_char) -> c_int;
    pub fn posix_spawn_file_actions_init(actions: *mut posix_spawn_file_actions_t) -> c_int;
    pub fn posix_spawn_file_actions_destroy(actions: *mut posix_spawn_file_actions_t) -> c_int;
    pub fn posix_spawn_file_actions_adddup2(actions: *mut posix_spawn_file_actions_t, fd: c_int, target: c_int) -> c_int;
    pub fn posix_spawn_file_actions_addclosefrom_np(actions: *mut posix_spawn_file_actions_t, from: c_int) -> c_int;
    pub static environ: *const *const c_char;
    pub fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    pub fn bind(fd: c_int, addr: *const sockaddr_un, len: u32) -> c_int;
    pub fn sendto(fd: c_int, buf: *const c_void, len: usize, flags: c_int, addr: *const sockaddr_un, addrlen: u32) -> isize;
    pub fn recvmsg(fd: c_int, msg: *mut msghdr, flags: c_int) -> isize;
    pub fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, len: u32) -> c_int;
    pub fn poll(fds: *mut pollfd, count: u64, timeout: c_int) -> c_int;
    pub fn shm_open(name: *const c_char, flags: c_int, mode: u32) -> c_int;
    pub fn shm_unlink(name: *const c_char) -> c_int;
    pub fn ftruncate(fd: c_int, length: i64) -> c_int;
    pub fn connect(fd: c_int, addr: *const sockaddr_in, len: u32) -> c_int;
    pub fn send(fd: c_int, buf: *const c_void, len: usize, flags: c_int) -> isize;
    pub fn recv(fd: c_int, buf: *mut c_void, len: usize, flags: c_int) -> isize;
    pub fn recvfrom(fd: c_int, buf: *mut c_void, len: usize, flags: c_int, addr: *mut sockaddr_in, addrlen: *mut u32) -> isize;
    pub fn getsockopt(fd: c_int, level: c_int, name: c_int, value: *mut c_void, len: *mut u32) -> c_int;
    pub fn getaddrinfo(node: *const c_char, service: *const c_char, hints: *const addrinfo, res: *mut *mut addrinfo) -> c_int;
    pub fn freeaddrinfo(res: *mut addrinfo);
    pub fn setgroups(size: usize, list: *const u32) -> c_int;
    pub fn getgroups(size: c_int, list: *mut u32) -> c_int;
    pub fn initgroups(user: *const c_char, gid: u32) -> c_int;
    pub fn getgid() -> u32;
    pub fn getegid() -> u32;
    pub fn tcgetattr(fd: c_int, termios: *mut u8) -> c_int;
    pub fn tcsetattr(fd: c_int, action: c_int, termios: *const u8) -> c_int;
    pub fn tcsetpgrp(fd: c_int, pgrp: c_int) -> c_int;
    pub fn tcgetpgrp(fd: c_int) -> c_int;
    pub fn getpgrp() -> c_int;
    pub fn getpgid(pid: c_int) -> c_int;
    pub fn setsid() -> c_int;
    pub fn reboot(cmd: c_int) -> c_int;
    pub fn mount(source: *const c_char, target: *const c_char, fstype: *const c_char, flags: u64, data: *const c_void) -> c_int;
    pub fn umount2(target: *const c_char, flags: c_int) -> c_int;
    pub fn pread(fd: c_int, buf: *mut c_void, count: usize, offset: i64) -> isize;
    pub fn pwrite(fd: c_int, buf: *const c_void, count: usize, offset: i64) -> isize;
    pub fn posix_openpt(flags: c_int) -> c_int;
    pub fn grantpt(fd: c_int) -> c_int;
    pub fn unlockpt(fd: c_int) -> c_int;
    pub fn ptsname_r(fd: c_int, buf: *mut c_char, len: usize) -> c_int;
    pub fn __errno_location() -> *mut c_int;
    pub fn __do_syscall6(num: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64, a6: i64) -> i64;
}

pub fn errno() -> i64 {
    unsafe { *__errno_location() as i64 }
}

pub fn result(ret: i64) -> i64 {
    if ret < 0 { -errno() } else { ret }
}
