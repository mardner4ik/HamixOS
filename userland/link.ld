ENTRY(_start)

SECTIONS {
    . = DEFINED(HAMIX_USER_BASE) ? HAMIX_USER_BASE : 0x8000000000;

    .note.hamix ALIGN(4096) (TYPE = SHT_NOTE) : {
        LONG(6)
        LONG(4)
        LONG(0x48584f53)
        BYTE(0x48) BYTE(0x61) BYTE(0x6d) BYTE(0x69) BYTE(0x78) BYTE(0x00) BYTE(0x00) BYTE(0x00)
        LONG(1)
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
        *(.bss .bss.*)
        *(COMMON)
    }
}
