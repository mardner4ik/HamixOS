# Getting hsh, sudo, and musl binaries out of the kernel and into rootfs

This is the same question asked three different ways:

1. "Why is `hsh` still `kernel/src/hsh/mod.rs` instead of `/bin/hsh` in the
   rootfs image?"
2. "Why is `sudo` a function call inside `hsh` instead of a real
   `/usr/bin/sudo` ELF binary?"
3. "How do I run a musl-compiled program on HamixOS?"

All three are blocked on the same missing piece of the kernel, described
already in `docs/MUSL.md`: there is no way to leave ring 0 and run
independently loaded code today. This file lays out, in order, exactly what
has to be built before that stops being true, so the work can be picked up
milestone by milestone instead of all at once.

## Why it's one project, not three

`hsh` and `sudo` are not special-cased in the kernel because of what they
do -- `kernel/src/hsh/mod.rs` already calls the exact same `fs::Vfs`
functions a userspace binary would reach through the `read`/`write`/`open`
syscalls in `kernel/src/syscall/mod.rs`. They're kernel code purely because
nothing exists yet to load an ELF file, map it into its own address space,
and jump to ring 3. A statically linked musl `hello_world` binary needs
precisely that same machinery. Build it once, and `hsh`, `sudo`, and any
musl program all move to rootfs together.

## Milestone order

1. **GDT ring-3 segments + TSS** (`kernel/src/arch/x86_64/gdt.rs`).
   Add user code/data descriptors, a `TSS` with `RSP0` pointed at a real
   kernel stack, and load it with `ltr`. Nothing can safely enter ring 3
   without this: a ring-3 `syscall`/interrupt with no `TSS.RSP0` runs on
   whatever the user stack happens to contain, which is a guaranteed crash
   or exploit primitive the moment anything reentrant happens (a timer
   interrupt, a page fault, a nested syscall).

2. **Per-process page tables** (`kernel/src/arch/x86_64/paging.rs`,
   `kernel/src/memory`). Right now there is exactly one address space, the
   kernel's own, built once in `boot.S`. A process needs its own `CR3`, its
   own user-mapped pages, and a kernel-mapped-into-every-process region so
   syscalls and interrupts keep working after the `CR3` switch.

3. **A task struct with real state** (`kernel/src/task/mod.rs` only tracks
   PIT ticks today). Needs at minimum: saved register state, a `CR3`, a
   uid/euid pair (this is where `kernel/src/users.rs`'s uid model plugs in
   directly), open-file table (already half-exists as `FD_TABLE` in
   `kernel/src/syscall/mod.rs`, just needs to move from a single global into
   per-task state), and a run/blocked/exited status.

4. **A minimal ELF64 loader.** Static, non-PIE binaries only, to start:
   parse `e_entry` and `PT_LOAD` segments, map them at their `p_vaddr` in
   the new process's page tables, zero `.bss`, set up the initial stack
   (argv/envp/auxv the way the ABI musl's CRT expects), and point `RIP` at
   `e_entry`. `docs/MUSL.md`'s existing build recipe (`-static -no-pie
   -nostdlib`) is exactly the shape of binary this loader needs to target
   first, precisely to avoid also having to write a dynamic linker and a
   PIE relocator in the same milestone.

5. **`sys_execve` and a real `sys_exit`.** `SYS_EXIT`/`SYS_EXIT_GROUP` in
   `kernel/src/syscall/mod.rs` currently just logs and returns; it needs to
   tear down the calling task's address space and hand control back to
   whoever is waiting on it. `execve` is what turns "load this ELF" into a
   syscall a running process can call on itself or, combined with a `fork`
   (or a simpler `spawn`-style syscall if fork's copy-on-write semantics are
   more than you want yet), on a new one.

6. **Move `hsh` and `sudo` into rootfs.** Once 1-5 exist, `apps/hsh` (today
   an empty `#![no_std]` stub) and a new `apps/sudo` stop being placeholders:
   they become real binaries built against `sdk/hamix_std`, calling
   `read`/`write`/`open` the same way `kernel/src/hsh/mod.rs` calls
   `fs::Vfs` today, and `sudo`'s binary gets an actual setuid bit backed by
   the `owner`/`mode` fields already on `fs::Node` (`docs/PERMISSIONS.md`
   has the detail). `kernel/src/main.rs`'s `rust_main` stops calling
   `hsh::run_login()` directly and instead `execve`s `/sbin/init`, which
   execs `/bin/hsh`.

7. **musl binaries "just work" at that point** for anything that only
   touches the syscalls in `docs/MUSL.md`'s table -- no separate musl
   milestone is needed once 1-5 land, which is the whole reason this is
   written as one roadmap instead of three.

## What to build first if you only have time for one milestone

Milestone 1 (GDT/TSS) and milestone 4 (ELF loader) are the two pieces you
can build and unit-test in isolation without the others being finished --
GDT/TSS work can be verified by manually constructing a tiny ring-3 stub
that immediately `int3`s back, and the ELF loader's parsing logic has zero
dependency on ring 3 at all (it's just reading bytes and populating page
tables you can then read back and assert against). Milestones 2, 3, 5 only
become testable once 1 and 4 both exist, since a process switch needs a
place to switch *to*.
