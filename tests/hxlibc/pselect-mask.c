#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/select.h>
#include <unistd.h>

int main(void)
{
    int fds[2];
    pipe(fds);
    write(fds[1], "x", 1);
    sigset_t mask;
    sigemptyset(&mask);
    sigaddset(&mask, SIGCHLD);
    sigaddset(&mask, SIGWINCH);
    sigaddset(&mask, SIGINT);
    fd_set readable;
    FD_ZERO(&readable);
    FD_SET(fds[0], &readable);
    struct timespec timeout = { 1, 0 };
    int n = pselect(fds[0] + 1, &readable, NULL, NULL, &timeout, &mask);
    printf("%s pselect with a signal mask (%d, %s)\n", n == 1 && FD_ISSET(fds[0], &readable) ? "PASS" : "FAIL", n, n < 0 ? strerror(errno) : "ok");
    return n == 1 ? 0 : 1;
}
