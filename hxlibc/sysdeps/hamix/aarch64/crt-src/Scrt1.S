.section .text
.global _start
_start:
	mov x0, sp
	adr x1, main

	bl __mlibc_entry
	brk #0

.section .note.GNU-stack,"",%progbits


.section .note.hamix,"a",%note
.balign 4
.long 6
.long 4
.long 0x48584f53
.asciz "Hamix"
.balign 4
.long 0x0100
