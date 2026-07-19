# HamixOS

Unix-like operating system written in Rust from scratch, targeting x86_64.

## Hardware Targets

- Pentium G640 (Sandy Bridge, 2 cores, 2.8 GHz)
- Celeron T3100 (Penryn, 2 cores, 1.9 GHz)
- Any x86_64 CPU without SSE/MMX requirements

## Architecture

```
HamixOS/
├── kernel/                       # Kernel (Ring 0)
│   ├── src/
│   │   ├── main.rs               # Entry point (rust_main), boot sequence, panic handler
│   │   ├── arch/
│   │   │   ├── mod.rs            # Arch module root (re-exports x86_64)
│   │   │   └── x86_64/
│   │   │       ├── boot.S        # _start, long-mode + identity-paging bring-up
│   │   │       ├── gdt.rs        # GDT + TSS (Long Mode, double-fault IST stack)
│   │   │       ├── idt.rs        # IDT, CPU exception handlers, PIC remap, IRQ handlers
│   │   │       ├── mod.rs        # outb/inb/hlt/cli/sti/rdmsr/wrmsr/without_interrupts
│   │   │       └── paging.rs     # Reserved for future dynamic paging (currently a stub)
│   │   ├── drivers/
│   │   │   ├── mod.rs
│   │   │   ├── tty.rs            # Colored console API on top of text_mode
│   │   │   ├── klog.rs           # Boot log ring buffer (backs `dmesg`)
│   │   │   ├── serial.rs         # COM1 serial (debug output, serial_print!/serial_println!)
│   │   │   ├── video/
│   │   │   │   ├── mod.rs
│   │   │   │   └── text_mode.rs  # VGA 80x25 text-mode driver, hw cursor, panic screen
│   │   │   └── input/
│   │   │       ├── mod.rs
│   │   │       └── keyboard.rs   # PS/2 keyboard, scancode set 1 + extended keys
│   │   ├── hsh/mod.rs            # hsh: login + shell (command loop, history, dev tools)
│   │   ├── memory/
│   │   │   ├── mod.rs            # Multiboot2 tag parser, subsystem init
│   │   │   ├── frame.rs          # Bitmap physical frame allocator
│   │   │   └── heap.rs           # Kernel heap (address-ordered, coalescing allocator)
│   │   ├── task/mod.rs           # PIT timer, tick counter
│   │   └── syscall/mod.rs        # SYSCALL MSR setup (LSTAR/STAR)
│   ├── linker.ld                 # Memory layout (1MB load, BSS symbols)
│   ├── x86_64-hamix_os.json      # Custom Rust target
│   └── .cargo/config.toml        # Build config (nightly, build-std)
├── isoroot/boot/grub/grub.cfg    # GRUB2 menu
├── apps/                         # Userland crates reserved for future ring-3 support
├── sdk/hamix_std/                # Userland std shim reserved for future ring-3 support
└── build.sh                      # Full build -> ISO script
```

The console is VGA text mode (80x25, `0xB8000`), not a linear framebuffer —
`drivers::video::text_mode` owns the hardware, `drivers::tty` layers 16-color
attributes and a `core::fmt::Write` impl on top of it, and `hsh` never touches
the hardware directly.

## Build Requirements

```bash
# Install Rust nightly with source
rustup install nightly
rustup component add rust-src --toolchain nightly

# Install GRUB tools and xorriso
sudo apt install grub-pc-bin grub-efi-amd64-bin xorriso mtools
# or on Arch:
sudo pacman -S grub xorriso
```

## Building

```bash
chmod +x build.sh
./build.sh
```

This produces `hamix_os.iso`.

## Running

### QEMU (development)
```bash
qemu-system-x86_64 \
    -cdrom hamix_os.iso \
    -m 256M \
    -serial stdio \
    -vga std
```

### Ventoy (real hardware)
1. Install Ventoy on USB stick
2. Copy `hamix_os.iso` to the Ventoy partition
3. Boot — HamixOS will appear in the Ventoy menu

## Login

Default accounts, backed by real `/etc/passwd` + `/etc/shadow` files in the
VFS (see [`docs/PERMISSIONS.md`](docs/PERMISSIONS.md)):

| User   | Password | uid  | sudoer |
|--------|----------|------|--------|
| `root` | `hamix`  | 0    | always |
| `user` | `user`   | 1000 | yes (listed in `/etc/sudoers`) |

Password is hidden while typing (Unix behavior). Change either password after
first boot with `passwd`.

## Shell: hsh

HamixOS ships with its own shell, **hsh** (`kernel/src/hsh/mod.rs`). It owns the
login prompt and command loop; the VGA text rendering itself lives in the
separate `drivers::tty` / `drivers::video::text_mode` driver. hsh supports
command history navigable with the Up/Down arrow keys. The prompt reflects the
real current directory (`user@hamix:/some/path$`, collapsing the user's home
directory to `~` the way bash does).

hsh is still a module compiled into the kernel binary, not a standalone
`/bin/hsh` executable loaded from rootfs -- see
[`docs/USERSPACE_ROADMAP.md`](docs/USERSPACE_ROADMAP.md) for exactly what is
missing to make that possible and the order in which it gets built.

| Command       | Description                      |
|---------------|----------------------------------|
| `help`        | List all commands                |
| `clear`       | Clear the screen                 |
| `echo <text>` | Print text                       |
| `uname [-a]`  | Kernel/OS name                   |
| `whoami`      | Current user (or `root` under `sudo`) |
| `id`          | Show current uid/euid            |
| `meminfo`     | RAM usage (free/used/total)      |
| `uptime`      | System uptime in seconds         |
| `pwd`         | Current directory                |
| `ls [path]`   | List directory entries (real in-RAM VFS) |
| `cd <path>`   | Change directory                 |
| `cat <path>`  | Print file contents              |
| `mkdir <p>`   | Create a directory                |
| `touch <p>`   | Create an empty file             |
| `rm <path>`   | Remove a file or empty directory |
| `chmod <mode> <p>` | Change permission bits (octal, e.g. `755`) |
| `chown <user> <p>` | Change file owner (root only)    |
| `tree`        | Recursive directory listing from cwd |
| `echo a > f`  | Write/append output to a file    |
| `fb <color>`  | Fill the linear framebuffer (`intel_penryn` driver) |
| `diskls`      | List root dir of the ext4 disk image module |
| `diskcat <p>` | Read a file straight off the ext4 disk image |
| `hostname`    | System hostname                  |
| `cpuinfo`     | CPU architecture info             |
| `sudo <cmd>`  | Run a command as root (see `docs/PERMISSIONS.md`) |
| `passwd [user]` | Change a password               |
| `useradd <name>` | Create a new user (root only)  |
| `version`     | HamixOS version                  |
| `history`     | Show command history              |
| `logout`      | Log out, return to login prompt  |
| `reboot`      | Reboot the machine (root only, or via `sudo`) |
| `halt`        | Halt the system (root only, or via `sudo`) |

## Filesystem (`kernel/src/fs`)

HamixOS boots as a **live image**: `build.sh` packs a real FHS-style tree
(`/bin /sbin /etc /dev /proc /sys /tmp /var /usr /home /root /lib ...`) into
`initramfs.tar` and GRUB hands it to the kernel as a multiboot2 module. At
boot the kernel un-tars it straight into an in-RAM filesystem (`fs::Vfs`) —
nothing is read from optical/USB media again after that point. `/etc/passwd`,
`/etc/hostname`, `/etc/os-release` are real files you can `cat`; `/proc/uptime`,
`/proc/meminfo`, `/proc/version`, `/proc/cpuinfo` are generated live from
kernel state on every read; `/dev/null`, `/dev/zero`, `/dev/console`,
`/dev/random` are real (if minimal) device nodes.

A second module, `disk.img`, is a genuine **ext4** filesystem image built with
`mkfs.ext4 -d rootfs/` when e2fsprogs is available. `kernel/src/fs/ext4.rs` is
a real, from-scratch read-only ext4 driver (superblock, group descriptors,
extent-tree block mapping, linear directory parsing) you can browse with
`diskls` / `diskcat`. It does not do journal replay, htree lookups, or
`metadata_csum`/64-bit group descriptors, so `build.sh` disables those
features when formatting.

## Video: `intel_penryn` driver

`kernel/src/drivers/video/intel_penryn.rs` drives the linear framebuffer that
GRUB/VBE hands off via the multiboot2 framebuffer tag (this is the practical
way to get pixels on real Intel GMA 4500MHD hardware, like the graphics core
paired with an Intel Celeron T3100, without writing a full mode-setting
driver). `fb <red|green|blue|black|gradient>` in `hsh` fills the whole screen
at whatever resolution the firmware reported.

That resolution is entirely up to GRUB, not to this driver, and two things
have to be right for it to match the real panel instead of falling back to a
low default:

1. The multiboot2 header in `kernel/src/main.rs` has to *ask* for a
   framebuffer at all (tag type 5). Without it, GRUB has no signal that the
   kernel wants graphics mode and may hand back nothing or a text-mode
   default.
2. `isoroot/boot/grub/grub.cfg` has to actually set the video mode with
   `set gfxpayload=keep` after loading a driver that can talk to the real
   hardware. `video_bochs`/`video_cirrus` only work against the *emulated*
   VGA adapters QEMU/VirtualBox/Bochs present — they do nothing on a real
   Intel GMA chip, so a config that only loads those two silently falls
   back to a low default resolution on real hardware while looking correct
   under QEMU. `insmod all_video` (plus `vbe` and, under UEFI, `efi_gop`)
   is what actually detects and drives the T3100's GPU.

Both are now in place: the header requests a framebuffer, and `grub.cfg`
loads the real-hardware video drivers before setting `gfxpayload=keep`.

## Syscalls & musl

`kernel/src/syscall/mod.rs` implements the real Linux x86_64 syscall ABI
(`write=1`, `exit=60`, etc. — the numbers musl's syscall stubs use) and wires
`SYSCALL`/`SYSRET` (`IA32_STAR/LSTAR/FMASK`) to a dispatcher backed by the VFS
above. See `docs/MUSL.md` for the exact syscall table, how to build a static
musl binary against it, and — importantly — what's still missing before a
loaded ELF binary can actually reach it (there's no ELF loader or ring-3
switch yet; that's the next milestone).

### Developer tools

| Command                     | Description                              |
|------------------------------|-------------------------------------------|
| `dmesg`                      | Show the kernel boot log                  |
| `hexdump <addr> [len]`       | Dump raw memory as hex + ASCII            |
| `inport <port>`              | Read a byte from an I/O port              |
| `outport <port> <val>`       | Write a byte to an I/O port               |
| `regs`                       | Dump CR0 / CR3 / CR4 control registers    |
| `alloctest <bytes>`          | Exercise the kernel heap allocator        |
| `crash <div0\|bp\|ud\|pf>`   | Trigger a CPU exception to test the IDT   |

## Future Plans

### ARM Support (armv7, aarch64)
Planned for future releases. The kernel architecture is designed to be modular, allowing for HAL layer expansion to support ARM processors.

## Boot Process

1. GRUB2 loads kernel at 1MB via Multiboot2
2. `_start` runs in 32-bit protected mode (GRUB entry), sets up identity paging
   for the first 4GB and switches to 64-bit Long Mode
3. `rust_main` copies the Multiboot2 info structure into a stack buffer
   *before* BSS is zeroed, so it can never be clobbered by the kernel's own
   `.bss` layout, then zeroes BSS and loads GDT/TSS
4. IDT installed, PIC remapped (IRQ0=timer, IRQ1=keyboard) — interrupts stay
   masked off at the CPU level until every subsystem below has finished
5. Multiboot2 tags parsed from the safe stack copy: memory map
6. Physical frame allocator initialized from the memory map
7. Kernel heap initialized (4MB static arena)
8. PS/2 keyboard driver registered
9. SYSCALL MSRs configured, PIT configured at 100Hz
10. Interrupts enabled, login prompt displayed

## Design Decisions

- **No external crates** except `spin` (for `Mutex`). All boot parsing is custom.
- **SSE enabled**: CR4.OSFXSR/OSXMMEXCPT are set during boot because the compiler emits SSE instructions (e.g. in `memcpy`/`memset` and `x86-interrupt` handlers) even for a "soft-float" target; leaving them off causes `#UD` on first use.
- **No red zone**: disabled for kernel interrupt safety.
- **Static heap**: 4MB compile-time arena, backed by an address-ordered
  free-list allocator with block coalescing — no `mmap`, no page allocator
  needed for the heap yet.
- **Text-mode console**: `drivers::video::text_mode` writes directly to the
  VGA buffer at `0xB8000` and drives the hardware cursor; `drivers::tty`
  layers colored output and `core::fmt::Write` on top; `hsh` (login + shell)
  is a separate module built on top of that.
- **Interrupts stay off during boot**: `sti` is only executed once in
  `rust_main`, after every subsystem (GDT/IDT/memory/keyboard/syscall/task)
  has finished initializing, to avoid handling IRQs against a half-built
  kernel.

## Fixed in this update

- **Kernel panic on allocation** (`allocation failed: Layout { size, align }`):
  the heap allocator returned freed blocks to the free list smaller than what
  it had actually taken from it (it dropped the block header and alignment
  padding on every `dealloc`), and never coalesced adjacent free blocks. Under
  normal shell use the heap fragmented until a small allocation had nowhere
  left to go. `memory::heap` was rewritten as an address-ordered, coalescing
  allocator that reclaims the exact block it handed out — see
  `kernel/src/memory/heap.rs`.
- **Cursor stuck at the top-left corner**: the VGA text driver wrote
  characters into the frame buffer but never moved the hardware text cursor
  (VGA CRT controller, ports `0x3D4`/`0x3D5`). It now updates the hardware
  cursor after every write, so it tracks the current column/row like a normal
  terminal.
- **Colors were dead code**: `drivers::tty`'s color constants and
  `*_colored` functions existed but were wired to a single hardcoded
  attribute byte, so every color argument was silently ignored. They now map
  to real VGA 16-color attribute bytes.
- **Panic screen**: kernel panics now render on a dedicated red full-screen
  panic display — "KERNEL PANIC" centered near the top, the panic reason
  word-wrapped and centered below it — instead of a plain scrolling text
  line. It's drawn with direct VGA writes and no heap allocation, so it also
  renders correctly for the "heap exhausted" class of panics that caused it.
- **Multiboot info could be clobbered**: `rust_main` now snapshots the
  Multiboot2 info structure to a stack buffer before zeroing `.bss`, so the
  boot-time memory map can never be corrupted by the kernel's own BSS layout
  landing on top of it.
- **Interrupts enabled too early**: the PIC/PIT/keyboard IRQs were unmasked
  and `sti` executed midway through `idt::init()`, before memory, keyboard,
  and task subsystems existed. Interrupts now stay masked at the CPU level
  until the entire boot sequence completes.

## Future Roadmap

- [ ] VFS (virtual filesystem)
- [ ] ext2 / FAT32 driver
- [ ] ATA/SATA disk driver
- [ ] Process management (fork/exec)
- [ ] Ring 3 user space
- [ ] Persistent user database in `/etc/passwd`
- [ ] ACPI power management
- [ ] Network stack (RTL8139/e1000)
- [ ] x86 (32-bit) support
- [ ] ARM support (armv7, aarch64) — planned for future releases
