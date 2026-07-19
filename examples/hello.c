/*
 * Build (no musl needed for this raw-syscall smoke test, plain gcc works):
 *
 *   gcc -static -no-pie -nostdlib -fno-stack-protector -fno-pic \
 *       -e _start -Wl,-Ttext-segment=0x2000000 -O2 -o hello.hamix hello.c
 *
 * -Wl,-Ttext-segment=0x2000000 links this at 32MB. HamixOS does not yet
 * have per-process page tables (docs/USERSPACE_ROADMAP.md); every binary
 * shares the kernel's own address space, and the kernel's image + its
 * static 4MB heap (kernel/src/memory/heap.rs) already occupies roughly
 * 0x100000 to somewhere past 0x400000. Linking below that, e.g. gcc's
 * default ~0x400000, lands your segments inside live kernel memory. The
 * loader (kernel/src/task/elf.rs) now refuses to load anything below
 * __kernel_end instead of silently corrupting it -- but you still need to
 * link high enough to have anywhere valid to load into.
 */

typedef unsigned long u64;

static u64 hamix_syscall3(u64 num, u64 a1, u64 a2, u64 a3) {
    u64 ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(num), "D"(a1), "S"(a2), "d"(a3)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void hamix_write(int fd, const char *buf, u64 len) {
    hamix_syscall3(1, (u64)fd, (u64)buf, len);
}

static void hamix_exit(int code) {
    hamix_syscall3(60, (u64)code, 0, 0);
    __builtin_unreachable();
}

void _start(void) {
    const char msg[] = "hello from userspace!\n";
    hamix_write(1, msg, sizeof(msg) - 1);
    hamix_exit(0);
}
