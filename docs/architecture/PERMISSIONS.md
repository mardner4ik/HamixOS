# Users, permissions, and sudo in HamixOS

## Where accounts live

Three files in the VFS, all real files you can `cat`, `chmod`, or edit like on
any Unix system:

- `/etc/passwd` -- `name:x:uid:gid:gecos:home:shell`, one line per account.
- `/etc/shadow` -- `name:salt_hex:hash_hex`, mode `600`, root-owned. The
  password itself is never stored; a keyed hash is.
- `/etc/sudoers` -- one username per line. Being listed here (or being
  `root`, who is always implicit) is what lets `sudo` succeed.

`kernel/src/users.rs` parses these three files into an in-memory table at
boot, right after the initramfs has loaded (so a custom rootfs can ship its
own `/etc/passwd`/`/etc/shadow`/`/etc/sudoers` and they take effect
automatically), and again whenever `passwd` or `useradd` change them.

## Password hashing

`hash_with_salt` in `kernel/src/users.rs` is a salted 32-bit
avalanche/mixing hash (FNV-1a seed, three SplitMix64-style mixing rounds). It
keeps a plaintext password out of `/etc/shadow`, but it is **not** a
cryptographic KDF like `sha512crypt`, `scrypt`, or `bcrypt` -- there's no
`core`-compatible constant-time implementation of any of those vendored yet,
and pulling one in is a reasonable follow-up once the crate story for
`no_std` is settled. Treat the current scheme as "no plaintext at rest",
not as resistant to a determined offline attacker.

## uid / euid, exactly like POSIX

Because HamixOS still runs the entire shell as kernel code with no separate
processes yet (see `docs/USERSPACE_ROADMAP.md`), there's no fork/exec to
hand off privilege to a child process the way a real `sudo` binary does.
Instead, `hsh` keeps the same two numbers the kernel would eventually track
per-process:

- `ruid` -- the real uid, fixed for the login session, used for the prompt,
  `$HOME`, and `whoami` outside of `sudo`.
- `euid` -- the effective uid used for every permission check. It is equal
  to `ruid` for a normal command, and temporarily `0` for the single command
  following a successful `sudo`.

`kernel/src/hsh/mod.rs` funnels every builtin through one `dispatch()`
function that takes both; `sudo <cmd>` re-invokes `dispatch()` with
`euid = 0` after checking `/etc/sudoers` and asking for the *invoking* user's
own password (matching real `sudo`, which never asks for root's password).

## File permission bits

`fs::Node` now carries an `owner: u32` and a `mode: u16`. `Vfs::can_write`
implements the same three-way check as Linux `access()`, simplified to a
single owner class instead of owner/group/other, since HamixOS has no group
membership concept yet:

- uid 0 can always write.
- the owning uid can write if the owner-write bit (`0o200`) is set.
- anyone can write if the world-write bit (`0o002`) is set.

Reads are not permission-checked yet -- everything is world-readable. That
matches the file's actual security value in a single-user hobby OS today;
extending `can_write`'s logic to reads is a small, mechanical follow-up once
there's an actual multi-user threat model worth enforcing it for (e.g. once
untrusted userspace binaries exist -- see the roadmap doc).

## What `sudo` and `useradd`/`passwd` do today

- `sudo <command>` -- checks `/etc/sudoers`, prompts for the caller's own
  password against `/etc/shadow`, and on success runs exactly one command
  line with `euid = 0`.
- `passwd [user]` -- changes your own password (asks for the current one
  first) or, run through `sudo`, anyone's.
- `useradd <name>` -- root-only, appends to `/etc/passwd`, creates
  `/home/<name>`, and sets an initial password.
- `chmod <octal-mode> <path>` / `chown <user> <path>` -- standard semantics,
  `chown` is root-only exactly like Linux.

## What real, separate-binary `sudo` needs

The user-visible behavior above is intentionally identical to real `sudo`.
What's different under the hood is that `/usr/bin/sudo` on Linux is a
setuid-root ELF binary that `execve()`s the target command in a *new
process* running as uid 0; HamixOS has no process concept to exec into yet,
so today's `sudo` is a trusted function inside the one shell "process" that
temporarily raises its own effective uid. The moment the ELF
loader/scheduler milestones in `docs/USERSPACE_ROADMAP.md` land, this same
permission table (`kernel/src/users.rs`) becomes the backing store for a
real `setuid` bit on `/bin/sudo`'s inode and a real `sys_setuid`/`sys_execve`
pair -- no data model changes required, only the process plumbing underneath
it.
