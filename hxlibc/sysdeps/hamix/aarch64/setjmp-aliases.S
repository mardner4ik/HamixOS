.section .text
.global _setjmp
.type _setjmp, "function"
_setjmp:
	b setjmp
.global _longjmp
.type _longjmp, "function"
_longjmp:
	b longjmp
.section .note.GNU-stack,"",%progbits
