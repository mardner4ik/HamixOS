#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static double now_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1e6 + ts.tv_nsec / 1e3;
}

static void report(const char *name, double total_us, long iterations, const char *unit) {
    printf("{\"test\":\"%s\",\"iterations\":%ld,\"us_per_op\":%.3f,\"unit\":\"%s\"}\n", name, iterations, total_us / iterations, unit);
    fflush(stdout);
}

static void bench_getpid(long n) {
    double t = now_us();
    for (long i = 0; i < n; i++)
        syscall(SYS_getpid);
    report("getpid", now_us() - t, n, "syscall");
}

static void bench_clock(long n) {
    struct timespec ts;
    double t = now_us();
    for (long i = 0; i < n; i++)
        clock_gettime(CLOCK_MONOTONIC, &ts);
    report("clock_gettime", now_us() - t, n, "call");
}

static void bench_pipe(long n) {
    int a[2], b[2];
    pipe(a);
    pipe(b);
    char c = 'x';
    pid_t child = fork();
    if (child == 0) {
        for (long i = 0; i < n; i++) {
            if (read(a[0], &c, 1) != 1)
                _exit(1);
            write(b[1], &c, 1);
        }
        _exit(0);
    }
    double t = now_us();
    for (long i = 0; i < n; i++) {
        write(a[1], &c, 1);
        read(b[0], &c, 1);
    }
    double total = now_us() - t;
    waitpid(child, NULL, 0);
    report("pipe_pingpong", total, n, "round trip");
}

static void bench_fork_exec(long n, const char *self) {
    double t = now_us();
    for (long i = 0; i < n; i++) {
        pid_t child = fork();
        if (child == 0) {
            execl(self, self, "--exit", (char *)NULL);
            _exit(127);
        }
        waitpid(child, NULL, 0);
    }
    report("fork_exec_wait", now_us() - t, n, "process");
}

static void bench_fork(long n) {
    double t = now_us();
    for (long i = 0; i < n; i++) {
        pid_t child = fork();
        if (child == 0)
            _exit(0);
        waitpid(child, NULL, 0);
    }
    report("fork_wait", now_us() - t, n, "process");
}

static void bench_mmap_fault(long pages) {
    size_t len = pages * 4096;
    double t = now_us();
    char *p = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (p == MAP_FAILED) {
        printf("{\"test\":\"mmap_fault\",\"error\":%d}\n", errno);
        return;
    }
    for (long i = 0; i < pages; i++)
        p[i * 4096] = 1;
    munmap(p, len);
    report("mmap_page_fault", now_us() - t, pages, "page");
}

static void bench_open_stat_close(long n) {
    const char *path = "/tmp/kbench.file";
    int fd = open(path, O_CREAT | O_WRONLY | O_TRUNC, 0644);
    close(fd);
    struct stat st;
    double t = now_us();
    for (long i = 0; i < n; i++) {
        fd = open(path, O_RDONLY);
        fstat(fd, &st);
        close(fd);
    }
    report("open_stat_close", now_us() - t, n, "open");
    unlink(path);
}

static void bench_create_files(long n) {
    mkdir("/tmp/kbench.dir", 0755);
    char name[64];
    double t = now_us();
    for (long i = 0; i < n; i++) {
        snprintf(name, sizeof name, "/tmp/kbench.dir/f%ld", i);
        int fd = open(name, O_CREAT | O_WRONLY, 0644);
        close(fd);
    }
    report("create_file", now_us() - t, n, "file");
    for (long i = 0; i < n; i++) {
        snprintf(name, sizeof name, "/tmp/kbench.dir/f%ld", i);
        unlink(name);
    }
    rmdir("/tmp/kbench.dir");
}

static void bench_read(long mib) {
    const char *path = "/tmp/kbench.read";
    char *buf = malloc(1 << 20);
    memset(buf, 7, 1 << 20);
    int fd = open(path, O_CREAT | O_WRONLY | O_TRUNC, 0644);
    for (long i = 0; i < mib; i++)
        write(fd, buf, 1 << 20);
    close(fd);
    double t = now_us();
    fd = open(path, O_RDONLY);
    long total = 0;
    ssize_t r;
    while ((r = read(fd, buf, 1 << 16)) > 0)
        total += r;
    close(fd);
    double us = now_us() - t;
    printf("{\"test\":\"read_cached\",\"bytes\":%ld,\"mib_per_s\":%.1f}\n", total, total / us);
    unlink(path);
    free(buf);
}

static void *thread_body(void *arg) {
    return arg;
}

static void bench_threads(long n) {
    double t = now_us();
    for (long i = 0; i < n; i++) {
        pthread_t th;
        pthread_create(&th, NULL, thread_body, NULL);
        pthread_join(th, NULL);
    }
    report("thread_create_join", now_us() - t, n, "thread");
}

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static int turn;
static long rounds;

static void *pong(void *arg) {
    for (long i = 0; i < rounds; i++) {
        pthread_mutex_lock(&mutex);
        while (turn != 1)
            pthread_cond_wait(&cond, &mutex);
        turn = 0;
        pthread_cond_signal(&cond);
        pthread_mutex_unlock(&mutex);
    }
    return arg;
}

static void bench_futex(long n) {
    rounds = n;
    turn = 0;
    pthread_t th;
    pthread_create(&th, NULL, pong, NULL);
    double t = now_us();
    for (long i = 0; i < n; i++) {
        pthread_mutex_lock(&mutex);
        turn = 1;
        pthread_cond_signal(&cond);
        while (turn != 0)
            pthread_cond_wait(&cond, &mutex);
        pthread_mutex_unlock(&mutex);
    }
    double total = now_us() - t;
    pthread_join(th, NULL);
    report("futex_condvar_pingpong", total, n, "round trip");
}

static int want(const char *name) {
    const char *only = getenv("KBENCH_ONLY");
    return !only || strstr(only, name) != NULL;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--exit") == 0)
        return 0;
    long scale = argc > 1 ? atol(argv[1]) : 1;
    if (scale < 1)
        scale = 1;
    printf("{\"kbench\":1,\"libc\":\"%s\"}\n",
#ifdef __hamix__
           "hxlibc"
#else
           "other"
#endif
    );
    if (want("getpid"))
        bench_getpid(100000 * scale);
    if (want("clock"))
        bench_clock(100000 * scale);
    if (want("pipe"))
        bench_pipe(5000 * scale);
    if (want("fork_wait"))
        bench_fork(200 * scale);
    if (want("fork_exec"))
        bench_fork_exec(100 * scale, argv[0]);
    if (want("mmap"))
        bench_mmap_fault(4096 * scale);
    if (want("open"))
        bench_open_stat_close(10000 * scale);
    if (want("create"))
        bench_create_files(1000 * scale);
    if (want("read"))
        bench_read(16 * scale);
    if (want("thread"))
        bench_threads(500 * scale);
    if (want("futex"))
        bench_futex(5000 * scale);
    printf("{\"done\":true}\n");
    return 0;
}
