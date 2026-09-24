# Loadable driver modules and the HamixOS KPI

This is stage 1 of point 5 in `docs/ARCHITECTURE_ROADMAP.md`: the piece the
roadmap calls a prerequisite ("модуль-завантажувач у ядрі -- передумова").
It makes driver code *optional and separately loadable* instead of statically
linked into every kernel image, so a machine only pays for a driver it
actually needs. It now also carries the GPU work: `drivers/virtio-gpu` is a
real accelerated display driver loaded this way, which is stage 3 of the
roadmap's GPU plan (mode setting and scanout, not 3D). No *Linux* driver has
been ported yet -- that is still stage 2 (NIC).

## What exists

* `kernel/src/module/elf.rs` -- an ET_REL (`.o`) loader: section placement,
  `SHN_COMMON` allocation, a per-module GOT, and the x86_64 relocations
  `R_X86_64_64`, `PC32`, `PLT32`, `32`, `32S`, `16`, `8`, `PC16`, `PC8`,
  `PC64`, `GOT32`, `GOTPCREL`, `GOTPCRELX`, `REX_GOTPCRELX`.
* `kernel/src/module/image.rs` -- the module image. Frames are allocated
  contiguously **below 4 GiB** (`frame::alloc_contiguous(pages, 1 << 32)`)
  because `R_X86_64_32`/`32S` and `PC32` require the module to sit within
  reach of the kernel, which is identity-mapped at 1 MiB. NX is not enabled
  (`boot.S` sets only EFER.LME), so these pages are executable.
* `kernel/src/module/kpi.rs` -- the shim: the C-ABI surface modules call.
* `kernel/src/module/symbols.rs` -- the kernel's export table. A module that
  references anything not in this table is refused at load time, by name.
* `sdk/hamix_kpi` -- the crate a module is written against.
* `drivers/example-nic` -- a reference module that claims QEMU's
  `8086:100e` e1000 through the KPI. It exists to keep the loader honest,
  not to replace `kernel/src/net/e1000.rs`.
* `drivers/virtio-gpu` -- a real display driver for the virtio-gpu 2D device
  (`1af4:1050`, QEMU's `virtio-vga` and `virtio-gpu-pci`): virtio 1.0 PCI
  capability discovery, split virtqueues, scanout resources, damage-driven
  transfer and flush, mode setting, and a hardware cursor plane. See
  "The display class" below.
* `drivers/intel-display` -- Intel integrated graphics, gen6 (Sandy Bridge,
  HD 2000/3000) through gen9 (Skylake HD 520, Kaby Lake, Coffee Lake): panel
  size and timings read from the active transcoder, refresh rate measured
  from the pipe frame counter, EDID over GMBUS, refresh switching, resolution
  changing through the panel fitter (gen6-gen8) or the pipe scaler (gen9),
  and a hardware cursor plane. **Not verified on real hardware** -- see the
  warning at the end of this file.
* `drivers/ahci` -- the SATA controller as a module (`CLASS_BLOCK`). With
  `early = 1` in its `.conf` it is packed into `/boot/kmods.tar`, which GRUB
  loads as `module2 ... kmods`, so the kernel can mount the root disk from it.
* `drivers/xhci` -- the xHCI USB 3 host controller (`CLASS_USB_HCD`): BIOS
  handoff, command and event rings, device enumeration, HID boot keyboards
  and mice, hot-plug. Builds and runs on x86_64, aarch64 and riscv64.
* `drivers/e1000e` -- Intel's Linux e1000e driver (v6.6) compiled against
  `sdk/hamix_linuxkpi`. It differs from upstream only by removed parts;
  `tools/linuxkpi/check-upstream.sh drivers/e1000e` proves it.
* `sdk/hamix_linuxkpi` -- the Linux-name layer: `include/linux/*.h` over one
  `hamix/linuxkpi.h`, plus `src/linuxkpi.c` (printk, timers, work queues,
  `pci_driver`, IRQ, DMA, `sk_buff`, NAPI, `net_device`).
* `docs/KPI_ROADMAP.md` -- the six directions this KPI grew in; all are done.
* `kernel/src/drivers/video/sysfs.rs` -- the synthetic `/sys/bus/pci/devices`
  and `/sys/class/drm` trees that let Linux programs (`fastfetch`, and
  anything else that walks sysfs) see the real graphics card.
* `kernel/src/drivers/video/gpu.rs` -- the kernel side of the display class:
  the provider registry, damage accumulation, and the dispatch that userspace
  reaches through `SYS_GPU_*`.

## The memory budget

The roadmap sets 20 MiB for the shim plus a resting driver.
`module::BUDGET_BYTES` enforces it: a load that would cross it is refused.
`resident_bytes()` counts module images plus everything a module allocated
through `hamix_kmalloc`/`hamix_kzalloc`/`hamix_dma_alloc`, so the number
is the real resting cost, not just code size.

Measured on QEMU with the reference module loaded:

```
$ modules
MODULE                 RESIDENT       TEXT       DATA  DEVICES
example-nic                4096        279        231        1
  example-nic drives eth-kpi0 (8086:100e)
budget: 4 KiB of 20 MiB used
```

That 4 KiB is the shim's runtime cost with one module: the shim itself is
code inside the kernel image, so it costs nothing extra at rest. This is the
baseline the roadmap asks to measure "before any driver".

`virtio-gpu` costs about 4 MiB at 1280x800, almost all of it the scanout
buffer itself (`width * height * 4`), which is why the budget is checked on
every allocation and not only at load time: `hamix_dma_alloc` refuses an
allocation that would cross the 20 MiB line, so a resolution change to a mode
the budget cannot hold fails cleanly instead of eating the kernel heap.

## Writing a module

```rust
#![no_std]
use hamix_kpi as kpi;

fn init() -> i32 {
    let Some(dev) = kpi::find_device(0x8086, 0x100e) else { return -1 };
    let (base, len) = kpi::bar(&dev, 0);
    unsafe { kpi::hamix_pci_enable(&dev) };
    if !kpi::claim(kpi::CLASS_NETWORK, "eth-kpi0", &dev, Some(poll), core::ptr::null_mut()) {
        return -1;
    }
    0
}

fn exit() {}

hamix_kpi::module!(init = init, exit = exit);
```

`module!` emits `hamix_module_init` / `hamix_module_exit` (the two names the
loader looks for), checks `hamix_kpi_version()` against the header the module
was built against, and supplies a panic handler.

## Building one

A module is a **single relocatable object**, not an executable. It is built
as a `staticlib` with the `module` profile (fat LTO, one codegen unit), and
`build.sh` takes the crate's own member out of the archive. LTO pulls every
piece of `core`/`alloc` the module uses into that one object, so the only
undefined symbols left are the KPI exports plus `memcpy`/`memset`.

```bash
cargo +nightly rustc --profile module \
    -Z build-std=core,compiler_builtins,alloc \
    -Z build-std-features=compiler-builtins-mem \
    --target kernel/x86_64-hamix_os.json \
    --crate-type staticlib \
    -- -C relocation-model=pie
```

`build.sh` does this for every name in `MODULES` and installs the result as
`/lib/modules/<name>.ko` with its `module.ids` next to it. Two checks stop
the build instead of failing on a user's machine:

* `nm -u` must list only names exported by `kernel/src/module/symbols.rs`
  (this is what broke `intel-display` on an HD 520: `core::str::from_utf8`
  and `hamix_display_current` were not exported);
* on x86_64 there must be no `R_X86_64_32`/`R_X86_64_32S` relocation. With
  `relocation-model=static` LTO emits them, and they only fit while the module
  sits below 2 GiB -- on a 3 GiB laptop the image landed near 3 GiB and every
  Rust module failed with "relocation does not fit in 32 bits". Modules are
  now PIE and the loader also keeps x86_64 images below 2 GiB, because calls
  into the kernel (at 1 MiB) are `PLT32`.

A module that needs a heap calls `hamix_kpi::kernel_heap!()`: `Vec`, `String`
and `format!` then allocate through `hamix_kmalloc` and count against the
module's `limit`. `hamix_kpi::io` has `Mmio` (bounds-checked `ioremap`) and
`DmaRegion` (below 4 GiB).

## Loading

`/lib/modules/<name>.ids` lists the PCI ids a module claims, one
`vendor:device` per line in hex. At boot the `modules` hxinit unit walks
`/lib/modules`, loads only the modules whose ids match a device actually on
the PCI bus, and reports what it loaded:

```
hxinit: ok modules (9) 1 loaded, 4 KiB of the 20 MiB budget
```

`module::load_from(path)` loads one by hand. `/proc/modules` and the `modules`
shell command show what is loaded, what each module claimed, and how much of
the budget is spent.

## The display class

A module can also drive the screen. `hamix_register_display(name, ops, fb)`
hands the kernel a framebuffer the module owns plus a `DisplayOps` table, and
from that point the kernel's framebuffer *is* the module's scanout buffer:
the text console, `display::map` (what `hxserver` gets from `fbmap`) and the
mouse bounds all follow it. `drivers/virtio-gpu` is the reference
implementation; `docs/GRAPHICS_ROADMAP.md` describes the layer it sits in.

`DisplayOps` is a capability table -- a module fills in only what its
hardware can do, and sets the matching `CAP_*` bit:

| capability | op | what the kernel does with it |
| --- | --- | --- |
| `CAP_FLUSH` | `flush(x, y, w, h)` | push a damaged rectangle to the screen |
| `CAP_MODESET` | `set_mode(w, h)` / `mode_list` | becomes the `modes.rs` backend, so Settings can change resolution; `mode_list` fills `(width, height)` pairs, native first, and the kernel offers exactly those |
| `CAP_SCALE` | -- | the smaller modes are produced by scaling, not by retiming the panel |
| `CAP_CURSOR` | `cursor_set` / `cursor_move` / `cursor_hide` | a hardware cursor plane |
| `CAP_REFRESH` | `set_refresh(hz)` / `refresh_list` | refresh rates Settings offers for the current mode |
| `CAP_FILL` | `fill(x, y, w, h, color)` | offload a solid rectangle |
| `CAP_COPY` | `copy(sx, sy, dx, dy, w, h)` | offload a screen-to-screen blit |

Two calls exist for a module that changes its own framebuffer:
`hamix_display_changed(fb)` re-points the kernel at a new scanout buffer
(this is how a modeset works), and `hamix_budget_left()` reports how much of
the 20 MiB budget is still free before allocating a bigger one.

**Flushing is explicit.** A module-provided framebuffer is ordinary RAM, so
nothing reaches the screen until someone calls `flush`. Two paths do:
`hxserver` calls `SYS_GPU_FLUSH` with its exact damage after each compose,
and for the text console `write_row` accumulates a dirty rectangle
(`gpu::mark_dirty`, four atomics, no lock) that a `gpud` kernel thread
flushes at 30 Hz whenever no process owns the display.

**The hardware cursor is the part that genuinely moves drawing off the CPU.**
When `CAP_CURSOR` is present, `hxserver` uploads the cursor image once per
shape change and then only sends positions; it stops compositing the pointer
and stops damaging the old and new cursor rectangles on every motion event.
Measured on QEMU with `virtio-vga`, 36 seconds idle versus 36 seconds of
continuous mouse motion cost **60 and 61 scanout flushes** -- moving the
pointer is free. Without it, each motion event costs a recomposition plus a
transfer of two 32x32 rectangles.

## Identifying the card

Naming a GPU needs no driver at all, only a PCI id database, so that part is
kept separate from everything above:

* `drivers/intel-graphics-driver/src/chipset/models.rs` maps 84 Intel device
  ids (gen6 through gen9) to marketing names and generations. `gpuinfo` uses
  it, so an Intel card is named correctly with nothing installed.
* `kernel/src/drivers/video/sysfs.rs` writes a Linux-shaped
  `/sys/bus/pci/devices/<slot>/{vendor,device,class,revision,subsystem_*,irq,modalias,uevent}`
  and `/sys/class/drm/card<N>/` at boot, from the PCI scan the kernel already
  did. This is what `fastfetch` walks.
* For anything the kernel table does not know, `gpuinfo` falls back to
  `/usr/share/hwdata/pci.ids` from userspace (Alpine `hwdata-pci` via
  pantry). The lookup is deliberately **not** in the kernel: a `/proc`
  generator runs with the VFS lock held, so reading a file from one
  deadlocks.

Verified on QEMU with `virtio-vga`: `gpuinfo` reports `Virtio 1.0 GPU` and
`fastfetch -s GPU` reports `RedHat Virtio 1.0 GPU`, both through this path.

## The KPI surface

Everything in `symbols.rs`, grouped:

| group | functions |
| --- | --- |
| version | `hamix_kpi_version` |
| memory | `hamix_kmalloc`, `hamix_kzalloc`, `hamix_kfree` |
| DMA | `hamix_dma_alloc`, `hamix_dma_free`, `hamix_virt_to_phys`, `hamix_phys_to_virt` |
| MMIO | `hamix_ioremap`, `hamix_iounmap`, `hamix_read{b,w,l,q}`, `hamix_write{b,w,l,q}` |
| port I/O | `hamix_in{b,w,l}`, `hamix_out{b,w,l}` |
| PCI | `hamix_pci_find`, `hamix_pci_read{16,32}`, `hamix_pci_write{16,32}`, `hamix_pci_bar`, `hamix_pci_enable` |
| time | `hamix_udelay`, `hamix_mdelay`, `hamix_uptime_ms` |
| log | `hamix_printk` |
| display | `hamix_display_abi`, `hamix_register_display`, `hamix_display_changed`, `hamix_budget_left` |

`hamix_kpi_version()` is **3** and `hamix_display_abi()` is **2**: display
ABI 2 added `mode_list` to `DisplayOps` and `CAP_SCALE`; KPI 3 added
interrupts, work queues, module parameters and levelled logging. A module
built against ABI 1 is refused at `hamix_register_display`, and one built
against an older KPI is refused by `module!` before `init` runs.

| group | functions added in KPI 3 |
| --- | --- |
| interrupts | `hamix_request_irq`, `hamix_free_irq`, `hamix_pci_msi_enable`, `hamix_in_interrupt` |
| deferred work | `hamix_schedule_work` |
| configuration | `hamix_param_u32` |
| logging | `hamix_dev_log` (`dev_err`/`dev_warn`/`dev_info`/`dev_dbg`) |
| registration | `hamix_claim_device` |
| compiler | `memcpy`, `memset`, `memmove`, `memcmp` |

KPI 4-6 added the device classes and the Linux layer:

| group | functions |
| --- | --- |
| discovery | `hamix_pci_find_class` (0xFF is a wildcard in every field), `hamix_device_from_pci`, `hamix_platform_find`, `hamix_bus_publish` |
| display ABI 3 | `hamix_display_hotplug`, `hamix_edid_modes`; `ConnectorDesc`, `page_flip`, `wait_vblank` in `DisplayOps` |
| block | `hamix_register_block`, `hamix_dma_run` |
| input | `hamix_input_key`, `hamix_input_pointer`, `hamix_input_absolute` |
| audio | `hamix_register_audio` |
| USB host | `hamix_usb_register_hcd`, `hamix_usb_add_device`, `hamix_usb_remove_device`, `hamix_usb_hid_report` |
| network | `hamix_register_netdev`, `hamix_unregister_netdev`, `hamix_net_receive`, `hamix_net_carrier` |

`hamix_kpi_version()` is now **6** and `hamix_display_abi()` **3**.

## C modules and linuxkpi

A driver directory with `src/*.c` and no `Cargo.toml` is a C module.
`build.sh` compiles every file with clang (freestanding, `-fpie` on x86_64,
general registers only), adds `sdk/hamix_linuxkpi/src/linuxkpi.c`, links the
lot with `ld.lld -r` into one relocatable object and installs it as
`<name>.ko`. The `.ids` file is generated from the object's
`MODULE_DEVICE_TABLE` symbol by `tools/linuxkpi/device-ids.py`.

`module_init(fn)` becomes `hamix_module_init`; `pci_register_driver` walks
every PCI function, matches the driver's `id_table`, builds a `pci_dev` and
calls `probe`, then claims the device so the built-in driver leaves it alone.
`register_netdev` registers the interface with the kernel and opens it right
away (there is no `ip link set up`). Frames the driver hands to
`napi_gro_receive` go to `hamix_net_receive`; the kernel's transmit path
wraps each frame in an `sk_buff` and calls `ndo_start_xmit`. NAPI polls and
`schedule_work` run on the kernel's work queue; `timer_list` timers fire from
the module's poll callback. Interrupt handlers run through
`hamix_request_irq`, which picks MSI when the device offers it.

## What is deliberately missing

**A full MSI-X table.** `hamix_request_irq` programs plain MSI
(capability 0x05) when the device has it, and otherwise falls back to the
legacy PIC line. A device that offers **only** MSI-X (capability 0x11, which
needs a vector table inside a BAR) stays on INTx; `hamix_pci_msi_enable`
reports which of the two the device has. There is also no IOAPIC: legacy
lines go through the 8259 pair, which is enough for one interrupting device
per line but not for heavy sharing.

**Interrupts themselves are done** -- see `docs/KPI_ROADMAP.md` point 2.
`kernel/src/drivers/irq.rs` holds the per-vector handler table, the
`hamix_schedule_work` bottom half and the callback watchdog; `interrupts`
and `/proc/interrupts` show the lines. The poll callback registered with
`hamix_claim_device` still exists and still works -- a driver can use either.

**2D fill and blit offload.** `CAP_FILL` and `CAP_COPY` are defined and
routed all the way to userspace (`SYS_GPU_FILL`, `SYS_GPU_COPY`), but no
backend implements them: virtio-gpu's 2D protocol has resource transfer and
flush commands and no drawing commands at all, so there is nothing to call.
Intel does have a blitter, but reaching it means setting up the GGTT and a
ring buffer on gen6-gen8, and **execlists** on gen9 -- a context descriptor
machinery larger than everything in this module put together, and none of it
testable without the hardware. Writing that blind is not defensible, so the
cursor plane is as far as Intel 2D offload goes here. Until a backend exists
`gpu::fill` and `gpu::copy` return `-95` and callers fall back to the CPU.

**Intel gen6-gen9 is not verified on hardware.** `drivers/intel-display` was
written against the documented register layout and cannot be tested here --
QEMU has no Intel GPU to emulate. Everything it does at load time is a
register *read* (transcoder timings, pipe frame counter) plus EDID over
GMBUS, all with timeouts, and it refuses to register if the registers read
back all-ones or no pipe is active. The write paths are the refresh
change (VTOTAL and VBLANK only, never the PLL -- the same trick the gen4
code already uses on real hardware), the cursor plane, and the resolution
change. The last one also never touches the PLL or the panel timings: it
writes `PIPESRC`, the primary plane's stride/size, and the panel fitter
(gen6-gen8, `PF_CTL`/`PF_WIN_SZ`) or the pipe scaler (gen9,
`PS_CTRL`/`PS_WIN_SZ`), so the panel keeps running at its own native mode
and the hardware scales a smaller source up to it -- exactly what "GPU
scaling" means on a laptop. Mode sizes are limited to what already fits in
the firmware framebuffer, so no new scanout memory is ever needed. If it misbehaves,
boot the GRUB entry **"safe graphics, no driver modules"**, which adds
`nomodules` to the kernel command line; `blacklist=intel-display` skips just
that one.

**Linux source compatibility.** This is a HamixOS-shaped KPI, not a
`linux/*.h` emulation. Making an unmodified `r8169.c` build against it means
adding the Linux-named wrappers (`struct device`, `workqueue`, `kfifo`,
`spinlock_t`, the DMA API) on top of these primitives -- that is the actual
LinuxKPI layer, and it now has something to be built on.

**Unloading a live driver.** `module::unload` runs `hamix_module_exit`, drops
the claims and frees the image, but nothing stops a device that is still
running from writing into freed DMA memory. Modules are loaded at boot and
not unloaded in practice yet.
