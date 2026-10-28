.section .mbr, "ax"
.code16
.global hxboot_mbr
hxboot_mbr:
    cli
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw $0x7c00, %sp
    cld
    movw $0x7c00, %si
    movw $0x0600, %di
    movw $256, %cx
    rep movsw
    ljmp $0, $mbr_go
mbr_go:
    sti
    movb %dl, mbr_drive
    movb $0x41, %ah
    movw $0x55aa, %bx
    int $0x13
    jc mbr_fail
    cmpw $0xaa55, %bx
    jne mbr_fail
    movb $3, mbr_tries
mbr_read:
    movw $mbr_dap, %si
    movb mbr_drive, %dl
    movb $0x42, %ah
    int $0x13
    jnc mbr_boot
    xorw %ax, %ax
    movb mbr_drive, %dl
    int $0x13
    decb mbr_tries
    jnz mbr_read
    jmp mbr_fail
mbr_boot:
    movb mbr_drive, %dl
    ljmp $0, $0x7c40
mbr_fail:
    movw $mbr_msg, %si
1:
    lodsb
    testb %al, %al
    jz 2f
    movb $0x0e, %ah
    movw $7, %bx
    int $0x10
    jmp 1b
2:
    hlt
    jmp 2b
mbr_drive:
    .byte 0
mbr_tries:
    .byte 0
mbr_msg:
    .asciz "hxboot: cannot read the USB stick or disk"
    .org hxboot_mbr + 0x1a0
mbr_dap:
    .byte 16, 0
    .word 4
    .word 0x7c00, 0
    .quad 0
    .org hxboot_mbr + 0x1b8
