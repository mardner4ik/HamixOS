OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS {
    . = 0x80200000;
    __kernel_start = .;

    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata ALIGN(4096) : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    .data ALIGN(4096) : {
        *(.data .data.*)
        *(.got .got.*)
        __global_pointer$ = . + 0x800;
        *(.sdata .sdata.*)
    }

    .bss ALIGN(4096) (NOLOAD) : {
        __bss_start = .;
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(8);
        __bss_end = .;
    }

    .stack ALIGN(4096) (NOLOAD) : {
        __stack_bottom = .;
        . += 0x20000;
        __stack_top = .;
    }

    . = ALIGN(4096);
    __kernel_end = .;
}
