# HamixOS

Unix-like operating system written in Rust from scratch, targeting x86_64.

## Hardware Targets

- Pentium G640 (Sandy Bridge, 2 cores, 2.8 GHz)
- Celeron T3100 (Penryn, 2 cores, 1.9 GHz)
- Any x86_64 CPU without SSE/MMX requirements

## Architecture

```
HamixOS/
├── kernel/                    # Kernel (Ring 0)
│   ├── src/
│   │   ├── main.rs            # Entry point (_start), subsystem init
│   │   ├── arch/x86_64/       # HAL: GDT, IDT, paging, I/O
│   │   │   ├── gdt.rs         # Global Descriptor Table (Long Mode)
│   │   │   ├── idt.rs         # Interrupt Descriptor Table + PIC
│   │   │   ├── mod.rs         # outb/inb/hlt/cli/sti/rdmsr/wrmsr
│   │   │   └── paging.rs      # Page tables (stub for future use)
│   │   ├── drivers/
│   │   │   ├── console.rs     # Graphical terminal, login, shell
│   │   │   ├── serial.rs      # COM1 serial (debugging)
│   │   │   ├── video/vesa.rs  # Linear framebuffer (VBE/Multiboot2)
│   │   │   ├── input/keyboard.rs  # PS/2 keyboard, scancode set 1
│   │   │   └── font8x16.bin   # Embedded 256-glyph 8x16 bitmap font
│   │   ├── memory/
│   │   │   ├── mod.rs         # Multiboot2 parser, subsystem init
│   │   │   ├── frame.rs       # Bitmap physical frame allocator
│   │   │   └── heap.rs        # Kernel heap (linked-list allocator)
│   │   ├── task/mod.rs        # PIT timer, tick counter
│   │   └── syscall/mod.rs     # SYSCALL MSR setup (LSTAR/STAR)
│   ├── linker.ld              # Memory layout (1MB load, BSS symbols)
│   ├── x86_64-hamix_os.json   # Custom Rust target
│   └── .cargo/config.toml     # Build config (nightly, build-std)
├── isoroot/boot/grub/grub.cfg # GRUB2 menu (1024x768x32, fallback 640x480)
└── build.sh                   # Full build -> ISO script
```

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

Default credentials:
- **User:** `root`
- **Password:** `hamix`

Password is hidden while typing (Unix behavior).

## Shell Commands

| Command       | Description                      |
|---------------|----------------------------------|
| `help`        | List all commands                |
| `clear`       | Clear the screen                 |
| `echo <text>` | Print text                       |
| `uname [-a]`  | Kernel/OS name                   |
| `whoami`      | Current user                     |
| `meminfo`     | RAM usage (free/used/total)      |
| `uptime`      | System uptime in seconds         |
| `pwd`         | Current directory (/)            |
| `ls`          | List directory entries           |
| `hostname`    | System hostname                  |
| `cpuinfo`     | CPU architecture info            |
| `version`     | HamixOS version                  |
| `logout`      | Log out, return to login prompt  |
| `reboot`      | Reboot the machine               |
| `halt`        | Halt the system                  |

## Boot Process

1. GRUB2 loads kernel at 1MB via Multiboot2
2. `_start()` is called (32-bit protected mode entry via GRUB)
3. BSS zeroed, GDT loaded (switches to 64-bit Long Mode)
4. IDT installed, PIC remapped (IRQ0=timer, IRQ1=keyboard)
5. Multiboot2 tags parsed: memory map + framebuffer address
6. Physical frame allocator initialized from memory map
7. Kernel heap initialized (4MB static arena)
8. VESA framebuffer configured from Multiboot2 tag
9. PS/2 keyboard driver registered
10. PIT configured at 100Hz
11. Login prompt displayed on graphical console

## Design Decisions

- **No external crates** except `spin` (for `Mutex`). All boot parsing is custom.
- **Soft-float only**: `-mmx,-sse,+soft-float` — safe on all target CPUs.
- **No red zone**: disabled for kernel interrupt safety.
- **Embedded font**: 4KB binary font baked into the kernel binary via `include_bytes!`.
- **Static heap**: 4MB compile-time arena — no `mmap`, no page allocator needed for heap yet.
- **Graphical console**: renders directly to linear framebuffer at any resolution GRUB provides.

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
