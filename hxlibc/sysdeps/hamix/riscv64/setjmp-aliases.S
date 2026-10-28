.section .text
.global _setjmp
.type _setjmp, "function"
_setjmp:
	tail setjmp
.global _longjmp
.type _longjmp, "function"
_longjmp:
	tail longjmp
.section .note.GNU-stack,"",%progbits
