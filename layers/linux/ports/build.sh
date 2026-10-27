#!/usr/bin/env bash
set -euo pipefail

ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"
ALPINE_BRANCH="${ALPINE_BRANCH:-v3.22}"
MUSL_VERSION="${MUSL_VERSION:-1.2.5-r12}"
HEADERS_VERSION="${HEADERS_VERSION:-6.14.2-r0}"
BUSYBOX_APK_VERSION="${BUSYBOX_APK_VERSION:-1.37.0-r20}"
BUSYBOX_VERSION="${BUSYBOX_VERSION:-1.37.0}"
BUSYBOX_SHA256="${BUSYBOX_SHA256:-3311dff32e746499f4df0d5df04d7eb396382d7e108bb9250e7b519b837043a4}"
WAYLAND_VERSION="${WAYLAND_VERSION:-1.23.1-r3}"
LIBFFI_VERSION="${LIBFFI_VERSION:-3.4.8-r0}"
WAYLAND_PROTOCOLS_VERSION="${WAYLAND_PROTOCOLS_VERSION:-1.44-r0}"
BUSYBOX_APPLETS="ls cat echo printf head tail wc sort uniq cut tr grep egrep fgrep sed awk find xargs stat du df
basename dirname pwd env printenv uname id whoami groups hostname date sleep usleep true false seq yes
md5sum sha1sum sha256sum sha512sum base64 hexdump od xxd rev tac nl fold expand unexpand cmp diff tee
touch mkdir rmdir rm cp mv ln chmod chown realpath readlink which expr test cal factor free uptime ps
strings dd truncate split paste comm shuf mktemp nproc tty stty clear reset less more vi hd sum cksum
gzip gunzip zcat bzip2 bunzip2 bzcat xz unxz xzcat tar unzip cpio wget nc nslookup ping sh ash"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
SYSROOT="$REPO_DIR/overlay/opt/linux"
WORK="${WORK_DIR:-$REPO_DIR/target/linux-sysroot}"
PKG_ROOT="$WORK/root"

mkdir -p "$WORK" "$PKG_ROOT"
for pkg in "musl-$MUSL_VERSION" "musl-dev-$MUSL_VERSION" "linux-headers-$HEADERS_VERSION" "busybox-$BUSYBOX_APK_VERSION"; do
    if [ ! -f "$WORK/$pkg.apk" ]; then
        echo "[linux-sysroot] fetching $pkg"
        curl -fsSL -o "$WORK/$pkg.apk" "$ALPINE_MIRROR/$ALPINE_BRANCH/main/x86_64/$pkg.apk"
    fi
    tar xzf "$WORK/$pkg.apk" -C "$PKG_ROOT" 2>/dev/null
done

CC="${CC:-clang}"
RESOURCE_INCLUDE="$("$CC" -print-resource-dir)/include"
CFLAGS=(--target=x86_64-linux-musl -nostdinc -isystem "$PKG_ROOT/usr/include" -isystem "$RESOURCE_INCLUDE" -O2 -fPIE -fuse-ld=lld -nostdlib)
LIB="$PKG_ROOT/usr/lib"

mkdir -p "$SYSROOT/lib" "$SYSROOT/bin"
install -m 755 "$PKG_ROOT/lib/ld-musl-x86_64.so.1" "$SYSROOT/lib/ld-musl-x86_64.so.1"

for src in "$SCRIPT_DIR"/src/*.c; do
    name="$(basename "$src" .c)"
    echo "[linux-sysroot] building $name-musl (dynamic PIE) and $name-musl-static (static PIE)"
    "$CC" "${CFLAGS[@]}" -pie "$src" "$LIB/Scrt1.o" "$LIB/crti.o" -L"$LIB" -lc "$LIB/crtn.o" \
        -Wl,--dynamic-linker=/lib/ld-musl-x86_64.so.1 -o "$SYSROOT/bin/$name-musl"
    "$CC" "${CFLAGS[@]}" -static-pie "$src" "$LIB/rcrt1.o" "$LIB/crti.o" "$LIB/libc.a" "$LIB/crtn.o" \
        -o "$SYSROOT/bin/$name-musl-static"
    llvm-strip "$SYSROOT/bin/$name-musl" "$SYSROOT/bin/$name-musl-static"
    chmod 755 "$SYSROOT/bin/$name-musl" "$SYSROOT/bin/$name-musl-static"
done

WAYLAND_ROOT="$WORK/wayland"
DEMO_LIB="$SYSROOT/usr/lib/hamix-demo"
mkdir -p "$WAYLAND_ROOT" "$DEMO_LIB"
for pkg in "wayland-dev-$WAYLAND_VERSION" "wayland-libs-client-$WAYLAND_VERSION" "libffi-$LIBFFI_VERSION" "wayland-protocols-$WAYLAND_PROTOCOLS_VERSION"; do
    file="$WORK/$pkg.apk"
    if [ ! -f "$file" ]; then
        echo "[linux-sysroot] fetching $pkg"
        curl -fsSL -o "$file" "$ALPINE_MIRROR/$ALPINE_BRANCH/main/x86_64/$pkg.apk"
    fi
    tar xzf "$file" -C "$WAYLAND_ROOT" 2>/dev/null
done
XDG_SHELL="$WAYLAND_ROOT/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml"
wayland-scanner client-header "$XDG_SHELL" "$WORK/xdg-shell-client-protocol.h"
wayland-scanner private-code "$XDG_SHELL" "$WORK/xdg-shell-protocol.c"
echo "[linux-sysroot] building wlhello (Wayland demo client)"
"$CC" "${CFLAGS[@]}" -pie -isystem "$WAYLAND_ROOT/usr/include" -I"$WORK" \
    "$SCRIPT_DIR/wayland/wlhello.c" "$WORK/xdg-shell-protocol.c" "$LIB/Scrt1.o" "$LIB/crti.o" \
    -L"$LIB" -L"$WAYLAND_ROOT/usr/lib" -lwayland-client -lc "$LIB/crtn.o" \
    -Wl,--dynamic-linker=/lib/ld-musl-x86_64.so.1 -Wl,-rpath,/usr/lib/hamix-demo -o "$SYSROOT/bin/wlhello"
llvm-strip "$SYSROOT/bin/wlhello"
install -m 755 "$WAYLAND_ROOT/usr/lib/libwayland-client.so.0."* "$DEMO_LIB/libwayland-client.so.0"
install -m 755 "$WAYLAND_ROOT/usr/lib/libffi.so.8."* "$DEMO_LIB/libffi.so.8"

BUSYBOX_SRC="$WORK/busybox-$BUSYBOX_VERSION"
BUSYBOX_TARBALL="$WORK/busybox-$BUSYBOX_VERSION.tar.bz2"
if [ ! -x "$BUSYBOX_SRC/busybox" ]; then
    if [ ! -f "$BUSYBOX_TARBALL" ]; then
        echo "[linux-sysroot] fetching busybox $BUSYBOX_VERSION source"
        curl -fsSL -o "$BUSYBOX_TARBALL" "https://busybox.net/downloads/busybox-$BUSYBOX_VERSION.tar.bz2"
    fi
    echo "$BUSYBOX_SHA256  $BUSYBOX_TARBALL" | sha256sum -c --quiet
    rm -rf "$BUSYBOX_SRC"
    tar xjf "$BUSYBOX_TARBALL" -C "$WORK"
    echo "[linux-sysroot] building busybox $BUSYBOX_VERSION (static PIE)"
    (
        cd "$BUSYBOX_SRC"
        export MUSL_ROOT="$PKG_ROOT"
        make HOSTCC=gcc defconfig >/dev/null
        sed -i 's/^CONFIG_TC=y/# CONFIG_TC is not set/' .config
        make HOSTCC=gcc oldconfig </dev/null >/dev/null
        make -j"$(nproc)" HOSTCC=gcc CC="$SCRIPT_DIR/musl-cc" AR=llvm-ar STRIP=llvm-strip >"$WORK/busybox-build.log" 2>&1 \
            || { tail -30 "$WORK/busybox-build.log"; exit 1; }
    )
fi
install -m 755 "$BUSYBOX_SRC/busybox" "$SYSROOT/bin/busybox"
install -m 755 "$PKG_ROOT/bin/busybox" "$SYSROOT/bin/busybox-dynamic"

mkdir -p "$SYSROOT/etc"
printf 'root:x:0:\nuser:x:1000:\n' > "$SYSROOT/etc/group"

PROVIDED="$REPO_DIR/overlay/etc/pantry/provided"
mkdir -p "$(dirname "$PROVIDED")"
cat > "$PROVIDED" <<PROVIDED_EOF
P:musl
V:$MUSL_VERSION
T:musl C library and dynamic linker (shipped with HamixOS)
p:so:libc.musl-x86_64.so.1=1

P:busybox
V:$BUSYBOX_VERSION-r99
T:BusyBox (static PIE, shipped with HamixOS)
p:cmd:busybox

P:busybox-binsh
V:$BUSYBOX_VERSION-r99
T:/bin/sh from BusyBox (shipped with HamixOS)
p:/bin/sh cmd:sh=$BUSYBOX_VERSION-r99
PROVIDED_EOF

for applet in $BUSYBOX_APPLETS; do
    printf '#!/opt/linux/bin/busybox\n' > "$SYSROOT/bin/$applet"
    chmod 755 "$SYSROOT/bin/$applet"
done

echo "[linux-sysroot] sysroot ready at $SYSROOT"
