# Graphics subsystem roadmap

HamixOS splits video support into three layers, each its own crate/module
with a single responsibility. This is the plan the current
`intel-graphics-driver` + `vellum` split follows, and how it is meant to
grow.

## Layer 1: `vellum` (`libs/vellum`)

Hardware-agnostic 2D primitives: `Color`, `Point`, `Rect`, and the `Canvas`
trait. No `alloc`, no hardware access, no dependency on the kernel. Anything
that can expose a `set_pixel(x, y, color)` can implement `Canvas` and get
fill/gradient helpers for free.

Grows one stage at a time — see `libs/vellum/README.md` for the exact
staging (lines/circles next, then fonts, then blitting).

## Layer 2: hardware drivers (`drivers/<name>-driver`)

One crate per hardware family, e.g. `drivers/intel-graphics-driver`. A
driver crate:

- owns raw pixel writes for its hardware,
- implements `vellum::Canvas` on its framebuffer type so it inherits
  drawing operations instead of reimplementing them,
- exposes chipset metadata (`ChipsetInfo`, PCI ids) even before there is a
  PCI bus driver to use it, so that piece slots in later without an API
  change.

Each driver crate has its own `README.md` describing exactly which hardware
it supports today and what's next for it.

## Layer 3: kernel adapters + registry (`kernel/src/drivers/video/`)

The kernel never allocates a framebuffer address or writes a pixel itself.
A thin adapter module per driver (e.g. `intel_graphics.rs`) is the only
kernel code that touches the driver crate: it reads boot-time hardware
facts (the Multiboot2 framebuffer tag today) and forwards drawing calls.

`registry.rs` defines a `VideoDriver` trait every adapter implements
(`name`, `version`, `is_ready`, `resolution`, `kind`) and a static list of
all of them, so `hsh`'s `drivers` command — and anything else that needs to
enumerate video drivers later — doesn't need to know about each driver by
name.

## Layer 4: accelerated display modules (`kernel/src/drivers/video/gpu.rs`)

Layers 1-3 assume one thing that stopped being true: that the framebuffer is
a fixed linear region the bootloader hands over, which the CPU writes and the
hardware scans out by itself. A real GPU does not work that way -- it owns
its scanout buffer, it wants to be told which rectangle changed, and it has
planes (a cursor, at minimum) that composite without the CPU touching a
pixel.

Layer 4 is the seam for that. A loadable module (`docs/MODULES.md`) can
register a `DisplayOps` capability table and a framebuffer it owns, and from
then on that buffer *is* the kernel framebuffer -- the text console, `fbmap`
and the mouse bounds all follow it. The capabilities are `flush` (push a
damage rectangle), `set_mode`, `cursor_set`/`cursor_move`/`cursor_hide`, and
`fill`/`copy` for 2D offload.

Nothing above changes when no module is loaded: `gpu::active()` is false,
`Screen::present` skips the flush, `hxserver` draws the pointer in software,
and `modes.rs` keeps its Bochs/Intel backends. That is deliberate -- the
architecture roadmap is explicit that Nook must not depend on a heavy GPU
path being present.

`drivers/virtio-gpu` is the first driver in this layer, and it is what the
architecture roadmap calls the first queue of the GPU work: mode setting and
scanout, no 3D. It gives HamixOS, on any machine with a virtio GPU:

* a scanout resource the device owns, with only the damaged rectangle
  transferred per frame instead of a full-screen linear framebuffer,
* real mode setting through Settings, at the panel's preferred size,
* a hardware cursor, which is the one place where moving work to the GPU is
  directly measurable: pointer motion stops costing the compositor anything.

`drivers/intel-display` is the second driver in this layer, covering Intel
integrated graphics from gen6 (Sandy Bridge, HD 2000) to gen9 (Skylake
HD 520, Kaby Lake, Coffee Lake). Unlike virtio-gpu it does not own a scanout
buffer -- the display engine already scans out the framebuffer the firmware
set up, so there is no flush at all. What it adds is the panel size and
timings read from the active transcoder, the real refresh rate measured from
the pipe frame counter (which avoids decoding a different PLL layout on every
generation), EDID over GMBUS for the rates the panel actually allows, refresh
switching, and the hardware cursor plane. It is written from the documented
register layout and is **not verified on hardware**; see `docs/MODULES.md`.

Identification is a separate, driver-free concern: `models.rs` names Intel
cards from their PCI id, and `kernel/src/drivers/video/sysfs.rs` publishes a
Linux-shaped `/sys/bus/pci/devices` and `/sys/class/drm` so `fastfetch` and
friends can see the card whether or not any driver claims it.

What layer 4 does *not* do is 3D. `fill`/`copy` exist in the interface but
have no backend, because virtio-gpu 2D has no drawing commands. The next
backend for them is the Intel gen4 BLT ring, on the same hardware
`drivers/intel-graphics-driver` already identifies.

## Why split it this way

- A driver crate with its own `Cargo.toml` can be built, versioned, and
  eventually tested on its own, instead of being one file that only
  compiles as part of the kernel binary.
- New chipsets are new modules under an existing driver crate's
  `chipset/`, not new top-level kernel files.
- New graphics libraries (fonts, compositing, a window manager some day)
  become new crates under `libs/`, consumed the same way `vellum` is
  consumed today, without ever touching driver or kernel code.
