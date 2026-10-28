# Hamix ABI v1 -- довідник нативних системних викликів

Status: current · Updated: 2026-10-06 · ADR: `docs/adr/0006B-hamix-abi-v1.md`

Цей файл -- **єдине джерело істини** для номерів нативних викликів. Таблиці нижче
читає `cargo xtask abi`: він генерує `sdk/hamix_abi/include/hamix/syscall.h` (C, для
hxlibc) і `hxlibc/rust/hxlibc-sys/src/nr.rs` (Rust) і перевіряє, що ядро
(`kernel/src/syscall/ext/*.rs`, файл на підсистему) має ті самі номери. Рядок таблиці =
`| номер | ІМ'Я | аргументи | результат | статус | примітка |`.

## 1. Угоди

| | x86_64 | aarch64 | riscv64 |
|---|---|---|---|
| інструкція | `syscall` | `svc #0` | `ecall` |
| номер | `rax` | `x8` | `a7` |
| аргументи 1-6 | `rdi rsi rdx r10 r8 r9` | `x0-x5` | `a0-a5` |
| результат | `rax` | `x0` | `a0` |

* Номери **однакові на всіх архітектурах** (для нативної особистості ядро не
  перекладає номери; діапазон core використовує нумерацію Linux x86-64 скрізь).
* Помилка -- від'ємне значення `-errno`; значення errno збігаються з Linux
  (`EPERM=1, ENOENT=2, ... ENOSYS=38`). Усе `>= 0` -- успіх.
* Рядки передаються як `(ptr, len)`, якщо в аргументах указано `str`, або як
  C-рядок з нулем, якщо `cstr`. Буфери результатів -- `(ptr, len)`; виклик повертає кількість байтів.
* Пари `x,y`/`w,h` пакуються в один u64 як `(x << 32) | y`.
* Структури `stat`, `dirent64`, `sockaddr`, `timespec`, `sigaction`, `ucontext` і
  кадр сигналу мають розкладку Linux для відповідної архітектури (`abis/hamix`
  у hxlibc -- копія `abis/linux`).

## 2. Версія і ELF-нота

Кожна нативна програма містить секцію `.note.hamix` (`PT_NOTE`):
`namesz=6, descsz=4, type=0x48584F53 ("HXOS"), name="Hamix\0", desc=u32 ревізія`.

| ревізія (`desc`) | Що означає |
|---|---|
| `1` | legacy 0.6 ("v0"): спеціальні семантики `wait4/kill/mkdir/unlink/rename/chmod/chown/mmap` у діапазоні core (див. §3.2) |
| `0x0100` | **Hamix ABI v1.0**: діапазон core має точну POSIX/Linux-семантику, сигнали доставляються |
| `0x01xx` | v1.x -- лише додає виклики |

Ядро приймає `1` і `0x0100..=HAMIX_ABI_VERSION`; новішу ревізію відхиляє
`execve` з `ENOEXEC`. Програма без ноти, з `PT_INTERP` або ET_DYN -- Linux-особистість.
Legacy-ревізія підтримується до релізу 0.8.

Поточне значення: `HAMIX_ABI_VERSION = 0x0100`.

**Заморожено 2026-09-28 (трек B, W8-W12).** Номери, аргументи й семантика викликів у
таблицях нижче для ревізії `0x0100` більше не змінюються; нове -- лише додаванням рядків
з наступною ревізією `0x0101`, `0x0102`... Перевірка: `tests/hxlibc/abi-core.c`
(`tests/boot/native-abi-core.sh`) викликає кожен виклик core-діапазону з таблиці
`hamix/syscall-table.h` у окремому процесі з некоректними аргументами і вимагає, щоб
жоден не повертав `ENOSYS`, не вбивав процес і не зависав (крім `ppoll` без тайм-ауту).
`cargo xtask abi` генерує цю таблицю разом із заголовками.

## 3. Діапазон core (0-511): POSIX-ядро

Ці виклики обслуговує POSIX-ядро (`kernel/src/syscall/*`); вони **не належать
Linux-шару**: доступні нативним процесам і при `linux=0`. `kernel/src/linux/` --
лише перекладачі Linux-особистості (шляхи `/linux`, нумерація asm-generic для aarch64/riscv64). Виклик із номером < 9000, якого немає в цій таблиці,
повертає нативному процесу `-ENOSYS`.

### 3.1 Таблиця

| nr | name | args | returns | status | note |
|---|---|---|---|---|---|
| 0 | READ | fd, buf, len | n | stable | |
| 1 | WRITE | fd, buf, len | n | stable | |
| 2 | OPEN | cstr path, flags, mode | fd | stable | |
| 3 | CLOSE | fd | 0 | stable | |
| 4 | STAT | cstr path, struct stat* | 0 | stable | |
| 5 | FSTAT | fd, struct stat* | 0 | stable | |
| 6 | LSTAT | cstr path, struct stat* | 0 | stable | |
| 7 | POLL | pollfd*, n, timeout_ms | n | stable | |
| 8 | LSEEK | fd, off, whence | off | stable | |
| 9 | MMAP | addr, len, prot, flags, fd, off | addr | stable | legacy: `flags & 0x23 == 0` -> анонімна ділянка довжини `len` |
| 10 | MPROTECT | addr, len, prot | 0 | stable | |
| 11 | MUNMAP | addr, len | 0 | stable | |
| 12 | BRK | addr | brk | stable | |
| 13 | RT_SIGACTION | sig, act*, old*, sigsetsize | 0 | stable | v1: доставка сигналів нативним процесам |
| 14 | RT_SIGPROCMASK | how, set*, old*, sigsetsize | 0 | stable | |
| 15 | RT_SIGRETURN | - | - | stable | |
| 16 | IOCTL | fd, req, arg | int | stable | tty/pty-запити (TCGETS, TIOCGWINSZ, TIOCGPTN...) |
| 17 | PREAD64 | fd, buf, len, off | n | stable | |
| 18 | PWRITE64 | fd, buf, len, off | n | stable | |
| 19 | READV | fd, iov*, n | n | stable | |
| 20 | WRITEV | fd, iov*, n | n | stable | |
| 21 | ACCESS | cstr path, mode | 0 | stable | |
| 22 | PIPE | int[2]* | 0 | stable | |
| 23 | SELECT | n, r*, w*, e*, timeval* | n | stable | |
| 24 | SCHED_YIELD | - | 0 | stable | |
| 25 | MREMAP | old, oldlen, newlen, flags, new | addr | stable | |
| 26 | MSYNC | addr, len, flags | 0 | stable | |
| 27 | MINCORE | addr, len, vec* | 0 | stable | |
| 28 | MADVISE | addr, len, advice | 0 | stable | |
| 32 | DUP | fd | fd | stable | |
| 33 | DUP2 | fd, fd2 | fd2 | stable | |
| 34 | PAUSE | - | -EINTR | stable | |
| 35 | NANOSLEEP | timespec*, rem* | 0 | stable | |
| 36 | GETITIMER | which, itimerval* | 0 | stable | |
| 37 | ALARM | secs | secs | stable | |
| 38 | SETITIMER | which, new*, old* | 0 | stable | |
| 39 | GETPID | - | pid | stable | |
| 40 | SENDFILE | out, in, off*, count | n | stable | |
| 41 | SOCKET | domain, type, proto | fd | stable | `AF_UNIX`, `AF_INET`, `AF_INET6` |
| 42 | CONNECT | fd, sockaddr*, len | 0 | stable | |
| 43 | ACCEPT | fd, sockaddr*, len* | fd | stable | |
| 44 | SENDTO | fd, buf, len, flags, sockaddr*, alen | n | stable | |
| 45 | RECVFROM | fd, buf, len, flags, sockaddr*, alen* | n | stable | |
| 46 | SENDMSG | fd, msghdr*, flags | n | stable | `SCM_RIGHTS` передає fd |
| 47 | RECVMSG | fd, msghdr*, flags | n | stable | |
| 48 | SHUTDOWN | fd, how | 0 | stable | |
| 49 | BIND | fd, sockaddr*, len | 0 | stable | |
| 50 | LISTEN | fd, backlog | 0 | stable | |
| 51 | GETSOCKNAME | fd, sockaddr*, len* | 0 | stable | |
| 52 | GETPEERNAME | fd, sockaddr*, len* | 0 | stable | |
| 53 | SOCKETPAIR | domain, type, proto, int[2]* | 0 | stable | |
| 54 | SETSOCKOPT | fd, level, opt, val*, len | 0 | stable | |
| 55 | GETSOCKOPT | fd, level, opt, val*, len* | 0 | stable | |
| 56 | CLONE | flags, stack, ptid*, ctid*, tls | tid | stable | потоки (`CLONE_VM|CLONE_THREAD`) і процеси |
| 57 | FORK | - | pid | stable | |
| 58 | VFORK | - | pid | stable | |
| 59 | EXECVE | cstr path, argv**, envp** | - | stable | особистість нового образу -- з ELF |
| 60 | EXIT | code | - | stable | завершує потік |
| 61 | WAIT4 | pid, status*, options, rusage* | pid | stable | legacy: `(pid, flags)`, повертає код виходу |
| 62 | KILL | pid, sig | 0 | stable | legacy: `(pid)`, завжди завершує (код 143) |
| 63 | UNAME | utsname* | 0 | stable | |
| 72 | FCNTL | fd, cmd, arg | int | stable | |
| 73 | FLOCK | fd, op | 0 | stable | |
| 74 | FSYNC | fd | 0 | stable | |
| 75 | FDATASYNC | fd | 0 | stable | |
| 76 | TRUNCATE | cstr path, len | 0 | stable | |
| 77 | FTRUNCATE | fd, len | 0 | stable | |
| 79 | GETCWD | buf, len | len | stable | |
| 80 | CHDIR | cstr path | 0 | stable | |
| 81 | FCHDIR | fd | 0 | stable | |
| 82 | RENAME | cstr from, cstr to | 0 | stable | |
| 83 | MKDIR | cstr path, mode | 0 | stable | legacy: без `mode` |
| 84 | RMDIR | cstr path | 0 | stable | |
| 85 | CREAT | cstr path, mode | fd | stable | |
| 86 | LINK | cstr from, cstr to | 0 | stable | |
| 87 | UNLINK | cstr path | 0 | stable | |
| 88 | SYMLINK | cstr target, cstr path | 0 | stable | |
| 89 | READLINK | cstr path, buf, len | n | stable | |
| 90 | CHMOD | cstr path, mode | 0 | stable | |
| 91 | FCHMOD | fd, mode | 0 | stable | |
| 92 | CHOWN | cstr path, uid, gid | 0 | stable | legacy: `(path, uid)` |
| 93 | FCHOWN | fd, uid, gid | 0 | stable | |
| 94 | LCHOWN | cstr path, uid, gid | 0 | stable | |
| 95 | UMASK | mask | old | stable | |
| 96 | GETTIMEOFDAY | timeval*, tz* | 0 | stable | |
| 97 | GETRLIMIT | res, rlimit* | 0 | stable | |
| 98 | GETRUSAGE | who, rusage* | 0 | stable | |
| 99 | SYSINFO | sysinfo* | 0 | stable | |
| 100 | TIMES | tms* | ticks | stable | |
| 101 | PTRACE | req, pid, addr, data | -EPERM | stable | налагодження процесів не підтримується; `enlightenment_start` та ін. продовжують без нього |
| 102 | GETUID | - | uid | stable | |
| 104 | GETGID | - | gid | stable | |
| 105 | SETUID | uid | 0 | stable | |
| 106 | SETGID | gid | 0 | stable | |
| 107 | GETEUID | - | uid | stable | |
| 108 | GETEGID | - | gid | stable | |
| 109 | SETPGID | pid, pgid | 0 | stable | |
| 110 | GETPPID | - | pid | stable | |
| 111 | GETPGRP | - | pgid | stable | |
| 112 | SETSID | - | sid | stable | |
| 113 | SETREUID | ruid, euid | 0 | stable | root або повернення до реального uid |
| 114 | SETREGID | rgid, egid | 0 | stable | групи не розрізняються: 0 |
| 115 | GETGROUPS | n, gid_t* | n | stable | |
| 116 | SETGROUPS | n, gid* | 0 | stable | 0 |
| 117 | SETRESUID | ruid, euid, suid | 0 | stable | як SETREUID |
| 118 | GETRESUID | ruid*, euid*, suid* | 0 | stable | |
| 119 | SETRESGID | rgid, egid, sgid | 0 | stable | 0 |
| 120 | GETRESGID | rgid*, egid*, sgid* | 0 | stable | |
| 121 | GETPGID | pid | pgid | stable | |
| 122 | SETFSUID | uid | old | stable | повертає euid |
| 123 | SETFSGID | gid | old | stable | |
| 124 | GETSID | pid | sid | stable | |
| 125 | CAPGET | hdr*, data* | 0 | stable | |
| 126 | CAPSET | hdr*, data* | 0 | stable | |
| 127 | RT_SIGPENDING | set*, size | 0 | stable | |
| 128 | RT_SIGTIMEDWAIT | set*, info*, timespec*, size | sig | stable | |
| 130 | RT_SIGSUSPEND | set*, size | -EINTR | stable | |
| 131 | SIGALTSTACK | ss*, old* | 0 | stable | |
| 132 | UTIME | cstr path, utimbuf* | 0 | stable | |
| 137 | STATFS | cstr path, statfs* | 0 | stable | |
| 138 | FSTATFS | fd, statfs* | 0 | stable | |
| 140 | GETPRIORITY | which, who | prio | stable | |
| 141 | SETPRIORITY | which, who, prio | 0 | stable | |
| 142 | SCHED_SETPARAM | pid, param* | 0 | stable | |
| 143 | SCHED_GETPARAM | pid, param* | 0 | stable | |
| 144 | SCHED_SETSCHEDULER | pid, policy, param* | 0 | stable | |
| 145 | SCHED_GETSCHEDULER | pid | policy | stable | |
| 146 | SCHED_GET_PRIORITY_MAX | policy | n | stable | |
| 147 | SCHED_GET_PRIORITY_MIN | policy | n | stable | |
| 148 | SCHED_RR_GET_INTERVAL | pid, timespec* | 0 | stable | |
| 149 | MLOCK | addr, len | 0 | stable | пам'ять не вивантажується: 0 |
| 150 | MUNLOCK | addr, len | 0 | stable | |
| 151 | MLOCKALL | flags | 0 | stable | |
| 152 | MUNLOCKALL | - | 0 | stable | |
| 157 | PRCTL | op, a2, a3, a4, a5 | int | stable | PR_SET_NAME/PR_GET_NAME та ін. |
| 158 | ARCH_PRCTL | code, addr | 0 | stable | лише x86_64: `ARCH_SET_FS` для TLS |
| 160 | SETRLIMIT | res, rlimit* | 0 | stable | |
| 162 | SYNC | - | 0 | stable | |
| 165 | MOUNT | cstr source, cstr target, cstr fstype, flags, data* | 0 | stable | root; `source` -- `/dev/sdX[N]`, `fstype` -- `hext`, `auto` або NULL; `MS_REMOUNT`/`MS_BIND` -> `EINVAL` |
| 166 | UMOUNT2 | cstr target, flags | 0 | stable | root |
| 169 | REBOOT | magic1, magic2, cmd, arg* | 0 | stable | root; `RB_AUTOBOOT`, `RB_HALT_SYSTEM`, `RB_POWER_OFF`; `CAD_ON/OFF` -- без дії |
| 186 | GETTID | - | tid | stable | |
| 188 | SETXATTR | cstr path, cstr name, value*, size, flags | 0 | stable | hext без xattr: -ENOTSUP |
| 189 | LSETXATTR | cstr path, cstr name, value*, size, flags | 0 | stable | |
| 190 | FSETXATTR | fd, cstr name, value*, size, flags | 0 | stable | |
| 191 | GETXATTR | cstr path, cstr name, value*, size | n | stable | |
| 192 | LGETXATTR | cstr path, cstr name, value*, size | n | stable | |
| 193 | FGETXATTR | fd, cstr name, value*, size | n | stable | |
| 194 | LISTXATTR | cstr path, list*, size | n | stable | |
| 195 | LLISTXATTR | cstr path, list*, size | n | stable | |
| 196 | FLISTXATTR | fd, list*, size | n | stable | |
| 200 | TKILL | tid, sig | 0 | stable | |
| 201 | TIME | time_t* | sec | stable | |
| 202 | FUTEX | uaddr, op, val, timeout*, uaddr2, val3 | int | stable | `WAIT/WAKE/REQUEUE/*_BITSET`; нативний `FUTEX_WAIT/WAKE` 9025/9026 -- синоніми |
| 203 | SCHED_SETAFFINITY | pid, len, mask* | 0 | stable | |
| 204 | SCHED_GETAFFINITY | pid, len, mask* | len | stable | |
| 213 | EPOLL_CREATE | size | fd | stable | |
| 217 | GETDENTS64 | fd, dirent64*, len | n | stable | |
| 218 | SET_TID_ADDRESS | int* | tid | stable | |
| 221 | FADVISE64 | fd, off, len, advice | 0 | stable | |
| 222 | TIMER_CREATE | clock, sigevent*, id* | 0 | stable | |
| 223 | TIMER_SETTIME | id, flags, new*, old* | 0 | stable | |
| 224 | TIMER_GETTIME | id, cur* | 0 | stable | |
| 225 | TIMER_GETOVERRUN | id | n | stable | |
| 226 | TIMER_DELETE | id | 0 | stable | |
| 228 | CLOCK_GETTIME | clock, timespec* | 0 | stable | |
| 229 | CLOCK_GETRES | clock, timespec* | 0 | stable | |
| 230 | CLOCK_NANOSLEEP | clock, flags, req*, rem* | 0 | stable | |
| 231 | EXIT_GROUP | code | - | stable | |
| 232 | EPOLL_WAIT | epfd, events*, n, timeout | n | stable | |
| 233 | EPOLL_CTL | epfd, op, fd, event* | 0 | stable | |
| 234 | TGKILL | tgid, tid, sig | 0 | stable | |
| 235 | UTIMES | cstr path, timeval[2]* | 0 | stable | |
| 247 | WAITID | idtype, id, siginfo*, options | 0 | stable | |
| 253 | INOTIFY_INIT | - | fd | stable | події файлів ще не доставляються |
| 254 | INOTIFY_ADD_WATCH | fd, cstr path, mask | wd | stable | |
| 255 | INOTIFY_RM_WATCH | fd, wd | 0 | stable | |
| 257 | OPENAT | dirfd, cstr path, flags, mode | fd | stable | |
| 258 | MKDIRAT | dirfd, cstr path, mode | 0 | stable | |
| 260 | FCHOWNAT | dirfd, cstr path, uid, gid, flags | 0 | stable | |
| 261 | FUTIMESAT | dirfd, cstr path, timeval[2]* | 0 | stable | |
| 262 | NEWFSTATAT | dirfd, cstr path, stat*, flags | 0 | stable | |
| 263 | UNLINKAT | dirfd, cstr path, flags | 0 | stable | |
| 264 | RENAMEAT | dirfd, cstr, dirfd, cstr | 0 | stable | |
| 265 | LINKAT | dirfd, cstr, dirfd, cstr, flags | 0 | stable | |
| 266 | SYMLINKAT | cstr target, dirfd, cstr path | 0 | stable | |
| 267 | READLINKAT | dirfd, cstr path, buf, len | n | stable | |
| 268 | FCHMODAT | dirfd, cstr path, mode | 0 | stable | |
| 269 | FACCESSAT | dirfd, cstr path, mode | 0 | stable | |
| 270 | PSELECT6 | n, r*, w*, e*, timespec*, sigmask* | n | stable | |
| 271 | PPOLL | pollfd*, n, timespec*, sigmask*, size | n | stable | |
| 273 | SET_ROBUST_LIST | head*, len | 0 | stable | |
| 277 | SYNC_FILE_RANGE | fd, off, len, flags | 0 | stable | |
| 280 | UTIMENSAT | dirfd, cstr path, timespec[2]*, flags | 0 | stable | |
| 281 | EPOLL_PWAIT | epfd, events*, max, timeout, sigmask*, size | n | stable | |
| 282 | SIGNALFD | fd, mask*, size | fd | stable | |
| 283 | TIMERFD_CREATE | clock, flags | fd | stable | |
| 284 | EVENTFD | init | fd | stable | |
| 285 | FALLOCATE | fd, mode, off, len | 0 | stable | |
| 286 | TIMERFD_SETTIME | fd, flags, new*, old* | 0 | stable | |
| 287 | TIMERFD_GETTIME | fd, cur* | 0 | stable | |
| 288 | ACCEPT4 | fd, sockaddr*, len*, flags | fd | stable | |
| 289 | SIGNALFD4 | fd, mask*, size, flags | fd | stable | |
| 290 | EVENTFD2 | init, flags | fd | stable | |
| 291 | EPOLL_CREATE1 | flags | fd | stable | |
| 292 | DUP3 | fd, fd2, flags | fd2 | stable | |
| 293 | PIPE2 | int[2]*, flags | 0 | stable | |
| 294 | INOTIFY_INIT1 | flags | fd | stable | |
| 302 | PRLIMIT64 | pid, res, new*, old* | 0 | stable | |
| 306 | SYNCFS | fd | 0 | stable | |
| 309 | GETCPU | cpu*, node*, cache* | 0 | stable | |
| 316 | RENAMEAT2 | dirfd, cstr, dirfd, cstr, flags | 0 | stable | RENAME_NOREPLACE |
| 318 | GETRANDOM | buf, len, flags | n | stable | |
| 319 | MEMFD_CREATE | cstr name, flags | fd | stable | іменований SHM-об'єкт; мапиться `MMAP` |
| 322 | EXECVEAT | dirfd, cstr path, argv*, envp*, flags | - | stable | |
| 324 | MEMBARRIER | cmd, flags, cpu | int | stable | |
| 332 | STATX | dirfd, cstr, flags, mask, statx* | 0 | stable | |
| 424 | PIDFD_SEND_SIGNAL | pidfd, sig, info*, flags | 0 | stable | v1.0 |
| 434 | PIDFD_OPEN | pid, flags | fd | stable | v1.0; fd стає читабельним, коли процес завершився |
| 436 | CLOSE_RANGE | first, last, flags | 0 | stable | |
| 439 | FACCESSAT2 | dirfd, cstr path, mode, flags | 0 | stable | |
| 441 | EPOLL_PWAIT2 | epfd, events*, max, timespec*, sigmask*, size | n | stable | |

### 3.2 Legacy-семантика (ревізія 1)

Для програм із ревізією ноти `1` (усе, що зібрано з `hamix_std` 0.6) ядро
зберігає старі значення аргументів у `WAIT4`, `KILL`, `MKDIR`, `UNLINK`,
`RENAME`, `CHMOD`, `CHOWN`, `MMAP` (див. колонку `note`) і не доставляє сигнали.
Ревізія `0x0100` і Linux-особистість отримують точну семантику.

## 4. Діапазон hamix (9000-9999): нативні можливості

| nr | name | args | returns | status | note |
|---|---|---|---|---|---|
| 9001 | HAMIX_FBMAP | fbinfo[24]* | 0 | stable | мапить фреймбуфер; `{addr u64, pitch u32, w u32, h u32, bpp u32}` |
| 9002 | HAMIX_READKEY | - | key | stable | блокуюче читання коду клавіші з консолі/pty |
| 9003 | HAMIX_TRUNCATE | cstr path | 0 | deprecated | створює/обнуляє файл; замінено `OPEN(O_CREAT|O_TRUNC)` |
| 9004 | HAMIX_MOUSE | mouse[16]* | 0 | stable | `{x i32, y i32, buttons|presses u32, wheel i32}` |
| 9005 | HAMIX_POLLKEY | - | key / -100 | stable | неблокуюче `READKEY` |
| 9006 | HAMIX_RELEASE_FB | - | 0 | stable | віддає дисплей |
| 9010 | HAMIX_SPAWN | str path, args*, (flags<<32)|argc, stdio, env_hdr* | pid | stable | `posix_spawn`-еквівалент: `DETACH=1, FOREGROUND=2, ROOT=4, ENV=8`; `stdio` -- 4x16 біт `fd+1` |
| 9012 | HAMIX_WAITPID | pid, flags | code | stable | `flags&1` = не чекати (`-EAGAIN`) |
| 9013 | HAMIX_SLEEP | ms | 0 | stable | |
| 9014 | HAMIX_WAIT_EVENT | mask, timeout_ms | ready | stable | `INPUT=1, MESSAGE=2, PIPE=4`; буде підмножиною `WAIT_ANY` |
| 9015 | HAMIX_KILL | pid | 0 | stable | завершити процес (перевірка власника) |
| 9016 | HAMIX_PROC_LIST | buf, len | n | stable | текст: `pid ppid state vt name kib cpu_ms uid cpu personality` |
| 9017 | HAMIX_SYSINFO | buf, 56/64 | 0 | stable | `total free heap_total heap_free uptime_ms tasks epoch [page_cache]` |
| 9018 | HAMIX_PROC_ALIVE | pid | 0/1 | stable | |
| 9020 | HAMIX_MSG_SEND | pid, buf, len | 0 | deprecated | повідомлення в поштову скриньку процесу -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9021 | HAMIX_MSG_RECV | buf, len, sender*, timeout_ms, from | n | stable | `from != 0` -- лише повідомлення від цього pid, решта лишається в черзі (v1.0) -- лише повідомлення ядра (pid 0); решта -- `docs/reference/ipc.md` |
| 9022 | HAMIX_SERVICE_REGISTER | str name | 0 | deprecated | іменований сервіс-порт -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9023 | HAMIX_SERVICE_LOOKUP | str name | pid | deprecated | -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9024 | HAMIX_MAILBOX_FD | - | fd | stable | fd, готовий до читання, коли є повідомлення (для `EPOLL`) -- лише повідомлення ядра (pid 0); решта -- `docs/reference/ipc.md` |
| 9025 | HAMIX_FUTEX_WAIT | uaddr, val, timeout_ns | 0 | stable | v1.0; `-EAGAIN`, якщо `*uaddr != val`; `timeout_ns=u64::MAX` -- без тайм-ауту |
| 9026 | HAMIX_FUTEX_WAKE | uaddr, n | woken | stable | v1.0 |
| 9027 | HAMIX_WAIT_ANY | waitset*, n, timeout_ns | ready | stable | v1.0; `{kind u32, flags u32, id u64, ready u64}[n]`, `kind`: FD=1, MAILBOX=2, CHILD=3, TIMER=4 |
| 9028 | HAMIX_ABI_INFO | buf, len | version | stable | v1.0; повертає `HAMIX_ABI_VERSION`, у буфер -- текст `version personality linux=on/off` |
| 9029 | HAMIX_SERVICE_ACTIVATOR | str name | 0 | deprecated | v1.0; root; ледача активація: `SERVICE_LOOKUP(name)` без живого сервісу надсилає активатору повідомлення `activate <name>` від pid 0 і чекає до 5 с на `SERVICE_REGISTER` -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9030 | HAMIX_SHM_CREATE | len | id | deprecated | -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9031 | HAMIX_SHM_MAP | id, len_out* | addr | deprecated | -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9032 | HAMIX_SHM_RELEASE | id | 0 | deprecated | -- замінено на AF_UNIX/`shm_open`, `docs/reference/ipc.md` |
| 9040 | HAMIX_CMD_REGISTER | str name, args*, argc | 0 | deprecated | реєстр команд hsh -> файл `/etc/hamix/commands.d` (userland) |
| 9041 | HAMIX_CMD_UNREGISTER | str name | 0 | deprecated | див. `CMD_REGISTER` |
| 9042 | HAMIX_CMD_LIST | buf, len | n | deprecated | див. `CMD_REGISTER` |
| 9043 | HAMIX_CMD_RUN | str name, args*, (flags<<32)|argc | pid | deprecated | див. `CMD_REGISTER` |
| 9050 | HAMIX_SYNC | - | 0 | deprecated | = `SYNC` (162) |
| 9051 | HAMIX_READDIR | str path, buf, len | n | stable | текст `name kind size mode owner`; для C -- `GETDENTS64` |
| 9052 | HAMIX_STAT | cstr path, buf[24] | 0 | deprecated | = `STAT` (4) |
| 9060 | HAMIX_REALTIME | - | epoch | stable | секунди UNIX |
| 9070 | HAMIX_DISK_LIST | buf, len | n | stable | текст дисків і розділів |
| 9071 | HAMIX_DISK_IO | cstr dev, lba, buf, sectors, write | 0 | stable | root; сирий доступ до тому |
| 9072 | HAMIX_DISK_RESCAN | - | 0 | stable | |
| 9073 | HAMIX_MKFS | cstr dev, cstr label | 0 | deprecated | -> утиліта `mkfs.hext` поверх `DISK_IO` |
| 9074 | HAMIX_MOUNT | cstr dev, cstr path | 0 | stable | root |
| 9075 | HAMIX_UMOUNT | cstr path | 0 | stable | root |
| 9090 | HAMIX_PIPE | int[2]* | 0 | deprecated | = `PIPE2` (293) |
| 9093 | HAMIX_ISATTY | fd | 0/1 | stable | |
| 9094 | HAMIX_TERMSIZE | fd | (cols<<16)|rows | stable | |
| 9095 | HAMIX_SET_TERMINAL | fd, cols, rows | 0 | stable | позначає pipe/pty як термінал |
| 9096 | HAMIX_SET_FOREGROUND | pid | 0 | stable | передній процес термінала |
| 9097 | HAMIX_TERMGEN | fd | gen | stable | лічильник змін розміру |
| 9098 | HAMIX_SESSION_ATTACH | tty | 0 | stable | v1.0; root; викликач стає сесією консолі `tty` (1..N), fd 0-2 -> консоль; використовує `hxlogin` |
| 9103 | HAMIX_TRACE | str name | 0 | stable | root; трасування викликів процесу в serial |
| 9104 | HAMIX_PROF | op | running | stable | v1.0; root; семплінговий профайлер: `0` стоп, `1` старт, `2` скинути; звіт -- `/proc/hxprof` |
| 9110 | HAMIX_POWER | mode | - | stable | `0` halt, `1` reboot, `2` poweroff |
| 9111 | HAMIX_LINUX_CONTROL | op | state | stable | v1.0; `0` status, `1` on, `2` off; root для 1/2 |
| 9120 | HAMIX_CPU_STATS | buf, len | n | stable | |
| 9130 | NET_STATUS | buf, len | n | stable | |
| 9131 | NET_SET_MODE | mode | 0 | stable | `0` off, `1` ethernet, `2` wifi |
| 9132 | WIFI_SCAN | buf, len, rescan | n | stable | |
| 9133 | WIFI_CONNECT | str ssid, str pass | 0 | stable | |
| 9134 | WIFI_DISCONNECT | - | 0 | stable | |
| 9135 | NET_CONFIGURE | -, -, str spec | 0 | stable | root |
| 9136 | WIFI_KNOWN | buf, len | n | stable | |
| 9137 | WIFI_FORGET | str ssid | 0 | stable | |
| 9138 | WIFI_AUTOJOIN | str ssid, on | 0 | stable | |
| 9140 | NET_SOCKET | kind, port | id | deprecated | нативні TCP/UDP-сокети 0.6; нове -- `SOCKET` (41) |
| 9141 | NET_SOCKET_CONNECT | id, (ip<<16)|port, timeout | 0 | deprecated | |
| 9142 | NET_SOCKET_SEND | id, buf, len | n | deprecated | |
| 9143 | NET_SOCKET_RECV | id, buf, len, timeout | n | deprecated | |
| 9144 | NET_SOCKET_CLOSE | id | 0 | deprecated | |
| 9145 | RESOLVE | str name, ip[4]* | 0 | stable | DNS-резолвер ядра |
| 9146 | PING | ip, seq, timeout_ms | rtt | stable | |
| 9147 | NET_SOCKET_LISTEN | id, port | 0 | deprecated | |
| 9148 | NET_SOCKET_STATE | id | state | deprecated | |
| 9149 | NET_SOCKET_SENDTO | id, (ip<<16)|port, buf, len | n | deprecated | |
| 9150 | NET_SOCKET_RECVFROM | id, buf, len, timeout, meta[8]* | n | deprecated | |
| 9160 | DISPLAY_INFO | buf, len | n | stable | |
| 9161 | DISPLAY_SET | (w<<32)|h, bpp, persist | 0 | stable | |
| 9162 | DISPLAY_CONFIRM | - | 0 | stable | |
| 9163 | DISPLAY_REVERT | - | 0 | stable | |
| 9164 | FB_GENERATION | - | gen | stable | |
| 9165 | DISPLAY_EDID | output, buf, len | n | stable | |
| 9166 | DISPLAY_OUTPUT | output, (w<<32)|h | 0 | stable | |
| 9170 | AUDIO_OPEN | rate, channels, format | stream | stable | |
| 9171 | AUDIO_WRITE | stream, buf, len, flags | n | stable | `flags&1` = не блокувати |
| 9172 | AUDIO_STATUS | stream, buf, len | 0 | stable | |
| 9173 | AUDIO_CONTROL | stream, op, arg | int | stable | |
| 9174 | AUDIO_CLOSE | stream, drain | 0 | stable | |
| 9175 | AUDIO_VOLUME | op, arg | word | stable | `0` get, `1` set, `2` mute, `3` step, `4` toggle |
| 9176 | AUDIO_INFO | buf, len | n | stable | |
| 9180 | GPU_CAPS | - | caps | stable | |
| 9181 | GPU_FLUSH | (x<<32)|y, (w<<32)|h | 0 | stable | власник дисплея |
| 9182 | GPU_FILL | (x<<32)|y, (w<<32)|h, argb | 0 | stable | |
| 9183 | GPU_COPY | src, dst, size | 0 | stable | |
| 9184 | GPU_CURSOR_SET | argb*, (w<<32)|h, (hx<<32)|hy | 0 | stable | |
| 9185 | GPU_CURSOR_MOVE | (x<<32)|y | 0 | stable | |
| 9186 | GPU_CURSOR_HIDE | - | 0 | stable | |
| 9187 | GPU_FLIP | buffer | 0 | stable | |
| 9188 | GPU_VBLANK | timeout_ms | 0 | stable | |
| 9189 | GPU_BUFFERS | - | n | stable | |

## 5. Аудит: що потрібно hxlibc і чого бракувало

| Потреба hxlibc | Чим закрито в v1.0 |
|---|---|
| файли, каталоги, `*at`-виклики, `stat` | діапазон core (Linux-розкладка `struct stat`) |
| `malloc` | `MMAP`/`MUNMAP`/`BRK`, `MADVISE(DONTNEED)` повертає пам'ять |
| потоки, TLS, `pthread_mutex` | `CLONE`, `ARCH_PRCTL`/`tp`, `SET_TID_ADDRESS`, `FUTEX` + нативні `FUTEX_WAIT/WAKE` |
| `fork/exec/wait` | `FORK`, `VFORK`, `EXECVE`, `WAIT4` (точна семантика з ревізії v1) |
| сигнали | `RT_SIG*`, доставка нативним процесам із ревізії v1, кадр -- Linux-розкладка |
| `poll/select`, події | `POLL`, `PPOLL`, `EPOLL_*`, `EVENTFD2`, `TIMERFD_*`; нативний `WAIT_ANY` для портів |
| сокети | `SOCKET` (41) і родина: `AF_UNIX`, `AF_INET`, `AF_INET6` |
| час | `CLOCK_GETTIME/GETRES/NANOSLEEP`, `GETTIMEOFDAY` |
| випадкові числа | `GETRANDOM` |
| tty | `IOCTL` (TCGETS/TCSETS/TIOCGWINSZ/TIOCGPTN/TIOCSPTLCK) |
| версія ABI і стан шару Linux | `HAMIX_ABI_INFO`, `HAMIX_LINUX_CONTROL` |

Не syscall-и (виносяться з ядра, статус `deprecated`): `CMD_*`, `MKFS`, `AUTH`,
`SET_OWN_PASSWORD`, дублікати `SYNC/STAT/PIPE/TRUNCATE`, нативні `SOCKET*` 0.6.

## 6. Linux-особистість

Процес Linux-особистості бачить повну таблицю Linux для своєї архітектури
(на aarch64/riscv64 -- generic-нумерація, перекладається в ядрі); усе, чого немає в
діапазоні core (netlink, `/proc` у форматі Linux, xattr,
`signalfd`, `personality`, `capget`...), існує лише в Linux-шарі.

## Видалені виклики

Номери не використовуються повторно.

* `9100 HAMIX_AUTH`, `9102 HAMIX_SET_OWN_PASSWORD` (2026-10-06) -- `hx-authd` (сокет), паролі в
  `/etc/shadow` як `crypt(3)` SHA-512 (`$6$`).
* `9101 HAMIX_USERS_RELOAD` (2026-10-06) -- ядро більше не тримає базу користувачів; облікові
  записи є лише в `/etc/passwd`, `/etc/shadow`, `/etc/group` образу.
* `9105 HAMIX_GRANT_TICKET` (2026-10-06) -- setuid-програми (`sudo`, `su`).
