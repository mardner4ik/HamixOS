#include <hamix/syscall-table.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define ENOSYS_CODE 38

static long raw(long nr, long a1, long a2, long a3, long a4, long a5, long a6)
{
    long ret;
    register long r10 __asm__("r10") = a4;
    register long r8 __asm__("r8") = a5;
    register long r9 __asm__("r9") = a6;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8), "r"(r9) : "rcx", "r11", "memory");
    return ret;
}

static const char *skipped[] = { "RT_SIGRETURN", "PAUSE", "VFORK", "EXIT", "EXIT_GROUP" };
static const char *may_block[] = { "PPOLL" };

static int in_list(const char **list, unsigned count, const char *name)
{
    for (unsigned i = 0; i < count; i++)
        if (strcmp(list[i], name) == 0)
            return 1;
    return 0;
}

static int is_skipped(const char *name)
{
    for (unsigned i = 0; i < sizeof skipped / sizeof skipped[0]; i++)
        if (strcmp(skipped[i], name) == 0)
            return 1;
    return 0;
}

static void sleep_ms(long ms)
{
    struct timespec ts = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&ts, NULL);
}

int main(void)
{
    int checked = 0, failures = 0, skips = 0;
    for (unsigned i = 0; i < HAMIX_SYSCALL_COUNT; i++) {
        const struct hamix_syscall_entry *e = &hamix_syscalls[i];
        if (!e->core)
            continue;
        if (is_skipped(e->name)) {
            skips++;
            continue;
        }
        int pipefd[2];
        if (pipe(pipefd) != 0)
            return 2;
        pid_t child = fork();
        if (child == 0) {
            close(pipefd[0]);
            long r = raw(e->nr, -1, 0, 0, 0, 0, 0);
            write(pipefd[1], &r, sizeof r);
            _exit(0);
        }
        close(pipefd[1]);
        int status = 0;
        int waited = 0;
        pid_t done = 0;
        while ((done = waitpid(child, &status, WNOHANG)) == 0 && waited < 3000) {
            sleep_ms(10);
            waited += 10;
        }
        long result = 0;
        int got = done == child ? (int)read(pipefd[0], &result, sizeof result) : 0;
        close(pipefd[0]);
        checked++;
        if (done != child) {
            kill(child, SIGKILL);
            waitpid(child, &status, 0);
            if (!in_list(may_block, sizeof may_block / sizeof may_block[0], e->name)) {
                printf("FAIL %s (%ld) blocks with invalid arguments\n", e->name, e->nr);
                failures++;
            }
        } else if (WIFSIGNALED(status) && WTERMSIG(status) != SIGKILL && WTERMSIG(status) != SIGTERM) {
            printf("FAIL %s (%ld) killed the caller with signal %d\n", e->name, e->nr, WTERMSIG(status));
            failures++;
        } else if (got == sizeof result && result == -ENOSYS_CODE) {
            printf("FAIL %s (%ld) is in the ABI table but returns ENOSYS\n", e->name, e->nr);
            failures++;
        }
    }
    printf("abi-core: %d core calls checked, %d skipped, %d failure(s)\n", checked, skips, failures);
    return failures ? 1 : 0;
}
