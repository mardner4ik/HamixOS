#!/usr/bin/env bash
set -euo pipefail

ARCH="${1:-x86_64}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_DIR="$SCRIPT_DIR/kernel"
ISO_DIR="$SCRIPT_DIR/isoroot"
ROOTFS_DIR="$SCRIPT_DIR/rootfs"
OUT="$SCRIPT_DIR/hamix_os.iso"
MODULES="virtio-gpu intel-display intel-gma ahci xhci e1000 e1000e yukon ar9285"
APPS="hsh hello_world hxserver hed hxhello hxmon hxfiles hxcmd hxterm hxview hxinstall hxcalc hxsettings hxnotes hxsound hxvideo pantry hxwayland"

case "$ARCH" in
    x86_64)
        KERNEL_TARGET="$KERNEL_DIR/x86_64-hamix_os.json"
        KERNEL_TRIPLE="x86_64-hamix_os"
        APP_TARGET="$KERNEL_DIR/x86_64-hamix_os.json"
        APP_TRIPLE="x86_64-hamix_os"
        ;;
    aarch64)
        KERNEL_TARGET="aarch64-unknown-none-softfloat"
        KERNEL_TRIPLE="$KERNEL_TARGET"
        APP_TARGET="aarch64-unknown-none"
        APP_TRIPLE="$APP_TARGET"
        ROOTFS_DIR="$SCRIPT_DIR/rootfs-aarch64"
        ;;
    riscv64)
        KERNEL_TARGET="riscv64imac-unknown-none-elf"
        KERNEL_TRIPLE="$KERNEL_TARGET"
        APP_TARGET="riscv64gc-unknown-none-elf"
        APP_TRIPLE="$APP_TARGET"
        ROOTFS_DIR="$SCRIPT_DIR/rootfs-riscv64"
        ;;
    *)
        echo "[ERROR] Unknown architecture '$ARCH'. Supported: x86_64 (default), aarch64, riscv64."
        exit 1
        ;;
esac
MODULE_TARGET="$KERNEL_TARGET"
MODULE_TRIPLE="$KERNEL_TRIPLE"
PORT_OUT="$SCRIPT_DIR/out/$ARCH"
HOST_TRIPLE="$(rustc +nightly -vV | sed -n 's/^host: //p')"
if command -v llvm-objcopy >/dev/null 2>&1; then
    OBJCOPY="llvm-objcopy"
elif command -v aarch64-linux-gnu-objcopy >/dev/null 2>&1; then
    OBJCOPY="aarch64-linux-gnu-objcopy"
else
    SYSROOT="$(rustc +nightly --print sysroot)"
    OBJCOPY="env LD_LIBRARY_PATH=$SYSROOT/lib $SYSROOT/lib/rustlib/$HOST_TRIPLE/bin/rust-objcopy"
fi

echo "[HamixOS] Building the $ARCH kernel..."
(
    cd "$KERNEL_DIR"
    cargo +nightly build \
        --release \
        -Z build-std=core,compiler_builtins,alloc \
        -Z build-std-features=compiler-builtins-mem \
        --target "$KERNEL_TARGET"
)
KERNEL_BIN="$SCRIPT_DIR/target/$KERNEL_TRIPLE/release/kernel"
if [ ! -f "$KERNEL_BIN" ]; then
    echo "[ERROR] Kernel binary not found at $KERNEL_BIN"
    exit 1
fi
if [ "$ARCH" = x86_64 ] && ! grub-file --is-x86-multiboot2 "$KERNEL_BIN"; then
    echo "[WARN] grub-file multiboot2 check failed -- verify manually"
fi

echo "[HamixOS] Building the root filesystem..."
mkdir -p "$ROOTFS_DIR"
find "$ROOTFS_DIR" -mindepth 1 -maxdepth 1 ! -name home ! -name root -exec rm -rf {} +
for d in bin sbin etc etc/hamix dev proc sys tmp var usr home root lib mnt media opt srv boot run \
         usr/bin usr/sbin usr/lib usr/lib/hamix/boot usr/share usr/share/doc var/log var/tmp var/cache home/user; do
    mkdir -p "$ROOTFS_DIR/$d"
done
mkdir -p "$ROOTFS_DIR/var/cache/nook" "$ROOTFS_DIR/home/user/Videos" "$ROOTFS_DIR/root/Videos"
chmod 1777 "$ROOTFS_DIR/tmp" "$ROOTFS_DIR/var/cache/nook"
chmod 700 "$ROOTFS_DIR/root"

echo "hamix" > "$ROOTFS_DIR/etc/hostname"

cat > "$ROOTFS_DIR/etc/hosts" <<'EOF'
127.0.0.1 localhost hamix
::1 localhost
EOF

cat > "$ROOTFS_DIR/etc/resolv.conf" <<'EOF'
nameserver 1.1.1.1
nameserver 8.8.8.8
EOF

head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$ROOTFS_DIR/etc/machine-id"
echo >> "$ROOTFS_DIR/etc/machine-id"

cat > "$ROOTFS_DIR/etc/passwd" <<'EOF'
root:x:0:0:root:/root:/usr/bin/hsh
user:x:1000:1000:user:/home/user:/usr/bin/hsh
EOF

cat > "$ROOTFS_DIR/etc/shadow" <<'EOF'
root:00000001:45132e2d
user:00000001:da7b420e
EOF
chmod 600 "$ROOTFS_DIR/etc/shadow"

cat > "$ROOTFS_DIR/etc/sudoers" <<'EOF'
user
EOF
chmod 600 "$ROOTFS_DIR/etc/sudoers"

cat > "$ROOTFS_DIR/etc/os-release" <<'EOF'
NAME="HamixOS"
ID=hamix
VERSION="0.6.1"
PRETTY_NAME="HamixOS 0.6 (live)"
EOF

cat > "$ROOTFS_DIR/etc/motd" <<'EOF'
HamixOS live system -- everything you change stays in RAM until you install.
  help          list shell commands        startx     start the Nook desktop
  diskls        disks and partitions       fetch      system summary
Install to a disk from Nook ("Install HamixOS") or read /usr/share/hamix/install.sh.
EOF

cat > "$ROOTFS_DIR/etc/fstab" <<'EOF'
# HamixOS mounts its filesystems itself, see /proc/mounts.
EOF

cat > "$ROOTFS_DIR/usr/share/doc/README" <<'EOF'
/usr/bin holds the userspace programs built from apps/ against sdk/hamix_std:
hsh (the shell), hxserver (Nook), hxterm, hxfiles, hxview, hxinstall, hxcalc,
hxsettings, hxnotes, hxvideo, hxsound, hxmon, hxcmd, hed. The shell used after login is chosen
in /etc/hamix/login.conf.
EOF

touch "$ROOTFS_DIR/var/log/boot.log"
chmod 700 "$ROOTFS_DIR/home/user"

cp "$KERNEL_BIN" "$ROOTFS_DIR/boot/kernel.bin"

if [ "$ARCH" = x86_64 ]; then
    echo "[HamixOS] Generating GRUB images for installed systems..."
    grub-mkimage -O i386-pc -o "$ROOTFS_DIR/usr/lib/hamix/boot/core.img" -p '(,msdos1)/boot/grub' \
        biosdisk part_msdos ext2 normal multiboot2 configfile all_video gfxterm video_bochs video_cirrus vbe vga echo test boot
    cp /usr/lib/grub/i386-pc/boot.img "$ROOTFS_DIR/usr/lib/hamix/boot/boot.img"
fi

echo "[HamixOS] Building userspace apps..."
for app in $APPS; do
    APP_DIR="$SCRIPT_DIR/apps/$app"
    if [ ! -d "$APP_DIR" ]; then
        echo "[WARN] apps/$app does not exist, skipping"
        continue
    fi
    (
        cd "$APP_DIR"
        cargo +nightly build \
            --release \
            -Z build-std=core,compiler_builtins,alloc \
            -Z build-std-features=compiler-builtins-mem \
            --target "$APP_TARGET"
    ) || { echo "[WARN] $app does not build for $ARCH, skipping"; continue; }
    APP_BIN="$SCRIPT_DIR/target/$APP_TRIPLE/release/$app"
    if [ -f "$APP_BIN" ]; then
        install -m 755 "$APP_BIN" "$ROOTFS_DIR/usr/bin/$app"
    else
        echo "[WARN] $app did not produce $APP_BIN"
    fi
done

echo "[HamixOS] Building loadable driver modules..."
case "$ARCH" in
    x86_64) MODULE_RELOCATION=pie ;;
    *) MODULE_RELOCATION=static ;;
esac
case "$ARCH" in
    x86_64) C_MODULE_FLAGS="-target x86_64-unknown-none -mno-red-zone -mgeneral-regs-only -fpie" ;;
    aarch64) C_MODULE_FLAGS="-target aarch64-unknown-none -mgeneral-regs-only -fno-pic" ;;
    riscv64) C_MODULE_FLAGS="-target riscv64-unknown-elf -march=rv64imac -mabi=lp64 -mcmodel=medany -fno-pic" ;;
esac
build_c_module() {
    local name="$1" dir="$2" work="$SCRIPT_DIR/target/cmodules/$ARCH/$1"
    command -v clang >/dev/null 2>&1 || { echo "[WARN] clang not found, cannot build $name"; return 1; }
    rm -rf "$work"
    mkdir -p "$work"
    local resource
    resource="$(clang -print-resource-dir)/include"
    local objects=()
    for source in "$dir"/src/*.c "$SCRIPT_DIR/sdk/hamix_linuxkpi/src/linuxkpi.c"; do
        local object="$work/$(basename "$source" .c).o"
        clang $C_MODULE_FLAGS -ffreestanding -fno-builtin -nostdinc -isystem "$resource" \
            -I"$SCRIPT_DIR/sdk/hamix_linuxkpi/include" -I"$dir/src" -O2 -fno-stack-protector -fno-common \
            -fno-asynchronous-unwind-tables -std=gnu11 -Wno-unused-function -Wno-pointer-sign \
            -DKBUILD_MODNAME="\"$name\"" -c "$source" -o "$object" || return 1
        objects+=("$object")
    done
    ld.lld -r -o "$work/$name.o" "${objects[@]}" || return 1
    check_module_symbols "$name" "$work/$name.o" || exit 1
    install -m 644 "$work/$name.o" "$ROOTFS_DIR/lib/modules/$name.ko"
    sha256sum "$work/$name.o" | cut -d' ' -f1 > "$ROOTFS_DIR/lib/modules/$name.sig"
    python3 "$SCRIPT_DIR/tools/linuxkpi/device-ids.py" "$work/$name.o" > "$ROOTFS_DIR/lib/modules/$name.ids" || cp "$dir/module.ids" "$ROOTFS_DIR/lib/modules/$name.ids" 2>/dev/null || true
    if [ -f "$dir/module.conf" ]; then
        install -m 644 "$dir/module.conf" "$ROOTFS_DIR/lib/modules/$name.conf"
    fi
}
KERNEL_EXPORTS="$SCRIPT_DIR/target/kernel-exports.txt"
grep -o '^ *"[A-Za-z0-9_]*" =>' "$KERNEL_DIR/src/module/symbols.rs" | tr -d ' "=>' | sort -u > "$KERNEL_EXPORTS"
check_module_symbols() {
    local name="$1" object="$2" missing
    missing="$(nm -u "$object" | awk '{print $2}' | sort -u | comm -23 - "$KERNEL_EXPORTS" | grep -v -E 'panic|_fail$|rust_begin_unwind|__stack_chk_fail' || true)"
    if [ -n "$missing" ]; then
        echo "[ERROR] module $name needs symbols the kernel does not export:"
        echo "$missing" | sed 's/^/    /'
        return 1
    fi
}
check_module_relocations() {
    local name="$1" object="$2" absolute
    [ "$ARCH" = x86_64 ] || return 0
    absolute="$(readelf -r "$object" | awk '$3 == "R_X86_64_32" || $3 == "R_X86_64_32S" {n++} END {print n+0}')"
    if [ "$absolute" != 0 ]; then
        echo "[ERROR] module $name has $absolute 32-bit absolute relocations; it would fail to load above 2 GiB"
        return 1
    fi
}
mkdir -p "$ROOTFS_DIR/lib/modules"
for module in $MODULES; do
    MODULE_DIR="$SCRIPT_DIR/drivers/$module"
    if [ ! -d "$MODULE_DIR" ]; then
        echo "[WARN] drivers/$module does not exist, skipping"
        continue
    fi
    if [ ! -f "$MODULE_DIR/Cargo.toml" ] && ls "$MODULE_DIR"/src/*.c >/dev/null 2>&1; then
        build_c_module "$module" "$MODULE_DIR" || echo "[WARN] module $module does not build for $ARCH, skipping"
        continue
    fi
    (
        cd "$MODULE_DIR"
        cargo +nightly rustc \
            --profile module \
            -Z build-std=core,compiler_builtins,alloc \
            -Z build-std-features=compiler-builtins-mem \
            --target "$MODULE_TARGET" \
            --crate-type staticlib \
            -- -C relocation-model="$MODULE_RELOCATION"
    ) || { echo "[WARN] module $module does not build for $ARCH, skipping"; continue; }
    CRATE="${module//-/_}"
    ARCHIVE="$SCRIPT_DIR/target/$MODULE_TRIPLE/module/lib$CRATE.a"
    WORK="$SCRIPT_DIR/target/rmodules/$ARCH/$module"
    rm -rf "$WORK"
    mkdir -p "$WORK"
    MEMBER="$(ar t "$ARCHIVE" 2>/dev/null | grep "^$CRATE-" | head -n1 || true)"
    if [ -z "$MEMBER" ]; then
        echo "[WARN] $module did not produce a relocatable object"
        continue
    fi
    (cd "$WORK" && ar x "$ARCHIVE" "$MEMBER")
    OBJ="$WORK/$module.o"
    mv "$WORK/$MEMBER" "$OBJ"
    check_module_symbols "$module" "$OBJ" || exit 1
    check_module_relocations "$module" "$OBJ" || exit 1
    install -m 644 "$OBJ" "$ROOTFS_DIR/lib/modules/$module.ko"
    sha256sum "$OBJ" | cut -d' ' -f1 > "$ROOTFS_DIR/lib/modules/$module.sig"
    if [ -f "$MODULE_DIR/module.ids" ]; then
        install -m 644 "$MODULE_DIR/module.ids" "$ROOTFS_DIR/lib/modules/$module.ids"
    fi
    if [ -f "$MODULE_DIR/module.conf" ]; then
        install -m 644 "$MODULE_DIR/module.conf" "$ROOTFS_DIR/lib/modules/$module.conf"
    fi
done

OVERLAY_DIR="$SCRIPT_DIR/overlay"
if [ -d "$OVERLAY_DIR" ]; then
    echo "[HamixOS] Copying overlay/ on top of the root filesystem..."
    cp -a "$OVERLAY_DIR/." "$ROOTFS_DIR/"
    chmod 755 "$ROOTFS_DIR/usr/share/hamix/install.sh"
fi

if [ -n "${HAMIX_EXTRA:-}" ] && [ -d "$HAMIX_EXTRA" ]; then
    echo "[HamixOS] Copying $HAMIX_EXTRA on top of the root filesystem..."
    cp -a "$HAMIX_EXTRA/." "$ROOTFS_DIR/"
fi

echo "[HamixOS] Packing the early module pack (boot/kmods.tar)..."
tar --format=ustar -C "$ROOTFS_DIR" -cf "$ROOTFS_DIR/boot/kmods.tar" lib/modules

echo "[HamixOS] Building mkhext (host tool)..."
(
    cd "$SCRIPT_DIR/tools/mkhext"
    cargo +nightly build --release --target "$HOST_TRIPLE"
)
MKHEXT="$(ls -t "$SCRIPT_DIR"/target/*/release/mkhext 2>/dev/null | head -n1)"
if [ ! -x "$MKHEXT" ]; then
    echo "[ERROR] mkhext was not built"
    exit 1
fi

if [ "$ARCH" != x86_64 ]; then
    mkdir -p "$PORT_OUT"
    echo "[HamixOS] Building the $ARCH root image ($PORT_OUT/hext.img)..."
    "$MKHEXT" --label hamix-live "$ROOTFS_DIR" "$PORT_OUT/hext.img"
    cp "$KERNEL_BIN" "$PORT_OUT/kernel.elf"
    if [ "$ARCH" = aarch64 ]; then
        $OBJCOPY -O binary "$KERNEL_BIN" "$PORT_OUT/Image"
        EFI_SIZE=$(( 0x$(nm "$KERNEL_BIN" | awk '/ __efi_file_size$/{print $1}') ))
        truncate -s "$EFI_SIZE" "$PORT_OUT/Image"
        rm -rf "$PORT_OUT/esp"
        mkdir -p "$PORT_OUT/esp/EFI/BOOT"
        cp "$PORT_OUT/Image" "$PORT_OUT/esp/EFI/BOOT/BOOTAA64.EFI"
        cp "$PORT_OUT/hext.img" "$PORT_OUT/esp/hext.img"
        if command -v mkfs.fat >/dev/null 2>&1 && command -v mcopy >/dev/null 2>&1; then
            ESP_MB=$(( ($(stat -c %s "$PORT_OUT/hext.img") + EFI_SIZE) / 1048576 + 16 ))
            rm -f "$PORT_OUT/efi.img"
            truncate -s "${ESP_MB}M" "$PORT_OUT/efi.img"
            mkfs.fat -F 32 -n HAMIXEFI "$PORT_OUT/efi.img" >/dev/null
            mmd -i "$PORT_OUT/efi.img" ::/EFI ::/EFI/BOOT
            mcopy -i "$PORT_OUT/efi.img" "$PORT_OUT/Image" ::/EFI/BOOT/BOOTAA64.EFI
            mcopy -i "$PORT_OUT/efi.img" "$PORT_OUT/hext.img" ::/hext.img
        fi
        echo "[HamixOS] Done: $PORT_OUT/Image (arm64 Image with an EFI stub) + $PORT_OUT/hext.img"
        echo "  direct: qemu-system-aarch64 -M virt -cpu cortex-a72 -m 1G -nographic -kernel $PORT_OUT/Image -initrd $PORT_OUT/hext.img"
        if [ -f "$PORT_OUT/efi.img" ]; then
            echo "  UEFI:   qemu-system-aarch64 -M virt,acpi=off -cpu cortex-a72 -m 1G -nographic -bios /usr/share/edk2/aarch64/QEMU_EFI.fd -drive file=$PORT_OUT/efi.img,format=raw,if=virtio"
            echo "  $PORT_OUT/efi.img is a FAT32 EFI system partition image; write it to a USB stick or SD card for UEFI/U-Boot boards"
        fi
    else
        echo "[HamixOS] Done: $PORT_OUT/kernel.elf + $PORT_OUT/hext.img"
        echo "  qemu-system-riscv64 -M virt -m 1G -nographic -kernel $PORT_OUT/kernel.elf -initrd $PORT_OUT/hext.img"
    fi
    exit 0
fi

mkdir -p "$ISO_DIR/boot/grub"
cp "$ROOTFS_DIR/boot/kmods.tar" "$ISO_DIR/boot/kmods.tar"
echo "[HamixOS] Building the live image (boot/hext.img.gz)..."
"$MKHEXT" --label hamix-live "$ROOTFS_DIR" "$ISO_DIR/boot/hext.img"
gzip -9 -n -f "$ISO_DIR/boot/hext.img"

DISK_IMG="$SCRIPT_DIR/hamix_disk.img"
if [ ! -f "$DISK_IMG" ]; then
    echo "[HamixOS] Creating a blank 1G test disk for QEMU ($DISK_IMG)..."
    truncate -s 1G "$DISK_IMG"
fi

echo "[HamixOS] Packing initramfs.tar (fallback boot entry)..."
tar --format=ustar -C "$ROOTFS_DIR" -cf "$ISO_DIR/boot/initramfs.tar" .
rm -f "$ISO_DIR/boot/disk.img"

cat > "$ISO_DIR/boot/grub/grub.cfg" <<'GRUBEOF'
set timeout=3
set default=0

insmod multiboot2
insmod all_video
insmod vbe
insmod vga

set gfxmode=1024x768x32,1024x768,800x600x32,800x600,auto
set gfxpayload=keep

menuentry "HamixOS 0.6 Live" {
    multiboot2 /boot/kernel.bin root=live
    module2 /boot/hext.img.gz hext
    module2 /boot/kmods.tar kmods
    boot
}

menuentry "HamixOS 0.6 (use the system installed on disk)" {
    multiboot2 /boot/kernel.bin root=auto
    module2 /boot/hext.img.gz hext
    module2 /boot/kmods.tar kmods
    boot
}

menuentry "HamixOS 0.6 Live (safe graphics, no driver modules)" {
    multiboot2 /boot/kernel.bin root=live nomodules
    module2 /boot/hext.img.gz hext
    boot
}

menuentry "HamixOS 0.6 Live (initramfs fallback)" {
    multiboot2 /boot/kernel.bin root=live
    module2 /boot/initramfs.tar initramfs.tar
    boot
}
GRUBEOF

echo "[HamixOS] Building ISO image..."
cp "$KERNEL_BIN" "$ISO_DIR/boot/kernel.bin"
grub-mkrescue -o "$OUT" "$ISO_DIR" \
    --modules="multiboot2 normal video gfxterm vbe video_bochs video_cirrus all_video" \
    2>/dev/null || \
grub-mkrescue -o "$OUT" "$ISO_DIR" 2>/dev/null

echo "[HamixOS] Done: $OUT"
echo "[HamixOS] Live system in QEMU (SATA test disk for the installer):"
echo "  qemu-system-x86_64 -M q35 -m 1G -cdrom hamix_os.iso -drive file=hamix_disk.img,format=raw,if=none,id=d0 \\"
echo "      -device ide-hd,drive=d0,bus=ide.0 -boot d -serial stdio"
echo "[HamixOS] Boot the installed disk afterwards:  qemu-system-x86_64 -M q35 -m 1G -drive file=hamix_disk.img,format=raw"
