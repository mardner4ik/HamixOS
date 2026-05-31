#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR/kernel"
ISO_DIR="$SCRIPT_DIR/isoroot"
OUT="$SCRIPT_DIR/hamix_os.iso"

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
    --modules="multiboot2 normal video gfxterm vbe all_video" \
    2>/dev/null || \
grub-mkrescue -o "$OUT" "$ISO_DIR" 2>/dev/null

echo "[HamixOS] Done: $OUT"
echo "[HamixOS] Boot with QEMU:"
echo "  qemu-system-x86_64 -cdrom hamix_os.iso -m 256M -serial stdio"
echo "[HamixOS] Or copy to USB with Ventoy and boot hamix_os.iso"
