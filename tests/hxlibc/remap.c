#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

int main(void) {
    size_t size = 64 * 1024;
    unsigned char *p = malloc(size);
    for (size_t i = 0; i < size; i++)
        p[i] = (unsigned char)(i * 7);
    int intact = 1;
    for (int round = 0; round < 8; round++) {
        size_t next = size * 2;
        p = realloc(p, next);
        if (!p) {
            intact = 0;
            break;
        }
        for (size_t i = 0; i < size; i++)
            if (p[i] != (unsigned char)(i * 7))
                intact = 0;
        for (size_t i = size; i < next; i++)
            p[i] = (unsigned char)(i * 7);
        size = next;
    }
    check(intact, "realloc keeps the contents while a block grows to 16 MiB");
    free(p);

    unsigned char *m = mmap(NULL, 3 * 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    memset(m, 0x5a, 3 * 4096);
    unsigned char *r = mremap(m, 3 * 4096, 40 * 4096, MREMAP_MAYMOVE);
    check(r != MAP_FAILED && r[0] == 0x5a && r[3 * 4096 - 1] == 0x5a && r[39 * 4096] == 0, "mremap moves an anonymous mapping");
    munmap(r, 40 * 4096);

    time_t now = time(NULL);
    struct tm tm;
    check(localtime_r(&now, &tm) != NULL, "localtime and the time zone file work");
    char buf[64];
    check(strftime(buf, sizeof(buf), "%Y-%m-%d %H:%M", &tm) > 0, "strftime formats the time");
    printf("remap: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
