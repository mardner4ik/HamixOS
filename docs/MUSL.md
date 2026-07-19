# musl libc support in HamixOS

See `docs/USERSPACE_ROADMAP.md` for the full milestone-by-milestone plan --
this file only covers the syscall ABI side of the story, which is already
built and testable today.

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
| 9 | mmap | anonymous only: `alloc_zeroed` from the kernel heap |
| 12 | brk | bump allocator over a fixed 4 MiB arena |
| 16 | ioctl | stub, returns 0 |
| 39 | getpid | always returns 1 (single "process" for now) |
| 60/231 | exit / exit_group | logs the exit code, does not yet tear down a task |
| 63 | uname | fills a real `struct utsname` with HamixOS's identity |
| 158 | arch_prctl | stub, returns 0 |
| anything else | — | returns `-ENOSYS` (-38), exactly like real Linux |

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

Once the ELF loader lands, a static musl binary needs nothing special
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

Until the loader exists, `hello.hamix` can be dropped into `/bin` inside
`rootfs/` (see `build.sh`) purely as a static artifact — HamixOS will not
execute it yet, but the file will be there, in the live filesystem, ready
for the day the loader lands.
