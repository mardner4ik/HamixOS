.section .text
.global _start
_start:
	mov %rsp, %rdi
	lea main(%rip), %rsi
	call __mlibc_entry
.section .note.GNU-stack,"",%progbits


.section .note.hamix,"a",%note
.balign 4
.long 6
.long 4
.long 0x48584f53
.asciz "Hamix"
.balign 4
.long 0x0100
