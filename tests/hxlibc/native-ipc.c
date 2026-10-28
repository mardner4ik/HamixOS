#include <hamix/syscall.h>
#include <poll.h>
#include <signal.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

static long read_pid(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f)
        return -1;
    long pid = -1;
    if (fscanf(f, "%ld", &pid) != 1)
        pid = -1;
    fclose(f);
    return pid > 0 && kill(pid, 0) == 0 ? pid : -1;
}

static int mailbox = -1;

static int send_to(long pid, const void *data, size_t len) {
    struct sockaddr_un to = { .sun_family = AF_UNIX };
    snprintf(to.sun_path, sizeof to.sun_path, "/run/hamix/ipc/%ld", pid);
    return sendto(mailbox, data, len, 0, (struct sockaddr *)&to, sizeof to) < 0 ? -1 : 0;
}

static long lookup(const char *service) {
    char path[128];
    snprintf(path, sizeof path, "/run/hamix/services/%s", service);
    long pid = read_pid(path);
    if (pid > 0)
        return pid;
    snprintf(path, sizeof path, "/run/hamix/activators/%s", service);
    long activator = read_pid(path);
    if (activator <= 0)
        return -2;
    char request[96];
    int n = snprintf(request, sizeof request, "activate %s", service);
    send_to(activator, request, n);
    snprintf(path, sizeof path, "/run/hamix/services/%s", service);
    for (int i = 0; i < 250 && (pid = read_pid(path)) < 0; i++)
        usleep(20000);
    return pid > 0 ? pid : -2;
}

static long auth(const char *user, const char *password) {
    long pid = lookup("hamix.auth");
    if (pid < 0)
        return pid;
    char request[128];
    int n = snprintf(request, sizeof request, "auth%c%s%c%s", 0, user, 0, password);
    if (send_to(pid, request, n) < 0)
        return -3;
    struct pollfd pfd = { .fd = mailbox, .events = POLLIN };
    if (poll(&pfd, 1, 10000) <= 0)
        return -110;
    char reply[32] = {0};
    if (recv(mailbox, reply, sizeof reply - 1, 0) <= 0)
        return -5;
    return atol(reply);
}

static int open_mailbox(void) {
    mailbox = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    int on = 1;
    setsockopt(mailbox, SOL_SOCKET, SO_PASSCRED, &on, sizeof on);
    struct sockaddr_un me = { .sun_family = AF_UNIX };
    snprintf(me.sun_path, sizeof me.sun_path, "/run/hamix/ipc/%d", (int)getpid());
    unlink(me.sun_path);
    if (bind(mailbox, (struct sockaddr *)&me, sizeof me) < 0)
        return -1;
    chmod(me.sun_path, 0777);
    return 0;
}

int main(void) {
    char info[256] = {0};
    long version = syscall(HAMIX_SYS_HAMIX_ABI_INFO, info, sizeof info);
    check(version == HAMIX_ABI_VERSION, "HAMIX_ABI_INFO reports the header's ABI version");
    check(strstr(info, "personality hamix") != NULL, "this program runs with the Hamix personality");
    check(open_mailbox() == 0, "a mailbox socket in /run/hamix/ipc");
    check(auth("root", "hamix") == 0, "hx-authd accepts root's password and grants a ticket");
    check(auth("root", "wrong") == -13, "hx-authd rejects a wrong password");
    check(auth("user", "user") == 1, "hx-authd: valid password of another user, no ticket");
    struct stat st;
    check(stat("/var/lib/hamix/commands", &st) == 0 && (st.st_mode & 01777) == 01777, "command registry directory is 1777");
    printf("native-ipc: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
