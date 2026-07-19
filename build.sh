#!/usr/bin/env bash
set -euo pipefail

ARCH="${1:-x86_64}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR/kernel"
ISO_DIR="$SCRIPT_DIR/isoroot"
ROOTFS_DIR="$SCRIPT_DIR/rootfs"
OUT="$SCRIPT_DIR/hamix_os.iso"

build_aarch64() {
    echo "[HamixOS] Building aarch64 skeleton (QEMU 'virt' machine only, NOT a full port)..."
    echo "[HamixOS] Same kernel crate, arch::aarch64 module -- proves the boot chain"
    echo "[HamixOS] (EL-drop -> BSS clear -> UART) and nothing else yet: no drivers,"
    echo "[HamixOS] no fs, no syscalls on this arch."

    cd "$KERNEL_DIR"
    cargo +nightly build \
        --release \
        -Z build-std=core,compiler_builtins,alloc \
        -Z build-std-features=compiler-builtins-mem \
        --target aarch64-unknown-none

    KERNEL_ELF="$SCRIPT_DIR/target/aarch64-unknown-none/release/kernel"
    if [ ! -f "$KERNEL_ELF" ]; then
        echo "[ERROR] aarch64 kernel binary not found at $KERNEL_ELF"
        exit 1
    fi

    echo "[HamixOS] Done: $KERNEL_ELF"
    echo "[HamixOS] Boot with QEMU:"
    echo "  qemu-system-aarch64 -M virt -cpu cortex-a53 -m 256M -nographic -kernel $KERNEL_ELF"
    echo "[HamixOS] Ctrl-A then X to exit QEMU."
}

if [ "$ARCH" = "aarch64" ]; then
    build_aarch64
    exit 0
elif [ "$ARCH" != "x86_64" ]; then
    echo "[ERROR] Unknown architecture '$ARCH'. Supported: x86_64 (default), aarch64 (skeleton only)."
    exit 1
fi

echo "[HamixOS] Building live root filesystem (loaded into RAM at boot)..."
rm -rf "$ROOTFS_DIR"
mkdir -p "$ROOTFS_DIR"

for d in bin sbin etc dev proc sys tmp var usr home root lib mnt media opt srv boot run \
         usr/bin usr/sbin usr/lib usr/share usr/share/doc var/log var/tmp home/user etc/init.d; do
    mkdir -p "$ROOTFS_DIR/$d"
done

cat > "$ROOTFS_DIR/etc/hostname" <<'EOF'
hamix
EOF

cat > "$ROOTFS_DIR/etc/passwd" <<'EOF'
root:x:0:0:root:/root:/bin/hsh
user:x:1000:1000:user:/home/user:/bin/hsh
EOF

cat > "$ROOTFS_DIR/etc/os-release" <<'EOF'
NAME="HamixOS"
ID=hamix
VERSION="0.1.0"
PRETTY_NAME="HamixOS 0.1.0 (live ramdisk)"
EOF

cat > "$ROOTFS_DIR/etc/motd" <<'EOF'
Welcome to HamixOS -- live image, root filesystem lives entirely in RAM.
EOF

cat > "$ROOTFS_DIR/usr/share/doc/README" <<'EOF'
/bin and /sbin are reserved for musl-linked static binaries built against
HamixOS's syscall ABI (see docs/MUSL.md at the repo root for the toolchain
setup). The kernel does not yet contain an ELF loader / ring3 process
switch, so this directory intentionally ships empty in this build.
EOF

touch "$ROOTFS_DIR/var/log/boot.log"

OVERLAY_DIR="$SCRIPT_DIR/overlay"
if [ -d "$OVERLAY_DIR" ]; then
    echo "[HamixOS] Copying overlay/ on top of rootfs (survives every rebuild)..."
    cp -a "$OVERLAY_DIR/." "$ROOTFS_DIR/"
fi

echo "[HamixOS] Packing initramfs.tar (ustar, no compression)..."
mkdir -p "$ISO_DIR/boot"
tar --format=ustar -C "$ROOTFS_DIR" -cf "$ISO_DIR/boot/initramfs.tar" .

if command -v mkfs.ext4 >/dev/null 2>&1; then
    echo "[HamixOS] Building ext4 disk.img from rootfs (mkfs.ext4 -d)..."
    rm -f "$ISO_DIR/boot/disk.img"
    dd if=/dev/zero of="$ISO_DIR/boot/disk.img" bs=1M count=16 status=none
    mkfs.ext4 -q -F -O ^metadata_csum,^64bit -d "$ROOTFS_DIR" "$ISO_DIR/boot/disk.img"
else
    echo "[HamixOS] mkfs.ext4 not found -- writing an empty disk.img placeholder"
    echo "[HamixOS] (install e2fsprogs to get a real bootable ext4 image for 'diskls')"
    dd if=/dev/zero of="$ISO_DIR/boot/disk.img" bs=1M count=1 status=none
fi

echo "[HamixOS] Building kernel..."

cd "$KERNEL_DIR"
cargo +nightly build \
    --release \
    -Z build-std=core,compiler_builtins,alloc \
    -Z build-std-features=compiler-builtins-mem \
    --target x86_64-hamix_os.json

KERNEL_BIN="$SCRIPT_DIR/target/x86_64-hamix_os/release/kernel"

if [ ! -f "$KERNEL_BIN" ]; then
    echo "[ERROR] Kernel binary not found at $KERNEL_BIN"
    exit 1
fi

echo "[HamixOS] Verifying multiboot2 header..."
if ! grub-file --is-x86-multiboot2 "$KERNEL_BIN"; then
    echo "[WARN] grub-file multiboot2 check failed — verify manually"
fi

echo "[HamixOS] Building ISO image..."
mkdir -p "$ISO_DIR/boot"
cp "$KERNEL_BIN" "$ISO_DIR/boot/kernel.bin"

grub-mkrescue -o "$OUT" "$ISO_DIR" \
    --modules="multiboot2 normal video gfxterm vbe video_bochs video_cirrus all_video" \
    2>/dev/null || \
grub-mkrescue -o "$OUT" "$ISO_DIR" 2>/dev/null

echo "[HamixOS] Done: $OUT"
echo "[HamixOS] This is a live image: GRUB loads kernel.bin + initramfs.tar as a"
echo "[HamixOS] multiboot2 module, and the kernel unpacks initramfs.tar straight"
echo "[HamixOS] into an in-RAM filesystem (see kernel/src/fs) -- nothing is read"
echo "[HamixOS] from disk again after boot."
echo "[HamixOS] Boot with QEMU:"
echo "  qemu-system-x86_64 -cdrom hamix_os.iso -m 256M -serial stdio"
echo "[HamixOS] Or copy to USB with Ventoy and boot hamix_os.iso"
