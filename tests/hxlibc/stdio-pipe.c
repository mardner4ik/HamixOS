#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "readone") == 0) {
        char line[64];
        if (!fgets(line, sizeof line, stdin))
            return 3;
        return strncmp(line, "hi", 2) == 0 ? 0 : 4;
    }
    int p[2];
    pipe(p);
    pid_t child = fork();
    if (child == 0) {
        dup2(p[0], 0);
        close(p[0]);
        close(p[1]);
        char *args[] = {argv[0], "readone", NULL};
        execv(argv[0], args);
        _exit(127);
    }
    close(p[0]);
    write(p[1], "hi\nmore\nlines\n", 14);
    close(p[1]);
    int status = 0;
    waitpid(child, &status, 0);
    printf("%s stdin from a pipe, exit early (status %d)\n", WEXITSTATUS(status) == 0 ? "PASS" : "FAIL", WEXITSTATUS(status));
    child = fork();
    if (child == 0) {
        int fd = open("/dev/null", O_RDONLY);
        _exit(fd >= 0 ? 0 : errno);
    }
    waitpid(child, &status, 0);
    printf("%s open /dev/null in a forked child (%d)\n", WEXITSTATUS(status) == 0 ? "PASS" : "FAIL", WEXITSTATUS(status));
    child = fork();
    if (child == 0) {
        close(0);
        int fd = open("/dev/null", O_RDONLY);
        _exit(fd == 0 ? 0 : (fd < 0 ? errno : 100 + fd));
    }
    waitpid(child, &status, 0);
    printf("%s reopen stdin as /dev/null (%d)\n", WEXITSTATUS(status) == 0 ? "PASS" : "FAIL", WEXITSTATUS(status));
    return 0;
}
