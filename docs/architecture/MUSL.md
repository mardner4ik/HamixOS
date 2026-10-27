# musl libc support in HamixOS

> Superseded for running real Linux/musl binaries by `docs/LINUXULATOR.md`
> (hxlinuxulator); the syscall table below predates it.

See `docs/USERSPACE_ROADMAP.md` for the full milestone-by-milestone plan --
this file only covers the syscall ABI side of the story, which is already
built and testable today.

If the goal is a pure-Rust userspace binary rather than a musl/C one, see
`sdk/hamix_std` instead: it is a small `#![no_std]` runtime (raw `syscall`
wrappers, a `brk`-backed global allocator, `println!`/`eprintln!`, an
`entry!` macro for `_start`) built directly against the same syscall table
documented below, with no musl toolchain required. `apps/hello_world` and
`apps/hxserver` are both built against it and are a better starting point
for a new Rust program than raw musl-gcc.

## Where the syscall table lives

`kernel/src/syscall/mod.rs` implements `handle_syscall`, matching the real
Linux x86_64 syscall ABI numbering (the same numbers musl's x86_64 syscall
stubs use):

| # | name | status |
|---|------|--------|
| 0 | read | fd 0 → keyboard, fd ≥ 3 → VFS-backed file |
| 1 | write | fd 1/2 → console, fd ≥ 3 → VFS-backed file |
| 2 | open | creates/looks up a path in the in-RAM VFS, returns an fd |
| 3 | close | frees the fd slot |
| 5 | fstat | fills a real 144-byte `struct stat`; only `st_size`/`st_mode`/`st_nlink`/`st_blksize`/`st_blocks` are meaningful, the rest is zeroed |
| 8 | lseek | SEEK_SET/CUR/END against the VFS file's real length |
| 9 | mmap | anonymous only: `alloc_zeroed` from the kernel heap |
| 11 | munmap | stub, always returns 0 (nothing is ever unmapped yet) |
| 12 | brk | bump allocator over a fixed 4 MiB arena |
| 13/14 | rt_sigaction / rt_sigprocmask | stub, returns 0 (no signal delivery yet) |
| 16 | ioctl | stub, returns 0 |
| 20 | writev | loops the existing `write` path over each iovec |
| 39 | getpid | always returns 1 (single "process" for now) |
| 60/231 | exit / exit_group | logs the exit code, does not yet tear down a task |
| 63 | uname | fills a real `struct utsname` with HamixOS's identity |
| 102/107 | getuid / geteuid | always returns 0 (single-user ring-3 bridge stage) |
| 158 | arch_prctl | stub, returns 0 |
| 186 | gettid | always returns 1 |
| 218 | set_tid_address | stub, returns 1 |
| 228 | clock_gettime | derived from the PIT tick counter (`kernel/src/task`), ignores `clockid_t` |
| 9001 | **hamix_fbmap** (HamixOS-specific, not a real Linux number) | fills `{addr,pitch,width,height,bpp}` for the boot framebuffer and grants the calling ring-3 program access to that memory range. This is how userspace programs (e.g. `apps/hxserver`, see `docs/XORG.md`) get at the screen without a kernel-side display component. |
| anything else | — | returns `-ENOSYS` (-38), exactly like real Linux |

None of the syscalls above need more than 3 arguments (`rdi`,`rsi`,`rdx`), which is all `syscall_entry`'s trampoline currently forwards to `handle_syscall` — `r10`/`r8`/`r9` (musl's arg4-arg6 registers) reach the trampoline but are not yet threaded through. A real 6-argument `mmap(addr,len,prot,flags,fd,offset)` needs `fd`/`offset` (arg5/arg6) and is the next syscall-ABI milestone once a program actually needs file-backed mmap instead of the `hamix_fbmap` shortcut above.

`init()` in that file programs `IA32_STAR/LSTAR/FMASK` so the `syscall`
instruction (the one musl always uses on x86_64, it never falls back to
`int 0x80`) lands in `syscall_entry`, a naked trampoline that reshuffles
`rax,rdi,rsi,rdx,r10` into the SysV order `handle_syscall` expects and
returns with `sysretq`.

## What is real vs. what is still missing

This is enough to dispatch and execute real syscalls **once you are
already running in ring 3**. What HamixOS does **not** have yet is the
part that gets you into ring 3 in the first place:

- no ELF loader (parsing `apps/*` binaries and mapping their segments),
- no per-process page tables / user-vs-kernel address space split,
- no TSS.RSP0 + `swapgs` kernel-stack switch (needed so a ring-3 `syscall`
  lands on a safe kernel stack instead of continuing on the user stack),
- no real scheduler/task struct (`kernel/src/task` only tracks PIT ticks).

So today the syscall table is fully wired and testable from kernel code
(and from `hsh` commands, which call the same VFS functions the syscalls
use), but nothing can yet `syscall` into it from a loaded ring-3 binary.
That's the natural next milestone — ELF loading + ring-3 switch — and it's
a separate, sizeable piece of work from the syscall table itself.

## Building a musl userland program against this ABI

A static musl binary needs nothing special
beyond disabling dynamic linking and floating-point-heavy CRT startup
paths that assume a working `mmap`/`futex` for TLS setup:

```bash
# 1. Build (or fetch) a musl-gcc cross toolchain for x86_64:
git clone https://git.musl-libc.org/musl
cd musl
./configure --prefix=/opt/musl-hamix --disable-shared
make -j$(nproc) && sudo make install

# 2. Compile statically, no dynamic linker, no PIE:
/opt/musl-hamix/bin/musl-gcc -static -no-pie -fno-stack-protector \
    -nostdlib -e _start -O2 hello.c -o hello.hamix

# 3. Only the syscalls listed in the table above are implemented; a
#    program that calls something else (fork, mmap with a fixed hint,
#    futex, etc.) will get -ENOSYS back, not a crash.
```

The loader now exists: `kernel/src/task/elf.rs` loads static ELF binaries
and `PT_INTERP` dynamic ones, so a musl binary built this way runs directly.
The packages under `/opt/linux` (see `docs/LINUXULATOR.md`) are the real
users of this path; `pantry` installs them.
