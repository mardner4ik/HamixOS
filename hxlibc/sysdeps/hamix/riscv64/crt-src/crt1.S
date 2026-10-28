.section .text
.global _start
_start:
	.weak __global_pointer$
	.hidden __global_pointer$
	.option push
	.option norelax
	lla gp, __global_pointer$
	.option pop

	mv a0, sp
	la a1, main
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
