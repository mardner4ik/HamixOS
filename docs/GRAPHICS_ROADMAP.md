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

## Why split it this way

- A driver crate with its own `Cargo.toml` can be built, versioned, and
  eventually tested on its own, instead of being one file that only
  compiles as part of the kernel binary.
- New chipsets are new modules under an existing driver crate's
  `chipset/`, not new top-level kernel files.
- New graphics libraries (fonts, compositing, a window manager some day)
  become new crates under `libs/`, consumed the same way `vellum` is
  consumed today, without ever touching driver or kernel code.
