.section .text
.global _setjmp
.type _setjmp, "function"
_setjmp:
	jmp setjmp
.global _longjmp
.type _longjmp, "function"
_longjmp:
	jmp longjmp
.section .note.GNU-stack,"",%progbits
