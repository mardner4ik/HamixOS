#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what, const char *device) {
    printf("%s %s %s\n", ok ? "PASS" : "FAIL", device, what);
    if (!ok)
        failures++;
}

static int has(const unsigned long *bits, int bit) {
    return (bits[bit / (8 * sizeof(long))] >> (bit % (8 * sizeof(long)))) & 1;
}

int main(void) {
    for (int n = 0; n < 3; n++) {
        char path[64];
        snprintf(path, sizeof path, "/dev/input/event%d", n);
        int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        check(fd >= 0, "opens", path);
        if (fd < 0)
            continue;
        struct stat st;
        check(fstat(fd, &st) == 0 && S_ISCHR(st.st_mode) && major(st.st_rdev) == 13 && minor(st.st_rdev) == 64 + (unsigned)n, "is char 13:64+n", path);
        int version = 0;
        check(ioctl(fd, EVIOCGVERSION, &version) == 0 && version == EV_VERSION, "EVIOCGVERSION", path);
        struct input_id id;
        check(ioctl(fd, EVIOCGID, &id) == 0 && id.bustype != 0, "EVIOCGID", path);
        char name[128] = {0};
        int len = ioctl(fd, EVIOCGNAME(sizeof name), name);
        check(len > 1 && strlen(name) > 0, "EVIOCGNAME", path);
        printf("  name: %s\n", name);
        unsigned long ev[1] = {0}, keys[KEY_CNT / (8 * sizeof(long))] = {0};
        check(ioctl(fd, EVIOCGBIT(0, sizeof ev), ev) > 0 && has(ev, EV_SYN), "EVIOCGBIT(0)", path);
        check(ioctl(fd, EVIOCGBIT(EV_KEY, sizeof keys), keys) > 0, "EVIOCGBIT(EV_KEY)", path);
        if (n == 0)
            check(has(keys, KEY_ENTER) && has(keys, KEY_LEFTMETA), "keyboard has KEY_ENTER", path);
        else
            check(has(keys, BTN_LEFT), "pointer has BTN_LEFT", path);
        if (n == 2) {
            struct input_absinfo abs;
            check(ioctl(fd, EVIOCGABS(ABS_X), &abs) == 0 && abs.maximum > 0, "EVIOCGABS(ABS_X)", path);
        }
        int clock = CLOCK_MONOTONIC;
        check(ioctl(fd, EVIOCSCLOCKID, &clock) == 0, "EVIOCSCLOCKID", path);
        check(ioctl(fd, EVIOCGRAB, (void *)1) == 0 && ioctl(fd, EVIOCGRAB, (void *)0) == 0, "EVIOCGRAB", path);
        struct input_event e;
        errno = 0;
        check(read(fd, &e, sizeof e) < 0 && errno == EAGAIN, "empty read is EAGAIN", path);
        close(fd);
    }
    check(access("/sys/dev/char/13:64/uevent", R_OK) == 0, "sysfs uevent", "/sys/dev/char/13:64");
    char link[256];
    ssize_t n = readlink("/sys/class/input/event0/subsystem", link, sizeof link - 1);
    check(n > 0, "subsystem link", "/sys/class/input/event0");
    char real[256] = {0};
    errno = 0;
    char *resolved = realpath("/sys/dev/char/13:64", real);
    printf("  realpath: %s errno %d\n", resolved ? real : "(null)", errno);
    check(resolved != NULL && strstr(real, "/sys/devices/virtual/input/input0/event0") != NULL, "realpath", "/sys/dev/char/13:64");
    struct stat ls;
    check(lstat("/sys/dev/char/13:64", &ls) == 0 && S_ISLNK(ls.st_mode), "lstat sees a link", "/sys/dev/char/13:64");
    check(lstat("/sys/dev/char", &ls) == 0 && S_ISDIR(ls.st_mode), "lstat sees a directory", "/sys/dev/char");
    return failures ? 1 : 0;
}
