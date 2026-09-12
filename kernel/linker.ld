ENTRY(_start)

SECTIONS {
    . = 1M;

    .boot ALIGN(8) : {
        KEEP(*(.multiboot_header))
    }

    .bootstrap ALIGN(4096) : {
        *(.bootstrap .bootstrap.*)
    }

    .bootstrap_stack ALIGN(4096) : {
        *(.bootstrap_stack .bootstrap_stack.*)
    }

    .text ALIGN(4096) : {
        *(.text .text.*)
    }

    .rodata ALIGN(4096) : {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4096) : {
        *(.data .data.*)
    }

    .bss ALIGN(4096) : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        /* USER_HEAP (see kernel/src/syscall/mod.rs) needs to start on a
           fresh 2MB boundary: sys_brk grants ring-3 access to it via
           paging::allow_user_access, which works at 2MB-page granularity.
           Without this alignment, wherever the linker happened to place a
           multi-megabyte array could straddle page 0 (IDT/GDT/low memory)
           or other kernel data, punching *those* open to ring 3 too the
           first time any user process calls brk(). */
        . = ALIGN(0x200000);
        *(.user_heap .user_heap.*)
        __bss_end = .;
    }

    __kernel_end = .;
}
