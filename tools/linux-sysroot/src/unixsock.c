#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "  ok  " : "  FAIL", what);
    if (!ok) failures++;
}

static int send_fds(int sock, const char *text, const int *fds, int count) {
    struct iovec iov = { .iov_base = (void *)text, .iov_len = strlen(text) };
    char control[CMSG_SPACE(sizeof(int) * 8)];
    struct msghdr msg = { .msg_iov = &iov, .msg_iovlen = 1 };
    if (count > 0) {
        memset(control, 0, sizeof control);
        msg.msg_control = control;
        msg.msg_controllen = CMSG_SPACE(sizeof(int) * count);
        struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
        c->cmsg_level = SOL_SOCKET;
        c->cmsg_type = SCM_RIGHTS;
        c->cmsg_len = CMSG_LEN(sizeof(int) * count);
        memcpy(CMSG_DATA(c), fds, sizeof(int) * count);
    }
    return sendmsg(sock, &msg, 0);
}

static int recv_fds(int sock, char *text, size_t cap, int *fds, int max) {
    struct iovec iov = { .iov_base = text, .iov_len = cap - 1 };
    char control[CMSG_SPACE(sizeof(int) * 8)];
    struct msghdr msg = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = control, .msg_controllen = sizeof control };
    ssize_t n = recvmsg(sock, &msg, 0);
    if (n < 0) return -1;
    text[n] = 0;
    int count = 0;
    for (struct cmsghdr *c = CMSG_FIRSTHDR(&msg); c; c = CMSG_NXTHDR(&msg, c)) {
        if (c->cmsg_level == SOL_SOCKET && c->cmsg_type == SCM_RIGHTS) {
            int k = (c->cmsg_len - CMSG_LEN(0)) / sizeof(int);
            for (int i = 0; i < k && count < max; i++) memcpy(&fds[count++], CMSG_DATA(c) + i * sizeof(int), sizeof(int));
        }
    }
    return count;
}

static int server(const char *path) {
    unlink(path);
    int ls = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    struct sockaddr_un addr = { .sun_family = AF_UNIX };
    strncpy(addr.sun_path, path, sizeof addr.sun_path - 1);
    check(bind(ls, (struct sockaddr *)&addr, sizeof addr) == 0, "server bind");
    struct stat st;
    check(stat(path, &st) == 0 && S_ISSOCK(st.st_mode), "socket file is S_IFSOCK");
    check(listen(ls, 4) == 0, "server listen");
    printf("server listening on %s\n", path);
    fflush(stdout);
    int c = accept4(ls, NULL, NULL, SOCK_CLOEXEC);
    check(c >= 0, "server accept");
    struct ucred cred;
    socklen_t len = sizeof cred;
    check(getsockopt(c, SOL_SOCKET, SO_PEERCRED, &cred, &len) == 0 && cred.pid > 0, "SO_PEERCRED");
    printf("peer pid=%d uid=%d\n", cred.pid, cred.uid);
    char text[64];
    int fds[8];
    int got = recv_fds(c, text, sizeof text, fds, 8);
    check(got == 2 && strcmp(text, "ping") == 0, "server received ping with 2 fds");
    char buf[64] = { 0 };
    check(pread(fds[0], buf, sizeof buf - 1, 0) > 0 && strcmp(buf, "hello from client") == 0, "server reads memfd contents");
    char *map = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fds[0], 0);
    check(map != MAP_FAILED, "server mmaps passed memfd");
    if (map != MAP_FAILED) strcpy(map, "server was here");
    check(write(fds[1], "via pipe", 8) == 8, "server writes into passed pipe");
    close(fds[1]);
    check(send_fds(c, "pong", &fds[0], 1) == 4, "server sends fd back");
    int ep = epoll_create1(EPOLL_CLOEXEC);
    struct epoll_event ev = { .events = EPOLLIN | EPOLLRDHUP, .data.u64 = 42 };
    check(epoll_ctl(ep, EPOLL_CTL_ADD, c, &ev) == 0, "epoll_ctl add");
    struct epoll_event out;
    int n = epoll_wait(ep, &out, 1, 5000);
    check(n == 1 && out.data.u64 == 42, "epoll_wait sees client hang-up");
    check(read(c, buf, sizeof buf) == 0, "server reads EOF");
    close(c);
    close(ls);
    unlink(path);
    printf(failures ? "SERVER FAILED (%d)\n" : "SERVER OK\n", failures);
    return failures != 0;
}

static int client(const char *path) {
    int mfd = memfd_create("buffer", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    check(mfd >= 0, "memfd_create");
    check(ftruncate(mfd, 4096) == 0, "ftruncate memfd");
    char *map = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, mfd, 0);
    check(map != MAP_FAILED, "client mmaps memfd");
    strcpy(map, "hello from client");
    int p[2];
    check(pipe(p) == 0, "pipe");
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    struct sockaddr_un addr = { .sun_family = AF_UNIX };
    strncpy(addr.sun_path, path, sizeof addr.sun_path - 1);
    int rc = -1;
    for (int i = 0; i < 50 && rc != 0; i++) {
        rc = connect(s, (struct sockaddr *)&addr, sizeof addr);
        if (rc != 0) usleep(100000);
    }
    check(rc == 0, "client connect");
    int fds[2] = { mfd, p[1] };
    check(send_fds(s, "ping", fds, 2) == 4, "client sends ping with memfd and pipe");
    close(p[1]);
    char text[64];
    int back[8];
    int got = recv_fds(s, text, sizeof text, back, 8);
    check(got == 1 && strcmp(text, "pong") == 0, "client received pong with 1 fd");
    check(strcmp(map, "server was here") == 0, "shared mapping shows server write");
    char buf[32] = { 0 };
    check(read(p[0], buf, sizeof buf) == 8 && memcmp(buf, "via pipe", 8) == 0, "client reads from pipe written by server");
    check(read(p[0], buf, sizeof buf) == 0, "pipe EOF after server closed it");
    struct stat a, b;
    check(got == 1 && fstat(mfd, &a) == 0 && fstat(back[0], &b) == 0 && a.st_ino == b.st_ino && back[0] != mfd, "round-tripped fd is the same memfd");
    char *again = got == 1 ? mmap(NULL, 4096, PROT_READ, MAP_SHARED, back[0], 0) : MAP_FAILED;
    check(again != MAP_FAILED && strcmp(again, "server was here") == 0, "second mapping of returned fd");
    close(s);
    printf(failures ? "CLIENT FAILED (%d)\n" : "CLIENT OK\n", failures);
    return failures != 0;
}

static int selftest(void) {
    int sv[2];
    check(socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0, "socketpair stream");
    check(write(sv[0], "abc", 3) == 3 && write(sv[0], "def", 3) == 3, "write both halves");
    char buf[16] = { 0 };
    check(recv(sv[1], buf, sizeof buf, MSG_PEEK) == 6 && read(sv[1], buf, sizeof buf) == 6 && memcmp(buf, "abcdef", 6) == 0, "stream coalesces and peeks");
    check(fcntl(sv[1], F_SETFL, O_NONBLOCK) == 0 && read(sv[1], buf, 1) == -1 && errno == EAGAIN, "nonblocking read gives EAGAIN");
    struct pollfd pfd = { .fd = sv[1], .events = POLLIN };
    check(poll(&pfd, 1, 0) == 0, "poll: nothing to read");
    write(sv[0], "x", 1);
    check(poll(&pfd, 1, 1000) == 1 && (pfd.revents & POLLIN), "poll: readable");
    read(sv[1], buf, 1);
    shutdown(sv[0], SHUT_WR);
    check(read(sv[1], buf, 1) == 0, "shutdown gives EOF");
    close(sv[0]);
    check(send(sv[1], "y", 1, MSG_NOSIGNAL) == -1 && errno == EPIPE, "EPIPE after peer close");
    close(sv[1]);

    int dg[2];
    check(socketpair(AF_UNIX, SOCK_DGRAM, 0, dg) == 0, "socketpair dgram");
    send(dg[0], "one", 3, 0);
    send(dg[0], "second", 6, 0);
    check(recv(dg[1], buf, 2, MSG_TRUNC) == 3, "datagram boundaries kept (MSG_TRUNC)");
    check(recv(dg[1], buf, sizeof buf, 0) == 6, "second datagram intact");
    close(dg[0]);
    close(dg[1]);

    int a = socket(AF_UNIX, SOCK_DGRAM, 0), b = socket(AF_UNIX, SOCK_DGRAM, 0);
    struct sockaddr_un an = { .sun_family = AF_UNIX }, bn = { .sun_family = AF_UNIX };
    strcpy(an.sun_path + 1, "hamix-a");
    strcpy(bn.sun_path + 1, "hamix-b");
    socklen_t al = offsetof(struct sockaddr_un, sun_path) + 8, bl = al;
    check(bind(a, (struct sockaddr *)&an, al) == 0 && bind(b, (struct sockaddr *)&bn, bl) == 0, "abstract bind");
    check(sendto(a, "hi", 2, 0, (struct sockaddr *)&bn, bl) == 2, "sendto abstract");
    struct sockaddr_un from;
    socklen_t fl = sizeof from;
    check(recvfrom(b, buf, sizeof buf, 0, (struct sockaddr *)&from, &fl) == 2 && from.sun_path[0] == 0 && strcmp(from.sun_path + 1, "hamix-a") == 0, "recvfrom reports sender");
    close(a);
    close(b);

    int m = memfd_create("grow", 0);
    check(write(m, "12345", 5) == 5 && lseek(m, 0, SEEK_END) == 5, "memfd write/lseek");
    check(ftruncate(m, 3 * 4096) == 0, "memfd grows");
    struct stat st;
    check(fstat(m, &st) == 0 && st.st_size == 3 * 4096, "memfd size");
    char *p = mmap(NULL, 3 * 4096, PROT_READ | PROT_WRITE, MAP_SHARED, m, 0);
    check(p != MAP_FAILED && memcmp(p, "12345", 5) == 0, "memfd mmap sees data");
    if (p != MAP_FAILED) {
        p[8192] = 'Z';
        check(pread(m, buf, 1, 8192) == 1 && buf[0] == 'Z', "write through mapping visible via pread");
        check(munmap(p, 3 * 4096) == 0, "munmap shared mapping");
    }
    char link[64];
    snprintf(buf, sizeof buf, "/proc/self/fd/%d", m);
    ssize_t ln = readlink(buf, link, sizeof link - 1);
    if (ln > 0) link[ln] = 0;
    check(ln > 0 && strncmp(link, "/memfd:grow", 11) == 0, "/proc/self/fd shows memfd");
    close(m);

    int ep = epoll_create1(0);
    int pp[2];
    pipe(pp);
    struct epoll_event ev = { .events = EPOLLIN, .data.fd = pp[0] };
    epoll_ctl(ep, EPOLL_CTL_ADD, pp[0], &ev);
    struct epoll_event out[4];
    check(epoll_wait(ep, out, 4, 50) == 0, "epoll timeout");
    write(pp[1], "z", 1);
    check(epoll_wait(ep, out, 4, 1000) == 1 && out[0].data.fd == pp[0], "epoll wakes on pipe");
    struct pollfd epp = { .fd = ep, .events = POLLIN };
    check(poll(&epp, 1, 0) == 1, "epoll fd itself is pollable");
    check(epoll_ctl(ep, EPOLL_CTL_DEL, pp[0], NULL) == 0 && epoll_wait(ep, out, 4, 0) == 0, "epoll del");
    close(ep);

    int inet = socket(AF_INET, SOCK_STREAM, 0);
    check(inet >= 0 || errno == EAFNOSUPPORT, "AF_INET either works or is EAFNOSUPPORT");
    if (inet >= 0) close(inet);
    printf(failures ? "SELFTEST FAILED (%d)\n" : "SELFTEST OK\n", failures);
    return failures != 0;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc == 3 && strcmp(argv[1], "server") == 0) return server(argv[2]);
    if (argc == 3 && strcmp(argv[1], "client") == 0) return client(argv[2]);
    if (argc == 2 && strcmp(argv[1], "selftest") == 0) return selftest();
    fprintf(stderr, "usage: %s server PATH | client PATH | selftest\n", argv[0]);
    return 2;
}
