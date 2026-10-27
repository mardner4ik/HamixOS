# hxlinuxulator: running Linux binaries on HamixOS

hxlinuxulator lets unmodified x86_64 Linux programs linked against **musl**
run on HamixOS next to native programs. All milestones of the plan are done:
M1 (kernel foundation), M2 (hsh integration), M3 (BusyBox bring-up), M4
(AF_UNIX sockets with fd passing) and M5 (the `hxwayland` bridge, which shows
Wayland windows on the Nook desktop). On top of that the kernel now has real
signal delivery, threads, `fork`/`execve`, pseudo-terminals and
timerfd/eventfd/signalfd, so full-screen terminal programs (htop, btop, nano,
vim, mc, tmux), shells (bash, BusyBox `sh` with pipelines), Python and
Wayland clients such as `foot` run.

## Trying it

```sh
./tools/linux-sysroot/build.sh
./build.sh
```

The first script downloads Alpine's `musl` and `musl-dev` packages, installs
the dynamic linker into `overlay/opt/linux/lib/`, and builds every
`tools/linux-sysroot/src/*.c` twice: `NAME-musl` (dynamic PIE) and
`NAME-musl-static` (static PIE). You need `clang`, `lld` and `llvm-strip`.
After you boot the image:

```
root@hamix:~# /opt/linux/bin/hello-musl a b
root@hamix:~# /opt/linux/bin/hello-musl-static
root@hamix:~# /opt/linux/lib/ld-musl-x86_64.so.1 /opt/linux/bin/hello-musl
```

## How a binary is classified

`kernel/src/task/elf.rs` (`Elf::classify`):

1. A `PT_NOTE` with owner `Hamix` and type `0x48584f53` means **native**.
   `apps/link.ld` emits this note (`.note.hamix`) into every native app, so
   after a rebuild, classification never has to guess.
2. `ET_DYN`, or any binary with `PT_INTERP`, is **Linux**.
3. An `ET_EXEC` without the note is **native** if it is linked inside the
   HamixOS user area (`>= 0x80_0000_0000`); this covers native binaries
   built before the note existed. Anything else is **Linux**.

`EI_OSABI` must be `SYSV` (0) or `GNU` (3). The native toolchain also emits
0, so this field cannot tell the two apart.

`Task.abi` (`Abi::Native` / `Abi::Linux`) records the result.

## Loading

* Every process (native too) now gets the full System V startup stack:
  `argc, argv, NULL, envp, NULL, auxv, AT_NULL`, where auxv holds
  `AT_PHDR/PHENT/PHNUM/PAGESZ/BASE/FLAGS/ENTRY/UID/EUID/GID/EGID/SECURE/
  RANDOM/PLATFORM/HWCAP/HWCAP2/CLKTCK/EXECFN`. If the program headers are
  not inside a loaded segment, the kernel copies them onto the stack for
  `AT_PHDR`.
* `ET_DYN` executables are loaded at `0x80_0040_0000`.
* When a binary has `PT_INTERP`, `kernel/src/task/linux_loader.rs` loads the
  interpreter at `0x90_0000_0000`. `AT_BASE` holds the interpreter base,
  `AT_ENTRY` the program's entry point, and execution starts at the
  interpreter's entry point.
* Static non-PIE Linux binaries (linked at `0x400000`) are rejected because
  that address lies outside the HamixOS user area. Rebuild them with
  `-static-pie`.
* The environment is described in the next section.

## Environment and hsh integration (M2)

* `SYS_HAMIX_SPAWN` (9010) accepts an environment. The caller sets the
  `SPAWN_ENV` (8) flag and passes a pointer in the sixth argument to
  `[env_ptr: u64, env_len: u64]`. `env_ptr` points to a block of
  NUL-separated `NAME=value` strings, the same format as the argument
  block. Entries without `=` are dropped, and at most 1024 entries are
  accepted.
* If the caller passes no environment, the child inherits the parent's
  startup environment (`Task.env`). If that is empty too (kernel callers
  such as login), the child gets `elf::default_env()`: `PATH`, `HOME`,
  `USER`, `LOGNAME`, `SHELL`, `TERM` and `LANG`.
* The default `PATH` everywhere is
  `/usr/bin:/bin:/sbin:/usr/sbin:/opt/linux/bin:/opt/linux/usr/bin`, so
  hsh finds sysroot programs by name.
* `hamix_std::env::vars()` / `env::var()` parse envp for native programs.
  `sys::spawn_io_env()` is the spawn wrapper that takes an environment.
* hsh imports its environment at startup and marks those variables as
  exported. It always exports `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`,
  `PWD` and `TERM`, and appends any missing default `PATH` entries.
  Every program hsh starts receives the exported variables, including
  `hsh -c` subshells, pipeline stages, background jobs, `startx`, `sh` and
  `sudo`. A prefix assignment (`FOO=1 prog`) is exported to that one
  command only. `unset` removes the export.
* `env` prints the exported environment, `export` with no arguments prints
  it as `export NAME=value` lines, and `set` still prints every shell
  variable.
* `type NAME` shows the binary kind: `native`, `linux, dynamically
  linked`, `linux, static-pie`, or `linux, static (unsupported: not PIE)`.
  `ps` has an `ABI` column (`native`, `linux`, `kernel`). The kernel
  appends it as a 10th tab-separated field of the process list, and
  `sys::ProcInfo::abi` exposes it.
* Loader failures (a bad ELF file or a missing interpreter) now return
  `ENOEXEC` instead of `ENOENT`, and the kernel logs the reason on serial
  as `spawn: PATH: reason`. hsh reports this as `cannot execute binary
  (KIND)`.

The sysroot ships `printenv-musl` / `printenv-musl-static`
(`tools/linux-sysroot/src/printenv.c`) for testing this.

## The `/opt/linux` sysroot

For an `Abi::Linux` task, absolute paths used by the lookup syscalls
(`open`, `openat`, `stat`, `lstat`, `newfstatat`, `access`, `faccessat`,
`readlink(at)`) and the `PT_INTERP` path are first tried under
`/opt/linux`. If that path does not exist, the real root is used. So
`/lib/ld-musl-x86_64.so.1` resolves to the sysroot copy, while
`/proc/self/exe` or `/tmp/x` still reach the real files. A new file (or
directory) is created at the real path when its real parent directory
exists; when only the prefixed parent exists (for example
`/usr/share/fonts/dejavu/fonts.dir` or `/var/cache/fontconfig/...`) it is
created inside `/opt/linux`. `mkdirat`, `unlinkat` and `renameat` use the
same mapping, so `fc-cache`, `glib-compile-schemas` and
`gdk-pixbuf-query-loaders` can write their caches.

## Syscalls added or changed for M1

| # | name | notes |
|---|------|-------|
| 4/6 | stat / lstat | via `newfstatat` |
| 5 | fstat | now reports a real `st_dev`/`st_ino` (musl's `ld.so` uses them to avoid loading a library twice), plus `st_uid`/`st_gid` and times |
| 9 | mmap | Linux tasks: `MAP_FIXED`, `MAP_FIXED_NOREPLACE`, anonymous and file-backed (`MAP_PRIVATE` copy). Native tasks keep the old anonymous-only path |
| 10 | mprotect | validated, then treated as a no-op (all user pages are RWX) |
| 13/14 | rt_sigaction / rt_sigprocmask | stubs that zero the returned old value |
| 16 | ioctl | `TCGETS`, `TCSETS*`, `TIOCGWINSZ`, `TIOCSWINSZ`, `TIOCGPGRP`, `TIOCSPGRP`, `FIONREAD`, `FIONBIO`, `FIOCLEX`; returns `ENOTTY` for anything else and for non-terminal fds |
| 17 / 19 | pread64 / readv | |
| 21 / 269 | access / faccessat | |
| 28 | madvise | no-op |
| 72 | fcntl | `F_GETFD/SETFD/GETFL/SETFL/DUPFD/DUPFD_CLOEXEC` |
| 89 / 267 | readlink / readlinkat | `/proc/self/exe` and `/proc/<own pid>/exe`; other existing paths return `EINVAL` because the VFS has no symlinks |
| 104/108 | getgid / getegid | |
| 131 | sigaltstack | stub |
| 158 | arch_prctl | `ARCH_SET_FS`/`ARCH_GET_FS` are real. `Task.fs_base` is loaded into `IA32_FS_BASE` on every context switch. The GS codes are not supported |
| 202 | futex | `WAKE` is a no-op. `WAIT` checks the value, sleeps for the timeout, and returns `EINTR` if no timeout was given |
| 257 | openat | `AT_FDCWD` or an absolute path |
| 262 | newfstatat | also supports `AT_EMPTY_PATH` |
| 273 | set_robust_list | stub |
| 318 | getrandom | backed by `kernel/src/random.rs` (RDRAND when the CPU has it, mixed with a TSC-seeded generator), which also serves `/dev/urandom` |

The syscall trampoline now forwards the sixth argument (`r9`).

Any syscall number the kernel does not handle is logged on the serial port
as `syscall: unimplemented N (pid P, abi linux|native)`. Use this to find
gaps while bringing up new programs.

`/proc/self/exe` (a file containing the executable's path) and
`/proc/self/maps` (a stub showing heap, mmap area and stack) exist as
volatile proc nodes.

## BusyBox and syscall coverage (M3)

`tools/linux-sysroot/build.sh` also installs the following into the
sysroot:

* `/opt/linux/bin/busybox`: BusyBox 1.37.0, built from source (the script
  checks the tarball's SHA-256) as a **static PIE** with
  `tools/linux-sysroot/musl-cc`, a clang wrapper around Alpine's musl. Its
  config is `defconfig` with `tc` turned off. Alpine's `busybox-static` is
  a non-PIE binary linked at `0x400000`, which HamixOS cannot load.
* `/opt/linux/bin/busybox-dynamic`: Alpine's dynamically linked PIE
  BusyBox, which runs through `ld-musl`.
* Applet wrappers such as `/opt/linux/bin/ls`, each a one-line script
  `#!/opt/linux/bin/busybox`. The VFS has no symlinks, and BusyBox takes
  the applet name from the basename of `argv[1]`. Native commands and hsh
  builtins come first in `PATH`, so call the Linux version by its full
  path (`/opt/linux/bin/ls`) or as `busybox ls`.
* `/opt/linux/etc/group`. Linux programs see it as `/etc/group` because
  of the sysroot path rewrite.

Verified in QEMU:

* Tools: `ls -la`, `cat`, `echo`, `printf`, `head`, `tail`, `wc`,
  `sort`, `grep`, `sed`, `awk`, `find`, `stat`, `du`, `df`, `uname`, `id`,
  `date`, `sleep`, `seq`, `md5sum`, `sha256sum`, `base64`, `hexdump`,
  `touch`, `mkdir -p`, `cp`, `mv`, `rm -r`, `rmdir`, `chmod`,
  `truncate`, `realpath`, `readlink -f`, `which`, `expr`, `free`,
  `uptime`, `ps`, `dd`, `tar`, `tty`, `nproc`, `mktemp`, `yes | head`.
* Scripts with only shell builtins: `busybox sh -c`.
* Interactive programs: `cat` reading the keyboard (canonical mode),
  `vi` and `less`, both on the text console and in the Nook terminal.

New kernel pieces:

* **Directory fds and `getdents64`.** A Linux task (or any task that
  passes `O_DIRECTORY`) can `open` a directory read-only.
  `OpenFile::Dir` stores a snapshot of the entries, including `.` and
  `..`. `openat`, `newfstatat` and friends accept a directory fd as
  `dirfd`, and `fchdir` works.
* **Synthetic procfs** (`kernel/src/syscall/linux/procfs.rs`).
  * `/proc/<pid>` and `/proc/self` provide `stat`, `statm`, `status`,
    `cmdline`, `comm` and `maps`, the links `exe` and `cwd`, and an `fd/`
    directory whose entries link to `/dev/console`, `pipe:[N]` or file
    paths.
  * Listing `/proc` shows one directory per process.
  * Files are generated when opened (`OpenFile::Mem`).
  * `stat`, `open` and `access` follow these links; `lstat` and
    `readlink` do not.
  * Linux tasks see this view first. Native tasks only see it where the
    VFS has no real node.
  * `Task` now also stores `args` (for `cmdline`) and its start tick.
* **Terminal layer** (`kernel/src/syscall/linux/tty.rs`).
  * Each task keeps a termios. `TCGETS` and `TCSETS*` read and write it.
  * Canonical mode (`ICANON`) does line editing with echo, erase, kill
    and EOF.
  * Raw mode honours `VMIN`/`VTIME` and `ICRNL`. Keys are translated to
    VT100 sequences (arrows, Home/End, Delete, PgUp/PgDn).
  * This applies to Linux tasks reading the console or a terminal pipe
    (hxterm). Echo goes to the console, or to the terminal pipe on
    fd 1/2.
  * `poll` and `ppoll` support files, directories, pipes and the console.
* **Alternate screen.** The text console and hxterm now handle
  `ESC[?1049h/l` (also 47/1047), so the shell screen comes back after
  `vi` or `less` exits.
* **SIGPIPE.** A Linux task that writes to a pipe with no readers is
  terminated with status 141, as the default SIGPIPE action would do.
* **More syscalls:**

| # | name | notes |
|---|------|-------|
| 7 / 271 | poll / ppoll | |
| 22 / 293 | pipe / pipe2 | same pipes as `SYS_HAMIX_PIPE` |
| 32 / 33 / 292 | dup / dup2 / dup3 | the fd table grows up to 256; a `dup` of an implicit console fd becomes `OpenFile::Console` |
| 35 / 230 | nanosleep / clock_nanosleep | `TIMER_ABSTIME` supported |
| 40 | sendfile | |
| 76 / 77 | truncate / ftruncate | any length (`Vfs::set_len`) |
| 81 | fchdir | |
| 83 / 258 | mkdir / mkdirat | the mode is applied with the task's umask |
| 84 / 87 / 263 | rmdir / unlink / unlinkat | Linux semantics: `EISDIR`, `ENOTDIR`, `ENOTEMPTY` |
| 82 / 264 / 316 | rename / renameat / renameat2 | replaces an existing file; supports `RENAME_NOREPLACE` |
| 90 / 91 / 268 | chmod / fchmod / fchmodat | |
| 92 / 93 / 94 / 260 | chown / fchown / lchown / fchownat | |
| 95 | umask | per task |
| 99 | sysinfo | |
| 115 | getgroups | the user's primary group |
| 137 / 138 | statfs / fstatfs | page-based RAM figures; `df` reads `/proc/mounts` |
| 204 | sched_getaffinity | one bit per CPU |
| 132 / 235 / 261 / 280 | utime / utimes / futimesat / utimensat | only check that the file exists; timestamps are not stored |
| 217 | getdents64 | |

## AF_UNIX sockets, fd passing, memfd and epoll (M4)

### Deviation from the plan

The plan suggested carrying socket traffic over the per-process mailbox
(`msg_send`/`msg_recv`) and passing fds as shared-memory ids. That does
not fit well:

* The mailbox is one queue per *process*, while sockets need one queue
  per *connection*.
* `SCM_RIGHTS` has to pass any fd (pipes, sockets, files), not only
  memory.

So `kernel/src/net/unix.rs` keeps its own socket table, built like the
pipe table. Passed fds travel inside the kernel: the sender's `OpenFile`
is duplicated into the message, and `recvmsg` installs it into the
receiver's fd table.

The mailbox still takes part in multiplexed waiting:
`SYS_HAMIX_MAILBOX_FD` (9024) returns an fd that becomes readable when
the calling process has a mailbox message. A native program such as the
M5 bridge can therefore `poll`/`epoll` on its Wayland sockets and its
hxproto mailbox at the same time.

### Sockets

* Supported families and types: `AF_UNIX` with `SOCK_STREAM`,
  `SOCK_DGRAM` and `SOCK_SEQPACKET`, plus `SOCK_NONBLOCK`. Any other
  family returns `EAFNOSUPPORT`.
* Syscalls: `socket`, `socketpair`, `bind`, `listen`, `connect`,
  `accept`/`accept4`, `sendto`/`recvfrom`, `sendmsg`/`recvmsg`,
  `shutdown`, `getsockname`/`getpeername`, `setsockopt` (accepted and
  ignored) and `getsockopt`. `getsockopt` supports `SO_TYPE`,
  `SO_ERROR`, `SO_PEERCRED`, `SO_SNDBUF`/`SO_RCVBUF`, `SO_ACCEPTCONN`,
  `SO_DOMAIN` and `SO_PROTOCOL`.
* Addresses: filesystem paths and the abstract namespace (a leading NUL
  byte).
  * `bind` creates a placeholder file, which `stat` reports as
    `S_IFSOCK`. The path must not exist yet (`EADDRINUSE`), as on Linux.
  * `unlink` removes the name.
  * `connect` to a path that exists but has no listener returns
    `ECONNREFUSED`.
* Stream sockets merge writes into one byte stream. A read stops at a
  message boundary that carries fds, and the fds arrive with the first
  byte of their message. Datagram and seqpacket sockets keep message
  boundaries, and `MSG_TRUNC` is supported.
* Flags: `MSG_PEEK`, `MSG_DONTWAIT`, `MSG_WAITALL` and `MSG_NOSIGNAL`.
  Without `MSG_NOSIGNAL`, a Linux task that sends to a closed peer exits
  with 141 (SIGPIPE).
* `SCM_RIGHTS` carries up to 253 fds per message. If the control buffer
  is too small, the extra fds are closed and `MSG_CTRUNC` is set.
* `read`/`write`, `poll`/`epoll`, `fcntl(F_GETFL/F_SETFL O_NONBLOCK)`,
  `ioctl(FIONBIO/FIONREAD)` and `fstat` work on socket fds.
  `/proc/<pid>/fd` shows sockets as `socket:[N]`.
* Buffers hold 256 KiB per socket, and a datagram can be at most
  212 KiB.

### memfd and shared mappings

* `memfd_create` (319) returns an fd backed by a kernel shared-memory
  segment, the same kind that `SYS_HAMIX_SHM_*` uses.
* `ftruncate` grows the segment by adding zeroed frames; existing frames
  never move, so current mappings stay valid. Shrinking zeroes the tail.
* `read`, `write`, `pread`, `pwrite` (`pwrite64` is new and also works
  on regular files) and `lseek` work on a memfd.
* `mmap(MAP_SHARED)` of a memfd maps the segment's frames, either in the
  shm area or at a `MAP_FIXED` address. Each mapping holds a reference
  to the segment, so it stays valid after every fd is closed. `munmap`
  releases it.
* `mmap(MAP_PRIVATE)` of a memfd gives a private copy.
* `F_ADD_SEALS`/`F_GET_SEALS` are accepted but do nothing.
* Native programs reach the Linux `mmap` path by passing any of
  `MAP_SHARED`, `MAP_PRIVATE` or `MAP_ANONYMOUS`. The old native
  anonymous call (flags = 0) behaves as before.

### epoll

* Syscalls: `epoll_create`/`epoll_create1`, `epoll_ctl` (ADD/MOD/DEL)
  and `epoll_wait`/`epoll_pwait`.
* Only level-triggered mode exists: `EPOLLET` is treated as level.
  `EPOLLONESHOT` and `EPOLLRDHUP` are supported.
* Interest lists are keyed by fd number, and closed fds drop out
  automatically.
* An epoll fd is itself pollable. Nesting one epoll inside another
  returns `ELOOP`.
* `poll`, `ppoll` and `epoll_wait` sleep until input, pipe, socket or
  mailbox activity.

### Native API

`hamix_std::unix` wraps all of this for native programs:

* `listen_at`, `connect_to`, `accept`
* `send_with_fds`, `recv_with_fds`, `peer_cred`, `set_nonblocking`
* `poll`, `epoll_*`, `mailbox_fd`
* `memfd_create`, `ftruncate`, `map_shared`, `unmap`, `pread`, `pwrite`

### Verification

`tools/linux-sysroot/src/unixsock.c` (`unixsock-musl` and
`unixsock-musl-static`) is the test program.

* `unixsock-musl selftest` checks, in one process: stream and datagram
  socketpairs, peeking, non-blocking `EAGAIN`, `poll`, `shutdown`/EOF,
  `EPIPE`, abstract `bind`/`sendto`/`recvfrom`, memfd
  write/grow/mmap/munmap, `/proc/self/fd`, and epoll add/wait/del. It
  also checks that an epoll fd is pollable.
* `unixsock-musl server /tmp/x &` together with
  `unixsock-musl-static client /tmp/x` runs the plan's round trip:
  1. The client passes a memfd and the write end of a pipe over
     `SCM_RIGHTS`.
  2. The server reads the memfd, maps it and writes into the mapping.
  3. The server writes into the pipe, closes it, and sends the memfd
     back.
  4. The client sees the server's write through its own mapping, reads
     the pipe to EOF, and checks that the returned fd is the same memfd
     (same inode, new fd number).
  5. The server's epoll reports the client hang-up.
* `hello_world unix /tmp/x` is a native client that speaks the same
  protocol against the Linux server. It also checks that `epoll` wakes
  up when a mailbox message arrives.

Fifteen server/client rounds in a row did not increase memory use.

## Packages, symlinks and Linux-format /proc

* **Packages.** `pantry` installs Alpine packages into `/opt/linux`; see
  `docs/PANTRY.md`.
* **Symlinks.** Symlinks from packages are stored as link files that the
  kernel follows for Linux tasks. This is described in `docs/PANTRY.md`.
* **/proc in Linux format.** Linux tasks see Linux-format `/proc/stat` (with
  `btime` and per-CPU times), `/proc/meminfo`, `/proc/uptime` and
  `/proc/loadavg`, while native programs keep the HamixOS formats.
  `/proc/<pid>/task/<pid>` exists because `htop` scans threads there.
* **Terminals.** See "Terminals (hxvt)" below.

## Terminals (hxvt)

`libs/hxvt` is one xterm-compatible terminal emulator shared by the Nook
terminal (`apps/hxterm`) and the kernel text console
(`kernel/src/drivers/video/text_mode.rs`). It replaces the two small
parsers they had before, which knew neither scroll regions nor
insert/delete line. That was the cause of the "artifacts" in htop when
moving through the process list: ncurses scrolls the list with
`CSI r` + `CSI L`/`CSI M`/`ESC M` and draws bars with `REP`/`ECH`.

* **Screen model:** scroll regions (`DECSTBM`), insert/delete lines and
  characters, `ECH`, `REP`, `SU`/`SD`, `IND`/`RI`/`NEL`, tab stops, origin
  mode, auto-wrap with the xterm "pending wrap" rule, insert mode, alternate
  screen (47/1047/1049 with cursor save), `DECSC`/`DECRC`, soft and hard
  reset, `DECALN`.
* **Attributes:** bold, dim, italic, underline, blink, reverse, invisible,
  strike-through; 16, 256 and 24-bit colours in both the `;` and the `:`
  form; reverse video (`?5`).
* **Text:** UTF-8, wide (CJK/emoji) characters take two cells, combining
  marks are dropped, DEC special graphics (`ESC ( 0`, used for ncurses
  line drawing) and the UK set.
* **Replies:** primary and secondary device attributes, `DSR` status and
  cursor position, window size report (`CSI 18 t`), OSC 10/11 colour
  queries. Replies go back to the program as input.
* **Input encoding** (`hxvt::encode_key`, `encode_mouse`, `encode_paste`,
  `encode_focus`): application cursor keys (`?1`), F1-F12, Insert,
  modifiers (`CSI 1;5A` and so on), Alt as an ESC prefix, Shift+Tab,
  bracketed paste (`?2004`), focus events (`?1004`) and mouse reporting
  (`?9`, `?1000`, `?1002`, `?1003`) in the default, UTF-8, SGR (`?1006`)
  and urxvt encodings.
* **Box drawing without fonts:** `hxvt::rasterize` draws U+2500-U+259F (box
  lines, blocks, shades, quadrants), braille (U+2800-U+28FF, btop's
  graphs) and the powerline triangles at any cell size. hxterm and the
  console use it, so the lines join up exactly.

Keyboard: the PS/2 driver now reports F1-F12 and Insert and remembers the
modifier state. `hamix_pollkey` returns the modifiers in bits 32-39 of the
key code (only when a modifier is held, so old callers are unaffected),
`hxproto`'s `Key` event carries a `mods` field, and `hamix_std::sys` has
`key_base`, `key_modifiers` and `poll_key_code`.

hxterm:

* Uses a real pseudo-terminal (`/dev/ptmx`) instead of two pipes, so
  `ttyname()`, `tmux` and `ssh`-style programs that reopen their terminal
  work. It falls back to the pipe pair if `/dev/ptmx` cannot be opened.
* Starts programs with `TERM=xterm-256color` and `COLORTERM=truecolor`.
* Enter sends `\r` like a real terminal. The kernel turns it back into
  `\n` for native programs reading the terminal.
* Mouse clicks, drags and the wheel go to the program when it asks for
  mouse reporting (htop, btop, mc, vim with `mouse=a`). In the alternate
  screen without mouse reporting the wheel sends arrow keys, otherwise it
  scrolls the history (5000 lines, also Shift+PgUp/PgDn).
* Ctrl+Shift+C / Ctrl+Shift+V copy and paste, selecting with the left
  button copies, the middle or right button pastes.
* The window title follows `OSC 0/2`.
* Bold text uses `mono-bold-14`, and both mono fonts now contain Greek,
  Cyrillic, arrows, maths, technical and geometric symbols
  (`tools/nook-assets/generate.py --mono` rebuilds them).

Text console: the same emulator runs per virtual terminal (80x25). Non-ASCII
characters that the 8x8 font does not have are drawn with
`hxvt::rasterize` or mapped to an ASCII look-alike. Programs on the console
also get mouse reporting.

Output processing: the kernel implements `OPOST`/`ONLCR`. Linux programs
that switch it off (ncurses does) get a plain line feed; native programs
always get `\r\n`, as before.

## Signals

`kernel/src/syscall/linux/signal.rs` delivers real signals to Linux tasks.

* `rt_sigaction`, `rt_sigprocmask`, `rt_sigpending`, `rt_sigsuspend`,
  `rt_sigtimedwait`, `sigaltstack`, `pause`, `kill` (process groups and
  `-1` too), `tkill`, `tgkill`, `alarm`, `setitimer`/`getitimer` and the
  POSIX timers `timer_create`/`settime`/`gettime`/`getoverrun`/`delete`.
* The syscall entry now saves a full trap frame (the same layout as the
  timer interrupt) and returns with `iretq`, so a handler can be entered
  from either path and `rt_sigreturn` restores every register, including
  `rcx`, `r11` and the FPU/SSE state.
* The signal frame is Linux's `rt_sigframe` (`ucontext` with the x86-64
  `sigcontext`, `siginfo`, fxsave area). `SA_RESTART`, `SA_NODEFER`,
  `SA_RESETHAND`, `SA_ONSTACK` and blocking are honoured. Signals arrive
  when a syscall returns or on the next timer tick.
* Blocking calls return `EINTR` when a signal is pending: terminal and pipe
  reads, `poll`, `select`, `epoll_wait`, `nanosleep`, `wait4`, futex waits,
  socket operations.
* Default actions: `SIGCHLD`, `SIGWINCH`, `SIGURG`, `SIGCONT` are
  ignored; the stop signals are ignored because there is no job control;
  everything else ends the process with status 128+N, and `wait4` reports
  it as "killed by signal N". Native programs that receive a signal are
  terminated as before.
* **SIGWINCH:** when hxterm (or anything using `TIOCSWINSZ` or
  `hamix_set_terminal`) changes a terminal's size, every Linux process
  with that terminal open gets `SIGWINCH`. This is what makes htop, btop,
  nano and vim follow the window size.
* **SIGINT/SIGQUIT/SIGTSTP:** Ctrl+C, Ctrl+\ and Ctrl+Z are turned into
  signals only when the reading program has `ISIG` set in its termios. In
  raw mode (nano, htop, vim) the key reaches the program, so Ctrl+C in nano
  no longer kills it.
* `SIGPIPE` is a real signal now, so programs that ignore it get `EPIPE`.
* `SIGCHLD` is sent to Linux parents when a child exits.

## Processes and threads

`kernel/src/syscall/linux/process.rs`.

* `clone` with `CLONE_THREAD` creates a thread: a task that shares the
  address space, file descriptors, working directory and signal handlers
  of its thread group leader, with its own kernel stack, TLS (`fs_base`),
  signal mask and pending set. `getpid` returns the group id, `gettid` the
  thread id. `CLONE_SETTLS`, `CLONE_PARENT_SETTID`, `CLONE_CHILD_SETTID`
  and `CLONE_CHILD_CLEARTID` (futex wake on thread exit) are supported.
  Threads of one process are kept on one CPU so that no TLB shootdown is
  needed.
* `futex` has real wait queues: `WAIT`, `WAKE`, the bitset variants,
  `REQUEUE`, `CMP_REQUEUE` and `WAKE_OP`.
* `fork`, `vfork` and `clone` without `CLONE_VM` copy the address space
  (every owned page is copied, shared memory stays shared) and duplicate
  the file table. `vfork` and `CLONE_VFORK` block the parent until the
  child calls `execve` or exits.
* `execve` and `execveat` (absolute paths) replace the process image,
  follow `#!` scripts (up to 4 levels, the interpreter is looked up in the
  sysroot first), close `O_CLOEXEC` descriptors, reset caught signals and
  stop the other threads.
* `wait4` and `waitid` return Linux status words, support `WNOHANG`,
  `WNOWAIT`, process groups and "any child".
* `exit` ends one thread, `exit_group` the whole process. A crash in any
  thread kills the whole process.
* File descriptors keep `FD_CLOEXEC` and `O_NONBLOCK` flags per descriptor
  (`fcntl`, `open`, `pipe2`, `dup3`, `socket`, `accept4`, `epoll_create1`,
  `memfd_create`, `F_DUPFD_CLOEXEC`). Non-blocking pipes and terminals
  return `EAGAIN`. The descriptor table grows to 256 entries.

## Pseudo-terminals

Opening `/dev/ptmx` creates a terminal pair made of two kernel pipes
(`kernel/src/task/pty.rs`). The master reads what programs write and
writes what they read; `/dev/pts/N` is the slave. `TIOCGPTN`,
`TIOCSPTLCK`, `TIOCGPTPEER`, `TIOCSCTTY`, `TIOCSWINSZ` (with `SIGWINCH`),
`/proc/<pid>/fd` links to `/dev/pts/N` and `/dev/tty` work, so musl's
`openpty`/`forkpty`, tmux, foot and Python's `pty` module run. Native
programs (hsh) work on a slave exactly like on the old terminal pipes.

## More syscalls

| # | name | notes |
|---|------|-------|
| 23 / 270 | select / pselect6 | on the same readiness code as `poll` |
| 25 | mremap | shrink in place, grow by moving (`MREMAP_MAYMOVE`) |
| 26, 28, 149-152, 221, 277, 324 | msync, madvise, mlock family, fadvise64, sync_file_range, membarrier | accepted |
| 73 / 285 | flock / fallocate | accepted for valid descriptors |
| 74 / 75 / 162 / 306 | fsync / fdatasync / sync / syncfs | flush the file system. Missing `fsync` was why nano could not save |
| 85 | creat | |
| 96 / 201 / 229 | gettimeofday / time / clock_getres | |
| 97 / 160 / 302 | getrlimit / setrlimit / prlimit64 | `NOFILE` 256, `STACK` 8 MiB, `NPROC` 4096, the rest unlimited |
| 98 / 100 | getrusage / times | CPU time from the scheduler |
| 105-124 | setuid, setgid, setreuid, setresuid, getresuid, getresgid, setfsuid, setfsgid, setpgid, getpgid, getpgrp, setsid, getsid, setgroups | process groups are kept per process |
| 125 / 126 / 135 | capget / capset / personality | |
| 140-148, 203, 309 | scheduling queries, getcpu | |
| 157 | prctl | `PR_SET_NAME`/`PR_GET_NAME` (thread names) and the common flags |
| 188-196 | xattr calls | `ENODATA` / empty list / `ENOTSUP` |
| 222-226 | POSIX timers | see Signals |
| 282 / 289 | signalfd / signalfd4 | read pending signals as `signalfd_siginfo` |
| 283 / 286 / 287 | timerfd_create / settime / gettime | monotonic and realtime, `TFD_TIMER_ABSTIME`, periodic |
| 284 / 290 | eventfd / eventfd2 | counter and semaphore mode |
| 439 / 441 | faccessat2 / epoll_pwait2 | |
| 270 / 271 / 281 / 441 | pselect6 / ppoll / epoll_pwait / epoll_pwait2 | the signal mask argument is applied while waiting (foot relies on it to notice that its shell exited) |
| 86 / 265 | link / linkat | copies the file (the VFS has no hard links); X servers use it for lock files |
| 88 / 266 | symlink / symlinkat | creates an `HXLINK` link file |
| 332 | statx | |
| 436 | close_range | including `CLOSE_RANGE_CLOEXEC` |
| 16 | ioctl | the request number is truncated to 32 bits (musl passes a signed `int`) |

`AF_NETLINK` sockets with `NETLINK_ROUTE` answer `RTM_GETLINK` and
`RTM_GETADDR` dumps with `lo` and the network adapters, so `getifaddrs()`
works (btop crashed without it: it frees the uninitialised list when
`getifaddrs` fails).

`/proc/<pid>/stat` marks kernel threads with `PF_KTHREAD` (htop's "Hide
kernel threads" works) and reports the real thread count. `/etc/fstab`
exists (empty).

## Wayland bridge (M5)

`apps/hxwayland` is a small Wayland compositor that is a normal hxproto
client of hxserver. hxserver starts it together with the desktop (and again
when a graphical Linux app is launched and the bridge is not running). It
listens on `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`, which the default
environment sets to `/tmp/runtime-<uid>/wayland-0`.

* **Globals:** `wl_compositor` 5, `wl_shm` 1 (ARGB8888, XRGB8888),
  `wl_seat` 7 (pointer and keyboard), `wl_output` 4, `xdg_wm_base` 5,
  `wl_subcompositor`, `wl_data_device_manager` and
  `zxdg_decoration_manager_v1` (always server-side decorations: Nook draws
  the frame).
* **Buffers:** `wl_shm` pools are memfds passed with `SCM_RIGHTS` and mapped
  shared, so the bridge reads the client's pixels directly.
* **Windows:** every `xdg_toplevel` gets a Nook window on its first commit
  with a buffer. A commit copies the buffer into the window's shared memory
  and sends `Present`; the buffer is released right away. A buffer of a
  different size re-attaches the window (`Attach`). Title, app id, minimum
  and maximum size and maximize requests are forwarded.
* **Configure:** the initial `configure` goes out on the first commit. When
  the user resizes or maximizes the window the client gets an
  `xdg_toplevel.configure` with the new size and the "activated" state.
  Closing the window sends `xdg_toplevel.close`.
* **Frame callbacks** are answered at most every 16 ms, which paces
  animated clients to about 60 frames per second.
* **Input:** pointer enter/leave/motion/button/axis/frame from hxproto mouse
  events (content-relative coordinates, `BTN_LEFT`/`RIGHT`/`MIDDLE`),
  keyboard enter/leave on focus changes, and key press + release with
  modifiers for every key. The keymap is a compiled US XKB keymap
  (`/usr/share/hamix/xkb/us.xkb`, generated with
  `xkbcli compile-keymap --layout us`) sent through a memfd.
* **Popups** (`xdg_popup`): the positioner (size, anchor rectangle, anchor,
  gravity, offset, flip constraints) is evaluated against the parent's screen
  position, and the popup becomes a borderless Nook popup window
  (`CreatePopup`). A click outside the client's popups closes them
  (`popup_done`); a grabbing popup gets the keyboard. `reposition` is
  supported. GTK menus (xfce4-terminal's File/Edit/..., context menus) and
  dialogs such as Help → About work.
* `xdg_surface.set_window_geometry` is honoured: only the geometry rectangle
  is shown (GTK's client-side shadows are cut off) and pointer coordinates
  are offset accordingly.
* `wl_shm` pools are tracked by an internal key, not by their object id, so a
  buffer keeps its memory when the client destroys its pool and reuses the
  id (Xwayland does this all the time).
* Subsurfaces are not composited, and there is no clipboard or drag and drop
  yet.

## X11 programs (Xwayland)

X11-only programs (dillo, xterm, LibreOffice's X11 backend, FLTK, Tk, Motif
...) run through Xwayland in rootless mode. `sudo pantry install xwayland`
(Pantry adds it by itself for packages that link libX11/libxcb without a
Wayland toolkit).

* hxwayland listens on `/tmp/.X11-unix/X0`; every program gets `DISPLAY=:0`.
  Xwayland is started only when the first X client connects
  (`Xwayland :0 -rootless -shm -noreset -listenfd 3`, a pre-connected Wayland
  socket as `WAYLAND_SOCKET`) and is stopped again 20 seconds after the last
  X window is gone, so it costs no memory while unused. Its log is
  `/tmp/xwayland.log`.
* `apps/hxwayland/src/xwm.rs` is the X window manager: it speaks the X11
  protocol directly, redirects the root's children with Composite (rootless
  Xwayland only creates `wl_surface`s for redirected windows), matches
  windows to surfaces through `WL_SURFACE_ID`, maps windows on
  `MapRequest`, honours `ConfigureRequest`, sets `WM_STATE`,
  `_NET_SUPPORTING_WM_CHECK`, `_NET_ACTIVE_WINDOW`.
* Managed windows become normal Nook windows: the title comes from
  `_NET_WM_NAME` or `WM_NAME` (COMPOUND_TEXT escapes are stripped), size
  limits from `WM_NORMAL_HINTS`, the close button sends `WM_DELETE_WINDOW`
  (or `KillClient`), focus uses `SetInputFocus` + `WM_TAKE_FOCUS`,
  `_NET_WM_STATE` maximize/fullscreen requests maximize the Nook window.
  The X window position follows the Nook window (hxserver reports it with the
  new `Placed` event), so absolute coordinates line up.
* Override-redirect windows (menus, tooltips) become Nook popups at their
  absolute X position and follow `ConfigureNotify`. A click outside all X
  windows goes to the topmost X popup, so X menus close themselves as they
  would under any X window manager.
* Tested: dillo (menus, typing, browsing, maximize, close) and LibreOffice
  Writer 25.2 (menus, typing).

## Memory for Linux programs

* **File mappings are lazy.** `mmap` of a file no longer copies the whole
  range: the page table gets non-present entries marked `LAZY` with an index
  into the address space's list of mapped files, and the page-fault handler
  fills up to 16 pages at a time from the file on first access
  (`linux::lazy_fault`). `fork` copies the lazy entries, `munmap` clears
  them, and kernel code that touches user memory faults them in through
  `user_slice`. Xwayland dropped from 231 MB to 57 MB this way (it links
  mesa, libgallium and libLLVM but touches little of them).
* Large files on an installed system are read in pieces straight from disk
  (`hext::read_range`) instead of being loaded into the file cache whole.
* A Linux task's working directory goes through the `/opt/linux` sysroot
  (`cd /usr/lib/libreoffice/program` works).

Try it: `wlhello` (built by `tools/linux-sysroot/build.sh` from
`tools/linux-sysroot/wayland/wlhello.c`, with its `libwayland-client` and
`libffi` in `/opt/linux/usr/lib/hamix-demo`) opens an animated window,
prints pointer buttons and key codes and exits on Escape or when the window
is closed. `sudo pantry install foot` gives a real Wayland terminal.

## Verified in QEMU

* Text console: htop (arrows, F-keys, setup screen), btop, nano (new lines
  and saving), BusyBox `sh -c` with pipes, subshells and `$( )`.
* Nook terminal: htop with mouse selection and live resizing, btop with
  mouse selection, nano, vim, bash, Python 3.12, tmux (split panes), mc.
* Wayland: `wlhello` (animation, pointer, keyboard, maximize, close) and
  `foot` running hsh and htop.
* `threads-musl`: four threads with TLS, a mutex and `malloc`/`free` in
  parallel.

## Internet sockets

Linux programs get real `AF_INET` sockets (`kernel/src/net/inet.rs`,
syscalls in `kernel/src/syscall/linux/inet.rs`) on top of the kernel smoltcp
stack, so `curl`, `wget`, `git`, `python3` and anything else using TCP or
UDP works, HTTPS included (TLS runs in the program).

* `SOCK_STREAM` (TCP) and `SOCK_DGRAM` (UDP), blocking and non-blocking:
  `connect` returns `EINPROGRESS` and `getsockopt(SO_ERROR)` reports the
  result; `bind`, `listen` with a backlog of up to 16, `accept4`,
  `send*`/`recv*` (`MSG_PEEK`, `MSG_DONTWAIT`, `MSG_WAITALL`,
  `MSG_NOSIGNAL`, `MSG_TRUNC`), `shutdown`, `getsockname`/`getpeername`,
  `poll`/`select`/`epoll`, `FIONREAD`, `SO_RCVTIMEO`/`SO_SNDTIMEO`,
  `SO_KEEPALIVE`, `TCP_NODELAY`. IPv4-mapped IPv6 addresses are accepted.
* The sockets are shared across `fork`/`dup` like any file and close
  gracefully (FIN, then a short linger) when the last descriptor goes away.
* The `netd` thread wakes blocked readers as soon as the stack changes, and
  polls every millisecond while sockets are open (every 10 ms otherwise).
* `/etc/resolv.conf` is rewritten from the DHCP lease; `/etc/hosts` and
  `/etc/machine-id` ship in the image.

Checked in QEMU: `curl -I https://example.com/` (DNS, TCP, TLS 1.3,
HTTP/2) and a 2 MiB HTTPS download at the speed of QEMU's user network.

## Priority-inheritance futexes

`FUTEX_LOCK_PI` (6), `FUTEX_LOCK_PI2` (13), `FUTEX_TRYLOCK_PI` (8) and
`FUTEX_UNLOCK_PI` (7) are implemented in `kernel/src/syscall/linux/base.rs`:
the owner's TID lives in the low 30 bits of the futex word, `FUTEX_WAITERS`
(bit 31) marks a contended lock, a thread that already owns the word gets
`EDEADLK`, unlocking someone else's word gets `EPERM`, and a contended
lock blocks through the same machinery as `FUTEX_WAIT`. Priorities are not
actually inherited -- the scheduler is round-robin -- so this is the
semantics, not the scheduling guarantee.

This matters far beyond one program. musl probes for PI support in
`pthread_mutexattr_setprotocol` by issuing the syscall and handing the raw
errno back to the caller, and **libpulse asserts on anything that is not 0
or `ENOTSUP`**:

```
Assertion 'r == 0 || r == 95' failed at ../src/pulsecore/mutex-posix.c:57,
function pa_mutex_new(). Aborting.
```

So every program that links libpulse -- which on a desktop is most of
them -- aborted at startup while the kernel answered `ENOSYS`. Found by
`syscalltrace`, which showed `202(..., 0x6, ...) = -38` immediately before
the abort message.

## Known limitations

* Only musl is supported, not glibc: glibc also needs a vDSO, ifunc/CPUID
  resolution, and more of the TLS machinery.
* There is no job control: `SIGSTOP`/`SIGTSTP` do not stop anything, and
  terminal process groups are only tracked as far as `tcgetpgrp` needs.
* `fork` copies memory eagerly (no copy-on-write).
* IPv6 sockets and raw/ICMP sockets are not available (`ping` does not
  work); TCP and UDP over IPv4 do, see "Internet sockets" above. Loopback
  (`127.0.0.1`) connections are refused.
* Missing: `SCM_CREDENTIALS`, `sendmmsg`/`recvmmsg`, `inotify`, `io_uring`,
  `clone3`, `pidfd_open`, GPU buffers (`dmabuf`/EGL) for Wayland clients.
  `mincore` answers "everything is resident", and the NUMA calls
  (`mbind`, `set_mempolicy`, `get_mempolicy`) are single-node no-ops.
* Qt 6 programs get as far as their event loop: Telegram Desktop starts,
  loads the Qt Wayland plugin and runs its main loop with a clean log, but
  does not open a window yet.
* **`execve` loads the whole binary into the kernel heap.**
  `elf::load_program` reads the file into a `Vec<u8>` and `elf::load` then
  copies every `PT_LOAD` segment into freshly allocated user pages. For
  Chromium (a 238 MiB binary) that is 238 MiB of kernel heap plus ~250 MiB
  of user pages per process, and it used to end in
  `allocation failed: Layout { size: 249690856 }` -- a kernel panic. The
  loader now refuses up front, with a message, when the file is larger than
  the memory that can be spared, so a huge binary fails cleanly instead of
  stopping the machine. The real fix is to map `PT_LOAD` lazily from the
  file the way `mmap` already does (`AddressSpace::map_lazy` +
  `linux::lazy_fault`), eagerly writing only the partial pages and the bss;
  that is the next piece of work here.
* Chromium 142 starts, brings up Ozone/Wayland and then takes a **#GP at a
  fixed address inside its own image** (`ip=0x8004950558`, offset 0x4550558
  in the binary: `mov (%rax),%eax` where `%rax` came from
  `mov 0x30(%rdi),%rax`). A plain load only takes #GP on a non-canonical
  address, so a pointer in its data is garbage. Reproducible under
  `-cpu host`, so it is not a missing CPU feature -- the suspect is how a
  very large multi-segment PIE (or its TLS) is set up.
* `mmap(MAP_SHARED)` on a file is a private (lazily filled) copy, so writes
  do not reach the file. Page protections are not enforced.
* `openat` with a directory fd only works for absolute paths.
* Alpine-built binaries link against `libc.musl-x86_64.so.1`. musl's
  `ld.so` treats that name as itself, so this works.

## Debugging

`syscalltrace NAME` (root, an hsh builtin) logs every system call of the
processes whose name contains NAME to the serial port, with arguments and
results; `syscalltrace` without a name stops it.

## Look of Linux programs

* `/usr/share/themes/Nook` is a GTK 3 theme: Adwaita (copied into the theme
  as `adwaita.css`/`adwaita-dark.css` with its assets, because importing it
  from `resource://` did not work from a file theme on HamixOS) plus
  `nook.css` with the Nook palette from `colors.css`/`colors-dark.css`.
  `gtk-2.0/gtkrc(-dark)` covers GTK 2.
* hxserver writes `~/.config/gtk-3.0/settings.ini` (theme, dark preference,
  Noto Sans 10, no animations), the same for GTK 4, `~/.gtkrc-2.0` and, if
  missing, `~/.dillo/dillorc`, every time the Nook style changes.
* hxwayland sends `xdg_toplevel` configures with the `activated` state on
  focus changes; without it GTK drew every window as unfocused.
* The XWM publishes `RESOURCE_MANAGER` (Nook colours, `*scheme: gtk+`, Xft
  settings) for X11 toolkits; `FLTK_SCHEME=gtk+` is in the default
  environment, so are `QT_QPA_PLATFORM=wayland;xcb` and `NO_AT_BRIDGE=1`.
* Anonymous memory is lazy too: `brk`, anonymous `mmap` and the 8 MiB user
  stack (only the top 256 KiB holding arguments is allocated at exec) use
  zero-filled `LAZY` entries (`paging::ANON_NODE`), filled 4 pages at a time.
