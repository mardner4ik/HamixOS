#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s", ok ? "PASS" : "FAIL", what);
    if (!ok) {
        printf(" (errno %d %s)", errno, strerror(errno));
        failures++;
    }
    printf("\n");
}

int main(void) {
    int fd = open("/dev/null", O_RDONLY | O_NOCTTY);
    check(fd >= 0, "open /dev/null read-only");
    if (fd >= 0)
        close(fd);
    fd = open("/dev/null", O_WRONLY);
    check(fd >= 0 && write(fd, "x", 1) == 1, "write /dev/null");
    if (fd >= 0)
        close(fd);
    int p[2];
    check(pipe(p) == 0, "pipe");
    struct stat st;
    check(fstat(p[0], &st) == 0 && S_ISFIFO(st.st_mode), "fstat on a pipe says FIFO");
    errno = 0;
    check(lseek(p[0], 0, SEEK_CUR) == -1 && errno == ESPIPE, "lseek on a pipe is ESPIPE");
    check(fstat(1, &st) == 0 && (S_ISCHR(st.st_mode) || S_ISFIFO(st.st_mode)), "fstat on stdout");
    check(stat("/usr/lib/libhxc.so", &st) == 0 && S_ISREG(st.st_mode), "stat a regular file");
    check(stat("/usr", &st) == 0 && S_ISDIR(st.st_mode), "stat a directory");
    fd = open("/dev/zero", O_RDONLY);
    char buf[4] = {1, 1, 1, 1};
    check(fd >= 0 && read(fd, buf, 4) == 4 && buf[0] == 0, "read /dev/zero");
    fd = open("/dev/urandom", O_RDONLY);
    check(fd >= 0 && read(fd, buf, 4) == 4, "read /dev/urandom");
    printf("posix-io: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
