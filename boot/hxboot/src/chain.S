.section .boot.chain, "awx"
.code32
.global hxboot_chain
hxboot_chain:
    cli
    movl 4(%esp), %eax
    movb %al, chain_drive
    movl 8(%esp), %eax
    movw %ax, chain_si
    ljmpl $0x18, $chain16
.code16
chain16:
    movw $0x20, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl %cr0, %eax
    andb $0xfe, %al
    movl %eax, %cr0
    ljmp $0, $chain_rm
chain_rm:
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl $0x7c00, %esp
    lidtl chain_idt
    movw $0x2f00, %ax
    movw %ax, %ds
    xorw %si, %si
    movw $0x7c00, %di
    movw $256, %cx
    cld
    rep movsw
    xorw %ax, %ax
    movw %ax, %ds
    movb chain_drive, %dl
    movw chain_si, %si
    sti
    ljmp $0, $0x7c00
chain_drive:
    .byte 0
chain_si:
    .word 0
chain_idt:
    .word 0x3ff
    .long 0
