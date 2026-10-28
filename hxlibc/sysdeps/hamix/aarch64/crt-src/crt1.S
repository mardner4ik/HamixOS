.section .text
.global _start
_start:
	mov x0, sp
	adrp x1, main
	add x1, x1, :lo12:main
	bl __mlibc_entry
.section .note.GNU-stack,"",%progbits


.section .note.hamix,"a",%note
.balign 4
.long 6
.long 4
.long 0x48584f53
.asciz "Hamix"
.balign 4
.long 0x0100
