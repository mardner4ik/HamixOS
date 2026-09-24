ENTRY(_start)

SECTIONS {
    . = 0x40200000;
    __kernel_start = .;

    .text : {
        KEEP(*(.text.boot))
        . = ALIGN(4096);
        __efi_text_start = .;
        *(.text .text.*)
    }

    .rodata ALIGN(4096) : {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4096) : {
        *(.data .data.*)
        *(.got .got.*)
    }

    . = ALIGN(4096);
    __efi_file_end = .;

    .bss ALIGN(4096) (NOLOAD) : {
        __bss_start = .;
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
    __kernel_image_size = __kernel_end - __kernel_start;
    __efi_raw_size = __efi_file_end - __efi_text_start;
    __efi_virtual_size = __kernel_end - __efi_text_start;
    __efi_entry_rva = hamix_efi_entry - __kernel_start;
    __efi_file_size = __efi_file_end - __kernel_start;
}
