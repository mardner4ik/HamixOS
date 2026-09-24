#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <sys/mman.h>

int main(int argc, char **argv) {
    printf("hello from musl, argc=%d pid=%d\n", argc, getpid());
    for (int i = 0; i < argc; i++) printf("argv[%d]=%s\n", i, argv[i]);
    const char *home = getenv("HOME");
    printf("HOME=%s isatty=%d\n", home ? home : "(null)", isatty(1));
    char *big = malloc(1 << 20);
    memset(big, 'x', 1 << 20);
    printf("malloc ok %p\n", (void *)big);
    free(big);
    char exe[256];
    ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
    if (n > 0) { exe[n] = 0; printf("exe=%s\n", exe); }
    return 7;
}
