ENTRY(_start)

SECTIONS {
    . = 0x40080000;

    .text ALIGN(4096) : {
        KEEP(*(.text.boot))
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
        __bss_end = .;
    }

    .stack ALIGN(4096) (NOLOAD) : {
        __stack_bottom = .;
        . += 0x10000;
        __stack_top = .;
    }

    __kernel_end = .;
}
