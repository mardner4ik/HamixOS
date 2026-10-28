#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

static int counter;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;

static void *worker(void *arg) {
    for (int i = 0; i < 10000; i++) {
        pthread_mutex_lock(&lock);
        counter++;
        pthread_mutex_unlock(&lock);
    }
    return arg;
}

static volatile sig_atomic_t got_signal;

static void on_usr1(int sig) {
    got_signal = sig;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "child") == 0)
        return 42;

    printf("hxlibc spike: pid %d, %s\n", getpid(), argv[0]);

    char *buffer = malloc(1 << 20);
    check(buffer != NULL, "malloc 1 MiB");
    memset(buffer, 'x', 1 << 20);
    buffer = realloc(buffer, 4 << 20);
    check(buffer != NULL && buffer[12345] == 'x', "realloc keeps data");
    free(buffer);

    int fd = open("/tmp/hxlibc-spike.txt", O_CREAT | O_TRUNC | O_WRONLY, 0644);
    check(fd >= 0, "open for writing");
    const char *text = "hello from hxlibc\n";
    check(write(fd, text, strlen(text)) == (ssize_t)strlen(text), "write");
    close(fd);
    FILE *f = fopen("/tmp/hxlibc-spike.txt", "r");
    char line[64] = {0};
    check(f && fgets(line, sizeof line, f) && strcmp(line, text) == 0, "stdio read back");
    if (f)
        fclose(f);
    struct stat st;
    check(stat("/tmp/hxlibc-spike.txt", &st) == 0 && st.st_size == (off_t)strlen(text), "stat size");

    pthread_t threads[4];
    for (int i = 0; i < 4; i++)
        pthread_create(&threads[i], NULL, worker, NULL);
    for (int i = 0; i < 4; i++)
        pthread_join(threads[i], NULL);
    check(counter == 40000, "4 pthreads with a mutex");

    pid_t child = fork();
    if (child == 0)
        _exit(7);
    int status = 0;
    check(child > 0 && waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 7, "fork + waitpid");

    child = fork();
    if (child == 0) {
        char *args[] = {argv[0], "child", NULL};
        execv(argv[0], args);
        _exit(127);
    }
    check(waitpid(child, &status, 0) == child && WEXITSTATUS(status) == 42, "fork + execv");

    signal(SIGUSR1, on_usr1);
    raise(SIGUSR1);
    check(got_signal == SIGUSR1, "signal handler");

    struct timespec a, b;
    clock_gettime(CLOCK_MONOTONIC, &a);
    usleep(20000);
    clock_gettime(CLOCK_MONOTONIC, &b);
    long ms = (b.tv_sec - a.tv_sec) * 1000 + (b.tv_nsec - a.tv_nsec) / 1000000;
    check(ms >= 15, "usleep and clock_gettime");

    char *env = getenv("HOME");
    check(env != NULL, "getenv");

    printf("hxlibc spike: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
