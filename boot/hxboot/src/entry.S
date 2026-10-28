.section .boot.entry, "awx"
.code16
.global _start16
_start16:
    jmp cd_entry
    .org _start16 + 8
.global boot_info
boot_info:
bi_pvd:
    .long 0
bi_file:
    .long 0
bi_length:
    .long 0
bi_csum:
    .long 0
    .fill 40, 1, 0

    .org _start16 + 0x40
hybrid_entry:
    cli
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movl $0x7c00, %esp
    movb $9, sector_shift
    jmp common

cd_entry:
    cli
    ljmp $0, $cd_normal
cd_normal:
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movl $0x7c00, %esp
    movb $11, sector_shift

common:
    movb %dl, boot_drive
    sti
    cld
    movl bi_file, %eax
    cmpb $9, sector_shift
    jne 1f
    shll $2, %eax
1:
    movl %eax, file_lba
    movzbl sector_shift, %ecx
    movl $2048, %ebx
    shrl %cl, %ebx
    addl %ebx, %eax
    movl %eax, dap_lba
    movl $0, dap_lba + 4
    movl $__load_end, %esi
    subl $0x8400, %esi
    jbe loaded
    movl $1, %edx
    shll %cl, %edx
    decl %edx
    addl %edx, %esi
    shrl %cl, %esi
    movw $0, dap_off
    movw $0x0840, dap_seg
read_loop:
    movl $32768, %ebx
    shrl %cl, %ebx
    cmpl %ebx, %esi
    jae 2f
    movl %esi, %ebx
2:
    movw %bx, dap_count
    movb $3, retries
retry:
    pushl %esi
    pushl %ebx
    pushl %ecx
    movw $dap, %si
    movb boot_drive, %dl
    movb $0x42, %ah
    int $0x13
    popl %ecx
    popl %ebx
    popl %esi
    jnc 3f
    decb retries
    jz disk_error
    pushl %ecx
    xorw %ax, %ax
    movb boot_drive, %dl
    int $0x13
    popl %ecx
    jmp retry
3:
    addl %ebx, dap_lba
    movl %ebx, %eax
    shll %cl, %eax
    shrl $4, %eax
    addw %ax, dap_seg
    subl %ebx, %esi
    jnz read_loop

loaded:
    call enable_a20
    cli
    lgdtl gdt_desc
    movl %cr0, %eax
    orb $1, %al
    movl %eax, %cr0
    ljmpl $0x08, $pm_entry

enable_a20:
    movw $0x2401, %ax
    int $0x15
    inb $0x92, %al
    testb $2, %al
    jnz 1f
    orb $2, %al
    andb $0xfe, %al
    outb %al, $0x92
1:
    ret

disk_error:
    movw $msg_disk, %si
print_hang:
    lodsb
    testb %al, %al
    jz hang
    movb $0x0e, %ah
    movw $7, %bx
    int $0x10
    jmp print_hang
hang:
    hlt
    jmp hang

msg_disk:
    .asciz "hxboot: cannot read the boot medium"

.global boot_drive
boot_drive:
    .byte 0
sector_shift:
    .byte 0
retries:
    .byte 0
    .p2align 2
file_lba:
    .long 0

    .p2align 4
dap:
    .byte 16, 0
dap_count:
    .word 0
dap_off:
    .word 0
dap_seg:
    .word 0
dap_lba:
    .quad 0

    .p2align 3
gdt:
    .quad 0
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00009a000000ffff
    .quad 0x000092000000ffff
gdt_end:
gdt_desc:
    .word gdt_end - gdt - 1
    .long gdt
rm_idt:
    .word 0x3ff
    .long 0

    .p2align 2
rm_regs:
    .fill 10, 4, 0
regs_ptr:
    .long 0
saved_esp:
    .long 0

.code32
pm_entry:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl $0x7fff0, %esp
    cld
    movl $__bss_start, %edi
    movl $__bss_end, %ecx
    subl %edi, %ecx
    xorl %eax, %eax
    rep stosb
    movzbl sector_shift, %eax
    pushl %eax
    pushl file_lba
    movzbl boot_drive, %eax
    pushl %eax
    call hxboot_main
1:
    hlt
    jmp 1b

.global bios_int
bios_int:
    pushal
    movl 36(%esp), %eax
    movb %al, int_no
    movl 40(%esp), %esi
    movl %esi, regs_ptr
    movl $rm_regs, %edi
    movl $10, %ecx
    cld
    rep movsl
    movl %esp, saved_esp
    ljmpl $0x18, $thunk16

.code16
thunk16:
    movw $0x20, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl %cr0, %eax
    andb $0xfe, %al
    movl %eax, %cr0
    ljmp $0, $thunk_rm
thunk_rm:
    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl $0x7bf0, %esp
    lidtl rm_idt
    movw rm_regs + 32, %ax
    movw %ax, %es
    movl rm_regs + 4, %ebx
    movl rm_regs + 8, %ecx
    movl rm_regs + 12, %edx
    movl rm_regs + 16, %esi
    movl rm_regs + 20, %edi
    movl rm_regs + 24, %ebp
    movl rm_regs + 0, %eax
    pushw rm_regs + 28
    popw %ds
    sti
    .byte 0xcd
int_no:
    .byte 0
    cli
    pushfl
    pushw %ds
    pushl %eax
    xorw %ax, %ax
    movw %ax, %ds
    popl %eax
    movl %eax, rm_regs + 0
    popw %ax
    movzwl %ax, %eax
    movl %eax, rm_regs + 28
    popl %eax
    movl %eax, rm_regs + 36
    movl %ebx, rm_regs + 4
    movl %ecx, rm_regs + 8
    movl %edx, rm_regs + 12
    movl %esi, rm_regs + 16
    movl %edi, rm_regs + 20
    movl %ebp, rm_regs + 24
    movw %es, %ax
    movzwl %ax, %eax
    movl %eax, rm_regs + 32
    lgdtl gdt_desc
    movl %cr0, %eax
    orb $1, %al
    movl %eax, %cr0
    ljmpl $0x08, $thunk_back

.code32
thunk_back:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movl saved_esp, %esp
    movl regs_ptr, %edi
    movl $rm_regs, %esi
    movl $10, %ecx
    cld
    rep movsl
    popal
    ret

.global hxboot_enter
hxboot_enter:
    cli
    movl 4(%esp), %ecx
    movl 8(%esp), %ebx
    movl $0x36d76289, %eax
    jmp *%ecx

.global port_out8
port_out8:
    movl 4(%esp), %edx
    movl 8(%esp), %eax
    outb %al, %dx
    ret

.global port_in8
port_in8:
    movl 4(%esp), %edx
    xorl %eax, %eax
    inb %dx, %al
    ret
