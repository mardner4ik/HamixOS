# hxlibc

The C library of HamixOS: [mlibc](https://github.com/managarm/mlibc) 5.0.0 with the
HamixOS system port, kept here as a complete source tree (like relibc in Redox). The build
uses this directory directly; nothing is fetched or patched at build time.

| Path | What it is |
|---|---|
| `options/` | portable libc parts (ANSI, POSIX, Linux, BSD, glibc extensions, iconv, intl, crypt, ELF) |
| `abis/linux/` | ABI constants and structures (HamixOS uses the Linux ABI values) |
| `sysdeps/hamix/` | the HamixOS port: system calls, `crt*`, `ld.hx` support, headers |
| `subprojects/` | frigg, cshim, cxxshim (vendored, used by meson) |
| `dummy-libs/` | empty `libm`, `libpthread`, `librt`, ... (everything lives in `libhxc.so`) |
| `rust/` | Rust side: `hxlibc-sys` (FFI bindings to `libhxc.so` and the HamixOS call numbers); future Rust parts of the library go here |
| `cross/`, `hx-cc`, `hx-c++` | cross toolchain wrappers and meson cross files |
| `applied-patches/` | the HamixOS changes to upstream mlibc, already applied to this tree; kept to rebase onto a newer mlibc |
| `UPSTREAM` | the upstream version this tree is based on |

Build: `cargo xtask hxlibc-build [--arch x86_64|aarch64|riscv64]` -> sysroot in
`out/hxlibc/ARCH/sysroot`. Change the library by editing the files here; to move to a new mlibc,
diff this tree against `UPSTREAM` and re-apply the differences.
