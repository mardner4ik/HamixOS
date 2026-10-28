#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/file.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what)
{
    printf("%s %s%s%s\n", ok ? "PASS" : "FAIL", what, ok ? "" : ": ", ok ? "" : strerror(errno));
    if (!ok)
        failures++;
}

int main(void)
{
    const char *lock = "/tmp/probe.lock";
    const char *path = "/tmp/probe.sock";
    unlink(lock);
    unlink(path);
    int fd = open(lock, O_CREAT | O_CLOEXEC | O_RDWR, S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP);
    check(fd >= 0, "open O_CREAT|O_CLOEXEC|O_RDWR");
    check(flock(fd, LOCK_EX | LOCK_NB) == 0, "flock LOCK_EX|LOCK_NB");
    struct stat st;
    errno = 0;
    int r = lstat(path, &st);
    check(r < 0 && errno == ENOENT, "lstat of a missing path fails with ENOENT");
    int server = socket(PF_LOCAL, SOCK_STREAM | SOCK_CLOEXEC, 0);
    check(server >= 0, "socket PF_LOCAL SOCK_STREAM|SOCK_CLOEXEC");
    struct sockaddr_un addr = { .sun_family = AF_LOCAL };
    strcpy(addr.sun_path, path);
    socklen_t size = offsetof(struct sockaddr_un, sun_path) + strlen(path);
    check(bind(server, (struct sockaddr *)&addr, size) == 0, "bind with an unterminated sun_path length");
    check(listen(server, 128) == 0, "listen");
    check(lstat(path, &st) == 0 && S_ISSOCK(st.st_mode), "the bound path is a socket");
    int client = socket(PF_LOCAL, SOCK_STREAM | SOCK_CLOEXEC, 0);
    check(connect(client, (struct sockaddr *)&addr, size) == 0, "connect");
    int accepted = accept4(server, NULL, NULL, SOCK_CLOEXEC);
    check(accepted >= 0, "accept4");
    int pipefd[2];
    check(pipe(pipefd) == 0, "pipe");
    char byte = 'x';
    struct iovec iov = { &byte, 1 };
    char control[CMSG_SPACE(sizeof(int))];
    memset(control, 0, sizeof control);
    struct msghdr msg = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = control, .msg_controllen = sizeof control };
    struct cmsghdr *cmsg = CMSG_FIRSTHDR(&msg);
    cmsg->cmsg_level = SOL_SOCKET;
    cmsg->cmsg_type = SCM_RIGHTS;
    cmsg->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(cmsg), &pipefd[1], sizeof(int));
    check(sendmsg(client, &msg, 0) == 1, "sendmsg with SCM_RIGHTS");
    char got = 0;
    char in_control[CMSG_SPACE(sizeof(int))];
    struct iovec in_iov = { &got, 1 };
    struct msghdr in = { .msg_iov = &in_iov, .msg_iovlen = 1, .msg_control = in_control, .msg_controllen = sizeof in_control };
    int passed = -1;
    if (recvmsg(accepted, &in, MSG_CMSG_CLOEXEC) == 1) {
        struct cmsghdr *c = CMSG_FIRSTHDR(&in);
        if (c && c->cmsg_type == SCM_RIGHTS)
            memcpy(&passed, CMSG_DATA(c), sizeof(int));
    }
    check(got == 'x' && passed >= 0, "recvmsg receives the byte and a descriptor");
    if (passed >= 0) {
        check(write(passed, "ok", 2) == 2, "write through the passed descriptor");
        char buf[3] = { 0 };
        check(read(pipefd[0], buf, 2) == 2 && strcmp(buf, "ok") == 0, "the pipe sees data written through it");
    }
    struct ucred cred;
    socklen_t len = sizeof cred;
    check(getsockopt(accepted, SOL_SOCKET, SO_PEERCRED, &cred, &len) == 0 && cred.pid == getpid(), "SO_PEERCRED");
    unlink(path);
    unlink(lock);
    printf("posix-sockets: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
