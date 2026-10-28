#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static long rss(void)
{
    char buf[1024];
    int fd = open("/proc/self/status", O_RDONLY);
    ssize_t n = read(fd, buf, sizeof(buf) - 1);
    close(fd);
    buf[n > 0 ? n : 0] = 0;
    char *p = strstr(buf, "VmRSS:");
    return p ? atol(p + 6) : -1;
}

int main(void)
{
    long start = rss();
    void *small = malloc(64);
    long after_malloc = rss();
    printf("%s hxlibc process starts in %ld kB (after malloc %ld kB)\n", start > 0 && start < 2048 ? "PASS" : "FAIL", start, after_malloc);
    free(small);
    return 0;
}
