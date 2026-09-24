
use super::*;

pub mod base;
pub mod epoll;
#[cfg(not(target_arch = "x86_64"))]
pub mod generic;
pub mod inet;
pub mod misc;
pub mod netlink;
pub mod procfs;
pub mod process;
pub mod signal;
pub mod socket;
pub mod special;
pub mod tty;

pub(super) const SYS_STAT: u64 = 4;
pub(super) const SYS_LSTAT: u64 = 6;
pub(super) const SYS_MPROTECT: u64 = 10;
pub(super) const SYS_RT_SIGACTION: u64 = 13;
pub(super) const SYS_RT_SIGPROCMASK: u64 = 14;
pub(super) const SYS_RT_SIGRETURN: u64 = 15;
pub(super) const SYS_PAUSE: u64 = 34;
pub(super) const SYS_GETITIMER: u64 = 36;
pub(super) const SYS_ALARM: u64 = 37;
pub(super) const SYS_SETITIMER: u64 = 38;
pub(super) const SYS_RT_SIGPENDING: u64 = 127;
pub(super) const SYS_RT_SIGTIMEDWAIT: u64 = 128;
pub(super) const SYS_RT_SIGSUSPEND: u64 = 130;
pub(super) const SYS_TKILL: u64 = 200;
pub(super) const SYS_TGKILL: u64 = 234;
pub(super) const SYS_IOCTL: u64 = 16;
pub(super) const SYS_PREAD64: u64 = 17;
pub(super) const SYS_READV: u64 = 19;
pub(super) const SYS_WRITEV: u64 = 20;
pub(super) const SYS_ACCESS: u64 = 21;
pub(super) const SYS_MADVISE: u64 = 28;
pub(super) const SYS_FCNTL: u64 = 72;
pub(super) const SYS_READLINK: u64 = 89;
pub(super) const SYS_GETGID: u64 = 104;
pub(super) const SYS_GETEGID: u64 = 108;
pub(super) const SYS_SIGALTSTACK: u64 = 131;
pub(super) const SYS_ARCH_PRCTL: u64 = 158;
pub(super) const SYS_GETTID: u64 = 186;
pub(super) const SYS_FUTEX: u64 = 202;
pub(super) const SYS_SET_TID_ADDRESS: u64 = 218;
pub(super) const SYS_EXIT_GROUP: u64 = 231;
pub(super) const SYS_OPENAT: u64 = 257;
pub(super) const SYS_NEWFSTATAT: u64 = 262;
pub(super) const SYS_READLINKAT: u64 = 267;
pub(super) const SYS_FACCESSAT: u64 = 269;
pub(super) const SYS_SET_ROBUST_LIST: u64 = 273;
pub(super) const SYS_GETRANDOM: u64 = 318;
pub(super) const SYS_DUP: u64 = 32;
pub(super) const SYS_PWRITE64: u64 = 18;
pub(super) const SYS_SOCKET_LINUX: u64 = 41;
pub(super) const SYS_CONNECT: u64 = 42;
pub(super) const SYS_ACCEPT: u64 = 43;
pub(super) const SYS_SENDTO: u64 = 44;
pub(super) const SYS_RECVFROM: u64 = 45;
pub(super) const SYS_SENDMSG: u64 = 46;
pub(super) const SYS_RECVMSG: u64 = 47;
pub(super) const SYS_SHUTDOWN: u64 = 48;
pub(super) const SYS_BIND: u64 = 49;
pub(super) const SYS_LISTEN: u64 = 50;
pub(super) const SYS_GETSOCKNAME: u64 = 51;
pub(super) const SYS_GETPEERNAME: u64 = 52;
pub(super) const SYS_SOCKETPAIR: u64 = 53;
pub(super) const SYS_SETSOCKOPT: u64 = 54;
pub(super) const SYS_GETSOCKOPT: u64 = 55;
pub(super) const SYS_EPOLL_CREATE: u64 = 213;
pub(super) const SYS_EPOLL_WAIT: u64 = 232;
pub(super) const SYS_EPOLL_CTL: u64 = 233;
pub(super) const SYS_EPOLL_PWAIT: u64 = 281;
pub(super) const SYS_ACCEPT4: u64 = 288;
pub(super) const SYS_EPOLL_CREATE1: u64 = 291;
pub(super) const SYS_MEMFD_CREATE: u64 = 319;
pub(super) const SYS_POLL: u64 = 7;
pub(super) const SYS_PIPE: u64 = 22;
pub(super) const SYS_PPOLL: u64 = 271;
pub(super) const SYS_PIPE2: u64 = 293;
pub(super) const SYS_DUP2: u64 = 33;
pub(super) const SYS_NANOSLEEP: u64 = 35;
pub(super) const SYS_SENDFILE: u64 = 40;
pub(super) const SYS_TRUNCATE: u64 = 76;
pub(super) const SYS_FTRUNCATE: u64 = 77;
pub(super) const SYS_FCHDIR: u64 = 81;
pub(super) const SYS_RMDIR: u64 = 84;
pub(super) const SYS_LINK: u64 = 86;
pub(super) const SYS_SYMLINK: u64 = 88;
pub(super) const SYS_FCHMOD: u64 = 91;
pub(super) const SYS_FCHOWN: u64 = 93;
pub(super) const SYS_LCHOWN: u64 = 94;
pub(super) const SYS_UMASK: u64 = 95;
pub(super) const SYS_SYSINFO: u64 = 99;
pub(super) const SYS_GETGROUPS: u64 = 115;
pub(super) const SYS_UTIME: u64 = 132;
pub(super) const SYS_STATFS: u64 = 137;
pub(super) const SYS_FSTATFS: u64 = 138;
pub(super) const SYS_SCHED_GETAFFINITY: u64 = 204;
pub(super) const SYS_GETDENTS64: u64 = 217;
pub(super) const SYS_CLOCK_NANOSLEEP: u64 = 230;
pub(super) const SYS_UTIMES: u64 = 235;
pub(super) const SYS_MKDIRAT: u64 = 258;
pub(super) const SYS_FCHOWNAT: u64 = 260;
pub(super) const SYS_FUTIMESAT: u64 = 261;
pub(super) const SYS_UNLINKAT: u64 = 263;
pub(super) const SYS_RENAMEAT: u64 = 264;
pub(super) const SYS_LINKAT: u64 = 265;
pub(super) const SYS_STATX: u64 = 332;
pub(super) const SYS_CLOSE_RANGE: u64 = 436;
pub(super) const SYS_SYMLINKAT: u64 = 266;
pub(super) const SYS_FCHMODAT: u64 = 268;
pub(super) const SYS_UTIMENSAT: u64 = 280;
pub(super) const SYS_DUP3: u64 = 292;
pub(super) const SYS_RENAMEAT2: u64 = 316;
pub(super) const SYS_SELECT: u64 = 23;
pub(super) const SYS_TIMER_CREATE: u64 = 222;
pub(super) const SYS_TIMER_SETTIME: u64 = 223;
pub(super) const SYS_TIMER_GETTIME: u64 = 224;
pub(super) const SYS_TIMER_GETOVERRUN: u64 = 225;
pub(super) const SYS_TIMER_DELETE: u64 = 226;
pub(super) const SYS_SIGNALFD: u64 = 282;
pub(super) const SYS_TIMERFD_CREATE: u64 = 283;
pub(super) const SYS_EVENTFD: u64 = 284;
pub(super) const SYS_TIMERFD_SETTIME: u64 = 286;
pub(super) const SYS_TIMERFD_GETTIME: u64 = 287;
pub(super) const SYS_SIGNALFD4: u64 = 289;
pub(super) const SYS_EVENTFD2: u64 = 290;
pub(super) const SYS_INOTIFY_INIT: u64 = 253;
pub(super) const SYS_INOTIFY_ADD_WATCH: u64 = 254;
pub(super) const SYS_INOTIFY_RM_WATCH: u64 = 255;
pub(super) const SYS_INOTIFY_INIT1: u64 = 294;
pub(super) const SYS_CLONE: u64 = 56;
pub(super) const SYS_FORK: u64 = 57;
pub(super) const SYS_VFORK: u64 = 58;
pub(super) const SYS_EXECVE: u64 = 59;
pub(super) const SYS_WAITID: u64 = 247;
pub(super) const SYS_EXECVEAT: u64 = 322;
pub(super) const SYS_MREMAP: u64 = 25;
pub(super) const SYS_MSYNC: u64 = 26;
pub(super) const SYS_FLOCK: u64 = 73;
pub(super) const SYS_FSYNC: u64 = 74;
pub(super) const SYS_FDATASYNC: u64 = 75;
pub(super) const SYS_CREAT: u64 = 85;
pub(super) const SYS_GETTIMEOFDAY: u64 = 96;
pub(super) const SYS_GETRLIMIT: u64 = 97;
pub(super) const SYS_GETRUSAGE: u64 = 98;
pub(super) const SYS_TIMES: u64 = 100;
pub(super) const SYS_PTRACE: u64 = 101;
pub(super) const SYS_SETUID: u64 = 105;
pub(super) const SYS_SETGID: u64 = 106;
pub(super) const SYS_SETPGID: u64 = 109;
pub(super) const SYS_GETPGRP: u64 = 111;
pub(super) const SYS_SETSID: u64 = 112;
pub(super) const SYS_SETREUID: u64 = 113;
pub(super) const SYS_SETREGID: u64 = 114;
pub(super) const SYS_SETGROUPS: u64 = 116;
pub(super) const SYS_SETRESUID: u64 = 117;
pub(super) const SYS_GETRESUID: u64 = 118;
pub(super) const SYS_SETRESGID: u64 = 119;
pub(super) const SYS_GETRESGID: u64 = 120;
pub(super) const SYS_GETPGID: u64 = 121;
pub(super) const SYS_SETFSUID: u64 = 122;
pub(super) const SYS_SETFSGID: u64 = 123;
pub(super) const SYS_GETSID: u64 = 124;
pub(super) const SYS_CAPGET: u64 = 125;
pub(super) const SYS_CAPSET: u64 = 126;
pub(super) const SYS_MKNOD: u64 = 133;
pub(super) const SYS_PERSONALITY: u64 = 135;
pub(super) const SYS_GETPRIORITY: u64 = 140;
pub(super) const SYS_SETPRIORITY: u64 = 141;
pub(super) const SYS_SCHED_SETPARAM: u64 = 142;
pub(super) const SYS_SCHED_GETPARAM: u64 = 143;
pub(super) const SYS_SCHED_SETSCHEDULER: u64 = 144;
pub(super) const SYS_SCHED_GETSCHEDULER: u64 = 145;
pub(super) const SYS_SCHED_GET_PRIORITY_MAX: u64 = 146;
pub(super) const SYS_SCHED_GET_PRIORITY_MIN: u64 = 147;
pub(super) const SYS_SCHED_RR_GET_INTERVAL: u64 = 148;
pub(super) const SYS_MLOCK: u64 = 149;
pub(super) const SYS_MUNLOCK: u64 = 150;
pub(super) const SYS_MLOCKALL: u64 = 151;
pub(super) const SYS_MUNLOCKALL: u64 = 152;
pub(super) const SYS_PRCTL: u64 = 157;
pub(super) const SYS_SETRLIMIT: u64 = 160;
pub(super) const SYS_CHROOT: u64 = 161;
pub(super) const SYS_SYNC: u64 = 162;
pub(super) const SYS_MOUNT: u64 = 165;
pub(super) const SYS_SETXATTR: u64 = 188;
pub(super) const SYS_LSETXATTR: u64 = 189;
pub(super) const SYS_FSETXATTR: u64 = 190;
pub(super) const SYS_GETXATTR: u64 = 191;
pub(super) const SYS_LGETXATTR: u64 = 192;
pub(super) const SYS_FGETXATTR: u64 = 193;
pub(super) const SYS_LISTXATTR: u64 = 194;
pub(super) const SYS_LLISTXATTR: u64 = 195;
pub(super) const SYS_FLISTXATTR: u64 = 196;
pub(super) const SYS_TIME: u64 = 201;
pub(super) const SYS_SCHED_SETAFFINITY: u64 = 203;
pub(super) const SYS_FADVISE64: u64 = 221;
pub(super) const SYS_CLOCK_GETRES: u64 = 229;
pub(super) const SYS_PSELECT6: u64 = 270;
pub(super) const SYS_SYNC_FILE_RANGE: u64 = 277;
pub(super) const SYS_FALLOCATE: u64 = 285;
pub(super) const SYS_PRLIMIT64: u64 = 302;
pub(super) const SYS_SYNCFS: u64 = 306;
pub(super) const SYS_GETCPU: u64 = 309;
pub(super) const SYS_MEMBARRIER: u64 = 324;
pub(super) const SYS_MBIND: u64 = 237;
pub(super) const SYS_MINCORE: u64 = 27;
pub(super) const SYS_PIDFD_OPEN: u64 = 434;
pub(super) const SYS_SET_MEMPOLICY: u64 = 238;
pub(super) const SYS_GET_MEMPOLICY: u64 = 239;
pub(super) const SYS_FACCESSAT2: u64 = 439;
pub(super) const SYS_EPOLL_PWAIT2: u64 = 441;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> Option<i64> {
    Some(match number {
        SYS_SELECT => tty::sys_select(a1, a2, a3, a4, a5),
        SYS_TIMER_CREATE => signal::sys_timer_create(a1, a2, a3),
        SYS_TIMER_SETTIME => signal::sys_timer_settime(a1, a2, a3, a4),
        SYS_TIMER_GETTIME => signal::sys_timer_gettime(a1, a2),
        SYS_TIMER_GETOVERRUN => signal::sys_timer_getoverrun(a1),
        SYS_TIMER_DELETE => signal::sys_timer_delete(a1),
        SYS_EVENTFD => special::sys_eventfd2(a1, 0),
        SYS_EVENTFD2 => special::sys_eventfd2(a1, a2),
        SYS_TIMERFD_CREATE => special::sys_timerfd_create(a1, a2),
        SYS_TIMERFD_SETTIME => special::sys_timerfd_settime(a1, a2, a3, a4),
        SYS_TIMERFD_GETTIME => special::sys_timerfd_gettime(a1, a2),
        SYS_SIGNALFD => special::sys_signalfd4(a1, a2, a3, 0),
        SYS_SIGNALFD4 => special::sys_signalfd4(a1, a2, a3, a4),
        SYS_INOTIFY_INIT => special::sys_inotify_init1(0),
        SYS_INOTIFY_INIT1 => special::sys_inotify_init1(a1),
        SYS_INOTIFY_ADD_WATCH => special::sys_inotify_add_watch(a1, a2, a3),
        SYS_INOTIFY_RM_WATCH => special::sys_inotify_rm_watch(a1, a2),
        SYS_PSELECT6 => {
            let (set, size) = if a6 == 0 {
                (0, 8)
            } else {
                match user_slice(a6, 16) {
                    Ok(b) => (u64::from_le_bytes(b[0..8].try_into().unwrap()), u64::from_le_bytes(b[8..16].try_into().unwrap())),
                    Err(e) => return Some(e),
                }
            };
            signal::with_wait_mask(set, size, || tty::sys_pselect6(a1, a2, a3, a4, a5))
        }
        SYS_MREMAP => misc::sys_mremap(a1, a2, a3, a4, a5),
        SYS_MSYNC | SYS_FADVISE64 | SYS_SYNC_FILE_RANGE | SYS_MLOCK | SYS_MUNLOCK | SYS_MLOCKALL | SYS_MUNLOCKALL => 0,
        SYS_FLOCK => misc::sys_flock(a1),
        SYS_FSYNC | SYS_FDATASYNC | SYS_SYNCFS => misc::sys_fsync(a1),
        SYS_SYNC => misc::sys_sync(),
        SYS_FALLOCATE => base::sys_fallocate(a1, a2, a3, a4),
        SYS_CREAT => base::sys_openat(base::AT_FDCWD, a1, 0o1101),
        SYS_GETTIMEOFDAY => misc::sys_gettimeofday(a1, a2),
        SYS_TIME => misc::sys_time(a1),
        SYS_CLOCK_GETRES => misc::sys_clock_getres(a1, a2),
        SYS_GETRLIMIT => misc::sys_prlimit64(0, a1, 0, a2),
        SYS_SETRLIMIT => 0,
        SYS_PRLIMIT64 => misc::sys_prlimit64(a1, a2, a3, a4),
        SYS_GETRUSAGE => misc::sys_getrusage(a1, a2),
        SYS_TIMES => misc::sys_times(a1),
        SYS_PTRACE | SYS_CHROOT | SYS_MOUNT | SYS_MKNOD => EPERM,
        SYS_SETUID => misc::sys_setuid(a1),
        SYS_SETGID | SYS_SETREGID | SYS_SETRESGID | SYS_SETGROUPS => misc::sys_setgid(a1),
        SYS_SETREUID => misc::sys_setreuid(a1, a2),
        SYS_SETRESUID => misc::sys_setresuid(a1, a2, a3),
        SYS_GETRESUID => misc::sys_getresuid(a1, a2, a3),
        SYS_GETRESGID => misc::sys_getresgid(a1, a2, a3),
        SYS_SETFSUID => misc::sys_setfsuid(a1),
        SYS_SETFSGID => misc::sys_setfsgid(a1),
        SYS_SETPGID => misc::sys_setpgid(a1, a2),
        SYS_GETPGID => misc::sys_getpgid(a1),
        SYS_GETPGRP => misc::sys_getpgid(0),
        SYS_SETSID => misc::sys_setsid(),
        SYS_GETSID => misc::sys_getsid(a1),
        SYS_CAPGET => misc::sys_capget(a1, a2),
        SYS_CAPSET => 0,
        SYS_PERSONALITY => 0,
        SYS_GETPRIORITY => misc::sys_getpriority(),
        SYS_SETPRIORITY | SYS_SCHED_SETPARAM | SYS_SCHED_SETSCHEDULER | SYS_SCHED_GETSCHEDULER | SYS_SCHED_GET_PRIORITY_MIN | SYS_SCHED_SETAFFINITY => 0,
        SYS_SCHED_GET_PRIORITY_MAX => 0,
        SYS_SCHED_GETPARAM => misc::sys_sched_getparam(a2),
        SYS_SCHED_RR_GET_INTERVAL => misc::sys_sched_rr_get_interval(a2),
        SYS_PRCTL => misc::sys_prctl(a1, a2),
        SYS_GETXATTR | SYS_LGETXATTR | SYS_FGETXATTR => misc::sys_xattr_get(),
        SYS_LISTXATTR | SYS_LLISTXATTR | SYS_FLISTXATTR => misc::sys_xattr_list(),
        SYS_SETXATTR | SYS_LSETXATTR | SYS_FSETXATTR => misc::sys_xattr_set(),
        SYS_GETCPU => misc::sys_getcpu(a1, a2),
        SYS_MEMBARRIER | SYS_MBIND | SYS_SET_MEMPOLICY => 0,
        SYS_GET_MEMPOLICY => misc::sys_get_mempolicy(a1, a2, a3),
        SYS_MINCORE => misc::sys_mincore(a1, a2, a3),
        SYS_PIDFD_OPEN => ENOSYS,
        SYS_FACCESSAT2 => base::sys_faccessat(a1, a2, a3),
        SYS_EPOLL_PWAIT2 => {
            let ms = if a4 == 0 {
                -1
            } else {
                match user_slice(a4, 16) {
                    Ok(b) => {
                        let secs = u64::from_le_bytes(b[0..8].try_into().unwrap());
                        let nanos = u64::from_le_bytes(b[8..16].try_into().unwrap());
                        (secs.saturating_mul(1000) + nanos.div_ceil(1_000_000)) as i64
                    }
                    Err(e) => return Some(e),
                }
            };
            signal::with_wait_mask(a5, a6, || epoll::sys_epoll_wait(a1, a2, a3, ms))
        }
        SYS_STAT => base::sys_fstatat(base::AT_FDCWD, a1, a2, 0),
        SYS_LSTAT => base::sys_lstat(a1, a2),
        SYS_DUP => base::sys_dup(a1),
        SYS_SOCKET_LINUX => with_open_flags(socket::sys_socket(a1, a2, a3), a2),
        SYS_CONNECT => socket::sys_connect(a1, a2, a3),
        SYS_ACCEPT => socket::sys_accept4(a1, a2, a3, 0),
        SYS_ACCEPT4 => with_open_flags(socket::sys_accept4(a1, a2, a3, a4), a4),
        SYS_SENDTO => socket::sys_sendto(a1, a2, a3, a4, a5, a6),
        SYS_RECVFROM => socket::sys_recvfrom(a1, a2, a3, a4, a5, a6),
        SYS_SENDMSG => socket::sys_sendmsg(a1, a2, a3),
        SYS_RECVMSG => socket::sys_recvmsg(a1, a2, a3),
        SYS_SHUTDOWN => socket::sys_shutdown(a1, a2),
        SYS_BIND => socket::sys_bind(a1, a2, a3),
        SYS_LISTEN => socket::sys_listen(a1, a2),
        SYS_GETSOCKNAME => socket::sys_getsockname(a1, a2, a3, false),
        SYS_GETPEERNAME => socket::sys_getsockname(a1, a2, a3, true),
        SYS_SOCKETPAIR => {
            let r = socket::sys_socketpair(a1, a2, a3, a4);
            if r == 0 {
                if let Ok(raw) = user_slice(a4, 8) {
                    let a = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as i64;
                    let b = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as i64;
                    with_open_flags(a, a2);
                    with_open_flags(b, a2);
                }
            }
            r
        }
        SYS_SETSOCKOPT => socket::sys_setsockopt(a1, a2, a3, a4, a5),
        SYS_GETSOCKOPT => socket::sys_getsockopt(a1, a2, a3, a4, a5),
        SYS_MEMFD_CREATE => with_open_flags(base::sys_memfd_create(a1, a2), if a2 & 1 != 0 { 0o2000000 } else { 0 }),
        SYS_PWRITE64 => base::sys_pwrite(a1, a2, a3, a4),
        SYS_EPOLL_CREATE => if (a1 as i32) <= 0 { EINVAL } else { epoll::sys_epoll_create(0) },
        SYS_EPOLL_CREATE1 => with_open_flags(epoll::sys_epoll_create(a1), a1 & 0o2000000),
        SYS_EPOLL_CTL => epoll::sys_epoll_ctl(a1, a2, a3, a4),
        SYS_EPOLL_WAIT => epoll::sys_epoll_wait(a1, a2, a3, a4 as i32 as i64),
        SYS_EPOLL_PWAIT => signal::with_wait_mask(a5, a6, || epoll::sys_epoll_wait(a1, a2, a3, a4 as i32 as i64)),
        SYS_PIPE => sys_pipe(a1, 0),
        SYS_PIPE2 => sys_pipe(a1, a2),
        SYS_POLL => tty::poll(a1, a2, a3 as i32 as i64),
        SYS_PPOLL => signal::with_wait_mask(a4, a5, || tty::ppoll(a1, a2, a3)),
        SYS_DUP2 => base::sys_dup2(a1, a2),
        SYS_DUP3 => base::sys_dup3(a1, a2, a3),
        SYS_NANOSLEEP => base::sys_nanosleep(a1, a2),
        SYS_CLOCK_NANOSLEEP => base::sys_clock_nanosleep(a1, a2, a3, a4),
        SYS_SENDFILE => base::sys_sendfile(a1, a2, a3, a4),
        SYS_TRUNCATE => base::sys_truncate(a1, a2),
        SYS_FTRUNCATE => base::sys_ftruncate(a1, a2),
        SYS_GETDENTS64 => base::sys_getdents64(a1, a2, a3),
        SYS_FCHDIR => base::sys_fchdir(a1),
        SYS_RMDIR => base::sys_unlinkat(base::AT_FDCWD, a1, 0x200),
        SYS_MKDIRAT => base::sys_mkdirat(a1, a2, a3),
        SYS_UNLINKAT => base::sys_unlinkat(a1, a2, a3),
        SYS_RENAMEAT => base::sys_renameat(a1, a2, a3, a4, 0),
        SYS_RENAMEAT2 => base::sys_renameat(a1, a2, a3, a4, a5),
        SYS_LINK => base::sys_linkat(base::AT_FDCWD, a1, base::AT_FDCWD, a2, 0),
        SYS_LINKAT => base::sys_linkat(a1, a2, a3, a4, a5),
        SYS_SYMLINK => base::sys_symlinkat(a1, base::AT_FDCWD, a2),
        SYS_SYMLINKAT => base::sys_symlinkat(a1, a2, a3),
        SYS_STATX => sys_statx(a1, a2, a3, a5),
        SYS_CLOSE_RANGE => sys_close_range(a1, a2, a3),
        SYS_FCHMOD => base::sys_fchmod(a1, a2),
        SYS_FCHMODAT => base::sys_fchmodat(a1, a2, a3),
        SYS_FCHOWN => base::sys_fchown(a1, a2),
        SYS_LCHOWN => base::sys_fchownat(base::AT_FDCWD, a1, a2),
        SYS_FCHOWNAT => base::sys_fchownat(a1, a2, a3),
        SYS_UMASK => base::sys_umask(a1),
        SYS_SYSINFO => base::sys_sysinfo(a1),
        SYS_GETGROUPS => base::sys_getgroups(a1, a2),
        SYS_STATFS => base::sys_statfs(a1, a2),
        SYS_FSTATFS => base::sys_fstatfs(a1, a2),
        SYS_SCHED_GETAFFINITY => base::sys_sched_getaffinity(a2, a3),
        SYS_UTIME | SYS_UTIMES => base::sys_utimensat(base::AT_FDCWD, a1, 0),
        SYS_FUTIMESAT => base::sys_utimensat(a1, a2, 0),
        SYS_UTIMENSAT => base::sys_utimensat(a1, a2, a4),
        SYS_PREAD64 => base::sys_pread(a1, a2, a3, a4),
        SYS_READV => base::sys_readv(a1, a2, a3),
        SYS_ACCESS => base::sys_faccessat(base::AT_FDCWD, a1, a2),
        SYS_MPROTECT => base::sys_mprotect(a1, a2),
        SYS_MADVISE => 0,
        SYS_FCNTL => base::sys_fcntl(a1, a2, a3),
        SYS_READLINK => base::sys_readlinkat(base::AT_FDCWD, a1, a2, a3),
        SYS_GETGID => base::sys_getgid(),
        SYS_GETEGID => base::sys_getegid(),
        SYS_SIGALTSTACK => signal::sys_sigaltstack(a1, a2),
        SYS_FUTEX => base::sys_futex(a1, a2, a3, a4, a5, a6),
        SYS_OPENAT => with_open_flags(base::sys_openat(a1, a2, a3), a3),
        SYS_NEWFSTATAT => base::sys_fstatat(a1, a2, a3, a4),
        SYS_READLINKAT => base::sys_readlinkat(a1, a2, a3, a4),
        SYS_FACCESSAT => base::sys_faccessat(a1, a2, a3),
        SYS_SET_ROBUST_LIST => 0,
        SYS_GETRANDOM => base::sys_getrandom(a1, a2),
        SYS_RT_SIGACTION => signal::sys_rt_sigaction(a1, a2, a3, a4),
        SYS_RT_SIGPROCMASK => signal::sys_rt_sigprocmask(a1, a2, a3, a4),
        SYS_RT_SIGPENDING => signal::sys_rt_sigpending(a1, a2),
        SYS_RT_SIGTIMEDWAIT => signal::sys_rt_sigtimedwait(a1, a2, a3, a4),
        SYS_RT_SIGSUSPEND => signal::sys_rt_sigsuspend(a1, a2),
        SYS_PAUSE => signal::sys_pause(),
        SYS_ALARM => signal::sys_alarm(a1),
        SYS_GETITIMER => signal::sys_getitimer(a1, a2),
        SYS_SETITIMER => signal::sys_setitimer(a1, a2, a3),
        SYS_TKILL => signal::sys_tgkill(0, a1, a2),
        SYS_TGKILL => signal::sys_tgkill(a1 as i64, a2, a3),
        129 => signal::sys_kill(a1 as i64, a2),
        297 => signal::sys_tgkill(a1 as i64, a2, a3),
        SYS_IOCTL => base::sys_ioctl(a1, a2, a3),
        SYS_ARCH_PRCTL => base::sys_arch_prctl(a1, a2),
        SYS_WRITEV => sys_writev(a1, a2, a3),
        SYS_GETTID => task::current_tid() as i64,
        SYS_SET_TID_ADDRESS => process::sys_set_tid_address(a1),
        SYS_CLONE => process::sys_clone(a1, a2, a3, a4, a5),
        SYS_FORK => process::sys_fork(),
        SYS_VFORK => process::sys_vfork(),
        SYS_EXECVE => process::sys_execve(a1, a2, a3),
        SYS_EXECVEAT => {
            if a5 & 0x1000 != 0 || (a1 != base::AT_FDCWD && !matches!(user_cstr(a2).as_deref().map(|p| p.starts_with('/')), Ok(true))) {
                ENOSYS
            } else {
                process::sys_execve(a2, a3, a4)
            }
        }
        SYS_WAITID => process::sys_waitid(a1, a2, a3, a4),
        SYS_EXIT_GROUP => process::sys_exit_group(a1),
        _ => return None,
    })
}
