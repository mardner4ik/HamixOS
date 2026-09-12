# hxserver: a minimal Xorg-equivalent for HamixOS

`apps/hxserver` is HamixOS's display server. Like Xorg on Linux, it is the
one program that owns the screen; everything else that wants to draw talks
to it instead of touching video hardware directly. Unlike Xorg, it is
intentionally tiny today -- this is the first milestone, not a finished
window system.

## It is not a kernel component

`hxserver` is an ordinary ring-3 ELF binary built from `apps/hxserver`
against `sdk/hamix_std`, installed to `/usr/bin/hxserver` in `rootfs/` by
`build.sh`, and loaded the same way any other userspace program is loaded:
`kernel/src/task/elf.rs`'s loader, jumped into with
`kernel/src/task/usermode.rs`. The kernel does not know `hxserver` exists.
It contains no display-server code, no window/compositing logic, and no
special-cased syscalls for it -- everything `hxserver` does goes through
the same syscall table every other program uses (see `docs/MUSL.md`),
plus one HamixOS-specific syscall (`hamix_fbmap`, #9001) that any ring-3
program is free to call, not just this one.

Run it from `hsh` with `startx` (a one-line alias for
`exec /usr/bin/hxserver`) or `exec /usr/bin/hxserver` directly.

## How it gets the screen

1. It calls `sys::fbmap()`, which wraps syscall #9001. The kernel looks up
   the boot-time linear framebuffer info that `kernel/src/memory/mod.rs`
   parsed out of the Multiboot2 tags, widens the page-table entries
   covering that physical range to allow ring-3 access
   (`arch::x86_64::paging::allow_user_access`, the same coarse
   2MB-page-granular tool the ELF loader itself uses), and copies
   `{addr, pitch, width, height, bpp}` into a buffer `hxserver` provided.
2. It wraps that address in a `drivers/intel-graphics-driver::Framebuffer`
   -- the exact same struct the kernel's own boot splash uses internally
   (`kernel/src/drivers/video/intel_graphics.rs`) -- so `hxserver` and the
   kernel's early boot code share one pixel-writing implementation instead
   of two.
3. `Framebuffer` implements `vellum::Canvas`, so drawing goes through
   `libs/vellum`'s hardware-agnostic `fill`/`fill_rect`/`set_pixel` API.
   This is the "interacts with graphics libraries" part: any future
   library that targets `vellum::Canvas` works against `hxserver`'s
   framebuffer without either side knowing about the other's internals.

This is the same "single address space, no per-process page tables yet"
bridge stage every other piece of ring-3 support in HamixOS is built on
(see `docs/USERSPACE_ROADMAP.md`). Once real per-process address spaces
land, `hamix_fbmap` maps the framebuffer into `hxserver`'s own address
space specifically instead of widening a shared one, and nothing above
this point needs to change.

## Talking to it: today's protocol

There is no socket syscall yet, so `hxserver` uses a VFS file as its
"socket": it creates `/run/hxserver.sock` and treats anything written to
it as a stream of newline-terminated text commands, polled in a loop
(there is no scheduler to block on, so this is a `pause`-throttled busy
loop, not an interrupt-driven wait -- another bridge-stage simplification,
not the final design):

```
CLEAR rrggbb
RECT x y w h rrggbb
```

Any program that can `open()` and `write()` a file -- including a musl
binary, an `hsh` builtin, or a future `hamix_std`-based client -- is a
valid `hxserver` client today. Unrecognized or malformed lines are
silently ignored rather than crashing the server.

## What's deliberately not here yet

- **No window manager, no input routing, no compositing.** `hxserver`
  owns the whole framebuffer and draws directly to it; there is exactly
  one "window" (the whole screen) today.
- **No binary protocol, no shared memory, no damage tracking.** The text
  protocol above is a placeholder proportional to how little there is to
  say yet -- it exists so the wiring (client → VFS file → `hxserver` →
  framebuffer) is provable end to end before optimizing it.
- **No multi-client arbitration.** Two clients writing to
  `/run/hxserver.sock` at once will interleave lines; there's no
  connection concept, because there's no socket syscall to give each
  client its own one.

Each of these is a natural next milestone once there's a second real
client that needs it, the same incremental philosophy
`docs/GRAPHICS_ROADMAP.md` and `docs/USERSPACE_ROADMAP.md` already use for
the rest of the graphics and userspace stack.
