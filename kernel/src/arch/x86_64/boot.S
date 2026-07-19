.global _start
.extern rust_main

.section .data
.p2align 12
p4_table:
    .zero 4096
.p2align 12
p3_table:
    .zero 4096
.p2align 12
p2_table_0:
    .zero 4096
.p2align 12
p2_table_1:
    .zero 4096
.p2align 12
p2_table_2:
    .zero 4096
.p2align 12
p2_table_3:
    .zero 4096

.section .bootstrap_stack, "aw", @nobits
.p2align 4
stack_bottom:
    .zero 262144
stack_top:

.section .bss
.align 16

.section .text
.code32
_start:
    cli
    movl %eax, %edi
    movl %ebx, %esi
    movl $stack_top, %esp
    xorl %ebp, %ebp
    call setup_long_mode
    lgdt gdt64_pointer
    ljmp $0x08, $long_mode_start

setup_long_mode:
    movl $p3_table, %eax
    orl $0x3, %eax
    movl %eax, p4_table
    movl $0x0, p4_table + 4

    movl $p2_table_0, %eax
    orl $0x3, %eax
    movl %eax, p3_table
    movl $0x0, p3_table + 4

    movl $p2_table_1, %eax
    orl $0x3, %eax
    movl %eax, p3_table + 8
    movl $0x0, p3_table + 12

    movl $p2_table_2, %eax
    orl $0x3, %eax
    movl %eax, p3_table + 16
    movl $0x0, p3_table + 20

    movl $p2_table_3, %eax
    orl $0x3, %eax
    movl %eax, p3_table + 24
    movl $0x0, p3_table + 28

    movl $0x0, %ecx
    movl $0x00000083, %eax
1:
    movl %eax, p2_table_0(,%ecx,8)
    movl $0x0, p2_table_0 + 4(,%ecx,8)
    addl $0x00200000, %eax
    incl %ecx
    cmpl $512, %ecx
    jne 1b

    movl $0x0, %ecx
    movl $0x40000083, %eax
1:
    movl %eax, p2_table_1(,%ecx,8)
    movl $0x0, p2_table_1 + 4(,%ecx,8)
    addl $0x00200000, %eax
    incl %ecx
    cmpl $512, %ecx
    jne 1b

    movl $0x0, %ecx
    movl $0x80000083, %eax
1:
    movl %eax, p2_table_2(,%ecx,8)
    movl $0x0, p2_table_2 + 4(,%ecx,8)
    addl $0x00200000, %eax
    incl %ecx
    cmpl $512, %ecx
    jne 1b

    movl $0x0, %ecx
    movl $0xC0000083, %eax
1:
    movl %eax, p2_table_3(,%ecx,8)
    movl $0x0, p2_table_3 + 4(,%ecx,8)
    addl $0x00200000, %eax
    incl %ecx
    cmpl $512, %ecx
    jne 1b

    movl $p4_table, %eax
    movl %eax, %cr3

    movl %cr4, %eax
    orl $0x00000620, %eax
    movl %eax, %cr4

    movl $0xC0000080, %ecx
    rdmsr
    orl $0x00000100, %eax
    wrmsr

    movl %cr0, %eax
    andl $0x9FFFFFFF, %eax
    orl $0x80000022, %eax
    movl %eax, %cr0

    fninit
    ret

.align 16
gdt64:
    .quad 0
    .quad 0x00209A0000000000
    .quad 0x0000920000000000

gdt64_pointer:
    .word 23
    .quad gdt64

.code64
long_mode_start:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %fs
    movw %ax, %gs
    movw %ax, %ss
    movq $stack_top, %rsp
    andq $-16, %rsp
    subq $16, %rsp
    xorq %rbp, %rbp
    movq $0x1F80, %rax
    pushq %rax
    ldmxcsr (%rsp)
    popq %rax
    call rust_main
    hlt
