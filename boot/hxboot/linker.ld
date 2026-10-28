ENTRY(_start16)

SECTIONS
{
    . = 0x7c00;
    .boot : { KEEP(*(.boot.entry)) KEEP(*(.boot.chain)) }
    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) *(.got .got.plt) }
    . = ALIGN(16);
    __load_end = .;
    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4);
        __bss_end = .;
    }
    .mbr 0x600 : AT(0x200000) { KEEP(*(.mbr)) }
    /DISCARD/ : { *(.eh_frame*) *(.comment*) *(.note*) }
}

ASSERT(__bss_end <= 0x2f000, "hxboot is too large for the low memory layout")
ASSERT(hxboot_chain >= 0x7e00, "the chain loader stub would overwrite itself")
ASSERT(__load_end > 0x8400, "hxboot image smaller than the El Torito load")
