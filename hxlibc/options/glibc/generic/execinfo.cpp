#include <execinfo.h>
#include <stdlib.h>

int backtrace(void **, int) {
	return 0;
}

char **backtrace_symbols(void *const *, int count) {
	return static_cast<char **>(calloc(count > 0 ? count : 1, sizeof(char *)));
}

void backtrace_symbols_fd(void *const *, int, int) {
}
