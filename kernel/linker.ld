ENTRY(_start)

SECTIONS {
    . = 1M;

    .boot ALIGN(8) : {
        KEEP(*(.multiboot_header))
    }

    . = ALIGN(4096);
    __text_start = .;

    .bootstrap ALIGN(4096) : {
        *(.bootstrap .bootstrap.*)
    }

    .text ALIGN(4096) : {
        *(.text .text.*)
    }

    . = ALIGN(4096);
    __text_end = .;

    .rodata ALIGN(4096) : {
        *(.rodata .rodata.*)
    }

    . = ALIGN(4096);
    __data_start = .;

    .data ALIGN(4096) : {
        *(.data .data.*)
        *(.got .got.*)
    }

    .bss ALIGN(4096) (NOLOAD) : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4096);
        __bss_end = .;
    }

    .bootstrap_stack ALIGN(4096) (NOLOAD) : {
        *(.bootstrap_stack .bootstrap_stack.*)
    }

    __kernel_end = .;
}
